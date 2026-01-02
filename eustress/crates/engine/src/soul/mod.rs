//! # Soul Engine Integration
//!
//! Build pipeline and Rune scripting for Soul natural language scripts.
//!
//! ## Active Components
//!
//! - **Build Pipeline**: Claude generates Rune code from markdown
//! - **Claude Client**: HTTP client for Anthropic API
//! - **Rune API**: Rune script execution and validation
//! - **Scope**: Script location and system prompt building
//!
//! ## Stub Components (legacy, kept for compatibility)
//!
//! - builder, cache, codegen, validator, hot_compile, soul_context

pub mod builder;
pub mod cache;
pub mod codegen;
pub mod validator;
pub mod claude_client;
pub mod hot_compile;
pub mod build_pipeline;
pub mod scope;
pub mod soul_context;
pub mod rune_api;
pub mod error_tracker;

pub use builder::*;
pub use cache::*;
pub use codegen::*;
pub use validator::*;
pub use claude_client::{ClaudeClient, ClaudeError, GenerationResult};
pub use hot_compile::{HotCompiler, HotCompileConfig, CompileResult, HotCompilePlugin};
pub use build_pipeline::{SoulBuildPipeline, BuildStage, BuildRequest, PipelineResult, SoulBuildPipelinePlugin, TriggerBuildEvent, CommandBarBuildEvent, CommandBarBuildState, CommandBarResult};
pub use scope::{ScriptLocation, ScriptScope, AvailableEvents, SystemPromptBuilder};
pub use soul_context::{SoulContext, EntityHandle, Value, Shape, LightType, Easing, CommandScript, SoulServiceScript, SoulEntityScript, SoulWorkspaceScript};
pub use rune_api::{RuneScriptEngine, ScriptCommand, SpawnPhysics, execute_rune_script, validate_rune_script, EntityData, InputData, PhysicsData, update_world_state, update_input_state, update_mouse_raycast};
pub use error_tracker::{RuneErrorTracker, ErrorCategory, TrackedError, SuggestedAction, ErrorStats};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
#[allow(unused_imports)]
use eustress_common::soul::{SoulConfig, ScriptRegistry, SoulPlugin as CommonSoulPlugin, SoulAST};

// ============================================================================
// Global Soul Settings (Persisted to ~/.eustress_engine/soul_settings.json)
// ============================================================================

/// Global Soul settings that persist across sessions and spaces
/// Stored in ~/.eustress_engine/soul_settings.json
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSoulSettings {
    /// Global Claude API key (used as default for new spaces)
    pub global_api_key: String,
    /// Whether the global API key has been validated
    #[serde(skip)]
    pub api_key_valid: Option<bool>,
    /// Auto-fill new spaces with global API key
    pub use_global_for_new_spaces: bool,
}

impl Default for GlobalSoulSettings {
    fn default() -> Self {
        Self {
            global_api_key: String::new(),
            api_key_valid: None,
            use_global_for_new_spaces: true,
        }
    }
}

impl GlobalSoulSettings {
    /// Get the settings file path (~/.eustress_engine/soul_settings.json)
    pub fn settings_path() -> Option<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            let settings_dir = home.join(".eustress_engine");
            Some(settings_dir.join("soul_settings.json"))
        } else {
            None
        }
    }
    
    /// Load settings from file or create default
    pub fn load() -> Self {
        if let Some(path) = Self::settings_path() {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<GlobalSoulSettings>(&content) {
                            Ok(settings) => {
                                info!("✅ Loaded global Soul settings from {:?}", path);
                                return settings;
                            }
                            Err(e) => {
                                warn!("⚠ Failed to parse Soul settings file: {}. Using defaults.", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("⚠ Failed to read Soul settings file: {}. Using defaults.", e);
                    }
                }
            } else {
                info!("ℹ No Soul settings file found. Creating default settings.");
            }
        } else {
            warn!("⚠ Could not determine home directory. Using default Soul settings.");
        }
        
        Self::default()
    }
    
    /// Save settings to file
    pub fn save(&self) -> Result<(), String> {
        let path = Self::settings_path()
            .ok_or_else(|| "Could not determine home directory".to_string())?;
        
        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create settings directory: {}", e))?;
        }
        
        // Serialize settings to JSON with pretty formatting
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize Soul settings: {}", e))?;
        
        // Write to file
        fs::write(&path, json)
            .map_err(|e| format!("Failed to write Soul settings file: {}", e))?;
        
        info!("✅ Saved global Soul settings to {:?}", path);
        Ok(())
    }
    
    /// Check if the global API key is set
    pub fn has_api_key(&self) -> bool {
        !self.global_api_key.trim().is_empty()
    }
}

// ============================================================================
// Per-Space Soul Service Settings (UI-editable, saved with scene)
// ============================================================================

/// Default telemetry to enabled (opt-out)
fn default_telemetry_enabled() -> bool {
    true
}

/// Resource for SoulService settings editable from the Properties panel
/// This is per-space and saved with the scene file
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct SoulServiceSettings {
    /// Per-space Claude API key (overrides global if set)
    pub claude_api_key: String,
    /// Whether the API key is valid (set after validation)
    #[serde(skip)]
    pub api_key_valid: Option<bool>,
    /// Whether this space uses the global API key (true = use global, false = use per-space)
    pub use_global_key: bool,
    /// Whether to send anonymous error reports to help improve Eustress (opt-out, default on)
    #[serde(default = "default_telemetry_enabled")]
    pub telemetry_enabled: bool,
}

