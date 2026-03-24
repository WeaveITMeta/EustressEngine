//! # Luau Scripting Runtime
//!
//! Roblox Luau scripting integration for Eustress Engine.
//! Provides Script, LocalScript, ModuleScript execution and
//! RemoteEvent/RemoteFunction/BindableEvent/BindableFunction networking primitives.
//!
//! ## Table of Contents
//!
//! 1. **Components** — ECS components for script types and event/function instances
//! 2. **Runtime** — mlua-based Luau virtual machine with sandboxing
//! 3. **Bridge** — Client-server communication bridge for Remote* objects
//! 4. **Compat** — Roblox Luau API compatibility shims for porting scripts
//! 5. **Plugin** — Bevy plugin wiring everything together
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │  .luau File │────▶│  mlua VM    │────▶│  Bevy ECS   │
//! │  (Source)   │     │  (Sandbox)  │     │  (Systems)  │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!       │                    │                   │
//!       │ Luau source       │ mlua 0.10         │ Bevy 0.18
//!       │ (.luau/.lua)      │ + Luau backend    │ + Avian3D
//!       ▼                    ▼                   ▼
//! ┌─────────────────────────────────────────────────────┐
//! │              Client-Server Bridge                    │
//! │  RemoteEvent / RemoteFunction (QUIC replication)    │
//! │  BindableEvent / BindableFunction (local in-process)│
//! └─────────────────────────────────────────────────────┘
//! ```

pub mod components;
pub mod runtime;
pub mod bridge;
pub mod compat;
pub mod raycast;
pub mod types;

pub use components::*;
pub use runtime::*;
pub use bridge::*;
pub use compat::*;
pub use raycast::*;
pub use types::*;

use bevy::prelude::*;
use tracing::{info, error};

// ============================================================================
// Luau Plugin
// ============================================================================

/// Luau scripting plugin for Bevy
/// Initializes the Luau VM, registers components, and sets up execution systems.
pub struct LuauPlugin;

impl Plugin for LuauPlugin {
    fn build(&self, app: &mut App) {
        // Register script components
        app
            .register_type::<LuauScript>()
            .register_type::<LuauLocalScript>()
            .register_type::<LuauModuleScript>()
            .register_type::<RemoteEvent>()
            .register_type::<RemoteFunction>()
            .register_type::<BindableEvent>()
            .register_type::<BindableFunction>();

        // Initialize resources
        app
            .init_resource::<LuauRuntimeState>()
            .init_resource::<ScriptExecutionQueue>()
            .init_resource::<RemoteEventBus>()
            .init_resource::<BindableEventBus>();

        // Register messages (Bevy 0.18 uses Message, not Event)
        app
            .add_message::<LuauScriptLoadEvent>()
            .add_message::<LuauScriptErrorEvent>()
            .add_message::<RemoteEventFired>()
            .add_message::<RemoteFunctionInvoked>()
            .add_message::<BindableEventFired>()
            .add_message::<BindableFunctionInvoked>();

        // Add systems
        app.add_systems(Update, (
            initialize_luau_runtime,
            process_script_execution_queue,
            process_remote_events,
            process_bindable_events,
            hot_reload_luau_scripts,
        ));

        info!("LuauPlugin initialized — Roblox Luau scripting ready");
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Initialize the Luau runtime on first run (idempotent)
fn initialize_luau_runtime(
    mut state: ResMut<LuauRuntimeState>,
) {
    if state.initialized {
        return;
    }

    match LuauRuntime::new() {
        Ok(runtime) => {
            state.runtime = Some(runtime);
            state.initialized = true;
            info!("Luau runtime initialized successfully");
        }
        Err(error) => {
            error!("Failed to initialize Luau runtime: {}", error);
        }
    }
}

/// Process queued script executions each frame
fn process_script_execution_queue(
    mut state: ResMut<LuauRuntimeState>,
    mut queue: ResMut<ScriptExecutionQueue>,
    mut error_events: MessageWriter<LuauScriptErrorEvent>,
) {
    let Some(runtime) = state.runtime.as_mut() else { return };

    // Process up to 16 queued executions per frame to avoid stalling
    let count = queue.pending.len().min(16);
    let batch: Vec<_> = queue.pending.drain(..count).collect();
    for request in batch {
        if let Err(err) = runtime.execute_chunk(&request.source, &request.script_name) {
            error_events.write(LuauScriptErrorEvent {
                script_name: request.script_name.clone(),
                error: err.to_string(),
                line: None,
            });
        }
    }
}

/// Route RemoteEvent fires through the bridge
fn process_remote_events(
    mut bus: ResMut<RemoteEventBus>,
    mut fired_events: MessageWriter<RemoteEventFired>,
) {
    let pending: Vec<_> = bus.pending.drain(..).collect();
    for event in pending {
        fired_events.write(event);
    }
}

/// Route BindableEvent fires in-process
fn process_bindable_events(
    mut bus: ResMut<BindableEventBus>,
    mut fired_events: MessageWriter<BindableEventFired>,
) {
    let pending: Vec<_> = bus.pending.drain(..).collect();
    for event in pending {
        fired_events.write(event);
    }
}

/// Hot-reload Luau scripts when source files change
fn hot_reload_luau_scripts(
    _state: Res<LuauRuntimeState>,
) {
    // TODO: Watch .luau/.lua files for changes and re-execute
    // Integration with notify crate for filesystem watching
}
