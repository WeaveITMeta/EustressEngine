// =============================================================================
// Eustress Web - Delete and restore a published Space (/projects)
// =============================================================================
// A published Space is a listing. Deleting it hides it from everyone at once and removes it for good after the restore window
// (90 days today). The owner proves the request with a six-digit code the Worker emails to the verified address. The contract is
// docs/architecture/DELETE_PUBLISHED_SPACE.md (section 10) and infrastructure/cloudflare/api/src/space_delete.mjs:
//
//   GET  /api/account/spaces/{id}/deletion        what a deletion removes, keeps and cannot reach; whether one can start;
//                                                 where an existing one is. Every list on the first step comes from here.
//   POST /api/account/spaces/{id}/delete-request  {typed_name}                 -> request_id, sent_to, expires_at, attempts_left
//   POST /api/account/spaces/{id}/delete-confirm  {request_id, typed_name, code} -> state "soft_deleted", dates
//   POST /api/account/spaces/{id}/restore-request, restore-confirm              the same two steps for a restore
//
// The code goes in the body of the confirm and nowhere else: never in an address, never in the page's state beyond the field.
// ApiClient is not used here because it turns a 401 and a 404 into bare variants and drops the body, and a wrong code answers
// 401 with the attempts that are left.
//
// Table of Contents:
// 1. The Worker's answers
// 2. The pure rules (tested)
// 3. The calls
// 4. The dialog
// 5. Tests
// =============================================================================

use leptos::prelude::*;
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::components::key_prompt::{tab_wraps_to, Place as FocusAt};
use crate::time_words::when_iso;
use crate::utils::after_event;

// -----------------------------------------------------------------------------
// 1. The Worker's answers
// -----------------------------------------------------------------------------

/// Which of the two flows: both are the same two steps against different routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Delete,
    Restore,
}

impl Mode {
    /// The word the Worker uses for the action in a pending request.
    pub fn action(self) -> &'static str {
        match self {
            Mode::Delete => "delete",
            Mode::Restore => "restore",
        }
    }

    fn title_verb(self) -> &'static str {
        match self {
            Mode::Delete => "Delete",
            Mode::Restore => "Restore",
        }
    }
}

/// Where a deleted listing stands, as the Projects list and the status route both say it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DeletionView {
    /// `soft` (hidden, restorable), `hard_deleting` or `hard_deleted`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub soft_deleted_at: Option<String>,
    #[serde(default)]
    pub hard_delete_after: Option<String>,
    /// Null once the window has closed or removal began.
    #[serde(default)]
    pub restore_until: Option<String>,
    /// Sent by the status route only; the Projects list leaves it out and the card works it out from the dates.
    #[serde(default)]
    pub removal_in_progress: bool,
}

/// A request that was started and not finished.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Pending {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub requested_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub attempts_left: Option<u32>,
}

/// Why nothing can start.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Blocked {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct EmailCode {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub sent_to: Option<String>,
    #[serde(default)]
    pub expires_minutes: Option<u32>,
    #[serde(default)]
    pub attempts: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Products {
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub on_sale: u32,
}

/// What a deletion takes away.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Removes {
    #[serde(default)]
    pub listing: bool,
    #[serde(default)]
    pub gallery_card: bool,
    #[serde(default)]
    pub play_and_join_links: bool,
    /// A flag today; a count in the design document. Either is read.
    #[serde(default)]
    pub live_sessions: Option<serde_json::Value>,
    #[serde(default)]
    pub website_namespace: Option<String>,
    #[serde(default)]
    pub products: Option<Products>,
    #[serde(default)]
    pub spaces: Vec<String>,
}

fn default_days() -> u32 {
    90
}

/// `GET /api/account/spaces/{id}/deletion`
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct DeletionStatus {
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub deletion: Option<DeletionView>,
    #[serde(default)]
    pub pending: Option<Pending>,
    #[serde(default)]
    pub can_delete: bool,
    #[serde(default)]
    pub can_restore: bool,
    #[serde(default)]
    pub blocked: Option<Blocked>,
    #[serde(default)]
    pub email_code: Option<EmailCode>,
    #[serde(default = "default_days")]
    pub soft_delete_days: u32,
    #[serde(default)]
    pub removes: Option<Removes>,
    #[serde(default)]
    pub keeps: Vec<String>,
    #[serde(default)]
    pub outside_our_reach: Vec<String>,
}

/// The 200 of `delete-request` and `restore-request`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RequestReply {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub sent_to: Option<String>,
    #[serde(default)]
    pub attempts_left: Option<u32>,
}

/// The 200 of `delete-confirm` and `restore-confirm`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ConfirmReply {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub hard_delete_after: Option<String>,
    #[serde(default)]
    pub restore_until: Option<String>,
    #[serde(default)]
    pub restored_at: Option<String>,
}

/// A refusal, read from the status and the body the Worker sent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refusal {
    /// 0 when no answer came.
    pub status: u16,
    pub code: Option<String>,
    /// The Worker's own sentence.
    pub error: Option<String>,
    pub attempts_left: Option<u32>,
    pub locked: bool,
    pub until: Option<String>,
    pub retry_after: Option<u64>,
}

/// Read a refusal from the status and the body text.
pub fn refusal_of(status: u16, body: &str) -> Refusal {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let text = |key: &str| v.get(key).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Refusal {
        status,
        code: text("code"),
        error: text("error"),
        attempts_left: v.get("attempts_left").and_then(|x| x.as_u64()).map(|n| n as u32),
        locked: v.get("locked").and_then(|x| x.as_bool()).unwrap_or(false),
        until: text("until"),
        retry_after: v.get("retry_after").and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64))),
    }
}

// -----------------------------------------------------------------------------
// 2. The pure rules
// -----------------------------------------------------------------------------

/// A typed name the way the Worker reads it: composed (NFC) and trimmed.
pub fn normalized_name(typed: &str) -> String {
    typed.nfc().collect::<String>().trim().to_string()
}

/// Whether the typed text is the Space's name: equal after NFC and trimming, case counted, never for an empty name.
pub fn name_matches(typed: &str, name: &str) -> bool {
    let want = normalized_name(name);
    !want.is_empty() && normalized_name(typed) == want
}

/// Only the digits of what was typed or pasted, at most six.
pub fn digits_only(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_digit()).take(6).collect()
}

/// Whether the field holds a whole code.
pub fn code_ready(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// "1 attempt left", "4 attempts left".
pub fn attempts_words(n: u32) -> String {
    if n == 1 { "1 attempt left".to_string() } else { format!("{n} attempts left") }
}

/// A wait the Worker asked for, in words.
pub fn wait_words(secs: u64) -> String {
    if secs <= 60 {
        "a minute".to_string()
    } else if secs < 3600 {
        format!("{} minutes", secs.div_ceil(60))
    } else {
        let hours = secs.div_ceil(3600);
        if hours == 1 { "an hour".to_string() } else { format!("{hours} hours") }
    }
}

/// What the dialog says where deleting or restoring is switched off.
pub fn unavailable_sentence(mode: Mode) -> &'static str {
    match mode {
        Mode::Delete => "Deleting Spaces isn't available yet.",
        Mode::Restore => "Restoring Spaces isn't available yet.",
    }
}

/// Whether the Worker said the feature is off.
pub fn is_unavailable(code: Option<&str>) -> bool {
    code == Some("delete_unavailable")
}

/// Every code the Worker can answer with on these routes. Each has a sentence of its own below.
pub const ERROR_CODES: [&str; 20] = [
    "name_mismatch",
    "request_invalid",
    "code_invalid",
    "request_locked",
    "request_used",
    "name_changed",
    "already_deleted",
    "not_deleted",
    "in_review",
    "products_on_sale",
    "refund_window_open",
    "no_verified_email",
    "request_expired",
    "removal_started",
    "restore_window_closed",
    "account_frozen",
    "rate_limited",
    "delete_unavailable",
    "account_unavailable",
    "code_not_sent",
];

