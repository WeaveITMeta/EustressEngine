//! =============================================================================
//! Revocation — Append-only revocation list with server signing
//! =============================================================================
//! When an identity is compromised, the issuing server adds the user_id
//! to its revocation list. Other servers fetch this list via:
//!   GET /.well-known/eustress-revoked
//!
//! Revocation is append-only — entries cannot be removed.
//! The list is signed by the issuing server for authenticity.
//! The index is auto-rebuilt on deserialization.
//! =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::IdentityError;

/// Individual revocation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevocationEntry {
    /// The revoked user's ID
    pub user_id: String,
    /// When the revocation was issued
    pub revoked_at: DateTime<Utc>,
    /// Optional reason (e.g. "key compromised", "succession activated")
    pub reason: Option<String>,
}

/// Append-only revocation list, signed by the issuing server.
#[derive(Debug, Clone, Serialize)]
pub struct RevocationList {
    /// Server that issued these revocations
    pub issued_by: String,
    /// Monotonically increasing version
    pub list_version: u64,
    /// All revocation entries (append-only)
    pub entries: Vec<RevocationEntry>,
    /// Server signature over the canonical list payload.
    /// Updated every time `sign()` is called.
    pub signature: String,
    /// O(1) lookup index — not serialized, rebuilt on load
    #[serde(skip)]
    index: HashSet<String>,
}

/// Custom Deserialize that auto-rebuilds the index after loading.
impl<'de> Deserialize<'de> for RevocationList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            issued_by: String,
            list_version: u64,
            entries: Vec<RevocationEntry>,
            signature: String,
        }

        let raw = Raw::deserialize(deserializer)?;
        let index = raw.entries.iter().map(|e| e.user_id.clone()).collect();

        Ok(RevocationList {
            issued_by: raw.issued_by,
            list_version: raw.list_version,
            entries: raw.entries,
            signature: raw.signature,
            index,
        })
    }
}

impl RevocationList {
    pub fn new(issued_by: String) -> Self {
        Self {
            issued_by,
            list_version: 0,
            entries: Vec::new(),
            signature: String::new(),
            index: HashSet::new(),
        }
    }

    /// Revoke a user ID. Append-only — cannot be undone.
    pub fn revoke(&mut self, user_id: String, reason: Option<String>) {
        if self.index.contains(&user_id) {
            return; // Already revoked
        }
        self.index.insert(user_id.clone());
        self.entries.push(RevocationEntry {
            user_id,
            revoked_at: Utc::now(),
            reason,
        });
        self.list_version += 1;
    }

    /// Check if a user ID has been revoked.
    pub fn is_revoked(&self, user_id: &str) -> bool {
        self.index.contains(user_id)
    }

    /// Rebuild the index from entries (for manual use; deserialization does this automatically).
    pub fn rebuild_index(&mut self) {
        self.index = self.entries.iter().map(|e| e.user_id.clone()).collect();
    }

    /// Number of revoked identities.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Build the canonical payload for signing.
    /// Format: "revocation|issued_by|list_version|user_id_1,user_id_2,..."
    pub fn canonical_payload(&self) -> String {
        let ids: Vec<&str> = self.entries.iter().map(|e| e.user_id.as_str()).collect();
        format!(
            "revocation|{}|{}|{}",
            self.issued_by,
            self.list_version,
            ids.join(","),
        )
    }

    /// Sign the revocation list with the server's keypair.
    /// Must be called after adding revocations and before publishing.
    pub fn sign(&mut self, server_keypair: &crate::keypair::UserKeypair) {
        let payload = self.canonical_payload();
        self.signature = server_keypair.sign_bytes(payload.as_bytes());
    }

    /// Verify the server's signature on this revocation list.
    pub fn verify_signature(&self, server_public_key_b64: &str) -> Result<(), IdentityError> {
        if self.signature.is_empty() {
            return Err(IdentityError::InvalidSignature);
        }
        let payload = self.canonical_payload();
        crate::keypair::verify_signed_data(
            server_public_key_b64,
            payload.as_bytes(),
            &self.signature,
        )
    }
}
