// =============================================================================
// Eustress Web - Auth API
// =============================================================================
// Table of Contents:
// 1. Request/Response Types
// 2. Auth API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use gloo_storage::Storage;
use super::{ApiClient, ApiError};
use crate::state::User;

// -----------------------------------------------------------------------------
// 1. Request/Response Types
// -----------------------------------------------------------------------------

/// Login request payload.
#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Register request payload.
#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// Auth response with token and user.
#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
}

/// Token refresh response.
#[derive(Debug, Deserialize)]
pub struct RefreshResponse {
    pub token: String,
}

/// Challenge request for Eustress Identity login.
#[derive(Debug, Serialize)]
pub struct ChallengeRequest {
    pub public_key: String,
}

/// Challenge response from server (nonce to sign).
#[derive(Debug, Deserialize)]
pub struct ChallengeResponse {
    pub challenge: String,
    pub expires_at: String,
}

/// Verify signed challenge for Eustress Identity login.
#[derive(Debug, Serialize)]
pub struct VerifyChallengeRequest {
    pub public_key: String,
    pub challenge: String,
    pub signature: String,
}

// -----------------------------------------------------------------------------
// 2. Auth API Functions
// -----------------------------------------------------------------------------

/// Login with email and password.
pub async fn login(client: &ApiClient, email: &str, password: &str) -> Result<AuthResponse, ApiError> {
    let request = LoginRequest {
        email: email.to_string(),
        password: password.to_string(),
    };
    
    let response: AuthResponse = client.post("/api/auth/login", &request).await?;
    
    // Store token in localStorage
    let _ = gloo_storage::LocalStorage::set("auth_token", &response.token);
    
    Ok(response)
}

/// Register a new user.
pub async fn register(
    client: &ApiClient,
    username: &str,
    email: &str,
    password: &str,
) -> Result<AuthResponse, ApiError> {
    let request = RegisterRequest {
        username: username.to_string(),
        email: email.to_string(),
        password: password.to_string(),
    };
    
    let response: AuthResponse = client.post("/api/auth/register", &request).await?;
    
    // Store token in localStorage
    let _ = gloo_storage::LocalStorage::set("auth_token", &response.token);
    
    Ok(response)
}

/// Get current user from token.
pub async fn get_current_user(client: &ApiClient) -> Result<User, ApiError> {
    client.get("/api/auth/me").await
}

/// Refresh auth token.
pub async fn refresh_token(client: &ApiClient) -> Result<RefreshResponse, ApiError> {
    client.post("/api/auth/refresh", &()).await
}

/// Logout (client-side only, clears token).
pub fn logout() {
    let _ = gloo_storage::LocalStorage::delete("auth_token");
}

/// Request a challenge nonce for Eustress Identity login.
pub async fn request_challenge(
    client: &ApiClient,
    public_key: &str,
) -> Result<ChallengeResponse, ApiError> {
    let request = ChallengeRequest {
        public_key: public_key.to_string(),
    };
    client.post("/api/auth/challenge", &request).await
}

/// Verify a signed challenge to complete Eustress Identity login.
pub async fn verify_challenge(
    client: &ApiClient,
    public_key: &str,
    challenge: &str,
    signature: &str,
) -> Result<AuthResponse, ApiError> {
    let request = VerifyChallengeRequest {
        public_key: public_key.to_string(),
        challenge: challenge.to_string(),
        signature: signature.to_string(),
    };

    let response: AuthResponse = client.post("/api/auth/verify-challenge", &request).await?;
    let _ = gloo_storage::LocalStorage::set("auth_token", &response.token);
    Ok(response)
}

/// Register a new Eustress Identity with the node.
#[derive(Debug, Serialize)]
pub struct IdentityRegisterRequest {
    pub username: String,
    pub public_key: String,
    pub birthday: Option<String>,
    pub id_type: Option<String>,
    pub id_hash: Option<String>,
    pub kyc_session_id: Option<String>,
}

pub async fn register_identity(
    client: &ApiClient,
    username: &str,
    public_key: &str,
    birthday: Option<&str>,
    id_type: Option<&str>,
    id_hash: Option<&str>,
    kyc_session_id: Option<&str>,
) -> Result<AuthResponse, ApiError> {
    let request = IdentityRegisterRequest {
        username: username.to_string(),
        public_key: public_key.to_string(),
        birthday: birthday.map(|s| s.to_string()),
        id_type: id_type.map(|s| s.to_string()),
        id_hash: id_hash.map(|s| s.to_string()),
        kyc_session_id: kyc_session_id.map(|s| s.to_string()),
    };

    let response: AuthResponse = client.post("/api/auth/register", &request).await?;
    let _ = gloo_storage::LocalStorage::set("auth_token", &response.token);
    Ok(response)
}

/// Add email and password to existing account.
#[derive(Debug, serde::Serialize)]
pub struct AddEmailRequest {
    pub email: String,
    pub password: String,
}

pub async fn add_email_password(client: &ApiClient, email: &str, password: &str) -> Result<User, ApiError> {
    let request = AddEmailRequest {
        email: email.to_string(),
        password: password.to_string(),
    };
    
    client.post("/api/auth/add-email", &request).await
}

/// Get user by token (for OAuth callback).
pub async fn get_me(client: &ApiClient, token: &str) -> Result<User, ApiError> {
    use gloo_net::http::Request;
    
    let url = format!("{}/api/auth/me", client.base_url());
    let response = Request::get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| ApiError::Network(e.to_string()))?;
    
    let status = response.status();
    match status {
        200..=299 => {
            response
                .json::<User>()
                .await
                .map_err(|e| ApiError::Deserialize(e.to_string()))
        }
        401 => Err(ApiError::Unauthorized),
        404 => Err(ApiError::NotFound),
        _ => {
            let message = response.text().await.unwrap_or_default();
            Err(ApiError::Server { status, message })
        }
    }
}
