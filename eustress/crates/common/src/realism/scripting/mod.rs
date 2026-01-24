//! # Rune Scripting API
//!
//! Dynamic physics scripting using Rune.
//!
//! ## Table of Contents
//!
//! 1. **API** - Exposed functions and types
//! 2. **Bindings** - ECS <-> Rune bindings
//! 3. **Hot Reload** - Script hot-reloading
//!
//! ## Architecture
//!
//! Rune provides:
//! - **Hot-reloadable** physics scripts
//! - **DSL** for defining custom laws and behaviors
//! - **Safe sandboxing** for user-generated content
//! - **Real-time interactivity** without recompilation

#[cfg(feature = "realism-scripting")]
pub mod api;
#[cfg(feature = "realism-scripting")]
pub mod bindings;
#[cfg(feature = "realism-scripting")]
pub mod hot_reload;
#[cfg(feature = "realism-scripting")]
pub mod viga_api;

pub mod prelude {
    #[cfg(feature = "realism-scripting")]
    pub use super::api::*;
    #[cfg(feature = "realism-scripting")]
    pub use super::bindings::*;
    #[cfg(feature = "realism-scripting")]
    pub use super::hot_reload::*;
    #[cfg(feature = "realism-scripting")]
    pub use super::ScriptingPlugin;
}

use bevy::prelude::*;

/// Rune scripting plugin for physics
#[cfg(feature = "realism-scripting")]
pub struct ScriptingPlugin;

#[cfg(feature = "realism-scripting")]
impl Plugin for ScriptingPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<hot_reload::ScriptManager>()
            .add_systems(Update, hot_reload::check_script_changes);
        info!("ScriptingPlugin initialized - Rune scripting ready");
    }
}

/// Placeholder plugin when feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub struct ScriptingPlugin;

#[cfg(not(feature = "realism-scripting"))]
impl Plugin for ScriptingPlugin {
    fn build(&self, _app: &mut App) {
        info!("ScriptingPlugin disabled - enable 'realism-scripting' feature");
    }
}
