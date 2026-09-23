//! # Particle simulation solver
//!
//! The solver behind the `ParticleSimulation` class: a structure-of-arrays
//! particle state stepped on the CPU (rayon), covering
//!
//! - **fluids**: SPH with a divergence-free, constant-density solver (DFSPH,
//!   the default) or a weakly compressible one (Tait equation of state),
//!   artificial and laminar viscosity, CSF surface tension, and walls and
//!   parts as Akinci boundary particles that push back on the bodies;
//! - **charged particles**: Boris push in uniform E and B fields, with
//!   self-consistent electrostatics by direct Coulomb summation or
//!   particle-in-cell (cloud-in-cell deposit, conjugate-gradient Poisson);
//! - **conduction**: Drude scattering against a lattice at a set
//!   temperature, so Ohm's law, the conductivity n e^2 tau / m and Joule
//!   heating emerge from the particle motion;
//! - **boundaries**: reflecting, periodic or absorbing domain faces,
//!   electrodes, and obstacles.
//!
//! Everything is SI. Simulated particles may stand for many real ones
//! (macro-particles), which keeps q/m, densities and currents physical at
//! any scale, from nanometre conductors to metre-scale tanks. A run is
//! deterministic for a given seed and parameter set, independent of the
//! thread count.
//!
//! The class itself (property tables, TOML, validation) and its Bevy runtime
//! live in `eustress_common::realism::particle_sim`, which re-exports these
//! modules. The solver is a crate of its own, depending only on `bevy_math`,
//! rayon and bytemuck, so that dev builds compile it optimised (see the
//! workspace `.cargo/config.toml`) and its physics tests run without building
//! the engine: `cargo test -p eustress-particle-sim --release`.

pub mod boundary;
pub mod colormap;
pub mod constants;
pub mod diagnostics;
pub mod grid;
pub mod kernel;
pub mod params;
pub mod poisson;
pub mod presets;
pub mod rng;
pub mod solver;

#[cfg(test)]
mod physics_tests;

pub use colormap::ColorMode;
pub use diagnostics::{SimStats, SpeciesStats, StatSpec, SIMULATION_STATS, SPECIES_STATS};
pub use params::*;
pub use presets::{ConductorMaterial, ParticlePreset, CONDUCTORS};
pub use solver::{ParticleInstance, ParticleSim, SpeciesRuntime};
