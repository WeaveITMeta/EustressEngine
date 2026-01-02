//! # Fundamental Physics Laws
//!
//! Mathematical implementations of fundamental physics laws.
//!
//! ## Table of Contents
//!
//! 1. **Thermodynamics** - PV=nRT, entropy, energy conservation
//! 2. **Mechanics** - F=ma, momentum, work-energy theorem
//! 3. **Conservation** - Mass, energy, momentum conservation
//! 4. **Electromagnetism** - (Future) Maxwell's equations

pub mod thermodynamics;
pub mod mechanics;
pub mod conservation;

pub mod prelude {
    pub use super::thermodynamics::*;
    pub use super::mechanics::*;
    pub use super::conservation::*;
}
