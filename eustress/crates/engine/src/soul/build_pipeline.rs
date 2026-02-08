//! # Soul Build Pipeline
//!
//! Complete build pipeline that integrates:
//! 1. Soul markdown parsing → AST
//! 2. Claude API code generation with error feedback
//! 3. Hot compilation to dynamic library
//! 4. Loading into running simulation

use bevy::prelude::*;
use std::collections::VecDeque;
use std::time::Instant;

use eustress_common::soul::{SoulAST, SoulParser, SoulConfig, GeneratedCode, ClaudeConfig};

use super::claude_client::{ClaudeClient, ClaudeError, GenerationResult};
use super::hot_compile::{HotCompiler, CompileResult};
use super::validator::SoulValidator;
use super::error_tracker::RuneErrorTracker;

/// Inject soul::wait(0.001) into loops that don't have a wait call
fn inject_wait_into_loop(code: &str) -> String {
    let mut result = code.to_string();
    
    // Find "loop {" and inject wait before the closing brace
    if let Some(loop_start) = result.find("loop {") {
        let after_loop = &result[loop_start + 6..];
        let mut brace_count = 1;
        let mut end_pos = 0;
        
        for (i, c) in after_loop.chars().enumerate() {
            match c {
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        end_pos = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        
        if end_pos > 0 {
            // Insert wait before the closing brace
            let insert_pos = loop_start + 6 + end_pos;
            result.insert_str(insert_pos, "\n        soul::wait(0.001);\n    ");
        }
    }
    
    result
}

/// Strip markdown code blocks from Claude's response
fn strip_markdown_code_blocks(response: &str) -> String {
    let trimmed = response.trim();
    
    // Check for ```rune or ```rust or ``` code blocks
    if let Some(start) = trimmed.find("```") {
        // Find the end of the opening fence line
        let code_start = if let Some(newline) = trimmed[start..].find('\n') {
            start + newline + 1
        } else {
            return trimmed.to_string();
        };
        
        // Find the closing fence
        if let Some(end) = trimmed[code_start..].find("```") {
            return trimmed[code_start..code_start + end].trim().to_string();
        }
    }
    
    // No code blocks found, return as-is
    trimmed.to_string()
}

/// Extract variable name from MissingLocal error
fn extract_missing_local(error: &str) -> Option<String> {
    // Look for pattern: MissingLocal { name: "variable_name" }
    if let Some(start) = error.find("MissingLocal") {
        if let Some(name_start) = error[start..].find("name: \"") {
            let var_start = start + name_start + 7; // Skip 'name: "'
            if let Some(name_end) = error[var_start..].find('"') {
                return Some(error[var_start..var_start + name_end].to_string());
            }
        }
    }
    None
}

/// Inject a variable declaration at the top of pub fn main()
fn inject_variable_declaration(code: &str, var_name: &str) -> String {
    // Check if already declared at the top level of main (not in a nested scope)
    // We need to be smarter - just check if it's declared right after main's opening brace
    let decl_pattern = format!("let {} = 0;", var_name);
    
    // Find "pub fn main()" or "fn main()"
    let main_pos = code.find("pub fn main()")
        .or_else(|| code.find("fn main()"));
    
    if let Some(main_pos) = main_pos {
        if let Some(brace_pos) = code[main_pos..].find('{') {
            let after_brace = main_pos + brace_pos + 1;
            
            // Check if this specific declaration already exists right after the brace
            let after_brace_content = &code[after_brace..];
            if after_brace_content.trim_start().starts_with(&format!("let {} =", var_name)) {
                // Already declared at the right place, something else is wrong
                return code.to_string();
            }
            
            // Insert the declaration
            let declaration = format!("\n    let {} = 0;", var_name);
            let mut result = code.to_string();
            result.insert_str(after_brace, &declaration);
            return result;
        }
    }
    
    // No main function found - this is likely the issue
    // Wrap the code in a main function
    format!("pub fn main() {{\n    let {} = 0;\n{}\n}}", var_name, code)
}

// ============================================================================
// Build Pipeline State
// ============================================================================

/// Current stage of the build pipeline
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildStage {
    /// Idle, no build in progress
    Idle,
    /// Parsing Soul markdown to AST
    Parsing,
    /// Generating Rust code via Claude API (spawning async task)
    Generating { attempt: u32 },
    /// Waiting for async Claude API response
    GeneratingAsync { attempt: u32 },
    /// Validating generated code
    Validating,
    /// Fixing compilation errors via Claude
    Fixing { iteration: u32, max: u32 },
    /// Compiling to dynamic library (async - started)
    Compiling,
    /// Waiting for async compilation to complete
    CompilingAsync,
    /// Loading into simulation
    Loading,
    /// Build complete
    Complete,
    /// Build failed
    Failed { stage: String, error: String },
}

impl std::fmt::Display for BuildStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildStage::Idle => write!(f, "Idle"),
            BuildStage::Parsing => write!(f, "Parsing Soul script..."),
            BuildStage::Generating { attempt } => write!(f, "Starting generation (attempt {})...", attempt),
            BuildStage::GeneratingAsync { attempt } => write!(f, "Generating code (attempt {})...", attempt),
            BuildStage::Validating => write!(f, "Validating code..."),
            BuildStage::Fixing { iteration, max } => write!(f, "Fixing errors ({}/{})...", iteration, max),
            BuildStage::Compiling => write!(f, "Starting compilation..."),
            BuildStage::CompilingAsync => write!(f, "Compiling (background)..."),
            BuildStage::Loading => write!(f, "Loading into simulation..."),
            BuildStage::Complete => write!(f, "Complete"),
            BuildStage::Failed { stage, error } => write!(f, "Failed at {}: {}", stage, error),
        }
    }
}

