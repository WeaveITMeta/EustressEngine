// ============================================================================
// Play Server - Network Protocol
// ============================================================================

use super::client::PlayerInput;
use super::replication::ReplicatedComponents;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Message channel types for prioritization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageChannel {
    /// Reliable ordered - for important state changes
    ReliableOrdered,
    /// Reliable unordered - for events that must arrive but order doesn't matter
    ReliableUnordered,
    /// Unreliable - for frequent updates like position (latest wins)
    Unreliable,
}

/// Game message types sent between server and clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GameMessage {
    // === Connection Messages ===
    /// Client joining the server
    Join {
        player_name: String,
    },
    /// Server accepting join
    JoinAccepted {
        session_id: u64,
        server_tick: u64,
    },
    /// Server rejecting join
    JoinRejected {
        reason: String,
    },
    /// Client or server disconnect
    Disconnect,
    /// Heartbeat ping
    Ping {
        timestamp: u64,
    },
    /// Heartbeat pong
    Pong {
        timestamp: u64,
        server_tick: u64,
    },
    
    // === Player Messages ===
    /// Player input from client
    PlayerInput(PlayerInput),
    /// Player spawned (server -> clients)
    PlayerSpawned {
        session_id: u64,
        player_name: String,
        entity_id: u64,
        position: [f32; 3],
        rotation: [f32; 4],
    },
    /// Player despawned (server -> clients)
    PlayerDespawned {
        session_id: u64,
    },
    /// Chat message
    ChatMessage {
        text: String,
    },
    /// Chat broadcast (server -> clients)
    ChatBroadcast {
        session_id: u64,
        player_name: String,
        text: String,
    },
    
    // === Replication Messages ===
    /// Entity replication update
    Replication(ReplicationMessage),
    /// Full world state snapshot
    WorldSnapshot {
        tick: u64,
        entities: Vec<EntitySnapshot>,
    },
    /// Delta update since last ack
    WorldDelta {
        base_tick: u64,
        current_tick: u64,
        spawned: Vec<EntitySnapshot>,
        updated: Vec<EntityUpdate>,
        despawned: Vec<u64>,
    },
    /// Client acknowledging received tick
    AckTick {
        tick: u64,
    },
    
    // === Physics Messages ===
    /// Physics authority transfer
    PhysicsAuthority {
        entity_id: u64,
        owner_session: Option<u64>,
    },
    /// Physics state correction
    PhysicsCorrection {
        entity_id: u64,
        position: [f32; 3],
        rotation: [f32; 4],
        linear_velocity: [f32; 3],
        angular_velocity: [f32; 3],
    },
    
    // === Script Messages ===
    /// Remote event (Soul script)
    RemoteEvent {
        event_name: String,
        args: Vec<u8>, // Serialized arguments
    },
    /// Remote function call
    RemoteFunction {
        call_id: u64,
        function_name: String,
        args: Vec<u8>,
    },
    /// Remote function return
    RemoteFunctionReturn {
        call_id: u64,
        result: Vec<u8>,
    },
}

impl GameMessage {
    /// Get the channel this message should use
    pub fn channel(&self) -> MessageChannel {
        match self {
            // Connection messages are reliable ordered
            Self::Join { .. } |
            Self::JoinAccepted { .. } |
            Self::JoinRejected { .. } |
            Self::Disconnect |
            Self::PlayerSpawned { .. } |
            Self::PlayerDespawned { .. } => MessageChannel::ReliableOrdered,
            
            // Chat is reliable but order within chat is important
            Self::ChatMessage { .. } |
            Self::ChatBroadcast { .. } => MessageChannel::ReliableOrdered,
            
            // Replication can be unreliable (latest state wins)
            Self::Replication(_) |
            Self::WorldDelta { .. } => MessageChannel::Unreliable,
            
            // Full snapshots must be reliable
            Self::WorldSnapshot { .. } => MessageChannel::ReliableOrdered,
            
            // Input is unreliable (we want latest)
            Self::PlayerInput(_) => MessageChannel::Unreliable,
            
            // Heartbeats are unreliable
            Self::Ping { .. } |
            Self::Pong { .. } |
            Self::AckTick { .. } => MessageChannel::Unreliable,
            
            // Physics corrections are reliable
            Self::PhysicsAuthority { .. } |
            Self::PhysicsCorrection { .. } => MessageChannel::ReliableOrdered,
            
            // Script messages are reliable
            Self::RemoteEvent { .. } |
            Self::RemoteFunction { .. } |
            Self::RemoteFunctionReturn { .. } => MessageChannel::ReliableOrdered,
        }
    }
    
    /// Serialize message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
    
