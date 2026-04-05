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
