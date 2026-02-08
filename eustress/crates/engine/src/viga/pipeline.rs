//! # VIGA Pipeline
//!
//! Orchestrates the generate-render-verify loop as a background process.

use bevy::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::agent::{VigaAgent, AgentState};
use super::context::{VigaContext, IterationHistory, CodeDiff};
use super::generator::{VigaGenerator, GeneratorConfig};
use super::verifier::{VigaVerifier, VerifierConfig, VerificationResult};
use super::image_utils::ImageData;

/// VIGA request - input for the pipeline
#[derive(Debug, Clone)]
pub struct VigaRequest {
    /// Reference image (base64 data URL)
    pub reference_image: String,
    /// Optional text description
    pub description: Option<String>,
    /// Maximum iterations
    pub max_iterations: u32,
    /// Target similarity threshold
    pub target_similarity: f32,
}

impl VigaRequest {
    /// Create request with just an image
    pub fn from_image(reference_image: String) -> Self {
        Self {
            reference_image,
            description: None,
            max_iterations: 10,
            target_similarity: 0.90,
        }
    }
    
    /// Create request with image and description
    pub fn with_description(reference_image: String, description: String) -> Self {
        Self {
            reference_image,
            description: Some(description),
            max_iterations: 10,
            target_similarity: 0.90,
        }
    }
}

/// VIGA result - output from the pipeline
#[derive(Debug, Clone)]
pub struct VigaResult {
    /// Whether generation was successful
    pub success: bool,
    /// Final Rune script code
    pub code: Option<String>,
    /// Final similarity score
    pub similarity: f32,
    /// Number of iterations used
    pub iterations: u32,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
}

/// VIGA pipeline status
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VigaStatus {
    /// Idle, no work in progress
    #[default]
    Idle,
    /// Processing a request
    Processing,
    /// Waiting for screenshot capture
    WaitingForScreenshot,
    /// Waiting for async LLM response
    WaitingForLLM,
    /// Completed
    Completed,
    /// Failed
    Failed,
}

/// Thread-safe async result container
type AsyncResult<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// In-progress VIGA task
struct VigaTask {
    /// The original request
    request: VigaRequest,
    /// Agent state machine
    agent: VigaAgent,
    /// Contextual memory
    context: VigaContext,
    /// Start time
    start_time: Instant,
    /// Current generated code
    current_code: Option<String>,
    /// Pending screenshot request
    screenshot_pending: bool,
    /// Last captured screenshot
    last_screenshot: Option<String>,
    /// Async LLM result container
    async_result: Option<AsyncResult<String>>,
    /// Last verification result
    last_verification: Option<VerificationResult>,
}

/// VIGA Pipeline Resource
#[derive(Resource)]
pub struct VigaPipeline {
    /// Request queue
    queue: VecDeque<VigaRequest>,
    /// Current task in progress
    current: Option<VigaTask>,
    /// Completed results
    results: VecDeque<VigaResult>,
    /// Generator
    generator: VigaGenerator,
    /// Verifier
    verifier: VigaVerifier,
    /// Current status
    status: VigaStatus,
}

impl Default for VigaPipeline {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            current: None,
            results: VecDeque::new(),
            generator: VigaGenerator::default(),
            verifier: VigaVerifier::default(),
            status: VigaStatus::Idle,
        }
    }
}

impl VigaPipeline {
    /// Create with custom configs
    pub fn new(generator_config: GeneratorConfig, verifier_config: VerifierConfig) -> Self {
        Self {
            queue: VecDeque::new(),
            current: None,
            results: VecDeque::new(),
            generator: VigaGenerator::new(generator_config),
            verifier: VigaVerifier::new(verifier_config),
            status: VigaStatus::Idle,
        }
    }
    
    /// Queue a new VIGA request
    pub fn queue_request(&mut self, request: VigaRequest) {
        info!("🎨 VIGA: Queued new request (queue size: {})", self.queue.len() + 1);
        self.queue.push_back(request);
    }
    
    /// Get current status
    pub fn status(&self) -> VigaStatus {
        self.status.clone()
    }
    
    /// Get current agent state (if processing)
    pub fn agent_state(&self) -> Option<AgentState> {
        self.current.as_ref().map(|t| t.agent.state)
    }
    
    /// Get current iteration (if processing)
    pub fn current_iteration(&self) -> Option<u32> {
        self.current.as_ref().map(|t| t.agent.iteration)
    }
    
