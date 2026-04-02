//! Bliss integration error types.

use thiserror::Error;

/// All error types for the Bliss integration layer.
#[derive(Debug, Error)]
pub enum BlissError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Wallet error: {0}")]
    Wallet(String),

    #[error("Co-signing failed: {0}")]
    Cosign(String),

    #[error("Node error: {0}")]
    Node(String),

    #[error("Identity not loaded — load identity.toml first")]
    NoIdentity,

    #[error("Contribution error: {0}")]
    Contribution(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
