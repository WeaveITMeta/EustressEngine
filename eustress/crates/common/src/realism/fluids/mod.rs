//! # Fluid Dynamics
//!
//! Water simulation, hydrodynamics, and aerodynamics.
//!
//! ## Table of Contents
//!
//! 1. **SPH** - Smoothed Particle Hydrodynamics
//! 2. **Water** - Water simulation
//! 3. **Aerodynamics** - Lift, drag, turbulence
//! 4. **Buoyancy** - Archimedes' principle

pub mod sph;
pub mod water;
pub mod aerodynamics;
pub mod buoyancy;

pub mod prelude {
    pub use super::sph::*;
    pub use super::water::*;
    pub use super::aerodynamics::*;
    pub use super::buoyancy::*;
    pub use super::FluidsPlugin;
}

use bevy::prelude::*;
use tracing::info;

/// Fluid dynamics plugin
pub struct FluidsPlugin;

impl Plugin for FluidsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<sph::SphConfig>()
            .register_type::<aerodynamics::AerodynamicBody>()
            .register_type::<buoyancy::BuoyancyBody>()
            .add_systems(Update, (
                sph::update_sph_density,
                sph::update_sph_forces,
                aerodynamics::apply_aerodynamic_forces,
                buoyancy::apply_buoyancy_forces,
            ).chain());
        
        info!("FluidsPlugin initialized");
    }
}
