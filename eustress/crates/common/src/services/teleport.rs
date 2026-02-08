//! # Teleport Service
//!
//! Cross-server travel and place teleportation.
//! Inspired by Roblox TeleportService but with modern improvements.
//!
//! ## Table of Contents
//!
//! 1. **TeleportDestination** - Where to teleport (place, server, reserved)
//! 2. **PartyTeleport** - Atomic group teleportation with reservation
//! 3. **ServerReservation** - Reserve server slots before teleporting
//! 4. **TeleportService** - Main service resource
//!
//! ## Features
//!
//! - **Cross-Server Travel**: Move players between game servers
//! - **Place Teleport**: Teleport to different places/scenes
//! - **Party System**: Teleport groups together atomically
//! - **Server Reservation**: Reserve slots before party teleport
//! - **Teleport Data**: Pass data between servers
//! - **Matchmaking**: Find/create servers with specific criteria
//! - **Countdown/Confirmation**: UI-friendly party teleport flow
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Teleport to another place
//! teleport_service.teleport(player, place_id, None);
//!
//! // Teleport with data
//! teleport_service.teleport_with_data(player, place_id, TeleportData {
//!     spawn_location: "BossArena",
//!     custom: json!({ "from_quest": "dragon_slayer" }),
//! });
//!
//! // Teleport party together (atomic)
//! let party_id = teleport_service.create_party_teleport(players, destination)?;
//! // Players confirm...
//! teleport_service.confirm_party_member(party_id, player)?;
//! // Once all confirmed or timeout, teleport executes
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use tracing::info;

// ============================================================================
// Teleport Types
// ============================================================================

/// Unique identifier for a place/scene
pub type PlaceId = u64;

/// Unique identifier for a server instance
pub type ServerId = String;

/// Teleport destination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeleportDestination {
    /// Teleport to a specific place (any server)
    Place(PlaceId),
    
    /// Teleport to a specific server instance
    Server { place_id: PlaceId, server_id: ServerId },
    
    /// Teleport to a reserved server (private)
    ReservedServer { place_id: PlaceId, access_code: String },
    
    /// Teleport to matchmaking (find best server)
    Matchmaking { place_id: PlaceId, criteria: MatchmakingCriteria },
}

/// Matchmaking criteria for server selection
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchmakingCriteria {
    /// Preferred region (e.g., "us-west", "eu-central")
    pub region: Option<String>,
    /// Minimum players in server
    pub min_players: Option<u32>,
    /// Maximum players in server
    pub max_players: Option<u32>,
    /// Custom tags to match
    pub tags: Vec<String>,
    /// Prefer friends' servers
    pub prefer_friends: bool,
}

/// Data passed during teleport
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeleportData {
    /// Spawn location name in destination
    pub spawn_location: Option<String>,
    /// Custom data (serialized JSON)
    pub custom: HashMap<String, String>,
    /// Source place ID
    pub source_place: Option<PlaceId>,
    /// Source server ID
    pub source_server: Option<ServerId>,
}

/// Teleport request status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeleportStatus {
    /// Request pending
    Pending,
    /// Finding server
    FindingServer,
    /// Connecting to server
    Connecting,
    /// Teleport complete
    Complete,
    /// Teleport failed
    Failed,
}

/// Teleport result
pub type TeleportResult<T> = Result<T, TeleportError>;

/// Teleport errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum TeleportError {
    #[error("Place not found: {0}")]
    PlaceNotFound(PlaceId),
    
    #[error("Server not found: {0}")]
    ServerNotFound(ServerId),
    
    #[error("Server full: {server_id} ({current}/{max} players)")]
    ServerFull { server_id: ServerId, current: u32, max: u32 },
    
    #[error("Access denied: {0}")]
    AccessDenied(String),
    
    #[error("Invalid access code")]
    InvalidAccessCode,
    
    #[error("Teleport in progress")]
    TeleportInProgress,
    
    #[error("Rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Teleport cancelled")]
    Cancelled,
}

// ============================================================================
// Server Info
// ============================================================================

/// Information about a game server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Unique server ID
    pub server_id: ServerId,
    /// Place ID this server is running
    pub place_id: PlaceId,
    /// Current player count
    pub player_count: u32,
    /// Maximum players
    pub max_players: u32,
    /// Server region
    pub region: String,
    /// Server tags
    pub tags: Vec<String>,
    /// Is this a reserved (private) server?
    pub is_reserved: bool,
    /// Server age in seconds
    pub age_seconds: u64,
    /// Average ping to this server (if known)
    pub ping_ms: Option<u32>,
}

