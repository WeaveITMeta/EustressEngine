//! =============================================================================
//! Issuer — Server-side identity issuance (client-side key generation)
//! =============================================================================
//! When a user creates an account:
//!   1. The CLIENT generates an Ed25519 keypair locally
//!   2. The client sends ONLY the public key to the server
//!   3. The server signs the identity block with its own key
//!   4. The server returns the signed TOML to the client
//!   5. The private key NEVER leaves the client machine
//!
//! This eliminates the server-side key generation exposure window entirely.
//! A malicious fork cannot exfiltrate keys because the server never has them.
//! =============================================================================

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Duration;
use ed25519_dalek::{Signer, SigningKey};
use uuid::Uuid;

use crate::{
    schema::{IdentityFile, IdentityMeta, UserProfile},
    IdentityError, SCHEMA_VERSION,
};

/// Default identity validity period: 1 year.
const DEFAULT_VALIDITY_DAYS: i64 = 365;

/// Server-side identity issuer.
/// Each server has its own Ed25519 signing key.
pub struct IdentityIssuer {
    /// The server's own signing key — signs issued identity files.
    server_signing_key: SigningKey,
    /// The server's domain (e.g. "eustress.dev")
    pub server_domain: String,
    /// Identity validity period in days
    pub validity_days: i64,
}

impl IdentityIssuer {
    pub fn new(server_signing_key: SigningKey, server_domain: String) -> Self {
        Self {
            server_signing_key,
            server_domain,
            validity_days: DEFAULT_VALIDITY_DAYS,
        }
    }

    /// Server public key in base64 — publish this so other servers
    /// and forks can verify credentials you issued.
    pub fn server_public_key_b64(&self) -> String {
        STANDARD.encode(self.server_signing_key.verifying_key().as_bytes())
    }

    /// Issue a new identity file for a user who generated their own keypair.
    ///
    /// The client generates the Ed25519 keypair locally and sends only the
    /// public key (base64-encoded). The server NEVER possesses the private key.
    ///
    /// Returns the signed `IdentityFile` ready for the client to save.
    pub fn issue_for_public_key(
        &self,
        username: String,
        user_public_key_b64: &str,
    ) -> Result<IdentityFile, IdentityError> {
        // Validate the provided public key is well-formed
        crate::keypair::decode_public_key(user_public_key_b64)?;

        let user_id = Uuid::new_v4().to_string();
        let issued_at = chrono::Utc::now();
        let expires_at = issued_at + Duration::days(self.validity_days);

        // Build canonical signing payload — order matters for verification
        let payload = canonical_payload(
            SCHEMA_VERSION,
            &user_id,
            user_public_key_b64,
            &self.server_domain,
            &issued_at.to_rfc3339(),
            &expires_at.to_rfc3339(),
        );

        let signature = self.server_signing_key.sign(payload.as_bytes());
        let server_signature = STANDARD.encode(signature.to_bytes());

        Ok(IdentityFile {
            identity: IdentityMeta {
                version: SCHEMA_VERSION.to_string(),
                user_id,
                public_key: user_public_key_b64.to_string(),
                issued_by: self.server_domain.clone(),
                issued_at,
                expires_at,
                server_signature,
            },
            profile: UserProfile {
                username,
                extra: Default::default(),
            },
            succession: None,
            contributions: None,
        })
    }

    /// Convenience: generate keypair + issue in one call (for testing).
    ///
    /// In production, the CLIENT should call `UserKeypair::generate()` and
    /// send only the public key via `issue_for_public_key()`.
    pub fn issue(&self, username: String) -> Result<(IdentityFile, String), IdentityError> {
        let user_keypair = crate::keypair::UserKeypair::generate();
        let identity = self.issue_for_public_key(
            username,
            &user_keypair.public_key_b64(),
        )?;
        let private_key = user_keypair.private_key_b64();
        Ok((identity, private_key))
    }
}

/// Build the canonical payload string that gets signed.
/// This MUST match between issuer and verifier exactly.
///
/// SECURITY: includes `expires_at` so the expiry cannot be tampered with
/// after issuance without invalidating the signature.
pub(crate) fn canonical_payload(
    version: &str,
    user_id: &str,
    public_key: &str,
    issued_by: &str,
    issued_at: &str,
    expires_at: &str,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        version, user_id, public_key, issued_by, issued_at, expires_at
    )
}
