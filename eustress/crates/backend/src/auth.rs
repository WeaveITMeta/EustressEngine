// =============================================================================
// Eustress Backend - Authentication (Cloudflare JWT Validation)
// =============================================================================
// Auth is handled by the Cloudflare Worker at api.eustress.dev.
// This module validates JWTs issued by that Worker using the shared secret.
// No local user management — users are stored in Cloudflare KV.
// =============================================================================

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------------
// JWT Claims (must match Cloudflare Worker's format)
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // User ID (UUID)
    pub exp: i64,     // Expiry timestamp
    pub iat: i64,     // Issued at
}

// -----------------------------------------------------------------------------
// Auth Extractor — validates JWT from Cloudflare Worker
// -----------------------------------------------------------------------------

/// Authenticated user extracted from JWT token.
/// The JWT was issued by the Cloudflare Worker at api.eustress.dev.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid authorization format"))?;

        // Validate against the shared JWT secret (same as Cloudflare Worker)
        let secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "default-dev-secret".to_string());

        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;

        let claims = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(AuthUser {
            user_id: claims.claims.sub,
        })
    }
}

/// Extract token string from Authorization header.
pub fn extract_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// Validate a token and return the user ID.
pub fn validate_token(token: &str, secret: &str) -> Result<String, crate::error::AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims.sub)
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => crate::error::AppError::TokenExpired,
        _ => crate::error::AppError::InvalidToken,
    })
}