    /// Get current similarity (if processing)
    pub fn current_similarity(&self) -> Option<f32> {
        self.current.as_ref().map(|t| t.agent.current_similarity)
    }
    
    /// Get status message
    pub fn status_message(&self) -> String {
        match &self.current {
            Some(task) => task.agent.status_message(),
            None => match self.status {
                VigaStatus::Idle => "VIGA: Idle".to_string(),
                VigaStatus::Completed => "VIGA: Completed".to_string(),
                VigaStatus::Failed => "VIGA: Failed".to_string(),
                _ => "VIGA: Unknown".to_string(),
            },
        }
    }
    
    /// Check if currently processing
    pub fn is_processing(&self) -> bool {
        self.current.is_some()
    }
    
    /// Poll for completed result
    pub fn poll_result(&mut self) -> Option<VigaResult> {
        self.results.pop_front()
    }
    
    /// Check if screenshot is needed
    pub fn needs_screenshot(&self) -> bool {
        self.current.as_ref().map(|t| t.screenshot_pending).unwrap_or(false)
    }
    
    /// Provide screenshot to pipeline
    pub fn provide_screenshot(&mut self, screenshot: String) {
        if let Some(ref mut task) = self.current {
            task.last_screenshot = Some(screenshot);
            task.screenshot_pending = false;
            self.status = VigaStatus::Processing;
            info!("🎨 VIGA: Screenshot received");
        }
    }
    
    /// Get current generated code (for execution)
    pub fn get_pending_code(&self) -> Option<String> {
        self.current.as_ref().and_then(|t| {
            if matches!(t.agent.state, AgentState::Executing) {
                t.current_code.clone()
            } else {
                None
            }
        })
    }
    
    /// Mark code as executed (move to rendering)
    pub fn mark_code_executed(&mut self) {
        if let Some(ref mut task) = self.current {
            if matches!(task.agent.state, AgentState::Executing) {
                task.agent.next_state(); // Move to Rendering
                task.screenshot_pending = true;
                self.status = VigaStatus::WaitingForScreenshot;
                info!("🎨 VIGA: Code executed, waiting for screenshot");
            }
        }
    }
    
    /// Cancel current task
    pub fn cancel(&mut self) {
        if self.current.is_some() {
            info!("🎨 VIGA: Task cancelled");
            self.current = None;
            self.status = VigaStatus::Idle;
        }
    }
    
    /// Process one tick of the pipeline
    /// Returns generated code if ready for execution
    pub fn tick(&mut self, api_key: Option<&str>) -> Option<String> {
        // Start next task if idle
        if self.current.is_none() {
            if let Some(request) = self.queue.pop_front() {
                self.start_task(request);
            }
            return None;
        }
        
        // Process current task
        let task = self.current.as_mut().unwrap();
        
        match task.agent.state {
            AgentState::Idle => {
                task.agent.start();
            }
            
            AgentState::AnalyzingReference => {
                // Skip to planning (analysis happens in LLM call)
                task.agent.next_state();
            }
            
            AgentState::Planning => {
                // Skip planning for now (can be enabled in config)
                task.agent.next_state();
            }
            
            AgentState::Generating => {
                // Start async LLM call if not already started
                if task.async_result.is_none() {
                    self.start_generation(api_key);
                } else {
                    // Check if async result is ready
                    if let Some(code) = self.poll_generation() {
                        return Some(code);
                    }
                }
            }
            
            AgentState::Executing => {
                // Code is being executed externally
                // Return code for execution if not yet returned
                if let Some(ref code) = task.current_code {
                    return Some(code.clone());
                }
            }
            
            AgentState::Rendering => {
                // Waiting for screenshot
                if !task.screenshot_pending && task.last_screenshot.is_some() {
                    task.agent.next_state(); // Move to Verifying
                }
            }
            
            AgentState::Verifying => {
                // Start async verification if not already started
                if task.async_result.is_none() {
                    self.start_verification(api_key);
                } else {
                    // Check if async result is ready
                    self.poll_verification();
                }
            }
            
            AgentState::Feedback => {
                // Process feedback and decide next step
                self.process_feedback();
            }
            
            AgentState::Complete => {
                self.finalize_success();
            }
            
            AgentState::Failed => {
                self.finalize_failure();
            }
        }
        
        None
    }
    
