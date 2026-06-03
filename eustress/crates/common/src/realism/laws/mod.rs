//! # Fundamental Physics Laws
//!
//! Mathematical implementations of fundamental physics laws.
//! All functions are pure (no Bevy, no ECS) — inputs/outputs are SI scalars.
//!
//! ## Table of Contents
//!
//! 1. **Thermodynamics**    — PV=nRT, entropy, Gibbs, heat transfer
//! 2. **Mechanics**         — F=ma, momentum, rotation, gravity, springs
//! 3. **Conservation**      — Mass, energy, momentum; Bernoulli
//! 4. **Electrochemistry**  — Nernst, Butler-Volmer, ionic transport, Na-S, degradation
//! 5. **Electromagnetism**  — Maxwell, Coulomb, Biot-Savart, Kirchhoff, R/L/C, induction
//! 6. **Kinetics**          — Arrhenius, rate laws, equilibrium, pH, Michaelis-Menten

pub mod thermodynamics;
pub mod mechanics;
pub mod conservation;
pub mod electrochemistry;
pub mod electromagnetism;
pub mod kinetics;
pub mod optics;
pub mod acoustics;
pub mod biology;

pub mod prelude {
    pub use super::thermodynamics::*;
    pub use super::mechanics::*;
    pub use super::conservation::*;
    pub use super::electrochemistry::*;
    pub use super::electromagnetism::prelude::*;
    pub use super::kinetics::prelude::*;
    pub use super::optics::prelude::*;
    pub use super::acoustics::prelude::*;
    pub use super::biology::prelude::*;
}
