// ============================================================================
// Eustress Engine - Play Server
// Local QUIC-based game server for multiplayer testing
// ============================================================================
//!
//! # Play Server
//!
//! In-process game server for local multiplayer testing when clicking "Play" in the editor.
//! Uses QUIC protocol for low-latency, reliable communication.
//!
//! ## Architecture
//!
//! - **Server**: Runs on user's local machine, manages authoritative game state
//! - **Clients**: Connect via QUIC, receive state updates, send inputs
//! - **Replication**: Server-authoritative with client-side prediction
//! - **Sessions**: Track connected players with unique IDs
//!
//! ## Integration with Forge
//!
//! Uses Forge SDK for:
//! - Port allocation (finds available port automatically)
//! - Lifecycle management (graceful shutdown)
//! - Session tracking
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In play_mode.rs when PlayModeType::Server is selected:
//! app.add_plugins(PlayServerPlugin);
//! commands.send_event(StartPlayServerMessage { port: 0, max_players: 8 });
//! ```

pub mod server;
pub mod client;
pub mod protocol;
pub mod replication;
pub mod session;

pub use server::{PlayServer, PlayServerConfig, ServerState};
pub use client::{PlayClient, PlayClientConfig, ClientState};
pub use protocol::{GameMessage, MessageChannel, ReplicationMessage};
pub use replication::{ReplicatedEntity, ReplicationPlugin, NetworkId};
pub use session::{PlayerSession, SessionManager};

use bevy::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Plugin for running an in-process server during Play Server mode
pub struct PlayServerPlugin;

impl Plugin for PlayServerPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<PlayServerState>()
            .init_resource::<SessionManager>()
            // Events
            .add_message::<StartPlayServerMessage>()
            .add_message::<StopPlayServerMessage>()
            .add_message::<PlayerConnectedEvent>()
            .add_message::<PlayerDisconnectedEvent>()
            // ReplicationMessage events handled by replication plugin
            // Systems
            .add_systems(Update, (
                handle_start_server,
                handle_stop_server,
                process_incoming_connections,
                process_client_messages,
                broadcast_replication_updates,
            ).chain())
            // Add replication plugin
            .add_plugins(ReplicationPlugin);
        
        info!("🎮 Play Server Plugin initialized - Local multiplayer ready");
    }
}

/// Play server runtime state
#[derive(Resource, Default)]
pub struct PlayServerState {
    /// Server instance (if running)
    pub server: Option<Arc<RwLock<PlayServer>>>,
    /// Current server state
    pub state: ServerState,
    /// Bound port (0 = not running)
    pub port: u16,
    /// Connected player count
    pub player_count: u32,
    /// Max players allowed
    pub max_players: u32,
    /// Tokio runtime handle for async operations
    pub runtime: Option<tokio::runtime::Handle>,
}

/// Message to start the play server
#[derive(Event, Message, Debug, Clone)]
pub struct StartPlayServerMessage {
    /// Port to bind (0 = auto-allocate)
    pub port: u16,
    /// Maximum players allowed
    pub max_players: u32,
}

impl Default for StartPlayServerMessage {
    fn default() -> Self {
        Self {
            port: 0, // Auto-allocate
            max_players: 8,
        }
    }
}

/// Message to stop the play server
#[derive(Event, Message, Debug, Clone, Default)]
pub struct StopPlayServerMessage;

/// Event fired when a player connects
#[derive(Event, Message, Debug, Clone)]
pub struct PlayerConnectedEvent {
    /// Unique session ID
    pub session_id: u64,
    /// Player display name
    pub player_name: String,
    /// Remote address
    pub remote_addr: String,
}

/// Event fired when a player disconnects
#[derive(Event, Message, Debug, Clone)]
pub struct PlayerDisconnectedEvent {
    /// Session ID of disconnected player
    pub session_id: u64,
    /// Reason for disconnect
    pub reason: DisconnectReason,
}

/// Reason for player disconnect
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// Player quit normally
    Quit,
    /// Connection timed out
    Timeout,
    /// Kicked by server
    Kicked(String),
    /// Server shutting down
    ServerShutdown,
    /// Network error
    NetworkError(String),
}

// ============================================================================
// Systems
// ============================================================================

/// Handle start server requests
fn handle_start_server(
    mut events: MessageReader<StartPlayServerMessage>,
    mut server_state: ResMut<PlayServerState>,
) {
    for event in events.read() {
        if server_state.state != ServerState::Stopped {
            warn!("Play server already running, ignoring start request");
            continue;
        }
        
        info!("🚀 Starting Play Server on port {} (max {} players)", 
            if event.port == 0 { "auto".to_string() } else { event.port.to_string() },
            event.max_players
        );
        
        // Create tokio runtime for async networking
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");
        
        let handle = runtime.handle().clone();
        
        // Allocate port - use specified or default
        let port = if event.port == 0 {
            // Default port for local play server
            7777
        } else {
            event.port
        };
        
        // Create server config
        let config = PlayServerConfig {
            port,
            max_players: event.max_players,
            tick_rate: 60,
            ..Default::default()
        };
        
        // Start server in background
        let server = Arc::new(RwLock::new(PlayServer::new(config)));
        let server_clone = Arc::clone(&server);
        
        handle.spawn(async move {
            let mut srv = server_clone.write().await;
            if let Err(e) = srv.start().await {
                error!("Failed to start play server: {}", e);
            }
        });
        
        server_state.server = Some(server);
        server_state.state = ServerState::Starting;
        server_state.port = port;
        server_state.max_players = event.max_players;
        server_state.runtime = Some(handle);
        
        // Keep runtime alive
        std::mem::forget(runtime);
        
        info!("✅ Play Server starting on port {}", port);
    }
}

