//! # Time Service
//!
//! Time management and synchronization.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Time service resource
#[derive(Resource, Clone, Debug)]
pub struct TimeService {
    pub server_time: f64,
    pub local_time: f64,
    pub time_scale: f32,
    pub paused: bool,
}

impl Default for TimeService {
    fn default() -> Self {
        Self {
            server_time: 0.0,
            local_time: 0.0,
            time_scale: 1.0,
            paused: false,
        }
    }
}

/// Time of day settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeOfDay {
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
}

impl TimeOfDay {
    pub fn to_normalized(&self) -> f32 {
        let total_seconds = self.hours * 3600 + self.minutes * 60 + self.seconds;
        total_seconds as f32 / 86400.0
    }
}
