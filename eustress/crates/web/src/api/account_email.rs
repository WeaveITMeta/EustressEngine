// =============================================================================
// Eustress Web - The account's email (API)
// =============================================================================
// The routes under /api/account/email: verify the address on file, ask for a
// new one (signed with the current key, waits seven days, the addresses the
// account already had can cancel it), cancel, and confirm after the wait.
// Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified
// email". The message the key signs is checked against
// docs/architecture/email-change-vectors.json, the file the Worker's tests read.
//
// Table of Contents:
// 1. The message
// 2. Types
// 3. Words
// 4. API Functions
// =============================================================================

use serde::{Deserialize, Serialize};

use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. The message
// -----------------------------------------------------------------------------

/// What the current key signs to ask for a new address.
pub const EMAIL_CHANGE_FORMAT: &str = "eustress-email-change-v1";

/// The message the current key signs: six lines joined by newlines, no trailing
/// newline. `address` is the one plain address, trimmed and in lower case (see
/// [`clean_address`]); `origin` is the API this request goes to.
pub fn email_change_message(origin: &str, account_id: &str, key: &str, address: &str, challenge: &str) -> String {
    format!("{EMAIL_CHANGE_FORMAT}\n{origin}\n{account_id}\n{key}\n{address}\n{challenge}")
}

/// What the holder of a NEW key signs, to show support that the key was not pasted from
/// nowhere: five lines joined by newlines (docs/architecture/admin-replace-vectors.json).
/// `origin` is the API this key will be used with; `username` is the account's, as
/// registered.
pub fn admin_replace_key_message(origin: &str, username: &str, case_ref: &str, new_key: &str) -> String {
    format!("eustress-admin-replace-key-v1\n{origin}\n{username}\n{case_ref}\n{new_key}")
}

/// An address as the Worker reads it: trimmed and lower case, or `None` when it
/// is not one plain address. The key signs this spelling.
pub fn clean_address(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v.len() > 254 || v.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | ',' | ';' | '"')) {
        return None;
    }
    let (local, domain) = v.split_once('@')?;
    if local.is_empty() || local.len() > 64 || domain.contains('@') || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') || domain.contains("..") {
        return None;
    }
    Some(v.to_lowercase())
}

// -----------------------------------------------------------------------------
// 2. Types
// -----------------------------------------------------------------------------

/// A change of address that was asked for and has not taken effect.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct EmailPending {
    pub address: String,
    #[serde(default)]
    pub requested_at: String,
    /// When it may be made the address on file (ISO 8601).
    pub effective_at: String,
    /// The waiting period is over: the owner may confirm it.
    #[serde(default)]
    pub ready: bool,
}

/// What the account knows about its address.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmailView {
    #[serde(default)]
    pub on_file: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub verified_at: Option<String>,
    /// Verified for thirty days: counts as a recovery factor.
    #[serde(default)]
    pub aged: bool,
    #[serde(default)]
    pub aged_at: Option<String>,
    #[serde(default)]
    pub pending: Option<EmailPending>,
}

#[derive(Debug, Deserialize)]
struct StatusReply {
    email: EmailView,
}

/// `POST /api/account/email/start`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StartReply {
    #[serde(default)]
    pub sent_to: Option<String>,
    #[serde(default)]
    pub already_verified: bool,
}

/// `POST /api/account/email/change/begin`
#[derive(Debug, Clone, Deserialize)]
pub struct EmailChangeBegin {
    pub challenge: String,
    pub origin: String,
    pub account_id: String,
    /// The key the account signs in with now.
    pub public_key: String,
}

#[derive(Debug, Serialize)]
struct EmailChangeRequest<'a> {
    challenge: &'a str,
    new_address: &'a str,
    signature: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChangeReply {
    pending: EmailPending,
}

// -----------------------------------------------------------------------------
// 3. Words
// -----------------------------------------------------------------------------

/// A time given as ISO 8601, in the reader's own zone and clock ("Oct 8, 2026, 5:00 AM MST"), or the text
/// itself when it does not parse. See `time_words`.
pub fn iso_to_words(iso: &str) -> String {
    crate::time_words::when_iso(iso)
}

/// "October 31, 2026" from the parts of a date (the month counted from zero).
pub fn date_words(year: i32, month0: u32, day: u32) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{} {}, {}", MONTHS.get(month0 as usize).copied().unwrap_or("?"), day, year)
}

/// The date of a time given as ISO 8601, in the reader's own time zone, so the date on the
/// page is the one on their calendar. The text itself when it does not parse.
#[cfg(not(feature = "ssr"))]
pub fn iso_to_local_date(iso: &str) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(iso));
    if d.get_time().is_nan() {
        return iso.to_string();
    }
    date_words(d.get_full_year() as i32, d.get_month(), d.get_date())
}