    /// Start a new task
    fn start_task(&mut self, request: VigaRequest) {
        info!("🎨 VIGA: Starting new task");
        
        let context = if let Some(ref desc) = request.description {
            VigaContext::with_description(request.reference_image.clone(), desc.clone())
        } else {
            VigaContext::new(request.reference_image.clone())
        };
        
        let mut agent = VigaAgent::new(request.max_iterations, request.target_similarity);
        agent.start();
        
        self.current = Some(VigaTask {
            request,
            agent,
            context,
            start_time: Instant::now(),
            current_code: None,
            screenshot_pending: false,
            last_screenshot: None,
            async_result: None,
            last_verification: None,
        });
        
        self.status = VigaStatus::Processing;
    }
    
    /// Start async generation
    fn start_generation(&mut self, api_key: Option<&str>) {
        let task = self.current.as_mut().unwrap();
        
        let Some(api_key) = api_key else {
            task.agent.fail("No API key configured".to_string());
            return;
        };
        
        // Create async result container
        let result_container: AsyncResult<String> = Arc::new(Mutex::new(None));
        task.async_result = Some(result_container.clone());
        
        // Build prompts
        let system_prompt = self.generator.build_system_prompt();
        let user_prompt = if task.context.iteration == 0 {
            self.generator.build_initial_prompt(&task.context)
        } else {
            let feedback = task.last_verification
                .as_ref()
                .map(|v| v.feedback.clone())
                .unwrap_or_default();
            self.generator.build_iteration_prompt(&task.context, &feedback)
        };
        
        // Get reference image
        let reference_image = task.context.reference_image.clone();
        let rendered_image = task.last_screenshot.clone();
        let api_key = api_key.to_string();
        let iteration = task.context.iteration;
        
        info!("🎨 VIGA: Starting generation (iteration {})", iteration);
        self.status = VigaStatus::WaitingForLLM;
        
        // Spawn background thread for LLM call
        std::thread::spawn(move || {
            let result = call_claude_vision(
                &api_key,
                &system_prompt,
                &user_prompt,
                reference_image.as_deref(),
                rendered_image.as_deref(),
            );
            
            if let Ok(mut guard) = result_container.lock() {
                *guard = Some(result);
            }
        });
    }
    
    /// Poll for generation result
    fn poll_generation(&mut self) -> Option<String> {
        let task = self.current.as_mut().unwrap();
        
        let result = {
            let async_result = task.async_result.as_ref()?;
            let guard = async_result.lock().ok()?;
            guard.clone()
        };
        
        if let Some(result) = result {
            task.async_result = None;
            self.status = VigaStatus::Processing;
            
            match result {
                Ok(response) => {
                    // Extract code from response
                    if let Some(code) = self.generator.extract_rune_code(&response) {
                        info!("🎨 VIGA: Generated {} chars of code", code.len());
                        task.current_code = Some(code.clone());
                        task.agent.next_state(); // Move to Executing
                        return Some(code);
                    } else {
                        warn!("🎨 VIGA: Failed to extract code from response");
                        task.agent.fail("Failed to extract code from LLM response".to_string());
                    }
                }
                Err(e) => {
                    error!("🎨 VIGA: Generation failed: {}", e);
                    task.agent.fail(e);
                }
            }
        }
        
        None
    }
    
    /// Start async verification
    fn start_verification(&mut self, api_key: Option<&str>) {
        let task = self.current.as_mut().unwrap();
        
        let Some(api_key) = api_key else {
            task.agent.fail("No API key configured".to_string());
            return;
        };
        
        // Create async result container
        let result_container: AsyncResult<String> = Arc::new(Mutex::new(None));
        task.async_result = Some(result_container.clone());
        
        // Build prompts
        let system_prompt = self.verifier.build_system_prompt();
        let user_prompt = self.verifier.build_verification_prompt(&task.context);
        
        // Get images
        let reference_image = task.context.reference_image.clone();
        let rendered_image = task.last_screenshot.clone();
        let api_key = api_key.to_string();
        
        info!("🎨 VIGA: Starting verification");
        self.status = VigaStatus::WaitingForLLM;
        
        // Spawn background thread for LLM call
        std::thread::spawn(move || {
            let result = call_claude_vision(
                &api_key,
                &system_prompt,
                &user_prompt,
                reference_image.as_deref(),
                rendered_image.as_deref(),
            );
            
            if let Ok(mut guard) = result_container.lock() {
                *guard = Some(result);
            }
        });
    }
    
