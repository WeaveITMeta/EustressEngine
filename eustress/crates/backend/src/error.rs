// =============================================================================
// Eustress Backend - Error Types
// =============================================================================

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Application error type.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Invalid token")]
    InvalidToken,

    #[error("Token expired")]
    TokenExpired,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Resource not found")]
    NotFound,

    #[error("Internal server error")]
    Internal,
}

/// API error type for handler endpoints.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Database error: {0}")]
    Database(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
            ApiError::Database(msg) => {
                tracing::error!("Database error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error".into())
            }
            ApiError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error".into())
            }
        };
        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token".into()),
            AppError::TokenExpired => (StatusCode::UNAUTHORIZED, "Token expired".into()),
            AppError::Database(e) => {
                tracing::error!("Database error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Database error".to_string())
            }
            AppError::NotFound => (StatusCode::NOT_FOUND, "Resource not found".into()),
            AppError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into()),
        };

        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}