/// On the server there is no reader's calendar: the date in UTC.
#[cfg(feature = "ssr")]
pub fn iso_to_local_date(iso: &str) -> String {
    use chrono::Datelike;
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| { let t = t.with_timezone(&chrono::Utc); date_words(t.year(), t.month0(), t.day()) })
        .unwrap_or_else(|_| iso.to_string())
}

/// Why a link in an email did not work, in the three ways a page treats differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFailure {
    /// It worked once and ran out of time: ask for a new one.
    Expired,
    /// Used, wrong, cut off, or made for something that has since changed: nothing to retry.
    Invalid,
    /// The answer did not come, or came as an error on the server's side: the same link may work in a minute.
    Unavailable,
}

/// Sort a failed link call. The Worker names an expired link (`token_expired`, 410) and a link that is not
/// valid (`token_invalid`, 400); anything else on the server's side, or no answer, is "try again".
pub fn link_failure(error: &ApiError) -> LinkFailure {
    match error {
        ApiError::Server { status, message } => {
            let code = serde_json::from_str::<serde_json::Value>(message).ok().and_then(|v| v.get("code").and_then(|c| c.as_str()).map(str::to_string));
            match (status, code.as_deref()) {
                (_, Some("token_expired")) | (410, _) => LinkFailure::Expired,
                (_, Some("token_invalid")) => LinkFailure::Invalid,
                (400..=499, _) if *status != 429 && *status != 408 => LinkFailure::Invalid,
                _ => LinkFailure::Unavailable,
            }
        }
        _ => LinkFailure::Unavailable,
    }
}

/// What a refusal says when the account was made before identity verification was required and has not
/// verified yet: the Worker answers every change with 403 `kyc_required`. The banner on every page and
/// /verify-account are where it verifies.
pub const KYC_REQUIRED_SENTENCE: &str = "Verify your identity to do this.";

/// What a refusal says when the ID is already linked to another account (409 `kyc_id_in_use`).
pub const KYC_ID_IN_USE_SENTENCE: &str = "This ID is already linked to another Eustress account. Contact support@eustress.dev.";

/// The Worker's `code` in an error body, when the body is its JSON and has one.
pub fn worker_code(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body).ok()?.get("code")?.as_str().map(str::to_string)
}

/// The Worker's own `error` sentence in an error body, when it sent one.
pub fn worker_sentence(body: &str) -> Option<String> {
    let said = serde_json::from_str::<serde_json::Value>(body).ok()?.get("error")?.as_str()?.trim().to_string();
    (!said.is_empty()).then_some(said)
}

/// The sentence for the two identity refusals, or `None` for any other answer. Both the API helpers
/// that keep the body and the ones that already unwrapped it (commerce) call this with the raw body.
pub fn kyc_sentence(status: u16, body: &str) -> Option<String> {
    match (status, worker_code(body).as_deref()) {
        (403, Some("kyc_required")) => Some(KYC_REQUIRED_SENTENCE.to_string()),
        (409, Some("kyc_id_in_use")) => Some(KYC_ID_IN_USE_SENTENCE.to_string()),
        _ => None,
    }
}

/// Whether a failed call was refused because the account must verify its identity first: the Worker's
/// JSON body, or the sentence the commerce calls already put in its place.
pub fn is_kyc_required(error: &ApiError) -> bool {
    matches!(error, ApiError::Server { status: 403, message }
        if worker_code(message).as_deref() == Some("kyc_required") || message == KYC_REQUIRED_SENTENCE)
}

/// The words for a failed call: the Worker's own sentence when it sent one.
pub fn failure_sentence(error: &ApiError) -> String {
    match error {
        ApiError::Unauthorized => "Your sign-in has ended. Sign in again, then try once more.".to_string(),
        ApiError::NotFound => "That is not available here, or there is nothing to do.".to_string(),
        ApiError::Server { status, message } => kyc_sentence(*status, message)
            .or_else(|| worker_sentence(message))
            .unwrap_or_else(|| format!("The server answered {status}. Try again in a minute.")),
        _ => "Could not reach Eustress. Check your connection and try again.".to_string(),
    }
}

// -----------------------------------------------------------------------------
// 4. API Functions
// -----------------------------------------------------------------------------

/// What the account knows about its address. Needs a signed-in session.
pub async fn email_status(client: &ApiClient) -> Result<EmailView, ApiError> {
    let reply: StatusReply = client.get("/api/account/email").await?;
    Ok(reply.email)
}

