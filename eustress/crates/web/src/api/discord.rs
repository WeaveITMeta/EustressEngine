// =============================================================================
// Eustress Web - Discord link API
// =============================================================================
// Linking the signed-in account's Discord account (Worker: src/discord_link.mjs).
// The Worker builds Discord's sign-in address; Discord sends the browser back to
// /auth/discord/callback, and that page hands the code to `finish_discord_link`.
// Only the account that began a link can finish it. Eustress keeps no Discord
// token: the link reads who the person is on Discord and nothing else.
//
// Table of Contents:
// 1. Types
// 2. Calls
// 3. What a refusal says
// =============================================================================

use serde::Deserialize;

use super::{kyc_sentence, worker_code, worker_sentence, ApiClient, ApiError};
use crate::state::User;

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct Begun {
    authorize_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Linked {
    user: User,
}

// -----------------------------------------------------------------------------
// 2. Calls
// -----------------------------------------------------------------------------

/// Starts a link: the Discord address to send the browser to.
pub async fn begin_discord_link(api_url: &str) -> Result<String, ApiError> {
    let begun: Begun = ApiClient::new(api_url).post("/api/auth/discord/begin", &serde_json::json!({})).await?;
    Ok(begun.authorize_url)
}

/// Finishes a link with what Discord handed the callback page. Answers the
/// account as it now is.
pub async fn finish_discord_link(api_url: &str, code: &str, state: &str) -> Result<User, ApiError> {
    let linked: Linked = ApiClient::new(api_url)
        .post("/api/auth/discord/finish", &serde_json::json!({ "code": code, "state": state }))
        .await?;
    Ok(linked.user)
}

/// Removes the account's Discord link. Answers the account as it now is.
pub async fn unlink_discord(api_url: &str) -> Result<User, ApiError> {
    let linked: Linked = ApiClient::new(api_url).post("/api/auth/discord/unlink", &serde_json::json!({})).await?;
    Ok(linked.user)
}

// -----------------------------------------------------------------------------
// 3. What a refusal says
// -----------------------------------------------------------------------------

/// One sentence for a failed Discord call: the Worker's own words when it sent
/// them, otherwise what to do next.
pub fn discord_failure_sentence(error: &ApiError) -> String {
    match error {
        ApiError::Unauthorized => "Your session ended. Sign in again, then connect Discord from Settings.".to_string(),
        ApiError::Server { status, message } => {
            if let Some(kyc) = kyc_sentence(*status, message) {
                return kyc;
            }
            if worker_code(message).as_deref() == Some("discord_unavailable") {
                return "Discord linking is not switched on yet. Try again later.".to_string();
            }
            worker_sentence(message).unwrap_or_else(|| "Discord could not be connected. Try again in a minute.".to_string())
        }
        _ => "Eustress could not be reached. Check your connection and try again.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_the_workers_sentence_or_what_to_do() {
        let taken = ApiError::Server { status: 409, message: r#"{"error":"That Discord account is already linked to another Eustress account.","code":"discord_taken"}"#.into() };
        assert_eq!(discord_failure_sentence(&taken), "That Discord account is already linked to another Eustress account.");
        let off = ApiError::Server { status: 503, message: r#"{"error":"x","code":"discord_unavailable"}"#.into() };
        assert_eq!(discord_failure_sentence(&off), "Discord linking is not switched on yet. Try again later.");
        let blank = ApiError::Server { status: 502, message: String::new() };
        assert_eq!(discord_failure_sentence(&blank), "Discord could not be connected. Try again in a minute.");
        assert!(discord_failure_sentence(&ApiError::Unauthorized).starts_with("Your session ended"));
        assert!(discord_failure_sentence(&ApiError::Network("x".into())).starts_with("Eustress could not be reached"));
    }

    #[test]
    fn the_account_reply_reads_a_link_and_its_absence() {
        let body = r#"{"ok":true,"user":{"id":"71111111-1111-4111-8111-111111111111","username":"ada","email":null,"avatar_url":null,
            "discord_id":"100000000000000001","discord_username":"ada","bliss_balance":0,"ticket_balance":0,"created_at":"2026-01-01T00:00:00Z"}}"#;
        let linked: Linked = serde_json::from_str(body).unwrap();
        assert_eq!(linked.user.discord_id.as_deref(), Some("100000000000000001"));
        assert_eq!(linked.user.discord_username.as_deref(), Some("ada"));
        let older = r#"{"user":{"id":"71111111-1111-4111-8111-111111111111","username":"ada","email":null,"avatar_url":null,
            "discord_id":null,"created_at":"2026-01-01T00:00:00Z"}}"#;
        let unlinked: Linked = serde_json::from_str(older).unwrap();
        assert_eq!(unlinked.user.discord_id, None);
        assert_eq!(unlinked.user.discord_username, None);
    }
}
