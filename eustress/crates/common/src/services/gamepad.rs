//! # Gamepad Service
//!
//! Gamepad input handling.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Gamepad service resource
#[derive(Resource, Default, Clone, Debug)]
pub struct GamepadService {
    pub connected_gamepads: Vec<Entity>,
}

/// Gamepad vibration settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GamepadVibration {
    pub left_motor: f32,
    pub right_motor: f32,
    pub duration: f32,
}