/// Send the first verification link to the address on file (never to another).
pub async fn email_start(client: &ApiClient) -> Result<StartReply, ApiError> {
    client.post("/api/account/email/start", &serde_json::json!({})).await
}

/// What the Worker says when the link was opened and the address is now verified.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfirmReply {
    #[serde(default)]
    pub verified_at: Option<String>,
    /// From when the address counts for account recovery (ISO 8601).
    #[serde(default)]
    pub aged_at: Option<String>,
    /// The address, masked (`m***@example.com`).
    #[serde(default)]
    pub address: Option<String>,
}

/// The link was opened and the button pressed: verify the address. No session
/// is needed; the token is the proof.
pub async fn email_confirm(client: &ApiClient, token: &str) -> Result<ConfirmReply, ApiError> {
    client.post("/api/account/email/confirm", &serde_json::json!({ "token": token })).await
}

/// A one-time challenge for asking to change the address.
pub async fn email_change_begin(client: &ApiClient) -> Result<EmailChangeBegin, ApiError> {
    client.post("/api/account/email/change/begin", &serde_json::json!({})).await
}

/// Ask for a new address. `signature` covers [`email_change_message`].
pub async fn email_change(client: &ApiClient, challenge: &str, new_address: &str, signature: &str) -> Result<EmailPending, ApiError> {
    let reply: ChangeReply = client.post("/api/account/email/change", &EmailChangeRequest { challenge, new_address, signature }).await?;
    Ok(reply.pending)
}

/// Stop a pending change from a signed-in session.
pub async fn email_cancel_session(client: &ApiClient) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/account/email/cancel", &serde_json::json!({})).await?;
    Ok(())
}

/// Stop a pending change with the link from the notice: this also ends every
/// sign-in on the account. No session is needed.
pub async fn email_cancel_link(client: &ApiClient, token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/account/email/cancel", &serde_json::json!({ "token": token })).await?;
    Ok(())
}

/// The link in the notice of an administrator's replacement: put the earlier key
/// back. No session is needed; the token is the proof.
pub async fn key_restore(client: &ApiClient, token: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/account/key-restore", &serde_json::json!({ "token": token })).await?;
    Ok(())
}

/// Tell the Worker the reader's time zone, so the emails it sends the account give times in it. A display preference: any
/// session may set it, and a zone the Worker does not know is refused.
pub async fn set_time_zone(client: &ApiClient, zone: &str) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/account/time-zone", &serde_json::json!({ "time_zone": zone })).await?;
    Ok(())
}

