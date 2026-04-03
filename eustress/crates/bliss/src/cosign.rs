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
}

/// Response from the co-signing endpoint.
#[derive(Debug, Deserialize)]
struct CosignResponse {
    server_signature: String,
    co_signed_at: String,
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
}

impl CosignClient {
    /// Create a new co-sign client pointing at the given witness URL.
    pub fn new(witness_url: String, fork_id: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            witness_url,
            fork_id,
        }
    }

    /// Request a co-signature for a contribution hash.
    pub async fn cosign(
        &self,
        user_id: &str,
        contribution_hash: &str,
        contribution_type: &str,
        duration_secs: u64,
    ) -> Result<CosignResult, BlissError> {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let request = CosignRequest {
            user_id: user_id.to_string(),
            contribution_hash: contribution_hash.to_string(),
            contribution_type: contribution_type.to_string(),
            duration_secs,
            timestamp,
        };

        let url = format!("{}/api/cosign", self.witness_url);
        let response = self
            .http
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(BlissError::Network)?;

        let status = response.status();
        if status.is_success() {
            let body: CosignResponse = response
                .json()
                .await
                .map_err(BlissError::Network)?;
            Ok(CosignResult {
                server_signature: body.server_signature,
                co_signed_at: body.co_signed_at,
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
    pub async fn heartbeat(
        &self,
        node_id: &str,
        mode: &str,
        players: u32,
        uptime_secs: u64,
    ) -> Result<(), BlissError> {
        let url = format!("{}/api/node/heartbeat", self.witness_url);
        let body = serde_json::json!({
            "node_id": node_id,
            "mode": mode,
            "players": players,
            "uptime_secs": uptime_secs,
            "fork_id": self.fork_id,
        });

        let _ = self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(BlissError::Network)?;

        Ok(())
    }
}
