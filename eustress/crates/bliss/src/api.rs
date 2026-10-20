//! Node API server: local identity checks and node health.
//!
//! The node does NOT handle registration, KYC or co-signing. Registration and
//! KYC are api.eustress.dev's job, and so is co-signing: the witness there is
//! the only service whose signature credits a contribution (see `cosign.rs`
//! for the client the engine uses). The node handles:
//!
//!   POST /api/identity/challenge  issue a nonce for a public key
//!   POST /api/identity/verify     verify an identity.toml signature locally
//!   GET  /health                  node status
//!   POST /api/cosign              answers 501: co-signing is the witness's
//!
//! The listener binds 127.0.0.1 only and sends no CORS headers: it serves
//! programs on this machine, never other hosts on the network and never web
//! pages, whose JSON requests a browser refuses to send without CORS.

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::BlissError;
use crate::node::NodeMode;

/// Pending challenges kept at once. Expired ones are dropped on every new
/// challenge; past this many live ones, new challenges are refused.
const MAX_PENDING_CHALLENGES: usize = 1024;

/// Verified identities remembered at once. Past this, the one seen longest
/// ago is forgotten (it can verify again).
const MAX_KNOWN_IDENTITIES: usize = 4096;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub node_mode: NodeMode,
    pub fork_id: String,
    pub witness_url: String,
    /// Known identities: public keys verified on this node (in-memory cache)
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