/// Scene entity context for Claude
#[derive(Debug, Clone)]
pub struct SceneEntityContext {
    /// Entity name
    pub name: String,
    /// Position in world space
    pub position: (f32, f32, f32),
    /// Entity type/class (Part, Light, Model, etc.)
    pub entity_type: String,
    /// Optional color (for parts)
    pub color: Option<String>,
    /// Optional size (for parts)
    pub size: Option<(f32, f32, f32)>,
}

/// Build request
#[derive(Debug, Clone)]
pub struct BuildRequest {
    /// Entity with SoulScriptData component
    pub entity: Entity,
    /// Soul markdown source
    pub source: String,
    /// Script name
    pub name: String,
    /// Force rebuild even if cached
    pub force: bool,
    /// Scene entities for context with full details
    pub scene_context: Vec<SceneEntityContext>,
}

/// Build result
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Entity that was built
    pub entity: Entity,
    /// Script name
    pub name: String,
    /// Success or failure
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Generated code (if successful)
    pub generated_code: Option<GeneratedCode>,
    /// Compilation result (if compiled)
    pub compile_result: Option<CompileResult>,
    /// Total build duration
    pub duration_ms: u64,
    /// Number of fix iterations used
    pub fix_iterations: u32,
    /// Warnings
    pub warnings: Vec<String>,
}

// ============================================================================
// Build Pipeline Resource
// ============================================================================

/// Main build pipeline resource
#[derive(Resource)]
pub struct SoulBuildPipeline {
    /// Build queue
    queue: VecDeque<BuildRequest>,
    /// Current build in progress
    current: Option<BuildInProgress>,
    /// Completed results waiting to be polled
    results: VecDeque<PipelineResult>,
    /// Parser
    parser: SoulParser,
    /// Claude client
    claude: ClaudeClient,
    /// Validator
    validator: SoulValidator,
    /// Configuration
    config: SoulConfig,
    /// Maximum fix iterations
    max_fix_iterations: u32,
    /// Cache of source hash -> generated code (to skip regeneration)
    code_cache: std::collections::HashMap<String, GeneratedCode>,
    /// Error tracker for systematic improvement
    error_tracker: RuneErrorTracker,
}

/// Thread-safe container for async generation result
type AsyncGenerationResult = std::sync::Arc<std::sync::Mutex<Option<Result<GenerationResult, super::claude_client::ClaudeError>>>>;

/// In-progress build state
struct BuildInProgress {
    request: BuildRequest,
    stage: BuildStage,
    start_time: Instant,
    ast: Option<SoulAST>,
    generated_code: Option<GeneratedCode>,
    generation_result: Option<GenerationResult>,
    fix_iterations: u32,
    warnings: Vec<String>,
    /// Shared result container for async Claude API response
    async_generation_result: Option<AsyncGenerationResult>,
}

impl Default for SoulBuildPipeline {
    fn default() -> Self {
        Self::new(SoulConfig::default())
    }
}

impl SoulBuildPipeline {
    /// Create a new build pipeline
    pub fn new(config: SoulConfig) -> Self {
        let claude_config = config.claude.clone();
        
        let error_tracker = RuneErrorTracker::new();
        
        // Log error tracker summary on startup
        let stats = error_tracker.get_stats().clone();
        if stats.total_errors > 0 {
            info!("📊 Rune Error Tracker: {} total errors tracked", stats.total_errors);
            if !stats.top_missing_functions.is_empty() {
                info!("   Top missing functions: {:?}", stats.top_missing_functions.iter().take(3).collect::<Vec<_>>());
            }
        }
        
        Self {
            queue: VecDeque::new(),
            current: None,
            results: VecDeque::new(),
            parser: SoulParser::new(),
            claude: ClaudeClient::new(claude_config),
            validator: SoulValidator::default(),
            config,
            max_fix_iterations: 10,
            code_cache: std::collections::HashMap::new(),
            error_tracker,
        }
    }
    
    /// Set Claude API key
    pub fn set_api_key(&mut self, key: String) {
        let mut config = self.config.claude.clone();
        // Trim whitespace/newlines from API key to prevent header injection errors
        config.api_key = Some(key.trim().to_string());
        self.claude = ClaudeClient::new(config);
    }
    
    /// Check if API key is configured
    pub fn has_api_key(&self) -> bool {
        self.claude.has_api_key()
    }
    
    /// Get error tracking report
    pub fn get_error_report(&self) -> String {
        self.error_tracker.generate_report()
    }
    