/// The sentence for a code when the Worker sent none of its own.
fn code_sentence(code: &str, mode: Mode, until: Option<&str>) -> Option<String> {
    let when = until.filter(|u| !u.is_empty()).map(when_iso);
    Some(match code {
        "name_mismatch" => "The name you typed does not match this Space's name.".to_string(),
        "request_invalid" => "This confirmation is not valid. Start over to get a new code.".to_string(),
        "code_invalid" => "That code is not right.".to_string(),
        "request_locked" => "Too many wrong codes. Start over to get a new code.".to_string(),
        "request_used" => "That code was already used. Start over to get a new one.".to_string(),
        "name_changed" => "This Space was renamed after the code was sent. Start over to get a new code.".to_string(),
        "already_deleted" => "This Space is already deleted.".to_string(),
        "not_deleted" => "This Space is not deleted, so there is nothing to restore.".to_string(),
        "in_review" => "This listing is being reviewed and cannot be changed right now".to_string(),
        "products_on_sale" => "Take every product in this Space off sale first.".to_string(),
        "refund_window_open" => match when {
            Some(w) => format!("You can delete this Space after {w}, when the last refund window closes."),
            None => "A sale in this Space is still inside its refund window. You can delete it when the last refund window closes.".to_string(),
        },
        "no_verified_email" => "This account has no verified email address old enough to receive a code. Verify your email address in Settings; a code can be sent to it 30 days after it is verified.".to_string(),
        "request_expired" => "This code has expired. Start over to get a new one.".to_string(),
        "removal_started" => "Removal of this Space has begun. It can no longer be restored.".to_string(),
        "restore_window_closed" => "The time to restore this Space has passed.".to_string(),
        "account_frozen" => match when {
            Some(w) => format!("Deleting a Space is paused until {w}, after the identity key was replaced."),
            None => "Deleting a Space is paused for a while after the identity key was replaced.".to_string(),
        },
        "rate_limited" => "Too many deletion requests. Try again later.".to_string(),
        "delete_unavailable" => unavailable_sentence(mode).to_string(),
        "account_unavailable" => "This is temporarily unavailable. Try again in a minute.".to_string(),
        "code_not_sent" => "We could not send the code. Try again in a few minutes.".to_string(),
        "kyc_required" => crate::api::KYC_REQUIRED_SENTENCE.to_string(),
        _ => return None,
    })
}

/// What the dialog says about a refusal: the Worker's own sentence when it sent one, else the sentence for its code, else one for
/// its status. A wrong code adds the attempts that are left; a limit adds the wait; a date goes through the site's time words.
pub fn error_sentence(r: &Refusal, mode: Mode) -> String {
    let code = r.code.as_deref().unwrap_or("");
    let own = r.error.clone().filter(|e| !e.is_empty());
    match code {
        // The feature switch has one sentence, ours.
        "delete_unavailable" => unavailable_sentence(mode).to_string(),
        // The Worker writes these two dates in the account's zone; the page writes them in the reader's.
        "refund_window_open" | "account_frozen" if r.until.is_some() => code_sentence(code, mode, r.until.as_deref()).unwrap_or_default(),
        "code_invalid" => {
            let base = own.unwrap_or_else(|| code_sentence(code, mode, None).unwrap_or_default());
            match r.attempts_left {
                _ if r.locked => format!("{base} That was the last attempt. Start over to get a new code."),
                Some(0) => format!("{base} That was the last attempt. Start over to get a new code."),
                Some(n) => format!("{base} {}.", attempts_words(n)),
                None => base,
            }
        }
        "rate_limited" => {
            let base = own.unwrap_or_else(|| code_sentence(code, mode, None).unwrap_or_default());
            match r.retry_after {
                Some(secs) => format!("{base} Try again in {}.", wait_words(secs)),
                None => base,
            }
        }
        _ => {
            if let Some(sentence) = own {
                return sentence;
            }
            if let Some(sentence) = code_sentence(code, mode, r.until.as_deref()) {
                return sentence;
            }
            match r.status {
                401 => "Your sign-in has ended. Sign in again, then try once more.".to_string(),
                404 => "This Space was not found. It may already be removed.".to_string(),
                0 => "Could not reach Eustress. Check your connection and try again.".to_string(),
                500..=599 => "Try again in a minute. Nothing was changed.".to_string(),
                _ => "That did not work. Nothing was changed.".to_string(),
            }
        }
    }
}

/// What the dialog does after a refusal at the code step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// The same code request is still good: try again.
    Retry,
    /// The request is spent, locked, expired or wrong: begin again.
    StartOver,
}

pub fn recovery(r: &Refusal) -> Recovery {
    match r.code.as_deref() {
        Some("code_invalid") if !r.locked && r.attempts_left != Some(0) => Recovery::Retry,
        Some("rate_limited" | "account_unavailable" | "code_not_sent") => Recovery::Retry,
        None if r.status == 0 || r.status >= 500 => Recovery::Retry,
        _ => Recovery::StartOver,
    }
}

/// The sentence for a `blocked` answer of the status route.
pub fn blocked_sentence(b: &Blocked, mode: Mode) -> String {
    error_sentence(&Refusal { status: 409, code: Some(b.code.clone()), error: b.error.clone(), until: b.until.clone(), ..Refusal::default() }, mode)
}

/// The lines under "What is removed", each from a fact the Worker sent.
pub fn removes_lines(r: &Removes) -> Vec<String> {
    let mut out = Vec::new();
    if r.listing {
        out.push("The listing and its page".to_string());
    }
    if r.gallery_card {
        out.push("Its Gallery card".to_string());
    }
    if r.play_and_join_links {
        out.push("Its play and join links".to_string());
    }
    let live = match &r.live_sessions {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) > 0,
        _ => false,
    };
    if live {
        out.push("Live sessions end".to_string());
    }
    if let Some(ns) = r.website_namespace.as_deref().filter(|n| !n.is_empty()) {
        out.push(format!("Its website at {ns}"));
    }
    if let Some(p) = r.products.as_ref().filter(|p| p.total > 0) {
        let on_sale = if p.on_sale > 0 { format!(", {} on sale", p.on_sale) } else { String::new() };
        out.push(format!("{} {}{}", p.total, if p.total == 1 { "product" } else { "products" }, on_sale));
    }
    if !r.spaces.is_empty() {
        out.push(format!("{}: {}", if r.spaces.len() == 1 { "Space" } else { "Spaces" }, r.spaces.join(", ")));
    }
    out
}

/// The name the dialog compares against: the Worker's, else the card's.
pub fn name_of(status: &DeletionStatus, fallback: &str) -> String {
    status.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| fallback.to_string())
}

/// Whether an earlier request of the same kind is still open and can be answered with the code already sent.
pub fn resumable(pending: Option<&Pending>, mode: Mode, now_ms: f64) -> bool {
    let Some(p) = pending else { return false };
    if p.status != "pending" || p.action != mode.action() || p.request_id.is_empty() {
        return false;
    }
    if p.attempts_left == Some(0) {
        return false;
    }
    match p.expires_at.as_deref().and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok()) {
        Some(t) => (t.timestamp_millis() as f64) > now_ms,
        None => false,
    }
}

/// What a card on /projects shows for a deleted Space.
#[derive(Clone, Debug, PartialEq)]
pub enum DeletedCard {
    /// Hidden, and the owner can still restore it until this time (ISO 8601).
    Deleting { restore_until: String },
    /// The window closed or the removal began. No button.
    RemovalInProgress,
}

/// A deleted listing's card state. Anything that is not clearly inside the restore window reads as removal in progress, so a
/// card never offers a restore the Worker would refuse.
pub fn deleted_card(d: Option<&DeletionView>, now_ms: f64) -> DeletedCard {
    let Some(d) = d else { return DeletedCard::RemovalInProgress };
    if d.removal_in_progress || d.state != "soft" {
        return DeletedCard::RemovalInProgress;
    }
    let until = d.restore_until.as_deref().filter(|u| !u.is_empty());
    match until.and_then(|u| chrono::DateTime::parse_from_rfc3339(u).ok()) {
        Some(t) if (t.timestamp_millis() as f64) > now_ms => DeletedCard::Deleting { restore_until: until.unwrap_or_default().to_string() },
        _ => DeletedCard::RemovalInProgress,
    }
}

