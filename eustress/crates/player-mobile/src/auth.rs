//! Identity sign-in.
//!
//! The same two calls Studio makes (`eustress/crates/engine/src/auth.rs`):
//!
//! ```text
//! POST /api/auth/challenge          { "public_key" }
//! POST /api/auth/verify-challenge   { public_key, challenge, signature, format }
//! GET  /api/auth/me                 Authorization: Bearer <token>
//! ```
//!
//! The signed text is [`eustress_common::api_base::login_message`]: the format
//! line, the API origin, and the 64-hex challenge. The token is stored by the
//! platform [`crate::platform`] under [`eustress_common::api_base::login_file`],
//! so a staging token is not offered to production.

use ed25519_dalek::Signer;
use eustress_common::api_base::{self, login_message, LOGIN_FORMAT};

use crate::platform;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    pub token: String,
    pub user_id: String,
    pub username: String,
}

/// `text` is either an identity file's toml or a path to that file.
pub fn sign_in(text: &str) -> Result<SignedIn, String> {
    let toml_text = if !text.contains("public_key") {
        let path = text.trim();
        if path.is_empty() {
            return Err("Paste an identity file, or the path to one.".into());
        }
        std::fs::read_to_string(path).map_err(|e| format!("Could not read the identity file: {e}"))?
    } else {
        text.to_string()
    };
    let key = IdentityKey::parse(&toml_text)?;
    let origin = api_base::api_origin();
    if origin == "null" {
        return Err("The API address is not valid.".into());
    }
    let agent = agent();
    let reply = agent
        .post(&api_base::api_url("/api/auth/challenge"))
        .send_json(serde_json::json!({ "public_key": key.public_hex.as_str() }))
        .map_err(http_err)?;
    let body: serde_json::Value = reply.into_json().map_err(|_| "Eustress sent an unreadable challenge.".to_string())?;
    let challenge = body["challenge"]
        .as_str()
        .filter(|c| is_challenge(c))
        .ok_or_else(|| "Eustress sent a challenge this Player does not sign.".to_string())?
        .to_string();
    let reply = agent
        .post(&api_base::api_url("/api/auth/verify-challenge"))
        .send_json(serde_json::json!({
            "public_key": key.public_hex.as_str(),
            "challenge": challenge.as_str(),
            "signature": key.sign_login(&origin, &challenge),
            "format": LOGIN_FORMAT,
        }))
        .map_err(http_err)?;
    let body: serde_json::Value = reply.into_json().map_err(|_| "Eustress sent an unreadable answer.".to_string())?;
    let token = body["token"].as_str().filter(|t| !t.is_empty()).ok_or("Eustress sent no token.")?.to_string();
    let user = user_from_json(&body["user"]).ok_or("Eustress sent an incomplete account.")?;
    platform::token_set(&api_base::login_file(), &token)?;
    Ok(SignedIn { token, user_id: user.0, username: user.1 })
}

/// The saved token, if `/api/auth/me` still accepts it.
pub fn restore() -> Result<Option<SignedIn>, String> {
    let Some(token) = platform::token_get(&api_base::login_file())? else {
        return Ok(None);
    };
    let agent = agent();
    let response = agent
        .get(&api_base::api_url("/api/auth/me"))
        .set("Authorization", &format!("Bearer {token}"))
        .call();
    match response {
        Ok(response) => {
            let body: serde_json::Value = response.into_json().map_err(|_| "Eustress sent an unreadable account.".to_string())?;
            let user = user_from_json(&body).or_else(|| user_from_json(&body["user"])).ok_or("Eustress sent an incomplete account.")?;
            Ok(Some(SignedIn { token, user_id: user.0, username: user.1 }))
        }
        Err(ureq::Error::Status(401, _)) => {
            let _ = platform::token_remove(&api_base::login_file());
            Ok(None)
        }
        Err(err) => Err(http_err(err)),
    }
}

struct IdentityKey {
    public_hex: String,
    signing: ed25519_dalek::SigningKey,
}

impl IdentityKey {
    /// The `[identity]` table's keys, or the same keys at the top of the file.
    fn parse(text: &str) -> Result<Self, String> {
        fn field<'a>(doc: &'a toml::Table, name: &str) -> Option<&'a str> {
            doc.get("identity")
                .and_then(|t| t.get(name))
                .or_else(|| doc.get(name))
                .and_then(|v| v.as_str())
                .map(str::trim)
        }
        let doc: toml::Table = toml::from_str(text).map_err(|_| "This is not an identity file.".to_string())?;
        let public_hex = field(&doc, "public_key").ok_or("This identity file has no public key.")?.to_ascii_lowercase();
        let private = field(&doc, "private_key").ok_or("This identity file has no private key, so it cannot sign in.")?;
        let public = hex32(&public_hex).ok_or("This identity file's public key is not valid.")?;
        let secret = hex32(private).ok_or("This identity file's private key is not valid.")?;
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        if signing.verifying_key().to_bytes() != public {
            return Err("This identity file's keys do not match.".into());
        }
        Ok(Self { public_hex, signing })
    }

    fn sign_login(&self, origin: &str, challenge: &str) -> String {
        hex::encode(self.signing.sign(login_message(origin, challenge).as_bytes()).to_bytes())
    }
}

fn is_challenge(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    hex::decode(text).ok()?.try_into().ok()
}

fn user_from_json(json: &serde_json::Value) -> Option<(String, String)> {
    let id = json["id"].as_str().filter(|s| !s.is_empty())?.to_string();
    let username = json["username"].as_str().filter(|s| !s.is_empty())?.to_string();
    Some((id, username))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(4))
        .timeout(std::time::Duration::from_secs(8))
        .redirects(0)
        .build()
}

fn http_err(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(404, _) => "This identity isn't registered on this server.".into(),
        ureq::Error::Status(401, _) => "Eustress did not accept this identity's signature.".into(),
        ureq::Error::Status(code, reply) => {
            let body: serde_json::Value = reply.into_json().unwrap_or_default();
            let said = body["error"].as_str().or_else(|| body["reason"].as_str()).unwrap_or("");
            if said.is_empty() {
                format!("Eustress could not sign you in (error {code}).")
            } else {
                said.chars().filter(|c| !c.is_control()).take(200).collect()
            }
        }
        ureq::Error::Transport(_) => "Could not reach Eustress.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signed_text_is_the_shared_login_message() {
        let message = login_message("https://api-staging.eustress.dev", &"ab".repeat(32));
        assert_eq!(
            message,
            format!("eustress-login-v1\nhttps://api-staging.eustress.dev\n{}", "ab".repeat(32))
        );
        assert!(is_challenge(&"ab".repeat(32)));
        assert!(!is_challenge("abc"));
    }

    #[test]
    fn a_mismatched_identity_file_is_refused() {
        let toml = "public_key = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nprivate_key = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n";
        match IdentityKey::parse(toml) {
            Err(err) => assert!(err.contains("do not match"), "{err}"),
            Ok(_) => panic!("mismatched keys were accepted"),
        }
    }
}
