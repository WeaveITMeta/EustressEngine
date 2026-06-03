//! # Propulsion — rockets, jets, propellers, electric thrusters.
//!
//! Pure propulsion law functions plus a marker plugin.

pub mod rockets;
pub mod jets;
pub mod propellers;
pub mod electric;

pub mod prelude {
    pub use super::rockets::*;
    pub use super::jets::*;
    pub use super::propellers::*;
    pub use super::electric::*;
}

use bevy::prelude::*;
use tracing::info;

pub struct PropulsionPlugin;
impl Plugin for PropulsionPlugin {
    fn build(&self, _app: &mut App) {
        info!("PropulsionPlugin ready — rockets, jets, propellers, electric thrusters");
    }
}
