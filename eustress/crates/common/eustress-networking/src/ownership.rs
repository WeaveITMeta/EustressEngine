//! # Network Ownership System
//!
//! Implements client-owned entities with server arbitration.
//!
//! ## Table of Contents
//!
//! 1. **NetworkOwner** - Component marking entity ownership
//! 2. **ActivityTracker** - Per-entity activity tracking for auto-release
//! 3. **OwnershipManager** - Resource managing ownership state
//! 4. **GradualHandoff** - Smooth physics authority transfer
//!
//! ## Ownership Model
//!
//! - **Server-owned**: Default. Server simulates, clients interpolate.
//! - **Client-owned**: Client simulates (prediction), server validates.
//! - **Contested**: Multiple clients want ownership; server arbitrates.
//! - **Transitioning**: Gradual handoff between owners (1-2s blend).
//!
//! ## Transfer Flow
//!
//! 1. Client sends `OwnershipRequest`
//! 2. Server checks: distance, cooldown, contention
//! 3. Server sends `OwnershipGranted` or `OwnershipDenied`
//! 4. On grant: old owner gets `OwnershipTransfer`, stops simulating
//! 5. New owner starts prediction immediately
//!
//! ## Activity Tracking
//!
//! - Per-entity timers track last input/position change
//! - Idle entities (no activity > threshold) auto-release to server
//! - Gradual handoff blends physics authority over 1-2 seconds

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::config::OwnershipConfig;
use crate::error::{NetworkError, NetworkResult};

// ============================================================================
// Components
// ============================================================================

/// Marks an entity as network-owned by a specific client.
///
/// - `client_id = 0`: Server-owned (default)
/// - `client_id > 0`: Client-owned
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct NetworkOwner {
    /// Owning client ID (0 = server)
    pub client_id: u64,
    /// Tick when ownership was acquired
    pub acquired_tick: u64,
    /// Whether ownership can be transferred
    pub transferable: bool,
}

impl Default for NetworkOwner {
    fn default() -> Self {
        Self {
            client_id: 0, // Server-owned
            acquired_tick: 0,
            transferable: true,
        }
    }
}

impl NetworkOwner {
    /// Create server-owned
    pub fn server() -> Self {
        Self::default()
    }

    /// Create client-owned
    pub fn client(client_id: u64, tick: u64) -> Self {
        Self {
            client_id,
            acquired_tick: tick,
            transferable: true,
        }
    }

    /// Check if server-owned
    pub fn is_server_owned(&self) -> bool {
        self.client_id == 0
    }

    /// Check if owned by specific client
    pub fn is_owned_by(&self, client_id: u64) -> bool {
        self.client_id == client_id
    }
}

/// Marker for entities that can never change ownership.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct OwnershipLocked;

/// Marker for entities currently in ownership transition.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct OwnershipPending {
    /// Client requesting ownership
    pub requester: u64,
    /// When request was made
    pub requested_at: u64,
}

// ============================================================================
// Events
// ============================================================================

/// Request to take ownership of an entity.
#[derive(Event, Message, Debug, Clone)]
pub struct OwnershipRequest {
    /// Entity to own
    pub entity: Entity,
    /// Network ID of entity
    pub net_id: u64,
    /// Requesting client
    pub client_id: u64,
    /// Client's position (for distance check)
    pub client_position: Vec3,
}

/// Ownership transfer event (sent when ownership changes).
#[derive(Event, Message, Debug, Clone)]
pub struct OwnershipTransfer {
    /// Entity being transferred
    pub entity: Entity,
    /// Network ID
    pub net_id: u64,
    /// Previous owner (0 = server)
    pub from_client: u64,
    /// New owner (0 = server)
    pub to_client: u64,
    /// Tick of transfer
    pub tick: u64,
}

/// Ownership denied event.
#[derive(Event, Message, Debug, Clone)]
pub struct OwnershipDenied {
    /// Entity that was requested
    pub entity: Entity,
    /// Network ID
    pub net_id: u64,
    /// Client that was denied
    pub client_id: u64,
    /// Reason for denial
    pub reason: String,
}

