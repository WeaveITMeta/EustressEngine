//! # Ocean surface
//!
//! The water surface behind the Terrain object's sea: a spectral wave field
//! synthesised by inverse FFT (Tessendorf's method) from an oceanographic
//! spectrum, so wave heights, lengths and speeds follow from the wind, the
//! fetch, the depth and gravity rather than from hand-tuned noise. The
//! Terrain object drives it and uploads its fields as textures; physics
//! samples it on the CPU for buoyancy.
//!
//! - **spectrum** ([`spectrum`]): JONSWAP in angular frequency with the
//!   Pierson-Moskowitz limit for a fully developed sea, an optional finite
//!   fetch, an optional narrow swell, Longuet-Higgins directional spreading,
//!   and linear dispersion over a flat bed of any depth;
//! - **synthesis** ([`cascade`]): random complex amplitudes on three
//!   wavenumber lattices of different tile sizes, each owning its own band
//!   of wavenumbers, evolved in time by the dispersion relation and turned
//!   into heights, choppy horizontal displacements, slopes and the folding
//!   Jacobian by inverse 2D FFTs ([`fft`]), two real fields per complex
//!   transform;
//! - **sampling** ([`ocean`]): the surface displacement, the height of the
//!   water above a world point and its rate of rise, its slopes, normal,
//!   folding Jacobian and foam, summed over the cascades;
//! - **surface rules** ([`surface`]): what drawing and physics share, so
//!   that what floats feels what is drawn: the normal from slopes, the foam
//!   threshold and ramp on the Jacobian, and the fade of the waves toward a
//!   shore, which keeps crests from cutting through land;
//! - **buoyancy** ([`buoyancy`]): a body cut into sample cells, each lifted
//!   by the weight of the water it displaces and damped by a linear drag
//!   relative to the moving surface, as plain forces at points for any
//!   physics engine to apply;
//! - **sea states** ([`sea_state`]): named winds for property text.
//!
//! World metres and seconds throughout; Y is up and the rest surface is the
//! XZ plane at height 0. Directions are unit vectors in XZ giving where
//! waves travel toward. Gravity is a setting, since worlds may change it.
//! A sea is deterministic for a given seed and settings, independent of the
//! thread count: every random amplitude is a pure function of its lattice
//! key, and every parallel pass writes rows that one thread computes in a
//! fixed order.
//!
//! The crate depends only on rayon, so its physics tests run without
//! building the engine: `cargo test -p eustress-ocean --release`.

pub mod buoyancy;
pub mod cascade;
pub mod fft;
pub mod ocean;
pub mod rng;
pub mod sea_state;
pub mod spectrum;
pub mod surface;

#[cfg(test)]
mod physics_tests;

pub use buoyancy::{
    box_samples, buoyancy_forces, sample_force, scale_volumes, stable_drag, submerged_fraction, submerged_volume,
    BuoyancyForce, BuoyancySample, WaterColumn, DEFAULT_DRAG, SEAWATER_DENSITY,
};
pub use cascade::Cascade;
pub use fft::{Complex, Fft, Fft2d};
pub use ocean::{Detail, Ocean, OceanSettings};
pub use sea_state::{pm_significant_wave_height, SeaState};
pub use spectrum::{Component, Dispersion, Jonswap, Spectrum, Spreading, EARTH_GRAVITY};
pub use surface::{
    foam_coverage, foam_threshold, normal_from_slopes, shore_fade, FOAM_FULL_THRESHOLD, FOAM_OFF_THRESHOLD, FOAM_RAMP,
    MIN_SHORE_FADE_DEPTH, SHORE_FADE_WAVE_HEIGHTS,
};