impl SoulServiceSettings {
    /// Get the effective API key (per-space or global fallback)
    /// Always trims whitespace/newlines to prevent HTTP header errors
    pub fn effective_api_key(&self, global: &GlobalSoulSettings) -> String {
        if self.use_global_key || self.claude_api_key.trim().is_empty() {
            global.global_api_key.trim().to_string()
        } else {
            self.claude_api_key.trim().to_string()
        }
    }
    
    /// Check if this space has a custom API key set
    pub fn has_custom_key(&self) -> bool {
        !self.use_global_key && !self.claude_api_key.trim().is_empty()
    }
    
    /// Initialize from global settings (for new spaces)
    pub fn from_global(global: &GlobalSoulSettings) -> Self {
        if global.use_global_for_new_spaces && global.has_api_key() {
            Self {
                claude_api_key: String::new(), // Don't copy the key, just reference global
                api_key_valid: None,
                use_global_key: true,
                telemetry_enabled: true, // Opt-out, default on
            }
        } else {
            Self::default()
        }
    }
}

// ============================================================================
// Soul Script Data Component
// ============================================================================

/// Component attached to SoulScript entities containing the script source and compiled state
#[derive(Component, Debug, Clone, Default, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct SoulScriptData {
    /// The markdown source code
    pub source: String,
    /// Whether the script has been modified since last save
    pub dirty: bool,
    /// Compiled AST (if successfully parsed)
    #[serde(skip)]
    #[reflect(ignore)]
    pub ast: Option<SoulAST>,
    /// Generated Rust code (if successfully compiled)
    pub generated_code: Option<String>,
    /// Build status
    pub build_status: SoulBuildStatus,
    /// Errors from last build attempt
    pub errors: Vec<String>,
}

/// Build status for a Soul script
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum SoulBuildStatus {
    #[default]
    NotBuilt,
    Building,
    Built,
    Failed,
    Stale,
}

// ============================================================================
// Engine Soul Plugin
// ============================================================================

/// Soul scripting plugin for Eustress Engine (Studio)
pub struct EngineSoulPlugin;

impl Plugin for EngineSoulPlugin {
    fn build(&self, app: &mut App) {
        // Add common Soul plugin first
        app.add_plugins(CommonSoulPlugin);
        
        // Add hot compile plugin
        app.add_plugins(HotCompilePlugin);
        
        // Add build pipeline plugin (includes Claude integration)
        app.add_plugins(SoulBuildPipelinePlugin);
        
        // Load global Soul settings from disk
        let global_settings = GlobalSoulSettings::load();
        
        // Initialize space settings from global (respects "use global for new spaces" setting)
        let space_settings = SoulServiceSettings::from_global(&global_settings);
        
        // Add engine-specific resources
        app
            .init_resource::<SoulBuilder>()
            .init_resource::<SoulCache>()
            .insert_resource(global_settings)
            .insert_resource(space_settings)
            .add_message::<BuildRequestEvent>()
            .add_message::<BuildCompleteEvent>()
            .add_systems(Update, (
                process_build_requests,
                update_build_status,
                cleanup_removed_soulscripts,
            ));
        
        info!("EngineSoulPlugin initialized - Claude API + Hot Compile ready");
    }
}


// ============================================================================
// Events
// ============================================================================

/// Request to build a script
#[derive(Event, Message, Debug, Clone)]
pub struct BuildRequestEvent {
    /// Scene name
    pub scene: String,
    /// Script path (optional - None = all scripts in scene)
    pub script: Option<String>,
    /// Force rebuild (ignore cache)
    pub force: bool,
}

/// Build completed
#[derive(Event, Message, Debug, Clone)]
pub struct BuildCompleteEvent {
    /// Scene name
    pub scene: String,
    /// Script path
    pub script: String,
    /// Success or failure
    pub success: bool,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Build duration (ms)
    pub duration_ms: u64,
}

// ============================================================================
// Systems
// ============================================================================

/// Process build requests
fn process_build_requests(
    mut events: MessageReader<BuildRequestEvent>,
    mut builder: ResMut<SoulBuilder>,
    _config: Res<SoulConfig>,
) {
    for event in events.read() {
        info!("Build request: scene={}, force={}", event.scene, event.force);
        
        builder.queue_build(BuildJob {
            scene: event.scene.clone(),
            script: event.script.clone(),
            force: event.force,
            status: BuildJobStatus::Pending,
            started_at: None,
        });
    }
}

/// Update build status
fn update_build_status(
    mut builder: ResMut<SoulBuilder>,
    mut complete_events: MessageWriter<BuildCompleteEvent>,
    _config: Res<SoulConfig>,
) {
    // Process pending builds
    if let Some(result) = builder.poll_result() {
        complete_events.write(BuildCompleteEvent {
            scene: result.scene,
            script: result.script,
            success: result.success,
            error: result.error,
            duration_ms: result.duration_ms,
        });
    }
}

/// Clean up compiled modules when SoulScript entities are removed
fn cleanup_removed_soulscripts(
    mut removed: RemovedComponents<SoulScriptData>,
    mut compiler: ResMut<HotCompiler>,
    names_query: Query<&Name>,
) {
    for entity in removed.read() {
        // Try to get the name of the removed entity for the module name
        // Note: The entity is already despawned, so we can't query it directly
        // We use a naming convention: module name = "soul_<entity_id>"
        let module_name = format!("soul_{}", entity.index());
        
        // Check if this module exists and clean it up
        if compiler.has_module(&module_name) {
            compiler.cleanup_module(&module_name);
        }
        
        info!("🗑️ SoulScript entity {:?} removed, cleaned up module", entity);
    }
}
