//! Networking module for Forge
//!
//! ## Table of Contents
//! - **QuicTransport**: QUIC-based peer communication (requires `quic` feature)
//! - **HttpServer**: Axum-based HTTP/REST API server
//! - **GrpcServer**: Tonic-based gRPC server (placeholder)

use crate::error::{ForgeError, Result};
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[cfg(feature = "quic")]
use std::sync::Arc as QuicArc;

/// HTTP server configuration
#[derive(Debug, Clone)]
pub struct HttpServerConfig {
    /// Bind address
    pub bind_addr: SocketAddr,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: ([0, 0, 0, 0], 8080).into(),
            cors_enabled: true,
            timeout_secs: 30,
        }
    }
}

impl HttpServerConfig {
    /// Create with custom bind address
    pub fn with_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Parse from string address
    pub fn with_addr_str(mut self, addr: &str) -> Result<Self> {
        self.bind_addr = addr
            .parse()
            .map_err(|e| ForgeError::config(format!("Invalid address: {}", e)))?;
        Ok(self)
    }
}

/// Shared state for HTTP handlers
pub struct HttpState<T> {
    /// Application state
    pub app: Arc<RwLock<T>>,
}

impl<T> Clone for HttpState<T> {
    fn clone(&self) -> Self {
        Self {
            app: Arc::clone(&self.app),
        }
    }
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

/// Create the base router with health endpoints
pub fn base_router<T: Send + Sync + 'static>() -> Router<HttpState<T>> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: 0, // TODO: Track actual uptime
    })
}

async fn ready_handler() -> StatusCode {
    StatusCode::OK
}

/// HTTP server wrapper
pub struct HttpServer {
    config: HttpServerConfig,
    router: Router,
}

impl HttpServer {
    /// Create a new HTTP server
    pub fn new(config: HttpServerConfig) -> Self {
        Self {
            config,
            router: Router::new(),
        }
    }

    /// Set the router
    pub fn with_router(mut self, router: Router) -> Self {
        self.router = router;
        self
    }

    /// Start the server
    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(self.config.bind_addr)
            .await
            .map_err(|e| ForgeError::network(format!("Failed to bind: {}", e)))?;

        info!(addr = %self.config.bind_addr, "HTTP server starting");

        axum::serve(listener, self.router)
            .await
            .map_err(|e| ForgeError::network(format!("Server error: {}", e)))?;

        Ok(())
    }
}

/// QUIC transport configuration
#[cfg(feature = "quic")]
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// Bind address
    pub bind_addr: SocketAddr,
    /// Server name for TLS
    pub server_name: String,
    /// Max concurrent streams
    pub max_streams: u32,
    /// Idle timeout in seconds
    pub idle_timeout_secs: u64,
}

#[cfg(feature = "quic")]
impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            bind_addr: ([0, 0, 0, 0], 4433).into(),
            server_name: "forge".to_string(),
            max_streams: 100,
            idle_timeout_secs: 30,
        }
    }
}

/// QUIC transport for peer communication
#[cfg(feature = "quic")]
pub struct QuicTransport {
    config: QuicConfig,
    endpoint: Option<quinn::Endpoint>,
}

#[cfg(feature = "quic")]
impl QuicTransport {
    /// Create a new QUIC transport
    pub fn new(config: QuicConfig) -> Self {
        Self {
            config,
            endpoint: None,
        }
    }

    /// Generate self-signed certificate for development
    fn generate_self_signed_cert() -> Result<(rustls::pki_types::CertificateDer<'static>, rustls::pki_types::PrivateKeyDer<'static>)> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .map_err(|e| ForgeError::network(format!("Failed to generate cert: {}", e)))?;

        let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
        let key_der = rustls::pki_types::PrivateKeyDer::try_from(cert.key_pair.serialize_der())
            .map_err(|e| ForgeError::network(format!("Failed to serialize key: {}", e)))?;

        Ok((cert_der, key_der))
    }

    /// Start as server
    pub async fn start_server(&mut self) -> Result<()> {
        let (cert, key) = Self::generate_self_signed_cert()?;

        let mut server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(|e| ForgeError::network(format!("TLS config error: {}", e)))?;

        server_crypto.alpn_protocols = vec![b"forge".to_vec()];

        let server_config = quinn::ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .map_err(|e| ForgeError::network(format!("QUIC config error: {}", e)))?,
        ));

        let endpoint = quinn::Endpoint::server(server_config, self.config.bind_addr)
            .map_err(|e| ForgeError::network(format!("Failed to create endpoint: {}", e)))?;

        info!(addr = %self.config.bind_addr, "QUIC server started");
        self.endpoint = Some(endpoint);

        Ok(())
    }

    /// Accept incoming connections
    pub async fn accept(&self) -> Result<quinn::Connection> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| ForgeError::network("Endpoint not started"))?;

        let incoming = endpoint
            .accept()
            .await
            .ok_or_else(|| ForgeError::network("Endpoint closed"))?;

        let conn = incoming
            .await
            .map_err(|e| ForgeError::network(format!("Connection error: {}", e)))?;

        info!(remote = %conn.remote_address(), "QUIC connection accepted");
        Ok(conn)
    }

    /// Connect to a peer
    pub async fn connect(&self, addr: SocketAddr) -> Result<quinn::Connection> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| ForgeError::network("Endpoint not started"))?;

        let conn = endpoint
            .connect(addr, &self.config.server_name)
            .map_err(|e| ForgeError::network(format!("Connect error: {}", e)))?
            .await
            .map_err(|e| ForgeError::network(format!("Connection error: {}", e)))?;

        info!(remote = %addr, "QUIC connection established");
        Ok(conn)
    }

    /// Get local address
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.endpoint.as_ref().and_then(|e| e.local_addr().ok())
    }

    /// Close the transport
    pub fn close(&self) {
        if let Some(endpoint) = &self.endpoint {
            endpoint.close(0u32.into(), b"shutdown");
        }
    }
}

/// Message types for peer communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeerMessage {
    /// Heartbeat ping
    Ping { node_id: String, timestamp: u64 },
    /// Heartbeat pong
    Pong { node_id: String, timestamp: u64 },
    /// Route request to expert
    RouteRequest { request_id: String, input: String },
    /// Route response from expert
    RouteResponse {
        request_id: String,
        expert_index: usize,
        result: Vec<u8>,
    },
    /// Shard assignment notification
    ShardAssign { shard_id: u64, node_id: String },
    /// Shard migration request
    ShardMigrate {
        shard_id: u64,
        from_node: String,
        to_node: String,
    },
}

impl PeerMessage {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| ForgeError::network(format!("Serialize error: {}", e)))
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| ForgeError::network(format!("Deserialize error: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_config_default() {
        let config = HttpServerConfig::default();
        assert_eq!(config.bind_addr.port(), 8080);
        assert!(config.cors_enabled);
    }

    #[test]
    #[cfg(feature = "quic")]
    fn test_quic_config_default() {
        let config = QuicConfig::default();
        assert_eq!(config.bind_addr.port(), 4433);
        assert_eq!(config.server_name, "forge");
    }

    #[test]
    fn test_peer_message_serialization() {
        let msg = PeerMessage::Ping {
            node_id: "node-1".to_string(),
            timestamp: 12345,
        };

        let bytes = msg.to_bytes().unwrap();
        let decoded = PeerMessage::from_bytes(&bytes).unwrap();

        match decoded {
            PeerMessage::Ping { node_id, timestamp } => {
                assert_eq!(node_id, "node-1");
                assert_eq!(timestamp, 12345);
            }
            _ => panic!("Wrong message type"),
        }
    }
}