    /// Poll for verification result
    fn poll_verification(&mut self) {
        let task = self.current.as_mut().unwrap();
        
        let result = {
            let async_result = task.async_result.as_ref();
            if async_result.is_none() {
                return;
            }
            let guard = async_result.unwrap().lock().ok();
            if guard.is_none() {
                return;
            }
            guard.unwrap().clone()
        };
        
        if let Some(result) = result {
            task.async_result = None;
            self.status = VigaStatus::Processing;
            
            match result {
                Ok(response) => {
                    // Parse verification response
                    let verification = self.verifier.parse_verification_response(&response);
                    
                    info!(
                        "🎨 VIGA: Verification complete - similarity: {:.1}%",
                        verification.similarity * 100.0
                    );
                    
                    task.agent.update_similarity(verification.similarity);
                    task.last_verification = Some(verification);
                    task.agent.next_state(); // Move to Feedback
                }
                Err(e) => {
                    error!("🎨 VIGA: Verification failed: {}", e);
                    task.agent.fail(e);
                }
            }
        }
    }
    
    /// Process feedback and update context
    fn process_feedback(&mut self) {
        let task = self.current.as_mut().unwrap();
        
        // Add iteration to history
        let history_entry = IterationHistory {
            iteration: task.context.iteration,
            generated_code: task.current_code.clone().unwrap_or_default(),
            rendered_screenshot: task.last_screenshot.clone(),
            similarity: task.agent.current_similarity,
            verifier_feedback: task.last_verification.as_ref().map(|v| v.feedback.clone()),
            code_diff: Some(CodeDiff::from_codes(
                task.context.best_code.as_deref(),
                task.current_code.as_deref().unwrap_or(""),
            )),
            duration_ms: task.start_time.elapsed().as_millis() as u64,
        };
        
        task.context.add_iteration(history_entry);
        
        // Clear screenshot for next iteration
        task.last_screenshot = None;
        
        // Move to next state (Generator will check if should continue)
        task.agent.next_state();
    }
    
    /// Finalize successful task
    fn finalize_success(&mut self) {
        if let Some(task) = self.current.take() {
            let result = VigaResult {
                success: true,
                code: task.context.best_code,
                similarity: task.context.best_similarity,
                iterations: task.context.iteration,
                duration_ms: task.start_time.elapsed().as_millis() as u64,
                error: None,
            };
            
            info!(
                "🎨 VIGA: Completed successfully - {:.1}% similarity in {} iterations",
                result.similarity * 100.0,
                result.iterations
            );
            
            self.results.push_back(result);
            self.status = VigaStatus::Completed;
        }
    }
    
    /// Finalize failed task
    fn finalize_failure(&mut self) {
        if let Some(task) = self.current.take() {
            let result = VigaResult {
                success: false,
                code: task.context.best_code,
                similarity: task.context.best_similarity,
                iterations: task.context.iteration,
                duration_ms: task.start_time.elapsed().as_millis() as u64,
                error: task.agent.last_error,
            };
            
            error!("🎨 VIGA: Failed - {:?}", result.error);
            
            self.results.push_back(result);
            self.status = VigaStatus::Failed;
        }
    }
}

/// Call Claude API with vision capability
fn call_claude_vision(
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    reference_image: Option<&str>,
    rendered_image: Option<&str>,
) -> Result<String, String> {
    use std::io::Read;
    
    // Build message content with images
    let mut content = Vec::new();
    
    // Add reference image if provided
    if let Some(ref_img) = reference_image {
        let (media_type, data) = parse_data_url(ref_img)?;
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        }));
    }
    
    // Add rendered image if provided
    if let Some(ren_img) = rendered_image {
        let (media_type, data) = parse_data_url(ren_img)?;
        content.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        }));
    }
    
    // Add text prompt
    content.push(serde_json::json!({
        "type": "text",
        "text": user_prompt
    }));
    
    // Build request body
    let request_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 8192,
        "system": system_prompt,
        "messages": [{
            "role": "user",
            "content": content
        }]
    });
    
    // Make HTTP request using ureq (blocking)
    let response = ureq::post("https://api.anthropic.com/v1/messages")
        .set("Content-Type", "application/json")
        .set("x-api-key", api_key)
        .set("anthropic-version", "2023-06-01")
        .send_json(&request_body)
        .map_err(|e| format!("HTTP request failed: {}", e))?;
    
    // Read response
    let mut body = String::new();
    response.into_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("Failed to read response: {}", e))?;
    
    // Parse response
    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    // Extract text content
    if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    return Ok(text.to_string());
                }
            }
        }
    }
    
    // Check for error
    if let Some(error) = json.get("error") {
        return Err(format!("API error: {}", error));
    }
    
    Err("No text content in response".to_string())
}

