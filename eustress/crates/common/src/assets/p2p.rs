//! # P2P Asset Distribution
//!
//! WebRTC-based peer-to-peer asset sharing as fallback for CDN.
//!
//! ## Table of Contents
//!
//! 1. **PeerManager** - Peer discovery and connection management
//! 2. **ChunkTransfer** - Chunk-based asset transfer protocol
//! 3. **SignalingClient** - WebRTC signaling server communication
//! 4. **PeerHealth** - Peer health monitoring and scoring
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   Peer A    │◀───▶│  Signaling  │◀───▶│   Peer B    │
//! │  (seeder)   │     │   Server    │     │  (leecher)  │
//! └──────┬──────┘     └─────────────┘     └──────┬──────┘
//!        │                                        │
//!        │         WebRTC Data Channel           │
//!        └───────────────────────────────────────┘
//!                    Chunk Transfer
//! ```
//!
//! ## Flow
//!
//! 1. Client requests asset from CDN
//! 2. If CDN fails, check P2P network for peers with asset
//! 3. Connect to peers via WebRTC signaling
//! 4. Download chunks from multiple peers in parallel
//! 5. Verify chunks via hash, assemble asset
//! 6. Become seeder for other peers

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

#[allow(unused_imports)]
use super::{ContentHash, ResolveError};

// ============================================================================
// P2P Types
// ============================================================================

/// Unique peer identifier
pub type PeerId = String;

/// Chunk index within an asset
pub type ChunkIndex = u32;

/// P2P configuration
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// Signaling server URL
    pub signaling_url: String,
    /// STUN server URLs
    pub stun_servers: Vec<String>,
    /// TURN server URLs (with credentials)
    pub turn_servers: Vec<TurnServer>,
    /// Maximum concurrent peer connections
    pub max_peers: usize,
    /// Chunk size in bytes
    pub chunk_size: usize,
    /// Maximum parallel chunk downloads
    pub max_parallel_chunks: usize,
    /// Peer connection timeout
    pub connection_timeout: Duration,
    /// Chunk request timeout
    pub chunk_timeout: Duration,
    /// Enable P2P (can be disabled)
    pub enabled: bool,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            signaling_url: "wss://signal.example.com".to_string(),
            stun_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            turn_servers: Vec::new(),
            max_peers: 5,
            chunk_size: 64 * 1024, // 64KB chunks
            max_parallel_chunks: 4,
            connection_timeout: Duration::from_secs(10),
            chunk_timeout: Duration::from_secs(30),
            enabled: true,
        }
    }
}

/// TURN server configuration
#[derive(Debug, Clone)]
pub struct TurnServer {
    pub url: String,
    pub username: String,
    pub credential: String,
}

// ============================================================================
// Peer Management
// ============================================================================

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Discovering peer
    Discovering,
    /// Connecting via signaling
    Connecting,
    /// WebRTC handshake in progress
    Handshaking,
    /// Connected and ready
    Connected,
    /// Disconnected
    Disconnected,
    /// Connection failed
    Failed,
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Unique peer ID
    pub id: PeerId,
    /// Connection state
    pub state: PeerState,
    /// Assets this peer has (content hashes)
    pub available_assets: HashSet<String>,
    /// Peer health score (0.0-1.0)
    pub health_score: f32,
    /// Average download speed from this peer (bytes/sec)
    pub avg_speed: f32,
    /// Number of successful chunk transfers
    pub successful_transfers: u32,
    /// Number of failed chunk transfers
    pub failed_transfers: u32,
    /// Last activity time
    pub last_activity: Instant,
    /// Round-trip time in ms
    pub rtt_ms: u32,
}

impl PeerInfo {
    pub fn new(id: PeerId) -> Self {
        Self {
            id,
            state: PeerState::Discovering,
            available_assets: HashSet::new(),
            health_score: 0.5,
            avg_speed: 0.0,
            successful_transfers: 0,
            failed_transfers: 0,
            last_activity: Instant::now(),
            rtt_ms: 0,
        }
    }
    
    /// Update health score based on transfer results
    pub fn update_health(&mut self) {
        let total = self.successful_transfers + self.failed_transfers;
        if total > 0 {
            let success_rate = self.successful_transfers as f32 / total as f32;
            // Blend with speed factor
            let speed_factor = (self.avg_speed / 1_000_000.0).min(1.0); // Normalize to 1MB/s
            self.health_score = success_rate * 0.7 + speed_factor * 0.3;
        }
    }
    
