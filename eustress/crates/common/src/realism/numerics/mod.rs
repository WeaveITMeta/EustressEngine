//! # Numerics — ODE solvers, interpolation, statistics, optimization.
//!
//! Pure mathematical infrastructure. No Bevy ECS components.
//! Used by all other simulation modules for time integration and analysis.

pub mod ode;
pub mod interpolation;
pub mod statistics;

pub mod prelude {
    pub use super::ode::prelude::*;
    pub use super::interpolation::*;
    pub use super::statistics::prelude::*;
}

use bevy::prelude::*;
use tracing::info;

pub struct NumericsPlugin;
impl Plugin for NumericsPlugin {
    fn build(&self, _app: &mut App) {
        info!("NumericsPlugin ready — RK4/RK45/BDF/Verlet integrators available");
    }
}
