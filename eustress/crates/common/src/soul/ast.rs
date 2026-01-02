//! # Soul AST
//!
//! Abstract Syntax Tree for parsed Soul scripts.
//! Represents the structured form of .md prose before code generation.

use serde::{Deserialize, Serialize};
use super::{
    ScriptFrontmatter, ScriptType, EventHandler, FunctionDef, 
    GlobalVar, QueryDef, ScriptService,
};
use crate::scene::NodeCategory;

// ============================================================================
// Soul AST
// ============================================================================

/// Complete AST for a Soul script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulAST {
    /// Source file path
    pub source_path: String,
    
    /// Parsed frontmatter
    pub frontmatter: ScriptFrontmatter,
    
    /// Scene name
    pub scene: String,
    
    /// Target service
    pub service: ScriptService,
    
    /// Script type classification
    pub script_type: ScriptType,
    
    /// Global variables
    pub globals: Vec<GlobalVar>,
    
    /// Event handlers
    pub event_handlers: Vec<EventHandler>,
    
    /// Function definitions
    pub functions: Vec<FunctionDef>,
    
    /// Queries used in the script
    pub queries: Vec<QueryDef>,
    
    /// Raw sections (for unstructured content)
    pub raw_sections: Vec<RawSection>,
    
    /// Required instance nodes that must be placed in 3D before compilation
    /// These are anchor points for critical infrastructure in the generated code
    pub required_instances: Vec<RequiredInstance>,
    
    /// Bound instances - required instances that have been placed and linked
    pub bound_instances: Vec<BoundInstance>,
    
    /// Parse warnings (non-fatal issues)
    pub warnings: Vec<String>,
    
    /// Raw markdown source for direct Claude interpretation
    /// When set, this bypasses AST-based generation and sends content directly to Claude
    pub raw_markdown: Option<String>,
}

impl Default for SoulAST {
    fn default() -> Self {
        Self {
            source_path: String::new(),
            frontmatter: ScriptFrontmatter::default(),
            scene: String::new(),
            service: ScriptService::Workspace,
            script_type: ScriptType::Mixed,
            globals: Vec::new(),
            event_handlers: Vec::new(),
            functions: Vec::new(),
            queries: Vec::new(),
            raw_sections: Vec::new(),
            required_instances: Vec::new(),
            bound_instances: Vec::new(),
            warnings: Vec::new(),
            raw_markdown: None,
        }
    }
}

impl SoulAST {
    /// Create a new AST for a scene
    pub fn new(scene: &str, service: ScriptService) -> Self {
        Self {
            scene: scene.to_string(),
            service,
            ..Default::default()
        }
    }
    
    /// Add a global variable
    pub fn add_global(&mut self, var: GlobalVar) {
        self.globals.push(var);
    }
    
    /// Add an event handler
    pub fn add_event_handler(&mut self, handler: EventHandler) {
        self.event_handlers.push(handler);
    }
    
    /// Add a function
    pub fn add_function(&mut self, func: FunctionDef) {
        self.functions.push(func);
    }
    
    /// Add a query
    pub fn add_query(&mut self, query: QueryDef) {
        self.queries.push(query);
    }
    
    /// Add a warning
    pub fn add_warning(&mut self, warning: &str) {
        self.warnings.push(warning.to_string());
    }
    
    /// Convert to JSON for Claude prompt
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    
    /// Get all meta sections
    pub fn get_meta_handlers(&self) -> Vec<&EventHandler> {
        self.event_handlers.iter()
            .filter(|h| h.handler_type == ScriptType::Meta)
            .collect()
    }
    
    /// Get all plausible sections
    pub fn get_plausible_handlers(&self) -> Vec<&EventHandler> {
        self.event_handlers.iter()
            .filter(|h| h.handler_type == ScriptType::Plausible)
            .collect()
    }
    
    /// Validate AST for service constraints
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        
        // Check service constraints
        if self.service == ScriptService::ServerScriptService {
            // Server-only scripts shouldn't have client-side operations
            for handler in &self.event_handlers {
                if handler.name.to_lowercase().contains("local") {
                    errors.push(format!(
                        "Handler '{}' appears to be client-side but is in ServerScriptService",
                        handler.name
                    ));
                }
            }
        }
        
