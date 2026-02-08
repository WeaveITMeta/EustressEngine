//! # AI Generation Service
//!
//! Orchestrates AI asset generation and integrates with the Asset System.
//!
//! ## Table of Contents
//!
//! 1. **AIGenerationService** - Main service resource
//! 2. **GenerationRequest** - Request to generate an asset
//! 3. **GenerationResult** - Result of generation
//! 4. **AIBackend** - Pluggable AI backends (Ollama, ComfyUI, etc.)
//!
//! ## Supported Backends
//!
//! - **Ollama** - Local LLM for text/prompts
//! - **ComfyUI** - Local Stable Diffusion for textures
//! - **Meshy** - Cloud 3D mesh generation
//! - **Tripo3D** - Cloud 3D mesh generation
//! - **Custom** - HTTP endpoint for custom AI servers
//!
//! ## Usage
//!
//! ```rust
//! // Request generation
//! generation_service.request_generation(GenerationRequest {
//!     entity_id: 42,
//!     prompt: "ancient stone pillar with glowing runes".to_string(),
//!     category: NodeCategory::Structure,
//!     detail_level: DetailLevel::High,
//! });
//!
//! // Poll for results
//! if let Some(result) = generation_service.poll_result(42) {
//!     entity.generated_mesh_id = Some(result.mesh_id.to_base58());
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::{info, warn};

#[allow(unused_imports)]
use crate::scene::{DetailLevel, NodeCategory, GenerationStatus};
#[allow(unused_imports)]
use crate::assets::{ContentHash, AssetService};

// ============================================================================
// AI Generation Service
// ============================================================================

/// AI Generation Service - orchestrates AI asset generation
#[derive(Resource)]
pub struct AIGenerationService {
    /// AI backend configuration
    pub config: AIGenerationConfig,
    
    /// Pending generation requests
    pending: Arc<RwLock<HashMap<u32, GenerationRequest>>>,
    
    /// Completed results (entity_id -> result)
    results: Arc<RwLock<HashMap<u32, GenerationResult>>>,
    
    /// Generation queue (priority sorted)
    queue: Arc<RwLock<Vec<u32>>>,
    
    /// Currently processing entity ID
    current: Arc<RwLock<Option<u32>>>,
    
    /// Statistics
    pub stats: GenerationStats,
}

impl Default for AIGenerationService {
    fn default() -> Self {
        Self {
            config: AIGenerationConfig::default(),
            pending: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(RwLock::new(Vec::new())),
            current: Arc::new(RwLock::new(None)),
            stats: GenerationStats::default(),
        }
    }
}

impl AIGenerationService {
    /// Create with custom config
    pub fn new(config: AIGenerationConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }
    
    /// Request generation for an entity
    pub fn request_generation(&mut self, request: GenerationRequest) {
        let entity_id = request.entity_id;
        let priority = request.priority;
        
        // Add to pending
        self.pending.write().insert(entity_id, request);
        
        // Add to queue (sorted by priority)
        let mut queue = self.queue.write();
        let pos = queue.iter()
            .position(|&id| {
                self.pending.read()
                    .get(&id)
                    .map(|r| r.priority < priority)
                    .unwrap_or(true)
            })
            .unwrap_or(queue.len());
        queue.insert(pos, entity_id);
        
        self.stats.total_requested += 1;
    }
    
    /// Cancel a pending generation
    pub fn cancel(&self, entity_id: u32) {
        self.pending.write().remove(&entity_id);
        self.queue.write().retain(|&id| id != entity_id);
    }
    
    /// Get generation status for an entity
    pub fn get_status(&self, entity_id: u32) -> GenerationStatus {
        // Check if completed
        if let Some(result) = self.results.read().get(&entity_id) {
            if result.success {
                return GenerationStatus::Complete {
                    completed_at: result.completed_at,
                    generation_time_ms: result.generation_time_ms,
                };
            } else {
                return GenerationStatus::Failed {
                    error: result.error.clone().unwrap_or_default(),
                    failed_at: result.completed_at,
                };
            }
        }
        
        // Check if currently processing
        if self.current.read().as_ref() == Some(&entity_id) {
            if let Some(request) = self.pending.read().get(&entity_id) {
                return GenerationStatus::InProgress {
                    progress: request.progress,
                    stage: request.current_stage.clone(),
                };
            }
        }
        
        // Check if pending
        if self.pending.read().contains_key(&entity_id) {
            return GenerationStatus::Pending;
        }
        
        GenerationStatus::NotRequested
    }
    