    /// Check if peer is healthy enough to use
    pub fn is_healthy(&self) -> bool {
        self.state == PeerState::Connected && self.health_score > 0.3
    }
    
    /// Check if peer is idle (no recent activity)
    pub fn is_idle(&self, threshold: Duration) -> bool {
        self.last_activity.elapsed() > threshold
    }
}

/// Peer manager - handles peer discovery and connections
#[derive(Resource)]
pub struct PeerManager {
    /// Configuration
    pub config: P2PConfig,
    /// Our peer ID
    pub local_peer_id: PeerId,
    /// Connected peers
    peers: HashMap<PeerId, PeerInfo>,
    /// Assets we're seeding (content hash -> chunk count)
    seeding: HashMap<String, u32>,
    /// Pending peer connections
    pending_connections: HashMap<PeerId, Instant>,
    /// Blacklisted peers (temporary)
    blacklist: HashMap<PeerId, Instant>,
}

impl Default for PeerManager {
    fn default() -> Self {
        Self {
            config: P2PConfig::default(),
            local_peer_id: format!("peer_{:016x}", rand::random::<u64>()),
            peers: HashMap::new(),
            seeding: HashMap::new(),
            pending_connections: HashMap::new(),
            blacklist: HashMap::new(),
        }
    }
}

impl PeerManager {
    /// Create with custom config
    pub fn with_config(config: P2PConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }
    
    /// Add a discovered peer
    pub fn add_peer(&mut self, peer_id: PeerId) {
        if peer_id == self.local_peer_id {
            return; // Don't add ourselves
        }
        
        if self.blacklist.contains_key(&peer_id) {
            return; // Blacklisted
        }
        
        if !self.peers.contains_key(&peer_id) {
            self.peers.insert(peer_id.clone(), PeerInfo::new(peer_id));
        }
    }
    
    /// Update peer state
    pub fn update_peer_state(&mut self, peer_id: &str, state: PeerState) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = state;
            peer.last_activity = Instant::now();
        }
    }
    
    /// Update peer's available assets
    pub fn update_peer_assets(&mut self, peer_id: &str, assets: HashSet<String>) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.available_assets = assets;
            peer.last_activity = Instant::now();
        }
    }
    
    /// Record successful chunk transfer
    pub fn record_success(&mut self, peer_id: &str, bytes: usize, duration: Duration) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.successful_transfers += 1;
            peer.last_activity = Instant::now();
            
            // Update average speed
            let speed = bytes as f32 / duration.as_secs_f32();
            peer.avg_speed = peer.avg_speed * 0.8 + speed * 0.2; // Exponential moving average
            
            peer.update_health();
        }
    }
    
    /// Record failed chunk transfer
    pub fn record_failure(&mut self, peer_id: &str) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.failed_transfers += 1;
            peer.last_activity = Instant::now();
            peer.update_health();
            
            // Blacklist if too many failures
            if peer.failed_transfers > 5 && peer.health_score < 0.2 {
                self.blacklist_peer(peer_id);
            }
        }
    }
    
    /// Blacklist a peer temporarily
    pub fn blacklist_peer(&mut self, peer_id: &str) {
        self.peers.remove(peer_id);
        self.blacklist.insert(peer_id.to_string(), Instant::now());
        warn!("Blacklisted peer: {}", peer_id);
    }
    
    /// Get peers that have a specific asset
    pub fn get_peers_with_asset(&self, content_hash: &str) -> Vec<&PeerInfo> {
        self.peers.values()
            .filter(|p| p.is_healthy() && p.available_assets.contains(content_hash))
            .collect()
    }
    
    /// Get best peers for downloading (sorted by health)
    pub fn get_best_peers(&self, content_hash: &str, count: usize) -> Vec<&PeerInfo> {
        let mut peers = self.get_peers_with_asset(content_hash);
        peers.sort_by(|a, b| b.health_score.partial_cmp(&a.health_score).unwrap());
        peers.into_iter().take(count).collect()
    }
    
    /// Start seeding an asset
    pub fn start_seeding(&mut self, content_hash: &str, chunk_count: u32) {
        self.seeding.insert(content_hash.to_string(), chunk_count);
    }
    
    /// Stop seeding an asset
    pub fn stop_seeding(&mut self, content_hash: &str) {
        self.seeding.remove(content_hash);
    }
    
    /// Get list of assets we're seeding
    pub fn get_seeding_assets(&self) -> Vec<&str> {
        self.seeding.keys().map(|s| s.as_str()).collect()
    }
    
    /// Check if we're seeding an asset
    pub fn is_seeding(&self, content_hash: &str) -> bool {
        self.seeding.contains_key(content_hash)
    }
    
    /// Clean up stale peers and blacklist entries
    pub fn cleanup(&mut self) {
        let idle_threshold = Duration::from_secs(300); // 5 minutes
        let blacklist_duration = Duration::from_secs(600); // 10 minutes
        
        // Remove idle peers
        self.peers.retain(|_, p| !p.is_idle(idle_threshold));
        
        // Remove expired blacklist entries
        self.blacklist.retain(|_, t| t.elapsed() < blacklist_duration);
        
        // Remove expired pending connections
        self.pending_connections.retain(|_, t| t.elapsed() < self.config.connection_timeout);
    }
    
    /// Get connected peer count
    pub fn connected_count(&self) -> usize {
        self.peers.values().filter(|p| p.state == PeerState::Connected).count()
    }
    
    /// Get total peer count
    pub fn total_count(&self) -> usize {
        self.peers.len()
    }
}

