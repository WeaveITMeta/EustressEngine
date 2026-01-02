//! # Server Plugin
//!
//! Server-side networking for Eustress.
//!
//! ## Responsibilities
//!
//! - Accept client connections (QUIC/TLS)
//! - Manage entity replication
//! - Arbitrate ownership
//! - Validate physics (anti-exploit)
//! - Broadcast state updates

use bevy::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use crate::config::{NetworkConfig, NetworkState, TickConfig};
use crate::error::NetworkError;
use crate::ownership::{NetworkOwner, OwnershipManager, OwnershipPlugin};
use crate::protocol::*;
use crate::replication::{DeltaTracker, InterestManager, ReplicationPlugin, Replicated, SpatialGrid};
use crate::transport::{
    BandwidthTracker, ClientConnected, ClientDisconnected, TransportBuilder, TransportPlugin,
    TransportState,
};

// ============================================================================
// Server State
// ============================================================================

/// Server state resource.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Not started
    Stopped,
    /// Starting up
    Starting,
    /// Running and accepting connections
    Running,
    /// Shutting down
    ShuttingDown,
}

impl Default for ServerState {
    fn default() -> Self {
        Self::Stopped
    }
}

/// Connected client info.
#[derive(Debug, Clone)]
pub struct ConnectedClient {
    /// Client ID
    pub id: u64,
    /// Socket address
    pub addr: SocketAddr,
    /// Player name
    pub name: String,
    /// Current RTT in ms
    pub rtt_ms: u32,
    /// Owned entities
    pub owned_entities: Vec<Entity>,
    /// Violation score (for anti-exploit)
    pub violations: f32,
    /// Last input tick received
    pub last_input_tick: u64,
}

/// Manages connected clients.
#[derive(Resource, Debug, Default)]
pub struct ClientManager {
    /// Connected clients by ID
    pub clients: HashMap<u64, ConnectedClient>,
    /// Next client ID
    next_id: u64,
}

impl ClientManager {
    /// Add a new client.
    pub fn add(&mut self, addr: SocketAddr, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.clients.insert(
            id,
            ConnectedClient {
                id,
                addr,
                name,
                rtt_ms: 0,
                owned_entities: Vec::new(),
                violations: 0.0,
                last_input_tick: 0,
            },
        );

        id
    }

    /// Remove a client.
    pub fn remove(&mut self, id: u64) -> Option<ConnectedClient> {
        self.clients.remove(&id)
    }

    /// Get client by ID.
    pub fn get(&self, id: u64) -> Option<&ConnectedClient> {
        self.clients.get(&id)
    }

    /// Get mutable client by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut ConnectedClient> {
        self.clients.get_mut(&id)
    }

    /// Get all client IDs.
    pub fn client_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.clients.keys().copied()
    }

    /// Count connected clients.
    pub fn count(&self) -> usize {
        self.clients.len()
    }
}

// ============================================================================
// Network ID Generator
// ============================================================================

/// Generates unique network IDs for entities.
#[derive(Resource, Debug, Default)]
pub struct NetworkIdGenerator {
    next_id: u64,
}

