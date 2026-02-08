//! # Teleport Networking
//!
//! Multi-server state transfer and matchmaking for cross-server teleportation.
//!
//! ## Table of Contents
//!
//! 1. **PlayerState** - Serializable player data for transfer
//! 2. **MatchmakingService** - Find/create servers with criteria
//! 3. **TeleportGateway** - Route players between servers
//! 4. **RegionRouter** - Cross-region routing with latency awareness
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   Client    │────▶│   Gateway   │────▶│   Target    │
//! │   Server    │     │   Service   │     │   Server    │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!       │                    │                   │
//!       │ 1. Serialize      │ 2. Route          │ 3. Deserialize
//!       │    PlayerState    │    + Reserve      │    + Spawn
//!       ▼                    ▼                   ▼
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use super::teleport::{PlaceId, ServerId, MatchmakingCriteria, TeleportData, TeleportError, TeleportResult};

// ============================================================================
// Player State Serialization
// ============================================================================

/// Complete player state for cross-server transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    /// Player's unique ID
    pub player_id: u64,
    /// Display name
    pub display_name: String,
    /// Position in world
    pub position: [f32; 3],
    /// Rotation (quaternion)
    pub rotation: [f32; 4],
    /// Velocity
    pub velocity: [f32; 3],
    /// Health and stats
    pub stats: PlayerStats,
    /// Inventory items
    pub inventory: PlayerInventory,
    /// Active buffs/debuffs
    pub effects: Vec<ActiveEffect>,
    /// Teleport-specific data
    pub teleport_data: TeleportData,
    /// Timestamp of serialization
    pub serialized_at: u64,
    /// Checksum for validation
    pub checksum: u32,
}

impl PlayerState {
    /// Create from player entity components
    pub fn from_components(
        player_id: u64,
        display_name: &str,
        position: Vec3,
        rotation: Quat,
        velocity: Vec3,
        stats: PlayerStats,
        inventory: PlayerInventory,
        effects: Vec<ActiveEffect>,
        teleport_data: TeleportData,
    ) -> Self {
        let mut state = Self {
            player_id,
            display_name: display_name.to_string(),
            position: [position.x, position.y, position.z],
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
            velocity: [velocity.x, velocity.y, velocity.z],
            stats,
            inventory,
            effects,
            teleport_data,
            serialized_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            checksum: 0,
        };
        state.checksum = state.compute_checksum();
        state
    }
    
    /// Compute checksum for validation
    fn compute_checksum(&self) -> u32 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        self.player_id.hash(&mut hasher);
        self.display_name.hash(&mut hasher);
        self.serialized_at.hash(&mut hasher);
        (hasher.finish() & 0xFFFFFFFF) as u32
    }
    
    /// Validate checksum
    pub fn validate(&self) -> bool {
        self.checksum == self.compute_checksum()
    }
    
    /// Serialize to bytes (using bincode for efficiency)
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self)
            .map_err(|e| format!("Serialization error: {}", e))
    }
    
    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let state: Self = bincode::deserialize(data)
            .map_err(|e| format!("Deserialization error: {}", e))?;
        
        if !state.validate() {
            return Err("Checksum validation failed".to_string());
        }
        
        Ok(state)
    }
    
    /// Get position as Vec3
    pub fn get_position(&self) -> Vec3 {
        Vec3::new(self.position[0], self.position[1], self.position[2])
    }
    
    /// Get rotation as Quat
    pub fn get_rotation(&self) -> Quat {
        Quat::from_xyzw(
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
            self.rotation[3],
        )
    }
    
    /// Get velocity as Vec3
    pub fn get_velocity(&self) -> Vec3 {
        Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2])
    }
    
    /// Estimate serialized size in bytes
    pub fn estimated_size(&self) -> usize {
        // Base size + inventory items + effects
        256 + self.inventory.items.len() * 64 + self.effects.len() * 32
    }
}

/// Player stats for transfer
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerStats {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
    pub level: u32,
    pub experience: u64,
    pub currency: u64,
}

/// Player inventory for transfer
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerInventory {
    /// Item IDs and quantities
    pub items: Vec<InventoryItem>,
    /// Equipped item slots
    pub equipped: HashMap<String, String>,
    /// Inventory capacity
    pub capacity: u32,
}

/// Single inventory item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub item_id: String,
    pub quantity: u32,
    pub metadata: Option<String>,
}

/// Active buff/debuff effect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveEffect {
    pub effect_id: String,
    pub remaining_secs: f32,
    pub stacks: u32,
    pub source: Option<String>,
}