/// Reserved server access code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservedServerCode {
    /// The access code
    pub code: String,
    /// Place ID
    pub place_id: PlaceId,
    /// Server ID (once created)
    pub server_id: Option<ServerId>,
    /// Expiration timestamp
    pub expires_at: Option<u64>,
}

// ============================================================================
// Teleport Events
// ============================================================================

/// Event: Player teleport requested
#[derive(Message, Debug, Clone)]
pub struct TeleportRequestEvent {
    /// Player entity
    pub player: Entity,
    /// Destination
    pub destination: TeleportDestination,
    /// Teleport data
    pub data: TeleportData,
}

/// Event: Player arriving from teleport
#[derive(Message, Debug, Clone)]
pub struct TeleportArrivalEvent {
    /// Player entity
    pub player: Entity,
    /// Teleport data from source
    pub data: TeleportData,
}

/// Event: Teleport status changed
#[derive(Message, Debug, Clone)]
pub struct TeleportStatusEvent {
    /// Player entity
    pub player: Entity,
    /// New status
    pub status: TeleportStatus,
    /// Error if failed
    pub error: Option<TeleportError>,
}

// ============================================================================
// Teleport Service Resource
// ============================================================================

/// TeleportService - manages cross-server travel
#[derive(Resource)]
pub struct TeleportService {
    /// Current server info
    pub current_server: Option<ServerInfo>,
    /// Pending individual teleports
    pending: HashMap<Entity, PendingTeleport>,
    /// Pending party teleports
    party_teleports: HashMap<PartyTeleportId, PartyTeleport>,
    /// Arrival data for players who just teleported in
    arrivals: HashMap<Entity, TeleportData>,
    /// Reserved server codes created by this server
    reserved_codes: Vec<ReservedServerCode>,
    /// Active server reservations
    reservations: HashMap<ReservationId, ServerReservation>,
    /// Next party teleport ID
    next_party_id: u64,
    /// Next reservation ID
    next_reservation_id: u64,
}

/// Unique ID for a party teleport
pub type PartyTeleportId = u64;

/// Unique ID for a server reservation
pub type ReservationId = u64;

/// Pending individual teleport
#[allow(dead_code)]
struct PendingTeleport {
    destination: TeleportDestination,
    data: TeleportData,
    status: TeleportStatus,
    started_at: Instant,
    /// Party ID if part of a party teleport
    party_id: Option<PartyTeleportId>,
}

/// Party teleport state
#[derive(Debug, Clone)]
pub struct PartyTeleport {
    /// Unique party teleport ID
    pub id: PartyTeleportId,
    /// All players in the party
    pub players: Vec<Entity>,
    /// Destination
    pub destination: TeleportDestination,
    /// Teleport data
    pub data: TeleportData,
    /// Current status
    pub status: PartyTeleportStatus,
    /// Players who have confirmed
    pub confirmed: Vec<Entity>,
    /// Players who have vetoed
    pub vetoed: Vec<Entity>,
    /// When party teleport was created
    pub created_at: Instant,
    /// Countdown duration (seconds)
    pub countdown_secs: u32,
    /// Require all players to confirm?
    pub require_all_confirm: bool,
    /// Minimum confirmation ratio (0.0-1.0)
    pub min_confirm_ratio: f32,
    /// Server reservation (if obtained)
    pub reservation: Option<ReservationId>,
}

/// Party teleport status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartyTeleportStatus {
    /// Waiting for confirmations
    WaitingForConfirmation,
    /// Reserving server slots
    ReservingServer,
    /// Countdown in progress
    Countdown { remaining_secs: u32 },
    /// Teleporting players
    Teleporting,
    /// Complete (all successful)
    Complete,
    /// Partial success (some failed)
    PartialSuccess { succeeded: u32, failed: u32 },
    /// Failed (quorum not met or reservation failed)
    Failed,
    /// Cancelled (vetoed or timeout)
    Cancelled,
}

