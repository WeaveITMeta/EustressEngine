//! # Transport Layer
//!
//! QUIC transport with TLS 1.3 via Lightyear's quinn integration.
//!
//! ## Features
//!
//! - **QUIC**: Modern UDP-based transport with built-in reliability
//! - **TLS 1.3**: Encrypted connections by default
//! - **0-RTT**: Fast reconnection for returning clients
//! - **Connection migration**: Handles IP changes gracefully

use bevy::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::config::TransportConfig;
use crate::error::{NetworkError, NetworkResult};

// ============================================================================
// Certificate Generation
// ============================================================================

/// Generate a self-signed certificate for development.
///
/// For production, use proper certificates from a CA.
pub fn generate_self_signed_cert(
    server_name: &str,
) -> NetworkResult<(Vec<u8>, Vec<u8>)> {
    use rcgen::{CertifiedKey, generate_simple_self_signed};

    let subject_alt_names = vec![
        server_name.to_string(),
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ];

    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(subject_alt_names)
        .map_err(|e| NetworkError::CertificateError(e.to_string()))?;

    let cert_der = cert.der().to_vec();
    let key_der = key_pair.serialize_der();

    Ok((cert_der, key_der))
}

/// Load certificate from files.
pub fn load_cert_from_files(
    cert_path: &str,
    key_path: &str,
) -> NetworkResult<(Vec<u8>, Vec<u8>)> {
    let cert = std::fs::read(cert_path)
        .map_err(|e| NetworkError::CertificateError(format!("Failed to read cert: {}", e)))?;
    let key = std::fs::read(key_path)
        .map_err(|e| NetworkError::CertificateError(format!("Failed to read key: {}", e)))?;
    Ok((cert, key))
}

// ============================================================================
// Transport State
// ============================================================================

/// Transport layer state.
#[derive(Resource, Debug)]
pub struct TransportState {
    /// Server address
    pub server_addr: SocketAddr,
    /// Is server running
    pub is_server: bool,
    /// Is client connected
    pub is_connected: bool,
    /// Current RTT in milliseconds
    pub rtt_ms: u32,
    /// Bytes sent this tick
    pub bytes_sent: u64,
    /// Bytes received this tick
    pub bytes_received: u64,
    /// Packet loss percentage
    pub packet_loss: f32,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:4433".parse().unwrap(),
            is_server: false,
            is_connected: false,
            rtt_ms: 0,
            bytes_sent: 0,
            bytes_received: 0,
            packet_loss: 0.0,
        }
    }
}

// ============================================================================
// Connection Events
// ============================================================================

/// Client connected event.
#[derive(Event, Message, Debug, Clone)]
pub struct ClientConnected {
    pub client_id: u64,
    pub addr: SocketAddr,
}

/// Client disconnected event.
#[derive(Event, Message, Debug, Clone)]
pub struct ClientDisconnected {
    pub client_id: u64,
    pub reason: String,
}

/// Connection established (client-side).
#[derive(Event, Message, Debug, Clone)]
pub struct Connected {
    pub server_addr: SocketAddr,
}

/// Connection lost (client-side).
#[derive(Event, Message, Debug, Clone)]
pub struct Disconnected {
    pub reason: String,
}

// ============================================================================
// Transport Configuration Builder
// ============================================================================

/// Builder for transport configuration.
#[derive(Debug, Clone)]
pub struct TransportBuilder {
    config: TransportConfig,
    cert: Option<Vec<u8>>,
    key: Option<Vec<u8>>,
}

impl TransportBuilder {
    /// Create new builder with defaults.
    pub fn new() -> Self {
        Self {
            config: TransportConfig::default(),
            cert: None,
            key: None,
        }
    }

    /// Set server address.
    pub fn server_addr(mut self, addr: SocketAddr) -> Self {
        self.config.server_addr = addr;
        self
    }

    /// Set maximum connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set connection timeout.
    pub fn timeout_secs(mut self, secs: u32) -> Self {
        self.config.connection_timeout_secs = secs;
        self
    }