// ============================================================================
// Matchmaking Service
// ============================================================================

/// Server info from matchmaking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingServer {
    pub server_id: ServerId,
    pub place_id: PlaceId,
    pub region: String,
    pub player_count: u32,
    pub max_players: u32,
    pub ping_ms: u32,
    pub tags: Vec<String>,
    pub score: f32, // Matchmaking score (higher = better match)
}

/// Matchmaking request
#[derive(Debug, Clone)]
pub struct MatchmakingRequest {
    pub id: u64,
    pub place_id: PlaceId,
    pub criteria: MatchmakingCriteria,
    pub player_count: u32,
    pub created_at: Instant,
    pub timeout_secs: u32,
}

/// Matchmaking response
#[derive(Debug, Clone)]
pub enum MatchmakingResponse {
    /// Found a suitable server
    Found(MatchmakingServer),
    /// Creating a new server
    Creating { estimated_wait_secs: u32 },
    /// No servers available
    NoServers,
    /// Request timed out
    Timeout,
    /// Error occurred
    Error(String),
}

/// Matchmaking service for finding/creating servers
#[derive(Resource, Default)]
pub struct MatchmakingService {
    /// Known servers by region
    servers: HashMap<String, Vec<MatchmakingServer>>,
    /// Pending matchmaking requests
    pending_requests: HashMap<u64, MatchmakingRequest>,
    /// Next request ID
    next_request_id: u64,
    /// Region ping cache (region -> ping_ms)
    region_pings: HashMap<String, u32>,
    /// Last server list refresh
    last_refresh: Option<Instant>,
}

impl MatchmakingService {
    /// Request matchmaking for a place
    pub fn request_match(
        &mut self,
        place_id: PlaceId,
        criteria: MatchmakingCriteria,
        player_count: u32,
    ) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        
        self.pending_requests.insert(request_id, MatchmakingRequest {
            id: request_id,
            place_id,
            criteria,
            player_count,
            created_at: Instant::now(),
            timeout_secs: 30,
        });
        
        request_id
    }
    
    /// Poll for matchmaking result
    pub fn poll_result(&mut self, request_id: u64) -> Option<MatchmakingResponse> {
        let request = self.pending_requests.get(&request_id)?;
        
        // Check timeout
        if request.created_at.elapsed().as_secs() > request.timeout_secs as u64 {
            self.pending_requests.remove(&request_id);
            return Some(MatchmakingResponse::Timeout);
        }
        
        // Find matching servers
        let matches = self.find_matching_servers(
            request.place_id,
            &request.criteria,
            request.player_count,
        );
        
        if let Some(best) = matches.first() {
            self.pending_requests.remove(&request_id);
            return Some(MatchmakingResponse::Found(best.clone()));
        }
        
        // No match yet, keep waiting
        None
    }
    
    /// Find servers matching criteria
    fn find_matching_servers(
        &self,
        place_id: PlaceId,
        criteria: &MatchmakingCriteria,
        player_count: u32,
    ) -> Vec<MatchmakingServer> {
        let mut matches: Vec<MatchmakingServer> = Vec::new();
        
        for servers in self.servers.values() {
            for server in servers {
                if server.place_id != place_id {
                    continue;
                }
                
                // Check capacity
                if server.player_count + player_count > server.max_players {
                    continue;
                }
                
                // Check region preference
                if let Some(ref region) = criteria.region {
                    if &server.region != region {
                        continue;
                    }
                }
                
                // Check player count criteria
                if let Some(min) = criteria.min_players {
                    if server.player_count < min {
                        continue;
                    }
                }
                if let Some(max) = criteria.max_players {
                    if server.player_count > max {
                        continue;
                    }
                }
                
                // Check tags
                if !criteria.tags.is_empty() {
                    let has_all_tags = criteria.tags.iter()
                        .all(|t| server.tags.contains(t));
                    if !has_all_tags {
                        continue;
                    }
                }
                
                // Calculate match score
                let mut score = 100.0;
                
                // Prefer lower ping
                score -= server.ping_ms as f32 * 0.1;
                
                // Prefer servers with some players (but not full)
                let fill_ratio = server.player_count as f32 / server.max_players as f32;
                score += (0.5 - (fill_ratio - 0.5).abs()) * 20.0;
                
                let mut matched = server.clone();
                matched.score = score;
                matches.push(matched);
            }
        }
        
        // Sort by score (descending)
        matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        matches
    }
    
    /// Update server list for a region
    pub fn update_servers(&mut self, region: &str, servers: Vec<MatchmakingServer>) {
        self.servers.insert(region.to_string(), servers);
        self.last_refresh = Some(Instant::now());
    }
    
    /// Update ping for a region
    pub fn update_region_ping(&mut self, region: &str, ping_ms: u32) {
        self.region_pings.insert(region.to_string(), ping_ms);
    }
    
    /// Get best region by ping
    pub fn get_best_region(&self) -> Option<&str> {
        self.region_pings.iter()
            .min_by_key(|(_, &ping)| ping)
            .map(|(region, _)| region.as_str())
    }
    
    /// Cancel a pending request
    pub fn cancel_request(&mut self, request_id: u64) {
        self.pending_requests.remove(&request_id);
    }
}

