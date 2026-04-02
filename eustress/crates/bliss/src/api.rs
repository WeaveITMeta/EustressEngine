//! Node API server — co-signing and contribution tracking.
//!
//! The node does NOT handle registration or KYC — that's Cloudflare's job
//! (api.eustress.dev). The node handles:
//!
//!   POST /api/cosign              — co-sign a contribution hash
//!   POST /api/identity/verify     — verify an identity.toml signature locally
//!   GET  /health                  — node status
//!
//! Users register at eustress.dev → get identity.toml → sign in to any node
//! by proving they hold the private key (Ed25519 challenge-response, local).

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::error::BlissError;
use crate::node::NodeMode;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub node_mode: NodeMode,
    pub fork_id: String,
    pub witness_url: String,
    /// Known identities — public keys we've verified before (in-memory cache)
    pub known_identities: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, KnownIdentity>>>,
    /// Pending challenges for local identity verification
    pub challenges: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, PendingChallenge>>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct KnownIdentity {
    pub public_key: String,
    pub username: String,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Clone, Debug)]
struct PendingChallenge {
    challenge: String,
    expires_at: chrono::DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
    pub public_key: String,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    pub challenge: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyIdentityRequest {
    pub public_key: String,
    pub username: String,
    pub challenge: String,
    pub signature: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyIdentityResponse {
    pub verified: bool,
    pub username: String,
    pub node_mode: String,
}

#[derive(Debug, Serialize)]
pub struct CosignResponse {
    pub server_signature: String,
    pub co_signed_at: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub node_mode: String,
    pub fork_id: String,
    pub known_identities: usize,
    pub uptime: String,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error: msg.to_string() }))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/api/identity/challenge", post(challenge))
        .route("/api/identity/verify", post(verify_identity))
        .route("/api/cosign", post(cosign))
        .layer(cors)
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(state): State<ApiState>) -> Json<HealthResponse> {
    let count = state.known_identities.read().await.len();
    Json(HealthResponse {
        status: "ok".to_string(),
        node_mode: format!("{}", state.node_mode),
        fork_id: state.fork_id.clone(),
        known_identities: count,
        uptime: "running".to_string(),
    })
}

/// POST /api/identity/challenge — issue a nonce for local identity verification.
async fn challenge(
    State(state): State<ApiState>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, (StatusCode, Json<ApiError>)> {
    let challenge_bytes: [u8; 32] = rand::random();
    let challenge = hex::encode(challenge_bytes);
    let expires_at = Utc::now() + chrono::Duration::minutes(5);

    state.challenges.write().await.insert(
        req.public_key.clone(),
        PendingChallenge {
            challenge: challenge.clone(),
            expires_at,
        },
    );

    Ok(Json(ChallengeResponse {
        challenge,
        expires_at: expires_at.to_rfc3339(),
    }))
}

/// POST /api/identity/verify — verify Ed25519 signature from identity.toml.
/// This is LOCAL verification — no Cloudflare call needed.
/// The user proves they hold the private key matching the public key in their identity.toml.
async fn verify_identity(
    State(state): State<ApiState>,
    Json(req): Json<VerifyIdentityRequest>,
) -> Result<Json<VerifyIdentityResponse>, (StatusCode, Json<ApiError>)> {
    // Get and consume challenge
    let pending = state
        .challenges
        .write()
        .await
        .remove(&req.public_key)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "No pending challenge for this key"))?;

    if Utc::now() > pending.expires_at {
        return Err(err(StatusCode::BAD_REQUEST, "Challenge expired"));
    }

    if pending.challenge != req.challenge {
        return Err(err(StatusCode::BAD_REQUEST, "Challenge mismatch"));
    }

    // Verify Ed25519 signature locally
    let pub_key = bliss_crypto::PublicKey::from_hex(&req.public_key)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "Invalid public key"))?;

    let sig_bytes = hex::decode(&req.signature)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "Invalid signature encoding"))?;

    let signature = bliss_crypto::Signature::from_bytes(&sig_bytes)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "Invalid signature format"))?;

    signature
        .verify(req.challenge.as_bytes(), &pub_key)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Signature verification failed"))?;

    // Cache this identity as known
    let now = Utc::now().to_rfc3339();
    let mut known = state.known_identities.write().await;
    let entry = known.entry(req.public_key.clone()).or_insert(KnownIdentity {
        public_key: req.public_key.clone(),
        username: req.username.clone(),
        first_seen: now.clone(),
        last_seen: now.clone(),
    });
    entry.last_seen = now;
    entry.username = req.username.clone();

    Ok(Json(VerifyIdentityResponse {
        verified: true,
        username: req.username,
        node_mode: format!("{}", state.node_mode),
    }))
}

/// POST /api/cosign — co-sign a contribution hash.
/// Requires the user to have verified their identity first.
async fn cosign(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<CosignRequest>,
) -> Result<Json<CosignResponse>, (StatusCode, Json<ApiError>)> {
    // Check the public key is known (user has verified identity on this node)
    let known = state.known_identities.read().await;
    if !known.contains_key(&req.public_key) {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Verify your identity first via /api/identity/verify",
        ));
    }

    // TODO: rate limit, validate contribution hash, sign with server key
    Ok(Json(CosignResponse {
        server_signature: format!("cosign_{}_{}", state.fork_id, &req.contribution_hash[..8.min(req.contribution_hash.len())]),
        co_signed_at: Utc::now().to_rfc3339(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct CosignRequest {
    pub public_key: String,
    pub contribution_hash: String,
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Server startup
// ---------------------------------------------------------------------------

/// Start the node API server on the given port.
pub async fn start_server(
    port: u16,
    node_mode: NodeMode,
    fork_id: String,
    witness_url: String,
) -> Result<tokio::task::JoinHandle<()>, BlissError> {
    let state = ApiState {
        node_mode,
        fork_id: fork_id.clone(),
        witness_url,
        known_identities: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        challenges: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    };

    let app = router(state);
    let addr = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| BlissError::Node(format!("Failed to bind port {}: {}", port, e)))?;

    tracing::info!("Bliss node API running on http://127.0.0.1:{}", port);
    tracing::info!("  Fork: {}", fork_id);
    tracing::info!("  Mode: {}", node_mode);
    tracing::info!("  Registration: https://api.eustress.dev (Cloudflare)");
    tracing::info!("  This node: co-signing + identity verification");

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Bliss node API error: {}", e);
        }
    });

    Ok(handle)
}