/// Server reservation for party teleport
#[derive(Debug, Clone)]
pub struct ServerReservation {
    /// Reservation ID
    pub id: ReservationId,
    /// Target server
    pub server_id: ServerId,
    /// Place ID
    pub place_id: PlaceId,
    /// Number of slots reserved
    pub slots: u32,
    /// When reservation was made
    pub created_at: Instant,
    /// Reservation TTL (seconds)
    pub ttl_secs: u32,
    /// Is reservation confirmed by target server?
    pub confirmed: bool,
}

impl ServerReservation {
    /// Check if reservation has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed().as_secs() > self.ttl_secs as u64
    }
}

impl Default for TeleportService {
    fn default() -> Self {
        Self {
            current_server: None,
            pending: HashMap::new(),
            party_teleports: HashMap::new(),
            arrivals: HashMap::new(),
            reserved_codes: Vec::new(),
            reservations: HashMap::new(),
            next_party_id: 1,
            next_reservation_id: 1,
        }
    }
}

impl TeleportService {
    /// Teleport a player to a destination
    pub fn teleport(
        &mut self,
        player: Entity,
        destination: TeleportDestination,
        data: Option<TeleportData>,
    ) -> TeleportResult<()> {
        // Check if already teleporting
        if self.pending.contains_key(&player) {
            return Err(TeleportError::TeleportInProgress);
        }
        
        self.pending.insert(player, PendingTeleport {
            destination,
            data: data.unwrap_or_default(),
            status: TeleportStatus::Pending,
            started_at: std::time::Instant::now(),
            party_id: None,
        });
        
        Ok(())
    }
    
    /// Teleport to a place (convenience method)
    pub fn teleport_to_place(&mut self, player: Entity, place_id: PlaceId) -> TeleportResult<()> {
        self.teleport(player, TeleportDestination::Place(place_id), None)
    }
    
    /// Teleport with custom data
    pub fn teleport_with_data(
        &mut self,
        player: Entity,
        place_id: PlaceId,
        data: TeleportData,
    ) -> TeleportResult<()> {
        self.teleport(player, TeleportDestination::Place(place_id), Some(data))
    }
    
    /// Teleport multiple players together (party) - simple version
    /// For atomic party teleport with confirmation, use create_party_teleport()
    pub fn teleport_party(
        &mut self,
        players: &[Entity],
        destination: TeleportDestination,
    ) -> TeleportResult<PartyTeleportId> {
        self.create_party_teleport(
            players,
            destination,
            None,
            PartyTeleportOptions::default(),
        )
    }
    
    /// Create a party teleport with full options
    pub fn create_party_teleport(
        &mut self,
        players: &[Entity],
        destination: TeleportDestination,
        data: Option<TeleportData>,
        options: PartyTeleportOptions,
    ) -> TeleportResult<PartyTeleportId> {
        if players.is_empty() {
            return Err(TeleportError::AccessDenied("No players in party".to_string()));
        }
        
        // Check if any player is already teleporting
        for &player in players {
            if self.pending.contains_key(&player) {
                return Err(TeleportError::TeleportInProgress);
            }
        }
        
        let party_id = self.next_party_id;
        self.next_party_id += 1;
        
        let party = PartyTeleport {
            id: party_id,
            players: players.to_vec(),
            destination,
            data: data.unwrap_or_default(),
            status: if options.skip_confirmation {
                PartyTeleportStatus::ReservingServer
            } else {
                PartyTeleportStatus::WaitingForConfirmation
            },
            confirmed: if options.skip_confirmation {
                players.to_vec()
            } else {
                Vec::new()
            },
            vetoed: Vec::new(),
            created_at: Instant::now(),
            countdown_secs: options.countdown_secs,
            require_all_confirm: options.require_all_confirm,
            min_confirm_ratio: options.min_confirm_ratio,
            reservation: None,
        };
        
        self.party_teleports.insert(party_id, party);
        
        info!("Created party teleport {} with {} players", party_id, players.len());
        
        Ok(party_id)
    }
    
    /// Confirm participation in a party teleport
    pub fn confirm_party_member(
        &mut self,
        party_id: PartyTeleportId,
        player: Entity,
    ) -> TeleportResult<()> {
        let party = self.party_teleports.get_mut(&party_id)
            .ok_or(TeleportError::AccessDenied("Party not found".to_string()))?;
        
        if !party.players.contains(&player) {
            return Err(TeleportError::AccessDenied("Not in party".to_string()));
        }
        
        if !party.confirmed.contains(&player) {
            party.confirmed.push(player);
            info!("Player {:?} confirmed party teleport {}", player, party_id);
        }
        
        Ok(())
    }
    
