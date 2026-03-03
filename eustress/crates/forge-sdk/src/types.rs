//! # Forge SDK Common Types
//!
//! ## Table of Contents
//!
//! 1. **Region** - Geographic deployment regions
//! 2. **ServerInfo** - Server instance information
//! 3. **PlayerInfo** - Player connection information

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Geographic deployment region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Region {
    /// US East (Virginia)
    UsEast,
    /// US West (Oregon)
    UsWest,
    /// EU West (Frankfurt)
    EuWest,
    /// EU North (Stockholm)
    EuNorth,
    /// Asia Pacific (Tokyo)
    ApNortheast,
    /// Asia Pacific (Singapore)
    ApSoutheast,
    /// South America (São Paulo)
    SaEast,
}

impl Region {
    /// Get the string identifier for this region.
    pub fn as_str(&self) -> &'static str {
        match self {
            Region::UsEast => "us-east-1",
            Region::UsWest => "us-west-2",
            Region::EuWest => "eu-west-1",
            Region::EuNorth => "eu-north-1",
            Region::ApNortheast => "ap-northeast-1",
            Region::ApSoutheast => "ap-southeast-1",
            Region::SaEast => "sa-east-1",
        }
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Information about a running server instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Unique server ID
    pub id: Uuid,
    /// Experience ID this server runs
    pub experience_id: String,
    /// Deployment version
    pub version: String,
    /// Region where the server is running
    pub region: Region,
    /// Server address (host:port)
    pub address: String,
    /// Current player count
    pub player_count: u32,
    /// Maximum player capacity
    pub max_players: u32,
    /// Server status
    pub status: ServerStatus,
    /// When the server was started
    pub started_at: DateTime<Utc>,
    /// CPU usage percentage (0-100)
    pub cpu_usage: f32,
    /// Memory usage in megabytes
    pub memory_mb: f32,
}

/// Server instance status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerStatus {
    /// Starting up
    Starting,
    /// Running and accepting players
    Running,
    /// Running but not accepting new players
    Draining,
    /// Shutting down
    Stopping,
    /// Stopped
    Stopped,
    /// Error state
    Error,
}

/// Information about a connected player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    /// Unique player ID
    pub id: Uuid,
    /// Player display name
    pub name: String,
    /// Server the player is connected to
    pub server_id: Option<Uuid>,
    /// Player's region (nearest)
    pub region: Region,
    /// Connection latency in milliseconds
    pub latency_ms: u32,
    /// When the player connected
    pub connected_at: Option<DateTime<Utc>>,
}
