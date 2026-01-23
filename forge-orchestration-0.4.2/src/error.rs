//! Error types for Forge
//!
//! ## Table of Contents
//! - **ForgeError**: Main error enum covering all failure modes
//! - **Result**: Type alias for `Result<T, ForgeError>`

use thiserror::Error;

/// Result type alias for Forge operations
pub type Result<T> = std::result::Result<T, ForgeError>;

/// Main error type for Forge operations
#[derive(Error, Debug)]
pub enum ForgeError {
    /// Configuration error during builder setup
    #[error("configuration error: {0}")]
    Config(String),

    /// Nomad API communication failure
    #[error("nomad error: {0}")]
    Nomad(String),

    /// Storage backend failure (RocksDB or etcd)
    #[error("storage error: {0}")]
    Storage(String),

    /// Networking failure (QUIC, HTTP, gRPC)
    #[error("network error: {0}")]
    Network(String),

    /// Consensus/Raft failure
    #[error("consensus error: {0}")]
    Consensus(String),

    /// MoE routing failure
    #[error("routing error: {0}")]
    Routing(String),

    /// Job submission or management failure
    #[error("job error: {0}")]
    Job(String),

    /// Autoscaling decision failure
    #[error("autoscaler error: {0}")]
    Autoscaler(String),

    /// Metrics collection or export failure
    #[error("metrics error: {0}")]
    Metrics(String),

    /// Runtime not initialized or already stopped
    #[error("runtime error: {0}")]
    Runtime(String),

    /// Generic IO error
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization error
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Internal error (should not occur in normal operation)
    #[error("internal error: {0}")]
    Internal(String),
}

impl ForgeError {
    /// Create a configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create a Nomad error
    pub fn nomad(msg: impl Into<String>) -> Self {
        Self::Nomad(msg.into())
    }

    /// Create a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Create a network error
    pub fn network(msg: impl Into<String>) -> Self {
        Self::Network(msg.into())
    }

    /// Create a job error
    pub fn job(msg: impl Into<String>) -> Self {
        Self::Job(msg.into())
    }

    /// Create a runtime error
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    /// Create a metrics error
    pub fn metrics(msg: impl Into<String>) -> Self {
        Self::Metrics(msg.into())
    }
}

impl From<reqwest::Error> for ForgeError {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(err.to_string())
    }
}

impl From<prometheus::Error> for ForgeError {
    fn from(err: prometheus::Error) -> Self {
        Self::Metrics(err.to_string())
    }
}
