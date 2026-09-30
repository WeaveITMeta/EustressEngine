//! # Nuclear physics
//!
//! Stateless laws for radioactive decay, radiation shielding, criticality and
//! reactor kinetics. Scripts reach them under `eustress::realism::nuclear`,
//! and a Space composes them into whatever it simulates, from a shielding
//! study to a whole reactor with its own controller.

pub mod decay;
pub mod shielding;
pub mod criticality;
pub mod kinetics;

pub mod prelude {
    // Namespaced, not globbed: several laws share names across these modules.
    pub use super::{decay, shielding, criticality, kinetics};
}