impl NetworkIdGenerator {
    /// Generate next ID.
    pub fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

// ============================================================================
// Server Events
// ============================================================================

/// Event to start the server.
#[derive(Event, Message, Debug)]
pub struct StartServer {
    pub addr: SocketAddr,
}

/// Event to stop the server.
#[derive(Event, Message, Debug)]
pub struct StopServer;

/// Event when server is ready.
#[derive(Event, Message, Debug)]
pub struct ServerReady {
    pub addr: SocketAddr,
}

// ============================================================================
// Server Systems
// ============================================================================

/// Handle server start request.
fn handle_start_server(
    mut events: EventReader<StartServer>,
    mut state: ResMut<ServerState>,
    mut transport: ResMut<TransportState>,
    config: Res<NetworkConfig>,
) {
    for event in events.read() {
        if *state != ServerState::Stopped {
            warn!("Server already running or starting");
            continue;
        }

        info!("Starting server on {}...", event.addr);
        *state = ServerState::Starting;
        transport.server_addr = event.addr;
        transport.is_server = true;

        // Lightyear server setup would go here
        // For now, just mark as running
        *state = ServerState::Running;
        info!(
            "Server running on {} ({}Hz, max {} clients)",
            event.addr, config.tick.tick_rate, config.transport.max_connections
        );
    }
}

/// Handle server stop request.
fn handle_stop_server(
    mut events: EventReader<StopServer>,
    mut state: ResMut<ServerState>,
    mut transport: ResMut<TransportState>,
    mut clients: ResMut<ClientManager>,
) {
    for _ in events.read() {
        if *state == ServerState::Stopped {
            continue;
        }

        info!("Stopping server...");
        *state = ServerState::ShuttingDown;

        // Disconnect all clients
        let client_ids: Vec<_> = clients.client_ids().collect();
        for id in client_ids {
            clients.remove(id);
        }

        transport.is_server = false;
        *state = ServerState::Stopped;
        info!("Server stopped");
    }
}

/// Process incoming client connections.
fn process_connections(
    mut clients: ResMut<ClientManager>,
    mut connected_events: EventWriter<ClientConnected>,
    config: Res<NetworkConfig>,
    // Lightyear connection events would be read here
) {
    // This would integrate with Lightyear's connection events
    // For now, placeholder
}

/// Process client disconnections.
fn process_disconnections(
    mut clients: ResMut<ClientManager>,
    mut disconnected_events: EventWriter<ClientDisconnected>,
    mut ownership: ResMut<OwnershipManager>,
    mut query: Query<&mut NetworkOwner>,
) {
    // This would integrate with Lightyear's disconnection events
    // For now, placeholder
}

/// Assign network IDs to new replicated entities.
fn assign_network_ids(
    mut commands: Commands,
    mut id_gen: ResMut<NetworkIdGenerator>,
    query: Query<Entity, (With<Replicated>, Without<NetworkEntity>)>,
) {
    for entity in query.iter() {
        let net_id = id_gen.next();
        commands.entity(entity).insert(NetworkEntity {
            net_id,
            class_name: "Entity".to_string(),
            name: format!("Entity_{}", net_id),
            parent_net_id: 0,
        });

        // Update Replicated component
        commands.entity(entity).insert(Replicated::new(net_id));
    }
}

/// Send replication updates to clients.
fn send_replication_updates(
    state: Res<ServerState>,
    clients: Res<ClientManager>,
    interest: Res<InterestManager>,
    mut delta_tracker: ResMut<DeltaTracker>,
    config: Res<NetworkConfig>,
    net_state: Res<NetworkState>,
    query: Query<(
        Entity,
        &Replicated,
        &NetworkTransform,
        &NetworkVelocity,
        Option<&NetworkOwner>,
    )>,
    mut bandwidth: ResMut<BandwidthTracker>,
) {
    if *state != ServerState::Running {
        return;
    }

    let threshold = config.replication.delta_threshold;

    for client_id in clients.client_ids() {
        let mut deltas = Vec::new();

        for entity in interest.get_interest(client_id) {
            if let Ok((_, replicated, transform, velocity, owner)) = query.get(entity) {
                // Skip if owned by this client (they have authoritative state)
                if owner.map(|o| o.client_id == client_id).unwrap_or(false) {
                    continue;
                }

                // Compute delta
                if let Some(delta) =
                    delta_tracker.compute_delta(replicated.net_id, transform, velocity, threshold)
                {
                    deltas.push(delta);

                    // Update tracker
                    delta_tracker.update(
                        replicated.net_id,
                        net_state.tick,
                        transform.clone(),
                        velocity.clone(),
                    );
                }
            }
        }

        if !deltas.is_empty() {
            // Serialize and send via Lightyear
            // For now, just track bandwidth
            let estimated_size = deltas.len() * 32; // Rough estimate
            bandwidth.record_sent(estimated_size as u64);
        }
    }
}

/// Validate physics updates from clients (anti-exploit).
fn validate_client_physics(
    mut clients: ResMut<ClientManager>,
    config: Res<NetworkConfig>,
    query: Query<(&NetworkOwner, &NetworkTransform, &NetworkVelocity)>,
    mut disconnected: EventWriter<ClientDisconnected>,
) {
    if !config.anti_exploit.validate_physics {
        return;
    }

    let max_speed = config.anti_exploit.max_speed;
    let violation_threshold = config.anti_exploit.violation_threshold;

    for (owner, transform, velocity) in query.iter() {
        if owner.is_server_owned() {
            continue;
        }

        let client_id = owner.client_id;

        // Check speed
        if velocity.linear.length() > max_speed {
            if let Some(client) = clients.get_mut(client_id) {
                client.violations += 1.0;
                warn!(
                    "Client {} speed violation: {:.1} > {:.1}",
                    client_id,
                    velocity.linear.length(),
                    max_speed
                );

                if client.violations >= violation_threshold as f32 {
                    disconnected.write(ClientDisconnected {
                        client_id,
                        reason: "Too many violations".to_string(),
                    });
                }
            }
        }
    }

    // Decay violations
    let decay = config.anti_exploit.violation_decay;
    for client in clients.clients.values_mut() {
        client.violations = (client.violations - decay * (1.0 / 120.0)).max(0.0);
    }
}

/// Update network state resource.
fn update_network_state(
    clients: Res<ClientManager>,
    mut state: ResMut<NetworkState>,
    bandwidth: Res<BandwidthTracker>,
    config: Res<NetworkConfig>,
) {
    state.tick += 1;
    state.client_count = clients.count();
    state.bytes_sent = bandwidth.bytes_per_sec_sent as u64;
    state.bytes_received = bandwidth.bytes_per_sec_recv as u64;

    // Calculate average RTT
    if !clients.clients.is_empty() {
        let total_rtt: u32 = clients.clients.values().map(|c| c.rtt_ms).sum();
        state.avg_rtt_ms = total_rtt as f32 / clients.count() as f32;
    }

    // Check fallback mode
    state.fallback_mode = state.avg_rtt_ms > config.tick.fallback_rtt_threshold_ms as f32;
    state.current_replication_rate = if state.fallback_mode {
        config.tick.fallback_rate
    } else {
        config.tick.owned_replication_rate
    };
}

// ============================================================================
// Server Plugin
// ============================================================================

/// Server networking plugin.
pub struct ServerNetworkPlugin;

impl Plugin for ServerNetworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ServerState>()
            .init_resource::<ClientManager>()
            .init_resource::<NetworkIdGenerator>()
            .init_resource::<NetworkState>()
            .add_event::<StartServer>()
            .add_event::<StopServer>()
            .add_event::<ServerReady>()
            // TransportPlugin added by EustressNetworkingPlugin
            .add_plugins(OwnershipPlugin)
            .add_plugins(ReplicationPlugin)
            .add_systems(
                Update,
                (
                    handle_start_server,
                    handle_stop_server,
                    process_connections,
                    process_disconnections,
                )
                    .chain(),
            )
            .add_systems(
                FixedUpdate,
                (
                    assign_network_ids,
                    send_replication_updates,
                    validate_client_physics,
                    update_network_state,
                )
                    .chain()
                    .run_if(|state: Res<ServerState>| *state == ServerState::Running),
            );

        info!("Server network plugin initialized");
    }
}

