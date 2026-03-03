//! MCP Server error types.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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

impl IntoResponse for McpError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            McpError::Config(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            McpError::Auth(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            McpError::InvalidApiKey => (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()),
            McpError::EntityNotFound(id) => (StatusCode::NOT_FOUND, format!("Entity not found: {}", id)),
            McpError::SpaceNotFound(id) => (StatusCode::NOT_FOUND, format!("Space not found: {}", id)),
            McpError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            McpError::PermissionDenied(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            McpError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".to_string()),
            McpError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            McpError::Serialization(e) => (StatusCode::BAD_REQUEST, e.to_string()),
            McpError::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            McpError::Protocol(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            McpError::WebSocket(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            McpError::RuneExecution(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body = serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        });

        (status, Json(body)).into_response()
    }
}