// ============================================================================
// Chunk Transfer
// ============================================================================

/// Chunk metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    /// Asset content hash
    pub content_hash: String,
    /// Chunk index
    pub index: ChunkIndex,
    /// Chunk size in bytes
    pub size: usize,
    /// Chunk hash for verification
    pub hash: String,
}

/// Chunk request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRequest {
    /// Asset content hash
    pub content_hash: String,
    /// Requested chunk indices
    pub chunks: Vec<ChunkIndex>,
    /// Requester peer ID
    pub requester: PeerId,
}

/// Chunk response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkResponse {
    /// Asset content hash
    pub content_hash: String,
    /// Chunk index
    pub index: ChunkIndex,
    /// Chunk data (base64 encoded for JSON transport)
    pub data: String,
    /// Chunk hash for verification
    pub hash: String,
}

/// Chunk download state
#[derive(Debug, Clone)]
pub struct ChunkDownload {
    /// Asset content hash
    pub content_hash: String,
    /// Total chunks needed
    pub total_chunks: u32,
    /// Chunks we have
    pub completed_chunks: HashSet<ChunkIndex>,
    /// Chunks in progress (chunk_index -> (peer_id, started_at))
    pub in_progress: HashMap<ChunkIndex, (PeerId, Instant)>,
    /// Chunks to request
    pub pending: VecDeque<ChunkIndex>,
    /// Downloaded chunk data
    pub data: HashMap<ChunkIndex, Vec<u8>>,
    /// Download started at
    pub started_at: Instant,
    /// Total bytes downloaded
    pub bytes_downloaded: usize,
}

impl ChunkDownload {
    pub fn new(content_hash: &str, total_chunks: u32) -> Self {
        let pending: VecDeque<ChunkIndex> = (0..total_chunks).collect();
        
        Self {
            content_hash: content_hash.to_string(),
            total_chunks,
            completed_chunks: HashSet::new(),
            in_progress: HashMap::new(),
            pending,
            data: HashMap::new(),
            started_at: Instant::now(),
            bytes_downloaded: 0,
        }
    }
    
    /// Get next chunk to request
    pub fn next_chunk(&mut self) -> Option<ChunkIndex> {
        self.pending.pop_front()
    }
    
    /// Mark chunk as in progress
    pub fn start_chunk(&mut self, index: ChunkIndex, peer_id: PeerId) {
        self.in_progress.insert(index, (peer_id, Instant::now()));
    }
    
    /// Complete a chunk
    pub fn complete_chunk(&mut self, index: ChunkIndex, data: Vec<u8>) {
        self.bytes_downloaded += data.len();
        self.completed_chunks.insert(index);
        self.in_progress.remove(&index);
        self.data.insert(index, data);
    }
    
    /// Fail a chunk (re-queue for retry)
    pub fn fail_chunk(&mut self, index: ChunkIndex) {
        self.in_progress.remove(&index);
        self.pending.push_back(index);
    }
    
