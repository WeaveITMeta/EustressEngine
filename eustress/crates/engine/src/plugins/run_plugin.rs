//! # Run Plugin
//! 
//! Registers RunService and game state management.

use bevy::prelude::*;
use eustress_common::services::run::*;

pub struct RunPlugin;

impl Plugin for RunPlugin {
    fn build(&self, app: &mut App) {
        // Use the helper from common
        RunServiceTypes::register(app);
        
        // Set to studio mode
        app.insert_resource(RunService::studio());
    }
}