        if self.service == ScriptService::ReplicatedFirst {
            // ReplicatedFirst shouldn't access Workspace directly
            for handler in &self.event_handlers {
                if let Some(ref when) = handler.when {
                    if when.to_lowercase().contains("workspace") {
                        errors.push(format!(
                            "Handler '{}' accesses Workspace from ReplicatedFirst (not allowed)",
                            handler.name
                        ));
                    }
                }
            }
        }
        
        errors
    }
    
    // ========================================================================
    // Complexity Analysis for Dynamic Model Selection
    // ========================================================================
    
    /// Calculate complexity score for dynamic model selection
    /// 
    /// Score ranges:
    /// - 0-30: Simple (Haiku) - Single handler, few globals, basic logic
    /// - 31-70: Medium (Sonnet) - Multiple handlers, queries, moderate logic
    /// - 71+: Complex (Opus) - Many systems, advanced patterns, networking
    pub fn complexity_score(&self) -> u32 {
        let analysis = self.analyze_complexity();
        analysis.total_score()
    }
    
    /// Get detailed complexity analysis
    pub fn analyze_complexity(&self) -> ComplexityAnalysis {
        ComplexityAnalysis::from_ast(self)
    }
}

// ============================================================================
// Complexity Analysis
// ============================================================================

/// Detailed complexity analysis of a Soul AST
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityAnalysis {
    /// Number of event handlers
    pub handler_count: u32,
    /// Number of functions
    pub function_count: u32,
    /// Number of global variables
    pub global_count: u32,
    /// Number of queries
    pub query_count: u32,
    /// Number of raw sections
    pub raw_section_count: u32,
    /// Total lines of prose content
    pub prose_lines: u32,
    /// Has networking/replication logic
    pub has_networking: bool,
    /// Has physics/collision logic
    pub has_physics: bool,
    /// Has animation logic
    pub has_animation: bool,
    /// Has UI/GUI logic
    pub has_ui: bool,
    /// Has state machine patterns
    pub has_state_machine: bool,
    /// Has async/coroutine patterns
    pub has_async: bool,
    /// Complexity keywords found
    pub complexity_keywords: Vec<String>,
    /// Estimated output size (lines of Rust)
    pub estimated_output_lines: u32,
}

