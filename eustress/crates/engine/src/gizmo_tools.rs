//! # Gizmo Tools Plugin
//!
//! Coordinates transform gizmos for selected objects.
//! Tool gizmos (move arrows, rotation arcs, scale handles) are still drawn
//! via Bevy's immediate-mode Gizmos API — they change every frame based on
//! mouse hover/drag state so mesh-based rendering isn't beneficial.
//!
//! Selection outlines are handled separately by SelectionBoxPlugin (mesh-based).

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, GizmoConfigGroup};

/// Custom gizmo group for transformation tools
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct TransformGizmoGroup;

pub struct GizmoToolsPlugin;

impl Plugin for GizmoToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<TransformGizmoGroup>()
           .add_systems(Startup, configure_transform_gizmos);
    }
}

/// Configure transform gizmos to render on the default layer (main camera).
fn configure_transform_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<TransformGizmoGroup>();
    config.depth_bias = -1.0;
    config.line.width = 3.0;
    config.enabled = true;
    // Default render_layers = layer 0 (main camera) — no custom layer needed
}