/// An Ed25519 public key as hex: exactly 64 hex digits.
fn is_public_key_hex(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/identity/challenge", post(challenge))
        .route("/api/identity/verify", post(verify_identity))
        .route("/api/cosign", post(cosign))
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

/// POST /api/identity/challenge: issue a nonce for local identity verification.
async fn challenge(
    State(state): State<ApiState>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, (StatusCode, Json<ApiError>)> {
    if !is_public_key_hex(&req.public_key) {
        return Err(err(StatusCode::BAD_REQUEST, "public_key must be 64 hex digits"));
    }
    let challenge_bytes: [u8; 32] = rand::random();
    let challenge = hex::encode(challenge_bytes);
    let now = Utc::now();
    let expires_at = now + chrono::Duration::minutes(5);

    let mut pending = state.challenges.write().await;
    pending.retain(|_, c| c.expires_at > now);
    if pending.len() >= MAX_PENDING_CHALLENGES && !pending.contains_key(&req.public_key) {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "Too many pending challenges; try again shortly"));
    }
    pending.insert(
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

/// POST /api/identity/verify: verify an Ed25519 signature from identity.toml.
/// This is LOCAL verification; no Cloudflare call is made. The user proves
/// they hold the private key matching the public key in their identity.toml.
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

    // Cache this identity as known, forgetting the longest-unseen one when full.
    let now = Utc::now().to_rfc3339();
    let mut known = state.known_identities.write().await;
    if known.len() >= MAX_KNOWN_IDENTITIES && !known.contains_key(&req.public_key) {
        let oldest = known
            .iter()
            .min_by(|a, b| a.1.last_seen.cmp(&b.1.last_seen))
            .map(|(k, _)| k.clone());
        if let Some(oldest) = oldest {
            known.remove(&oldest);
        }
    }
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

/// POST /api/cosign: not served by a node. Only the witness at
/// api.eustress.dev co-signs contributions, and a node answering with a
/// string that looks like a signature would be a signature nobody issued.
async fn cosign(State(state): State<ApiState>) -> (StatusCode, Json<ApiError>) {
    err(
        StatusCode::NOT_IMPLEMENTED,
        &format!("This node does not co-sign; contributions are co-signed by the witness at {}/api/cosign", state.witness_url),
    )
}

// ---------------------------------------------------------------------------
// Server startup
// ---------------------------------------------------------------------------

fn new_state(node_mode: NodeMode, fork_id: String, witness_url: String) -> ApiState {
    ApiState {
        node_mode,
        fork_id,
        witness_url,
        known_identities: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        challenges: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    }
}

/// The address Bliss listens on when nobody configures one.
pub const DEFAULT_BIND: &str = "127.0.0.1";

/// `cli` (`--bliss-bind`), else `env` (`EUSTRESS_BLISS_BIND`), else
/// [`DEFAULT_BIND`]. The string must be an IP address.
pub fn resolve_bind(cli: Option<&str>, env: Option<&str>) -> Result<std::net::IpAddr, String> {
    let raw = cli
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| env.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or(DEFAULT_BIND);
    raw.parse::<std::net::IpAddr>()
        .map_err(|_| format!("Bliss bind address '{raw}' is not an IP address"))
}

/// The line both the tracing log and Studio's stdout print. It is the
/// listener's `local_addr`, not a constant, so it cannot disagree with the socket.
pub fn listener_log_line(addr: std::net::SocketAddr) -> String {
    format!("http://{addr}")
}

/// Bind `addr:port`. The default address is loopback; a configured address
/// is used as given.
async fn bind_on(addr: std::net::IpAddr, port: u16) -> Result<tokio::net::TcpListener, BlissError> {
    tokio::net::TcpListener::bind((addr, port))
        .await
        .map_err(|e| BlissError::Node(format!("Failed to bind {addr}:{port}: {e}")))
}

/// Start the node API server on `bind:port`.
///
/// The `Ok` address is `listener.local_addr()` — the same address the log
/// prints. Port 0 comes back as the port the OS assigned.
pub async fn start_server(
    bind: std::net::IpAddr,
    port: u16,
    node_mode: NodeMode,
    fork_id: String,
    witness_url: String,
) -> Result<(tokio::task::JoinHandle<()>, std::net::SocketAddr), BlissError> {
    let app = router(new_state(node_mode, fork_id.clone(), witness_url));
    let listener = bind_on(bind, port).await?;
    let bound = listener
        .local_addr()
        .map_err(|e| BlissError::Node(format!("listener has no local address: {e}")))?;

    tracing::info!("Bliss node API running on {}", listener_log_line(bound));
    tracing::info!("  Fork: {}", fork_id);
    tracing::info!("  Mode: {}", node_mode);
    tracing::info!("  Registration and co-signing: https://api.eustress.dev (Cloudflare)");
    tracing::info!("  This node: identity verification + health");

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Bliss node API error: {}", e);
        }
    });

    Ok((handle, bound))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ApiState {
        new_state(NodeMode::Light, "eustress.dev".to_string(), "https://api.eustress.dev".to_string())
    }

    #[test]
    fn the_default_bind_is_loopback_and_a_bad_address_is_refused() {
        assert_eq!(resolve_bind(None, None).unwrap(), std::net::Ipv4Addr::LOCALHOST);
        assert_eq!(
            resolve_bind(Some("10.0.0.8"), Some("0.0.0.0")).unwrap(),
            "10.0.0.8".parse::<std::net::IpAddr>().unwrap(),
            "the command-line address wins"
        );
        assert_eq!(
            resolve_bind(None, Some("0.0.0.0")).unwrap(),
            "0.0.0.0".parse::<std::net::IpAddr>().unwrap()
        );
        assert!(resolve_bind(Some("every-interface"), None).is_err());
    }

    #[tokio::test]
    async fn listener_binds_the_address_it_logs() {
        let ip = resolve_bind(None, None).unwrap();
        let listener = bind_on(ip, 0).await.expect("bind an ephemeral loopback port");
        let addr = listener.local_addr().expect("local address");
        assert!(addr.ip().is_loopback(), "bound {addr}, which other hosts could reach");
        assert_eq!(addr.ip(), ip);
        let line = listener_log_line(addr);
        assert_eq!(line, format!("http://{addr}"));
        assert!(!line.contains("0.0.0.0"), "the log must not claim a different interface");
    }

    #[tokio::test]
    async fn cosign_is_refused_and_names_the_witness() {
        let (status, Json(body)) = cosign(State(state())).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert!(body.error.contains("https://api.eustress.dev/api/cosign"), "{}", body.error);
    }

    #[tokio::test]
    async fn challenge_refuses_a_malformed_key() {
        let res = challenge(State(state()), Json(ChallengeRequest { public_key: "not-a-key".to_string() })).await;
        assert_eq!(res.err().map(|e| e.0), Some(StatusCode::BAD_REQUEST));
    }

    #[tokio::test]
    async fn pending_challenges_are_capped() {
        let s = state();
        for i in 0..MAX_PENDING_CHALLENGES {
            let key = format!("{i:064x}");
            assert!(challenge(State(s.clone()), Json(ChallengeRequest { public_key: key })).await.is_ok());
        }
        let over = challenge(State(s.clone()), Json(ChallengeRequest { public_key: "f".repeat(64) })).await;
        assert_eq!(over.err().map(|e| e.0), Some(StatusCode::SERVICE_UNAVAILABLE));
        // A key that already holds a challenge can still renew it.
        let renew = challenge(State(s), Json(ChallengeRequest { public_key: format!("{:064x}", 0) })).await;
        assert!(renew.is_ok());
    }
}
