//! MCP Server error types.

use thiserror::Error;

/// MCP Server errors
#[derive(Error, Debug)]
pub enum McpError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("Entity not found: {0}")]
    EntityNotFound(String),

    #[error("Space not found: {0}")]
    SpaceNotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Rune execution error: {0}")]
    RuneExecution(String),
}

/// Result type for MCP operations
pub type McpResult<T> = Result<T, McpError>;