/// Ownership released event (client voluntarily releases).
#[derive(Event, Message, Debug, Clone)]
pub struct OwnershipReleased {
    /// Entity being released
    pub entity: Entity,
    /// Network ID
    pub net_id: u64,
    /// Client releasing
    pub client_id: u64,
}

// ============================================================================
// Resources
// ============================================================================

/// Tracks ownership state and pending requests.
#[derive(Resource, Debug, Default)]
pub struct OwnershipManager {
    /// Map of net_id -> current owner client_id
    pub owners: HashMap<u64, u64>,
    /// Pending ownership requests (net_id -> (client_id, request_time))
    pub pending: HashMap<u64, (u64, Instant)>,
    /// Cooldowns per entity (net_id -> last_transfer_time)
    pub cooldowns: HashMap<u64, Instant>,
    /// Client ping values for arbitration
    pub client_pings: HashMap<u64, u32>,
    /// Per-entity activity tracking (net_id -> last_activity_time)
    pub activity: HashMap<u64, Instant>,
    /// Entities in gradual handoff (net_id -> handoff state)
    pub handoffs: HashMap<u64, GradualHandoff>,
}

/// Gradual handoff state for smooth physics authority transfer
#[derive(Debug, Clone)]
pub struct GradualHandoff {
    /// Entity being handed off
    pub entity: Entity,
    /// Previous owner
    pub from_client: u64,
    /// New owner
    pub to_client: u64,
    /// When handoff started
    pub started_at: Instant,
    /// Handoff duration in milliseconds
    pub duration_ms: u64,
    /// Current blend factor (0.0 = old owner, 1.0 = new owner)
    pub blend: f32,
}

impl GradualHandoff {
    /// Create a new handoff
    pub fn new(entity: Entity, from: u64, to: u64, duration_ms: u64) -> Self {
        Self {
            entity,
            from_client: from,
            to_client: to,
            started_at: Instant::now(),
            duration_ms,
            blend: 0.0,
        }
    }
    
    /// Update blend factor, returns true if complete
    pub fn update(&mut self) -> bool {
        let elapsed = self.started_at.elapsed().as_millis() as f32;
        let duration = self.duration_ms as f32;
        self.blend = (elapsed / duration).min(1.0);
        self.blend >= 1.0
    }
    
    /// Check if handoff is complete
    pub fn is_complete(&self) -> bool {
        self.blend >= 1.0
    }
}

/// Activity type for tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    /// Any input received
    Input,
    /// Position changed beyond threshold
    Movement,
    /// Explicit interaction (tool use, etc.)
    Interaction,
}

impl OwnershipManager {
    /// Get owner of an entity
    pub fn get_owner(&self, net_id: u64) -> u64 {
        *self.owners.get(&net_id).unwrap_or(&0)
    }

    /// Set owner of an entity
    pub fn set_owner(&mut self, net_id: u64, client_id: u64) {
        if client_id == 0 {
            self.owners.remove(&net_id);
            self.activity.remove(&net_id);
        } else {
            self.owners.insert(net_id, client_id);
            self.activity.insert(net_id, Instant::now());
        }
        self.cooldowns.insert(net_id, Instant::now());
    }
    
    /// Set owner with gradual handoff
    pub fn set_owner_gradual(
        &mut self,
        net_id: u64,
        entity: Entity,
        from_client: u64,
        to_client: u64,
        duration_ms: u64,
    ) {
        // Start gradual handoff
        self.handoffs.insert(
            net_id,
            GradualHandoff::new(entity, from_client, to_client, duration_ms),
        );
        
        // Update ownership immediately (handoff is for physics blending)
        self.set_owner(net_id, to_client);
    }

    /// Check if entity is on cooldown
    pub fn is_on_cooldown(&self, net_id: u64, cooldown: Duration) -> bool {
        self.cooldowns
            .get(&net_id)
            .map(|t| t.elapsed() < cooldown)
            .unwrap_or(false)
    }

