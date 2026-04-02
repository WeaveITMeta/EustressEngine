//! =============================================================================
//! Succession — Inheritance / Will System
//! =============================================================================
//! The user opens their TOML and adds an ordered list of heir public keys.
//! The list is signed by the owner's private key (proves authorization).
//!
//! When succession activates:
//!   - The FIRST heir in the list receives the full Bliss balance
//!   - If the first heir is revoked/unavailable, fall to the next
//!   - Transferred assets are flagged as inherited (zero contribution score)
//!   - The original identity is archived, not deleted
//!   - Assets are NOT duplicated — single transfer to first valid heir
//!
//! This works like a will: you designate heirs before you die.
//! Anyone with the private key can edit the succession list.
//! =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::keypair::UserKeypair;

/// The succession block stored in the TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessionBlock {
    /// Ordered list of heir public keys (base64-encoded).
    /// First valid heir receives the full balance.
    /// Remaining entries are fallbacks.
    pub heirs: Vec<String>,
    /// When the succession list was last updated
    pub updated_at: DateTime<Utc>,
    /// Owner's signature over the heir list (proves authorization)
    pub owner_signature: String,
}

impl SuccessionBlock {
    /// Create a new signed succession block.
    ///
    /// SECURITY: The owner's public key is included in the canonical payload
    /// so the succession block cannot be transplanted to a different identity.
    pub fn create(keypair: &UserKeypair, heirs: Vec<String>) -> Self {
        let updated_at = chrono::Utc::now();
        let owner_pubkey = keypair.public_key_b64();
        let payload = canonical_succession_payload(&owner_pubkey, &heirs, &updated_at.to_rfc3339());
        let signature = keypair.sign_bytes(payload.as_bytes());

        Self {
            heirs,
            updated_at,
            owner_signature: signature,
        }
    }

    /// Verify the owner's signature on the succession list.
    pub fn verify(&self, owner_public_key_b64: &str) -> Result<(), crate::IdentityError> {
        let payload = canonical_succession_payload(owner_public_key_b64, &self.heirs, &self.updated_at.to_rfc3339());
        crate::keypair::verify_signed_data(
            owner_public_key_b64,
            payload.as_bytes(),
            &self.owner_signature,
        )
    }

    /// Get the first heir (primary beneficiary).
    pub fn primary_heir(&self) -> Option<&str> {
        self.heirs.first().map(|s| s.as_str())
    }

    /// Get the Nth fallback heir (0-indexed after primary).
    pub fn fallback_heir(&self, n: usize) -> Option<&str> {
        self.heirs.get(n + 1).map(|s| s.as_str())
    }

    /// Number of designated heirs.
    pub fn heir_count(&self) -> usize {
        self.heirs.len()
    }
}

/// Build the canonical payload that gets signed.
///
/// SECURITY: Includes the owner's public key to bind the succession
/// block to a specific identity. Without this, an attacker could copy
/// a signed succession block to a different identity file.
fn canonical_succession_payload(owner_pubkey: &str, heirs: &[String], updated_at: &str) -> String {
    let heirs_joined = heirs.join(",");
    format!("succession|{}|{}|{}", owner_pubkey, heirs_joined, updated_at)
}
