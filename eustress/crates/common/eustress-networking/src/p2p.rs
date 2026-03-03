//! # Peer-to-Peer Networking for Persistent Distributed Worlds
//!
//! CRDT-based state synchronization using Loro for conflict-free merging,
//! with QUIC transport via Quinn/bevy_quinnet.
//!
//! ## Table of Contents
//!
//! 1. **P2PSyncConfig** - Configuration for P2P sync behavior
//! 2. **PeerId / PeerInfo** - Peer identity and connection state
//! 3. **HostManager** - Host election and peer management
//! 4. **ChunkId / ChunkRegistry / DistributedChunk** - Spatial chunking for distributed worlds
//! 5. **SyncMessage** - Network messages for CRDT sync
//! 6. **DistributedWorldPlugin** - Main Bevy plugin
//!
//! ## Architecture
//!
//! - Each peer maintains a Loro document (CRDT) for shared state
//! - World is divided into spatial chunks; each chunk has a designated host
//! - Changes are merged conflict-free across all peers via Loro
//! - Quinn provides QUIC transport with built-in encryption
//! - bevy_quinnet integrates Quinn into the Bevy event loop

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use tracing::{info, warn};

// ============================================================================
// Peer Identity
// ============================================================================

/// Unique identifier for a peer in the distributed network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Reflect)]
pub struct PeerId(pub u64);

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({})", self.0)
    }
}

/// Information about a connected peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer identifier
    pub id: PeerId,
    /// Peer address
    pub addr: SocketAddr,
    /// Connection state
    pub state: PeerConnectionState,
    /// Round-trip time in milliseconds
    pub rtt_ms: u32,
    /// Last heartbeat timestamp
    pub last_heartbeat: f64,
    /// Chunks this peer is hosting
    pub hosted_chunks: Vec<ChunkId>,
}

/// Connection state for a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerConnectionState {
    /// Attempting to connect
    Connecting,
    /// Connected and syncing CRDT state
    Syncing,
    /// Fully connected and operational
    Connected,
    /// Disconnected
    Disconnected,
}

// ============================================================================
// P2P Sync Configuration
// ============================================================================

/// Configuration for peer-to-peer CRDT synchronization.
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct P2PSyncConfig {
    /// Local listening address
    pub listen_addr: SocketAddr,
    /// Maximum number of peers
    pub max_peers: usize,
    /// Heartbeat interval in milliseconds
    pub heartbeat_ms: u64,
    /// Connection timeout in seconds
    pub timeout_secs: u32,
    /// Enable automatic peer discovery
    pub auto_discovery: bool,
    /// Chunk size in studs (spatial subdivision for host assignment)
    pub chunk_size: f32,
    /// Sync frequency in hertz (how often CRDT updates are broadcast)
    pub sync_hz: u32,
}

impl Default for P2PSyncConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:4434".parse().unwrap(),
            max_peers: 16,
            heartbeat_ms: 1000,
            timeout_secs: 10,
            auto_discovery: false,
            chunk_size: 256.0,
            sync_hz: 20,
        }
    }
}

// ============================================================================
// Spatial Chunking
// ============================================================================

/// Identifier for a spatial chunk in the distributed world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub struct ChunkId {
    /// Chunk X coordinate (chunk_size units)
    pub x: i32,
    /// Chunk Y coordinate (chunk_size units)
    pub y: i32,
    /// Chunk Z coordinate (chunk_size units)
    pub z: i32,
}

impl ChunkId {
    /// Create a new chunk ID from grid coordinates.
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Compute chunk ID from world position and chunk size.
    pub fn from_world_position(position: Vec3, chunk_size: f32) -> Self {
        Self {
            x: (position.x / chunk_size).floor() as i32,
            y: (position.y / chunk_size).floor() as i32,
            z: (position.z / chunk_size).floor() as i32,
        }
    }

    /// Get the world-space center of this chunk.
    pub fn center(&self, chunk_size: f32) -> Vec3 {
        Vec3::new(
            (self.x as f32 + 0.5) * chunk_size,
            (self.y as f32 + 0.5) * chunk_size,
            (self.z as f32 + 0.5) * chunk_size,
        )
    }
}

impl std::fmt::Display for ChunkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chunk({},{},{})", self.x, self.y, self.z)
    }
}

/// Component marking an entity as belonging to a distributed chunk.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct DistributedChunk {
    /// Which chunk this entity belongs to
    pub chunk_id: ChunkId,
    /// The peer currently hosting (authoritative for) this chunk
    pub host: PeerId,
    /// Whether this entity's state is synced via CRDT
    pub synced: bool,
}