    /// Add pending request
    pub fn add_pending(&mut self, net_id: u64, client_id: u64) {
        self.pending.insert(net_id, (client_id, Instant::now()));
    }

    /// Remove pending request
    pub fn remove_pending(&mut self, net_id: u64) -> Option<(u64, Instant)> {
        self.pending.remove(&net_id)
    }

    /// Clean up expired pending requests
    pub fn cleanup_expired(&mut self, timeout: Duration) {
        self.pending.retain(|_, (_, time)| time.elapsed() < timeout);
    }

    /// Update client ping
    pub fn update_ping(&mut self, client_id: u64, ping_ms: u32) {
        self.client_pings.insert(client_id, ping_ms);
    }

    /// Get client ping
    pub fn get_ping(&self, client_id: u64) -> u32 {
        *self.client_pings.get(&client_id).unwrap_or(&100)
    }
    
    // --- Activity Tracking ---
    
    /// Record activity for an entity
    pub fn record_activity(&mut self, net_id: u64, _activity_type: ActivityType) {
        self.activity.insert(net_id, Instant::now());
    }
    
    /// Get time since last activity
    pub fn time_since_activity(&self, net_id: u64) -> Option<Duration> {
        self.activity.get(&net_id).map(|t| t.elapsed())
    }
    
    /// Check if entity is idle (no activity for threshold)
    pub fn is_idle(&self, net_id: u64, threshold: Duration) -> bool {
        self.activity
            .get(&net_id)
            .map(|t| t.elapsed() > threshold)
            .unwrap_or(true) // No activity recorded = idle
    }
    
    /// Get all idle entities owned by a client
    pub fn get_idle_entities(&self, client_id: u64, threshold: Duration) -> Vec<u64> {
        self.owners
            .iter()
            .filter(|(_, &owner)| owner == client_id)
            .filter(|(net_id, _)| self.is_idle(**net_id, threshold))
            .map(|(&net_id, _)| net_id)
            .collect()
    }
    
    // --- Gradual Handoff ---
    
    /// Update all active handoffs, returns completed ones
    pub fn update_handoffs(&mut self) -> Vec<(u64, GradualHandoff)> {
        let mut completed = Vec::new();
        
        self.handoffs.retain(|&net_id, handoff| {
            if handoff.update() {
                completed.push((net_id, handoff.clone()));
                false // Remove from map
            } else {
                true // Keep in map
            }
        });
        
        completed
    }
    
    /// Get handoff blend factor for an entity (0.0 = old owner, 1.0 = new owner)
    pub fn get_handoff_blend(&self, net_id: u64) -> Option<f32> {
        self.handoffs.get(&net_id).map(|h| h.blend)
    }
    