/// Handle stop server requests
fn handle_stop_server(
    mut events: MessageReader<StopPlayServerMessage>,
    mut server_state: ResMut<PlayServerState>,
    mut disconnect_events: MessageWriter<PlayerDisconnectedEvent>,
    session_manager: Res<SessionManager>,
) {
    for _event in events.read() {
        if server_state.state == ServerState::Stopped {
            continue;
        }
        
        info!("🛑 Stopping Play Server...");
        
        // Notify all connected players
        for session in session_manager.sessions.iter() {
            disconnect_events.write(PlayerDisconnectedEvent {
                session_id: *session.key(),
                reason: DisconnectReason::ServerShutdown,
            });
        }
        
        // Stop server
        if let Some(server) = &server_state.server {
            if let Some(handle) = &server_state.runtime {
                let server_clone = Arc::clone(server);
                handle.spawn(async move {
                    let mut srv = server_clone.write().await;
                    srv.stop().await;
                });
            }
        }
        
        // Port is released when server stops
        
        server_state.server = None;
        server_state.state = ServerState::Stopped;
        server_state.port = 0;
        server_state.player_count = 0;
        
        info!("✅ Play Server stopped");
    }
}

/// Process incoming player connections
fn process_incoming_connections(
    server_state: Res<PlayServerState>,
    mut session_manager: ResMut<SessionManager>,
    mut connect_events: MessageWriter<PlayerConnectedEvent>,
) {
    let Some(server) = &server_state.server else { return };
    let Some(handle) = &server_state.runtime else { return };
    
    // Poll for new connections (non-blocking)
    let server_clone = Arc::clone(server);
    let pending = handle.block_on(async {
        let srv = server_clone.read().await;
        srv.pending_connections()
    });
    
    for conn_info in pending {
        let session_id = session_manager.create_session(conn_info.clone());
        
        connect_events.write(PlayerConnectedEvent {
            session_id,
            player_name: conn_info.player_name,
            remote_addr: conn_info.remote_addr,
        });
        
        info!("👤 Player connected: session {}", session_id);
    }
}

/// Process messages from connected clients
fn process_client_messages(
    server_state: Res<PlayServerState>,
    session_manager: Res<SessionManager>,
    mut commands: Commands,
) {
    let Some(server) = &server_state.server else { return };
    let Some(handle) = &server_state.runtime else { return };
    
    // Poll for messages from all sessions
    for session in session_manager.sessions.iter() {
        let session_id = *session.key();
        let server_clone = Arc::clone(server);
        
        let messages = handle.block_on(async {
            let srv = server_clone.read().await;
            srv.receive_messages(session_id)
        });
        
        for msg in messages {
            match msg {
                GameMessage::PlayerInput(input) => {
                    // Apply player input to their character
                    // This will be handled by the replication system
                }
                GameMessage::ChatMessage { text } => {
                    info!("💬 [{}]: {}", session_id, text);
                }
                GameMessage::Disconnect => {
                    // Handle disconnect
                }
                _ => {}
            }
        }
    }
}

/// Broadcast entity state updates to all clients
fn broadcast_replication_updates(
    server_state: Res<PlayServerState>,
    session_manager: Res<SessionManager>,
    replicated_query: Query<(&NetworkId, &Transform, Option<&replication::ReplicatedComponents>), Changed<Transform>>,
) {
    let Some(server) = &server_state.server else { return };
    let Some(handle) = &server_state.runtime else { return };
    
    if replicated_query.is_empty() {
        return;
    }
    
    // Collect all changed entities
    let mut updates = Vec::new();
    for (net_id, transform, components) in replicated_query.iter() {
        updates.push(ReplicationMessage::EntityUpdate {
            network_id: net_id.0,
            transform: *transform,
            components: components.cloned(),
        });
    }
    
    if updates.is_empty() {
        return;
    }
    
    // Broadcast to all connected clients
    let server_clone = Arc::clone(server);
    let session_ids: Vec<u64> = session_manager.sessions.iter().map(|s| *s.key()).collect();
    
    handle.spawn(async move {
        let srv = server_clone.read().await;
        for session_id in session_ids {
            for update in &updates {
                let _ = srv.send_message(session_id, GameMessage::Replication(update.clone())).await;
            }
        }
    });
}
