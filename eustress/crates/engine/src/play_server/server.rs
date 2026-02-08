// ============================================================================
// Play Server - QUIC Server Implementation
// ============================================================================

use super::protocol::GameMessage;
use super::session::ConnectionInfo;
use bevy::prelude::*;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Server configuration
#[derive(Debug, Clone)]
pub struct PlayServerConfig {
    /// Port to bind (0 = auto-allocate)
    pub port: u16,
    /// Maximum concurrent players
    pub max_players: u32,
    /// Server tick rate (Hz)
    pub tick_rate: u32,
    /// Server name for discovery
    pub server_name: String,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
}

impl Default for PlayServerConfig {
    fn default() -> Self {
        Self {
            port: 7777,
            max_players: 8,
            tick_rate: 60,
            server_name: "Eustress Play Server".to_string(),
            idle_timeout_secs: 30,
        }
    }
}

/// Server runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    #[default]
    Stopped,
    Starting,
    Running,
    ShuttingDown,
}

/// Connected client handle
struct ClientConnection {
    /// QUIC connection
    connection: quinn::Connection,
    /// Outbound message channel
    send_tx: mpsc::UnboundedSender<GameMessage>,
    /// Session ID
    session_id: u64,
    /// Player info
    info: ConnectionInfo,
}

/// Play Server - QUIC-based game server
pub struct PlayServer {
    config: PlayServerConfig,
    state: ServerState,
    endpoint: Option<quinn::Endpoint>,
    /// Connected clients by session ID
    clients: HashMap<u64, ClientConnection>,
    /// Pending connection info (polled by Bevy systems)
    pending_connections: Vec<ConnectionInfo>,
    /// Incoming messages from clients
    incoming_messages: HashMap<u64, Vec<GameMessage>>,
    /// Next session ID
    next_session_id: u64,
}

impl PlayServer {
    /// Create a new play server
    pub fn new(config: PlayServerConfig) -> Self {
        Self {
            config,
            state: ServerState::Stopped,
            endpoint: None,
            clients: HashMap::new(),
            pending_connections: Vec::new(),
            incoming_messages: HashMap::new(),
            next_session_id: 1,
        }
    }
    
    /// Get current state
    pub fn state(&self) -> ServerState {
        self.state
    }
    
    /// Get bound port
    pub fn port(&self) -> u16 {
        self.endpoint
            .as_ref()
            .and_then(|e| e.local_addr().ok())
            .map(|a| a.port())
            .unwrap_or(0)
    }
    
    /// Get connected player count
    pub fn player_count(&self) -> usize {
        self.clients.len()
    }
    
