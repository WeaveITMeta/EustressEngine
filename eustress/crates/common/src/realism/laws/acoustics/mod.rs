//! # Acoustics — wave physics, propagation, room acoustics.
pub mod waves;
pub mod propagation;
pub mod rooms;
pub mod prelude {
    pub use super::waves::*;
    pub use super::propagation::*;
    pub use super::rooms::*;
}