/// The words on a card for each state.
pub fn deleted_card_words(card: &DeletedCard) -> (&'static str, String) {
    match card {
        DeletedCard::Deleting { restore_until } => ("Deleting", format!("Hidden from everyone. You can restore it until {}.", when_iso(restore_until))),
        DeletedCard::RemovalInProgress => ("Removal is in progress", "This Space can no longer be restored.".to_string()),
    }
}

/// Whether a card offers Delete: every Space that is not already deleted. The Worker decides the rest and says why when it
/// refuses.
pub fn can_offer_delete(deleted: bool) -> bool {
    !deleted
}

/// The line the dialog shows once it is done.
pub fn done_message(mode: Mode, name: &str, reply: &ConfirmReply) -> String {
    match mode {
        Mode::Delete => {
            let hard = reply.hard_delete_after.as_deref().filter(|d| !d.is_empty());
            let restore = reply.restore_until.as_deref().filter(|d| !d.is_empty());
            match (hard, restore) {
                (Some(h), Some(r)) if h == r => format!("{name} is deleted. It is hidden from everyone now and will be permanently removed on {}. You can restore it from Projects until then.", when_iso(h)),
                (Some(h), Some(r)) => format!("{name} is deleted. It is hidden from everyone now and will be permanently removed on {}. You can restore it from Projects until {}.", when_iso(h), when_iso(r)),
                (Some(h), None) => format!("{name} is deleted. It is hidden from everyone now and will be permanently removed on {}.", when_iso(h)),
                _ => format!("{name} is deleted. It is hidden from everyone now."),
            }
        }
        Mode::Restore => format!("{name} is restored. It is back as it was before it was deleted."),
    }
}

// -----------------------------------------------------------------------------
// 3. The calls
// -----------------------------------------------------------------------------

/// One request to the Worker.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub method: &'static str,
    pub path: String,
    pub body: Option<serde_json::Value>,
}

fn base_path(sim_id: &str) -> String {
    format!("/api/account/spaces/{}", urlencoding::encode(sim_id))
}

pub fn status_call(sim_id: &str) -> Call {
    Call { method: "GET", path: format!("{}/deletion", base_path(sim_id)), body: None }
}

pub fn request_call(sim_id: &str, mode: Mode, typed: &str) -> Call {
    Call { method: "POST", path: format!("{}/{}-request", base_path(sim_id), mode.action()), body: Some(serde_json::json!({ "typed_name": normalized_name(typed) })) }
}

/// The confirm. The code is in the body and nowhere in the path.
pub fn confirm_call(sim_id: &str, mode: Mode, request_id: &str, typed: &str, code: &str) -> Call {
    Call {
        method: "POST",
        path: format!("{}/{}-confirm", base_path(sim_id), mode.action()),
        body: Some(serde_json::json!({ "request_id": request_id, "typed_name": normalized_name(typed), "code": code })),
    }
}

/// Send a call and give back the status and the body text, whatever the status. `Err` is no answer at all.
async fn send(api_url: &str, call: Call) -> Result<(u16, String), String> {
    use gloo_net::http::Request;
    use gloo_storage::Storage;
    let url = format!("{}{}", api_url, call.path);
    let mut req = if call.method == "GET" { Request::get(&url) } else { Request::post(&url) };
    if let Ok(token) = gloo_storage::LocalStorage::get::<String>("auth_token") {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let req = req.header("Content-Type", "application/json");
    let response = match &call.body {
        Some(body) => req.json(body).map_err(|e| e.to_string())?.send().await,
        None => req.send().await,
    }
    .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    Ok((status, text))
}

// -----------------------------------------------------------------------------
// 4. The dialog
// -----------------------------------------------------------------------------

/// The code step's facts.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeStep {
    pub request_id: String,
    pub sent_to: Option<String>,
    pub expires_at: Option<String>,
    pub expires_minutes: u32,
    pub attempts_left: u32,
    /// The request cannot be answered any more; the only way on is to start over.
    pub dead: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Loading,
    LoadFailed(String),
    /// The feature is switched off: one sentence, no form.
    Unavailable,
    /// Step 1: what happens, and the name to type.
    Review(Box<DeletionStatus>),
    /// Step 2: the emailed code.
    Code(Box<CodeStep>),
    Done(String),
}

impl Step {
    fn tag(&self) -> u8 {
        match self {
            Step::Loading => 0,
            Step::LoadFailed(_) => 1,
            Step::Unavailable => 2,
            Step::Review(_) => 3,
            Step::Code(_) => 4,
            Step::Done(_) => 5,
        }
    }
}

/// The step a status read leads to: the one sentence when deleting is switched off, else the review.
pub fn step_for_status(status: DeletionStatus) -> Step {
    if status.blocked.as_ref().is_some_and(|b| is_unavailable(Some(&b.code))) {
        Step::Unavailable
    } else {
        Step::Review(Box::new(status))
    }
}

/// The step a refused read leads to.
pub fn step_for_refusal(r: &Refusal, mode: Mode) -> Step {
    if is_unavailable(r.code.as_deref()) { Step::Unavailable } else { Step::LoadFailed(error_sentence(r, mode)) }
}

#[cfg(all(not(feature = "ssr"), target_arch = "wasm32"))]
fn active_element() -> Option<web_sys::HtmlElement> {
    use wasm_bindgen::JsCast;
    web_sys::window()?.document()?.active_element()?.dyn_into::<web_sys::HtmlElement>().ok()
}

#[cfg(not(all(not(feature = "ssr"), target_arch = "wasm32")))]
fn active_element() -> Option<web_sys::HtmlElement> {
    None
}