    /// Deserialize message from bytes
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

/// Replication message types
#[derive(Debug, Clone, Serialize, Deserialize, bevy::prelude::Event)]
pub enum ReplicationMessage {
    /// Entity spawned
    EntitySpawn {
        network_id: u64,
        class_name: String,
        name: String,
        parent_id: Option<u64>,
        transform: Transform,
        components: Option<ReplicatedComponents>,
    },
    /// Entity updated
    EntityUpdate {
        network_id: u64,
        transform: Transform,
        components: Option<ReplicatedComponents>,
    },
    /// Entity despawned
    EntityDespawn {
        network_id: u64,
    },
    /// Property changed
    PropertyChange {
        network_id: u64,
        property_name: String,
        value: Vec<u8>, // Serialized value
    },
    /// Attribute changed
    AttributeChange {
        network_id: u64,
        attribute_name: String,
        value: Vec<u8>,
    },
}

/// Full entity snapshot for world state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub network_id: u64,
    pub class_name: String,
    pub name: String,
    pub parent_id: Option<u64>,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub components: Vec<u8>, // Serialized component data
}

/// Entity update (delta)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityUpdate {
    pub network_id: u64,
    pub position: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<[f32; 3]>,
    pub components: Option<Vec<u8>>,
}

impl EntitySnapshot {
    /// Create from Bevy entity
    pub fn from_entity(
        network_id: u64,
        name: &str,
        class_name: &str,
        parent_id: Option<u64>,
        transform: &Transform,
        components: &[u8],
    ) -> Self {
        Self {
            network_id,
            class_name: class_name.to_string(),
            name: name.to_string(),
            parent_id,
            position: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
            components: components.to_vec(),
        }
    }
    
    /// Convert to Transform
    pub fn to_transform(&self) -> Transform {
        Transform {
            translation: Vec3::from_array(self.position),
            rotation: Quat::from_array(self.rotation),
            scale: Vec3::from_array(self.scale),
        }
    }
}

/// Server tick rate configuration
#[derive(Debug, Clone)]
pub struct TickConfig {
    /// Ticks per second
    pub tick_rate: u32,
    /// Snapshot send rate (every N ticks)
    pub snapshot_interval: u32,
    /// Delta send rate (every N ticks)
    pub delta_interval: u32,
}

