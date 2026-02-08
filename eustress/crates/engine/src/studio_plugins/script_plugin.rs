//! # Script Plugin (Studio Plugins)
//!
//! Stub module for script plugin in studio plugins.

use bevy::prelude::*;

/// Script plugin placeholder
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, _app: &mut App) {
        // TODO: Implement script plugin
    }
}

/// Script plugin info
#[derive(Debug, Clone, Default)]
pub struct ScriptPluginInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub script_type: ScriptType,
    pub permissions: Vec<ScriptPermission>,
}

/// Script type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptType {
    #[default]
    Rune,
    Lua,
    Python,
}

/// Script permission
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPermission {
    FileSystem,
    Network,
    UI,
    Physics,
    Audio,
}

/// Script plugin registry
#[derive(Resource, Debug, Default)]
pub struct ScriptPluginRegistry {
    pub plugins: Vec<ScriptPluginWrapper>,
}

/// Script plugin wrapper
#[derive(Debug, Clone, Default)]
pub struct ScriptPluginWrapper {
    pub info: ScriptPluginInfo,
    pub enabled: bool,
    pub path: String,
}