    /// Veto a party teleport (cancel for this player)
    pub fn veto_party_teleport(
        &mut self,
        party_id: PartyTeleportId,
        player: Entity,
    ) -> TeleportResult<()> {
        let party = self.party_teleports.get_mut(&party_id)
            .ok_or(TeleportError::AccessDenied("Party not found".to_string()))?;
        
        if !party.players.contains(&player) {
            return Err(TeleportError::AccessDenied("Not in party".to_string()));
        }
        
        if !party.vetoed.contains(&player) {
            party.vetoed.push(player);
            info!("Player {:?} vetoed party teleport {}", player, party_id);
        }
        
        // Check if veto cancels the teleport
        if party.require_all_confirm {
            party.status = PartyTeleportStatus::Cancelled;
        }
        
        Ok(())
    }
    
    /// Get party teleport status
    pub fn get_party_status(&self, party_id: PartyTeleportId) -> Option<&PartyTeleport> {
        self.party_teleports.get(&party_id)
    }
    
    /// Cancel a party teleport
    pub fn cancel_party_teleport(&mut self, party_id: PartyTeleportId) -> bool {
        if let Some(party) = self.party_teleports.get_mut(&party_id) {
            party.status = PartyTeleportStatus::Cancelled;
            
            // Release reservation if any
            if let Some(res_id) = party.reservation {
                self.reservations.remove(&res_id);
            }
            
            true
        } else {
            false
        }
    }
    
    /// Reserve server slots for a party
    pub fn reserve_server_slots(
        &mut self,
        place_id: PlaceId,
        slots: u32,
        ttl_secs: u32,
    ) -> TeleportResult<ReservationId> {
        let res_id = self.next_reservation_id;
        self.next_reservation_id += 1;
        
        // In a real implementation, this would communicate with matchmaking
        // For now, create a placeholder reservation
        let reservation = ServerReservation {
            id: res_id,
            server_id: format!("reserved_{}", res_id),
            place_id,
            slots,
            created_at: Instant::now(),
            ttl_secs,
            confirmed: false, // Would be confirmed by matchmaking response
        };
        
        self.reservations.insert(res_id, reservation);
        
        info!("Created server reservation {} for {} slots", res_id, slots);
        
        Ok(res_id)
    }
    
    /// Confirm a server reservation (called when matchmaking responds)
    pub fn confirm_reservation(&mut self, res_id: ReservationId, server_id: ServerId) -> bool {
        if let Some(res) = self.reservations.get_mut(&res_id) {
            res.server_id = server_id;
            res.confirmed = true;
            true
        } else {
            false
        }
    }
    
    /// Get reservation status
    pub fn get_reservation(&self, res_id: ReservationId) -> Option<&ServerReservation> {
        self.reservations.get(&res_id)
    }
    
    /// Reserve a private server
    pub fn reserve_server(&mut self, place_id: PlaceId) -> TeleportResult<ReservedServerCode> {
        // Generate access code
        let code = format!("{:016x}", rand::random::<u64>());
        
        let reserved = ReservedServerCode {
            code: code.clone(),
            place_id,
            server_id: None,
            expires_at: None, // Never expires by default
        };
        
        self.reserved_codes.push(reserved.clone());
        Ok(reserved)
    }
    
    /// Get teleport status for a player
    pub fn get_status(&self, player: Entity) -> Option<TeleportStatus> {
        self.pending.get(&player).map(|p| p.status)
    }
    
    /// Cancel a pending teleport
    pub fn cancel(&mut self, player: Entity) -> bool {
        self.pending.remove(&player).is_some()
    }
    
    /// Get arrival data for a player (consumes it)
    pub fn get_arrival_data(&mut self, player: Entity) -> Option<TeleportData> {
        self.arrivals.remove(&player)
    }
    
    /// Check if player has arrival data
    pub fn has_arrival_data(&self, player: Entity) -> bool {
        self.arrivals.contains_key(&player)
    }
    
    /// Set arrival data (called when player connects after teleport)
    pub fn set_arrival_data(&mut self, player: Entity, data: TeleportData) {
        self.arrivals.insert(player, data);
    }
    
    /// Get current place ID
    pub fn get_place_id(&self) -> Option<PlaceId> {
        self.current_server.as_ref().map(|s| s.place_id)
    }
    