// ============================================================================
// Teleport Gateway
// ============================================================================

/// Teleport gateway for routing players between servers
#[derive(Resource)]
pub struct TeleportGateway {
    /// Gateway endpoint URL
    pub endpoint: String,
    /// Authentication token
    auth_token: Option<String>,
    /// Pending transfers (player_id -> transfer state)
    pending_transfers: HashMap<u64, PendingTransfer>,
    /// Completed transfers waiting for acknowledgment
    completed_transfers: HashMap<u64, CompletedTransfer>,
    /// Transfer timeout
    transfer_timeout: Duration,
}

/// Pending player transfer
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PendingTransfer {
    player_id: u64,
    player_state: PlayerState,
    target_server: ServerId,
    started_at: Instant,
    status: TransferStatus,
}

/// Transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    /// Serializing player state
    Serializing,
    /// Sending to gateway
    Sending,
    /// Gateway routing to target
    Routing,
    /// Target server receiving
    Receiving,
    /// Transfer complete
    Complete,
    /// Transfer failed
    Failed,
}

/// Completed transfer info
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompletedTransfer {
    player_id: u64,
    target_server: ServerId,
    completed_at: Instant,
    transfer_time_ms: u64,
}

impl Default for TeleportGateway {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080/gateway".to_string(),
            auth_token: None,
            pending_transfers: HashMap::new(),
            completed_transfers: HashMap::new(),
            transfer_timeout: Duration::from_secs(30),
        }
    }
}

impl TeleportGateway {
    /// Create with custom endpoint
    pub fn with_endpoint(endpoint: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            ..Default::default()
        }
    }
    
    /// Set authentication token
    pub fn set_auth_token(&mut self, token: &str) {
        self.auth_token = Some(token.to_string());
    }
    
    /// Initiate player transfer to target server
    pub fn transfer_player(
        &mut self,
        player_state: PlayerState,
        target_server: ServerId,
    ) -> TeleportResult<()> {
        let player_id = player_state.player_id;
        
        // Check if already transferring
        if self.pending_transfers.contains_key(&player_id) {
            return Err(TeleportError::TeleportInProgress);
        }
        
        // Validate state
        if !player_state.validate() {
            return Err(TeleportError::NetworkError("Invalid player state".to_string()));
        }
        
        self.pending_transfers.insert(player_id, PendingTransfer {
            player_id,
            player_state,
            target_server,
            started_at: Instant::now(),
            status: TransferStatus::Serializing,
        });
        
        info!("Initiated transfer for player {} to server", player_id);
        
        Ok(())
    }
    
    /// Get transfer status
    pub fn get_transfer_status(&self, player_id: u64) -> Option<TransferStatus> {
        self.pending_transfers.get(&player_id).map(|t| t.status)
    }
    
    /// Cancel a pending transfer
    pub fn cancel_transfer(&mut self, player_id: u64) -> bool {
        self.pending_transfers.remove(&player_id).is_some()
    }
    
    /// Process pending transfers (call each frame)
    pub fn update(&mut self) {
        let mut completed = Vec::new();
        let mut failed = Vec::new();
        
        for (&player_id, transfer) in self.pending_transfers.iter_mut() {
            // Check timeout
            if transfer.started_at.elapsed() > self.transfer_timeout {
                failed.push(player_id);
                continue;
            }
            
            // Simulate transfer progress
            // In real implementation, this would be async HTTP/WebSocket calls
            match transfer.status {
                TransferStatus::Serializing => {
                    // Serialization is instant
                    transfer.status = TransferStatus::Sending;
                }
                TransferStatus::Sending => {
                    // Simulate network delay
                    if transfer.started_at.elapsed().as_millis() > 100 {
                        transfer.status = TransferStatus::Routing;
                    }
                }
                TransferStatus::Routing => {
                    // Simulate gateway routing
                    if transfer.started_at.elapsed().as_millis() > 300 {
                        transfer.status = TransferStatus::Receiving;
                    }
                }
                TransferStatus::Receiving => {
                    // Simulate target server receiving
                    if transfer.started_at.elapsed().as_millis() > 500 {
                        transfer.status = TransferStatus::Complete;
                        completed.push((player_id, transfer.target_server.clone(), 
                            transfer.started_at.elapsed().as_millis() as u64));
                    }
                }
                _ => {}
            }
        }
        
        // Handle completed transfers
        for (player_id, target_server, transfer_time_ms) in completed {
            self.pending_transfers.remove(&player_id);
            self.completed_transfers.insert(player_id, CompletedTransfer {
                player_id,
                target_server,
                completed_at: Instant::now(),
                transfer_time_ms,
            });
            info!("Transfer complete for player {} ({}ms)", player_id, transfer_time_ms);
        }
        
        // Handle failed transfers
        for player_id in failed {
            self.pending_transfers.remove(&player_id);
            warn!("Transfer failed for player {} (timeout)", player_id);
        }
        
        // Clean up old completed transfers
        self.completed_transfers.retain(|_, t| {
            t.completed_at.elapsed().as_secs() < 60
        });
    }
    
    /// Acknowledge a completed transfer (removes from completed list)
    pub fn acknowledge_transfer(&mut self, player_id: u64) -> Option<CompletedTransfer> {
        self.completed_transfers.remove(&player_id)
    }
    
    /// Receive incoming player transfer (called on target server)
    pub fn receive_transfer(&self, data: &[u8]) -> TeleportResult<PlayerState> {
        PlayerState::from_bytes(data)
            .map_err(|e| TeleportError::NetworkError(e))
    }
}

