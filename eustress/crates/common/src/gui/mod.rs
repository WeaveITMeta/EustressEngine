//! Slint-based in-game GUI rendering for ScreenGui, BillboardGui, SurfaceGui.
//!
//! Lives in eustress-common so both the engine (editor) and client (player)
//! can render interactive UI elements in the 3D world.
//!
//! Architecture:
//! - Each GUI instance gets a `GuiWindowAdapter` (Slint software renderer)
//! - Renders to a pixel buffer → Bevy `Image` → `StandardMaterial`
//! - ScreenGui: fullscreen camera overlay
//! - BillboardGui: quad mesh always facing camera
//! - SurfaceGui: material applied to part face
//! - Input: raycast from camera → UV → Slint PointerMoved/Pressed/Released

#[cfg(feature = "gui")]
pub mod renderer;

#[cfg(feature = "gui")]
pub mod input;

#[cfg(feature = "gui")]
pub mod generator;

#[cfg(feature = "gui")]
pub mod plugin;

#[cfg(feature = "gui")]
pub use plugin::SlintGuiPlugin;

#[cfg(feature = "gui")]
pub use generator::{GuiElement, generate_slint_markup};

#[cfg(feature = "gui")]
pub mod billboard_renderer;

#[cfg(feature = "gui")]
pub use billboard_renderer::BillboardRendererPlugin;

pub mod gui_commands;
pub use gui_commands::{GuiCommand, push_gui_command, drain_gui_commands, set_gui_snapshot, gui_snapshot_get, clear_gui_snapshot,
    ScriptLogLevel, ScriptLogEntry, push_script_log, drain_script_logs};

#[cfg(feature = "gui")]
pub mod gui_bridge;

#[cfg(feature = "gui")]
pub use gui_bridge::GuiBridgePlugin;

pub mod physics_commands;
pub use physics_commands::{PhysicsCommand, PhysicsSnapshot, push_physics_command, drain_physics_commands,
    set_physics_state, get_physics_snapshot, set_workspace_gravity, get_workspace_gravity};
