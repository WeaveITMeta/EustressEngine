//! # Quantum Effects
//!
//! Quantum statistical mechanics for advanced simulations.
//!
//! ## Table of Contents
//!
//! 1. **Statistics** - Bose-Einstein, Fermi-Dirac distributions
//! 2. **Condensates** - Bose-Einstein condensate simulation
//! 3. **Tunneling** - Quantum tunneling effects
//!
//! ## Architecture
//!
//! Quantum effects are integrated via Symbolica for exact arithmetic:
//! - Partition functions with rational coefficients
//! - Bose-Einstein distribution for bosons (photons, phonons)
//! - Fermi-Dirac distribution for fermions (electrons)
//! - Quantum corrections to classical thermodynamics

pub mod statistics;
pub mod condensates;

pub mod prelude {
    pub use super::statistics::*;
    pub use super::condensates::*;
    pub use super::QuantumPlugin;
}

use bevy::prelude::*;

/// Quantum effects plugin
pub struct QuantumPlugin;

impl Plugin for QuantumPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<statistics::QuantumState>()
            .add_systems(Update, statistics::update_quantum_statistics);
        
        info!("QuantumPlugin initialized");
    }
}
