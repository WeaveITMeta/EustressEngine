//! # Forge SDK Error Types
//!
//! ## Table of Contents
//!
//! 1. **SdkError** - Top-level error enum for all SDK operations

use thiserror::Error;

/// Top-level error type for the Forge SDK.
#[derive(Error, Debug)]
pub enum SdkError {
    /// HTTP request failed
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization/deserialization failed
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Server returned an error response
    #[error("Server error ({status}): {message}")]
    Server {
        status: u16,
        message: String,
    },

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Connection failed
    #[error("Connection failed: {0}")]
    Connection(String),

    /// Timeout
    #[error("Operation timed out after {0}s")]
    Timeout(u64),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Deployment error
    #[error("Deployment error: {0}")]
    Deployment(String),

    /// Session error
    #[error("Session error: {0}")]
    Session(String),

    /// Generic error
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Result type alias for SDK operations.
pub type SdkResult<T> = Result<T, SdkError>;