    /// Get error statistics
    pub fn get_error_stats(&self) -> super::error_tracker::ErrorStats {
        self.error_tracker.get_stats().clone()
    }
    
    /// Get functions that need implementation (high frequency missing functions)
    pub fn get_implementation_candidates(&self) -> Vec<String> {
        self.error_tracker.get_implementation_candidates()
    }
    
    /// Print full error report to terminal
    pub fn print_error_report(&self) {
        let report = self.error_tracker.generate_report();
        info!("\n{}", report);
    }
    
    /// Queue a build request
    pub fn queue_build(&mut self, request: BuildRequest) {
        self.queue.push_back(request);
    }
    
    /// Get current build stage
    pub fn current_stage(&self) -> BuildStage {
        self.current.as_ref()
            .map(|b| b.stage.clone())
            .unwrap_or(BuildStage::Idle)
    }
    
    /// Is currently building?
    pub fn is_building(&self) -> bool {
        self.current.is_some()
    }
    
    /// Get queue length
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }
    
    /// Poll for completed result
    pub fn poll_result(&mut self) -> Option<PipelineResult> {
        self.results.pop_front()
    }
    
    /// Peek at the next result without removing it
    pub fn peek_result(&self) -> Option<&PipelineResult> {
        self.results.front()
    }
    
    /// Process one step of the build pipeline
    /// Call this each frame to progress builds
    pub fn tick(&mut self, compiler: &mut HotCompiler) {
        // Start next build if idle
        if self.current.is_none() {
            if let Some(request) = self.queue.pop_front() {
                self.current = Some(BuildInProgress {
                    request,
                    stage: BuildStage::Parsing,
                    start_time: Instant::now(),
                    ast: None,
                    generated_code: None,
                    generation_result: None,
                    fix_iterations: 0,
                    warnings: Vec::new(),
                    async_generation_result: None,
                });
            }
            return;
        }
        
        // Process current build
        let build = self.current.as_mut().unwrap();
        
        match &build.stage {
            BuildStage::Parsing => {
                self.process_parsing();
            }
            BuildStage::Generating { .. } => {
                self.process_generating_start();
            }
            BuildStage::GeneratingAsync { .. } => {
                self.process_generating_poll();
            }
            BuildStage::Validating => {
                self.process_validating();
            }
            BuildStage::Fixing { .. } => {
                // Fixing is handled in process_validating
            }
            BuildStage::Compiling => {
                self.process_compiling_start(compiler);
            }
            BuildStage::CompilingAsync => {
                self.process_compiling_poll(compiler);
            }
            BuildStage::Loading => {
                self.process_loading();
            }
            BuildStage::Complete | BuildStage::Failed { .. } => {
                self.finalize_build();
            }
            BuildStage::Idle => {}
        }
    }
    
    /// Process parsing stage - NOW SKIPS AST AND GOES DIRECTLY TO GENERATION
    /// Soul scripts are free-form markdown that Claude interprets directly
    fn process_parsing(&mut self) {
        let build = self.current.as_mut().unwrap();
        
        // NEW: Skip AST parsing entirely - send raw markdown to Claude
        // Any markdown content is valid - Claude will interpret it
        info!("📝 SoulScript: Skipping AST, sending raw markdown to Claude ({} chars)", 
              build.request.source.len());
        
        // Create a minimal AST just to satisfy the pipeline structure
        // The actual content interpretation happens in Claude
        let mut ast = SoulAST::default();
        ast.scene = build.request.name.clone();
        ast.source_path = build.request.name.clone();
        // Store raw source for Claude to interpret
        ast.raw_markdown = Some(build.request.source.clone());
        
        build.ast = Some(ast);
        build.stage = BuildStage::Generating { attempt: 1 };
    }
    
    /// Start async code generation (spawns background thread)
    fn process_generating_start(&mut self) {
        let build = self.current.as_mut().unwrap();
        let ast = build.ast.as_ref().unwrap();
        
        // Check if we have API key
        if !self.claude.has_api_key() {
            build.stage = BuildStage::Failed {
                stage: "Generation".to_string(),
                error: "No Claude API key configured. Set ANTHROPIC_API_KEY environment variable or configure in Soul Settings.".to_string(),
            };
            return;
        }
        
        // Create shared result container for async result
        let result_container: AsyncGenerationResult = std::sync::Arc::new(std::sync::Mutex::new(None));
        build.async_generation_result = Some(result_container.clone());
        
        // Clone data needed for the background thread
        let markdown = ast.raw_markdown.clone();
        let scene_name = ast.scene.clone();
        let scene_context = build.request.scene_context.clone();
        let claude_config = self.claude.get_config().clone();
        let validator_config = self.validator.clone();
        
        // Get current attempt number
        let attempt = match &build.stage {
            BuildStage::Generating { attempt } => *attempt,
            _ => 1,
        };
        
        info!("🚀 Starting async Claude API call for '{}' (attempt {})", scene_name, attempt);
        
        // Spawn background thread for Claude API call
        std::thread::spawn(move || {
            let claude = super::claude_client::ClaudeClient::new(claude_config);
            
            let result = if let Some(ref md) = markdown {
                claude.generate_from_markdown_with_scene(
                    md,
                    &scene_name,
                    &scene_context,
                    &validator_config
                )
            } else {
                // Fallback - shouldn't happen with current flow
                Err(super::claude_client::ClaudeError::InvalidResponse("No markdown source".to_string()))
            };
            
            // Store result in shared container
            if let Ok(mut guard) = result_container.lock() {
                *guard = Some(result);
            }
        });
        
        // Transition to async polling stage
        build.stage = BuildStage::GeneratingAsync { attempt };
    }
    
    /// Poll for async generation result (non-blocking)
    fn process_generating_poll(&mut self) {
        // First, try to extract the result without holding borrows
        let generation_result = {
            let build = self.current.as_mut().unwrap();
            
            // Check if we have a result container
            let result_container = match build.async_generation_result.as_ref() {
                Some(rc) => rc.clone(),
                None => {
                    // No container - something went wrong, fail
                    build.stage = BuildStage::Failed {
                        stage: "Generation".to_string(),
                        error: "Internal error: no async result container".to_string(),
                    };
                    return;
                }
            };
            
            // Try to get result (non-blocking)
            let result = if let Ok(mut guard) = result_container.try_lock() {
                guard.take()
            } else {
                None
            };
            
            // If we got a result, clear the container reference
            if result.is_some() {
                build.async_generation_result = None;
            }
            
            result
        };
        
        // Now process the result if we got one
        if let Some(result) = generation_result {
            self.handle_generation_result(result);
        }
        // If None, still waiting - do nothing, will poll again next frame
    }
    
    /// Handle the generation result (shared by sync and async paths)
    fn handle_generation_result(&mut self, generation_result: Result<GenerationResult, ClaudeError>) {
        let build = self.current.as_mut().unwrap();
        let ast = build.ast.as_ref().unwrap();
        
        match generation_result {
            Ok(result) => {
                build.fix_iterations = result.fix_iterations;
                build.warnings.extend(result.warnings.clone());
                
                // Convert to GeneratedCode
                let model = result.model.clone();
                let duration_ms = result.duration_ms;
                let generated = GeneratedCode {
                    module_name: format!("{}_{}", ast.scene, ast.service.as_str().to_lowercase()),
                    source: result.code.clone(),
                    imports: Vec::new(),
                    systems: Vec::new(),
                    components: Vec::new(),
                    events: Vec::new(),
                    service: ast.service,
                    metadata: eustress_common::soul::GenerationMetadata {
                        source_path: ast.source_path.clone(),
                        generated_at: chrono::Utc::now().to_rfc3339(),
                        model,
                        duration_ms,
                        cache_key: String::new(),
                        from_cache: false,
                    },
                };
                
                build.generated_code = Some(generated);
                build.generation_result = Some(result.clone());
                
                // Pre-process: Ensure loops have soul::wait() to prevent freezing
                let mut code = result.code.clone();
                if (code.contains("loop {") || code.contains("loop{")) && !code.contains("soul::wait") {
                    warn!("⚠️ Loop detected without soul::wait() - injecting soul::wait(0.001) to prevent freeze");
                    code = inject_wait_into_loop(&code);
                }
                
                // Validate Rune script before marking as ready
                match crate::soul::validate_rune_script(&code) {
                    Ok(()) => {
                        info!("✅ Rune script validation passed");
                        build.stage = BuildStage::Loading;
                    }
                    Err(rune_error) => {
                        info!("📝 Generated Rune code:\n{}", code);
                        
                        // Try to auto-fix errors
                        let max_fix_iterations = 10;
                        let mut current_code = code.clone();
                        let mut current_error = rune_error.clone();
                        let mut fixed = false;
                        let mut injected_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
                        
                        for iteration in 1..=max_fix_iterations {
                            let error_str = current_error.join("\n");
                            warn!("⚠️ Rune validation failed (attempt {}/{}): {}", iteration, max_fix_iterations, error_str);
                            self.error_tracker.track_error(super::error_tracker::TrackedError {
                                category: "validation".to_string(),
                                message: error_str.clone(),
                                timestamp: std::time::Instant::now(),
                                context: None,
                            });
                            
                            if let Some(fix) = self.error_tracker.get_deterministic_fix(&error_str) {
                                let fix_key = format!("{:?}", fix);
                                if injected_vars.contains(&fix_key) {
                                    warn!("⚠️ Already tried fix {:?}, but error persists", fix);
                                    break;
                                }
                                injected_vars.insert(fix_key);
                                
                                info!("🔧 Applying deterministic fix: {:?}", fix);
                                current_code = super::error_tracker::RuneErrorTracker::apply_fix(&current_code, &fix);
                                
                                match crate::soul::validate_rune_script(&current_code) {
                                    Ok(()) => {
                                        info!("✅ Rune script fixed on iteration {}", iteration);
                                        fixed = true;
                                        break;
                                    }
                                    Err(new_error) => {
                                        current_error = new_error;
                                    }
                                }
                            } else {
                                warn!("⚠️ No deterministic fix for error: {}", error_str);
                                break;
                            }
                        }
                        
                        let _ = self.error_tracker.save(std::path::Path::new("error_tracker.json"));
                        
                        if fixed {
                            if let Some(ref mut gen) = build.generated_code {
                                gen.source = current_code;
                            }
                            build.fix_iterations += 1;
                            build.stage = BuildStage::Loading;
                        } else {
                            let error_str = rune_error.join("\n");
                            crate::telemetry::report_rune_validation_error(
                                &build.request.name,
                                &error_str,
                            );
                            
                            build.stage = BuildStage::Failed {
                                stage: "Rune Validation".to_string(),
                                error: format!("Rune script validation failed after {} fix attempts:\n{}", max_fix_iterations, error_str),
                            };
                        }
                    }
                }
            }
            Err(ClaudeError::NoApiKey) => {
                build.stage = BuildStage::Failed {
                    stage: "Generation".to_string(),
                    error: "No Claude API key configured".to_string(),
                };
            }
            Err(ClaudeError::CompilationError { errors }) => {
                let error_msgs: Vec<String> = errors.iter()
                    .map(|e| e.message.clone())
                    .collect();
                let error_str = format!("Code generation failed after {} fix attempts:\n{}", 
                    self.max_fix_iterations, error_msgs.join("\n"));
                
                crate::telemetry::report_claude_generation_error(
                    &build.request.name,
                    &error_str,
                );
                
                build.stage = BuildStage::Failed {
                    stage: "Generation".to_string(),
                    error: error_str,
                };
            }
            Err(e) => {
                let error_str = e.to_string();
                
                crate::telemetry::report_claude_generation_error(
                    &build.request.name,
                    &error_str,
                );
                
                build.stage = BuildStage::Failed {
                    stage: "Generation".to_string(),
                    error: error_str,
                };
            }
        }
    }
    
    /// Process validation stage
    fn process_validating(&mut self) {
        let build = self.current.as_mut().unwrap();
        let code = build.generated_code.as_ref().unwrap();
        
        let result = self.validator.validate(&code.source);
        
        if result.valid {
            build.warnings.extend(result.warnings);
            build.stage = BuildStage::Compiling;
        } else {
            build.stage = BuildStage::Failed {
                stage: "Validation".to_string(),
                error: result.errors.join("\n"),
            };
        }
    }
    
    /// Start async compilation (non-blocking)
    fn process_compiling_start(&mut self, compiler: &mut HotCompiler) {
        let build = self.current.as_mut().unwrap();
        let code = build.generated_code.as_ref().unwrap();
        
        // Start async compilation in background thread
        compiler.compile_async(code);
        build.stage = BuildStage::CompilingAsync;
    }
    
    /// Poll for async compilation result (non-blocking)
    fn process_compiling_poll(&mut self, compiler: &mut HotCompiler) {
        // Check if compilation is complete
        if let Some(result) = compiler.poll_result() {
            let build = self.current.as_mut().unwrap();
            
            if result.success {
                info!("✅ Compilation successful for '{}'", result.module_name);
                build.warnings.extend(result.warnings.clone());
                build.stage = BuildStage::Loading;
            } else {
                error!("❌ Compilation failed: {:?}", result.errors);
                build.stage = BuildStage::Failed {
                    stage: "Compilation".to_string(),
                    error: result.errors.join("\n"),
                };
            }
        }
        // If no result yet, stay in CompilingAsync stage (will be polled next frame)
    }
    
    /// Process loading stage
    fn process_loading(&mut self) {
        let build = self.current.as_mut().unwrap();
        
        // For now, just mark as complete
        // In a full implementation, this would:
        // 1. Load the dynamic library
        // 2. Extract the Plugin
        // 3. Add it to the running App
        
        build.stage = BuildStage::Complete;
    }
    
    /// Finalize the build and create result
    fn finalize_build(&mut self) {
        let build = self.current.take().unwrap();
        
        let (success, error) = match &build.stage {
            BuildStage::Complete => (true, None),
            BuildStage::Failed { error, .. } => (false, Some(error.clone())),
            _ => (false, Some("Unexpected state".to_string())),
        };
        
        let result = PipelineResult {
            entity: build.request.entity,
            name: build.request.name,
            success,
            error,
            generated_code: build.generated_code,
            compile_result: None,
            duration_ms: build.start_time.elapsed().as_millis() as u64,
            fix_iterations: build.fix_iterations,
            warnings: build.warnings,
        };
        
        self.results.push_back(result);
    }
    
    /// Build synchronously (blocking)
    pub fn build_sync(
        &mut self,
        source: &str,
        name: &str,
        compiler: &mut HotCompiler,
    ) -> PipelineResult {
        // Parse
        let ast = match self.parser.parse(source, name) {
            Ok(ast) => ast,
            Err(e) => {
                return PipelineResult {
                    entity: Entity::PLACEHOLDER,
                    name: name.to_string(),
                    success: false,
                    error: Some(format!("Parse error: {}", e)),
                    generated_code: None,
                    compile_result: None,
                    duration_ms: 0,
                    fix_iterations: 0,
                    warnings: Vec::new(),
                };
            }
        };
        
        // Validate AST
        let errors = ast.validate();
        if !errors.is_empty() {
            return PipelineResult {
                entity: Entity::PLACEHOLDER,
                name: name.to_string(),
                success: false,
                error: Some(format!("AST validation errors:\n{}", errors.join("\n"))),
                generated_code: None,
                compile_result: None,
                duration_ms: 0,
                fix_iterations: 0,
                warnings: Vec::new(),
            };
        }
        
        // Generate code
        let gen_result = match self.claude.generate_with_feedback(&ast, &self.validator) {
            Ok(r) => r,
            Err(e) => {
                return PipelineResult {
                    entity: Entity::PLACEHOLDER,
                    name: name.to_string(),
                    success: false,
                    error: Some(format!("Generation error: {}", e)),
                    generated_code: None,
                    compile_result: None,
                    duration_ms: 0,
                    fix_iterations: 0,
                    warnings: Vec::new(),
                };
            }
        };
        
        // Create GeneratedCode
        let generated = GeneratedCode {
            module_name: format!("{}_{}", ast.scene, ast.service.as_str().to_lowercase()),
            source: gen_result.code.clone(),
            imports: Vec::new(),
            systems: Vec::new(),
            components: Vec::new(),
            events: Vec::new(),
            service: ast.service,
            metadata: eustress_common::soul::GenerationMetadata {
                source_path: ast.source_path.clone(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                model: gen_result.model.clone(),
                duration_ms: gen_result.duration_ms,
                cache_key: String::new(),
                from_cache: false,
            },
        };
        
        // Compile
        let compile_result = compiler.compile(&generated);
        
        PipelineResult {
            entity: Entity::PLACEHOLDER,
            name: name.to_string(),
            success: compile_result.success,
            error: if compile_result.success { None } else { Some(compile_result.errors.join("\n")) },
            generated_code: Some(generated),
            compile_result: Some(compile_result.clone()),
            duration_ms: gen_result.duration_ms,
            fix_iterations: gen_result.fix_iterations,
            warnings: gen_result.warnings,
        }
    }
}

