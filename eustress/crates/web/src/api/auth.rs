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
///
/// `backup_emailed` is populated only by `/api/auth/register` when the
/// caller supplied `email` + `toml_content` and the Worker successfully
/// relayed the welcome email. Other endpoints (login, verify-challenge)
/// don't set it; serde defaults to `None` so existing call sites stay
/// compatible.
#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: User,
    #[serde(default)]
    pub backup_emailed: Option<bool>,
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
    /// Always [`LOGIN_FORMAT`]: the signature covers the login message, not the
    /// bare challenge.
    pub format: String,
}

/// What an identity signature covers (docs/architecture/STUDIO_IDENTITY_SIGNIN.md):
/// three lines joined by newlines, no trailing newline. `origin` is the API this
/// sign-in talks to, so a signature made for one API is no sign-in on another.
pub const LOGIN_FORMAT: &str = "eustress-login-v1";

pub fn login_message(origin: &str, challenge: &str) -> String {
    format!("{LOGIN_FORMAT}\n{origin}\n{challenge}")
}

/// The origin of an API base address: scheme and host in lower case, the default
/// port dropped, no path. The same rule Studio applies (`api_origin` in
/// common/src/api_base.rs); the shared vectors in
/// docs/architecture/login-v1-vectors.json hold both to it.
pub fn api_origin(base: &str) -> String {
    let base = base.trim();
    let Some((scheme, rest)) = base.split_once("://") else {
        return base.to_ascii_lowercase();
    };
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, p.parse::<u32>().ok()),
        _ => (authority, None),
    };
    let default = match scheme.as_str() {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    };
    let host = host.to_ascii_lowercase();
    match port {
        Some(p) if Some(p) != default => format!("{scheme}://{host}:{p}"),
        _ => format!("{scheme}://{host}"),
    }
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

/// Ends every sign-in of the account (`POST /api/auth/logout` moves the session floor past all of them), this one included.
/// The caller clears the stored token itself once this says it worked.
pub async fn logout_everywhere(client: &ApiClient) -> Result<(), ApiError> {
    client.post::<serde_json::Value, _>("/api/auth/logout", &serde_json::json!({})).await.map(|_| ())
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
        format: LOGIN_FORMAT.to_string(),
    };

    let response: AuthResponse = client.post("/api/auth/verify-challenge", &request).await?;
    let _ = gloo_storage::LocalStorage::set("auth_token", &response.token);
    Ok(response)
}

/// Register a new Eustress Identity with the node.
///
/// `email` + `toml_content` are optional. When BOTH are supplied, the
/// Cloudflare Worker auto-relays the identity-registration welcome
/// email (with the TOML as a `.toml` attachment) before returning,
/// and stamps `backup_emailed: true` on the response. Callers
/// supplying only one of the two get treated as if neither were
/// present — the Worker won't half-send.
///
/// Skipping `serialize_if = None` so the JSON payload stays compact
/// and the Worker's `&& email && toml_content` check is simple.
#[derive(Serialize)]
pub struct IdentityRegisterRequest {
    pub username: String,
    pub public_key: String,
    pub birthday: Option<String>,
    pub id_type: Option<String>,
    pub id_hash: Option<String>,
    pub kyc_session_id: Option<String>,
    /// The claim secret of the KYC session (from /api/kyc/start): the proof that
    /// this sign-up started the verification. Never printed (see Debug below).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kyc_claim_secret: Option<String>,
    /// Email address to send the identity backup to. None = no email.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Full identity.toml body (with private key) — built CLIENT-SIDE
    /// only; never round-trips back to the Worker after this single
    /// signup call. None = no email.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toml_content: Option<String>,
}

/// Debug without the claim secret, the private key or the email address.
impl std::fmt::Debug for IdentityRegisterRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityRegisterRequest")
            .field("username", &self.username)
            .field("public_key", &self.public_key)
            .field("kyc_session_id", &self.kyc_session_id)
            .field("kyc_claim_secret", &self.kyc_claim_secret.as_ref().map(|_| "[redacted]"))
            .finish_non_exhaustive()
    }
}

pub async fn register_identity(
    client: &ApiClient,
    username: &str,
    public_key: &str,
    birthday: Option<&str>,
    id_type: Option<&str>,
    id_hash: Option<&str>,
    kyc_session_id: Option<&str>,
    kyc_claim_secret: Option<&str>,
    email: Option<&str>,
    toml_content: Option<&str>,
) -> Result<AuthResponse, ApiError> {
    let request = IdentityRegisterRequest {
        username: username.to_string(),
        public_key: public_key.to_string(),
        birthday: birthday.map(|s| s.to_string()),
        id_type: id_type.map(|s| s.to_string()),
        id_hash: id_hash.map(|s| s.to_string()),
        kyc_session_id: kyc_session_id.map(|s| s.to_string()),
        kyc_claim_secret: kyc_claim_secret.map(|s| s.to_string()),
        email: email.map(|s| s.to_string()),
        toml_content: toml_content.map(|s| s.to_string()),
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

#[cfg(test)]
mod login_tests {
    use super::*;

    /// The one file Studio's tests and the Worker's tests read too.
    const VECTORS: &str = include_str!("../../../../../docs/architecture/login-v1-vectors.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS).expect("the shared vectors are JSON")
    }

    #[test]
    fn the_login_message_is_the_shared_one() {
        for s in vectors()["signatures"].as_array().unwrap() {
            let message = login_message(s["origin"].as_str().unwrap(), s["challenge"].as_str().unwrap());
            assert_eq!(message, s["message"].as_str().unwrap());
        }
    }

    #[test]
    fn origins_are_normalised_as_in_the_shared_vectors() {
        for o in vectors()["origins"].as_array().unwrap() {
            let base = o["base"].as_str().unwrap();
            assert_eq!(api_origin(base), o["origin"].as_str().unwrap(), "{base}");
        }
    }

    #[test]
    fn the_site_apis_are_their_own_origins() {
        for api in ["https://api.eustress.dev", "https://api-staging.eustress.dev", "http://127.0.0.1:8787"] {
            assert_eq!(api_origin(api), api);
        }
    }
}