/// Resource tracking all active chunks and their host assignments.
#[derive(Resource, Debug, Default)]
pub struct ChunkRegistry {
    /// Map from chunk ID to hosting peer
    pub chunk_hosts: HashMap<ChunkId, PeerId>,
    /// Active chunks (have entities or nearby players)
    pub active_chunks: Vec<ChunkId>,
}

impl ChunkRegistry {
    /// Assign a host to a chunk.
    pub fn assign_host(&mut self, chunk_id: ChunkId, host: PeerId) {
        self.chunk_hosts.insert(chunk_id, host);
        if !self.active_chunks.contains(&chunk_id) {
            self.active_chunks.push(chunk_id);
        }
        info!("Chunk {} assigned to {}", chunk_id, host);
    }

    /// Get the host for a chunk, if assigned.
    pub fn get_host(&self, chunk_id: &ChunkId) -> Option<&PeerId> {
        self.chunk_hosts.get(chunk_id)
    }

    /// Remove a chunk from active tracking.
    pub fn deactivate_chunk(&mut self, chunk_id: &ChunkId) {
        self.chunk_hosts.remove(chunk_id);
        self.active_chunks.retain(|c| c != chunk_id);
    }

    /// Reassign all chunks hosted by a given peer (e.g., after disconnect).
    pub fn reassign_from_peer(&mut self, peer: PeerId) -> Vec<ChunkId> {
        let orphaned: Vec<ChunkId> = self
            .chunk_hosts
            .iter()
            .filter(|(_, &host)| host == peer)
            .map(|(&chunk_id, _)| chunk_id)
            .collect();
        for chunk_id in &orphaned {
            self.chunk_hosts.remove(chunk_id);
        }
        orphaned
    }
}

// ============================================================================
// Host Manager
// ============================================================================

/// Resource managing host election and peer connections.
#[derive(Resource, Debug, Default)]
pub struct HostManager {
    /// Connected peers indexed by peer ID
    pub peers: HashMap<PeerId, PeerInfo>,
    /// Local peer ID
    pub local_id: PeerId,
    /// Next peer ID to assign
    next_id: u64,
}

impl HostManager {
    /// Add a new peer connection.
    pub fn add_peer(&mut self, addr: SocketAddr) -> PeerId {
        let id = PeerId(self.next_id);
        self.next_id += 1;
        self.peers.insert(id, PeerInfo {
            id,
            addr,
            state: PeerConnectionState::Connecting,
            rtt_ms: 0,
            last_heartbeat: 0.0,
            hosted_chunks: Vec::new(),
        });
        info!("Added {} at {}", id, addr);
        id
    }

    /// Remove a peer connection.
    pub fn remove_peer(&mut self, id: PeerId) -> Option<PeerInfo> {
        let peer = self.peers.remove(&id);
        if peer.is_some() {
            info!("Removed {}", id);
        }
        peer
    }

    /// Get a peer by ID.
    pub fn get_peer(&self, id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(id)
    }

    /// Get all connected peer IDs.
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .iter()
            .filter(|(_, p)| p.state == PeerConnectionState::Connected)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Elect a host for a chunk (lowest RTT peer near the chunk).
    pub fn elect_host(&self, _chunk_id: &ChunkId) -> Option<PeerId> {
        // Simple strategy: pick the connected peer with lowest RTT
        self.peers
            .values()
            .filter(|p| p.state == PeerConnectionState::Connected)
            .min_by_key(|p| p.rtt_ms)
            .map(|p| p.id)
    }
}

// ============================================================================
// Sync Messages
// ============================================================================

/// Network messages for CRDT state synchronization between peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Full state snapshot (sent on initial connection)
    FullSync {
        /// Serialized Loro document snapshot
        snapshot: Vec<u8>,
    },
    /// Incremental CRDT update
    DeltaSync {
        /// Source peer
        from: PeerId,
        /// Serialized Loro update operations
        data: Vec<u8>,
    },
    /// Chunk host assignment notification
    ChunkAssignment {
        /// Chunk being assigned
        chunk_id: ChunkId,
        /// New host peer
        host: PeerId,
    },
    /// Heartbeat / keep-alive
    Heartbeat {
        /// Sender peer
        from: PeerId,
        /// Sender's current timestamp
        timestamp: f64,
    },
    /// Peer join notification
    PeerJoined {
        /// New peer
        peer_id: PeerId,
        /// Peer's address
        addr: SocketAddr,
    },
    /// Peer leave notification
    PeerLeft {
        /// Departing peer
        peer_id: PeerId,
        /// Reason for departure
        reason: String,
    },
}

// ============================================================================
// CRDT Document State
// ============================================================================

