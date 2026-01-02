//! # LightingService Plugin - Client-side lighting
//! 
//! Re-exports the shared lighting plugin from eustress_common.
//! The client uses the same lighting implementation as the engine.

use bevy::prelude::*;

// Re-export shared plugin and types
#[allow(unused_imports)]
pub use eustress_common::plugins::lighting_plugin::{
    SharedLightingPlugin, SkyboxHandle,
    create_procedural_skybox, regenerate_skybox,
};
#[allow(unused_imports)]
pub use eustress_common::services::lighting::LightingService;
#[allow(unused_imports)]
pub use eustress_common::classes::Atmosphere;
#[allow(unused_imports)]
pub use eustress_common::classes::Sky;

/// Marks the skybox camera component (client-specific)
#[derive(Component)]
#[allow(dead_code)]
pub struct SkyboxCamera;

// ============================================================================
// Plugin
// ============================================================================

/// Client lighting plugin - wraps the shared lighting plugin
/// and adds any client-specific extensions
pub struct LightingServicePlugin;

impl Plugin for LightingServicePlugin {
    fn build(&self, app: &mut App) {
        // Use the shared lighting plugin from common
        app.add_plugins(SharedLightingPlugin);
        
        // Client-specific extensions can be added here
        // e.g., skybox camera, environment map for reflections, etc.
    }
}
