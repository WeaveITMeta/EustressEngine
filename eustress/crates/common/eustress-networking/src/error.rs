//! # Network Errors
//!
//! Error types for Eustress networking.

use thiserror::Error;

/// Network error types.
#[derive(Error, Debug, Clone)]
pub enum NetworkError {
    // ========================================================================
    // Connection Errors
    // ========================================================================
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Connection closed: {0}")]
    ConnectionClosed(String),

    #[error("Connection timeout after {0}ms")]
    ConnectionTimeout(u32),

    #[error("Maximum connections reached ({0})")]
    MaxConnectionsReached(usize),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    // ========================================================================
    // Transport Errors
    // ========================================================================

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("TLS error: {0}")]
    TlsError(String),

    #[error("Certificate error: {0}")]
    CertificateError(String),

    #[error("Send failed: {0}")]
    SendFailed(String),

    #[error("Receive failed: {0}")]
    ReceiveFailed(String),

    #[error("Packet too large: {size} > {max}")]
    PacketTooLarge { size: usize, max: usize },

    // ========================================================================
    // Protocol Errors
    // ========================================================================

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    #[error("Version mismatch: client={client}, server={server}")]
    VersionMismatch { client: u32, server: u32 },

    // ========================================================================
    // Replication Errors
    // ========================================================================

    #[error("Entity not found: {0:?}")]
    EntityNotFound(bevy::prelude::Entity),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("Replication failed: {0}")]
    ReplicationFailed(String),

    #[error("Snapshot too old: tick {requested} < {oldest}")]
    SnapshotTooOld { requested: u64, oldest: u64 },

    // ========================================================================
    // Ownership Errors
    // ========================================================================

    #[error("Ownership denied: {0}")]
    OwnershipDenied(String),

    #[error("Ownership transfer failed: {0}")]
    OwnershipTransferFailed(String),

    #[error("Not owner of entity {0:?}")]
    NotOwner(bevy::prelude::Entity),

    #[error("Ownership request timeout")]
    OwnershipTimeout,

    #[error("Entity already owned by client {0}")]
    AlreadyOwned(u64),

    // ========================================================================
    // Validation Errors (Anti-Exploit)
    // ========================================================================

    #[error("Speed violation: {speed} > {max} studs/s")]
    SpeedViolation { speed: f32, max: f32 },

    #[error("Acceleration violation: {accel} > {max} studs/s²")]
    AccelerationViolation { accel: f32, max: f32 },

    #[error("Teleport violation: {distance} > {max} studs")]
    TeleportViolation { distance: f32, max: f32 },

    #[error("Position out of bounds: {0:?}")]
    OutOfBounds(bevy::prelude::Vec3),

    #[error("Input rate exceeded: {rate}/s > {max}/s")]
    InputRateExceeded { rate: u32, max: u32 },

    #[error("Client kicked: {0}")]
    Kicked(String),

    // ========================================================================
    // State Errors
    // ========================================================================

    #[error("Not connected")]
    NotConnected,

    #[error("Already connected")]
    AlreadyConnected,

    #[error("Server not running")]
    ServerNotRunning,

    #[error("Invalid state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },
}

impl NetworkError {
    /// Check if this error is recoverable (can retry).
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            NetworkError::ConnectionTimeout(_)
                | NetworkError::SendFailed(_)
                | NetworkError::OwnershipTimeout
                | NetworkError::SnapshotTooOld { .. }
        )
    }

    /// Check if this error should disconnect the client.
    pub fn should_disconnect(&self) -> bool {
        matches!(
            self,
            NetworkError::AuthenticationFailed(_)
                | NetworkError::VersionMismatch { .. }
                | NetworkError::Kicked(_)
                | NetworkError::SpeedViolation { .. }
                | NetworkError::TeleportViolation { .. }
        )
    }

    /// Get error code for network transmission.
    pub fn code(&self) -> u16 {
        match self {
            NetworkError::ConnectionFailed(_) => 1000,
            NetworkError::ConnectionClosed(_) => 1001,
            NetworkError::ConnectionTimeout(_) => 1002,
            NetworkError::MaxConnectionsReached(_) => 1003,
            NetworkError::AuthenticationFailed(_) => 1004,
            NetworkError::TransportError(_) => 2000,
            NetworkError::TlsError(_) => 2001,
            NetworkError::CertificateError(_) => 2002,
            NetworkError::SendFailed(_) => 2003,
            NetworkError::ReceiveFailed(_) => 2004,
            NetworkError::PacketTooLarge { .. } => 2005,
            NetworkError::ProtocolError(_) => 3000,
            NetworkError::SerializationError(_) => 3001,
            NetworkError::DeserializationError(_) => 3002,
            NetworkError::InvalidMessageType(_) => 3003,
            NetworkError::VersionMismatch { .. } => 3004,
            NetworkError::EntityNotFound(_) => 4000,
            NetworkError::ComponentNotFound(_) => 4001,
            NetworkError::ReplicationFailed(_) => 4002,
            NetworkError::SnapshotTooOld { .. } => 4003,
            NetworkError::OwnershipDenied(_) => 5000,
            NetworkError::OwnershipTransferFailed(_) => 5001,
            NetworkError::NotOwner(_) => 5002,
            NetworkError::OwnershipTimeout => 5003,
            NetworkError::AlreadyOwned(_) => 5004,
            NetworkError::SpeedViolation { .. } => 6000,
            NetworkError::AccelerationViolation { .. } => 6001,
            NetworkError::TeleportViolation { .. } => 6002,
            NetworkError::OutOfBounds(_) => 6003,
            NetworkError::InputRateExceeded { .. } => 6004,
            NetworkError::Kicked(_) => 6005,
            NetworkError::NotConnected => 7000,
            NetworkError::AlreadyConnected => 7001,
            NetworkError::ServerNotRunning => 7002,
            NetworkError::InvalidState { .. } => 7003,
        }
    }
}

/// Result type for network operations.
pub type NetworkResult<T> = Result<T, NetworkError>;

