// ============================================================================
// Eustress Engine - IO Manager
// Async data fetching for Parameters system
// ============================================================================

use bevy::prelude::*;

/// Plugin for async IO operations
pub struct IoManagerPlugin;

impl Plugin for IoManagerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IoManager>();
        // IO systems:
        // - Async file loading
        // - Network data fetching
        // - Parameter system data sources
    }
}

/// Resource for managing async IO operations
#[derive(Resource, Default)]
pub struct IoManager {
    /// Pending fetch operations
    pending_fetches: Vec<PendingFetch>,
}

/// A pending async fetch operation
pub struct PendingFetch {
    pub url: String,
    pub callback_id: u64,
}

impl IoManager {
    /// Queue a fetch operation
    pub fn fetch(&mut self, url: &str, callback_id: u64) {
        self.pending_fetches.push(PendingFetch {
            url: url.to_string(),
            callback_id,
        });
    }
    
    /// Get pending fetches
    pub fn pending(&self) -> &[PendingFetch] {
        &self.pending_fetches
    }
    
    /// Clear completed fetches
    pub fn clear_completed(&mut self) {
        self.pending_fetches.clear();
    }
}
