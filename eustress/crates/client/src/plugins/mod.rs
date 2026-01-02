//! # Client Plugins
//! 
//! Modular plugins for each service. Add only what you need.
//! 
//! ## Usage
//! ```rust
//! app.add_plugins(WorkspacePlugin)
//!    .add_plugins(PlayerServicePlugin)
//!    .add_plugins(LightingServicePlugin);
//! ```

// Core services
pub mod workspace_plugin;
pub mod sound_plugin;
pub mod physics_plugin;
pub mod input_plugin;
pub mod run_plugin;

// Game services
pub mod player_plugin;
pub mod lighting_plugin;
pub mod terrain_plugin;
pub mod enhancement_plugin;
pub mod pause_menu;
pub mod character_controller;
pub mod animation_plugin;

// Re-export plugins
pub use workspace_plugin::WorkspacePlugin;
pub use sound_plugin::SoundPlugin;
pub use physics_plugin::PhysicsPlugin;
pub use input_plugin::InputPlugin;
pub use run_plugin::RunPlugin;
pub use player_plugin::PlayerServicePlugin;
pub use lighting_plugin::LightingServicePlugin;
pub use terrain_plugin::ClientTerrainPlugin;
pub use enhancement_plugin::*;
pub use pause_menu::PauseMenuPlugin;
pub use animation_plugin::CharacterAnimationPlugin;

// Re-export shared types from common
#[allow(unused_imports)]
pub use eustress_common::classes;
#[allow(unused_imports)]
pub use eustress_common::services;

use bevy::prelude::*;

/// All-in-one plugin that adds all core services
#[allow(dead_code)]
pub struct AllServicesPlugin;

impl Plugin for AllServicesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            WorkspacePlugin,
            SoundPlugin,
            PhysicsPlugin,
            InputPlugin,
            RunPlugin,
            PlayerServicePlugin,
            LightingServicePlugin,
        ));
    }
}
