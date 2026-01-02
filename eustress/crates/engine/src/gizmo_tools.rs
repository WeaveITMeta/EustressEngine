//! # Gizmo Tools Plugin
//!
//! This plugin coordinates transform gizmos for selected objects.
//!
//! ## Architecture
//!
//! The actual tool gizmos are drawn by their respective tool plugins:
//! - `SelectionBoxPlugin` -> draws selection box highlights
//! - `MoveToolPlugin` -> draws move arrows when `Tool::Move` is active
//! - `RotateToolPlugin` -> draws rotation circles when `Tool::Rotate` is active
//! - `ScaleToolPlugin` -> draws scale handles when `Tool::Scale` is active
//!
//! All tool gizmos check before drawing:
//! 1. `state.active` - the tool is currently selected
//! 2. `!query.is_empty()` - there are selected entities
//!
//! This ensures gizmos are NEVER drawn at origin or without a selection.

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, GizmoConfigGroup};

/// Custom gizmo group for transformation tools to ensure they render on top
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct TransformGizmoGroup;

/// Plugin for transform gizmo coordination
///
/// This is a placeholder plugin that ensures the gizmo system is properly
/// initialized. The actual gizmo drawing is delegated to individual tool plugins.
pub struct GizmoToolsPlugin;

impl Plugin for GizmoToolsPlugin {
    fn build(&self, app: &mut App) {
        // Register the shared gizmo group and configure it
        app.init_gizmo_group::<TransformGizmoGroup>()
           .add_systems(Startup, configure_transform_gizmos);
    }
}

/// Configure transform gizmos to render on top
fn configure_transform_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<TransformGizmoGroup>();
    config.depth_bias = -1.0; // Render on top of everything
    config.line.width = 3.0; // Thicker lines for better visibility
}
