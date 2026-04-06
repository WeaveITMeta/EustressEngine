//! # Soul Client Runtime
//!
//! Full script execution on the client using shared Rune + Luau runtimes
//! from eustress_common. Mirrors engine capabilities for play mode parity.
//!
//! ## Architecture
//! - Rune scripts: compiled + executed via RuneRuntimeState (from common)
//! - Luau scripts: executed via LuauRuntime (from common)
//! - GUI bridge: GuiBridgePlugin applies script commands to GuiElementDisplay
//! - Physics bridge: Avian3D force/velocity commands from scripts

pub mod runtime;
pub mod hot_reload;

pub use runtime::*;
pub use hot_reload::*;

use bevy::prelude::*;
use avian3d::prelude::Gravity;
use eustress_common::soul::{SoulConfig, ScriptRegistry, SoulPlugin as CommonSoulPlugin};
use eustress_common::soul::rune_runtime::{
    RuneRuntimeState, RuneModuleRegistry,
    run_script_init, run_script_update, cleanup_scripts,
};

// ============================================================================
// Client Soul Plugin — full script execution
// ============================================================================

/// Soul scripting plugin for Eustress Client.
/// Provides Rune + Luau script execution with GUI and physics bridges.
pub struct ClientSoulPlugin;

impl Plugin for ClientSoulPlugin {
    fn build(&self, app: &mut App) {
        // Add common Soul plugin first (config, registry, types)
        app.add_plugins(CommonSoulPlugin);

        // GUI bridge — applies gui_set_text etc. to GuiElementDisplay components
        app.add_plugins(eustress_common::gui::GuiBridgePlugin);

        // Luau runtime (from common)
        app.add_plugins(eustress_common::luau::LuauPlugin);

        // Rune runtime resources
        app.init_resource::<RuneRuntimeState>()
            .init_resource::<RuneModuleRegistry>()
            .init_resource::<SoulRuntime>()
            .init_resource::<HotReloadWatcher>();

        // Register client Rune modules at startup
        app.add_systems(Startup, register_client_rune_modules);

        // Script execution events
        app.add_message::<ScriptExecuteEvent>()
            .add_message::<ScriptReloadEvent>();

        // Script execution systems (always running — scripts may need to run
        // outside of a formal "play mode" on the client)
        app.add_systems(Update, (
            run_script_init,
            run_script_update.after(run_script_init),
            execute_scripts,
            watch_hot_reload,
        ));

        info!("ClientSoulPlugin initialized — Rune + Luau + GUI bridge ready");
    }
}

/// Register Rune modules available to client scripts.
/// Installs the shared GUI + logging module from common.
fn register_client_rune_modules(
    mut module_registry: ResMut<RuneModuleRegistry>,
) {
    // Install shared GUI + logging module
    match eustress_common::soul::rune_gui_module::create_gui_module() {
        Ok(module) => {
            module_registry.add_module(module);
            info!("✅ Registered GUI module for client Rune runtime");
        }
        Err(e) => {
            error!("Failed to create GUI module: {}", e);
        }
    }

    // Install event bus module
    match eustress_common::events::event_bus_rune_module() {
        Ok(module) => {
            module_registry.add_module(module);
            info!("✅ Registered EventBus module for client Rune runtime");
        }
        Err(e) => {
            error!("Failed to create EventBus module: {}", e);
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event: Execute a script
#[derive(Event, Message, Debug, Clone)]
pub struct ScriptExecuteEvent {
    /// Script ID
    pub script_id: String,
    /// Execution context
    pub context: ExecutionContext,
}

/// Event: Script reloaded
#[derive(Event, Message, Debug, Clone)]
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

/// Execute scripts from event queue
fn execute_scripts(
    mut events: MessageReader<ScriptExecuteEvent>,
    mut _runtime: ResMut<SoulRuntime>,
    _registry: Res<ScriptRegistry>,
) {
    for event in events.read() {
        info!("📜 Executing script '{}' (context: {:?})", event.script_id, event.context.event_name);
        // TODO: Look up script in registry, compile if needed, execute via RuneRuntimeState
    }
}

/// Watch for hot reload
fn watch_hot_reload(
    mut watcher: ResMut<HotReloadWatcher>,
    mut reload_events: MessageWriter<ScriptReloadEvent>,
    config: Res<SoulConfig>,
) {
    if !config.hot_reload {
        return;
    }

    for changed in watcher.poll_changes() {
        info!("🔄 Hot reloading script: {}", changed);
        reload_events.write(ScriptReloadEvent {
            script_id: changed,
            success: true,
        });
    }
}

// ============================================================================
// Client Physics Bridge — Avian3D integration for scripts
// ============================================================================

/// Plugin that bridges script physics commands to Avian3D.
/// Mirrors the engine's RunePhysicsBridgePlugin.
pub struct ClientPhysicsBridgePlugin;

impl Plugin for ClientPhysicsBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            apply_client_physics_commands,
            sync_client_gravity,
        ));
    }
}

/// Apply physics commands from scripts to Avian3D entities.
/// Currently a stub — will be wired when PhysicsCommand moves to common.
fn apply_client_physics_commands() {
    // Physics commands would come from a shared thread-local in common
    // For now this is a no-op until PhysicsCommand is migrated
}

/// Sync gravity from script thread-local to Avian3D.
fn sync_client_gravity(
    mut _gravity: ResMut<Gravity>,
) {
    // Will be wired when PhysicsCommand moves to common
}
