//! # Particle ECS System
//!
//! High-performance particle system with physical properties.
//!
//! ## Table of Contents
//!
//! 1. **Components** - Particle, ThermodynamicState, KineticState
//! 2. **Systems** - Update systems (parallel via Rayon)
//! 3. **Spawner** - Particle emission
//! 4. **Spatial** - Spatial hashing for neighbor queries

pub mod components;
pub mod systems;
pub mod spawner;
pub mod spatial;

pub mod prelude {
    pub use super::components::*;
    pub use super::systems::*;
    pub use super::spawner::*;
    pub use super::spatial::*;
    pub use super::ParticlePlugin;
}

use bevy::prelude::*;

/// Particle physics plugin
pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<spatial::SpatialHash>()
            .init_resource::<spawner::ParticleSpawnerConfig>()
            .register_type::<components::Particle>()
            .register_type::<components::ThermodynamicState>()
            .register_type::<components::KineticState>()
            .register_type::<components::ParticleType>()
            .add_systems(Update, (
                systems::update_spatial_hash,
                systems::update_thermodynamics,
                systems::update_kinematics,
                systems::apply_particle_forces,
            ).chain());
        
        info!("ParticlePlugin initialized");
    }
}
