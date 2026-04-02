//! =============================================================================
//! Witness — HTTP client for the co-signing API
//! =============================================================================
//! Connects the local EustressEngine to the remote witness Worker for
//! contribution co-signing. The engine records contributions locally, then
//! sends the hash to the witness for an independent server co-signature.
//!
//! Flow:
//!   1. Engine records a contribution entry (user-signed, hash-chained)
//!   2. Engine calls `WitnessClient::cosign()` with the entry hash
//!   3. Witness Worker validates the user and signs the hash
//!   4. Engine stores the server co-signature on the contribution entry
//!
//! The witness never sees the user's private key or content — only the hash.
//! =============================================================================

use serde::{Deserialize, Serialize};

use crate::IdentityError;

/// Default witness endpoint for mainnet.
pub const DEFAULT_WITNESS_URL: &str = "https://witness.eustress.dev";

/// Client for the co-signing witness Worker.
#[derive(Debug, Clone)]
pub struct WitnessClient {
    /// Base URL of the witness API (e.g. "https://witness.eustress.dev")
    base_url: String,
    /// HTTP client
    client: reqwest::Client,
}

/// Request body for /api/cosign.
#[derive(Debug, Serialize)]
struct CosignRequest<'a> {
    user_id: &'a str,
    contribution_hash: &'a str,
    timestamp: &'a str,
}

/// Response from /api/cosign.
#[derive(Debug, Deserialize)]
pub struct CosignResponse {
    pub server_signature: String,
    pub co_signed_at: String,
}

/// Error response from the witness API.
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

/// Request body for /api/register.
#[derive(Debug, Serialize)]
struct RegisterRequest<'a> {
    user_id: &'a str,
    public_key: &'a str,
}

/// Response from /api/register.
#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub registered: bool,
    pub user_id: String,
}

/// Fork info from /.well-known/eustress-fork.
#[derive(Debug, Deserialize)]
pub struct ForkInfo {
    pub fork_id: String,
    pub public_key: String,
    pub chain_id: u32,
    pub bliss_version: String,
    pub identity_schema_version: String,
    pub contact: String,
}

impl WitnessClient {
    /// Create a new witness client pointing at the given URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Create a client pointing at the mainnet witness.
    pub fn mainnet() -> Self {
        Self::new(DEFAULT_WITNESS_URL)
    }

    /// Request a co-signature for a contribution hash.
    ///
    /// Returns the server's Ed25519 signature over the canonical payload
    /// `cosign|{user_id}|{contribution_hash}|{timestamp}`.
    pub async fn cosign(
        &self,
        user_id: &str,
        contribution_hash: &str,
        timestamp: &str,
    ) -> Result<CosignResponse, IdentityError> {
        let url = format!("{}/api/cosign", self.base_url);

        let resp = self
            .client
            .post(&url)
            .json(&CosignRequest {
                user_id,
                contribution_hash,
                timestamp,
            })
            .send()
            .await
            .map_err(|e| IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("witness unreachable: {e}"),
            )))?;

        if resp.status().is_success() {
            let cosign: CosignResponse = resp.json().await.map_err(|e| {
                IdentityError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid cosign response: {e}"),
                ))
            })?;
            Ok(cosign)
        } else if resp.status().as_u16() == 429 {
            Err(IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "rate limited by witness — try again later",
            )))
        } else {
            let err: ErrorResponse = resp.json().await.unwrap_or(ErrorResponse {
                error: "unknown error".to_string(),
            });
            match err.error.as_str() {
                "unknown_user" => Err(IdentityError::UnknownIssuer),
                "user_revoked" => Err(IdentityError::Revoked),
                _ => Err(IdentityError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("witness error: {}", err.error),
                ))),
            }
        }
    }

    /// Register a user identity with the witness.
    ///
    /// Called once after identity issuance so the witness knows this user
    /// exists and can co-sign their contributions.
    pub async fn register_user(
        &self,
        user_id: &str,
        public_key: &str,
    ) -> Result<RegisterResponse, IdentityError> {
        let url = format!("{}/api/register", self.base_url);

        let resp = self
            .client
            .post(&url)
            .json(&RegisterRequest {
                user_id,
                public_key,
            })
            .send()
            .await
            .map_err(|e| IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("witness unreachable: {e}"),
            )))?;

        if resp.status().is_success() {
            let reg: RegisterResponse = resp.json().await.map_err(|e| {
                IdentityError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid register response: {e}"),
                ))
            })?;
            Ok(reg)
        } else {
            let err: ErrorResponse = resp.json().await.unwrap_or(ErrorResponse {
                error: "unknown error".to_string(),
            });
            Err(IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("registration failed: {}", err.error),
            )))
        }
    }

    /// Fetch the witness server's fork info.
    pub async fn fork_info(&self) -> Result<ForkInfo, IdentityError> {
        let url = format!("{}/.well-known/eustress-fork", self.base_url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                format!("witness unreachable: {e}"),
            )))?;

        resp.json().await.map_err(|e| {
            IdentityError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid fork info: {e}"),
            ))
        })
    }

    /// Fetch the witness server's public key for verifying co-signatures.
    pub async fn server_public_key(&self) -> Result<String, IdentityError> {
        let info = self.fork_info().await?;
        Ok(info.public_key)
    }
}