    /// Start the server
    pub async fn start(&mut self) -> Result<(), ServerError> {
        if self.state != ServerState::Stopped {
            return Err(ServerError::AlreadyRunning);
        }
        
        self.state = ServerState::Starting;
        
        // Generate self-signed certificate for local development
        let (cert, key) = generate_self_signed_cert()
            .map_err(|e| ServerError::TlsError(e.to_string()))?;
        
        // Configure TLS
        let mut server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(|e| ServerError::TlsError(e.to_string()))?;
        
        server_crypto.alpn_protocols = vec![b"eustress-play".to_vec()];
        
        // Configure QUIC
        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .map_err(|e| ServerError::QuicError(e.to_string()))?
        ));
        
        // Tune for game traffic
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_concurrent_bidi_streams(100u32.into());
        transport_config.max_concurrent_uni_streams(100u32.into());
        transport_config.max_idle_timeout(Some(
            std::time::Duration::from_secs(self.config.idle_timeout_secs).try_into().unwrap()
        ));
        
        // Bind endpoint
        let bind_addr: SocketAddr = ([0, 0, 0, 0], self.config.port).into();
        let endpoint = quinn::Endpoint::server(server_config, bind_addr)
            .map_err(|e| ServerError::BindError(e.to_string()))?;
        
        let actual_port = endpoint.local_addr()
            .map_err(|e| ServerError::BindError(e.to_string()))?
            .port();
        
        info!("🎮 Play Server listening on port {}", actual_port);
        
        self.endpoint = Some(endpoint);
        self.state = ServerState::Running;
        
        Ok(())
    }
    
    /// Stop the server
    pub async fn stop(&mut self) {
        if self.state == ServerState::Stopped {
            return;
        }
        
        self.state = ServerState::ShuttingDown;
        
        // Disconnect all clients
        for (session_id, client) in self.clients.drain() {
            let _ = client.send_tx.send(GameMessage::Disconnect);
            client.connection.close(0u32.into(), b"server shutdown");
            info!("Disconnected client {}", session_id);
        }
        
        // Close endpoint
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"server shutdown");
        }
        
        self.state = ServerState::Stopped;
        info!("🛑 Play Server stopped");
    }
    
    /// Accept a new connection (called from async task)
    pub async fn accept_connection(&mut self) -> Result<u64, ServerError> {
        let endpoint = self.endpoint.as_ref()
            .ok_or(ServerError::NotRunning)?;
        
        // Check player limit
        if self.clients.len() >= self.config.max_players as usize {
            return Err(ServerError::ServerFull);
        }
        
        // Accept incoming connection
        let incoming = endpoint.accept().await
            .ok_or(ServerError::EndpointClosed)?;
        
        let connection = incoming.await
            .map_err(|e| ServerError::ConnectionError(e.to_string()))?;
        
        let remote_addr = connection.remote_address().to_string();
        
        // Create session
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        
        // Create message channel
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<GameMessage>();
        
        // Spawn send task
        let conn_clone = connection.clone();
        tokio::spawn(async move {
            while let Some(msg) = send_rx.recv().await {
                if let Ok(mut send) = conn_clone.open_uni().await {
                    let data = msg.serialize();
                    let _ = send.write_all(&data).await;
                    let _ = send.finish();
                }
            }
        });
        
        // Create connection info
        let info = ConnectionInfo {
            session_id,
            player_name: format!("Player{}", session_id),
            remote_addr: remote_addr.clone(),
        };
        
        // Store client
        self.clients.insert(session_id, ClientConnection {
            connection,
            send_tx,
            session_id,
            info: info.clone(),
        });
        
        // Add to pending (for Bevy to pick up)
        self.pending_connections.push(info);
        
        info!("👤 Client connected: session {} from {}", session_id, remote_addr);
        
        Ok(session_id)
    }
    
    /// Get pending connections (drains the queue)
    pub fn pending_connections(&self) -> Vec<ConnectionInfo> {
        // Note: In real impl, this would drain. For now, clone.
        self.pending_connections.clone()
    }
    
    /// Clear pending connections
    pub fn clear_pending_connections(&mut self) {
        self.pending_connections.clear();
    }
    
    /// Receive messages from a specific session
    pub fn receive_messages(&self, session_id: u64) -> Vec<GameMessage> {
        self.incoming_messages
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
    }
    
    /// Send a message to a specific client
    pub async fn send_message(&self, session_id: u64, msg: GameMessage) -> Result<(), ServerError> {
        let client = self.clients.get(&session_id)
            .ok_or(ServerError::SessionNotFound(session_id))?;
        
        client.send_tx.send(msg)
            .map_err(|_| ServerError::SendFailed)?;
        
        Ok(())
    }
    
    /// Broadcast a message to all clients
    pub async fn broadcast(&self, msg: GameMessage) {
        for client in self.clients.values() {
            let _ = client.send_tx.send(msg.clone());
        }
    }
    
    /// Broadcast a message to all clients except one
    pub async fn broadcast_except(&self, msg: GameMessage, except_session: u64) {
        for (session_id, client) in &self.clients {
            if *session_id != except_session {
                let _ = client.send_tx.send(msg.clone());
            }
        }
    }
    
    /// Disconnect a client
    pub async fn disconnect(&mut self, session_id: u64, reason: &str) {
        if let Some(client) = self.clients.remove(&session_id) {
            let _ = client.send_tx.send(GameMessage::Disconnect);
            client.connection.close(0u32.into(), reason.as_bytes());
            info!("Disconnected client {}: {}", session_id, reason);
        }
    }
}

/// Server error types
#[derive(Debug, Clone)]
pub enum ServerError {
    AlreadyRunning,
    NotRunning,
    ServerFull,
    EndpointClosed,
    BindError(String),
    TlsError(String),
    QuicError(String),
    ConnectionError(String),
    SessionNotFound(u64),
    SendFailed,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "Server already running"),
            Self::NotRunning => write!(f, "Server not running"),
            Self::ServerFull => write!(f, "Server full"),
            Self::EndpointClosed => write!(f, "Endpoint closed"),
            Self::BindError(e) => write!(f, "Bind error: {}", e),
            Self::TlsError(e) => write!(f, "TLS error: {}", e),
            Self::QuicError(e) => write!(f, "QUIC error: {}", e),
            Self::ConnectionError(e) => write!(f, "Connection error: {}", e),
            Self::SessionNotFound(id) => write!(f, "Session {} not found", id),
            Self::SendFailed => write!(f, "Send failed"),
        }
    }
}

impl std::error::Error for ServerError {}

/// Generate self-signed certificate for local development
fn generate_self_signed_cert() -> Result<(
    rustls::pki_types::CertificateDer<'static>,
    rustls::pki_types::PrivateKeyDer<'static>,
), Box<dyn std::error::Error>> {
    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ])?;
    
    let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der())?;
    
    Ok((cert_der, key_der))
}