    /// Get current server ID
    pub fn get_server_id(&self) -> Option<&ServerId> {
        self.current_server.as_ref().map(|s| &s.server_id)
    }
    
    /// Get all active party teleports
    pub fn get_active_parties(&self) -> Vec<&PartyTeleport> {
        self.party_teleports.values()
            .filter(|p| !matches!(p.status, 
                PartyTeleportStatus::Complete | 
                PartyTeleportStatus::Failed | 
                PartyTeleportStatus::Cancelled
            ))
            .collect()
    }
    
    /// Check if player is in any active party teleport
    pub fn get_player_party(&self, player: Entity) -> Option<PartyTeleportId> {
        self.party_teleports.iter()
            .find(|(_, p)| p.players.contains(&player) && !matches!(p.status,
                PartyTeleportStatus::Complete |
                PartyTeleportStatus::Failed |
                PartyTeleportStatus::Cancelled
            ))
            .map(|(&id, _)| id)
    }
}

/// Options for party teleport
#[derive(Debug, Clone)]
pub struct PartyTeleportOptions {
    /// Countdown duration before teleport (seconds)
    pub countdown_secs: u32,
    /// Require all players to confirm?
    pub require_all_confirm: bool,
    /// Minimum confirmation ratio (0.0-1.0)
    pub min_confirm_ratio: f32,
    /// Skip confirmation (teleport immediately)
    pub skip_confirmation: bool,
}