    /// Check for timed out chunks
    pub fn check_timeouts(&mut self, timeout: Duration) -> Vec<(ChunkIndex, PeerId)> {
        let mut timed_out = Vec::new();
        
        for (&index, (peer_id, started)) in &self.in_progress {
            if started.elapsed() > timeout {
                timed_out.push((index, peer_id.clone()));
            }
        }
        
        for (index, _) in &timed_out {
            self.fail_chunk(*index);
        }
        
        timed_out
    }
    
    /// Check if download is complete
    pub fn is_complete(&self) -> bool {
        self.completed_chunks.len() == self.total_chunks as usize
    }
    
    /// Get progress (0.0-1.0)
    pub fn progress(&self) -> f32 {
        self.completed_chunks.len() as f32 / self.total_chunks as f32
    }
    
    /// Get download speed (bytes/sec)
    pub fn speed(&self) -> f32 {
        let elapsed = self.started_at.elapsed().as_secs_f32();
        if elapsed > 0.0 {
            self.bytes_downloaded as f32 / elapsed
        } else {
            0.0
        }
    }
    
    /// Assemble final asset data
    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        
        let mut result = Vec::new();
        for i in 0..self.total_chunks {
            if let Some(chunk) = self.data.get(&i) {
                result.extend_from_slice(chunk);
            } else {
                return None;
            }
        }
        
        Some(result)
    }
}

/// Chunk transfer manager
#[derive(Resource, Default)]
pub struct ChunkTransferManager {
    /// Active downloads
    downloads: HashMap<String, ChunkDownload>,
    /// Chunk upload queue (for seeding)
    upload_queue: VecDeque<ChunkResponse>,
    /// Stats
    pub total_downloaded: u64,
    pub total_uploaded: u64,
}

impl ChunkTransferManager {
    /// Start downloading an asset
    pub fn start_download(&mut self, content_hash: &str, total_chunks: u32) {
        if !self.downloads.contains_key(content_hash) {
            self.downloads.insert(
                content_hash.to_string(),
                ChunkDownload::new(content_hash, total_chunks),
            );
        }
    }
    
    /// Get download state
    pub fn get_download(&self, content_hash: &str) -> Option<&ChunkDownload> {
        self.downloads.get(content_hash)
    }
    
    /// Get mutable download state
    pub fn get_download_mut(&mut self, content_hash: &str) -> Option<&mut ChunkDownload> {
        self.downloads.get_mut(content_hash)
    }
    
    /// Complete a download
    pub fn complete_download(&mut self, content_hash: &str) -> Option<Vec<u8>> {
        if let Some(download) = self.downloads.remove(content_hash) {
            self.total_downloaded += download.bytes_downloaded as u64;
            download.assemble()
        } else {
            None
        }
    }
    
    /// Cancel a download
    pub fn cancel_download(&mut self, content_hash: &str) {
        self.downloads.remove(content_hash);
    }
    
    /// Queue chunk for upload
    pub fn queue_upload(&mut self, response: ChunkResponse) {
        self.upload_queue.push_back(response);
    }
    
    /// Get next upload
    pub fn next_upload(&mut self) -> Option<ChunkResponse> {
        self.upload_queue.pop_front()
    }
    
    /// Record uploaded bytes
    pub fn record_upload(&mut self, bytes: usize) {
        self.total_uploaded += bytes as u64;
    }
}

// ============================================================================
// Signaling
// ============================================================================

/// Signaling message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    /// Announce our presence and assets
    Announce {
        peer_id: PeerId,
        assets: Vec<String>,
    },
    /// Request peers with specific asset
    FindPeers {
        content_hash: String,
    },
    /// Response with peers that have asset
    PeersFound {
        content_hash: String,
        peers: Vec<PeerId>,
    },
    /// WebRTC offer
    Offer {
        from: PeerId,
        to: PeerId,
        sdp: String,
    },
    /// WebRTC answer
    Answer {
        from: PeerId,
        to: PeerId,
        sdp: String,
    },
    /// ICE candidate
    IceCandidate {
        from: PeerId,
        to: PeerId,
        candidate: String,
    },
    /// Peer disconnected
    Disconnect {
        peer_id: PeerId,
    },
}

