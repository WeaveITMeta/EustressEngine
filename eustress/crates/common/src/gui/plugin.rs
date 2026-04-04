//! SlintGuiPlugin — registers all GUI rendering and input systems.
//!
//! Used by both engine (editor) and client (player) crates.
//! Requires the `gui` feature flag on eustress-common.

use bevy::prelude::*;
use super::renderer::{self, SlintGuiAdapters};
use super::input;

/// Plugin for Slint-based in-game GUI rendering.
///
/// Registers:
/// - `SlintGuiAdapters` NonSend resource (per-instance adapter storage)
/// - `render_slint_guis` system (renders all GUI textures each frame)
/// - `billboard_face_camera` system (orients billboard GUIs toward camera)
/// - `handle_gui_input` system (raycasts mouse against 3D GUI quads)
pub struct SlintGuiPlugin;

impl Plugin for SlintGuiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_non_send_resource(SlintGuiAdapters::default())
            .add_systems(Update, (
                input::handle_gui_input,
                renderer::render_slint_guis.after(input::handle_gui_input),
                renderer::billboard_face_camera,
            ));
    }
}