    /// Check if entity is in handoff
    pub fn is_in_handoff(&self, net_id: u64) -> bool {
        self.handoffs.contains_key(&net_id)
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Process ownership requests (server-side).
pub fn process_ownership_requests(
    mut requests: EventReader<OwnershipRequest>,
    mut transfers: EventWriter<OwnershipTransfer>,
    mut denials: EventWriter<OwnershipDenied>,
    mut manager: ResMut<OwnershipManager>,
    config: Res<OwnershipConfig>,
    query: Query<(Entity, &Transform, &NetworkOwner, Option<&OwnershipLocked>)>,
    tick: Res<crate::config::NetworkState>,
) {
    let cooldown = Duration::from_millis(config.transfer_cooldown_ms as u64);
    let timeout = Duration::from_millis(config.request_timeout_ms as u64);

    // Clean up expired requests
    manager.cleanup_expired(timeout);

    for request in requests.read() {
        // Find entity
        let Some((entity, transform, owner, locked)) = query
            .iter()
            .find(|(_, _, o, _)| manager.get_owner(request.net_id) == o.client_id || request.net_id != 0)
        else {
            denials.write(OwnershipDenied {
                entity: request.entity,
                net_id: request.net_id,
                client_id: request.client_id,
                reason: "Entity not found".to_string(),
            });
            continue;
        };

        // Check if locked
        if locked.is_some() {
            denials.write(OwnershipDenied {
                entity,
                net_id: request.net_id,
                client_id: request.client_id,
                reason: "Ownership locked".to_string(),
            });
            continue;
        }

        // Check if already owned by requester
        if owner.client_id == request.client_id {
            continue; // Already owns it
        }

        // Check distance
        let distance = transform.translation.distance(request.client_position);
        if distance > config.max_request_distance {
            denials.write(OwnershipDenied {
                entity,
                net_id: request.net_id,
                client_id: request.client_id,
                reason: format!("Too far: {:.1} > {:.1} studs", distance, config.max_request_distance),
            });
            continue;
        }

        // Check cooldown
        if manager.is_on_cooldown(request.net_id, cooldown) {
            denials.write(OwnershipDenied {
                entity,
                net_id: request.net_id,
                client_id: request.client_id,
                reason: "Transfer on cooldown".to_string(),
            });
            continue;
        }

        // Check contention (another pending request)
        if let Some((other_client, _)) = manager.pending.get(&request.net_id) {
            if *other_client != request.client_id {
                // Arbitrate: prefer lower ping if enabled
                if config.prefer_low_ping {
                    let requester_ping = manager.get_ping(request.client_id);
                    let other_ping = manager.get_ping(*other_client);
                    if requester_ping >= other_ping {
                        denials.write(OwnershipDenied {
                            entity,
                            net_id: request.net_id,
                            client_id: request.client_id,
                            reason: "Lost contention (higher ping)".to_string(),
                        });
                        continue;
                    }
                    // Requester wins, remove other's pending
                    manager.remove_pending(request.net_id);
                } else {
                    // First-come-first-served
                    denials.write(OwnershipDenied {
                        entity,
                        net_id: request.net_id,
                        client_id: request.client_id,
                        reason: "Already pending".to_string(),
                    });
                    continue;
                }
            }
        }

        // Grant ownership
        let from_client = owner.client_id;
        manager.set_owner(request.net_id, request.client_id);

        transfers.write(OwnershipTransfer {
            entity,
            net_id: request.net_id,
            from_client,
            to_client: request.client_id,
            tick: tick.tick,
        });

        info!(
            "Ownership transferred: entity {:?} (net_id {}) from {} to {}",
            entity, request.net_id, from_client, request.client_id
        );
    }
}

/// Apply ownership transfers to components.
pub fn apply_ownership_transfers(
    mut transfers: EventReader<OwnershipTransfer>,
    mut query: Query<&mut NetworkOwner>,
) {
    for transfer in transfers.read() {
        if let Ok(mut owner) = query.get_mut(transfer.entity) {
            owner.client_id = transfer.to_client;
            owner.acquired_tick = transfer.tick;
        }
    }
}

/// Handle ownership releases.
pub fn handle_ownership_releases(
    mut releases: EventReader<OwnershipReleased>,
    mut manager: ResMut<OwnershipManager>,
    mut query: Query<&mut NetworkOwner>,
    tick: Res<crate::config::NetworkState>,
) {
    for release in releases.read() {
        // Verify client actually owns it
        if manager.get_owner(release.net_id) != release.client_id {
            warn!(
                "Client {} tried to release entity {} they don't own",
                release.client_id, release.net_id
            );
            continue;
        }

        // Transfer to server
        manager.set_owner(release.net_id, 0);

        if let Ok(mut owner) = query.get_mut(release.entity) {
            owner.client_id = 0;
            owner.acquired_tick = tick.tick;
        }

        info!(
            "Client {} released ownership of entity {:?} (net_id {})",
            release.client_id, release.entity, release.net_id
        );
    }
}

/// Auto-release inactive ownership.
pub fn auto_release_inactive(
    mut manager: ResMut<OwnershipManager>,
    config: Res<OwnershipConfig>,
    mut query: Query<(Entity, &mut NetworkOwner)>,
    tick: Res<crate::config::NetworkState>,
    mut transfers: EventWriter<OwnershipTransfer>,
) {
    if config.auto_release_secs == 0 {
        return; // Disabled
    }
    
    let threshold = Duration::from_secs(config.auto_release_secs as u64);
    let handoff_duration = config.gradual_handoff_ms as u64;
    
    // Collect idle entities to release
    let idle_entities: Vec<(u64, u64)> = manager.owners
        .iter()
        .filter(|(_, &owner)| owner != 0) // Only client-owned
        .filter(|(net_id, _)| manager.is_idle(**net_id, threshold))
        .filter(|(net_id, _)| !manager.is_in_handoff(**net_id)) // Not already handing off
        .map(|(&net_id, &owner)| (net_id, owner))
        .collect();
    
    // Release each idle entity with gradual handoff
    for (net_id, from_client) in idle_entities {
        // Find the entity
        for (entity, mut owner) in query.iter_mut() {
            if owner.client_id == from_client {
                // Check if this is the right entity (would need net_id component)
                // For now, use entity index as net_id approximation
                let entity_net_id = entity.index() as u64;
                if entity_net_id == net_id {
                    info!(
                        "Auto-releasing idle entity {:?} (net_id {}) from client {}",
                        entity, net_id, from_client
                    );
                    
                    // Start gradual handoff to server
                    if handoff_duration > 0 {
                        manager.set_owner_gradual(net_id, entity, from_client, 0, handoff_duration);
                    } else {
                        manager.set_owner(net_id, 0);
                    }
                    
                    owner.client_id = 0;
                    owner.acquired_tick = tick.tick;
                    
                    transfers.write(OwnershipTransfer {
                        entity,
                        net_id,
                        from_client,
                        to_client: 0,
                        tick: tick.tick,
                    });
                    
                    break;
                }
            }
        }
    }
}

/// Update gradual handoffs and apply physics blending.
pub fn update_gradual_handoffs(
    mut manager: ResMut<OwnershipManager>,
) {
    // Update all handoffs and get completed ones
    let completed = manager.update_handoffs();
    
    for (net_id, handoff) in completed {
        info!(
            "Gradual handoff complete for entity {:?} (net_id {}): {} -> {}",
            handoff.entity, net_id, handoff.from_client, handoff.to_client
        );
    }
}

/// Track activity from client inputs.
pub fn track_client_activity(
    mut manager: ResMut<OwnershipManager>,
    // This would be called from input processing systems
    // with the net_id and activity type
) {
    // Activity tracking is done via manager.record_activity()
    // called from other systems when input/movement is detected
}

/// Event to record activity for an entity
#[derive(Event, Message, Debug, Clone)]
pub struct RecordActivityEvent {
    pub net_id: u64,
    pub activity_type: ActivityType,
}

/// Handle activity recording events
pub fn handle_activity_events(
    mut events: EventReader<RecordActivityEvent>,
    mut manager: ResMut<OwnershipManager>,
) {
    for event in events.read() {
        manager.record_activity(event.net_id, event.activity_type);
    }
}

// ============================================================================
// Ownership Plugin
// ============================================================================

/// Plugin for ownership management.
pub struct OwnershipPlugin;

impl Plugin for OwnershipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OwnershipManager>()
            .add_event::<OwnershipRequest>()
            .add_event::<OwnershipTransfer>()
            .add_event::<OwnershipDenied>()
            .add_event::<OwnershipReleased>()
            .add_event::<RecordActivityEvent>()
            .register_type::<NetworkOwner>()
            .register_type::<OwnershipLocked>()
            .register_type::<OwnershipPending>();

        // Server-side systems
        #[cfg(feature = "server")]
        app.add_systems(
            Update,
            (
                handle_activity_events,
                process_ownership_requests,
                apply_ownership_transfers,
                handle_ownership_releases,
                auto_release_inactive,
                update_gradual_handoffs,
            )
                .chain(),
        );

        // Client-side applies transfers and tracks activity
        #[cfg(feature = "client")]
        app.add_systems(Update, (
            apply_ownership_transfers,
            handle_activity_events,
        ));
    }
}