/// Signaling client state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingState {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// Signaling client for WebRTC coordination
#[derive(Resource)]
pub struct SignalingClient {
    /// Server URL
    pub url: String,
    /// Connection state
    pub state: SignalingState,
    /// Our peer ID
    pub peer_id: PeerId,
    /// Outgoing message queue
    outgoing: VecDeque<SignalingMessage>,
    /// Incoming message queue
    incoming: VecDeque<SignalingMessage>,
    /// Last heartbeat
    last_heartbeat: Option<Instant>,
    /// Heartbeat interval
    heartbeat_interval: Duration,
}

impl Default for SignalingClient {
    fn default() -> Self {
        Self {
            url: "wss://signal.example.com".to_string(),
            state: SignalingState::Disconnected,
            peer_id: format!("peer_{:016x}", rand::random::<u64>()),
            outgoing: VecDeque::new(),
            incoming: VecDeque::new(),
            last_heartbeat: None,
            heartbeat_interval: Duration::from_secs(30),
        }
    }
}

impl SignalingClient {
    /// Create with URL
    pub fn with_url(url: &str) -> Self {
        Self {
            url: url.to_string(),
            ..Default::default()
        }
    }
    
    /// Connect to signaling server
    pub fn connect(&mut self) {
        if self.state == SignalingState::Disconnected {
            self.state = SignalingState::Connecting;
            info!("Connecting to signaling server: {}", self.url);
            // In real implementation, this would open WebSocket connection
        }
    }
    
    /// Disconnect from signaling server
    pub fn disconnect(&mut self) {
        self.state = SignalingState::Disconnected;
        self.outgoing.clear();
        self.incoming.clear();
    }
    
    /// Send a message
    pub fn send(&mut self, message: SignalingMessage) {
        self.outgoing.push_back(message);
    }
    
    /// Announce our assets
    pub fn announce(&mut self, assets: Vec<String>) {
        self.send(SignalingMessage::Announce {
            peer_id: self.peer_id.clone(),
            assets,
        });
    }
    
    /// Find peers with asset
    pub fn find_peers(&mut self, content_hash: &str) {
        self.send(SignalingMessage::FindPeers {
            content_hash: content_hash.to_string(),
        });
    }
    
    /// Send WebRTC offer
    pub fn send_offer(&mut self, to: &str, sdp: &str) {
        self.send(SignalingMessage::Offer {
            from: self.peer_id.clone(),
            to: to.to_string(),
            sdp: sdp.to_string(),
        });
    }
    
    /// Send WebRTC answer
    pub fn send_answer(&mut self, to: &str, sdp: &str) {
        self.send(SignalingMessage::Answer {
            from: self.peer_id.clone(),
            to: to.to_string(),
            sdp: sdp.to_string(),
        });
    }
    
    /// Send ICE candidate
    pub fn send_ice_candidate(&mut self, to: &str, candidate: &str) {
        self.send(SignalingMessage::IceCandidate {
            from: self.peer_id.clone(),
            to: to.to_string(),
            candidate: candidate.to_string(),
        });
    }
    
    /// Get next outgoing message
    pub fn next_outgoing(&mut self) -> Option<SignalingMessage> {
        self.outgoing.pop_front()
    }
    
    /// Queue incoming message
    pub fn queue_incoming(&mut self, message: SignalingMessage) {
        self.incoming.push_back(message);
    }
    
    /// Get next incoming message
    pub fn next_incoming(&mut self) -> Option<SignalingMessage> {
        self.incoming.pop_front()
    }
    
    /// Check if heartbeat needed
    pub fn needs_heartbeat(&self) -> bool {
        self.last_heartbeat
            .map(|t| t.elapsed() > self.heartbeat_interval)
            .unwrap_or(true)
    }
    
    /// Mark heartbeat sent
    pub fn mark_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
    }
}

// ============================================================================
// P2P Asset Resolver
// ============================================================================

/// P2P asset fetch result
pub enum P2PFetchResult {
    /// Asset data retrieved
    Success(Vec<u8>),
    /// Download in progress
    InProgress { progress: f32, speed: f32 },
    /// No peers available
    NoPeers,
    /// Download failed
    Failed(String),
}