/// After the wait, make the new address the one on file.
pub async fn email_apply(client: &ApiClient) -> Result<(), ApiError> {
    let _: serde_json::Value = client.post("/api/account/email/apply", &serde_json::json!({})).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::login::sign_challenge_ed25519;

    /// The one file the Worker's tests read too.
    const VECTORS: &str = include_str!("../../../../../docs/architecture/email-change-vectors.json");

    fn vectors() -> serde_json::Value {
        serde_json::from_str(VECTORS).expect("the shared vectors are JSON")
    }

    #[test]
    fn the_message_is_the_shared_one() {
        let v = vectors();
        let key = v["key"]["public_key"].as_str().unwrap();
        for c in v["changes"].as_array().unwrap() {
            let message = email_change_message(c["origin"].as_str().unwrap(), c["account_id"].as_str().unwrap(), key, c["address"].as_str().unwrap(), c["challenge"].as_str().unwrap());
            assert_eq!(message, c["message"].as_str().unwrap());
        }
    }

    #[test]
    fn signing_it_gives_the_shared_signature() {
        let v = vectors();
        let secret = v["key"]["secret_key"].as_str().unwrap();
        for c in v["changes"].as_array().unwrap() {
            assert_eq!(sign_challenge_ed25519(secret, c["message"].as_str().unwrap()).unwrap(), c["signature"].as_str().unwrap());
        }
    }

    /// The vectors of an administrator's replacement: the file the Worker's tests and the support script read.
    const ADMIN_VECTORS: &str = include_str!("../../../../../docs/architecture/admin-replace-vectors.json");

    #[test]
    fn the_message_the_new_key_signs_is_the_shared_one_and_signs_to_the_shared_signature() {
        let v: serde_json::Value = serde_json::from_str(ADMIN_VECTORS).expect("the shared vectors are JSON");
        let (new_key, secret) = (v["new_key"]["public_key"].as_str().unwrap(), v["new_key"]["secret_key"].as_str().unwrap());
        for c in v["cases"].as_array().unwrap() {
            let message = admin_replace_key_message(c["origin"].as_str().unwrap(), c["username"].as_str().unwrap(), c["case_ref"].as_str().unwrap(), new_key);
            assert_eq!(message, c["key_message"].as_str().unwrap());
            assert_eq!(sign_challenge_ed25519(secret, &message).unwrap(), c["key_signature"].as_str().unwrap());
        }
    }

    #[test]
    fn an_address_is_cleaned_the_way_the_worker_cleans_it() {
        assert_eq!(clean_address("  Owner@Example.COM ").as_deref(), Some("owner@example.com"));
        assert_eq!(clean_address("a+b@sub.example.org").as_deref(), Some("a+b@sub.example.org"));
        for bad in ["", "no-at-sign", "a@b", "a b@c.de", "a@b.cd, d@e.fg", "Name <a@b.cd>", "a@@b.cd", "@b.cd", "a@.cd", "a@b..cd", "a@b.cd\nBcc: x@y.zz"] {
            assert_eq!(clean_address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_time_is_put_in_words_and_text_that_is_not_a_time_is_kept() {
        // Without a browser the time is in UTC, on a twelve-hour clock, with the zone named.
        assert_eq!(iso_to_words("2026-10-08T12:00:00.000Z"), "Oct 8, 2026, 12:00 PM UTC");
        assert_eq!(iso_to_words("soon"), "soon");
    }

    fn server(status: u16, body: &str) -> ApiError {
        ApiError::Server { status, message: body.to_string() }
    }

    #[test]
    fn an_expired_link_is_told_from_one_that_is_not_valid_and_from_one_that_did_not_arrive() {
        assert_eq!(link_failure(&server(410, "{\"code\":\"token_expired\"}")), LinkFailure::Expired);
        assert_eq!(link_failure(&server(410, "")), LinkFailure::Expired, "the status alone says expired");
        assert_eq!(link_failure(&server(400, "{\"code\":\"token_invalid\"}")), LinkFailure::Invalid);
        assert_eq!(link_failure(&server(400, "not json")), LinkFailure::Invalid);
        assert_eq!(link_failure(&server(409, "{}")), LinkFailure::Invalid);
        assert_eq!(link_failure(&server(429, "{\"code\":\"rate_limited\"}")), LinkFailure::Unavailable, "a limit is a reason to wait, not a verdict on the link");
        assert_eq!(link_failure(&server(503, "{\"code\":\"account_unavailable\"}")), LinkFailure::Unavailable);
        assert_eq!(link_failure(&server(501, "{\"code\":\"not_configured\"}")), LinkFailure::Unavailable);
        assert_eq!(link_failure(&ApiError::Network("offline".to_string())), LinkFailure::Unavailable);
        assert_eq!(link_failure(&ApiError::NotFound), LinkFailure::Unavailable, "a route that is not there is not the link's fault");
        assert_eq!(link_failure(&ApiError::Unauthorized), LinkFailure::Unavailable);
        assert_eq!(link_failure(&ApiError::Deserialize("x".to_string())), LinkFailure::Unavailable);
    }

    #[test]
    fn a_confirmed_answer_reads_the_masked_address_and_the_recovery_date() {
        let r: ConfirmReply = serde_json::from_str("{\"ok\":true,\"verified_at\":\"2026-10-01T12:00:00.000Z\",\"aged_at\":\"2026-10-31T12:00:00.000Z\",\"address\":\"m***@gmail.com\"}").unwrap();
        assert_eq!(r.address.as_deref(), Some("m***@gmail.com"));
        assert_eq!(r.aged_at.as_deref(), Some("2026-10-31T12:00:00.000Z"));
        let older: ConfirmReply = serde_json::from_str("{\"ok\":true}").unwrap();
        assert!(older.address.is_none() && older.aged_at.is_none(), "an answer without them is still an answer");
    }

    #[test]
    fn a_date_is_said_with_the_month_in_words() {
        assert_eq!(date_words(2026, 9, 31), "October 31, 2026");
        assert_eq!(date_words(2027, 0, 1), "January 1, 2027");
        assert_eq!(date_words(2026, 11, 5), "December 5, 2026");
    }

    #[test]
    fn a_failure_is_in_the_workers_words_when_it_has_some() {
        let e = ApiError::Server { status: 429, message: "{\"error\":\"Too many requests. Try again later.\",\"code\":\"rate_limited\"}".to_string() };
        assert_eq!(failure_sentence(&e), "Too many requests. Try again later.");
        let plain = ApiError::Server { status: 502, message: "bad gateway".to_string() };
        assert!(failure_sentence(&plain).contains("502"));
        assert!(failure_sentence(&ApiError::Unauthorized).contains("Sign in again"));
    }
}