impl Default for PartyTeleportOptions {
    fn default() -> Self {
        Self {
            countdown_secs: 5,
            require_all_confirm: false,
            min_confirm_ratio: 0.5, // At least 50% must confirm
            skip_confirmation: false,
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Event: Party teleport status changed
#[derive(Message, Debug, Clone)]
pub struct PartyTeleportStatusEvent {
    /// Party teleport ID
    pub party_id: PartyTeleportId,
    /// New status
    pub status: PartyTeleportStatus,
    /// Players affected
    pub players: Vec<Entity>,
}

/// Process pending individual teleports
fn process_teleports(
    mut teleport_service: ResMut<TeleportService>,
    mut status_events: MessageWriter<TeleportStatusEvent>,
) {
    let mut completed = Vec::new();
    
    for (&player, pending) in teleport_service.pending.iter_mut() {
        // Skip if part of a party teleport (handled separately)
        if pending.party_id.is_some() {
            continue;
        }
        
        // Timeout after 30 seconds
        if pending.started_at.elapsed().as_secs() > 30 {
            status_events.write(TeleportStatusEvent {
                player,
                status: TeleportStatus::Failed,
                error: Some(TeleportError::NetworkError("Timeout".to_string())),
            });
            completed.push(player);
            continue;
        }
        
        // Simulate teleport processing
        // In real implementation, this would communicate with matchmaking/gateway
        if pending.started_at.elapsed().as_millis() > 500 {
            pending.status = TeleportStatus::Complete;
            status_events.write(TeleportStatusEvent {
                player,
                status: TeleportStatus::Complete,
                error: None,
            });
            completed.push(player);
        }
    }
    
    for player in completed {
        teleport_service.pending.remove(&player);
    }
}

/// Process party teleports
fn process_party_teleports(
    mut teleport_service: ResMut<TeleportService>,
    mut party_events: MessageWriter<PartyTeleportStatusEvent>,
    mut status_events: MessageWriter<TeleportStatusEvent>,
) {
    let mut updates = Vec::new();
    
    for (&party_id, party) in teleport_service.party_teleports.iter() {
        match party.status {
            PartyTeleportStatus::WaitingForConfirmation => {
                let elapsed = party.created_at.elapsed().as_secs() as u32;
                let confirm_timeout = party.countdown_secs + 10; // Extra time for confirmation
                
                // Check if enough players confirmed
                let confirm_ratio = party.confirmed.len() as f32 / party.players.len() as f32;
                let all_confirmed = party.confirmed.len() == party.players.len();
                
                if all_confirmed || (!party.require_all_confirm && confirm_ratio >= party.min_confirm_ratio) {
                    // Move to reserving server
                    updates.push((party_id, PartyTeleportStatus::ReservingServer));
                } else if elapsed > confirm_timeout {
                    // Timeout - check if we have quorum
                    if confirm_ratio >= party.min_confirm_ratio {
                        updates.push((party_id, PartyTeleportStatus::ReservingServer));
                    } else {
                        updates.push((party_id, PartyTeleportStatus::Failed));
                    }
                }
            }
            
            PartyTeleportStatus::ReservingServer => {
                // In real implementation, wait for reservation confirmation
                // For now, simulate instant reservation
                updates.push((party_id, PartyTeleportStatus::Countdown { 
                    remaining_secs: party.countdown_secs 
                }));
            }
            
            PartyTeleportStatus::Countdown { remaining_secs } => {
                let elapsed = party.created_at.elapsed().as_secs() as u32;
                let countdown_start = party.countdown_secs + 10; // After confirmation phase
                
                if elapsed >= countdown_start + party.countdown_secs {
                    // Countdown complete, start teleporting
                    updates.push((party_id, PartyTeleportStatus::Teleporting));
                } else if elapsed >= countdown_start {
                    let new_remaining = (countdown_start + party.countdown_secs).saturating_sub(elapsed);
                    if new_remaining != remaining_secs {
                        updates.push((party_id, PartyTeleportStatus::Countdown { 
                            remaining_secs: new_remaining 
                        }));
                    }
                }
            }
            
            PartyTeleportStatus::Teleporting => {
                // Simulate teleport completion
                // In real implementation, track individual player teleport status
                let elapsed = party.created_at.elapsed().as_secs() as u32;
                let teleport_start = party.countdown_secs + 11; // After countdown
                
                if elapsed >= teleport_start + 2 {
                    // All teleported (simulated)
                    updates.push((party_id, PartyTeleportStatus::Complete));
                }
            }
            
            _ => {} // Terminal states
        }
    }
    
    // Apply updates
    for (party_id, new_status) in updates {
        // Extract data we need before modifying
        let party_data = teleport_service.party_teleports.get(&party_id).map(|party| {
            (
                party.status,
                party.confirmed.clone(),
                party.destination.clone(),
                party.data.clone(),
            )
        });
        
        if let Some((old_status, confirmed, destination, data)) = party_data {
            // Update status
            if let Some(party) = teleport_service.party_teleports.get_mut(&party_id) {
                party.status = new_status;
            }
            
            info!("Party teleport {} status: {:?} -> {:?}", party_id, old_status, new_status);
            
            party_events.write(PartyTeleportStatusEvent {
                party_id,
                status: new_status,
                players: confirmed.clone(),
            });
            
            // If teleporting, create individual pending teleports
            if matches!(new_status, PartyTeleportStatus::Teleporting) {
                for &player in &confirmed {
                    teleport_service.pending.insert(player, PendingTeleport {
                        destination: destination.clone(),
                        data: data.clone(),
                        status: TeleportStatus::Connecting,
                        started_at: Instant::now(),
                        party_id: Some(party_id),
                    });
                    
                    status_events.write(TeleportStatusEvent {
                        player,
                        status: TeleportStatus::Connecting,
                        error: None,
                    });
                }
            }
            
            // If complete, notify all players
            if matches!(new_status, PartyTeleportStatus::Complete) {
                for &player in &confirmed {
                    teleport_service.pending.remove(&player);
                    status_events.write(TeleportStatusEvent {
                        player,
                        status: TeleportStatus::Complete,
                        error: None,
                    });
                }
            }
        }
    }
    
    // Clean up old completed/failed party teleports
    teleport_service.party_teleports.retain(|_, p| {
        !matches!(p.status, 
            PartyTeleportStatus::Complete | 
            PartyTeleportStatus::Failed | 
            PartyTeleportStatus::Cancelled
        ) || p.created_at.elapsed().as_secs() < 60 // Keep for 60s for status queries
    });
    
    // Clean up expired reservations
    teleport_service.reservations.retain(|_, r| !r.is_expired());
}

// ============================================================================
// Plugin
// ============================================================================

/// Teleport plugin for Bevy
pub struct TeleportPlugin;

impl Plugin for TeleportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TeleportService>()
            .add_message::<TeleportRequestEvent>()
            .add_message::<TeleportArrivalEvent>()
            .add_message::<TeleportStatusEvent>()
            .add_message::<PartyTeleportStatusEvent>()
            .add_systems(Update, (
                process_teleports,
                process_party_teleports,
            ));
        
        info!("TeleportService initialized with party support");
    }
}