// ============================================================================
// Bevy Systems
// ============================================================================

/// System to process the build pipeline each frame
pub fn process_build_pipeline(
    mut pipeline: ResMut<SoulBuildPipeline>,
    mut compiler: ResMut<HotCompiler>,
) {
    pipeline.tick(&mut compiler);
}

/// System to update UI with build status (for visual feedback)
/// Build status is now communicated via Slint UI bindings
pub fn update_build_status_ui(
    _pipeline: Res<SoulBuildPipeline>,
) {
    // Build status UI is handled by Slint - see slint_ui.rs
}

/// Event to trigger a build
#[derive(Event, Message)]
pub struct TriggerBuildEvent {
    pub entity: Entity,
}

/// Event to trigger a Command Bar build (one-shot, no entity)
#[derive(Event, Message, Clone)]
pub struct CommandBarBuildEvent {
    pub command: String,
    /// If true, the command is a cached Rune script that should be executed directly
    /// without calling Claude API (re-executing from history)
    pub use_cached: bool,
}

/// Result of a Command Bar build
#[derive(Clone)]
pub struct CommandBarResult {
    pub success: bool,
    pub rune_code: Option<String>,
    pub error: Option<String>,
    pub output: Vec<String>,
}

/// System to handle build triggers
pub fn handle_build_triggers(
    mut events: MessageReader<TriggerBuildEvent>,
    mut pipeline: ResMut<SoulBuildPipeline>,
    query: Query<(&crate::classes::Instance, &crate::soul::SoulScriptData)>,
    // Query scene entities with their properties for rich context
    scene_query: Query<(
        &Name, 
        &Transform, 
        Option<&crate::classes::Instance>,
        Option<&crate::classes::BasePart>,
    )>,
) {
    for event in events.read() {
        if let Ok((instance, script_data)) = query.get(event.entity) {
            let source_len = script_data.source.len();
            info!("📥 Build trigger received for '{}' (source: {} chars)", instance.name, source_len);
            
            if source_len == 0 {
                warn!("⚠ Soul script '{}' has empty source - nothing to build!", instance.name);
                continue;
            }
            
            // Collect scene entity context with positions and properties
            let scene_context: Vec<SceneEntityContext> = scene_query.iter()
                .filter(|(name, _, _, _)| {
                    let n = name.as_str();
                    !n.is_empty() && n != "SoulScript" && !n.starts_with("Camera")
                })
                .map(|(name, transform, inst, base_part)| {
                    let pos = transform.translation;
                    let entity_type = inst
                        .map(|i| format!("{:?}", i.class_name))
                        .unwrap_or_else(|| "Unknown".to_string());
                    
                    let color = base_part.map(|bp| {
                        let c = bp.color.to_srgba();
                        format!("rgb({:.0}, {:.0}, {:.0})", c.red * 255.0, c.green * 255.0, c.blue * 255.0)
                    });
                    
                    let size = base_part.map(|bp| (bp.size.x, bp.size.y, bp.size.z));
                    
                    SceneEntityContext {
                        name: name.as_str().to_string(),
                        position: (pos.x, pos.y, pos.z),
                        entity_type,
                        color,
                        size,
                    }
                })
                .collect();
            
            info!("📥 Queuing build for '{}' with {} scene entities...", instance.name, scene_context.len());
            pipeline.queue_build(BuildRequest {
                entity: event.entity,
                source: script_data.source.clone(),
                name: instance.name.clone(),
                force: false,
                scene_context,
            });
        } else {
            warn!("⚠ Build trigger for entity {:?} but no Instance/SoulScriptData found", event.entity);
        }
    }
}

