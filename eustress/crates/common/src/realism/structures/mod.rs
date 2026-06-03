//! # Structures — beams, columns, fatigue, composites.
//!
//! Pure structural-engineering law functions plus a marker plugin.
//! Static analysis (no per-frame ECS state by default).

pub mod beams;
pub mod columns;
pub mod fatigue;
pub mod composites;

pub mod prelude {
    pub use super::beams::*;
    pub use super::columns::*;
    pub use super::fatigue::*;
    pub use super::composites::*;
}

use bevy::prelude::*;
use tracing::info;

pub struct StructuresPlugin;
impl Plugin for StructuresPlugin {
    fn build(&self, _app: &mut App) {
        info!("StructuresPlugin ready — beams, columns, fatigue, composites available");
    }
}