/// The dialog for deleting a Space or restoring one. `preloaded` is for tests: it skips the status read.
#[component]
pub fn DeleteSpaceModal(
    #[prop(into)] sim_id: String,
    #[prop(into)] fallback_name: String,
    mode: Mode,
    #[prop(into)] api_url: String,
    on_close: Callback<()>,
    /// Called once the Worker has done it, so the page can read its list again.
    on_done: Callback<()>,
    #[prop(optional)] preloaded: Option<DeletionStatus>,
) -> impl IntoView {
    let step = RwSignal::new(match preloaded.clone() {
        Some(status) => step_for_status(status),
        None => Step::Loading,
    });
    let typed = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let dialog = NodeRef::<leptos::html::Div>::new();
    let name_input = NodeRef::<leptos::html::Input>::new();
    let code_input = NodeRef::<leptos::html::Input>::new();

    // Focus comes back to what opened the dialog.
    let opener = StoredValue::new_local(active_element());
    on_cleanup(move || {
        opener.with_value(|o| {
            if let Some(el) = o {
                let _ = el.focus();
            }
        });
    });

    let verb = mode.title_verb();
    let sim_for_load = sim_id.clone();
    let api_for_load = api_url.clone();
    let load = Callback::new(move |_: ()| {
        step.set(Step::Loading);
        let sim = sim_for_load.clone();
        let api = api_for_load.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match send(&api, status_call(&sim)).await {
                Ok((s, body)) if (200..300).contains(&s) => match serde_json::from_str::<DeletionStatus>(&body) {
                    Ok(status) => step.set(step_for_status(status)),
                    Err(_) => step.set(Step::LoadFailed("Eustress sent an answer this page could not read. Try again in a minute.".to_string())),
                },
                Ok((s, body)) => step.set(step_for_refusal(&refusal_of(s, &body), mode)),
                Err(_) => step.set(step_for_refusal(&Refusal::default(), mode)),
            }
        });
    });
    if preloaded.is_none() {
        Effect::new(move |_| load.run(()));
    }

    // Focus moves to the field a step is about, or to the dialog.
    Effect::new(move |_| {
        let tag = step.with(Step::tag);
        after_event(move || match tag {
            3 => {
                if let Some(input) = name_input.get_untracked() {
                    let _ = input.focus();
                } else if let Some(d) = dialog.get_untracked() {
                    let _ = d.focus();
                }
            }
            4 => {
                if let Some(input) = code_input.get_untracked() {
                    let _ = input.focus();
                }
            }
            _ => {
                if let Some(d) = dialog.get_untracked() {
                    let _ = d.focus();
                }
            }
        });
    });

    let fallback = StoredValue::new(fallback_name);
    let shown_name = move || match step.get() {
        Step::Review(status) => name_of(&status, &fallback.get_value()),
        _ => fallback.get_value(),
    };

    // Step 1 to step 2: ask for the code.
    let sim_for_request = sim_id.clone();
    let api_for_request = api_url.clone();
    let ask = Callback::new(move |_: ()| {
        let Step::Review(status) = step.get_untracked() else { return };
        if busy.get_untracked() || !name_matches(&typed.get_untracked(), &name_of(&status, &fallback.get_value())) {
            return;
        }
        busy.set(true);
        problem.set(None);
        let sim = sim_for_request.clone();
        let api = api_for_request.clone();
        let typed_now = typed.get_untracked();
        wasm_bindgen_futures::spawn_local(async move {
            let answer = send(&api, request_call(&sim, mode, &typed_now)).await;
            busy.set(false);
            match answer {
                Ok((s, body)) if (200..300).contains(&s) => match serde_json::from_str::<RequestReply>(&body) {
                    Ok(reply) if !reply.request_id.is_empty() => {
                        code.set(String::new());
                        let ec = status.email_code.clone().unwrap_or_default();
                        step.set(Step::Code(Box::new(CodeStep {
                            request_id: reply.request_id,
                            sent_to: reply.sent_to.or(ec.sent_to),
                            expires_at: reply.expires_at,
                            expires_minutes: ec.expires_minutes.unwrap_or(15),
                            attempts_left: reply.attempts_left.or(ec.attempts).unwrap_or(5),
                            dead: false,
                        })));
                    }
                    _ => problem.set(Some("Eustress sent an answer this page could not read. Nothing was changed.".to_string())),
                },
                Ok((s, body)) => {
                    let r = refusal_of(s, &body);
                    if is_unavailable(r.code.as_deref()) {
                        step.set(Step::Unavailable);
                    } else {
                        problem.set(Some(error_sentence(&r, mode)));
                    }
                }
                Err(_) => problem.set(Some(error_sentence(&Refusal::default(), mode))),
            }
        });
    });

    // The code already sent by an earlier request: go straight to step 2.
    let resume = Callback::new(move |_: ()| {
        let Step::Review(status) = step.get_untracked() else { return };
        if !name_matches(&typed.get_untracked(), &name_of(&status, &fallback.get_value())) {
            return;
        }
        let Some(p) = status.pending.clone().filter(|p| resumable(Some(p), mode, crate::components::play_chooser::now_ms())) else { return };
        let ec = status.email_code.clone().unwrap_or_default();
        problem.set(None);
        code.set(String::new());
        step.set(Step::Code(Box::new(CodeStep {
            request_id: p.request_id,
            sent_to: ec.sent_to,
            expires_at: p.expires_at,
            expires_minutes: ec.expires_minutes.unwrap_or(15),
            attempts_left: p.attempts_left.or(ec.attempts).unwrap_or(5),
            dead: false,
        })));
    });

    // Step 2: send the code.
    let sim_for_confirm = sim_id.clone();
    let api_for_confirm = api_url.clone();
    let confirm = Callback::new(move |_: ()| {
        let Step::Code(cs) = step.get_untracked() else { return };
        let typed_code = code.get_untracked();
        if busy.get_untracked() || cs.dead || !code_ready(&typed_code) {
            return;
        }
        busy.set(true);
        problem.set(None);
        let sim = sim_for_confirm.clone();
        let api = api_for_confirm.clone();
        let typed_now = typed.get_untracked();
        let name = fallback.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let answer = send(&api, confirm_call(&sim, mode, &cs.request_id, &typed_now, &typed_code)).await;
            busy.set(false);
            match answer {
                Ok((s, body)) if (200..300).contains(&s) => {
                    let reply = serde_json::from_str::<ConfirmReply>(&body).unwrap_or_default();
                    code.set(String::new());
                    step.set(Step::Done(done_message(mode, &name, &reply)));
                    on_done.run(());
                }
                Ok((s, body)) => {
                    let r = refusal_of(s, &body);
                    let mut next = (*cs).clone();
                    match recovery(&r) {
                        Recovery::Retry => {
                            if let Some(n) = r.attempts_left {
                                next.attempts_left = n;
                            }
                        }
                        Recovery::StartOver => {
                            next.dead = true;
                            next.attempts_left = r.attempts_left.unwrap_or(0);
                        }
                    }
                    code.set(String::new());
                    problem.set(Some(error_sentence(&r, mode)));
                    step.set(Step::Code(Box::new(next)));
                }
                Err(_) => problem.set(Some(error_sentence(&Refusal::default(), mode))),
            }
        });
    });

    let start_over = Callback::new(move |_: ()| {
        problem.set(None);
        code.set(String::new());
        load.run(());
    });
    // The page drops this dialog when it closes, so it closes once the click that asked has finished.
    let close = Callback::new(move |_: ()| after_event(move || on_close.run(())));

    // Escape closes. Tab stays inside: the last control is always the one marked data-sd-last (Cancel or Close), the first is the
    // first enabled control in the dialog.
    let keys = move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
        "Escape" => {
            ev.prevent_default();
            close.run(());
        }
        "Tab" => {
            let Some(root) = dialog.get_untracked() else { return };
            let last = root.query_selector("[data-sd-last]").ok().flatten();
            let first = root.query_selector("button:not([disabled]), input:not([disabled]), a[href]").ok().flatten();
            let Some(last) = last else { return };
            let active = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element());
            let root_el: &web_sys::Element = root.as_ref();
            let at = match &active {
                Some(el) if el == &last => FocusAt::Last,
                Some(el) if first.as_ref() == Some(el) => FocusAt::First,
                Some(el) if el == root_el => FocusAt::Dialog,
                _ => FocusAt::Between,
            };
            match tab_wraps_to(at, ev.shift_key()) {
                Some(FocusAt::Last) => {
                    ev.prevent_default();
                    if let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(last) {
                        let _ = el.focus();
                    }
                }
                Some(FocusAt::First) => {
                    ev.prevent_default();
                    if let Some(el) = first.and_then(|f| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(f).ok()) {
                        let _ = el.focus();
                    }
                }
                _ => {}
            }
        }
        _ => {}
    };

    let title = move || format!("{verb} {}?", shown_name());

    view! {
        <div class="modal-overlay" on:click=move |_| { if !busy.get_untracked() { close.run(()); } }>
            <div class="modal-content archive-dialog sd-dialog" role="dialog" aria-modal="true" aria-labelledby="sd-title" aria-describedby="sd-lead"
                tabindex="-1" node_ref=dialog on:keydown=keys on:click=|e| e.stop_propagation()>
                <h3 id="sd-title">{title}</h3>
                {move || match step.get() {
                    Step::Loading => view! {
                        <p id="sd-lead" role="status" aria-busy="true">"Loading what this will do..."</p>
                        <div class="modal-actions">
                            <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Cancel"</button>
                        </div>
                    }.into_any(),
                    Step::LoadFailed(why) => view! {
                        <p id="sd-lead" class="sd-problem" role="alert"><strong>"Problem. "</strong>{why}</p>
                        <div class="modal-actions">
                            <button type="button" class="btn btn-secondary" on:click=move |_| load.run(())>"Try again"</button>
                            <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Close"</button>
                        </div>
                    }.into_any(),
                    Step::Unavailable => view! {
                        <p id="sd-lead" role="status">{unavailable_sentence(mode)}</p>
                        <div class="modal-actions">
                            <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Close"</button>
                        </div>
                    }.into_any(),
                    Step::Done(message) => view! {
                        <p id="sd-lead" role="status"><strong>"Done. "</strong>{message}</p>
                        <div class="modal-actions">
                            <button type="button" class="btn btn-primary" data-sd-last="" on:click=move |_| close.run(())>"Close"</button>
                        </div>
                    }.into_any(),
                    Step::Review(status) => review_view(*status, mode, fallback.get_value(), typed, problem, busy, name_input, ask, resume, close).into_any(),
                    Step::Code(cs) => code_view(*cs, mode, code, problem, busy, code_input, confirm, start_over, close).into_any(),
                }}
            </div>
        </div>
    }
}

