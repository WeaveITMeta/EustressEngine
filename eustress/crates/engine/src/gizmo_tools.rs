//! # Gizmo Tools Plugin
//!
//! Coordinates transform gizmos and debug visualizations for the editor.
//!
//! ## Architecture
//!
//! - `TransformGizmoGroup` — custom group for move/rotate/scale tool handles
//! - `LightGizmoConfigGroup` — Bevy built-in, visualizes point/spot/directional lights
//! - `DefaultGizmoConfigGroup` — selection outlines (mesh-based, not gizmos)
//!
//! Tool gizmos (move arrows, rotation arcs, scale handles) are drawn via
//! Bevy's immediate-mode Gizmos API — they change every frame based on
//! mouse hover/drag state so mesh-based rendering isn't beneficial.
//!
//! Selection outlines are handled separately by SelectionBoxPlugin (mesh-based).

use bevy::prelude::*;
use bevy::gizmos::config::{GizmoConfigStore, GizmoConfigGroup};

/// Custom gizmo group for transformation tools (move/rotate/scale handles)
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct TransformGizmoGroup;

pub struct GizmoToolsPlugin;

impl Plugin for GizmoToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<TransformGizmoGroup>()
           .add_systems(Startup, configure_gizmos);
    }
}

/// Configure all gizmo groups on startup
fn configure_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    // Transform tool gizmos — render on top of everything
    {
        let (config, _) = config_store.config_mut::<TransformGizmoGroup>();
        config.depth_bias = -1.0; // Always on top
        config.line.width = 3.0;
        config.enabled = true;
        // Default render_layers = layer 0 (main camera)
    }

    // Default gizmos — used for debug visualization
    {
        let (config, _) = config_store.config_mut::<bevy::gizmos::config::DefaultGizmoConfigGroup>();
        config.depth_bias = -0.5;
        config.line.width = 2.0;
        config.enabled = true;
    }

    // Light gizmos — visualize point/spot/directional light shapes and ranges
    {
        let (config, light_config) = config_store.config_mut::<bevy::gizmos::light::LightGizmoConfigGroup>();
        config.enabled = true;
        config.depth_bias = -0.5;
        config.line.width = 1.5;
        light_config.draw_all = true;
        light_config.color = bevy::gizmos::light::LightGizmoColor::MatchLightColor;
    }
}