/// State for Command Bar builds
#[derive(Resource, Default)]
pub struct CommandBarBuildState {
    /// Current build in progress
    pub building: bool,
    /// Pending command to build
    pub pending_command: Option<String>,
    /// Result of last build
    pub result: Option<CommandBarResult>,
    /// Scene context for the build
    scene_context: Vec<SceneEntityContext>,
}

/// System to handle Command Bar builds
pub fn handle_command_bar_builds(
    mut events: MessageReader<CommandBarBuildEvent>,
    mut state: ResMut<CommandBarBuildState>,
    mut pipeline: ResMut<SoulBuildPipeline>,
    mut cmd_bar_state: Option<ResMut<crate::ui::command_bar::CommandBarState>>,
    scene_query: Query<(
        &Name, 
        &Transform, 
        Option<&crate::classes::Instance>,
        Option<&crate::classes::BasePart>,
    )>,
) {
    // Handle new command bar build requests
    for event in events.read() {
        if state.building {
            warn!("⚠️ Build already in progress, please wait...");
            continue;
        }
        
        // Check if this is a cached script re-execution (no Claude API call needed)
        if event.use_cached {
            info!("🔄 Command Bar: Re-executing cached script");
            
            // Execute the cached Rune script directly
            let mut context = crate::soul::soul_context::SoulContext::default();
            match crate::soul::execute_rune_script(&event.command, &mut context) {
                Ok(()) => {
                    info!("✅ Re-executed script from cache");
                    
                    state.result = Some(CommandBarResult {
                        success: true,
                        rune_code: Some(event.command.clone()),
                        error: None,
                        output: vec!["Script executed successfully".to_string()],
                    });
                }
                Err(e) => {
                    warn!("❌ Failed to re-execute cached script: {}", e);
                    state.result = Some(CommandBarResult {
                        success: false,
                        rune_code: Some(event.command.clone()),
                        error: Some(e.to_string()),
                        output: vec![],
                    });
                }
            }
            continue;
        }
        
        info!("💻 Command Bar: Building '{}'", event.command);
        
        // Collect scene context
        let scene_context: Vec<SceneEntityContext> = scene_query.iter()
            .filter(|(name, _, _, _)| {
                let n = name.as_str();
                !n.is_empty() && n != "SoulScript" && !n.starts_with("Camera")
            })
            .map(|(name, transform, inst, base_part)| {
                let pos = transform.translation;
                let entity_type = inst
                    .map(|i| format!("{:?}", i.class_name))
                    .unwrap_or_else(|| "Unknown".to_string());
                
                let color = base_part.map(|bp| {
                    let c = bp.color.to_srgba();
                    format!("rgb({:.0}, {:.0}, {:.0})", c.red * 255.0, c.green * 255.0, c.blue * 255.0)
                });
                
                let size = base_part.map(|bp| (bp.size.x, bp.size.y, bp.size.z));
                
                SceneEntityContext {
                    name: name.as_str().to_string(),
                    position: (pos.x, pos.y, pos.z),
                    entity_type,
                    color,
                    size,
                }
            })
            .collect();
        
        state.building = true;
        state.pending_command = Some(event.command.clone());
        state.scene_context = scene_context.clone();
        
        // Save undo checkpoint before executing
        // TODO: Implement proper undo snapshot
        
        // Queue the build - we'll use a special entity ID for command bar
        pipeline.queue_build(BuildRequest {
            entity: Entity::PLACEHOLDER,
            source: event.command.clone(),
            name: "CommandBar".to_string(),
            force: true,
            scene_context,
        });
    }
    
    // Check for completed Command Bar builds
    if state.building {
        // Poll for results - check if our CommandBar build completed
        if let Some(result) = pipeline.peek_result() {
            if result.name == "CommandBar" {
                let result = pipeline.poll_result().unwrap();
                state.building = false;
                
                if result.success {
                    if let Some(ref generated) = result.generated_code {
                        info!("✅ Code generated, executing...");
                        
                        // Execute the Rune script
                        let mut context = crate::soul::soul_context::SoulContext::default();
                        match crate::soul::execute_rune_script(&generated.source, &mut context) {
                            Ok(()) => {
                                info!("✅ Script executed successfully");
                                
                                state.result = Some(CommandBarResult {
                                    success: true,
                                    rune_code: Some(generated.source.clone()),
                                    error: None,
                                    output: vec!["Script executed successfully".to_string()],
                                });
                                
                                // Cache the Rune script in CommandBarState for history re-execution
                                if let Some(ref mut cmd_state) = cmd_bar_state {
                                    crate::ui::command_bar::CommandBarPanel::cache_rune_script(
                                        cmd_state.as_mut(),
                                        generated.source.clone()
                                    );
                                }
                            }
                            Err(e) => {
                                error!("❌ Execution failed: {}", e);
                                state.result = Some(CommandBarResult {
                                    success: false,
                                    rune_code: Some(generated.source.clone()),
                                    error: Some(e.clone()),
                                    output: vec![],
                                });
                            }
                        }
                    }
                } else {
                    let err = result.error.unwrap_or_else(|| "Unknown error".to_string());
                    error!("❌ Build failed: {}", err);
                    state.result = Some(CommandBarResult {
                        success: false,
                        rune_code: None,
                        error: Some(err),
                        output: vec![],
                    });
                }
                
                state.pending_command = None;
            }
        }
    }
}

