// ============================================================================
// Play Server - QUIC Client Implementation
// ============================================================================

use super::protocol::GameMessage;
use bevy::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Client configuration
#[derive(Debug, Clone)]
pub struct PlayClientConfig {
    /// Server address to connect to
    pub server_addr: SocketAddr,
    /// Player name
    pub player_name: String,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
}

impl Default for PlayClientConfig {
    fn default() -> Self {
        Self {
            server_addr: ([127, 0, 0, 1], 7777).into(),
            player_name: "Player".to_string(),
            connect_timeout_secs: 10,
        }
    }
}

/// Client runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

/// Play Client - connects to a Play Server
pub struct PlayClient {
    config: PlayClientConfig,
    state: ClientState,
    endpoint: Option<quinn::Endpoint>,
    connection: Option<quinn::Connection>,
    /// Outbound message sender
    send_tx: Option<mpsc::UnboundedSender<GameMessage>>,
    /// Inbound message receiver
    recv_rx: Option<mpsc::UnboundedReceiver<GameMessage>>,
    /// Assigned session ID from server
    session_id: Option<u64>,
}

impl PlayClient {
    /// Create a new play client
    pub fn new(config: PlayClientConfig) -> Self {
        Self {
            config,
            state: ClientState::Disconnected,
            endpoint: None,
            connection: None,
            send_tx: None,
            recv_rx: None,
            session_id: None,
        }
    }
    
    /// Get current state
    pub fn state(&self) -> ClientState {
        self.state
    }
    
    /// Get session ID (if connected)
    pub fn session_id(&self) -> Option<u64> {
        self.session_id
    }
    
    /// Connect to server
    pub async fn connect(&mut self) -> Result<(), ClientError> {
        if self.state != ClientState::Disconnected {
            return Err(ClientError::AlreadyConnected);
        }
        
        self.state = ClientState::Connecting;
        
        // Create client endpoint
        let mut endpoint = quinn::Endpoint::client(([0, 0, 0, 0], 0).into())
            .map_err(|e| ClientError::EndpointError(e.to_string()))?;
        
        // Configure TLS (skip verification for local dev)
        let crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();
        
        let mut client_config = quinn::ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
                .map_err(|e| ClientError::TlsError(e.to_string()))?
        ));
        
        // Tune for game traffic
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100u32.into());
        transport_config.max_concurrent_uni_streams(100u32.into());
        client_config.transport_config(Arc::new(transport_config));
        
        endpoint.set_default_client_config(client_config);
        
        // Connect with timeout
        let connect_future = endpoint.connect(self.config.server_addr, "localhost")
            .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        
        let connection = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.connect_timeout_secs),
            connect_future
        )
        .await
        .map_err(|_| ClientError::Timeout)?
        .map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        
        info!("🔗 Connected to server at {}", self.config.server_addr);
        
        // Create message channels
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<GameMessage>();
        let (recv_tx, recv_rx) = mpsc::unbounded_channel::<GameMessage>();
        
        // Spawn send task
        let conn_send = connection.clone();
        tokio::spawn(async move {
            while let Some(msg) = send_rx.recv().await {
                if let Ok(mut send) = conn_send.open_uni().await {
                    let data = msg.serialize();
                    let _ = send.write_all(&data).await;
                    let _ = send.finish();
                }
            }
        });
        
        // Spawn receive task
        let conn_recv = connection.clone();
        tokio::spawn(async move {
            loop {
                match conn_recv.accept_uni().await {
                    Ok(mut recv) => {
                        let data = match recv.read_to_end(64 * 1024).await {
                            Ok(d) => d,
                            Err(_) => continue,
                        };
                        
                        if let Some(msg) = GameMessage::deserialize(&data) {
                            let _ = recv_tx.send(msg);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        
        // Send join message
        send_tx.send(GameMessage::Join {
            player_name: self.config.player_name.clone(),
        }).map_err(|_| ClientError::SendFailed)?;
        
        self.endpoint = Some(endpoint);
        self.connection = Some(connection);
        self.send_tx = Some(send_tx);
        self.recv_rx = Some(recv_rx);
        self.state = ClientState::Connected;
        
        Ok(())
    }
    
    /// Disconnect from server
    pub async fn disconnect(&mut self) {
        if self.state == ClientState::Disconnected {
            return;
        }
        
        self.state = ClientState::Disconnecting;
        
        // Send disconnect message
        if let Some(tx) = &self.send_tx {
            let _ = tx.send(GameMessage::Disconnect);
        }
        
        // Close connection
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"client disconnect");
        }
        
        // Close endpoint
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"client disconnect");
        }
        
        self.send_tx = None;
        self.recv_rx = None;
        self.session_id = None;
        self.state = ClientState::Disconnected;
        
        info!("🔌 Disconnected from server");
    }
    
    /// Send a message to the server
    pub fn send(&self, msg: GameMessage) -> Result<(), ClientError> {
        let tx = self.send_tx.as_ref()
            .ok_or(ClientError::NotConnected)?;
        
        tx.send(msg).map_err(|_| ClientError::SendFailed)?;
        Ok(())
    }
    
    /// Receive pending messages (non-blocking)
    pub fn receive(&mut self) -> Vec<GameMessage> {
        let Some(rx) = &mut self.recv_rx else {
            return Vec::new();
        };
        
        let mut messages = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }
        messages
    }
    
    /// Send player input
    pub fn send_input(&self, input: PlayerInput) -> Result<(), ClientError> {
        self.send(GameMessage::PlayerInput(input))
    }
    
    /// Send chat message
    pub fn send_chat(&self, text: String) -> Result<(), ClientError> {
        self.send(GameMessage::ChatMessage { text })
    }
}

/// Player input state
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PlayerInput {
    /// Movement direction (normalized)
    pub movement: [f32; 3],
    /// Look direction (pitch, yaw)
    pub look: [f32; 2],
    /// Jump pressed
    pub jump: bool,
    /// Primary action (e.g., attack)
    pub primary_action: bool,
    /// Secondary action (e.g., block)
    pub secondary_action: bool,
    /// Input sequence number (for reconciliation)
    pub sequence: u32,
    /// Client timestamp
    pub timestamp: f64,
}

/// Client error types
#[derive(Debug, Clone)]
pub enum ClientError {
    AlreadyConnected,
    NotConnected,
    Timeout,
    EndpointError(String),
    TlsError(String),
    ConnectionError(String),
    SendFailed,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyConnected => write!(f, "Already connected"),
            Self::NotConnected => write!(f, "Not connected"),
            Self::Timeout => write!(f, "Connection timeout"),
            Self::EndpointError(e) => write!(f, "Endpoint error: {}", e),
            Self::TlsError(e) => write!(f, "TLS error: {}", e),
            Self::ConnectionError(e) => write!(f, "Connection error: {}", e),
            Self::SendFailed => write!(f, "Send failed"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Skip server certificate verification for local development
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
