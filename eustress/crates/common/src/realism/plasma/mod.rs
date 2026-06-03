//! # Plasma Physics & Fusion — Debye, MHD, fusion, ECS state.
pub mod debye;
pub mod mhd;
pub mod fusion;
pub mod components;

pub mod prelude {
    pub use super::debye::*;
    pub use super::mhd::*;
    pub use super::fusion::*;
    pub use super::components::{PlasmaState, FusionPlasma};
}

use bevy::prelude::*;
use tracing::info;
use components::{PlasmaState, FusionPlasma};

pub struct PlasmaPlugin;
impl Plugin for PlasmaPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<PlasmaState>()
           .register_type::<FusionPlasma>();
        info!("PlasmaPlugin ready — Debye, MHD, fusion physics available");
    }
}
