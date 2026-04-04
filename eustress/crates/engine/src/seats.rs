// ============================================================================
// Eustress Engine - Seat Systems
// Auto-sit, controller input for vehicles
// ============================================================================

use bevy::prelude::*;

/// Plugin for seat and vehicle seat systems
pub struct SeatPlugin;

impl Plugin for SeatPlugin {
    fn build(&self, _app: &mut App) {
        // Seat systems:
        // - Auto-sit when player touches seat
        // - Controller input routing for vehicles
        // - Seat occupancy tracking
    }
}