// ============================================================================
// Region Router
// ============================================================================

/// Region configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionConfig {
    pub id: String,
    pub name: String,
    pub gateway_url: String,
    pub latitude: f32,
    pub longitude: f32,
}

/// Cross-region router with latency awareness
#[derive(Resource, Default)]
pub struct RegionRouter {
    /// Available regions
    regions: HashMap<String, RegionConfig>,
    /// Region ping measurements (region_id -> ping_ms)
    pings: HashMap<String, u32>,
    /// Current region
    current_region: Option<String>,
    /// Last ping measurement time
    last_ping_check: Option<Instant>,
    /// Ping check interval
    ping_interval: Duration,
}

impl RegionRouter {
    /// Create with default regions
    pub fn with_default_regions() -> Self {
        let mut router = Self {
            ping_interval: Duration::from_secs(60),
            ..Default::default()
        };
        
        // Add default regions
        router.add_region(RegionConfig {
            id: "us-west".to_string(),
            name: "US West".to_string(),
            gateway_url: "https://us-west.gateway.example.com".to_string(),
            latitude: 37.7749,
            longitude: -122.4194,
        });
        
        router.add_region(RegionConfig {
            id: "us-east".to_string(),
            name: "US East".to_string(),
            gateway_url: "https://us-east.gateway.example.com".to_string(),
            latitude: 40.7128,
            longitude: -74.0060,
        });
        
        router.add_region(RegionConfig {
            id: "eu-central".to_string(),
            name: "EU Central".to_string(),
            gateway_url: "https://eu-central.gateway.example.com".to_string(),
            latitude: 50.1109,
            longitude: 8.6821,
        });
        
        router.add_region(RegionConfig {
            id: "asia-east".to_string(),
            name: "Asia East".to_string(),
            gateway_url: "https://asia-east.gateway.example.com".to_string(),
            latitude: 35.6762,
            longitude: 139.6503,
        });
        
        router
    }
    
    /// Add a region
    pub fn add_region(&mut self, config: RegionConfig) {
        self.regions.insert(config.id.clone(), config);
    }
    
    /// Update ping for a region
    pub fn update_ping(&mut self, region_id: &str, ping_ms: u32) {
        self.pings.insert(region_id.to_string(), ping_ms);
    }
    
    /// Get ping for a region
    pub fn get_ping(&self, region_id: &str) -> Option<u32> {
        self.pings.get(region_id).copied()
    }
    
