//! # Soul - Natural Language Scripting Layer
//!
//! English-to-Rust compiler for per-scene, prose-powered scripting.
//! Files live as `.md` (e.g., `level1.md`), keeping it lightweight and editor-agnostic.
//!
//! ## Table of Contents
//!
//! 1. **ScriptService** - Service containers (Workspace, ServerScriptService, etc.)
//! 2. **SoulAST** - Parsed representation of .md scripts
//! 3. **Units** - Universal unit conversion (feet, meters, studs, etc.)
//! 4. **ScriptType** - Meta Engine Code vs Plausible Edits classification
//! 5. **BuildStatus** - Pipeline status updates
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   .md File  │────▶│  Soul AST   │────▶│  Rust Code  │
//! │   (Prose)   │     │  (Parser)   │     │  (Bevy ECS) │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!       │                    │                   │
//!       │ Markdown          │ Claude Opus 4.5   │ rustc
//!       │ + YAML            │ Code Generation   │ + Miri
//!       ▼                    ▼                   ▼
//! ```
//!
//! ## Script Services (Roblox-inspired)
//!
//! | Service | Description | Access |
//! |---------|-------------|--------|
//! | Workspace | World simulation, spatial queries | Server + Client |
//! | ServerScriptService | Server-only logic | Server only |
//! | ReplicatedStorage | Shared assets/modules | Server + Client |
//! | ReplicatedFirst | Early client loads | Client first |
//! | Lighting | Environment effects | Server + Client |
//! | Players | Player authentication | Server + Client |
//! | SoundService | Audio management | Server + Client |

pub mod ast;
pub mod context;
pub mod services;
pub mod units;
pub mod types;
pub mod parser;
pub mod config;
pub mod toon;

pub use ast::*;
pub use context::*;
pub use services::*;
pub use units::*;
pub use types::*;
pub use parser::*;
pub use config::*;
pub use toon::*;

use bevy::prelude::*;
use tracing::info;

// ============================================================================
// Soul Plugin
// ============================================================================

/// Soul scripting plugin for Bevy
pub struct SoulPlugin;

impl Plugin for SoulPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<SoulConfig>()
            .init_resource::<ScriptRegistry>()
            .add_message::<ScriptLoadEvent>()
            .add_message::<ScriptCompileEvent>()
            .add_message::<ScriptErrorEvent>()
            .add_systems(Update, (
                process_script_loads,
                hot_reload_scripts,
            ));
        
        info!("SoulPlugin initialized - Natural Language Scripting ready");
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event: Request to load a script
#[derive(Message, Debug, Clone)]
pub struct ScriptLoadEvent {
    /// Path to .md file
    pub path: String,
    /// Target service
    pub service: ScriptService,
    /// Scene name
    pub scene: String,
}

/// Event: Script compilation completed
#[derive(Message, Debug, Clone)]
pub struct ScriptCompileEvent {
    /// Script identifier
    pub script_id: String,
    /// Compilation result
    pub result: CompileResult,
}

/// Event: Script error occurred
#[derive(Message, Debug, Clone)]
pub struct ScriptErrorEvent {
    /// Script identifier
    pub script_id: String,
    /// Error message
    pub error: String,
    /// Line number (if available)
    pub line: Option<u32>,
}

/// Compilation result
#[derive(Debug, Clone)]
pub enum CompileResult {
    Success {
        /// Generated Rust code
        code: String,
        /// Output path
        output_path: String,
    },
    Failed {
        /// Error messages
        errors: Vec<String>,
    },
}

// ============================================================================
// Script Registry
// ============================================================================

/// Registry of loaded scripts
#[derive(Resource, Default)]
pub struct ScriptRegistry {
    /// Loaded scripts by ID
    scripts: std::collections::HashMap<String, LoadedScript>,
    /// Scripts by service
    by_service: std::collections::HashMap<ScriptService, Vec<String>>,
    /// Scripts by scene
    by_scene: std::collections::HashMap<String, Vec<String>>,
}

/// A loaded script
#[derive(Debug, Clone)]
pub struct LoadedScript {
    /// Unique identifier
    pub id: String,
    /// Source .md path
    pub source_path: String,
    /// Parsed AST
    pub ast: Option<SoulAST>,
    /// Target service
    pub service: ScriptService,
    /// Scene name
    pub scene: String,
    /// Script type (meta/plausible/mixed)
    pub script_type: ScriptType,
    /// Last modified time
    pub last_modified: std::time::SystemTime,
    /// Compilation status
    pub status: BuildStatus,
}

impl ScriptRegistry {
    /// Register a new script
    pub fn register(&mut self, script: LoadedScript) {
        let id = script.id.clone();
        let service = script.service;
        let scene = script.scene.clone();
        
        self.scripts.insert(id.clone(), script);
        
        self.by_service
            .entry(service)
            .or_default()
            .push(id.clone());
        
        self.by_scene
            .entry(scene)
            .or_default()
            .push(id);
    }
    
    /// Get script by ID
    pub fn get(&self, id: &str) -> Option<&LoadedScript> {
        self.scripts.get(id)
    }
    
    /// Get mutable script by ID
    pub fn get_mut(&mut self, id: &str) -> Option<&mut LoadedScript> {
        self.scripts.get_mut(id)
    }
    
    /// Get all scripts for a service
    pub fn get_by_service(&self, service: ScriptService) -> Vec<&LoadedScript> {
        self.by_service
            .get(&service)
            .map(|ids| ids.iter().filter_map(|id| self.scripts.get(id)).collect())
            .unwrap_or_default()
    }
    
    /// Get all scripts for a scene
    pub fn get_by_scene(&self, scene: &str) -> Vec<&LoadedScript> {
        self.by_scene
            .get(scene)
            .map(|ids| ids.iter().filter_map(|id| self.scripts.get(id)).collect())
            .unwrap_or_default()
    }
    
    /// Remove a script
    pub fn remove(&mut self, id: &str) -> Option<LoadedScript> {
        if let Some(script) = self.scripts.remove(id) {
            if let Some(ids) = self.by_service.get_mut(&script.service) {
                ids.retain(|i| i != id);
            }
            if let Some(ids) = self.by_scene.get_mut(&script.scene) {
                ids.retain(|i| i != id);
            }
            Some(script)
        } else {
            None
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Process script load requests
fn process_script_loads(
    mut events: MessageReader<ScriptLoadEvent>,
    mut registry: ResMut<ScriptRegistry>,
    _config: Res<SoulConfig>,
) {
    for event in events.read() {
        let script_id = format!("{}:{}", event.scene, event.path);
        
        info!("Loading script: {} for service {:?}", event.path, event.service);
        
        // Create script entry
        let script = LoadedScript {
            id: script_id.clone(),
            source_path: event.path.clone(),
            ast: None,
            service: event.service,
            scene: event.scene.clone(),
            script_type: ScriptType::Mixed,
            last_modified: std::time::SystemTime::now(),
            status: BuildStatus::Pending,
        };
        
        registry.register(script);
        
        // TODO: Trigger async parsing
    }
}

/// Hot reload scripts that have changed
fn hot_reload_scripts(
    _registry: ResMut<ScriptRegistry>,
    config: Res<SoulConfig>,
) {
    if !config.hot_reload {
        return;
    }
    
    // TODO: Check file modification times and reload changed scripts
}
