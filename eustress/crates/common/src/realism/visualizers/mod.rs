//! # Visualizers
//!
//! Real-time property display for Studio Engine.
//!
//! ## Table of Contents
//!
//! 1. **Property Overlay** - T, P, V, U, S overlays
//! 2. **Vector Field** - Force/velocity field visualization
//! 3. **Heat Map** - Temperature gradients
//! 4. **Stress Visualization** - Stress tensor display

pub mod property_overlay;
pub mod vector_field;
pub mod heat_map;
pub mod stress_viz;

pub mod prelude {
    pub use super::property_overlay::*;
    pub use super::vector_field::*;
    pub use super::heat_map::*;
    pub use super::stress_viz::*;
    pub use super::VisualizersPlugin;
}

use bevy::prelude::*;

/// Visualizers plugin
pub struct VisualizersPlugin;

impl Plugin for VisualizersPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<property_overlay::OverlaySettings>()
            .init_resource::<vector_field::VectorFieldSettings>()
            .init_resource::<heat_map::HeatMapSettings>()
            .register_type::<property_overlay::PropertyOverlay>()
            .add_systems(Update, (
                property_overlay::update_property_overlays,
                vector_field::draw_vector_field,
                stress_viz::draw_stress_indicators,
            ));
        
        info!("VisualizersPlugin initialized");
    }
}
