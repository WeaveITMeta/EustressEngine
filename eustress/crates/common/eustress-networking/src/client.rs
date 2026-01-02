//! # Client Plugin
//!
//! Client-side networking for Eustress.
//!
//! ## Responsibilities
//!
//! - Connect to server (QUIC/TLS)
//! - Send player input
//! - Predict owned entities
//! - Interpolate remote entities
//! - Handle ownership transfers

use bevy::prelude::*;
use std::collections::VecDeque;
use std::net::SocketAddr;

use crate::config::{NetworkConfig, NetworkState};
use crate::error::NetworkError;
use crate::ownership::{NetworkOwner, OwnershipRequest, OwnershipTransfer};
use crate::protocol::*;
use crate::transport::{BandwidthTracker, Connected, Disconnected, TransportPlugin, TransportState};

// ============================================================================
// Client State
// ============================================================================

/// Client connection state.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    /// Not connected
    Disconnected,
    /// Connecting to server
    Connecting,
    /// Connected and syncing
    Syncing,
    /// Fully connected and playing
    Connected,
    /// Reconnecting after disconnect
    Reconnecting,
}

impl Default for ClientState {
    fn default() -> Self {
        Self::Disconnected
    }
}

/// Local client info.
#[derive(Resource, Debug, Clone)]
pub struct LocalClient {
    /// Assigned client ID
    pub id: u64,
    /// Player name
    pub name: String,
    /// Server address
    pub server_addr: SocketAddr,
    /// Current RTT in ms
    pub rtt_ms: u32,
    /// Owned entities
    pub owned_entities: Vec<Entity>,
    /// Current tick
    pub tick: u64,
    /// Server tick (may differ due to latency)
    pub server_tick: u64,
}

impl Default for LocalClient {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Player".to_string(),
            server_addr: "127.0.0.1:4433".parse().unwrap(),
            rtt_ms: 0,
            owned_entities: Vec::new(),
            tick: 0,
            server_tick: 0,
        }
    }
}

// ============================================================================
// Prediction Components
// ============================================================================

/// Marks an entity as predicted (client simulates ahead).
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Predicted {
    /// Last confirmed tick from server
    pub confirmed_tick: u64,
    /// Pending inputs not yet confirmed
    pub pending_inputs: u32,
}

/// Marks an entity as interpolated (smooth remote movement).
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Interpolated {
    /// Previous state
    pub from: NetworkTransform,
    /// Target state
    pub to: NetworkTransform,
    /// Interpolation progress (0-1)
    pub t: f32,
    /// Time between states
    pub duration: f32,
}

/// Confirmed state from server (for rollback).
#[derive(Component, Debug, Clone)]
pub struct Confirmed {
    /// Tick of this state
    pub tick: u64,
    /// Confirmed transform
    pub transform: NetworkTransform,
    /// Confirmed velocity
    pub velocity: NetworkVelocity,
}

// ============================================================================
// Input Buffer
// ============================================================================

/// Buffered input for prediction and reconciliation.
#[derive(Debug, Clone)]
pub struct BufferedInput {
    pub tick: u64,
    pub input: PlayerInput,
    pub predicted_transform: NetworkTransform,
}

/// Input buffer resource.
#[derive(Resource, Debug, Default)]
pub struct InputBuffer {
    /// Buffered inputs (oldest first)
    pub buffer: VecDeque<BufferedInput>,
    /// Maximum buffer size
    pub max_size: usize,
}

