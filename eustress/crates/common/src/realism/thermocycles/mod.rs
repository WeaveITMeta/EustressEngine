//! # Thermodynamic Cycles — Rankine, Brayton, Otto/Diesel, refrigeration, heat exchangers.
//!
//! Pure steady-state cycle analysis functions plus a marker plugin.

pub mod rankine;
pub mod brayton;
pub mod otto;
pub mod refrigeration;
pub mod heat_exchangers;

pub mod prelude {
    pub use super::rankine::*;
    pub use super::brayton::*;
    pub use super::otto::*;
    pub use super::refrigeration::*;
    pub use super::heat_exchangers::*;
}

use bevy::prelude::*;
use tracing::info;

pub struct ThermoCyclesPlugin;
impl Plugin for ThermoCyclesPlugin {
    fn build(&self, _app: &mut App) {
        info!("ThermoCyclesPlugin ready — Rankine, Brayton, Otto, refrigeration, heat exchangers");
    }
}
