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
use bevy::input::mouse::AccumulatedMouseMotion;
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
            // Per-frame heartbeat: advance the coroutine scheduler and fire the
            // RunService signals. Ordered AFTER queue processing so a script
            // enqueued this frame is spawned before its first Heartbeat tick.
            drive_luau_frame.after(process_script_execution_queue),
            // Pump Bevy keyboard/mouse into UserInputService (live IsKeyDown +
            // InputBegan/Ended/Changed). Runs before the frame driver so a key
            // pressed this frame is observable in the same Heartbeat tick.
            sync_luau_input.before(drive_luau_frame),
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

/// Process queued script executions each frame.
///
/// Scripts are spawned as managed scheduler coroutines (not run-to-completion)
/// so they can `task.wait`, connect to `RunService.Heartbeat`, and otherwise
/// persist across frames. Only a COMPILE error surfaces here; runtime errors
/// are reported by the in-VM scheduler to the Output log.
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
        if let Err(err) = runtime.spawn_script(&request.source, &request.script_name) {
            error_events.write(LuauScriptErrorEvent {
                script_name: request.script_name.clone(),
                error: err.to_string(),
                line: None,
            });
        }
    }
}

/// Per-frame driver: advance the coroutine scheduler (waking `task.wait` /
/// `task.delay` threads and running deferred ones) and fire the RunService
/// frame signals. This is what makes `Heartbeat:Connect` and `task.wait`
/// actually tick — without it the VM is still "run once, top-to-bottom".
///
/// Signal order mirrors Roblox: timers first, then `Stepped`/`RenderStepped`
/// (nominally before physics) and `Heartbeat` (nominally after). Bracketing
/// the engine's physics step exactly would require splitting across schedules;
/// a single ordered pass is the documented simplification.
pub fn drive_luau_frame(
    time: Res<Time>,
    mut state: ResMut<LuauRuntimeState>,
) {
    let Some(runtime) = state.runtime.as_mut() else { return };
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs_f64();

    runtime.step_scheduler(now);
    runtime.fire_stepped(now, dt);
    runtime.fire_render_stepped(dt);
    runtime.fire_heartbeat(dt);
}

/// Pump Bevy input into the live `UserInputService`.
///
/// Two jobs each frame:
///  1. Refresh the held-key / held-button / mouse-position state read by
///     `UserInputService:IsKeyDown` / `:IsMouseButtonPressed` / `:GetMouseLocation`.
///  2. Fire `InputBegan` / `InputEnded` on key & mouse-button edge transitions
///     and `InputChanged` on mouse movement, each carrying an InputObject.
///
/// All input resources are optional so the plugin still loads in a headless
/// app that has no `InputPlugin`.
pub fn sync_luau_input(
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mouse_buttons: Option<Res<ButtonInput<MouseButton>>>,
    mouse_motion: Option<Res<AccumulatedMouseMotion>>,
    windows: Query<&Window>,
    mut state: ResMut<LuauRuntimeState>,
) {
    let Some(runtime) = state.runtime.as_mut() else { return };
    let (Some(keyboard), Some(mouse_buttons)) = (keyboard, mouse_buttons) else { return };

    let cursor = windows
        .iter()
        .next()
        .and_then(|w| w.cursor_position())
        .unwrap_or(Vec2::ZERO);
    let (cx, cy) = (cursor.x as f64, cursor.y as f64);
    let delta = mouse_motion.map(|m| m.delta).unwrap_or(Vec2::ZERO);

    // (1) Held state for the polling queries.
    let keys_down: Vec<String> = keyboard
        .get_pressed()
        .filter_map(|k| bevy_keycode_to_roblox(*k))
        .map(str::to_string)
        .collect();
    let buttons_down: Vec<String> = mouse_buttons
        .get_pressed()
        .filter_map(|b| bevy_mouse_to_roblox(*b))
        .map(str::to_string)
        .collect();
    runtime.update_input_state(&keys_down, &buttons_down, cx, cy, delta.x as f64, delta.y as f64);

    // (2) Edge transitions → signals.
    for k in keyboard.get_just_pressed() {
        if let Some(name) = bevy_keycode_to_roblox(*k) {
            runtime.fire_input("InputBegan", "Keyboard", name, false, cx, cy, 0.0, 0.0);
        }
    }
    for k in keyboard.get_just_released() {
        if let Some(name) = bevy_keycode_to_roblox(*k) {
            runtime.fire_input("InputEnded", "Keyboard", name, false, cx, cy, 0.0, 0.0);
        }
    }
    for b in mouse_buttons.get_just_pressed() {
        if let Some(name) = bevy_mouse_to_roblox(*b) {
            runtime.fire_input("InputBegan", name, "Unknown", false, cx, cy, 0.0, 0.0);
        }
    }
    for b in mouse_buttons.get_just_released() {
        if let Some(name) = bevy_mouse_to_roblox(*b) {
            runtime.fire_input("InputEnded", name, "Unknown", false, cx, cy, 0.0, 0.0);
        }
    }
    if delta != Vec2::ZERO {
        runtime.fire_input("InputChanged", "MouseMovement", "Unknown", false, cx, cy, delta.x as f64, delta.y as f64);
    }
}

