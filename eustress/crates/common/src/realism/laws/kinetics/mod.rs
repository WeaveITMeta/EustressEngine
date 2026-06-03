//! Chemical kinetics laws — Arrhenius, rate laws, equilibrium, catalysis.
pub mod chemical;
pub mod catalysis;
pub mod prelude {
    pub use super::chemical::*;
    pub use super::catalysis::*;
}