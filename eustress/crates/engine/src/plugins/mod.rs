//! # Engine Plugins
//! 
//! Modular plugins for each service. Add only what you need.
//! 
//! ## Usage
//! ```rust
//! app.add_plugins(WorkspacePlugin)
//!    .add_plugins(LightingPlugin)
//!    .add_plugins(CelestialPlugin)
//!    .add_plugins(CloudsPlugin)
//!    .add_plugins(SoundPlugin);
//! ```

pub mod workspace_plugin;
pub mod lighting_plugin;
pub mod sound_plugin;
pub mod physics_plugin;
pub mod input_plugin;
pub mod run_plugin;
pub mod celestial_plugin;
pub mod clouds_plugin;

pub use workspace_plugin::WorkspacePlugin;
pub use lighting_plugin::LightingPlugin;
pub use sound_plugin::SoundPlugin;
pub use physics_plugin::PhysicsPlugin;
pub use input_plugin::InputPlugin;
pub use run_plugin::RunPlugin;
pub use celestial_plugin::CelestialPlugin;
pub use clouds_plugin::CloudsPlugin;

use bevy::prelude::*;
use eustress_common::{AttributesPlugin, ParametersPlugin};

/// All-in-one plugin that adds all services. Use individual plugins for more control.
pub struct AllServicesPlugin;

impl Plugin for AllServicesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            WorkspacePlugin,
            LightingPlugin,
            CelestialPlugin,
            CloudsPlugin,
            SoundPlugin,
            PhysicsPlugin,
            InputPlugin,
            RunPlugin,
            AttributesPlugin,
            ParametersPlugin,
        ));
    }
}
