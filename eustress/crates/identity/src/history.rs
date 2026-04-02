//! =============================================================================
//! History — Portable contribution hash chain with server co-signatures
//! =============================================================================
//! Each contribution event is hashed and signed by BOTH:
//!   1. The user's key (proves they authored it)
//!   2. The server's key (proves the server witnessed it)
//!
//! Events chain: each hash includes the previous hash, forming
//! a tamper-evident linked list of contributions.
//!
//! This travels with the TOML. Any fork can verify the chain
//! without querying the original server.
//!
//! An AI with access to the user's machine could forge user signatures,
//! but cannot forge the server co-signature. Both are required for validity.
//! =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::keypair::UserKeypair;
use crate::IdentityError;

/// A single contribution event in the hash chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionEntry {
    /// Hash of this entry (SHA-256 of: prev_hash + session_id + score + fork + timestamp)
    pub hash: String,
    /// Hash of the previous entry (empty string for first entry)
    pub prev_hash: String,
    /// Session identifier on the originating server
    pub session_id: String,
    /// Contribution score earned in this session
    pub score: f64,
    /// Fork/server domain where this contribution was earned
    pub fork: String,
    /// When the contribution was recorded
    pub timestamp: DateTime<Utc>,
    /// User's signature over the hash (proves they authored it)
    pub signature: String,
    /// Server's co-signature over the hash (proves the server witnessed it).
    /// An AI on the user's machine can forge user signatures but NOT this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_signature: Option<String>,
}

/// Rolling contribution history.
pub struct ContributionHistory {
    pub entries: Vec<ContributionEntry>,
}

impl ContributionHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Load from existing entries (e.g. from a TOML file).
    pub fn from_entries(entries: Vec<ContributionEntry>) -> Self {
        Self { entries }
    }

    /// Record a new contribution event, chaining to the previous hash.
    /// User-signed only — server co-signature added separately via `co_sign_last`.
    pub fn record(
        &mut self,
        keypair: &UserKeypair,
        session_id: &str,
        score: f64,
        fork: &str,
    ) {
        let prev_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_default();

        let timestamp = Utc::now();
        let hash = compute_entry_hash(
            &prev_hash,
            session_id,
            score,
            fork,
            &timestamp.to_rfc3339(),
        );

        let signature = keypair.sign_bytes(hash.as_bytes());

        self.entries.push(ContributionEntry {
            hash,
            prev_hash,
            session_id: session_id.to_string(),
            score,
            fork: fork.to_string(),
            timestamp,
            signature,
            server_signature: None,
        });
    }

    /// Add a server co-signature to the last recorded entry.
    /// Called by the server after witnessing the contribution.
    pub fn co_sign_last(&mut self, server_keypair: &UserKeypair) {
        if let Some(last) = self.entries.last_mut() {
            last.server_signature = Some(server_keypair.sign_bytes(last.hash.as_bytes()));
        }
    }

    /// Verify the entire hash chain is intact and all signatures are valid.
    ///
    /// If `server_public_key_b64` is provided, also verifies server co-signatures.
    /// Entries without a server co-signature are accepted but flagged as
    /// user-only (lower trust level).
    pub fn verify_chain(&self, owner_public_key_b64: &str) -> Result<(), IdentityError> {
        self.verify_chain_with_server(owner_public_key_b64, None)
    }

    /// Full verification: user signatures + optional server co-signatures.
    pub fn verify_chain_with_server(
        &self,
        owner_public_key_b64: &str,
        server_public_key_b64: Option<&str>,
    ) -> Result<(), IdentityError> {
        let mut expected_prev = String::new();

        for entry in &self.entries {
            // Verify chain linkage
            if entry.prev_hash != expected_prev {
                return Err(IdentityError::InvalidSignature);
            }

            // Verify hash computation
            let computed = compute_entry_hash(
                &entry.prev_hash,
                &entry.session_id,
                entry.score,
                &entry.fork,
                &entry.timestamp.to_rfc3339(),
            );
            if computed != entry.hash {
                return Err(IdentityError::InvalidSignature);
            }

            // Verify owner's signature
            crate::keypair::verify_signed_data(
                owner_public_key_b64,
                entry.hash.as_bytes(),
                &entry.signature,
            )?;

            // Verify server co-signature if present and server key provided
            if let (Some(server_sig), Some(server_key)) =
                (&entry.server_signature, server_public_key_b64)
            {
                crate::keypair::verify_signed_data(
                    server_key,
                    entry.hash.as_bytes(),
                    server_sig,
                )?;
            }

            expected_prev = entry.hash.clone();
        }

        Ok(())
    }

    /// Get total contribution score across all entries.
    pub fn total_score(&self) -> f64 {
        self.entries.iter().map(|e| e.score).sum()
    }

    /// Get total score for a specific fork.
    pub fn score_for_fork(&self, fork: &str) -> f64 {
        self.entries
            .iter()
            .filter(|e| e.fork == fork)
            .map(|e| e.score)
            .sum()
    }

    /// Get the number of unique forks contributed to.
    pub fn fork_count(&self) -> usize {
        let forks: std::collections::HashSet<&str> =
            self.entries.iter().map(|e| e.fork.as_str()).collect();
        forks.len()
    }

    /// Count entries with server co-signatures (higher trust).
    pub fn co_signed_count(&self) -> usize {
        self.entries.iter().filter(|e| e.server_signature.is_some()).count()
    }
}

impl Default for ContributionHistory {
    fn default() -> Self {
        Self::new()
    }
}

fn compute_entry_hash(
    prev_hash: &str,
    session_id: &str,
    score: f64,
    fork: &str,
    timestamp: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(session_id.as_bytes());
    hasher.update(b"|");
    hasher.update(score.to_le_bytes());
    hasher.update(b"|");
    hasher.update(fork.as_bytes());
    hasher.update(b"|");
    hasher.update(timestamp.as_bytes());
    hex::encode(hasher.finalize())
}