impl InputBuffer {
    /// Create with max size.
    pub fn new(max_size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Add input to buffer.
    pub fn push(&mut self, input: BufferedInput) {
        if self.buffer.len() >= self.max_size {
            self.buffer.pop_front();
        }
        self.buffer.push_back(input);
    }

    /// Remove inputs up to and including tick.
    pub fn acknowledge(&mut self, tick: u64) {
        while let Some(front) = self.buffer.front() {
            if front.tick <= tick {
                self.buffer.pop_front();
            } else {
                break;
            }
        }
    }

    /// Get inputs after tick (for replay).
    pub fn inputs_after(&self, tick: u64) -> impl Iterator<Item = &BufferedInput> {
        self.buffer.iter().filter(move |i| i.tick > tick)
    }
}

// ============================================================================
// Client Events
// ============================================================================

/// Event to connect to server.
#[derive(Event, Message, Debug)]
pub struct Connect {
    pub server_addr: SocketAddr,
    pub player_name: String,
}

/// Event to disconnect from server.
#[derive(Event, Message, Debug)]
pub struct Disconnect {
    pub reason: String,
}

/// Event when connection is established.
#[derive(Event, Message, Debug)]
pub struct ConnectionEstablished {
    pub client_id: u64,
}

/// Event when connection is lost.
#[derive(Event, Message, Debug)]
pub struct ConnectionLost {
    pub reason: String,
}

// ============================================================================
// Client Systems
// ============================================================================

/// Handle connect request.
fn handle_connect(
    mut events: EventReader<Connect>,
    mut state: ResMut<ClientState>,
    mut local: ResMut<LocalClient>,
    mut transport: ResMut<TransportState>,
) {
    for event in events.read() {
        if *state != ClientState::Disconnected {
            warn!("Already connected or connecting");
            continue;
        }

        info!("Connecting to {}...", event.server_addr);
        *state = ClientState::Connecting;
        local.server_addr = event.server_addr;
        local.name = event.player_name.clone();
        transport.server_addr = event.server_addr;

        // Lightyear connection would happen here
        // For now, simulate immediate connection
        *state = ClientState::Syncing;
    }
}

/// Handle disconnect request.
fn handle_disconnect(
    mut events: EventReader<Disconnect>,
    mut state: ResMut<ClientState>,
    mut transport: ResMut<TransportState>,
    mut local: ResMut<LocalClient>,
) {
    for event in events.read() {
        if *state == ClientState::Disconnected {
            continue;
        }

        info!("Disconnecting: {}", event.reason);
        transport.is_connected = false;
        local.owned_entities.clear();
        *state = ClientState::Disconnected;
    }
}

/// Sample and send player input.
fn sample_and_send_input(
    state: Res<ClientState>,
    mut local: ResMut<LocalClient>,
    mut input_buffer: ResMut<InputBuffer>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut bandwidth: ResMut<BandwidthTracker>,
    query: Query<&NetworkTransform, With<Predicted>>,
) {
    if *state != ClientState::Connected {
        return;
    }

    // Sample input
    let input = PlayerInput {
        tick: local.tick,
        movement: Vec3::new(
            if keyboard.pressed(KeyCode::KeyD) { 1.0 } else if keyboard.pressed(KeyCode::KeyA) { -1.0 } else { 0.0 },
            0.0,
            if keyboard.pressed(KeyCode::KeyW) { -1.0 } else if keyboard.pressed(KeyCode::KeyS) { 1.0 } else { 0.0 },
        ).normalize_or_zero(),
        look: Vec2::ZERO, // Would come from mouse
        jump: keyboard.just_pressed(KeyCode::Space),
        sprint: keyboard.pressed(KeyCode::ShiftLeft),
        primary: mouse.pressed(MouseButton::Left),
        secondary: mouse.pressed(MouseButton::Right),
        actions: 0,
    };

    // Get current predicted transform (for reconciliation)
    let predicted_transform = query
        .iter()
        .next()
        .cloned()
        .unwrap_or_default();

    // Buffer input
    input_buffer.push(BufferedInput {
        tick: local.tick,
        input: input.clone(),
        predicted_transform,
    });

    // Send to server via Lightyear
    // For now, just track bandwidth
    bandwidth.record_sent(32); // Approximate input size

    local.tick += 1;
}

/// Apply prediction to owned entities.
fn apply_prediction(
    state: Res<ClientState>,
    local: Res<LocalClient>,
    input_buffer: Res<InputBuffer>,
    time: Res<Time>,
    mut query: Query<(&NetworkOwner, &mut Transform, &mut NetworkTransform, &mut Predicted)>,
) {
    if *state != ClientState::Connected {
        return;
    }

    for (owner, mut transform, mut net_transform, mut predicted) in query.iter_mut() {
        if !owner.is_owned_by(local.id) {
            continue;
        }

        // Get latest input
        if let Some(buffered) = input_buffer.buffer.back() {
            let input = &buffered.input;
            let dt = time.delta_secs();

            // Simple movement prediction
            let speed = if input.sprint { 32.0 } else { 16.0 }; // studs/s
            let velocity = input.movement * speed;

            transform.translation += velocity * dt;
            *net_transform = NetworkTransform::from_transform(&transform);

            predicted.pending_inputs = input_buffer.buffer.len() as u32;
        }
    }
}

/// Interpolate remote entities.
fn interpolate_remote(
    state: Res<ClientState>,
    local: Res<LocalClient>,
    time: Res<Time>,
    mut query: Query<(&NetworkOwner, &mut Transform, &mut Interpolated)>,
) {
    if *state != ClientState::Connected {
        return;
    }

    for (owner, mut transform, mut interp) in query.iter_mut() {
        // Skip owned entities (they use prediction)
        if owner.is_owned_by(local.id) {
            continue;
        }

        // Advance interpolation
        interp.t += time.delta_secs() / interp.duration.max(0.001);
        interp.t = interp.t.min(1.0);

        // Lerp transform
        transform.translation = interp.from.position.lerp(interp.to.position, interp.t);
        transform.rotation = interp.from.rotation.slerp(interp.to.rotation, interp.t);
        transform.scale = interp.from.scale.lerp(interp.to.scale, interp.t);
    }
}

/// Handle server state updates.
fn handle_state_updates(
    state: Res<ClientState>,
    mut local: ResMut<LocalClient>,
    mut input_buffer: ResMut<InputBuffer>,
    mut query: Query<(
        &NetworkOwner,
        &mut Transform,
        &mut NetworkTransform,
        &mut Predicted,
        Option<&mut Interpolated>,
    )>,
    // Lightyear would provide state updates here
) {
    if *state != ClientState::Connected {
        return;
    }

    // This would process incoming EntityDelta messages from server
    // For owned entities: reconcile (compare server state, replay inputs if mismatch)
    // For remote entities: update interpolation targets
}

/// Reconcile prediction with server state.
fn reconcile_prediction(
    local: Res<LocalClient>,
    mut input_buffer: ResMut<InputBuffer>,
    mut query: Query<(&NetworkOwner, &mut Transform, &Confirmed, &mut Predicted)>,
) {
    for (owner, mut transform, confirmed, mut predicted) in query.iter_mut() {
        if !owner.is_owned_by(local.id) {
            continue;
        }

        // Check if server state differs from our prediction at that tick
        let server_tick = confirmed.tick;

        // Find our predicted state at that tick
        let our_prediction = input_buffer
            .buffer
            .iter()
            .find(|b| b.tick == server_tick);

        if let Some(prediction) = our_prediction {
            let diff = confirmed.transform.position.distance(prediction.predicted_transform.position);

            if diff > 0.1 {
                // Mismatch! Rollback and replay
                warn!("Prediction mismatch at tick {}: {:.2} studs", server_tick, diff);

                // Reset to confirmed state
                *transform = confirmed.transform.to_transform();

                // Replay all inputs after confirmed tick
                for buffered in input_buffer.inputs_after(server_tick) {
                    let input = &buffered.input;
                    let speed = if input.sprint { 32.0 } else { 16.0 };
                    let velocity = input.movement * speed;
                    transform.translation += velocity * (1.0 / 120.0); // Fixed timestep
                }
            }
        }

        // Acknowledge confirmed tick
        input_buffer.acknowledge(server_tick);
        predicted.confirmed_tick = server_tick;
        predicted.pending_inputs = input_buffer.buffer.len() as u32;
    }
}

/// Handle ownership transfers.
fn handle_ownership_transfers(
    mut transfers: EventReader<OwnershipTransfer>,
    local: Res<LocalClient>,
    mut commands: Commands,
    query: Query<Entity, With<NetworkOwner>>,
) {
    for transfer in transfers.read() {
        // If we gained ownership, add Predicted
        if transfer.to_client == local.id {
            if let Ok(entity) = query.get(transfer.entity) {
                commands.entity(entity).insert(Predicted::default());
                commands.entity(entity).remove::<Interpolated>();
                info!("Gained ownership of {:?}", entity);
            }
        }

        // If we lost ownership, add Interpolated
        if transfer.from_client == local.id && transfer.to_client != local.id {
            if let Ok(entity) = query.get(transfer.entity) {
                commands.entity(entity).remove::<Predicted>();
                commands.entity(entity).insert(Interpolated::default());
                info!("Lost ownership of {:?}", entity);
            }
        }
    }
}

/// Update client network state.
fn update_client_state(
    state: Res<ClientState>,
    local: Res<LocalClient>,
    mut net_state: ResMut<NetworkState>,
    bandwidth: Res<BandwidthTracker>,
) {
    if *state != ClientState::Connected {
        return;
    }

    net_state.tick = local.tick;
    net_state.bytes_sent = bandwidth.bytes_per_sec_sent as u64;
    net_state.bytes_received = bandwidth.bytes_per_sec_recv as u64;
    net_state.avg_rtt_ms = local.rtt_ms as f32;
}

// ============================================================================
// Client Plugin
// ============================================================================

/// Client networking plugin.
pub struct ClientNetworkPlugin;

impl Plugin for ClientNetworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientState>()
            .init_resource::<LocalClient>()
            .init_resource::<NetworkState>()
            .insert_resource(InputBuffer::new(256))
            .add_event::<Connect>()
            .add_event::<Disconnect>()
            .add_event::<ConnectionEstablished>()
            .add_event::<ConnectionLost>()
            // TransportPlugin added by EustressNetworkingPlugin
            .register_type::<Predicted>()
            .register_type::<Interpolated>()
            .add_systems(
                Update,
                (
                    handle_connect,
                    handle_disconnect,
                    handle_ownership_transfers,
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    sample_and_send_input,
                    apply_prediction,
                    interpolate_remote,
                    handle_state_updates,
                    reconcile_prediction,
                    update_client_state,
                )
                    .chain()
                    .run_if(|state: Res<ClientState>| *state == ClientState::Connected),
            );

        info!("Client network plugin initialized");
    }
}