/// Map a Bevy [`KeyCode`] to the matching Roblox `Enum.KeyCode` name (the
/// string a script sees after `Enum.KeyCode.X` is normalised). Covers the
/// gameplay-critical set (letters, digits, arrows, modifiers, function keys,
/// common punctuation); unmapped keys return `None` and are simply not
/// surfaced to scripts.
pub fn bevy_keycode_to_roblox(key: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match key {
        KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E",
        KeyF => "F", KeyG => "G", KeyH => "H", KeyI => "I", KeyJ => "J",
        KeyK => "K", KeyL => "L", KeyM => "M", KeyN => "N", KeyO => "O",
        KeyP => "P", KeyQ => "Q", KeyR => "R", KeyS => "S", KeyT => "T",
        KeyU => "U", KeyV => "V", KeyW => "W", KeyX => "X", KeyY => "Y", KeyZ => "Z",
        Digit0 => "Zero", Digit1 => "One", Digit2 => "Two", Digit3 => "Three",
        Digit4 => "Four", Digit5 => "Five", Digit6 => "Six", Digit7 => "Seven",
        Digit8 => "Eight", Digit9 => "Nine",
        Space => "Space",
        Enter | NumpadEnter => "Return",
        Escape => "Escape",
        Tab => "Tab",
        Backspace => "Backspace",
        Delete => "Delete",
        ArrowUp => "Up", ArrowDown => "Down", ArrowLeft => "Left", ArrowRight => "Right",
        ShiftLeft => "LeftShift", ShiftRight => "RightShift",
        ControlLeft => "LeftControl", ControlRight => "RightControl",
        AltLeft => "LeftAlt", AltRight => "RightAlt",
        SuperLeft => "LeftSuper", SuperRight => "RightSuper",
        F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
        F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
        Minus => "Minus", Equal => "Equals",
        BracketLeft => "LeftBracket", BracketRight => "RightBracket",
        Backslash => "BackSlash", Slash => "Slash",
        Semicolon => "Semicolon", Quote => "Quote",
        Comma => "Comma", Period => "Period", Backquote => "Backquote",
        CapsLock => "CapsLock",
        _ => return None,
    })
}

/// Map a Bevy [`MouseButton`] to the Roblox `Enum.UserInputType` name.
pub fn bevy_mouse_to_roblox(button: MouseButton) -> Option<&'static str> {
    Some(match button {
        MouseButton::Left => "MouseButton1",
        MouseButton::Right => "MouseButton2",
        MouseButton::Middle => "MouseButton3",
        _ => return None,
    })
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