    /// Get best region by ping
    pub fn get_best_region(&self) -> Option<&RegionConfig> {
        self.pings.iter()
            .min_by_key(|(_, &ping)| ping)
            .and_then(|(id, _)| self.regions.get(id))
    }
    
    /// Get region by ID
    pub fn get_region(&self, region_id: &str) -> Option<&RegionConfig> {
        self.regions.get(region_id)
    }
    
    /// Set current region
    pub fn set_current_region(&mut self, region_id: &str) {
        if self.regions.contains_key(region_id) {
            self.current_region = Some(region_id.to_string());
        }
    }
    
    /// Get current region
    pub fn get_current_region(&self) -> Option<&RegionConfig> {
        self.current_region.as_ref()
            .and_then(|id| self.regions.get(id))
    }
    
    /// Check if cross-region teleport (different regions)
    pub fn is_cross_region(&self, target_region: &str) -> bool {
        self.current_region.as_ref()
            .map(|current| current != target_region)
            .unwrap_or(true)
    }
    
    /// Get estimated latency for cross-region teleport
    pub fn estimate_cross_region_latency(&self, target_region: &str) -> u32 {
        let current_ping = self.current_region.as_ref()
            .and_then(|id| self.pings.get(id))
            .copied()
            .unwrap_or(50);
        
        let target_ping = self.pings.get(target_region)
            .copied()
            .unwrap_or(100);
        
        // Estimate: current ping + target ping + routing overhead
        current_ping + target_ping + 50
    }
    
    /// Should we warn about high latency?
    pub fn should_warn_latency(&self, target_region: &str) -> bool {
        self.estimate_cross_region_latency(target_region) > 200
    }
    
    /// Get all regions sorted by ping
    pub fn get_regions_by_ping(&self) -> Vec<(&RegionConfig, u32)> {
        let mut regions: Vec<_> = self.regions.values()
            .map(|r| (r, self.pings.get(&r.id).copied().unwrap_or(999)))
            .collect();
        
        regions.sort_by_key(|(_, ping)| *ping);
        regions
    }
    
    /// Check if ping measurements are stale
    pub fn needs_ping_refresh(&self) -> bool {
        self.last_ping_check
            .map(|t| t.elapsed() > self.ping_interval)
            .unwrap_or(true)
    }
    
    /// Mark ping check as done
    pub fn mark_ping_checked(&mut self) {
        self.last_ping_check = Some(Instant::now());
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event: Player transfer initiated
#[derive(Message, Debug, Clone)]
pub struct TransferInitiatedEvent {
    pub player_id: u64,
    pub target_server: ServerId,
    pub target_region: Option<String>,
}

/// Event: Player transfer completed
#[derive(Message, Debug, Clone)]
pub struct TransferCompletedEvent {
    pub player_id: u64,
    pub target_server: ServerId,
    pub transfer_time_ms: u64,
}

/// Event: Player transfer failed
#[derive(Message, Debug, Clone)]
pub struct TransferFailedEvent {
    pub player_id: u64,
    pub error: String,
}

/// Event: Incoming player from transfer
#[derive(Message, Debug, Clone)]
pub struct IncomingPlayerEvent {
    pub player_state: PlayerState,
    pub source_server: ServerId,
}

// ============================================================================
// Systems
// ============================================================================

/// Update teleport gateway
fn update_gateway(mut gateway: ResMut<TeleportGateway>) {
    gateway.update();
}

/// Ping regions periodically
fn ping_regions(
    mut router: ResMut<RegionRouter>,
    // In real implementation, would use async HTTP client
) {
    if !router.needs_ping_refresh() {
        return;
    }
    
    // Simulate ping measurements
    // In real implementation, this would be async HTTP HEAD requests
    router.update_ping("us-west", 45);
    router.update_ping("us-east", 65);
    router.update_ping("eu-central", 120);
    router.update_ping("asia-east", 180);
    
    router.mark_ping_checked();
}

// ============================================================================
// Plugin
// ============================================================================

/// Teleport networking plugin
pub struct TeleportNetworkingPlugin;

impl Plugin for TeleportNetworkingPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<MatchmakingService>()
            .init_resource::<TeleportGateway>()
            .insert_resource(RegionRouter::with_default_regions())
            .add_message::<TransferInitiatedEvent>()
            .add_message::<TransferCompletedEvent>()
            .add_message::<TransferFailedEvent>()
            .add_message::<IncomingPlayerEvent>()
            .add_systems(Update, (
                update_gateway,
                ping_regions,
            ));
        
        info!("TeleportNetworkingPlugin initialized");
    }
}
