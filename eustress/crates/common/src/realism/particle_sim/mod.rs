//! # Particle Simulation
//!
//! The `ParticleSimulation` and `ParticleSpecies` classes (property tables,
//! TOML round trip, validation) and their Bevy runtime (stepping on the world
//! clock within the frame budget, commands, the render buffer).
//!
//! The solver is the `eustress-particle-sim` crate: SPH fluids (DFSPH and
//! WCSPH), charged particles in fields, particle-in-cell electrostatics and
//! Drude conduction, all SI and deterministic. Its modules are re-exported
//! here, so `particle_sim::solver::ParticleSim`, `particle_sim::params` and
//! the rest resolve through this module.

pub mod class;
pub mod plugin;

pub use eustress_particle_sim::{boundary, colormap, diagnostics, grid, params, poisson, presets, rng, solver};

pub use class::{FieldKind, FieldSpec, FieldTable, FieldValue, ParticleSimulation, ParticleSpecies};
pub use colormap::ColorMode;
pub use diagnostics::{SimStats, SpeciesStats, StatSpec, SIMULATION_STATS, SPECIES_STATS};
pub use params::*;
pub use plugin::{
    ParticleCloud, ParticleSimAction, ParticleSimCommand, ParticleSimControl, ParticleSimRuntime, ParticleSimSet,
    ParticleSimulationPlugin,
};
pub use presets::{ConductorMaterial, ParticlePreset, CONDUCTORS};
pub use solver::{ParticleInstance, ParticleSim, SpeciesRuntime};

#[cfg(test)]
mod tests {
    use crate::realism::constants as common;
    use eustress_particle_sim::constants as solver;

    /// The solver sits below this crate and carries its own copy of the
    /// constants it uses; they must stay the engine's values.
    #[test]
    fn solver_constants_are_the_engine_constants() {
        assert_eq!(solver::K_B, common::K_B);
        assert_eq!(solver::EPSILON_0, common::EPSILON_0);
        assert_eq!(solver::ELEMENTARY_CHARGE, common::ELEMENTARY_CHARGE);
        assert_eq!(solver::ELECTRON_MASS, common::ELECTRON_MASS);
        assert_eq!(solver::PROTON_MASS, common::PROTON_MASS);
        assert_eq!(solver::ATOMIC_MASS_UNIT, common::ATOMIC_MASS_UNIT);
        assert_eq!(solver::COULOMB_K, common::COULOMB_K);
        assert_eq!(solver::WATER_DENSITY, common::WATER_DENSITY);
        assert_eq!(solver::WATER_VISCOSITY, common::WATER_VISCOSITY);
    }
}