/// Fetch an asset via P2P
pub fn fetch_asset_p2p(
    content_hash: &str,
    peer_manager: &PeerManager,
    transfer_manager: &mut ChunkTransferManager,
    config: &P2PConfig,
) -> P2PFetchResult {
    if !config.enabled {
        return P2PFetchResult::NoPeers;
    }
    
    // Check if download already in progress
    if let Some(download) = transfer_manager.get_download(content_hash) {
        let is_complete = download.is_complete();
        let progress = download.progress();
        let speed = download.speed();
        
        if is_complete {
            if let Some(data) = transfer_manager.complete_download(content_hash) {
                return P2PFetchResult::Success(data);
            }
        }
        return P2PFetchResult::InProgress { progress, speed };
    }
    
    // Find peers with this asset
    let peers = peer_manager.get_best_peers(content_hash, config.max_peers);
    
    if peers.is_empty() {
        return P2PFetchResult::NoPeers;
    }
    
    // Start download
    // In real implementation, we'd query peers for chunk count
    let estimated_chunks = 10; // Placeholder
    transfer_manager.start_download(content_hash, estimated_chunks);
    
    P2PFetchResult::InProgress {
        progress: 0.0,
        speed: 0.0,
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event: Peer discovered
#[derive(Message, Debug, Clone)]
pub struct PeerDiscoveredEvent {
    pub peer_id: PeerId,
    pub assets: Vec<String>,
}

/// Event: Peer connected
#[derive(Message, Debug, Clone)]
pub struct PeerConnectedEvent {
    pub peer_id: PeerId,
}

/// Event: Peer disconnected
#[derive(Message, Debug, Clone)]
pub struct PeerDisconnectedEvent {
    pub peer_id: PeerId,
}

/// Event: Chunk received
#[derive(Message, Debug, Clone)]
pub struct ChunkReceivedEvent {
    pub content_hash: String,
    pub chunk_index: ChunkIndex,
    pub peer_id: PeerId,
}

/// Event: Asset download complete
#[derive(Message, Debug, Clone)]
pub struct P2PDownloadCompleteEvent {
    pub content_hash: String,
    pub size: usize,
    pub duration_ms: u64,
}

// ============================================================================
// Systems
// ============================================================================

/// Update P2P systems
fn update_p2p(
    mut peer_manager: ResMut<PeerManager>,
    mut transfer_manager: ResMut<ChunkTransferManager>,
    mut signaling: ResMut<SignalingClient>,
) {
    // Cleanup stale peers
    peer_manager.cleanup();
    
    // Process signaling messages
    while let Some(message) = signaling.next_incoming() {
        match message {
            SignalingMessage::Announce { peer_id, assets } => {
                peer_manager.add_peer(peer_id.clone());
                peer_manager.update_peer_assets(&peer_id, assets.into_iter().collect());
            }
            SignalingMessage::PeersFound { content_hash: _, peers } => {
                for peer_id in peers {
                    peer_manager.add_peer(peer_id);
                }
            }
            SignalingMessage::Disconnect { peer_id } => {
                peer_manager.update_peer_state(&peer_id, PeerState::Disconnected);
            }
            _ => {
                // WebRTC signaling handled elsewhere
            }
        }
    }
    
    // Check for chunk timeouts
    let timeout = peer_manager.config.chunk_timeout;
    for download in transfer_manager.downloads.values_mut() {
        let timed_out = download.check_timeouts(timeout);
        for (_, peer_id) in timed_out {
            peer_manager.record_failure(&peer_id);
        }
    }
}

/// Announce seeding assets periodically
fn announce_assets(
    peer_manager: Res<PeerManager>,
    mut signaling: ResMut<SignalingClient>,
) {
    if signaling.state != SignalingState::Connected {
        return;
    }
    
    if signaling.needs_heartbeat() {
        let assets = peer_manager.get_seeding_assets()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        
        signaling.announce(assets);
        signaling.mark_heartbeat();
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// P2P asset distribution plugin
pub struct P2PAssetPlugin;

impl Plugin for P2PAssetPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<PeerManager>()
            .init_resource::<ChunkTransferManager>()
            .init_resource::<SignalingClient>()
            .add_message::<PeerDiscoveredEvent>()
            .add_message::<PeerConnectedEvent>()
            .add_message::<PeerDisconnectedEvent>()
            .add_message::<ChunkReceivedEvent>()
            .add_message::<P2PDownloadCompleteEvent>()
            .add_systems(Update, (
                update_p2p,
                announce_assets,
            ));
        
        info!("P2PAssetPlugin initialized");
    }
}
