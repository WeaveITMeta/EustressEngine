//! # Soul Client Runtime
//!
//! Runtime execution of Soul scripts on the client.
//! Handles hot reload and script lifecycle.

pub mod runtime;
pub mod hot_reload;

pub use runtime::*;
pub use hot_reload::*;

use bevy::prelude::*;
use eustress_common::soul::{SoulConfig, ScriptRegistry, SoulPlugin as CommonSoulPlugin};

// ============================================================================
// Client Soul Plugin
// ============================================================================

/// Soul scripting plugin for Eustress Client
pub struct ClientSoulPlugin;

impl Plugin for ClientSoulPlugin {
    fn build(&self, app: &mut App) {
        // Add common Soul plugin first
        app.add_plugins(CommonSoulPlugin);
        
        // Add client-specific resources
        app
            .init_resource::<SoulRuntime>()
            .init_resource::<HotReloadWatcher>()
            .add_event::<ScriptExecuteEvent>()
            .add_event::<ScriptReloadEvent>()
            .add_systems(Update, (
                execute_scripts,
                watch_hot_reload,
            ));
        
        info!("ClientSoulPlugin initialized - Runtime ready");
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event: Execute a script
#[derive(Event, Debug, Clone)]
pub struct ScriptExecuteEvent {
    /// Script ID
    pub script_id: String,
    /// Execution context
    pub context: ExecutionContext,
}

/// Event: Script reloaded
#[derive(Event, Debug, Clone)]
pub struct ScriptReloadEvent {
    /// Script ID
    pub script_id: String,
    /// Was reload successful?
    pub success: bool,
}

/// Execution context
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    /// Triggering entity (if any)
    pub trigger_entity: Option<Entity>,
    /// Event name
    pub event_name: Option<String>,
    /// Custom data
    pub data: std::collections::HashMap<String, String>,
}

// ============================================================================
// Systems
// ============================================================================

/// Execute scripts
fn execute_scripts(
    mut events: EventReader<ScriptExecuteEvent>,
    mut runtime: ResMut<SoulRuntime>,
    registry: Res<ScriptRegistry>,
) {
    for event in events.read() {
        if let Some(script) = registry.get(&event.script_id) {
            runtime.execute(&event.script_id, &event.context);
        }
    }
}

/// Watch for hot reload
fn watch_hot_reload(
    mut watcher: ResMut<HotReloadWatcher>,
    mut reload_events: EventWriter<ScriptReloadEvent>,
    config: Res<SoulConfig>,
) {
    if !config.hot_reload {
        return;
    }
    
    // Check for changed files
    for changed in watcher.poll_changes() {
        info!("Hot reloading script: {}", changed);
        
        // TODO: Trigger actual reload
        reload_events.send(ScriptReloadEvent {
            script_id: changed,
            success: true,
        });
    }
}
