// =============================================================================
// Eustress Web - Passkeys (API)
// =============================================================================
// The routes under /api/passkeys, and the two messages the identity file signs
// to add or remove one. Contract: docs/architecture/PASSKEYS.md, section 3.
//
// Table of Contents:
// 1. The messages
// 2. Types
// 3. API Functions
// =============================================================================

use serde::Deserialize;

use super::{ApiClient, ApiError, AuthResponse};

// -----------------------------------------------------------------------------
// 1. The messages
// -----------------------------------------------------------------------------

/// What the identity file signs to add a passkey.
pub const PASSKEY_ADD_FORMAT: &str = "eustress-passkey-add-v1";

/// What the identity file signs to remove a passkey.
pub const PASSKEY_REMOVE_FORMAT: &str = "eustress-passkey-remove-v1";

/// Five lines joined by newlines, no trailing newline. `origin` is the API the request goes to,
/// `credential_id` the new passkey's id in base64url, `challenge` the 32 challenge bytes as 64 lower-case hex digits (not the base64url text the Worker sent).
pub fn passkey_add_message(origin: &str, account_id: &str, credential_id: &str, challenge: &str) -> String {
    format!("{PASSKEY_ADD_FORMAT}\n{origin}\n{account_id}\n{credential_id}\n{challenge}")
}

/// The same five lines, for removing the passkey `credential_id`.
pub fn passkey_remove_message(origin: &str, account_id: &str, credential_id: &str, challenge: &str) -> String {
    format!("{PASSKEY_REMOVE_FORMAT}\n{origin}\n{account_id}\n{credential_id}\n{challenge}")
}

// -----------------------------------------------------------------------------
// 2. Types
// -----------------------------------------------------------------------------

/// A time the Worker wrote as milliseconds or seconds since 1970, or as ISO 8601 text.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Stamp {
    Number(f64),
    Text(String),
}

impl Stamp {
    /// The time in words, in the reader's zone. Empty when it is not a time.
    pub fn words(&self) -> String {
        match self {
            // Under 100 billion is seconds (that is before 1973 in milliseconds).
            Stamp::Number(n) if *n > 0.0 && *n < 1.0e11 => crate::time_words::when_secs(*n as i64),
            Stamp::Number(n) if *n > 0.0 => crate::time_words::when_ms(*n),
            Stamp::Number(_) => String::new(),
            Stamp::Text(t) => crate::time_words::when_iso(t),
        }
    }
}

/// One passkey on the account (never its key).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PasskeyInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: Option<Stamp>,
    #[serde(default)]
    pub last_used_at: Option<Stamp>,
    #[serde(default)]
    pub alg: Option<i64>,
    #[serde(default)]
    pub backed_up: bool,
}

#[derive(Debug, Deserialize)]
struct PasskeyListReply {
    #[serde(default)]
    passkeys: Vec<PasskeyInfo>,
}

