//! Electromagnetism laws — fields, circuits, induction, waves.
pub mod fields;
pub mod circuits;
pub mod induction;
pub mod waves;
pub mod prelude {
    pub use super::fields::*;
    pub use super::circuits::*;
    pub use super::induction::*;
    pub use super::waves::*;
}
