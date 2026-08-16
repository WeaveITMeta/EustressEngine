//! # Shared Plugins
//! 
//! Bevy plugins that can be used by both Engine and Client.
//! These provide common functionality with shared implementations.

pub mod lighting_plugin;
pub mod sky_atmosphere;
pub mod reflections;
pub mod character_plugin;
pub mod humanoid;
pub mod skinned_character;
pub mod animation_plugin;

pub use lighting_plugin::*;
// Named re-exports rather than globs: `lighting_plugin` already re-exports the
// shared sky types for backwards compatibility, and two globs offering the same
// item make the path ambiguous.
pub use sky_atmosphere::{
    build_atmosphere_settings, build_planet, build_scattering_medium, AtmospherePlanet,
    SkyAtmospherePlugin,
};
pub use reflections::{ReflectionProbe, ReflectionSettings, ReflectionsPlugin, SsrQuality};
pub use character_plugin::*;
pub use humanoid::*;
pub use skinned_character::*;
pub use animation_plugin::*;