/// System to update SoulScriptData with build results
/// Note: CommandBar builds are handled by handle_command_bar_builds, not this system
pub fn apply_build_results(
    mut pipeline: ResMut<SoulBuildPipeline>,
    mut query: Query<&mut crate::soul::SoulScriptData>,
) {
    while let Some(result) = pipeline.peek_result() {
        // Skip CommandBar builds - they're handled by handle_command_bar_builds
        if result.name == "CommandBar" || result.entity == Entity::PLACEHOLDER {
            break; // Don't consume this result, let handle_command_bar_builds get it
        }
        
        // Consume the result now that we know it's not a CommandBar build
        let result = pipeline.poll_result().unwrap();
        
        if let Ok(mut script_data) = query.get_mut(result.entity) {
            if result.success {
                script_data.build_status = crate::soul::SoulBuildStatus::Built;
                
                // Store and log generated code for debugging
                if let Some(ref generated) = result.generated_code {
                    let code_preview = if generated.source.len() > 200 {
                        format!("{}...", &generated.source[..200])
                    } else {
                        generated.source.clone()
                    };
                    info!("📝 Generated code preview:\n{}", code_preview);
                    script_data.generated_code = Some(generated.source.clone());
                }
                
                script_data.errors.clear();
                info!("✅ Soul script '{}' built successfully in {}ms (fix iterations: {})", 
                      result.name, result.duration_ms, result.fix_iterations);
                
                // Report success to telemetry (sampled)
                crate::telemetry::report_rune_success(
                    &result.name,
                    result.duration_ms,
                );
                
                if !result.warnings.is_empty() {
                    for warning in &result.warnings {
                        warn!("⚠ Build warning: {}", warning);
                    }
                }
            } else {
                script_data.build_status = crate::soul::SoulBuildStatus::Failed;
                let error_msg = result.error.clone().unwrap_or_else(|| "Unknown error".to_string());
                script_data.errors = vec![error_msg.clone()];
                error!("❌ Soul script '{}' build failed: {}", result.name, error_msg);
            }
        } else {
            warn!("⚠ Build result for entity {:?} but entity not found", result.entity);
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for the Soul build pipeline
pub struct SoulBuildPipelinePlugin;

impl Plugin for SoulBuildPipelinePlugin {
    fn build(&self, app: &mut App) {
        // Check for API key in environment first
        let env_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        
        let mut config = SoulConfig::default();
        if let Some(key) = env_api_key {
            config.claude.api_key = Some(key);
            info!("🔑 Using Claude API key from ANTHROPIC_API_KEY environment variable");
        }
        
        app
            .insert_resource(SoulBuildPipeline::new(config))
            .init_resource::<CommandBarBuildState>()
            .add_message::<TriggerBuildEvent>()
            .add_message::<CommandBarBuildEvent>()
            .add_systems(Update, process_build_pipeline)
            .add_systems(Update, update_build_status_ui)
            .add_systems(Update, handle_build_triggers)
            .add_systems(Update, handle_command_bar_builds)
            .add_systems(Update, apply_build_results)
            .add_systems(Update, sync_api_key_from_settings);
    }
}

/// System to sync API key from GlobalSoulSettings to SoulBuildPipeline
/// This ensures the build pipeline always has the latest API key
/// Uses a Local to track if initial sync has been done
fn sync_api_key_from_settings(
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
    space_settings: Option<Res<crate::soul::SoulServiceSettings>>,
    mut pipeline: ResMut<SoulBuildPipeline>,
    mut synced: Local<bool>,
) {
    // Skip if already synced and pipeline has a key
    let pipeline_has_key = pipeline.has_api_key();
    if *synced && pipeline_has_key {
        return;
    }
    
    // Get effective API key from settings
    let effective_key = match (&global_settings, &space_settings) {
        (Some(global), Some(space)) => {
            let key = space.effective_api_key(global);
            if !key.is_empty() { Some(key) } else { None }
        }
        (Some(global), None) => {
            if global.has_api_key() { 
                Some(global.global_api_key.clone()) 
            } else { 
                None 
            }
        }
        _ => None,
    };
    
    // Sync if we have a key from settings and haven't synced yet
    if let Some(key) = effective_key {
        if !*synced {
            info!("🔑 Synced API key from Soul Settings to build pipeline");
            pipeline.set_api_key(key);
            *synced = true;
        }
    }
}
