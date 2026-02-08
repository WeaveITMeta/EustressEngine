// ============================================================================
// Eustress Engine - Embedded Client
// Embedded client runtime (same codebase as standalone client)
// ============================================================================

use bevy::prelude::*;

/// Plugin for running embedded client runtime during play mode
pub struct EmbeddedClientPlugin;

impl Plugin for EmbeddedClientPlugin {
    fn build(&self, app: &mut App) {
        // Embedded client systems will be added here
        // This shares code with the standalone client for consistency
    }
}