/// Step 1: what is removed, kept and out of reach, the promise, and the name to type.
#[allow(clippy::too_many_arguments)]
fn review_view(
    status: DeletionStatus,
    mode: Mode,
    fallback: String,
    typed: RwSignal<String>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    name_input: NodeRef<leptos::html::Input>,
    ask: Callback<()>,
    resume: Callback<()>,
    close: Callback<()>,
) -> impl IntoView {
    let name = name_of(&status, &fallback);
    let allowed = match mode {
        Mode::Delete => status.can_delete,
        Mode::Restore => status.can_restore,
    };
    let blocked = status.blocked.as_ref().map(|b| blocked_sentence(b, mode));
    // Allowed, or blocked with a reason the Worker gave. Neither: say so rather than show a form that cannot work.
    let stuck = (!allowed && blocked.is_none()).then(|| match mode {
        Mode::Delete if status.deletion.is_some() => "This Space is already deleted.".to_string(),
        Mode::Delete => "This Space cannot be deleted right now.".to_string(),
        Mode::Restore if status.deletion.as_ref().is_some_and(|d| d.removal_in_progress || d.state != "soft") => "Removal of this Space has begun. It can no longer be restored.".to_string(),
        Mode::Restore => "This Space cannot be restored right now.".to_string(),
    });
    let lead = match mode {
        Mode::Delete => format!(
            "It is hidden from everyone at once. You can restore it from Projects for {} days; after that it is removed for good.",
            status.soft_delete_days
        ),
        Mode::Restore => {
            let until = status.deletion.as_ref().and_then(|d| d.restore_until.clone()).filter(|u| !u.is_empty());
            match until {
                Some(u) => format!("It comes back as it was before it was deleted. You can restore it until {}.", when_iso(&u)),
                None => "It comes back as it was before it was deleted.".to_string(),
            }
        }
    };
    let removes = status.removes.as_ref().map(removes_lines).unwrap_or_default();
    let keeps = status.keeps.clone();
    let reach = status.outside_our_reach.clone();
    let email_note = status.email_code.as_ref().filter(|e| e.available).and_then(|e| e.sent_to.clone()).map(|to| format!("To confirm, Eustress emails a 6-digit code to {to}."));
    let can_resume = allowed && resumable(status.pending.as_ref(), mode, crate::components::play_chooser::now_ms());
    let name_for_match = name.clone();
    let name_for_show = name.clone();
    let matches = move || name_matches(&typed.get(), &name_for_match);
    let matches_a = matches.clone();
    let matches_b = matches.clone();
    let matches_c = matches.clone();
    let verb = mode.title_verb();
    let heads_removed = mode == Mode::Delete;

    view! {
        <p id="sd-lead">{lead}</p>
        {blocked.map(|b| view! { <p class="sd-problem" role="alert"><strong>"Not possible now. "</strong>{b}</p> })}
        {stuck.map(|b| view! { <p class="sd-problem" role="alert"><strong>"Not possible now. "</strong>{b}</p> })}
        {(heads_removed && !removes.is_empty()).then(|| view! {
            <h4>"What is removed"</h4>
            <ul class="sd-list">{removes.into_iter().map(|l| view! { <li>{l}</li> }).collect_view()}</ul>
        })}
        {(heads_removed && !keeps.is_empty()).then(|| view! {
            <h4>"What is kept"</h4>
            <ul class="sd-list">{keeps.into_iter().map(|l| view! { <li>{l}</li> }).collect_view()}</ul>
        })}
        {(heads_removed && !reach.is_empty()).then(|| view! {
            <h4>"What Eustress cannot reach"</h4>
            <ul class="sd-list">{reach.into_iter().map(|l| view! { <li>{l}</li> }).collect_view()}</ul>
        })}
        {allowed.then(|| view! {
            <form class="sd-form" on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); ask.run(()); }>
                <label for="sd-name" class="form-label">"Type the Space's name to confirm"</label>
                <code class="sd-name">{name_for_show}</code>
                <input id="sd-name" class="form-input sd-input" type="text" autocomplete="off" autocapitalize="off" spellcheck="false"
                    aria-describedby="sd-match" node_ref=name_input
                    prop:value=move || typed.get()
                    on:input=move |e| typed.set(event_target_value(&e)) />
                <p id="sd-match" class="sd-hint" role="status" aria-live="polite">
                    {move || if typed.get().trim().is_empty() { "Capital letters count.".to_string() } else if matches_a() { "The name matches.".to_string() } else { "The name does not match yet. Capital letters count.".to_string() }}
                </p>
                {email_note.map(|n| view! { <p class="sd-hint">{n}</p> })}
                {move || problem.get().map(|p| view! { <p class="sd-problem" role="alert"><strong>"Problem. "</strong>{p}</p> })}
                <div class="modal-actions">
                    {can_resume.then(|| view! {
                        <button type="button" class="btn btn-secondary" disabled=move || !matches_b() || busy.get() on:click=move |_| resume.run(())>"I already have a code"</button>
                    })}
                    <button type="submit" class=if mode == Mode::Delete { "btn btn-danger" } else { "btn btn-primary" } disabled=move || !matches_c() || busy.get()>
                        {move || if busy.get() { "Sending the code...".to_string() } else { format!("{verb} Space") }}
                    </button>
                    <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Cancel"</button>
                </div>
            </form>
        })}
        {(!allowed).then(|| view! {
            <div class="modal-actions">
                <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Close"</button>
            </div>
        })}
    }
}

/// Step 2: the emailed code.
#[allow(clippy::too_many_arguments)]
fn code_view(
    cs: CodeStep,
    mode: Mode,
    code: RwSignal<String>,
    problem: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    code_input: NodeRef<leptos::html::Input>,
    confirm: Callback<()>,
    start_over: Callback<()>,
    close: Callback<()>,
) -> impl IntoView {
    let verb = mode.title_verb();
    let sent = match &cs.sent_to {
        Some(to) => format!("We sent a code to {to}."),
        None => "We sent you a code.".to_string(),
    };
    let expiry = match cs.expires_at.as_deref().filter(|e| !e.is_empty()) {
        Some(at) => format!("It works for {} minutes, until {}.", cs.expires_minutes, when_iso(at)),
        None => format!("It works for {} minutes.", cs.expires_minutes),
    };
    let attempts = if cs.dead { "This code can no longer be used.".to_string() } else { format!("{}.", attempts_words(cs.attempts_left)) };
    let lead = format!("{sent} {expiry} {attempts}");
    let dead = cs.dead;
    view! {
        <p id="sd-lead">{lead}</p>
        <form class="sd-form" on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); confirm.run(()); }>
            <label for="sd-code" class="form-label">"6-digit code"</label>
            <input id="sd-code" class="form-input sd-input sd-code" type="text" inputmode="numeric" autocomplete="one-time-code" maxlength="6"
                pattern="[0-9]{6}" spellcheck="false" aria-describedby="sd-lead" disabled=dead node_ref=code_input
                prop:value=move || code.get()
                on:input=move |e| code.set(digits_only(&event_target_value(&e))) />
            {move || problem.get().map(|p| view! { <p class="sd-problem" role="alert"><strong>"Problem. "</strong>{p}</p> })}
            <div class="modal-actions">
                <button type="submit" class=if mode == Mode::Delete { "btn btn-danger" } else { "btn btn-primary" }
                    disabled=move || dead || busy.get() || !code_ready(&code.get())>
                    {move || if busy.get() { "Checking...".to_string() } else { format!("{verb} Space") }}
                </button>
                <button type="button" class=if dead { "btn btn-primary" } else { "btn btn-secondary" } on:click=move |_| start_over.run(())>"Start over"</button>
                <button type="button" class="btn btn-secondary" data-sd-last="" on:click=move |_| close.run(())>"Cancel"</button>
            </div>
        </form>
    }
}

