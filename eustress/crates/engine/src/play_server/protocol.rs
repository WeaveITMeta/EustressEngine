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
