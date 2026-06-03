//! # Optics — geometric, wave, photon physics.
pub mod geometric;
pub mod wave;
pub mod photons;
pub mod prelude {
    pub use super::geometric::*;
    pub use super::wave::*;
    pub use super::photons::*;
}