impl ComplexityAnalysis {
    /// Analyze an AST and produce complexity metrics
    pub fn from_ast(ast: &SoulAST) -> Self {
        let mut analysis = Self {
            handler_count: ast.event_handlers.len() as u32,
            function_count: ast.functions.len() as u32,
            global_count: ast.globals.len() as u32,
            query_count: ast.queries.len() as u32,
            raw_section_count: ast.raw_sections.len() as u32,
            prose_lines: 0,
            has_networking: false,
            has_physics: false,
            has_animation: false,
            has_ui: false,
            has_state_machine: false,
            has_async: false,
            complexity_keywords: Vec::new(),
            estimated_output_lines: 0,
        };
        
        // Count prose lines and detect patterns
        let mut all_content = String::new();
        
        for handler in &ast.event_handlers {
            // Count then_actions as prose lines
            analysis.prose_lines += handler.then_actions.len() as u32;
            for action in &handler.then_actions {
                all_content.push_str(action);
                all_content.push('\n');
            }
            
            // Include when condition
            if let Some(ref when) = handler.when {
                all_content.push_str(when);
                all_content.push('\n');
            }
            
            // Include if condition
            if let Some(ref cond) = handler.if_condition {
                all_content.push_str(cond);
                all_content.push('\n');
            }
            
            // Include else branch
            if let Some(ref else_branch) = handler.else_branch {
                analysis.prose_lines += else_branch.len() as u32;
                for line in else_branch {
                    all_content.push_str(line);
                    all_content.push('\n');
                }
            }
        }
        
        for func in &ast.functions {
            // body is Vec<String>
            analysis.prose_lines += func.body.len() as u32;
            for line in &func.body {
                all_content.push_str(line);
                all_content.push('\n');
            }
        }
        
        for section in &ast.raw_sections {
            analysis.prose_lines += section.content.len() as u32;
            for line in &section.content {
                all_content.push_str(line);
                all_content.push('\n');
            }
        }
        
        let content_lower = all_content.to_lowercase();
        
        // Detect networking patterns
        let networking_keywords = ["replicate", "remote", "server", "client", "sync", "network", "rpc"];
        for kw in networking_keywords {
            if content_lower.contains(kw) {
                analysis.has_networking = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Detect physics patterns
        let physics_keywords = ["collision", "velocity", "force", "raycast", "physics", "gravity", "impulse"];
        for kw in physics_keywords {
            if content_lower.contains(kw) {
                analysis.has_physics = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Detect animation patterns
        let animation_keywords = ["animate", "tween", "lerp", "keyframe", "animation", "motor6d"];
        for kw in animation_keywords {
            if content_lower.contains(kw) {
                analysis.has_animation = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Detect UI patterns
        let ui_keywords = ["gui", "button", "textlabel", "frame", "screen", "ui", "billboard"];
        for kw in ui_keywords {
            if content_lower.contains(kw) {
                analysis.has_ui = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Detect state machine patterns
        let state_keywords = ["state", "transition", "phase", "mode", "switch", "enum"];
        for kw in state_keywords {
            if content_lower.contains(kw) {
                analysis.has_state_machine = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Detect async patterns
        let async_keywords = ["wait", "delay", "async", "spawn", "coroutine", "yield", "timer"];
        for kw in async_keywords {
            if content_lower.contains(kw) {
                analysis.has_async = true;
                analysis.complexity_keywords.push(kw.to_string());
            }
        }
        
        // Estimate output lines (rough heuristic)
        analysis.estimated_output_lines = 
            50 + // Base imports and boilerplate
            (analysis.handler_count * 30) + // ~30 lines per handler
            (analysis.function_count * 20) + // ~20 lines per function
            (analysis.query_count * 10) + // ~10 lines per query
            (analysis.global_count * 5) + // ~5 lines per global
            (analysis.prose_lines / 2); // Prose converts roughly 2:1
        
        analysis
    }
    
    /// Calculate total complexity score
    pub fn total_score(&self) -> u32 {
        let mut score: u32 = 0;
        
        // Base counts (linear scaling)
        score += self.handler_count * 8;      // Each handler adds 8 points
        score += self.function_count * 6;     // Each function adds 6 points
        score += self.global_count * 2;       // Each global adds 2 points
        score += self.query_count * 5;        // Each query adds 5 points
        score += self.raw_section_count * 3;  // Each raw section adds 3 points
        
        // Prose length (logarithmic scaling to avoid explosion)
        score += (self.prose_lines as f32).sqrt() as u32 * 2;
        
        // Feature flags (significant complexity indicators)
        if self.has_networking { score += 20; }
        if self.has_physics { score += 15; }
        if self.has_animation { score += 12; }
        if self.has_ui { score += 10; }
        if self.has_state_machine { score += 15; }
        if self.has_async { score += 12; }
        
        // Keyword diversity bonus
        let unique_keywords = self.complexity_keywords.len() as u32;
        score += unique_keywords * 2;
        
        // Output size factor
        if self.estimated_output_lines > 200 { score += 10; }
        if self.estimated_output_lines > 500 { score += 15; }
        if self.estimated_output_lines > 1000 { score += 25; }
        
        score
    }
    
    /// Get recommended model tier
    pub fn recommended_tier(&self) -> super::ModelTier {
        super::ModelTier::from_complexity(self.total_score())
    }
    
    /// Get human-readable complexity level
    pub fn complexity_level(&self) -> &'static str {
        match self.total_score() {
            0..=30 => "Simple",
            31..=70 => "Medium",
            71..=100 => "Complex",
            _ => "Very Complex",
        }
    }
    
    /// Generate a summary string
    pub fn summary(&self) -> String {
        format!(
            "Complexity: {} (score: {})\n\
             Handlers: {}, Functions: {}, Queries: {}\n\
             Features: {}\n\
             Recommended: {}",
            self.complexity_level(),
            self.total_score(),
            self.handler_count,
            self.function_count,
            self.query_count,
            self.feature_flags_string(),
            self.recommended_tier()
        )
    }
    
    /// Get feature flags as a string
    fn feature_flags_string(&self) -> String {
        let mut flags = Vec::new();
        if self.has_networking { flags.push("Networking"); }
        if self.has_physics { flags.push("Physics"); }
        if self.has_animation { flags.push("Animation"); }
        if self.has_ui { flags.push("UI"); }
        if self.has_state_machine { flags.push("StateMachine"); }
        if self.has_async { flags.push("Async"); }
        
        if flags.is_empty() {
            "None".to_string()
        } else {
            flags.join(", ")
        }
    }
}

// ============================================================================
// Raw Section
// ============================================================================

/// Unstructured section from the script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSection {
    /// Section heading
    pub heading: String,
    
    /// Heading level (1-6)
    pub level: u8,
    
    /// Raw content lines
    pub content: Vec<String>,
    
    /// Section type tag (if any)
    pub section_type: Option<ScriptType>,
}

// ============================================================================
// AST Node Types
// ============================================================================

/// Generic AST node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ASTNode {
    /// Global variable declaration
    Global(GlobalVar),
    
    /// Event handler
    EventHandler(EventHandler),
    
    /// Function definition
    Function(FunctionDef),
    
    /// Query definition
    Query(QueryDef),
    
    /// Raw/unstructured section
    Raw(RawSection),
    
    /// Comment (preserved for context)
    Comment(String),
}

// ============================================================================
// Code Generation Context
// ============================================================================

/// Context passed to code generator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGenContext {
    /// Scene name
    pub scene: String,
    
    /// Target service
    pub service: ScriptService,
    
    /// Script type
    pub script_type: ScriptType,
    
    /// Default unit for distances
    pub default_unit: String,
    
    /// Unit conversion table (for Claude prompt)
    pub unit_conversions: std::collections::HashMap<String, f32>,
    
    /// Available components (for type hints)
    pub available_components: Vec<String>,
    
    /// Available events
    pub available_events: Vec<String>,
}

impl Default for CodeGenContext {
    fn default() -> Self {
        let mut unit_conversions = std::collections::HashMap::new();
        unit_conversions.insert("foot_to_studs".to_string(), 1.0886);
        unit_conversions.insert("meter_to_studs".to_string(), 3.571);
        unit_conversions.insert("yard_to_studs".to_string(), 3.266);
        unit_conversions.insert("km_to_studs".to_string(), 3571.43);
        unit_conversions.insert("mile_to_studs".to_string(), 5748.03);
        
        Self {
            scene: String::new(),
            service: ScriptService::Workspace,
            script_type: ScriptType::Mixed,
            default_unit: "studs".to_string(),
            unit_conversions,
            available_components: vec![
                "Transform".to_string(),
                "BasePart".to_string(),
                "Humanoid".to_string(),
                "Instance".to_string(),
                "Model".to_string(),
            ],
            available_events: vec![
                "Touched".to_string(),
                "PlayerAdded".to_string(),
                "PlayerRemoving".to_string(),
                "ChildAdded".to_string(),
                "ChildRemoved".to_string(),
            ],
        }
    }
}

impl CodeGenContext {
    /// Create context from AST
    pub fn from_ast(ast: &SoulAST) -> Self {
        Self {
            scene: ast.scene.clone(),
            service: ast.service,
            script_type: ast.script_type,
            default_unit: ast.frontmatter.unit.clone().unwrap_or("studs".to_string()),
            ..Default::default()
        }
    }
}

// ============================================================================
// Generated Code
// ============================================================================

/// Generated Rust code from Soul script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedCode {
    /// Module name
    pub module_name: String,
    
    /// Full Rust source code
    pub source: String,
    
    /// Imports needed
    pub imports: Vec<String>,
    
    /// System functions generated
    pub systems: Vec<String>,
    
    /// Components defined
    pub components: Vec<String>,
    
    /// Events defined
    pub events: Vec<String>,
    
    /// Target service
    pub service: ScriptService,
    
    /// Generation metadata
    pub metadata: GenerationMetadata,
}

/// Metadata about code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMetadata {
    /// Source script path
    pub source_path: String,
    
    /// Generation timestamp
    pub generated_at: String,
    
    /// Claude model used
    pub model: String,
    
    /// Generation duration (ms)
    pub duration_ms: u64,
    
    /// Cache key (blake3 hash)
    pub cache_key: String,
    
    /// Was this from cache?
    pub from_cache: bool,
}

// ============================================================================
// Required Instance System
// ============================================================================

/// A required instance node that must be placed in 3D before Soul compilation.
/// 
/// This bridges Soul scripts with the scene graph - the script declares what
/// entities it needs, and Studio requires the user to place them before
/// the code can be generated.
/// 
/// # Example in Soul Markdown
/// 
/// ```markdown
/// ## Required Instances
/// 
/// - **spawn_point**: SpawnLocation - Where players spawn
/// - **boss_arena**: Trigger - The boss fight area
/// - **treasure_chest**: Part - The reward container
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequiredInstance {
    /// Unique identifier within the script (e.g., "spawn_point")
    pub name: String,
    
    /// Human-readable description
    pub description: String,
    
    /// Expected node category (determines what can be bound)
    pub category: NodeCategory,
    
    /// Expected entity class name (e.g., "Part", "Model", "SpawnLocation")
    pub expected_class: String,
    
    /// Is this instance optional?
    pub optional: bool,
    
    /// Minimum count (for arrays of instances)
    pub min_count: u32,
    
    /// Maximum count (0 = unlimited)
    pub max_count: u32,
    
    /// Constraints on the instance (parsed from Soul)
    pub constraints: Vec<InstanceConstraint>,
    
    /// Default prompt for AI generation if not placed
    pub default_prompt: Option<String>,
    
    /// Line number in source where this was declared
    pub source_line: u32,
}

impl Default for RequiredInstance {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            category: NodeCategory::Empty,
            expected_class: "Part".to_string(),
            optional: false,
            min_count: 1,
            max_count: 1,
            constraints: Vec::new(),
            default_prompt: None,
            source_line: 0,
        }
    }
}

/// Constraint on a required instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstanceConstraint {
    /// Must be within a certain distance of another instance
    NearTo { target: String, max_distance: f32 },
    
    /// Must be above a certain height
    MinHeight(f32),
    
    /// Must be below a certain height
    MaxHeight(f32),
    
    /// Must be inside a specific region/trigger
    InsideRegion(String),
    
    /// Must have a specific tag
    HasTag(String),
    
    /// Must have a specific attribute value
    HasAttribute { name: String, value: String },
    
    /// Custom constraint (passed to Claude for interpretation)
    Custom(String),
}

/// A bound instance - a required instance that has been placed in the scene
/// and linked to a specific entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundInstance {
    /// The required instance this binds to
    pub required_name: String,
    
