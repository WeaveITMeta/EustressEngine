//! # Billboard GUI Types
//!
//! Shared component types for 3D GUI surfaces — `BillboardGuiMarker`,
//! `SurfaceGuiMarker`, and `GuiElementDisplay`. The actual rendering lives
//! in the engine crate (`engine::billboard_gui`), which instantiates a
//! `BillboardCard` Slint component per marker and software-renders it into
//! a GPU texture mapped onto a world-space quad.
//!
//! Historical note: this file used to host a CPU-atlas + fontdue renderer.
//! That path was removed once the Slint-based renderer landed — it couldn't
//! share theming with the editor UI and had a hard 128 px font-size ceiling.
//! The `BillboardRendererPlugin` is now a no-op shim kept so upstream crates
//! registering it don't need coordinated edits; prefer
//! `engine::billboard_gui::BillboardGuiPlugin` in new code.

use bevy::prelude::*;

/// GUI element display data — attached to each Frame / TextLabel / Button /
/// ... entity. Used by the Slint billboard renderer to build the card's
/// `BillboardLabelData` model, and by the screen-space UI pipeline.
#[derive(Component, Debug, Clone)]
pub struct GuiElementDisplay {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub z_order: i32,
    pub visible: bool,
    pub clip_children: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub bg_color: [f32; 4],
    pub border_size: f32,
    pub border_color: [f32; 4],
    pub corner_radius: f32,
    pub text: String,
    pub text_color: [f32; 4],
    pub font_size: f32,
    /// CSS-style font weight (400 = normal, 700 = bold). Drives
    /// `Text.font-weight` in the Slint billboard card; irrelevant
    /// for non-text elements.
    pub font_weight: i32,
    pub text_align: String,
    pub image_path: String,
    pub class_type: String,
    /// Mouse filter mode (Godot-style):
    /// - "stop" (default): consumes mouse events, blocks 3D selection behind
    /// - "pass": receives events but passes them through
    /// - "ignore": transparent to mouse, events go straight through
    pub mouse_filter: String,
}

/// Marker for BillboardGui entities — rendered by the engine crate as 3D
/// quads facing the camera. `size` is in billboard-local pixels; the Slint
/// card's canvas matches those dimensions.
///
/// All fields are live-edited: the engine's `sync_billboard_properties`
/// system watches `Changed<BillboardGuiMarker>` and pushes updates into
/// the quad's visibility, material depth-bias, and texture size each frame.
#[derive(Component, Debug, Clone)]
pub struct BillboardGuiMarker {
    /// Pixel canvas dimensions of the Slint card ([width, height]).
    pub size: [f32; 2],
    /// Distance in meters beyond which the quad is hidden. 0 disables culling.
    pub max_distance: f32,
    /// Render in front of all 3D geometry (depth-bias override).
    pub always_on_top: bool,
    /// When true the quad yaws to face the active camera each frame.
    /// Set false for e.g. world-anchored signs that keep a fixed rotation.
    pub face_camera: bool,
    /// Explicit visibility toggle from scripts / Properties panel. Distinct
    /// from `max_distance` culling so the two don't fight each other.
    pub visible: bool,
}

impl Default for BillboardGuiMarker {
    fn default() -> Self {
        Self {
            size: [200.0, 100.0],
            max_distance: 100.0,
            always_on_top: false,
            face_camera: true,
            visible: true,
        }
    }
}

/// Marker for SurfaceGui entities — rendered as textures mapped to a part face.
#[derive(Component, Debug)]
pub struct SurfaceGuiMarker {
    pub face: String,
    pub target_part: String,
    /// Texture resolution per world meter on the target surface.
    pub pixels_per_meter: f32,
}

/// Legacy plugin name preserved for registration sites. The real billboard
/// systems live in `engine::billboard_gui::BillboardGuiPlugin` now.
pub struct BillboardRendererPlugin;

impl Plugin for BillboardRendererPlugin {
    fn build(&self, _app: &mut App) {
        // Intentionally empty. See module docs.
    }
}