    /// Poll for a completed result
    pub fn poll_result(&self, entity_id: u32) -> Option<GenerationResult> {
        self.results.write().remove(&entity_id)
    }
    
    /// Get next entity to process from queue
    pub fn pop_queue(&self) -> Option<GenerationRequest> {
        let mut queue = self.queue.write();
        if let Some(entity_id) = queue.pop() {
            *self.current.write() = Some(entity_id);
            return self.pending.read().get(&entity_id).cloned();
        }
        None
    }
    
    /// Mark generation as complete
    pub fn complete(&mut self, entity_id: u32, result: GenerationResult) {
        self.pending.write().remove(&entity_id);
        *self.current.write() = None;
        
        if result.success {
            self.stats.total_completed += 1;
            self.stats.total_generation_time_ms += result.generation_time_ms;
        } else {
            self.stats.total_failed += 1;
        }
        
        self.results.write().insert(entity_id, result);
    }
    
    /// Update progress for current generation
    pub fn update_progress(&self, entity_id: u32, progress: f32, stage: &str) {
        if let Some(request) = self.pending.write().get_mut(&entity_id) {
            request.progress = progress;
            request.current_stage = stage.to_string();
        }
    }
    
    /// Get queue length
    pub fn queue_len(&self) -> usize {
        self.queue.read().len()
    }
    
    /// Check if service is busy
    pub fn is_busy(&self) -> bool {
        self.current.read().is_some()
    }
    
    /// Get average generation time
    pub fn avg_generation_time_ms(&self) -> u64 {
        if self.stats.total_completed == 0 {
            0
        } else {
            self.stats.total_generation_time_ms / self.stats.total_completed as u64
        }
    }
}

// ============================================================================
// Generation Request
// ============================================================================

/// Request to generate an AI asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRequest {
    /// Entity ID to generate for
    pub entity_id: u32,
    
    /// Generation prompt
    pub prompt: String,
    
    /// Global scene theme (prepended to prompt)
    pub global_theme: String,
    
    /// Asset category
    pub category: NodeCategory,
    
    /// Detail level
    pub detail_level: DetailLevel,
    
    /// Priority (higher = process first)
    pub priority: u8,
    
    /// Generate LODs?
    pub generate_lods: bool,
    
    /// Generate textures?
    pub generate_textures: bool,
    
    /// Current progress (0.0 - 1.0)
    #[serde(skip)]
    pub progress: f32,
    
    /// Current stage description
    #[serde(skip)]
    pub current_stage: String,
    
    /// Request timestamp
    pub requested_at: u64,
}