/// Resource holding the shared CRDT document for conflict-free state merging.
#[derive(Resource)]
pub struct SharedDocument {
    /// Loro document for CRDT-based state synchronization
    pub document: loro::LoroDoc,
    /// Pending operations to broadcast to peers
    pub pending_ops: Vec<Vec<u8>>,
}

impl Default for SharedDocument {
    fn default() -> Self {
        Self {
            document: loro::LoroDoc::new(),
            pending_ops: Vec::new(),
        }
    }
}

impl SharedDocument {
    /// Export pending changes as bytes for network transmission.
    pub fn export_updates(&mut self) -> Option<Vec<u8>> {
        if self.pending_ops.is_empty() {
            return None;
        }
        // Collect all pending ops into a single update
        let updates: Vec<u8> = self.pending_ops.drain(..).flatten().collect();
        Some(updates)
    }

    /// Import updates received from a peer.
    pub fn import_updates(&mut self, data: &[u8]) -> Result<(), String> {
        self.document
            .import(data)
            .map(|_| ())
            .map_err(|e| format!("Failed to import CRDT update: {}", e))
    }
}

// ============================================================================
// Events
// ============================================================================

/// Message to connect to a peer.
#[derive(Message, Debug)]
pub struct ConnectToPeer {
    pub addr: SocketAddr,
}

/// Message to disconnect from a peer.
#[derive(Message, Debug)]
pub struct DisconnectPeer {
    pub peer_id: PeerId,
    pub reason: String,
}

/// Message when a peer connects.
#[derive(Event, Message, Debug, Clone)]
pub struct PeerConnected {
    pub peer_id: PeerId,
    pub addr: SocketAddr,
}

/// Message when a peer disconnects.
#[derive(Event, Message, Debug, Clone)]
pub struct PeerDisconnected {
    pub peer_id: PeerId,
    pub reason: String,
}

// ============================================================================
// Systems
// ============================================================================

/// Process peer heartbeats and detect timeouts.
fn process_heartbeats(
    time: Res<Time>,
    config: Res<P2PSyncConfig>,
    mut manager: ResMut<HostManager>,
) {
    let timeout = config.timeout_secs as f64;
    let current_time = time.elapsed_secs_f64();

    let timed_out: Vec<PeerId> = manager
        .peers
        .iter()
        .filter(|(_, p)| {
            p.state == PeerConnectionState::Connected
                && (current_time - p.last_heartbeat) > timeout
        })
        .map(|(&id, _)| id)
        .collect();

    for id in timed_out {
        warn!("{} timed out", id);
        if let Some(peer) = manager.peers.get_mut(&id) {
            peer.state = PeerConnectionState::Disconnected;
        }
    }
}

/// Broadcast pending CRDT updates to all connected peers.
fn broadcast_crdt_updates(
    mut _document: ResMut<SharedDocument>,
    _manager: Res<HostManager>,
) {
    // Export pending updates and send to all connected peers via Quinn
    // Implementation will integrate with bevy_quinnet for actual transport
}

/// Reassign chunks from disconnected peers.
fn reassign_orphaned_chunks(
    mut registry: ResMut<ChunkRegistry>,
    manager: Res<HostManager>,
) {
    // Find disconnected peers that still host chunks
    let disconnected: Vec<PeerId> = manager
        .peers
        .iter()
        .filter(|(_, p)| p.state == PeerConnectionState::Disconnected)
        .map(|(&id, _)| id)
        .collect();

    for peer_id in disconnected {
        let orphaned = registry.reassign_from_peer(peer_id);
        for chunk_id in orphaned {
            // Elect a new host
            if let Some(new_host) = manager.elect_host(&chunk_id) {
                registry.assign_host(chunk_id, new_host);
            } else {
                warn!("No available host for orphaned chunk {}", chunk_id);
            }
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for distributed world networking with CRDT state synchronization.
///
/// Provides spatial chunking, host election, and conflict-free state merging
/// via Loro CRDTs over QUIC transport.
pub struct DistributedWorldPlugin;

impl Plugin for DistributedWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<P2PSyncConfig>()
            .init_resource::<HostManager>()
            .init_resource::<ChunkRegistry>()
            .init_resource::<SharedDocument>()
            .add_message::<ConnectToPeer>()
            .add_message::<DisconnectPeer>()
            .add_message::<PeerConnected>()
            .add_message::<PeerDisconnected>()
            .register_type::<ChunkId>()
            .register_type::<DistributedChunk>()
            .add_systems(Update, (
                process_heartbeats,
                broadcast_crdt_updates,
                reassign_orphaned_chunks,
            ));

        info!("Distributed World P2P plugin initialized");
    }
}
