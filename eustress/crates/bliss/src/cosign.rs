//! Co-signing client — requests witness co-signatures from the Cloudflare Worker.
//!
//! Every contribution must be co-signed by an independent witness (the Worker)
//! to prevent self-signing fraud. The user's engine signs the contribution hash
//! locally, then sends it to the witness for a second signature.

use serde::{Deserialize, Serialize};

use crate::error::BlissError;
use crate::node::CosignResult;

/// Request payload for the co-signing endpoint.
#[derive(Debug, Serialize)]
struct CosignRequest {
    user_id: String,
    contribution_hash: String,
    contribution_type: String,
    duration_secs: u64,
    timestamp: String,
    /// "Light" or "Full" — the witness applies the +10% Full-node bonus
    /// server-side so the client can't self-award it silently.
    node_mode: String,
}

/// Response from the co-signing endpoint.
#[derive(Debug, Deserialize)]
struct CosignResponse {
    server_signature: String,
    co_signed_at: String,
    /// Weighted score the witness credited for this contribution
    /// (weight × minutes × node bonus). Absent on older witnesses.
    #[serde(default)]
    score_added: f64,
    /// Lifetime contribution score after this credit.
    #[serde(default)]
    total_score: f64,
}

/// Error response from the co-signing endpoint.
#[derive(Debug, Deserialize)]
struct CosignErrorResponse {
    error: String,
}

/// HTTP client for witness Worker co-signing requests.
pub struct CosignClient {
    http: reqwest::Client,
    witness_url: String,
    fork_id: String,
    /// Bearer JWT from the Eustress login flow. The witness rejects
    /// unauthenticated co-sign requests (401), so this must be set
    /// before `cosign` is called with a logged-in user.
    auth_token: Option<String>,
}

impl CosignClient {
    /// Create a new co-sign client pointing at the given witness URL.
    pub fn new(witness_url: String, fork_id: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            witness_url,
            fork_id,
            auth_token: None,
        }
    }

    /// Set (or clear) the bearer token used to authenticate co-sign
    /// requests. The token is the JWT issued by the witness's own
    /// login flow (`/api/auth/verify-challenge`).
    pub fn set_auth_token(&mut self, token: Option<String>) {
        self.auth_token = token;
    }

    /// Request a co-signature for a contribution hash.
    ///
    /// `node_mode` is "Light" or "Full" — the witness applies the
    /// +10% Full-node bonus server-side.
    pub async fn cosign(
        &self,
        user_id: &str,
        contribution_hash: &str,
        contribution_type: &str,
        duration_secs: u64,
        node_mode: &str,
    ) -> Result<CosignResult, BlissError> {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let request = CosignRequest {
            user_id: user_id.to_string(),
            contribution_hash: contribution_hash.to_string(),
            contribution_type: contribution_type.to_string(),
            duration_secs,
            timestamp,
            node_mode: node_mode.to_string(),
        };

        let url = format!("{}/api/cosign", self.witness_url);
        let mut req = self.http.post(&url).json(&request);
        if let Some(token) = self.auth_token.as_ref() {
            req = req.bearer_auth(token);
        }
        let response = req.send().await.map_err(BlissError::Network)?;

        let status = response.status();
        if status.is_success() {
            let body: CosignResponse = response
                .json()
                .await
                .map_err(BlissError::Network)?;
            Ok(CosignResult {
                server_signature: body.server_signature,
                co_signed_at: body.co_signed_at,
                score_added: body.score_added,
                total_score: body.total_score,
            })
        } else {
            let error_text = response
                .json::<CosignErrorResponse>()
                .await
                .map(|e| e.error)
                .unwrap_or_else(|_| format!("HTTP {}", status));
            Err(BlissError::Cosign(error_text))
        }
    }

    /// Get the witness URL this client is configured for.
    pub fn witness_url(&self) -> &str {
        &self.witness_url
    }

    /// Get the fork ID this client operates under.
    pub fn fork_id(&self) -> &str {
        &self.fork_id
    }

    /// Send a heartbeat to the witness to report node status.
    ///
    /// When `user_id` is provided the witness replies with the user's
    /// authoritative BLS balance and today's pending contribution score
    /// — this is how the engine badge stays in sync with the ledger.
    pub async fn heartbeat(
        &self,
        node_id: &str,
        mode: &str,
        players: u32,
        uptime_secs: u64,
        user_id: Option<&str>,
    ) -> Result<HeartbeatReply, BlissError> {
        let url = format!("{}/api/node/heartbeat", self.witness_url);
        let mut body = serde_json::json!({
            "node_id": node_id,
            "mode": mode,
            "players": players,
            "uptime_secs": uptime_secs,
            "fork_id": self.fork_id,
        });
        if let Some(uid) = user_id {
            body["user_id"] = serde_json::Value::String(uid.to_string());
        }

        let response = self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(BlissError::Network)?;

        if response.status().is_success() {
            let reply: HeartbeatReply = response
                .json()
                .await
                .map_err(BlissError::Network)?;
            Ok(reply)
        } else {
            Err(BlissError::Cosign(format!(
                "heartbeat HTTP {}",
                response.status()
            )))
        }
    }
}

/// Witness reply to a node heartbeat. Balance fields are zero unless
/// a `user_id` was included in the request.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HeartbeatReply {
    #[serde(default)]
    pub ok: bool,
    /// Authoritative BLS balance from the ledger (whole BLS).
    #[serde(default)]
    pub bliss_balance: f64,
    /// Today's pending (not yet distributed) contribution score.
    #[serde(default)]
    pub pending_score: f64,
}