// -----------------------------------------------------------------------------
// 5. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = include_str!("project_delete.rs");
    const CSS: &str = include_str!("../../style/project_delete.css");
    const INDEX: &str = include_str!("../../index.html");

    /// The code of this file, without these tests.
    fn source() -> &'static str {
        FILE.split(concat!("#[cfg", "(test)]")).next().unwrap()
    }

    fn status_json() -> &'static str {
        r#"{"sim_id":"abc","name":"The Super Station","state":"public","deletion":null,"pending":null,"can_delete":true,"can_restore":false,
            "blocked":null,"email_code":{"available":true,"sent_to":"o***@example.com","expires_minutes":15,"attempts":5},"soft_delete_days":90,
            "removes":{"listing":true,"gallery_card":true,"play_and_join_links":true,"live_sessions":true,"website_namespace":"superstation",
                       "products":{"total":3,"on_sale":0},"spaces":["Main","Hangar"]},
            "keeps":["Receipts and sales records"],"outside_our_reach":["Copies players already downloaded"]}"#
    }

    fn status() -> DeletionStatus {
        serde_json::from_str(status_json()).unwrap()
    }

    fn html_of(status: DeletionStatus, mode: Mode) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            view! {
                <DeleteSpaceModal sim_id="abc" fallback_name="The Super Station" mode=mode api_url="https://api.example"
                    on_close=Callback::new(|_| ()) on_done=Callback::new(|_| ()) preloaded=status />
            }
            .to_html()
        })
    }

    #[test]
    fn delete_space_name_rule_is_exact_nfc_trimmed_and_case_sensitive() {
        assert!(name_matches("The Super Station", "The Super Station"));
        // Surrounding space is trimmed on both sides.
        assert!(name_matches("  The Super Station \n", "The Super Station"));
        assert!(name_matches("The Super Station", "  The Super Station  "));
        // Case counts.
        assert!(!name_matches("the super station", "The Super Station"));
        assert!(!name_matches("THE SUPER STATION", "The Super Station"));
        // Inner spacing counts; a prefix or a suffix is not the name.
        assert!(!name_matches("The  Super Station", "The Super Station"));
        assert!(!name_matches("The Super", "The Super Station"));
        assert!(!name_matches("The Super Station 2", "The Super Station"));
        // A composed and a decomposed e-acute are the same name.
        assert!(name_matches("Caf\u{e9} Racer", "Cafe\u{301} Racer"));
        assert!(name_matches("Cafe\u{301} Racer", "Caf\u{e9} Racer"));
        // An empty or blank name is never matched, not even by empty text.
        assert!(!name_matches("", ""));
        assert!(!name_matches("   ", "   "));
        assert_eq!(normalized_name("  Cafe\u{301} "), "Caf\u{e9}");
    }

    #[test]
    fn delete_space_every_code_has_its_own_sentence_and_the_workers_own_is_used_when_sent() {
        let generic = error_sentence(&Refusal { status: 409, code: Some("never_heard_of_it".to_string()), ..Refusal::default() }, Mode::Delete);
        let mut seen = std::collections::HashSet::new();
        for code in ERROR_CODES {
            let r = Refusal { status: 409, code: Some(code.to_string()), ..Refusal::default() };
            let s = error_sentence(&r, Mode::Delete);
            assert!(!s.is_empty() && s != generic, "{code} has no sentence of its own: {s}");
            seen.insert(s);
            // The Worker's sentence wins over ours (the date and count cases below add to it).
            if !["delete_unavailable", "code_invalid", "rate_limited"].contains(&code) {
                let own = Refusal { status: 409, code: Some(code.to_string()), error: Some("From the Worker.".to_string()), ..Refusal::default() };
                assert_eq!(error_sentence(&own, Mode::Delete), "From the Worker.", "{code}");
            }
        }
        assert_eq!(seen.len(), ERROR_CODES.len(), "every code has a sentence of its own: {seen:?}");
        // By status when there is no code.
        assert!(error_sentence(&refusal_of(401, ""), Mode::Delete).contains("sign-in"));
        assert!(error_sentence(&refusal_of(404, "{}"), Mode::Delete).contains("not found"));
        assert!(error_sentence(&refusal_of(503, "not json"), Mode::Delete).contains("Try again in a minute"));
        assert!(error_sentence(&Refusal::default(), Mode::Delete).contains("Could not reach Eustress"));
        // The feature switch has one sentence, whatever the Worker wrote.
        let off = refusal_of(503, r#"{"error":"Deleting or restoring a published Space is not available yet.","code":"delete_unavailable"}"#);
        assert_eq!(error_sentence(&off, Mode::Delete), "Deleting Spaces isn't available yet.");
        assert!(is_unavailable(off.code.as_deref()) && !is_unavailable(Some("in_review")) && !is_unavailable(None));
    }

    #[test]
    fn delete_space_a_wrong_code_says_how_many_attempts_are_left_and_the_last_one_says_start_over() {
        let four = refusal_of(401, r#"{"error":"That code is not right.","code":"code_invalid","attempts_left":4}"#);
        assert_eq!(error_sentence(&four, Mode::Delete), "That code is not right. 4 attempts left.");
        let one = refusal_of(401, r#"{"error":"That code is not right.","code":"code_invalid","attempts_left":1}"#);
        assert_eq!(error_sentence(&one, Mode::Delete), "That code is not right. 1 attempt left.");
        let last = refusal_of(401, r#"{"error":"That code is not right.","code":"code_invalid","attempts_left":0,"locked":true}"#);
        let sentence = error_sentence(&last, Mode::Delete);
        assert!(sentence.contains("last attempt") && sentence.contains("Start over"), "{sentence}");
        // Retry while attempts are left; start over when locked or none are left.
        assert_eq!(recovery(&four), Recovery::Retry);
        assert_eq!(recovery(&last), Recovery::StartOver);
        assert_eq!(recovery(&refusal_of(401, r#"{"code":"code_invalid","attempts_left":0}"#)), Recovery::StartOver);
    }

    #[test]
    fn delete_space_lock_expiry_and_spent_requests_start_over_and_busy_answers_retry() {
        for code in ["request_locked", "request_expired", "request_invalid", "request_used", "name_changed", "in_review", "already_deleted", "products_on_sale"] {
            assert_eq!(recovery(&Refusal { status: 409, code: Some(code.to_string()), ..Refusal::default() }), Recovery::StartOver, "{code}");
        }
        for code in ["rate_limited", "account_unavailable", "code_not_sent"] {
            assert_eq!(recovery(&Refusal { status: 503, code: Some(code.to_string()), ..Refusal::default() }), Recovery::Retry, "{code}");
        }
        assert_eq!(recovery(&Refusal::default()), Recovery::Retry, "no answer at all: try again");
        // A limit says how long to wait.
        let limited = refusal_of(429, r#"{"error":"Too many deletion requests. Try again later.","code":"rate_limited","retry_after":600}"#);
        assert_eq!(error_sentence(&limited, Mode::Delete), "Too many deletion requests. Try again later. Try again in 10 minutes.");
        assert_eq!(wait_words(30), "a minute");
        assert_eq!(wait_words(3600), "an hour");
        assert_eq!(wait_words(7300), "3 hours");
    }

    #[test]
    fn delete_space_dates_in_a_refusal_go_through_the_time_words() {
        // 2026-12-12T20:06:00Z, written in UTC without a browser.
        let refund = refusal_of(409, r#"{"error":"You can delete this Space after Dec 12, 1:06 PM MST, when the last refund window closes.","code":"refund_window_open","until":"2026-12-12T20:06:00.000Z"}"#);
        assert_eq!(error_sentence(&refund, Mode::Delete), "You can delete this Space after Dec 12, 2026, 8:06 PM UTC, when the last refund window closes.");
        let frozen = refusal_of(423, r#"{"code":"account_frozen","until":"2026-12-12T20:06:00.000Z"}"#);
        assert_eq!(error_sentence(&frozen, Mode::Delete), "Deleting a Space is paused until Dec 12, 2026, 8:06 PM UTC, after the identity key was replaced.");
        let blocked = Blocked { code: "refund_window_open".to_string(), error: Some("x".to_string()), until: Some("2026-12-12T20:06:00.000Z".to_string()) };
        assert!(blocked_sentence(&blocked, Mode::Delete).contains("Dec 12, 2026, 8:06 PM UTC"));
        // A blocked reason without a date keeps the Worker's sentence.
        let held = Blocked { code: "in_review".to_string(), error: Some("This listing is being reviewed and cannot be changed right now".to_string()), until: None };
        assert_eq!(blocked_sentence(&held, Mode::Delete), "This listing is being reviewed and cannot be changed right now");
    }

    #[test]
    fn delete_space_card_states_deleting_with_a_deadline_or_removal_in_progress() {
        let now = 1_793_481_240_000.0; // 2026-10-31T21:14:00Z
        let soft = DeletionView { state: "soft".to_string(), soft_deleted_at: Some("2026-10-31T21:00:00Z".to_string()), hard_delete_after: Some("2027-01-29T21:00:00Z".to_string()), restore_until: Some("2027-01-29T21:00:00Z".to_string()), removal_in_progress: false };
        let card = deleted_card(Some(&soft), now);
        assert_eq!(card, DeletedCard::Deleting { restore_until: "2027-01-29T21:00:00Z".to_string() });
        let (head, line) = deleted_card_words(&card);
        assert_eq!(head, "Deleting");
        assert_eq!(line, "Hidden from everyone. You can restore it until Jan 29, 2027, 9:00 PM UTC.");
        // The Worker says removal is in progress; the window is closed; removal began; there is no deletion object at all.
        let flagged = DeletionView { removal_in_progress: true, ..soft.clone() };
        let closed = DeletionView { restore_until: None, ..soft.clone() };
        let past = DeletionView { restore_until: Some("2026-10-01T00:00:00Z".to_string()), ..soft.clone() };
        let hard = DeletionView { state: "hard_deleting".to_string(), ..soft.clone() };
        for (what, d) in [("flagged", Some(&flagged)), ("closed", Some(&closed)), ("past", Some(&past)), ("hard", Some(&hard)), ("none", None)] {
            let card = deleted_card(d, now);
            assert_eq!(card, DeletedCard::RemovalInProgress, "{what}");
            assert_eq!(deleted_card_words(&card).0, "Removal is in progress");
        }
        assert!(can_offer_delete(false) && !can_offer_delete(true));
    }

    #[test]
    fn delete_space_the_code_is_in_the_body_and_never_in_any_address() {
        let id = "0a1b2c3d-1111-2222-3333-444455556666";
        let code = "492817";
        let confirm = confirm_call(id, Mode::Delete, "sdr_abc", "The Super Station", code);
        assert_eq!(confirm.path, format!("/api/account/spaces/{id}/delete-confirm"));
        assert!(!confirm.path.contains(code) && !confirm.path.contains('?'), "{}", confirm.path);
        assert_eq!(confirm.body.as_ref().unwrap()["code"], code);
        assert_eq!(confirm.body.as_ref().unwrap()["request_id"], "sdr_abc");
        assert_eq!(confirm.body.as_ref().unwrap()["typed_name"], "The Super Station");
        let restore = confirm_call(id, Mode::Restore, "sdr_abc", "x", code);
        assert_eq!(restore.path, format!("/api/account/spaces/{id}/restore-confirm"));
        assert!(!restore.path.contains(code));
        // Every call the page can make, over a name with spaces and the code: no address carries the name or the code.
        let calls = [status_call(id), request_call(id, Mode::Delete, "My Space & Co"), request_call(id, Mode::Restore, "My Space & Co"), confirm, restore];
        for call in &calls {
            assert!(call.path.starts_with("/api/account/spaces/"), "{}", call.path);
            assert!(!call.path.contains(code) && !call.path.contains("Space") && !call.path.contains("code") && !call.path.contains('?'), "{}", call.path);
        }
        assert_eq!(status_call(id).method, "GET");
        assert!(status_call(id).body.is_none());
        assert_eq!(request_call(id, Mode::Delete, " Name ").body.unwrap()["typed_name"], "Name");
        // The code is never put in an address by the code of this file: the only place an address is built is `base_path`.
        let src = source();
        assert_eq!(src.matches("format!(\"/api/account/spaces/").count(), 1, "the only place an address is built");
        // The needles are built here so this test's own text does not match them.
        let (query, param) = (format!("{}code=", '?'), format!("{}code=", '&'));
        assert!(!src.contains(&query) && !src.contains(&param), "no query string carries the code");
        assert!(!src.contains("set_href") && !src.contains("pushState") && !src.contains("navigate("), "the code never reaches the address bar");
    }

    #[test]
    fn delete_space_removed_kept_and_out_of_reach_come_from_the_workers_answer() {
        let s = status();
        let lines = removes_lines(s.removes.as_ref().unwrap());
        for want in ["The listing and its page", "Its Gallery card", "Its play and join links", "Live sessions end", "Its website at superstation", "3 products", "Spaces: Main, Hangar"] {
            assert!(lines.iter().any(|l| l == want), "missing {want:?} in {lines:?}");
        }
        let empty = removes_lines(&Removes::default());
        assert!(empty.is_empty(), "{empty:?}");
        let html = html_of(s, Mode::Delete);
        for line in lines.iter().map(String::as_str).chain(["Receipts and sales records", "Copies players already downloaded", "What is removed", "What is kept", "What Eustress cannot reach"]) {
            assert!(html.contains(&line.replace('\'', "&#x27;")) || html.contains(line), "the dialog is missing {line:?}");
        }
        // Nothing the Worker did not send is listed.
        let mut bare = status();
        bare.keeps.clear();
        bare.outside_our_reach.clear();
        bare.removes = None;
        let html = html_of(bare, Mode::Delete);
        assert!(!html.contains("What is kept") && !html.contains("What Eustress cannot reach") && !html.contains("What is removed"), "{html}");
        // The promise is the Worker's number of days.
        assert!(html_of(status(), Mode::Delete).contains("90 days"));
        let mut thirty = status();
        thirty.soft_delete_days = 30;
        assert!(html_of(thirty, Mode::Delete).contains("30 days"));
    }

    #[test]
    fn delete_space_the_form_has_a_labelled_name_field_and_delete_waits_for_the_name() {
        let html = html_of(status(), Mode::Delete);
        assert!(html.contains("Type the Space&#x27;s name to confirm") || html.contains("Type the Space's name to confirm"), "{html}");
        assert!(html.contains("for=\"sd-name\"") && html.contains("id=\"sd-name\""), "the field has a label");
        assert!(html.contains("role=\"dialog\"") && html.contains("aria-modal=\"true\"") && html.contains("aria-labelledby=\"sd-title\""), "{html}");
        assert!(html.contains("The Super Station"), "the name is shown to be typed");
        assert!(html.contains("To confirm, Eustress emails a 6-digit code to o***@example.com."), "{html}");
        // Nothing typed yet: the Delete button is disabled, and the status is words.
        let button = html.split("<button").find(|b| b.contains("Delete Space")).expect("a Delete Space button");
        assert!(button.contains("disabled"), "{button}");
        assert!(html.contains("Capital letters count."), "{html}");
        assert!(html.contains("data-sd-last"), "a last control for the focus trap");
        // The Worker's `blocked` answer is said and no form is drawn.
        let mut blocked = status();
        blocked.can_delete = false;
        blocked.blocked = Some(Blocked { code: "products_on_sale".to_string(), error: Some("Take every product in this Space off sale first.".to_string()), until: None });
        let html = html_of(blocked, Mode::Delete);
        assert!(html.contains("Take every product in this Space off sale first.") && html.contains("Not possible now."), "{html}");
        assert!(!html.contains("<input") && !html.contains("Delete Space"), "{html}");
    }

    #[test]
    fn delete_space_when_deleting_is_switched_off_it_says_so_and_draws_no_form() {
        let mut off = status();
        off.can_delete = false;
        off.blocked = Some(Blocked { code: "delete_unavailable".to_string(), error: Some("Deleting or restoring a published Space is not available yet.".to_string()), until: None });
        assert_eq!(step_for_status(off.clone()), Step::Unavailable);
        assert_eq!(step_for_status(status()).tag(), 3);
        // The 503 of a route does the same, and any other refusal is a failed read with its sentence.
        assert_eq!(step_for_refusal(&refusal_of(503, r#"{"code":"delete_unavailable"}"#), Mode::Delete), Step::Unavailable);
        assert!(matches!(step_for_refusal(&refusal_of(500, ""), Mode::Delete), Step::LoadFailed(_)));
        let html = html_of(off, Mode::Delete);
        assert!(html.contains("Deleting Spaces isn&#x27;t available yet.") || html.contains("Deleting Spaces isn't available yet."), "{html}");
        for gone in ["<input", "<form", "Type the Space", "What is removed"] {
            assert!(!html.contains(gone), "the switched-off dialog still draws {gone}: {html}");
        }
    }

    #[test]
    fn delete_space_restore_uses_the_restore_routes_and_a_blocked_restore_draws_no_form() {
        let mut deleted = status();
        deleted.state = "soft_deleted".to_string();
        deleted.can_delete = false;
        deleted.can_restore = true;
        deleted.deletion = Some(DeletionView { state: "soft".to_string(), soft_deleted_at: None, hard_delete_after: Some("2027-01-29T21:00:00Z".to_string()), restore_until: Some("2027-01-29T21:00:00Z".to_string()), removal_in_progress: false });
        let html = html_of(deleted.clone(), Mode::Restore);
        assert!(html.contains("Restore The Super Station?") && html.contains("Restore Space") && html.contains("until Jan 29, 2027, 9:00 PM UTC"), "{html}");
        assert!(!html.contains("What is removed"), "a restore does not list what a delete removes");
        assert_eq!(request_call("abc", Mode::Restore, "n").path, "/api/account/spaces/abc/restore-request");
        deleted.can_restore = false;
        deleted.blocked = Some(Blocked { code: "removal_started".to_string(), error: None, until: None });
        let html = html_of(deleted, Mode::Restore);
        assert!(html.contains("Removal of this Space has begun. It can no longer be restored.") && !html.contains("<input"), "{html}");
    }

    #[test]
    fn delete_space_an_open_request_can_be_resumed_only_while_it_is_good() {
        let now = 1_793_481_240_000.0; // 2026-10-31T21:14:00Z
        let open = Pending { request_id: "sdr_1".to_string(), action: "delete".to_string(), status: "pending".to_string(), requested_at: None, expires_at: Some("2026-10-31T21:20:00Z".to_string()), attempts_left: Some(3) };
        assert!(resumable(Some(&open), Mode::Delete, now));
        assert!(!resumable(Some(&open), Mode::Restore, now), "a delete request is not a restore");
        assert!(!resumable(Some(&Pending { status: "locked".to_string(), ..open.clone() }), Mode::Delete, now));
        assert!(!resumable(Some(&Pending { attempts_left: Some(0), ..open.clone() }), Mode::Delete, now));
        assert!(!resumable(Some(&Pending { expires_at: Some("2026-10-31T21:00:00Z".to_string()), ..open.clone() }), Mode::Delete, now));
        assert!(!resumable(Some(&Pending { expires_at: None, ..open.clone() }), Mode::Delete, now));
        assert!(!resumable(None, Mode::Delete, now));
    }

    #[test]
    fn delete_space_the_code_field_takes_six_digits_and_nothing_else() {
        assert_eq!(digits_only("12 34-56"), "123456");
        assert_eq!(digits_only("1234567890"), "123456");
        assert_eq!(digits_only("abc"), "");
        assert!(code_ready("123456") && !code_ready("12345") && !code_ready("1234567") && !code_ready("12345a") && !code_ready(""));
        let cs = CodeStep { request_id: "sdr_1".to_string(), sent_to: Some("o***@example.com".to_string()), expires_at: Some("2026-10-31T21:20:00Z".to_string()), expires_minutes: 15, attempts_left: 5, dead: false };
        use leptos::tachys::view::RenderHtml;
        let html = Owner::new().with(|| {
            view! { {code_view(cs.clone(), Mode::Delete, RwSignal::new(String::new()), RwSignal::new(None), RwSignal::new(false), NodeRef::new(), Callback::new(|_| ()), Callback::new(|_| ()), Callback::new(|_| ())) } }.to_html()
        });
        assert!(html.contains("We sent a code to o***@example.com."), "{html}");
        assert!(html.contains("It works for 15 minutes, until Oct 31, 2026, 9:20 PM UTC."), "{html}");
        assert!(html.contains("5 attempts left."), "{html}");
        assert!(html.contains("for=\"sd-code\"") && html.contains("id=\"sd-code\"") && html.contains("inputmode=\"numeric\"") && html.contains("autocomplete=\"one-time-code\""), "{html}");
        assert!(html.contains("Start over"), "{html}");
        // A dead request offers only a start over: the field and the confirm are disabled.
        let dead = CodeStep { dead: true, attempts_left: 0, ..cs };
        let html = Owner::new().with(|| {
            view! { {code_view(dead.clone(), Mode::Delete, RwSignal::new("123456".to_string()), RwSignal::new(None), RwSignal::new(false), NodeRef::new(), Callback::new(|_| ()), Callback::new(|_| ()), Callback::new(|_| ())) } }.to_html()
        });
        assert!(html.contains("This code can no longer be used."), "{html}");
        let confirm = html.split("<button").find(|b| b.contains("Delete Space")).expect("a confirm button");
        assert!(confirm.contains("disabled"), "{confirm}");
    }

    #[test]
    fn delete_space_done_lines_name_the_dates_and_the_way_back() {
        let reply = ConfirmReply { ok: true, state: "soft_deleted".to_string(), hard_delete_after: Some("2027-01-29T21:00:00Z".to_string()), restore_until: Some("2027-01-29T21:00:00Z".to_string()), restored_at: None };
        assert_eq!(
            done_message(Mode::Delete, "The Super Station", &reply),
            "The Super Station is deleted. It is hidden from everyone now and will be permanently removed on Jan 29, 2027, 9:00 PM UTC. You can restore it from Projects until then."
        );
        assert_eq!(done_message(Mode::Restore, "The Super Station", &ConfirmReply::default()), "The Super Station is restored. It is back as it was before it was deleted.");
    }

    #[test]
    fn delete_space_the_dialog_traps_focus_closes_on_escape_and_has_no_dashes() {
        let src = source();
        for needle in ["tab_wraps_to(at, ev.shift_key())", "\"Escape\" =>", "data-sd-last", "aria-modal=\"true\"", "on_cleanup(move ||", "autocomplete=\"one-time-code\""] {
            assert!(src.contains(needle), "{needle}");
        }
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!src.contains(dash) && !CSS.contains(dash), "no dashes in the copy");
        }
        assert!(!src.to_lowercase().contains("game engine"));
        let link = "<link data-trunk rel=\"css\" href=\"style/project_delete.css\">";
        assert_eq!(INDEX.matches(link).count(), 1, "project_delete.css is linked once");
        assert!(INDEX.find(link).unwrap() < INDEX.find("<link data-trunk rel=\"css\" href=\"style/topbar.css\">").unwrap(), "topbar.css stays last");
        assert!(CSS.contains(".sd-dialog") && CSS.contains(".sd-problem") && CSS.contains(".sd-name"));
    }
}