/// `rp` of the add answer.
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeyRp {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// `user` of the add answer. `id` is the account's stable user handle, base64url.
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeyUser {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub display_name: String,
}

/// `authenticator_selection` of the add answer.
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeySelection {
    pub resident_key: String,
    pub user_verification: String,
}

/// `POST /api/passkeys/add/begin`
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeyAddBegin {
    /// base64url of 32 bytes.
    pub challenge: String,
    pub rp: PasskeyRp,
    pub user: PasskeyUser,
    pub pub_key_cred_params: Vec<i64>,
    #[serde(default)]
    pub exclude_credentials: Vec<String>,
    pub timeout_ms: u32,
    pub authenticator_selection: PasskeySelection,
    pub attestation: String,
    pub api_origin: String,
}

/// `POST /api/passkeys/login/begin`
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeyLoginBegin {
    /// base64url of 32 bytes.
    pub challenge: String,
    pub rp_id: String,
    pub timeout_ms: u32,
    pub user_verification: String,
}

/// `POST /api/passkeys/remove/begin`
#[derive(Debug, Clone, Deserialize)]
pub struct PasskeyRemoveBegin {
    /// base64url of 32 bytes.
    pub challenge: String,
    pub api_origin: String,
    /// The account the Worker signs the message for; the site prefers it to its own copy.
    #[serde(default)]
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PasskeyAddReply {
    passkey: PasskeyInfo,
}

// -----------------------------------------------------------------------------
// 3. API Functions
// -----------------------------------------------------------------------------

/// The account's passkeys. Any session may ask.
pub async fn passkeys_list(client: &ApiClient) -> Result<Vec<PasskeyInfo>, ApiError> {
    let reply: PasskeyListReply = client.get("/api/passkeys").await?;
    Ok(reply.passkeys)
}

/// A challenge and the options for a new passkey. Needs an identity-file session.
pub async fn passkey_add_begin(client: &ApiClient) -> Result<PasskeyAddBegin, ApiError> {
    client.post("/api/passkeys/add/begin", &serde_json::json!({})).await
}

/// Add the passkey. `body` is built by `webauthn::add_body`.
pub async fn passkey_add(client: &ApiClient, body: &serde_json::Value) -> Result<PasskeyInfo, ApiError> {
    let reply: PasskeyAddReply = client.post("/api/passkeys/add", body).await?;
    Ok(reply.passkey)
}

/// A challenge for removing the passkey `id`. Needs an identity-file session.
pub async fn passkey_remove_begin(client: &ApiClient, id: &str) -> Result<PasskeyRemoveBegin, ApiError> {
    client.post("/api/passkeys/remove/begin", &serde_json::json!({ "id": id })).await
}

/// Remove the passkey. `signature` covers [`passkey_remove_message`].
pub async fn passkey_remove(client: &ApiClient, id: &str, challenge: &str, signature: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = client
        .post("/api/passkeys/remove", &serde_json::json!({ "id": id, "challenge": challenge, "signature": signature }))
        .await?;
    Ok(())
}

/// A challenge for signing in. No session is needed.
pub async fn passkey_login_begin(client: &ApiClient) -> Result<PasskeyLoginBegin, ApiError> {
    client.post("/api/passkeys/login/begin", &serde_json::json!({})).await
}

/// Sign in. `body` is built by `webauthn::login_body`. The answer is the same as the identity-file
/// sign-in's; the caller stores the token (`AppState::login_with_token`).
pub async fn passkey_login(client: &ApiClient, body: &serde_json::Value) -> Result<AuthResponse, ApiError> {
    client.post("/api/passkeys/login", body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://api.eustress.dev";
    const ACCOUNT: &str = "11111111-2222-4333-8444-555555555555";
    const CHALLENGE: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn the_add_message_is_the_contracts_five_lines() {
        assert_eq!(
            passkey_add_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE),
            format!("eustress-passkey-add-v1\n{ORIGIN}\n{ACCOUNT}\nAQIDBA\n{CHALLENGE}")
        );
        let m = passkey_add_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE);
        assert_eq!(m.lines().count(), 5);
        assert!(!m.ends_with('\n'), "no trailing newline");
    }

    #[test]
    fn the_remove_message_is_the_contracts_five_lines() {
        assert_eq!(
            passkey_remove_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE),
            format!("eustress-passkey-remove-v1\n{ORIGIN}\n{ACCOUNT}\nAQIDBA\n{CHALLENGE}")
        );
        let m = passkey_remove_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE);
        assert_eq!(m.lines().count(), 5);
        assert!(!m.ends_with('\n'), "no trailing newline");
    }

    #[test]
    fn adding_and_removing_sign_different_things() {
        assert_ne!(passkey_add_message(ORIGIN, ACCOUNT, "x", CHALLENGE), passkey_remove_message(ORIGIN, ACCOUNT, "x", CHALLENGE));
    }

    #[test]
    fn a_signed_add_message_is_a_signature_the_identity_key_made() {
        // The same signer the login uses; the key is the shared test vector key.
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../../../docs/architecture/login-v1-vectors.json")).unwrap();
        let secret = v["secret_key"].as_str().unwrap();
        let add = crate::pages::login::sign_challenge_ed25519(secret, &passkey_add_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE)).unwrap();
        let remove = crate::pages::login::sign_challenge_ed25519(secret, &passkey_remove_message(ORIGIN, ACCOUNT, "AQIDBA", CHALLENGE)).unwrap();
        assert_eq!(add.len(), 128);
        assert_ne!(add, remove, "a signature to add cannot be replayed to remove");
    }

    #[test]
    fn the_list_answer_parses_with_times_in_any_of_the_three_forms() {
        let reply: PasskeyListReply = serde_json::from_str(
            r#"{ "passkeys": [
                { "id": "AQID", "name": "Laptop", "created_at": 1800000000000, "last_used_at": null, "alg": -7, "backed_up": true },
                { "id": "BAUG", "name": "Phone", "created_at": "2026-10-08T12:00:00Z", "last_used_at": 1800000000 },
                { "id": "BwgJ" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(reply.passkeys.len(), 3);
        assert_eq!(reply.passkeys[0].created_at.as_ref().unwrap().words(), "Jan 15, 2027, 8:00 AM UTC");
        assert_eq!(reply.passkeys[0].last_used_at, None);
        assert_eq!(reply.passkeys[1].created_at.as_ref().unwrap().words(), "Oct 8, 2026, 12:00 PM UTC");
        assert_eq!(reply.passkeys[1].last_used_at.as_ref().unwrap().words(), "Jan 15, 2027, 8:00 AM UTC", "seconds are read as seconds");
        assert_eq!(reply.passkeys[2].name, "");
        assert!(!reply.passkeys[2].backed_up);
    }

    #[test]
    fn a_zero_or_missing_time_says_nothing() {
        assert_eq!(Stamp::Number(0.0).words(), "");
        assert_eq!(Stamp::Number(-5.0).words(), "");
    }

    #[test]
    fn no_text_in_this_file_uses_a_long_dash() {
        let source = include_str!("passkeys.rs");
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "a long dash in api/passkeys.rs");
        }
    }
}