impl Default for GenerationRequest {
    fn default() -> Self {
        Self {
            entity_id: 0,
            prompt: String::new(),
            global_theme: String::new(),
            category: NodeCategory::Empty,
            detail_level: DetailLevel::Medium,
            priority: 5,
            generate_lods: true,
            generate_textures: true,
            progress: 0.0,
            current_stage: String::new(),
            requested_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

impl GenerationRequest {
    /// Create a new request
    pub fn new(entity_id: u32, prompt: &str, category: NodeCategory) -> Self {
        Self {
            entity_id,
            prompt: prompt.to_string(),
            category,
            ..Default::default()
        }
    }
    
    /// Set detail level
    pub fn with_detail(mut self, level: DetailLevel) -> Self {
        self.detail_level = level;
        self
    }
    
    /// Set priority
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
    
    /// Set global theme
    pub fn with_theme(mut self, theme: &str) -> Self {
        self.global_theme = theme.to_string();
        self
    }
    
    /// Build full prompt (theme + entity prompt)
    pub fn full_prompt(&self) -> String {
        if self.global_theme.is_empty() {
            self.prompt.clone()
        } else {
            format!("{}, {}", self.global_theme, self.prompt)
        }
    }
}

// ============================================================================
// Generation Result
// ============================================================================

/// Result of AI asset generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationResult {
    /// Entity ID
    pub entity_id: u32,
    
    /// Success?
    pub success: bool,
    
    /// Error message (if failed)
    pub error: Option<String>,
    
    /// Generated mesh asset ID
    pub mesh_id: Option<ContentHash>,
    
    /// Generated texture asset ID
    pub texture_id: Option<ContentHash>,
    
    /// Generated LOD asset IDs (index 0 = highest quality)
    pub lod_ids: Vec<ContentHash>,
    
    /// Completion timestamp
    pub completed_at: u64,
    
    /// Generation time in milliseconds
    pub generation_time_ms: u64,
    
    /// Backend used
    pub backend: String,
    
    /// Model/checkpoint used
    pub model: String,
}

impl GenerationResult {
    /// Create a success result
    pub fn success(entity_id: u32, mesh_id: ContentHash, generation_time_ms: u64) -> Self {
        Self {
            entity_id,
            success: true,
            error: None,
            mesh_id: Some(mesh_id),
            texture_id: None,
            lod_ids: Vec::new(),
            completed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            generation_time_ms,
            backend: String::new(),
            model: String::new(),
        }
    }
    
    /// Create a failure result
    pub fn failure(entity_id: u32, error: &str) -> Self {
        Self {
            entity_id,
            success: false,
            error: Some(error.to_string()),
            mesh_id: None,
            texture_id: None,
            lod_ids: Vec::new(),
            completed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            generation_time_ms: 0,
            backend: String::new(),
            model: String::new(),
        }
    }
    
    /// Add texture
    pub fn with_texture(mut self, texture_id: ContentHash) -> Self {
        self.texture_id = Some(texture_id);
        self
    }
    
    /// Add LODs
    pub fn with_lods(mut self, lods: Vec<ContentHash>) -> Self {
        self.lod_ids = lods;
        self
    }
    
    /// Set backend info
    pub fn with_backend(mut self, backend: &str, model: &str) -> Self {
        self.backend = backend.to_string();
        self.model = model.to_string();
        self
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// AI Generation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIGenerationConfig {
    /// Default AI backend
    pub default_backend: AIBackend,
    
    /// Backend for mesh generation
    pub mesh_backend: AIBackend,
    
    /// Backend for texture generation
    pub texture_backend: AIBackend,
    
    /// Backend for text/narrative generation
    pub text_backend: AIBackend,
    
    /// Max concurrent generations
    pub max_concurrent: usize,
    
    /// Generation timeout (seconds)
    pub timeout_secs: u64,
    
    /// Auto-generate LODs
    pub auto_lods: bool,
    
    /// LOD reduction factors [0.5, 0.25, 0.1]
    pub lod_factors: Vec<f32>,
    
    /// Cache generated assets
    pub cache_results: bool,
}

impl Default for AIGenerationConfig {
    fn default() -> Self {
        Self {
            default_backend: AIBackend::default(),
            mesh_backend: AIBackend::Meshy { api_key: None },
            texture_backend: AIBackend::ComfyUI { 
                endpoint: "http://127.0.0.1:8188".to_string(),
                workflow: "txt2img".to_string(),
            },
            text_backend: AIBackend::Ollama {
                endpoint: "http://127.0.0.1:11434".to_string(),
                model: "llama3.2".to_string(),
            },
            max_concurrent: 1,
            timeout_secs: 120,
            auto_lods: true,
            lod_factors: vec![0.5, 0.25, 0.1],
            cache_results: true,
        }
    }
}

/// AI Backend types
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AIBackend {
    /// No AI backend (manual only)
    #[default]
    None,
    
    /// Ollama - local LLM server
    Ollama {
        endpoint: String,
        model: String,
    },
    
    /// ComfyUI - local Stable Diffusion
    ComfyUI {
        endpoint: String,
        workflow: String,
    },
    
    /// Meshy - cloud 3D mesh generation
    Meshy {
        api_key: Option<String>,
    },
    
    /// Tripo3D - cloud 3D mesh generation
    Tripo3D {
        api_key: Option<String>,
    },
    
    /// OpenAI - GPT for text
    OpenAI {
        api_key: String,
        model: String,
    },
    
    /// Custom HTTP endpoint
    Custom {
        endpoint: String,
        headers: HashMap<String, String>,
    },
}

impl AIBackend {
    /// Get endpoint URL
    pub fn endpoint(&self) -> Option<&str> {
        match self {
            Self::Ollama { endpoint, .. } => Some(endpoint),
            Self::ComfyUI { endpoint, .. } => Some(endpoint),
            Self::Custom { endpoint, .. } => Some(endpoint),
            _ => None,
        }
    }
    
    /// Check if backend is available (has required config)
    pub fn is_available(&self) -> bool {
        match self {
            Self::None => false,
            Self::Ollama { endpoint, .. } => !endpoint.is_empty(),
            Self::ComfyUI { endpoint, .. } => !endpoint.is_empty(),
            Self::Meshy { api_key } => api_key.is_some(),
            Self::Tripo3D { api_key } => api_key.is_some(),
            Self::OpenAI { api_key, .. } => !api_key.is_empty(),
            Self::Custom { endpoint, .. } => !endpoint.is_empty(),
        }
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Generation statistics
#[derive(Debug, Clone, Default)]
pub struct GenerationStats {
    pub total_requested: usize,
    pub total_completed: usize,
    pub total_failed: usize,
    pub total_generation_time_ms: u64,
}

// ============================================================================
// Events
// ============================================================================

/// Event to request AI generation
#[derive(Message, Debug, Clone)]
pub struct RequestGenerationEvent {
    pub request: GenerationRequest,
}

/// Event when generation completes
#[derive(Message, Debug, Clone)]
pub struct GenerationCompleteEvent {
    pub result: GenerationResult,
}

/// Event when generation fails
#[derive(Message, Debug, Clone)]
pub struct GenerationFailedEvent {
    pub entity_id: u32,
    pub error: String,
}

/// Event to cancel generation
#[derive(Message, Debug, Clone)]
pub struct CancelGenerationEvent {
    pub entity_id: u32,
}

// ============================================================================
// Plugin
// ============================================================================

/// AI Generation Plugin
pub struct AIGenerationPlugin {
    pub config: AIGenerationConfig,
}

impl Default for AIGenerationPlugin {
    fn default() -> Self {
        Self {
            config: AIGenerationConfig::default(),
        }
    }
}

impl Plugin for AIGenerationPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(AIGenerationService::new(self.config.clone()))
            .add_message::<RequestGenerationEvent>()
            .add_message::<GenerationCompleteEvent>()
            .add_message::<GenerationFailedEvent>()
            .add_message::<CancelGenerationEvent>()
            .add_systems(Update, (
                handle_generation_requests,
                process_generation_queue,
                handle_cancel_requests,
            ));
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Handle incoming generation requests
fn handle_generation_requests(
    mut events: MessageReader<RequestGenerationEvent>,
    mut service: ResMut<AIGenerationService>,
) {
    for event in events.read() {
        info!("🤖 Generation requested for entity {}: {}", 
            event.request.entity_id, 
            event.request.prompt
        );
        service.request_generation(event.request.clone());
    }
}

/// Process the generation queue
fn process_generation_queue(
    service: Res<AIGenerationService>,
    mut complete_events: MessageWriter<GenerationCompleteEvent>,
    mut failed_events: MessageWriter<GenerationFailedEvent>,
) {
    // Check for completed results and emit events
    let results: Vec<_> = service.results.write().drain().collect();
    for (entity_id, result) in results {
        if result.success {
            info!("✅ Generation complete for entity {}", entity_id);
            complete_events.write(GenerationCompleteEvent { result });
        } else {
            warn!("❌ Generation failed for entity {}: {:?}", entity_id, result.error);
            failed_events.write(GenerationFailedEvent {
                entity_id,
                error: result.error.unwrap_or_default(),
            });
        }
    }
    
    // Note: Actual AI calls happen in async tasks spawned by the backend
    // This system just manages the queue and emits events
}

/// Handle cancel requests
fn handle_cancel_requests(
    mut events: MessageReader<CancelGenerationEvent>,
    service: Res<AIGenerationService>,
) {
    for event in events.read() {
        info!("🚫 Cancelling generation for entity {}", event.entity_id);
        service.cancel(event.entity_id);
    }
}
