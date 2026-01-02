//! # Animation Plugin (Client)
//!
//! Re-exports the shared animation plugin from eustress_common.
//! The client uses the same animation implementation as the engine.

// Re-export everything from the shared animation plugin
pub use eustress_common::plugins::animation_plugin::*;

/// Client-side animation plugin - wraps the shared plugin
/// and adds any client-specific extensions
pub struct CharacterAnimationPlugin;

impl bevy::prelude::Plugin for CharacterAnimationPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        // Use the shared animation plugin from common
        app.add_plugins(eustress_common::plugins::SharedAnimationPlugin);
        
        // Client-specific extensions can be added here
        // e.g., network animation sync, LOD animation, etc.
    }
}
