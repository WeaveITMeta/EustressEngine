//! =============================================================================
//! Schema — Canonical TOML identity file structure
//! =============================================================================
//! The [identity] block is the interoperability contract.
//! Every fork MUST honor schema version 1.0's identity fields.
//! The [profile] block is extensible — forks may add fields freely.
//! The [succession] block is the user's will — ordered heir list.
//! The [contributions] block carries portable signed event hashes.
//! =============================================================================

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::history::ContributionEntry;
use crate::keypair::UserKeypair;
use crate::succession::SuccessionBlock;

/// The full identity file stored on the user's Desktop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityFile {
    /// Core identity — signed by issuing server, fork-portable
    pub identity: IdentityMeta,
    /// User profile — extensible, forks add fields freely
    pub profile: UserProfile,
    /// Succession / will — ordered heir list, signed by owner
    #[serde(skip_serializing_if = "Option::is_none")]
    pub succession: Option<SuccessionBlock>,
    /// Portable contribution history — signed event hash chain
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contributions: Option<Vec<ContributionEntry>>,
}

/// Core identity block — all forks must honor this.
/// Server signs: version|user_id|public_key|issued_by|issued_at|expires_at
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityMeta {
    /// Schema version (e.g. "1.0")
    pub version: String,
    /// Unique user ID (UUID v4)
    pub user_id: String,
    /// User's Ed25519 public key, base64-encoded.
    /// This same key derives the Bliss wallet address.
    /// The private key never leaves the client.
    pub public_key: String,
    /// Domain of the issuing server
    pub issued_by: String,
    /// When this identity was issued
    pub issued_at: chrono::DateTime<chrono::Utc>,
    /// When this identity expires. Must be re-issued/renewed before this time.
    /// Included in the signed payload to prevent expiry manipulation.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Server's Ed25519 signature over the canonical identity fields
    pub server_signature: String,
}

/// Extensible profile block — forks may add fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Display name / username
    pub username: String,
    /// Arbitrary fork-specific fields
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl IdentityFile {
    /// Set the succession (will) block. Signed by the owner's private key.
    pub fn set_succession(&mut self, keypair: &UserKeypair, heirs: Vec<String>) {
        self.succession = Some(SuccessionBlock::create(keypair, heirs));
    }

    /// Clear the succession list (revoke the will).
    pub fn clear_succession(&mut self) {
        self.succession = None;
    }
}
