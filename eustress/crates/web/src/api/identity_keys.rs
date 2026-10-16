// =============================================================================
// Eustress Web - Replacing the identity key (API)
// =============================================================================
// The routes under /api/identity/rotate and the message both keys sign.
// Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md. The message
// is checked against docs/architecture/key-rotation-vectors.json, the file the
// Worker's tests read too.
//
// Table of Contents:
// 1. The message
// 2. Types
// 3. API Functions
// =============================================================================

use serde::{Deserialize, Serialize};

use super::{ApiClient, ApiError, PasskeyInfo};
use crate::state::User;

// -----------------------------------------------------------------------------
// 1. The message
// -----------------------------------------------------------------------------

/// What both signatures of a rotation cover.
pub const ROTATION_FORMAT: &str = "eustress-key-rotation-v1";

/// The message both the old key and the new key sign: six lines joined by
/// newlines, no trailing newline. Every field is hex or a UUID, so none can
/// hold a newline. `origin` is the API this request goes to.
pub fn rotation_message(origin: &str, account_id: &str, old_key: &str, new_key: &str, challenge: &str) -> String {
    format!("{ROTATION_FORMAT}\n{origin}\n{account_id}\n{old_key}\n{new_key}\n{challenge}")
}

// -----------------------------------------------------------------------------
// 2. Types
// -----------------------------------------------------------------------------

/// One API key the account owns (never the key itself).
#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub key_prefix: String,
    #[serde(default)]
    pub last4: String,
}

/// One webhook endpoint the account owns (its host only).
#[derive(Debug, Clone, Deserialize)]
pub struct WebhookInfo {
    pub id: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub livemode: bool,
}

/// What the account owns that acts as the account.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    pub api_keys: Vec<ApiKeyInfo>,
    #[serde(default)]
    pub webhooks: Vec<WebhookInfo>,
    #[serde(default)]
    pub discord_linked: bool,
    #[serde(default)]
    pub payout_connected: bool,
    /// The passkeys on the account: they sign in to the website, so replacing the identity file
    /// offers to remove them too.
    #[serde(default)]
    pub passkeys: Vec<PasskeyInfo>,
    /// False when the Worker could not list everything.
    #[serde(default = "yes")]
    pub complete: bool,
}

fn yes() -> bool {
    true
}

/// `POST /api/identity/rotate/begin`
#[derive(Debug, Clone, Deserialize)]
pub struct RotateBegin {
    pub challenge: String,
    pub origin: String,
    pub account_id: String,
    /// The key the account signs in with now.
    pub public_key: String,
    #[serde(default)]
    pub credentials: Credentials,
}

/// The credentials to revoke in the same step.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RevokeList {
    pub api_keys: Vec<String>,
    pub webhooks: Vec<String>,
    pub discord: bool,
    /// Passkey ids to remove in the same step.
    pub passkeys: Vec<String>,
}

/// `POST /api/identity/rotate`
#[derive(Debug, Serialize)]
pub struct RotateRequest {
    pub new_public_key: String,
    pub challenge: String,
    pub old_signature: String,
    pub new_signature: String,
    pub revoke: RevokeList,
}

/// The reply: a token for this device, the account, and what was frozen.
#[derive(Debug, Clone, Deserialize)]
pub struct RotateReply {
    pub token: String,
    pub user: User,
    #[serde(default)]
    pub key_epoch: u32,
    /// Milliseconds since 1970; 0 when nothing is frozen.
    #[serde(default)]
    pub value_frozen_until: f64,
    #[serde(default)]
    pub payout_pinned_until: f64,
    #[serde(default)]
    pub holds_value: bool,
    #[serde(default)]
    pub notices_sent: u32,
}

// -----------------------------------------------------------------------------
// 3. API Functions
// -----------------------------------------------------------------------------

/// A one-time challenge, and what the key owns. Needs a signed-in session.
pub async fn rotate_begin(client: &ApiClient) -> Result<RotateBegin, ApiError> {
    client.post("/api/identity/rotate/begin", &serde_json::json!({})).await
}

/// Replace the key. Both signatures cover [`rotation_message`].
pub async fn rotate_key(client: &ApiClient, request: &RotateRequest) -> Result<RotateReply, ApiError> {
    client.post("/api/identity/rotate", request).await
}

/// The one-time prompt to replace the key has been shown to this account.
pub async fn mark_key_prompt_seen(client: &ApiClient) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/identity/rotate/prompt-seen", &serde_json::json!({})).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::login::sign_challenge_ed25519;

    /// The one file the Worker's tests read too.
    const VECTORS: &str = include_str!("../../../../../docs/architecture/key-rotation-vectors.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS).expect("the shared vectors are JSON")
    }

    #[test]
    fn the_message_is_the_shared_one() {
        let v = vectors();
        let (old, new) = (v["old_key"]["public_key"].as_str().unwrap(), v["new_key"]["public_key"].as_str().unwrap());
        for r in v["rotations"].as_array().unwrap() {
            let message = rotation_message(r["origin"].as_str().unwrap(), r["account_id"].as_str().unwrap(), old, new, r["challenge"].as_str().unwrap());
            assert_eq!(message, r["message"].as_str().unwrap());
        }
    }

    #[test]
    fn signing_it_gives_the_shared_signatures() {
        let v = vectors();
        let (old_secret, new_secret) = (v["old_key"]["secret_key"].as_str().unwrap(), v["new_key"]["secret_key"].as_str().unwrap());
        for r in v["rotations"].as_array().unwrap() {
            let message = r["message"].as_str().unwrap();
            assert_eq!(sign_challenge_ed25519(old_secret, message).unwrap(), r["old_signature"].as_str().unwrap());
            assert_eq!(sign_challenge_ed25519(new_secret, message).unwrap(), r["new_signature"].as_str().unwrap());
        }
    }
}
