// =============================================================================
// Eustress Web - Presence WebSocket API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Presence WebSocket Functions
// =============================================================================

use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Presence status for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PresenceStatus {
    Online,
    Away,
    InExperience,
    Offline,
}

/// Presence update message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    pub user_id: String,
    pub status: PresenceStatus,
    pub experience_id: Option<String>,
    pub timestamp: String,
}

/// Presence subscription message (sent to server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceSubscribe {
    pub user_ids: Vec<String>,
}

// -----------------------------------------------------------------------------
// 2. Presence WebSocket Functions
// -----------------------------------------------------------------------------

/// Connect to the presence WebSocket endpoint.
/// Returns the WebSocket URL for the presence service.
pub fn presence_ws_url(base_url: &str) -> String {
    let ws_base = base_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    format!("{}/ws/presence", ws_base)
}