impl Default for TickConfig {
    fn default() -> Self {
        Self {
            tick_rate: 60,
            snapshot_interval: 600, // Every 10 seconds
            delta_interval: 3,      // 20 times per second
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// NetworkTickRate — validated server sync frequencies
// ─────────────────────────────────────────────────────────────────────────────

/// Validated server tick rate. Clients must sustain at least `min_client_fps()`
/// to stay in sync; the server remains authoritative regardless.
///
/// | Variant  | Hz  | Interval  | Bandwidth multiplier vs 24 Hz |
/// |----------|-----|-----------|-------------------------------|
/// | `Hz24`   | 24  | 41.7 ms   | 1×  (minimum viable)          |
/// | `Hz60`   | 60  | 16.7 ms   | 2.5× (recommended)            |
/// | `Hz144`  | 144 | 6.9 ms    | 6×  (competitive / physics)   |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkTickRate {
    /// 24 Hz — minimum viable sync. Suitable for slow-paced or low-bandwidth worlds.
    Hz24,
    /// 60 Hz — standard. Recommended for most worlds. **(default)**
    #[default]
    Hz60,
    /// 144 Hz — maximum. Competitive play and physics-intensive worlds.
    Hz144,
}

impl NetworkTickRate {
    /// Ticks per second.
    pub fn hz(self) -> u32 {
        match self {
            Self::Hz24  => 24,
            Self::Hz60  => 60,
            Self::Hz144 => 144,
        }
    }

    /// Duration of one tick in seconds.
    pub fn tick_dt(self) -> f32 {
        1.0 / self.hz() as f32
    }

    /// Minimum client FPS required to stay synchronised.
    pub fn min_client_fps(self) -> u32 {
        self.hz()
    }

    /// Replication interval in ticks for a given priority.
    ///
    /// Intervals are chosen so the effective **Hz is the same** regardless of
    /// tick rate — e.g. `Normal` always replicates at ~10 Hz:
    ///
    /// | Priority | Target Hz | Hz24 interval | Hz60 interval | Hz144 interval |
    /// |----------|-----------|---------------|---------------|----------------|
    /// | Critical | full rate | 1             | 1             | 1              |
    /// | High     | ~30 Hz    | 1             | 2             | 5              |
    /// | Normal   | ~10 Hz    | 2             | 6             | 14             |
    /// | Low      | ~2 Hz     | 12            | 30            | 72             |
    pub fn replication_interval(self, priority: ReplicationPriority) -> u32 {
        let hz = self.hz() as f32;
        match priority {
            ReplicationPriority::Critical => 1,
            ReplicationPriority::High     => (hz / 30.0).round().max(1.0) as u32,
            ReplicationPriority::Normal   => (hz / 10.0).round().max(1.0) as u32,
            ReplicationPriority::Low      => (hz / 2.0 ).round().max(1.0) as u32,
        }
    }

    /// Upstream per-player bandwidth in bytes/s (48-byte input packet).
    pub fn upstream_bps(self) -> u32 {
        48 * self.hz()
    }

    /// Downstream per-player bandwidth in bytes/s for `visible` nearby players
    /// (56-byte entity update per visible entity per tick).
    pub fn downstream_bps(self, visible: u32) -> u32 {
        56 * visible * self.hz()
    }

    /// Maximum players given a total outbound bandwidth cap (bytes/s) and
    /// average visible player count.
    ///
    /// ```
    /// use eustress_engine::play_server::protocol::NetworkTickRate;
    /// let max = NetworkTickRate::Hz60.max_players(100_000_000 / 8, 20);
    /// assert!(max > 1_000);
    /// ```
    pub fn max_players(self, bandwidth_bytes_per_sec: u64, visible: u32) -> u64 {
        let per_player = self.downstream_bps(visible) as u64;
        if per_player == 0 { return u64::MAX; }
        bandwidth_bytes_per_sec / per_player
    }

    /// **Auto-select** the appropriate tick rate based on the nature of the
    /// content being replicated.
    ///
    /// Rules (highest match wins):
    /// - Player characters, projectiles, vehicles → `Hz144`
    /// - Standard interactive objects, NPCs → `Hz60`
    /// - Environmental / ambient / physics simulation → `Hz24`
    ///
    /// Pass an optional measured RTT (ms) to cap the rate when the link is
    /// too slow — if RTT > 40 ms the rate is capped at `Hz24`.
    pub fn auto(class: TickRateClass, rtt_ms: Option<u32>) -> Self {
        let rate = match class {
            TickRateClass::Competitive  => Self::Hz144,
            TickRateClass::Interactive  => Self::Hz60,
            TickRateClass::Environmental => Self::Hz24,
        };
        // Cap when the link can't sustain full-rate updates.
        if let Some(rtt) = rtt_ms {
            if rtt > 40 {
                return rate.min(Self::Hz24);
            } else if rtt > 16 {
                return rate.min(Self::Hz60);
            }
        }
        rate
    }

    fn min(self, other: Self) -> Self {
        if self.hz() <= other.hz() { self } else { other }
    }
}

/// Content class used by `NetworkTickRate::auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickRateClass {
    /// Player characters, projectiles, vehicles, combat entities.
    /// Target: 144 Hz (6.9 ms interval).
    Competitive,
    /// Standard interactive objects, NPCs, doors, interactables.
    /// Target: 60 Hz (16.7 ms interval).
    Interactive,
    /// Environmental entities: terrain details, particles, ambient objects,
    /// simulation results, AI world model data.
    /// Target: 24 Hz (41.7 ms interval).
    Environmental,
}

/// Replication priority — also used by `NetworkTickRate::replication_interval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplicationPriority {
    /// Static / ambient objects (~2 Hz at 60 tick).
    Low,
    /// Most game objects (~10 Hz at 60 tick).
    #[default]
    Normal,
    /// Player characters, important objects (~30 Hz at 60 tick).
    High,
    /// Always replicate every tick.
    Critical,
}

impl ReplicationPriority {
    /// Fixed interval in ticks at the **default 60 Hz** tick rate.
    /// For tick-rate-aware intervals, use `NetworkTickRate::replication_interval`.
    pub fn interval(self) -> u32 {
        match self {
            Self::Low      => 30,
            Self::Normal   => 6,
            Self::High     => 2,
            Self::Critical => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_message_serialization() {
        let msg = GameMessage::Join {
            player_name: "TestPlayer".to_string(),
        };
        
        let bytes = msg.serialize();
        let decoded = GameMessage::deserialize(&bytes).unwrap();
        
        match decoded {
            GameMessage::Join { player_name } => {
                assert_eq!(player_name, "TestPlayer");
            }
            _ => panic!("Wrong message type"),
        }
    }
    
    #[test]
    fn test_entity_snapshot() {
        let transform = Transform::from_xyz(1.0, 2.0, 3.0);
        let snapshot = EntitySnapshot::from_entity(
            42,
            "TestEntity",
            "Part",
            None,
            &transform,
            &[],
        );
        
        assert_eq!(snapshot.network_id, 42);
        assert_eq!(snapshot.name, "TestEntity");
        
        let restored = snapshot.to_transform();
        assert_eq!(restored.translation, transform.translation);
    }
}
