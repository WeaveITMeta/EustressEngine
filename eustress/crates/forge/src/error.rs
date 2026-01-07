//! Error types for Eustress Forge.

use thiserror::Error;

/// Errors that can occur in the Forge orchestration system.
#[derive(Error, Debug)]
pub enum ForgeError {
    /// Failed to connect to Nomad cluster
    #[error("Failed to connect to Nomad: {0}")]
    NomadConnection(String),
    
    /// Failed to connect to Consul
    #[error("Failed to connect to Consul: {0}")]
    ConsulConnection(String),
    
    /// Job submission failed
    #[error("Failed to submit job '{job_id}': {reason}")]
    JobSubmission {
        job_id: String,
        reason: String,
    },
    
    /// Job not found
    #[error("Job not found: {0}")]
    JobNotFound(String),
    
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
    
    /// Internal error
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
    
    /// HTTP request error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    
    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for Forge operations.
pub type ForgeResult<T> = Result<T, ForgeError>;