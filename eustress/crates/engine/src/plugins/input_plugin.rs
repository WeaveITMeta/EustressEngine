//! # Input Plugin
//! 
//! Registers InputService and input action system.

use bevy::prelude::*;
use eustress_common::services::input::*;

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<InputService>()
            .register_type::<InputService>()
            .insert_resource(default_input_actions())
            .register_type::<InputActionMap>();
    }
}
