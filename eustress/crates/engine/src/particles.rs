//! # Particles
//!
//! Engine side of the `ParticleSimulation` / `ParticleSpecies` classes. The
//! solver and its per-entity runtime live in
//! `eustress_common::realism::particle_sim` (so the headless runner and the
//! player simulate identically); this module adds what needs the engine:
//!
//! - [`bridge`]: Avian obstacles and two-way coupling, Play/Stop semantics,
//!   `psim.*` sim values for scripts and MCP, species parenting, section
//!   persistence and hot reload. Runs headless too.
//! - [`render`]: the instanced particle renderer (editor and player).
//! - The domain outline, drawn with gizmos in the editor.

pub mod bridge;
pub mod render;

use bevy::prelude::*;

use eustress_common::realism::particle_sim::{ParticleCloud, ParticleSimulation};

pub use bridge::ParticleSimBridgePlugin;
pub use render::ParticleRenderPlugin;

/// Editor-side particle simulation support: rendering plus the domain
/// outline. The simulation itself runs from `RealismPlugin`, and the
/// engine bridge from [`ParticleSimBridgePlugin`] (registered with the core
/// simulation plugins so headless runs get it too).
pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ParticleRenderPlugin)
            .add_systems(Update, draw_domain_outlines);
    }
}

/// Outline of each simulation's domain box (world size = domain size times
/// display scale), so an empty or settled simulation is still findable.
fn draw_domain_outlines(
    mut gizmos: Gizmos,
    sims: Query<(&ParticleSimulation, &ParticleCloud, &GlobalTransform, Option<&InheritedVisibility>)>,
) {
    for (class, cloud, global, visibility) in &sims {
        if !class.show_domain || !visibility.map_or(true, |v| v.get()) {
            continue;
        }
        let (_, rotation, translation) = global.to_scale_rotation_translation();
        let size = cloud.domain_size * cloud.display_scale;
        if !size.is_finite() || size.min_element() <= 0.0 {
            continue;
        }
        let color = if class.enabled { Color::srgba(0.35, 0.8, 1.0, 0.6) } else { Color::srgba(0.5, 0.5, 0.5, 0.4) };
        gizmos.cube(Transform { translation, rotation, scale: size }, color);
    }
}
