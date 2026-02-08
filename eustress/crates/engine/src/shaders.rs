// ============================================================================
// Eustress Engine - Custom Shaders
// MoonDisk and other custom shader plugins
// ============================================================================

use bevy::prelude::*;

/// Plugin for MoonDisk rendering
/// Note: Currently disabled as Bevy's atmosphere shader already renders the moon
pub struct MoonDiskPlugin;

impl Plugin for MoonDiskPlugin {
    fn build(&self, app: &mut App) {
        // MoonDisk shader systems
        // Currently disabled - Bevy's built-in atmosphere handles this
    }
}