    /// Use self-signed certificate (development only).
    pub fn self_signed(mut self) -> NetworkResult<Self> {
        let (cert, key) = generate_self_signed_cert(&self.config.server_name)?;
        self.cert = Some(cert);
        self.key = Some(key);
        Ok(self)
    }

    /// Use certificate from files.
    pub fn cert_files(mut self, cert_path: &str, key_path: &str) -> NetworkResult<Self> {
        let (cert, key) = load_cert_from_files(cert_path, key_path)?;
        self.cert = Some(cert);
        self.key = Some(key);
        Ok(self)
    }

    /// Build the configuration.
    pub fn build(self) -> (TransportConfig, Option<Vec<u8>>, Option<Vec<u8>>) {
        (self.config, self.cert, self.key)
    }
}

impl Default for TransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Bandwidth Tracking
// ============================================================================

/// Tracks bandwidth usage.
#[derive(Resource, Debug, Default)]
pub struct BandwidthTracker {
    /// Bytes sent per second (rolling average)
    pub bytes_per_sec_sent: f64,
    /// Bytes received per second (rolling average)
    pub bytes_per_sec_recv: f64,
    /// Peak bytes/sec sent
    pub peak_sent: f64,
    /// Peak bytes/sec received
    pub peak_recv: f64,
    /// Accumulator for current second
    sent_accumulator: u64,
    recv_accumulator: u64,
    /// Time since last reset
    last_reset: f64,
}

impl BandwidthTracker {
    /// Record bytes sent.
    pub fn record_sent(&mut self, bytes: u64) {
        self.sent_accumulator += bytes;
    }

    /// Record bytes received.
    pub fn record_recv(&mut self, bytes: u64) {
        self.recv_accumulator += bytes;
    }

    /// Update per-second stats.
    pub fn update(&mut self, delta_secs: f64) {
        self.last_reset += delta_secs;

        if self.last_reset >= 1.0 {
            // Calculate bytes/sec with smoothing
            let alpha = 0.3; // Smoothing factor
            let sent = self.sent_accumulator as f64 / self.last_reset;
            let recv = self.recv_accumulator as f64 / self.last_reset;

            self.bytes_per_sec_sent = self.bytes_per_sec_sent * (1.0 - alpha) + sent * alpha;
            self.bytes_per_sec_recv = self.bytes_per_sec_recv * (1.0 - alpha) + recv * alpha;

            // Update peaks
            self.peak_sent = self.peak_sent.max(sent);
            self.peak_recv = self.peak_recv.max(recv);

            // Reset accumulators
            self.sent_accumulator = 0;
            self.recv_accumulator = 0;
            self.last_reset = 0.0;
        }
    }

    /// Get human-readable bandwidth string.
    pub fn format_bandwidth(&self) -> String {
        fn format_bytes(bytes: f64) -> String {
            if bytes >= 1_000_000.0 {
                format!("{:.2} MB/s", bytes / 1_000_000.0)
            } else if bytes >= 1_000.0 {
                format!("{:.2} KB/s", bytes / 1_000.0)
            } else {
                format!("{:.0} B/s", bytes)
            }
        }

        format!(
            "↑{} ↓{}",
            format_bytes(self.bytes_per_sec_sent),
            format_bytes(self.bytes_per_sec_recv)
        )
    }
}

/// Update bandwidth tracker.
pub fn update_bandwidth_tracker(time: Res<Time>, mut tracker: ResMut<BandwidthTracker>) {
    tracker.update(time.delta_secs_f64());
}

// ============================================================================
// Transport Plugin
// ============================================================================

/// Plugin for transport layer.
/// 
/// This plugin is safe to add multiple times - it will only initialize once.
pub struct TransportPlugin;

impl Plugin for TransportPlugin {
    fn build(&self, app: &mut App) {
        // Guard against duplicate registration (both server and client plugins add this)
        if app.world().contains_resource::<TransportState>() {
            return;
        }
        
        app.init_resource::<TransportState>()
            .init_resource::<BandwidthTracker>()
            .add_event::<ClientConnected>()
            .add_event::<ClientDisconnected>()
            .add_event::<Connected>()
            .add_event::<Disconnected>()
            .add_systems(Update, update_bandwidth_tracker);
    }
}