/// Parse data URL into media type and base64 data
fn parse_data_url(data_url: &str) -> Result<(String, String), String> {
    // Format: data:image/png;base64,<data>
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 {
        return Err("Invalid data URL format".to_string());
    }
    
    let header = parts[0];
    let data = parts[1].to_string();
    
    // Extract media type
    let media_type = if header.contains("image/png") {
        "image/png"
    } else if header.contains("image/jpeg") || header.contains("image/jpg") {
        "image/jpeg"
    } else if header.contains("image/gif") {
        "image/gif"
    } else if header.contains("image/webp") {
        "image/webp"
    } else {
        "image/png" // Default
    };
    
    Ok((media_type.to_string(), data))
}

/// VIGA Pipeline Plugin
pub struct VigaPipelinePlugin;

impl Plugin for VigaPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VigaPipeline>()
            .add_message::<VigaRequestEvent>()
            .add_message::<VigaCodeReadyEvent>()
            .add_message::<VigaCompleteEvent>()
            .add_systems(Update, (
                process_viga_requests,
                tick_viga_pipeline,
                handle_viga_code_ready,
            ).chain());
        
        info!("🎨 VIGA Pipeline Plugin initialized");
    }
}

/// Event to request VIGA processing
#[derive(Event, Message, Clone)]
pub struct VigaRequestEvent {
    /// Reference image (base64 data URL)
    pub reference_image: String,
    /// Optional description
    pub description: Option<String>,
}

/// Event when VIGA has code ready for execution
#[derive(Event, Message, Clone)]
pub struct VigaCodeReadyEvent {
    /// Generated Rune code
    pub code: String,
}

/// Event when VIGA completes
#[derive(Event, Message, Clone)]
pub struct VigaCompleteEvent {
    /// Result
    pub result: VigaResult,
}

/// Process incoming VIGA requests
fn process_viga_requests(
    mut events: MessageReader<VigaRequestEvent>,
    mut pipeline: ResMut<VigaPipeline>,
) {
    for event in events.read() {
        let request = VigaRequest {
            reference_image: event.reference_image.clone(),
            description: event.description.clone(),
            max_iterations: 10,
            target_similarity: 0.90,
        };
        pipeline.queue_request(request);
    }
}

/// Tick the VIGA pipeline
fn tick_viga_pipeline(
    mut pipeline: ResMut<VigaPipeline>,
    mut code_events: MessageWriter<VigaCodeReadyEvent>,
    mut complete_events: MessageWriter<VigaCompleteEvent>,
    soul_settings: Option<Res<crate::soul::SoulServiceSettings>>,
    global_settings: Option<Res<crate::soul::GlobalSoulSettings>>,
) {
    // Get API key
    let api_key = match (&soul_settings, &global_settings) {
        (Some(soul), Some(global)) => {
            let key = soul.effective_api_key(global);
            if key.is_empty() { None } else { Some(key) }
        }
        _ => None,
    };
    
    // Tick pipeline
    if let Some(code) = pipeline.tick(api_key.as_deref()) {
        code_events.write(VigaCodeReadyEvent { code });
    }
    
    // Check for completed results
    while let Some(result) = pipeline.poll_result() {
        complete_events.write(VigaCompleteEvent { result });
    }
}

/// Handle code ready events (execute via Soul pipeline)
fn handle_viga_code_ready(
    mut events: MessageReader<VigaCodeReadyEvent>,
    mut pipeline: ResMut<VigaPipeline>,
) {
    for event in events.read() {
        info!("🎨 VIGA: Code ready for execution ({} chars)", event.code.len());
        // Mark as executed - in real implementation, this would trigger
        // the Soul build pipeline and wait for execution
        pipeline.mark_code_executed();
    }
}
