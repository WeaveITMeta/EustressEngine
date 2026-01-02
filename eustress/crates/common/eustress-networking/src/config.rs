//! # Network Configuration
//!
//! Central configuration for Eustress networking.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

// ============================================================================
// Main Config
// ============================================================================

/// Main network configuration resource.
///
/// Insert this before adding [`EustressNetworkingPlugin`] to customize behavior.
#[derive(Resource, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Resource)]
pub struct NetworkConfig {
    /// Tick rate configuration
    pub tick: TickConfig,

    /// Transport layer configuration
    #[reflect(ignore)]
    pub transport: TransportConfig,

    /// Replication settings
    pub replication: ReplicationConfig,

    /// Ownership settings
    pub ownership: OwnershipConfig,

    /// Anti-exploit settings
    pub anti_exploit: AntiExploitConfig,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            tick: TickConfig::default(),
            transport: TransportConfig::default(),
            replication: ReplicationConfig::default(),
            ownership: OwnershipConfig::default(),
            anti_exploit: AntiExploitConfig::default(),
        }
    }
}

// ============================================================================
// Tick Configuration
// ============================================================================

/// Tick rate configuration for network updates.
///
/// Eustress uses a multi-tier tick system:
/// - **Physics**: 120Hz (fixed timestep for determinism)
/// - **Replication**: 120Hz for owned entities, 60Hz for remote
/// - **Fallback**: 30Hz when RTT > threshold
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect)]
pub struct TickConfig {
    /// Main simulation tick rate (Hz)
    pub tick_rate: u32,

    /// Physics substep rate (Hz) - should be >= tick_rate
    pub physics_rate: u32,

    /// Replication rate for owned entities (Hz)
    pub owned_replication_rate: u32,

    /// Replication rate for remote entities (Hz)
    pub remote_replication_rate: u32,

    /// Fallback rate when network is congested (Hz)
    pub fallback_rate: u32,

    /// RTT threshold to trigger fallback (ms)
    pub fallback_rtt_threshold_ms: u32,
}

impl Default for TickConfig {
    fn default() -> Self {
        Self {
            tick_rate: 120,
            physics_rate: 120,
            owned_replication_rate: 120,
            remote_replication_rate: 60,
            fallback_rate: 30,
            fallback_rtt_threshold_ms: 150,
        }
    }
}

impl TickConfig {
    /// Get tick duration for Lightyear
    pub fn tick_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.tick_rate as f64)
    }

    /// Get physics timestep
    pub fn physics_timestep(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.physics_rate as f64)
    }
}

// ============================================================================
// Transport Configuration
// ============================================================================

/// Transport layer configuration (QUIC/TLS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Server bind address
    pub server_addr: SocketAddr,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// Connection timeout (seconds)
    pub connection_timeout_secs: u32,

    /// Keep-alive interval (seconds)
    pub keepalive_secs: u32,

    /// Maximum packet size (bytes)
    pub max_packet_size: usize,

    /// Enable TLS (always true for production)
    pub use_tls: bool,

    /// Server name for TLS (SNI)
    pub server_name: String,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:4433".parse().unwrap(),
            max_connections: 250,
            connection_timeout_secs: 30,
            keepalive_secs: 5,
            max_packet_size: 1400, // MTU-safe
            use_tls: true,
            server_name: "eustress.local".to_string(),
        }
    }
}

// ============================================================================
// Replication Configuration
// ============================================================================

/// Replication and AOI (Area of Interest) settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect, Resource)]
pub struct ReplicationConfig {
    /// AOI radius in studs (entities beyond this aren't replicated)
    pub aoi_radius: f32,

    /// AOI hysteresis (extra radius before removing from interest)
    pub aoi_hysteresis: f32,

    /// Grid cell size for spatial hashing (studs)
    pub grid_cell_size: f32,

    /// Maximum entities per player (bandwidth limit)
    pub max_entities_per_player: usize,

    /// Delta compression threshold (don't send if change < this)
    pub delta_threshold: f32,

    /// Full state sync interval (ticks) - periodic full sync
    pub full_sync_interval: u32,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            aoi_radius: 200.0,           // 200 studs = 56m
            aoi_hysteresis: 20.0,        // 20 stud buffer
            grid_cell_size: 32.0,        // 32x32 stud cells
            max_entities_per_player: 500,
            delta_threshold: 0.01,       // 0.01 stud minimum change
            full_sync_interval: 600,     // Every 5 seconds at 120Hz
        }
    }
}

// ============================================================================
// Ownership Configuration
// ============================================================================

/// Entity ownership and transfer settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect, Resource)]
pub struct OwnershipConfig {
    /// Ownership request timeout (ms)
    pub request_timeout_ms: u32,

    /// Minimum time between ownership transfers (ms)
    pub transfer_cooldown_ms: u32,

    /// Maximum distance to request ownership (studs)
    pub max_request_distance: f32,

    /// Prefer lower-ping clients for ownership
    pub prefer_low_ping: bool,

    /// Auto-release ownership after inactivity (seconds, 0 = never)
    pub auto_release_secs: u32,
    
    /// Gradual handoff duration for smooth physics transfer (ms, 0 = instant)
    pub gradual_handoff_ms: u32,
}

impl Default for OwnershipConfig {
    fn default() -> Self {
        Self {
            request_timeout_ms: 500,
            transfer_cooldown_ms: 100,
            max_request_distance: 50.0,
            prefer_low_ping: true,
            auto_release_secs: 30,
            gradual_handoff_ms: 1500, // 1.5 second gradual handoff
        }
    }
}

// ============================================================================
// Anti-Exploit Configuration
// ============================================================================

/// Anti-exploit and validation settings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Reflect, Resource)]
pub struct AntiExploitConfig {
    /// Maximum speed before flagging (studs/s)
    pub max_speed: f32,

    /// Maximum acceleration before flagging (studs/s²)
    pub max_acceleration: f32,

    /// Maximum teleport distance per tick (studs)
    pub max_teleport_distance: f32,

    /// Input queue size limit (prevent spam)
    pub max_input_queue: usize,

    /// Violation threshold before kick
    pub violation_threshold: u32,

    /// Violation decay rate (per second)
    pub violation_decay: f32,

    /// Enable server-side physics validation
    pub validate_physics: bool,
}

impl Default for AntiExploitConfig {
    fn default() -> Self {
        Self {
            max_speed: 100.0,            // 100 studs/s (28 m/s)
            max_acceleration: 200.0,     // 200 studs/s²
            max_teleport_distance: 5.0,  // 5 studs per tick max
            max_input_queue: 256,
            violation_threshold: 10,
            violation_decay: 1.0,        // 1 violation/second decay
            validate_physics: true,
        }
    }
}

// ============================================================================
// Runtime State
// ============================================================================

/// Current network state (runtime, not serialized).
#[derive(Resource, Debug, Clone, Default)]
pub struct NetworkState {
    /// Current tick number
    pub tick: u64,

    /// Connected client count
    pub client_count: usize,

    /// Average RTT across all clients (ms)
    pub avg_rtt_ms: f32,

    /// Current replication rate (may be reduced under load)
    pub current_replication_rate: u32,

    /// Bytes sent this second
    pub bytes_sent: u64,

    /// Bytes received this second
    pub bytes_received: u64,

    /// Is in fallback mode (high RTT)
    pub fallback_mode: bool,
}

