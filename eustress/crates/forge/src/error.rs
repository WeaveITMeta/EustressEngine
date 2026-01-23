//! Error types for Eustress Forge.
//!
//! Extends `forge_orchestration::ForgeError` with game-server-specific errors.

use thiserror::Error;

/// Errors that can occur in the Eustress Forge game server orchestration.
#[derive(Error, Debug)]
pub enum EustressForgeError {
    /// Underlying forge-orchestration error
    #[error("Orchestration error: {0}")]
    Orchestration(#[from] forge_orchestration::ForgeError),
    
    /// Experience not found
    #[error("Experience not found: {0}")]
    ExperienceNotFound(String),
    
    /// Server allocation failed
    #[error("Failed to allocate server: {0}")]
    AllocationFailed(String),
    
    /// No healthy servers available
    #[error("No healthy servers available in region {0}")]
    NoHealthyServers(String),
    
    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(uuid::Uuid),
    
    /// Player routing failed
    #[error("Failed to route player {player_id} to server {server_id}: {reason}")]
    RoutingFailed {
        player_id: uuid::Uuid,
        server_id: String,
        reason: String,
    },
    
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),
    
    /// Scaling policy violation
    #[error("Scaling policy violation: {0}")]
    ScalingViolation(String),
    
    /// Health check failed
    #[error("Health check failed for {server_id}: {reason}")]
    HealthCheckFailed {
        server_id: String,
        reason: String,
    },
    
    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for Eustress Forge operations.
pub type Result<T> = std::result::Result<T, EustressForgeError>;

// Re-export base forge error for convenience
pub use forge_orchestration::ForgeError as BaseForgeError;