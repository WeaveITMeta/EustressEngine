// =============================================================================
// Eustress Web - Slack link API
// =============================================================================
// Linking the signed-in account's Slack account (Worker: src/slack_link.mjs).
// The Worker builds the "Sign in with Slack" address; Slack sends the browser
// back to /auth/slack/callback, and that page hands the code to
// `finish_slack_link`. Only the account that began a link can finish it.
// Eustress keeps no Slack token: the Worker reads who the person is and in which
// workspace, then revokes the token at once.
//
// Table of Contents:
// 1. Types
// 2. Calls
// 3. What the account shows
// 4. What a refusal says
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

/// Starts a link: the Slack address to send the browser to.
pub async fn begin_slack_link(api_url: &str) -> Result<String, ApiError> {
    let begun: Begun = ApiClient::new(api_url).post("/api/auth/slack/begin", &serde_json::json!({})).await?;
    Ok(begun.authorize_url)
}

/// Finishes a link with what Slack handed the callback page. Answers the
/// account as it now is.
pub async fn finish_slack_link(api_url: &str, code: &str, state: &str) -> Result<User, ApiError> {
    let linked: Linked = ApiClient::new(api_url)
        .post("/api/auth/slack/finish", &serde_json::json!({ "code": code, "state": state }))
        .await?;
    Ok(linked.user)
}

/// Removes the account's Slack link. Answers the account as it now is.
pub async fn unlink_slack(api_url: &str) -> Result<User, ApiError> {
    let linked: Linked = ApiClient::new(api_url).post("/api/auth/slack/unlink", &serde_json::json!({})).await?;
    Ok(linked.user)
}

// -----------------------------------------------------------------------------
// 3. What the account shows
// -----------------------------------------------------------------------------

/// The Slack row's status line in Settings: who the link is and in which
/// workspace, as far as Slack said.
pub fn slack_status_text(user: &User) -> String {
    if !user.slack_linked {
        return "Not connected".to_string();
    }
    let name = user.slack_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let team = user.slack_team_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match (name, team) {
        (Some(name), Some(team)) => format!("Connected as {name} in {team}"),
        (Some(name), None) => format!("Connected as {name}"),
        (None, Some(team)) => format!("Connected in {team}"),
        (None, None) => "Connected".to_string(),
    }
}

// -----------------------------------------------------------------------------
// 4. What a refusal says
// -----------------------------------------------------------------------------

/// One sentence for a failed Slack call: the Worker's own words when it sent
/// them, otherwise what to do next.
pub fn slack_failure_sentence(error: &ApiError) -> String {
    match error {
        ApiError::Unauthorized => "Your session ended. Sign in again, then connect Slack from Settings.".to_string(),
        ApiError::Server { status, message } => {
            if let Some(kyc) = kyc_sentence(*status, message) {
                return kyc;
            }
            match worker_code(message).as_deref() {
                Some("slack_unavailable") => return "Slack linking is not switched on yet. Try again later.".to_string(),
                Some("slack_needs_https") => {
                    return "Slack only connects from the secure Eustress website. Open eustress.dev and connect Slack from Settings there.".to_string()
                }
                _ => {}
            }
            worker_sentence(message).unwrap_or_else(|| "Slack could not be connected. Try again in a minute.".to_string())
        }
        _ => "Eustress could not be reached. Check your connection and try again.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(extra: serde_json::Value) -> User {
        let mut v = serde_json::json!({
            "id": "71111111-1111-4111-8111-111111111111", "username": "ada", "email": null, "avatar_url": null,
            "discord_id": null, "created_at": "2026-01-01T00:00:00Z"
        });
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn slack_a_refusal_says_the_workers_sentence_or_what_to_do() {
        let taken = ApiError::Server { status: 409, message: r#"{"error":"That Slack account is already linked to another Eustress account.","code":"slack_taken"}"#.into() };
        assert_eq!(slack_failure_sentence(&taken), "That Slack account is already linked to another Eustress account.");
        let off = ApiError::Server { status: 503, message: r#"{"error":"x","code":"slack_unavailable"}"#.into() };
        assert_eq!(slack_failure_sentence(&off), "Slack linking is not switched on yet. Try again later.");
        let http = ApiError::Server { status: 400, message: r#"{"error":"x","code":"slack_needs_https"}"#.into() };
        assert!(slack_failure_sentence(&http).starts_with("Slack only connects from the secure Eustress website."));
        let blank = ApiError::Server { status: 502, message: String::new() };
        assert_eq!(slack_failure_sentence(&blank), "Slack could not be connected. Try again in a minute.");
        assert!(slack_failure_sentence(&ApiError::Unauthorized).starts_with("Your session ended"));
        assert!(slack_failure_sentence(&ApiError::Network("x".into())).starts_with("Eustress could not be reached"));
        for e in [&taken, &off, &http, &blank] {
            assert!(!slack_failure_sentence(e).contains('\u{2014}'), "no em dash");
        }
    }

    #[test]
    fn slack_the_account_reply_reads_a_link_and_its_absence() {
        let body = r#"{"ok":true,"user":{"id":"71111111-1111-4111-8111-111111111111","username":"ada","email":null,"avatar_url":null,
            "discord_id":null,"slack_linked":true,"slack_name":"Ada L","slack_team_name":"Acme","created_at":"2026-01-01T00:00:00Z"}}"#;
        let linked: Linked = serde_json::from_str(body).unwrap();
        assert!(linked.user.slack_linked);
        assert_eq!(linked.user.slack_name.as_deref(), Some("Ada L"));
        assert_eq!(linked.user.slack_team_name.as_deref(), Some("Acme"));
        // An answer from before Slack linking existed, or a session stored then, reads as not linked.
        let older = r#"{"user":{"id":"71111111-1111-4111-8111-111111111111","username":"ada","email":null,"avatar_url":null,
            "discord_id":null,"created_at":"2026-01-01T00:00:00Z"}}"#;
        let unlinked: Linked = serde_json::from_str(older).unwrap();
        assert!(!unlinked.user.slack_linked);
        assert_eq!(unlinked.user.slack_name, None);
        assert_eq!(unlinked.user.slack_team_name, None);
    }

    #[test]
    fn slack_status_says_who_and_which_workspace() {
        assert_eq!(slack_status_text(&user(serde_json::json!({}))), "Not connected");
        assert_eq!(
            slack_status_text(&user(serde_json::json!({ "slack_linked": false, "slack_name": "Ada", "slack_team_name": "Acme" }))),
            "Not connected"
        );
        assert_eq!(
            slack_status_text(&user(serde_json::json!({ "slack_linked": true, "slack_name": "Ada L", "slack_team_name": "Acme" }))),
            "Connected as Ada L in Acme"
        );
        assert_eq!(slack_status_text(&user(serde_json::json!({ "slack_linked": true, "slack_name": "Ada L" }))), "Connected as Ada L");
        assert_eq!(slack_status_text(&user(serde_json::json!({ "slack_linked": true, "slack_team_name": "Acme" }))), "Connected in Acme");
        assert_eq!(slack_status_text(&user(serde_json::json!({ "slack_linked": true, "slack_name": "  " }))), "Connected");
    }
}
