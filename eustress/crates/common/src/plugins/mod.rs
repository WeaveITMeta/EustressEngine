//! # Shared Plugins
//! 
//! Bevy plugins that can be used by both Engine and Client.
//! These provide common functionality with shared implementations.

pub mod lighting_plugin;
pub mod character_plugin;
pub mod humanoid;
pub mod skinned_character;
pub mod animation_plugin;

pub use lighting_plugin::*;
pub use character_plugin::*;
pub use humanoid::*;
pub use skinned_character::*;
pub use animation_plugin::*;
