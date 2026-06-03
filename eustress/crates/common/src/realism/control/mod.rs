//! # Control Systems — PID, state-space, frequency analysis, digital filters.
//!
//! Generalizes the nuclear reactor PID into a universal control toolkit.
//! Use PidController from this module; the nuclear/components.rs PidState
//! is kept for backward compatibility but new code should use this.

pub mod pid;
pub mod state_space;
pub mod frequency;
pub mod discrete;

pub mod prelude {
    pub use super::pid::{PidController, gain_scheduled_update, cascade_update, bumpless_transfer};
    pub use super::state_space::{StateSpaceModel, tf_to_state_space, second_order_to_ss, first_order_to_ss};
    pub use super::frequency::*;
    pub use super::discrete::*;
}

use bevy::prelude::*;
use tracing::info;

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, _app: &mut App) {
        info!("ControlPlugin ready — PID, state-space, Bode, digital filters active");
    }
}