    /// The entity ID in the scene
    pub entity_id: u32,
    
    /// The entity name (for display)
    pub entity_name: String,
    
    /// World position at bind time
    pub position: [f32; 3],
    
    /// World rotation at bind time (quaternion)
    pub rotation: [f32; 4],
    
    /// Whether the binding is valid (constraints satisfied)
    pub valid: bool,
    
    /// Validation errors (if any)
    pub validation_errors: Vec<String>,
    
    /// When this was bound (Unix timestamp)
    pub bound_at: u64,
}

impl Default for BoundInstance {
    fn default() -> Self {
        Self {
            required_name: String::new(),
            entity_id: 0,
            entity_name: String::new(),
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            valid: false,
            validation_errors: Vec::new(),
            bound_at: 0,
        }
    }
}

// ============================================================================
// SoulAST Instance Methods
// ============================================================================

impl SoulAST {
    /// Add a required instance
    pub fn add_required_instance(&mut self, instance: RequiredInstance) {
        self.required_instances.push(instance);
    }
    
    /// Bind an entity to a required instance
    pub fn bind_instance(
        &mut self,
        required_name: &str,
        entity_id: u32,
        entity_name: &str,
        position: [f32; 3],
        rotation: [f32; 4],
    ) -> Result<(), String> {
        // Find the required instance
        let required = self.required_instances.iter()
            .find(|r| r.name == required_name)
            .ok_or_else(|| format!("No required instance named '{}'", required_name))?;
        
        // Check if already bound (for single instances)
        if required.max_count == 1 {
            if self.bound_instances.iter().any(|b| b.required_name == required_name) {
                return Err(format!("'{}' is already bound", required_name));
            }
        }
        
        // Create binding
        let binding = BoundInstance {
            required_name: required_name.to_string(),
            entity_id,
            entity_name: entity_name.to_string(),
            position,
            rotation,
            valid: true, // Will be validated separately
            validation_errors: Vec::new(),
            bound_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        
        self.bound_instances.push(binding);
        Ok(())
    }
    
    /// Unbind an instance
    pub fn unbind_instance(&mut self, required_name: &str, entity_id: u32) {
        self.bound_instances.retain(|b| {
            !(b.required_name == required_name && b.entity_id == entity_id)
        });
    }
    
    /// Get all unbound required instances
    pub fn unbound_instances(&self) -> Vec<&RequiredInstance> {
        self.required_instances.iter()
            .filter(|r| {
                let bound_count = self.bound_instances.iter()
                    .filter(|b| b.required_name == r.name)
                    .count() as u32;
                bound_count < r.min_count
            })
            .collect()
    }
    
    /// Check if all required instances are bound
    pub fn all_instances_bound(&self) -> bool {
        self.unbound_instances().is_empty()
    }
    
    /// Validate all bindings against constraints
    pub fn validate_bindings(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        
        // First pass: collect positions for NearTo constraints
        let positions: std::collections::HashMap<String, [f32; 3]> = self.bound_instances.iter()
            .map(|b| (b.required_name.clone(), b.position))
            .collect();
        
        // Second pass: validate each binding
        for binding in &mut self.bound_instances {
            binding.validation_errors.clear();
            binding.valid = true;
            
            // Find the required instance
            if let Some(required) = self.required_instances.iter()
                .find(|r| r.name == binding.required_name)
            {
                // Check constraints
                for constraint in &required.constraints {
                    match constraint {
                        InstanceConstraint::MinHeight(min) => {
                            if binding.position[1] < *min {
                                let err = format!(
                                    "'{}' must be at least {} studs high (currently {})",
                                    binding.entity_name, min, binding.position[1]
                                );
                                binding.validation_errors.push(err.clone());
                                errors.push(err);
                                binding.valid = false;
                            }
                        }
                        InstanceConstraint::MaxHeight(max) => {
                            if binding.position[1] > *max {
                                let err = format!(
                                    "'{}' must be at most {} studs high (currently {})",
                                    binding.entity_name, max, binding.position[1]
                                );
                                binding.validation_errors.push(err.clone());
                                errors.push(err);
                                binding.valid = false;
                            }
                        }
                        InstanceConstraint::NearTo { target, max_distance } => {
                            // Find the target position from our pre-collected map
                            if let Some(target_pos) = positions.get(target) {
                                let dx = binding.position[0] - target_pos[0];
                                let dy = binding.position[1] - target_pos[1];
                                let dz = binding.position[2] - target_pos[2];
                                let dist = (dx*dx + dy*dy + dz*dz).sqrt();
                                
                                if dist > *max_distance {
                                    let err = format!(
                                        "'{}' must be within {} studs of '{}' (currently {:.1} studs)",
                                        binding.entity_name, max_distance, target, dist
                                    );
                                    binding.validation_errors.push(err.clone());
                                    errors.push(err);
                                    binding.valid = false;
                                }
                            }
                        }
                        _ => {
                            // Other constraints require scene access
                        }
                    }
                }
            }
        }
        
        errors
    }
    
    /// Get binding status summary for UI
    pub fn binding_status(&self) -> BindingStatus {
        let total = self.required_instances.len();
        let required_count = self.required_instances.iter()
            .filter(|r| !r.optional)
            .count();
        let bound_count = self.required_instances.iter()
            .filter(|r| {
                self.bound_instances.iter()
                    .any(|b| b.required_name == r.name)
            })
            .count();
        let valid_count = self.bound_instances.iter()
            .filter(|b| b.valid)
            .count();
        
        BindingStatus {
            total_required: total,
            required_non_optional: required_count,
            bound: bound_count,
            valid: valid_count,
            ready_to_compile: bound_count >= required_count && valid_count == bound_count,
        }
    }
}

/// Summary of instance binding status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingStatus {
    /// Total required instances declared
    pub total_required: usize,
    /// Non-optional required instances
    pub required_non_optional: usize,
    /// Number of bound instances
    pub bound: usize,
    /// Number of valid bindings
    pub valid: usize,
    /// Ready to compile (all required bound and valid)
    pub ready_to_compile: bool,
}
