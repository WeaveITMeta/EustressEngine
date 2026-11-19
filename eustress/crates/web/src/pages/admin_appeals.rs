// =============================================================================
// Eustress Web - Appeals, decided by a person (/admin/appeals)
// =============================================================================
// Every appeal of a listing review goes to a person, who decides it against the Guardian Code's exact text (docs/moderation/
// PLAYBOOK.md, "Appeals"). An appeal asks one question: was the article or test applied correctly to this evidence? This page
// is where that person works: the appeals waiting, oldest first; the case (the listing's title and description, the appeal's
// text, the decision and its reasons, the judge's test scores with their evidence, the captures the judge saw, the history);
// and Overturn or Uphold with a written rationale.
//
// Every route is the Worker's, under /api/admin/moderation/, open to an administrator only and audited before it answers:
//
//   GET  /api/admin/moderation/workspace/queue?kind=appeal   the appeals waiting
//   GET  /api/admin/moderation/case/{id}                     the full case
//   GET  /api/admin/moderation/captures/{id}/{n}             one capture (needs the session header, so it is fetched, not linked)
//   GET  /api/simulations/{id}                               the listing's title and description (an admin may read it unlisted)
//   POST /api/admin/moderation/tool                          moderation_resolve_appeal: overturned (lists it) or upheld
//
// The page only displays and asks; the Worker decides who may. A 401 or 403 is the "admin access required" state, and a 503
// `audit_unavailable` is a read the Worker refused because it could not write the audit first.
//
// A newer review can decide a case while its appeal waits (a republish, or an edit of the listing's text). The case's status is
// then no longer `appealed`, and upholding would put the old decision back over the newer one. The page says so above the
// buttons and asks the reviewer to confirm before an Uphold.
//
// Table of Contents:
// 1. Data the Worker sends
// 2. The pure rules (tested)
// 3. Fetching
// 4. The page
// 5. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::Value;

use gloo_storage::Storage;

use crate::api::catalog::{rating_words, Listing};
use crate::api::{ApiClient, ApiError};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// 1. Data the Worker sends
// -----------------------------------------------------------------------------

/// One appeal waiting in the Review Workspace.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct QueueItem {
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub reasons: Vec<Value>,
    /// When the appeal was queued.
    #[serde(default)]
    pub at: String,
    /// The case's status now: `appealed` while nothing newer decided it.
    #[serde(default)]
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct QueueReply {
    #[serde(default)]
    items: Vec<QueueItem>,
}

/// The appeal on a case.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Appeal {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub status: String,
    /// The decision appealed: `rejected`, `changes_requested`, or a removal's `quarantined`.
    #[serde(default)]
    pub from_status: Option<String>,
}

/// One line of a case's history.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct HistoryEntry {
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub detail: Value,
}

/// The case, read loosely: the decision and the judge's verdict are drawn from their JSON, so a field the Worker adds later
/// never stops the page from loading.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AppealCase {
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub decision: Value,
    #[serde(default)]
    pub judge: Value,
    #[serde(default)]
    pub appeal: Option<Appeal>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub legal_hold: Value,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct CaseReply {
    #[serde(default)]
    case: AppealCase,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ToolReply {
    #[serde(default)]
    summary: Option<String>,
}

// -----------------------------------------------------------------------------
// 2. The pure rules
// -----------------------------------------------------------------------------

/// The shortest rationale the Worker accepts, in characters of the trimmed text.
pub const RATIONALE_MIN: usize = 20;
/// The most captures a case can have (the Worker serves 0 to 7).
pub const CAPTURES_MAX: usize = 8;
/// The ratings an overturn may list a listing at, in the Worker's order (moderation.mjs RATINGS).
pub const RATINGS: [&str; 6] = ["all_ages", "ages_10", "teen_13", "mature_17", "adult_18", "adult_21"];

/// A person's decision on an appeal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The review was wrong: the listing is listed.
    Overturned,
    /// The review stands: the case returns to the decision appealed.
    Upheld,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Overturned => "overturned",
            Decision::Upheld => "upheld",
        }
    }
}

/// The appeals, oldest first: the one that has waited longest is worked first. Ties keep the order the Worker sent.
pub fn oldest_first(mut items: Vec<QueueItem>) -> Vec<QueueItem> {
    items.sort_by(|a, b| a.at.cmp(&b.at));
    items
}

/// Why the rationale cannot be sent yet, or nothing. Counted as the Worker counts it (JavaScript length of the trimmed text).
pub fn rationale_problem(text: &str) -> Option<String> {
    let n = text.trim().encode_utf16().count();
    (n < RATIONALE_MIN).then(|| format!("Write at least {RATIONALE_MIN} characters on why ({n} so far). The author does not see it; it is the case's record."))
}

// CONTRACT: moderation_resolve_appeal (src/moderation.mjs, docs/moderation/PLAYBOOK.md "Appeals")
/// The body of `POST /api/admin/moderation/tool` that decides an appeal. A rating goes only with an overturn, and only one
/// the Worker knows; without it the Worker lists the listing at the case's own rating.
pub fn resolve_body(sim_id: &str, decision: Decision, rationale: &str, rating: Option<&str>) -> Value {
    let mut args = serde_json::json!({ "sim_id": sim_id, "decision": decision.as_str(), "rationale": rationale.trim() });
    if decision == Decision::Overturned {
        if let Some(r) = rating.filter(|r| RATINGS.contains(r)) {
            args["rating"] = Value::String(r.to_string());
        }
    }
    serde_json::json!({ "name": "moderation_resolve_appeal", "args": args })
}

/// What the reviewer must know when a newer review decided the case while the appeal waited, or nothing when it did not.
pub fn superseded_words(case_status: &str, from_status: Option<&str>) -> Option<String> {
    if case_status.is_empty() || case_status == "appealed" {
        return None;
    }
    let back = from_status.unwrap_or("rejected");
    Some(format!(
        "A newer review decided this case after the appeal was filed: it is now {}. Upholding puts it back to {} and replaces that newer decision. Overturning lists it.",
        status_words(case_status),
        status_words(back)
    ))
}

/// A case status in words.
pub fn status_words(status: &str) -> String {
    match status {
        "approved" => "approved".to_string(),
        "rejected" => "not listed".to_string(),
        "changes_requested" => "changes requested".to_string(),
        "quarantined" => "removed (quarantined)".to_string(),
        "held" => "held for a person".to_string(),
        "pending" | "classifying" => "in review".to_string(),
        "appealed" => "appealed".to_string(),
        other => other.replace('_', " "),
    }
}

/// One reason of a decision on one line: its code, then the article, test, rule and action it names, then the score.
pub fn reason_line(reason: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for key in ["code", "article", "test", "rule", "action", "lane"] {
        if let Some(s) = reason.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            parts.push(s.to_string());
        }
    }
    if let Some(p) = reason.get("p").and_then(Value::as_f64) {
        parts.push(format!("p {p:.2}"));
    }
    if parts.is_empty() {
        reason.to_string()
    } else {
        parts.join(" \u{00b7} ")
    }
}

/// The judge's test scores, T1 first: the test, its score (0 to 3) and the evidence it cited.
pub fn test_rows(judge: &Value) -> Vec<(String, Option<i64>, String)> {
    let mut rows: Vec<(String, Option<i64>, String)> = judge
        .get("tests")
        .and_then(Value::as_object)
        .map(|tests| {
            tests
                .iter()
                .map(|(k, v)| {
                    let score = v.get("score").and_then(Value::as_i64);
                    let evidence = v.get("evidence").and_then(Value::as_str).unwrap_or("").to_string();
                    (k.clone(), score, evidence)
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Why the buttons are off when the appeal is not waiting.
pub fn not_pending_words(appeal_status: &str) -> String {
    if appeal_status.is_empty() {
        "This case has no appeal: there is nothing to decide.".to_string()
    } else {
        format!("This appeal is already {appeal_status}: there is nothing to decide.")
    }
}

/// A text field of a JSON object, or nothing when it is absent or empty.
pub fn text_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// "2026-11-14 16:05 UTC" from an ISO time; anything else as it came.
pub fn short_time(iso: &str) -> String {
    let b = iso.as_bytes();
    if iso.len() >= 16 && b.get(10) == Some(&b'T') {
        format!("{} {} UTC", &iso[..10], &iso[11..16])
    } else {
        iso.to_string()
    }
}

/// Why a read or a decision failed, in words.
pub fn failure_words(e: &ApiError) -> String {
    match e {
        ApiError::Server { status: 503, message } if message.contains("audit_unavailable") => {
            "The audit log could not be written, so the Worker did not answer. Try again in a minute.".to_string()
        }
        ApiError::Server { message, .. } => serde_json::from_str::<Value>(message)
            .ok()
            .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| crate::api::failure_sentence(e)),
        other => crate::api::failure_sentence(other),
    }
}

fn denied(e: &ApiError) -> bool {
    matches!(e, ApiError::Unauthorized | ApiError::Server { status: 403, .. })
}

// -----------------------------------------------------------------------------
// 3. Fetching
// -----------------------------------------------------------------------------

/// One capture as an object URL the page can show, `Ok(None)` when there is no capture at `n` (the list ends there), or an
/// error. The route needs the session header, which an `<img src>` cannot send.
async fn fetch_capture(api_url: &str, sim_id: &str, n: usize) -> Result<Option<String>, String> {
    let url = format!("{api_url}/api/admin/moderation/captures/{}/{n}", urlencoding::encode(sim_id));
    let mut req = gloo_net::http::Request::get(&url);
    if let Ok(token) = gloo_storage::LocalStorage::get::<String>("auth_token") {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    match res.status() {
        200 => {}
        404 => return Ok(None),
        503 => return Err("The audit log could not be written, so the captures were not read.".to_string()),
        s => return Err(format!("The captures could not be read (the server answered {s}).")),
    }
    let mime = res.headers().get("content-type").unwrap_or_else(|| "image/png".to_string());
    let bytes = res.binary().await.map_err(|e| e.to_string())?;
    let array = js_sys::Uint8Array::from(&bytes[..]);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(&mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(|_| "The capture could not be shown.".to_string())?;
    web_sys::Url::create_object_url_with_blob(&blob).map(Some).map_err(|_| "The capture could not be shown.".to_string())
}

fn revoke_all(urls: &[String]) {
    for url in urls {
        let _ = web_sys::Url::revoke_object_url(url);
    }
}

// -----------------------------------------------------------------------------
// 4. The page
// -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Load<T> {
    Loading,
    Denied,
    Failed(String),
    Ready(T),
}

/// The case being read, with its listing.
#[derive(Clone, Debug)]
struct Opened {
    case: AppealCase,
    listing: Option<Listing>,
}

#[component]
pub fn AdminAppealsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());

    let queue = RwSignal::new(Load::<Vec<QueueItem>>::Loading);
    let selected = RwSignal::new(None::<String>);
    let opened = RwSignal::new(None::<Load<Opened>>);
    let captures = RwSignal::new(Vec::<String>::new());
    let captures_note = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);

    let load_queue = move || {
        queue.set(Load::Loading);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match client.get::<QueueReply>("/api/admin/moderation/workspace/queue?kind=appeal&limit=200").await {
                Ok(r) => queue.set(Load::Ready(oldest_first(r.items))),
                Err(e) if denied(&e) => queue.set(Load::Denied),
                Err(e) => queue.set(Load::Failed(failure_words(&e))),
            }
        });
    };
    Effect::new(move |_| load_queue());

    // Opening a case: the case, then the listing, then the captures one by one until there are no more.
    let open = move |sim_id: String| {
        revoke_all(&captures.get_untracked());
        captures.set(Vec::new());
        captures_note.set(None);
        notice.set(None);
        selected.set(Some(sim_id.clone()));
        opened.set(Some(Load::Loading));
        spawn_local(async move {
            let api = api_url.get_value();
            let client = ApiClient::new(&api);
            let path = format!("/api/admin/moderation/case/{}", urlencoding::encode(&sim_id));
            let case = match client.get::<CaseReply>(&path).await {
                Ok(r) => r.case,
                Err(e) if denied(&e) => return opened.set(Some(Load::Denied)),
                Err(e) => return opened.set(Some(Load::Failed(failure_words(&e)))),
            };
            let listing = client.get::<Listing>(&format!("/api/simulations/{}", urlencoding::encode(&sim_id))).await.ok();
            if selected.get_untracked().as_deref() != Some(sim_id.as_str()) {
                return;
            }
            opened.set(Some(Load::Ready(Opened { case, listing })));
            for n in 0..CAPTURES_MAX {
                match fetch_capture(&api, &sim_id, n).await {
                    Ok(Some(url)) => {
                        if selected.get_untracked().as_deref() != Some(sim_id.as_str()) {
                            revoke_all(&[url]);
                            return;
                        }
                        captures.update(|c| c.push(url));
                    }
                    Ok(None) => break,
                    Err(why) => {
                        captures_note.set(Some(why));
                        break;
                    }
                }
            }
            if captures.get_untracked().is_empty() && captures_note.get_untracked().is_none() {
                captures_note.set(Some("This case has no captures.".to_string()));
            }
        });
    };
    on_cleanup(move || revoke_all(&captures.get_untracked()));

    // A decision was recorded: the case leaves the queue and the panel closes.
    let decided = Callback::new(move |(sim_id, words): (String, String)| {
        queue.update(|q| {
            if let Load::Ready(items) = q {
                items.retain(|i| i.sim_id != sim_id);
            }
        });
        revoke_all(&captures.get_untracked());
        captures.set(Vec::new());
        selected.set(None);
        opened.set(None);
        notice.set(Some(words));
    });

    let list = move || match queue.get() {
        Load::Loading => view! { <div class="loading-state"><div class="spinner"></div><p>"Loading the appeals..."</p></div> }.into_any(),
        Load::Denied => view! {
            <div class="admin-denied">
                <h2>"Admin access required"</h2>
                <p>"Appeals are decided by a Eustress reviewer. Sign in with an admin identity to continue."</p>
                <a class="btn-primary" href="/login">"Sign in"</a>
            </div>
        }.into_any(),
        Load::Failed(why) => view! {
            <div class="alert alert-error admin-alert">
                <strong>"The appeals could not be loaded. "</strong><span>{why}</span>
                <button type="button" class="btn btn-secondary" on:click=move |_| load_queue()>"Try again"</button>
            </div>
        }.into_any(),
        Load::Ready(items) if items.is_empty() => view! { <p class="admin-empty appeals-empty">"No appeals are waiting."</p> }.into_any(),
        Load::Ready(items) => view! {
            <ul class="appeals-queue" aria-label="Appeals waiting, oldest first">
                {items.into_iter().map(|item| {
                    let id = item.sim_id.clone();
                    let id_for_class = item.sim_id.clone();
                    let reference = item.reference.clone().unwrap_or_else(|| item.sim_id.clone());
                    let first = item.reasons.first().map(reason_line).unwrap_or_default();
                    let moved = item.status != "appealed" && !item.status.is_empty();
                    view! {
                        <li>
                            <button
                                type="button"
                                class="appeals-item"
                                class:on=move || selected.get().as_deref() == Some(id_for_class.as_str())
                                on:click=move |_| open(id.clone())
                            >
                                <span class="appeals-ref">{reference}</span>
                                <span class="appeals-when">{format!("Waiting since {}", short_time(&item.at))}</span>
                                {(!first.is_empty()).then(|| view! { <span class="appeals-why">{first}</span> })}
                                {moved.then(|| view! { <span class="appeals-flag">{format!("Now {}", status_words(&item.status))}</span> })}
                            </button>
                        </li>
                    }
                }).collect_view()}
            </ul>
        }.into_any(),
    };

    let detail = move || match opened.get() {
        None => view! { <p class="admin-empty">"Choose an appeal to read its case."</p> }.into_any(),
        Some(Load::Loading) => view! { <div class="loading-state"><div class="spinner"></div><p>"Reading the case..."</p></div> }.into_any(),
        Some(Load::Denied) => view! { <p class="alert alert-error admin-alert">"Admin access required."</p> }.into_any(),
        Some(Load::Failed(why)) => view! { <p class="alert alert-error admin-alert">{why}</p> }.into_any(),
        Some(Load::Ready(o)) => view! { <CaseDetail opened=o captures=captures captures_note=captures_note on_decided=decided /> }.into_any(),
    };

    view! {
        <div class="page page-admin page-admin-appeals">
            <CentralNav active="admin".to_string() />
            <section class="dashboard-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"ADMIN"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="dashboard-title">"Appeals"</h1>
                <p class="dashboard-subtitle">"Was the article or test applied correctly to this evidence? Decide each appeal against the Code's exact text."</p>
            </section>
            <div class="dashboard-container">
                {move || notice.get().map(|words| view! { <p class="alert appeals-notice" role="status">{words}</p> })}
                <div class="appeals-layout">
                    <section class="dashboard-section appeals-list">
                        <div class="section-header-industrial">
                            <h2>"Waiting"</h2>
                            <button type="button" class="btn btn-secondary appeals-refresh" on:click=move |_| load_queue()>"Refresh"</button>
                        </div>
                        {list}
                    </section>
                    <section class="dashboard-section appeals-case">{detail}</section>
                </div>
            </div>
            <Footer />
        </div>
    }
}

/// One case: what was decided and why, what the author says, the evidence, and the decision.
#[component]
fn CaseDetail(opened: Opened, captures: RwSignal<Vec<String>>, captures_note: RwSignal<Option<String>>, on_decided: Callback<(String, String)>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let Opened { case, listing } = opened;
    let sim_id = StoredValue::new(case.sim_id.clone());
    let appeal = case.appeal.clone().unwrap_or_default();
    let from_status = appeal.from_status.clone();
    let superseded = superseded_words(&case.status, from_status.as_deref());
    let needs_confirm = superseded.is_some();
    let pending = appeal.status == "pending";

    let title = listing.as_ref().map(|l| l.name.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled Space".to_string());
    let description = listing.as_ref().map(|l| l.description.trim().to_string()).unwrap_or_default();
    let author = listing.as_ref().and_then(|l| l.author_name.clone()).filter(|a| !a.trim().is_empty());
    let page_href = format!("/simulation/{}", urlencoding::encode(&case.sim_id));
    let reference = case.reference.clone().unwrap_or_else(|| case.sim_id.clone());

    let reasons: Vec<String> = case.decision.get("reasons").and_then(Value::as_array).map(|r| r.iter().map(reason_line).collect()).unwrap_or_default();
    let decision_rationale = text_of(&case.decision, "rationale").map(str::to_string);
    let suggested = text_of(&case.decision, "suggested_edit").map(str::to_string);
    let case_rating = text_of(&case.decision, "rating").map(str::to_string);
    let tests = test_rows(&case.judge);
    let judge_rationale = text_of(&case.judge, "rationale").map(str::to_string);
    let statement_check = text_of(&case.judge, "statement_check").map(str::to_string);
    let legal_hold = !case.legal_hold.is_null() && case.legal_hold != Value::Bool(false);
    let history: Vec<HistoryEntry> = case.history.iter().rev().take(30).cloned().collect();

    let rationale = RwSignal::new(String::new());
    let rating = RwSignal::new(case_rating.clone().filter(|r| RATINGS.contains(&r.as_str())).unwrap_or_default());
    let confirmed = RwSignal::new(false);
    let sending = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);

    let decide = move |decision: Decision| {
        if sending.get_untracked() {
            return;
        }
        let text = rationale.get_untracked();
        if let Some(why) = rationale_problem(&text) {
            problem.set(Some(why));
            return;
        }
        if decision == Decision::Upheld && needs_confirm && !confirmed.get_untracked() {
            problem.set(Some("Confirm that upholding replaces the newer decision, or overturn instead.".to_string()));
            return;
        }
        let chosen = rating.get_untracked();
        let body = resolve_body(&sim_id.get_value(), decision, &text, Some(chosen.as_str()).filter(|r| !r.is_empty()));
        sending.set(true);
        problem.set(None);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match client.post::<ToolReply, _>("/api/admin/moderation/tool", &body).await {
                Ok(r) => {
                    sending.set(false);
                    let words = match decision {
                        Decision::Overturned => "Appeal overturned: the listing is listed, and the author is told.",
                        Decision::Upheld => "Appeal upheld: the decision stands, and the author is told.",
                    };
                    let line = r.summary.map(|s| format!("{words} ({s})")).unwrap_or_else(|| words.to_string());
                    on_decided.run((sim_id.get_value(), line));
                }
                Err(e) => {
                    sending.set(false);
                    problem.set(Some(failure_words(&e)));
                }
            }
        });
    };

    view! {
        <article class="appeals-detail">
            <header class="appeals-head">
                <h2>{title}</h2>
                <p class="appeals-meta">
                    {format!("Case {reference}")}
                    {author.map(|a| format!(" \u{00b7} by @{a}"))}
                    " \u{00b7} "<a href=page_href target="_blank" rel="noopener">"Open the listing page"</a>
                </p>
            </header>

            <section class="appeals-block">
                <h3>"The appeal"</h3>
                <p class="appeals-meta">{format!("Filed {} against {}", short_time(&appeal.at), status_words(from_status.as_deref().unwrap_or("rejected")))}</p>
                <blockquote class="appeals-text">{appeal.text.clone()}</blockquote>
            </section>

            <section class="appeals-block">
                <h3>"What was decided"</h3>
                {if reasons.is_empty() {
                    view! { <p class="admin-empty">"The decision recorded no reasons."</p> }.into_any()
                } else {
                    view! { <ul class="appeals-reasons">{reasons.into_iter().map(|r| view! { <li><code>{r}</code></li> }).collect_view()}</ul> }.into_any()
                }}
                {decision_rationale.map(|r| view! { <p><strong>"Rationale: "</strong>{r}</p> })}
                {suggested.map(|s| view! { <p><strong>"Change it asked for: "</strong>{s}</p> })}
                {case_rating.map(|r| view! { <p><strong>"Rating: "</strong>{rating_words(Some(r.as_str())).unwrap_or("not set").to_string()}</p> })}
            </section>

            <section class="appeals-block">
                <h3>"The judge's tests"</h3>
                {if tests.is_empty() {
                    view! { <p class="admin-empty">"No test scores were recorded (the judge did not run, or the case was decided before it)."</p> }.into_any()
                } else {
                    view! {
                        <table class="admin-table appeals-tests">
                            <thead><tr><th>"Test"</th><th>"Score"</th><th>"Evidence"</th></tr></thead>
                            <tbody>
                                {tests.into_iter().map(|(t, score, evidence)| view! {
                                    <tr class:low=score.map(|s| s < 2).unwrap_or(false)>
                                        <td>{t}</td>
                                        <td>{score.map(|s| format!("{s} of 3")).unwrap_or_else(|| "none".to_string())}</td>
                                        <td>{evidence}</td>
                                    </tr>
                                }).collect_view()}
                            </tbody>
                        </table>
                    }.into_any()
                }}
                {statement_check.map(|s| view! { <p><strong>"Qualification Statement: "</strong>{s}</p> })}
                {judge_rationale.map(|r| view! { <p><strong>"Judge's rationale: "</strong>{r}</p> })}
            </section>

            <section class="appeals-block">
                <h3>"The listing"</h3>
                {if description.is_empty() {
                    view! { <p class="admin-empty">"No description."</p> }.into_any()
                } else {
                    view! { <p class="appeals-description">{description}</p> }.into_any()
                }}
            </section>

            <section class="appeals-block">
                <h3>"Captures the judge saw"</h3>
                <div class="appeals-captures">
                    {move || captures.get().into_iter().enumerate().map(|(i, src)| {
                        let href = src.clone();
                        let alt = format!("Capture {}", i + 1);
                        view! { <a href=href target="_blank" rel="noopener"><img src=src alt=alt /></a> }
                    }).collect_view()}
                </div>
                {move || captures_note.get().map(|n| view! { <p class="admin-empty">{n}</p> })}
            </section>

            <details class="appeals-block appeals-history">
                <summary>"History"</summary>
                <ul>
                    {history.into_iter().map(|h| view! {
                        <li><span class="appeals-when">{short_time(&h.at)}</span>" "<code>{h.event}</code></li>
                    }).collect_view()}
                </ul>
            </details>

            <section class="appeals-block appeals-decide">
                <h3>"Decide"</h3>
                {(!pending).then(|| view! { <p class="alert alert-error admin-alert">{not_pending_words(&appeal.status)}</p> })}
                {legal_hold.then(|| view! { <p class="alert admin-notice">"A legal hold is on this listing. Overturning lifts it, except a TAKE IT DOWN request, which only the takedown review can lift."</p> })}
                {superseded.clone().map(|words| view! {
                    <div class="alert admin-notice appeals-superseded">
                        <p>{words}</p>
                        <label><input type="checkbox" prop:checked=move || confirmed.get() on:change=move |e| confirmed.set(event_target_checked(&e)) />" Uphold anyway and replace the newer decision"</label>
                    </div>
                })}
                <label class="appeals-label" for="appeals-rationale">"Rationale (kept with the case; names the article or test and the evidence)"</label>
                <textarea
                    id="appeals-rationale"
                    class="appeals-rationale"
                    rows="5"
                    prop:value=move || rationale.get()
                    on:input=move |e| { rationale.set(event_target_value(&e)); problem.set(None); }
                ></textarea>
                <p class="appeals-count">{move || format!("{} characters, at least {RATIONALE_MIN}", rationale.get().trim().encode_utf16().count())}</p>
                <label class="appeals-label" for="appeals-rating">"Rating if overturned"</label>
                <select id="appeals-rating" class="appeals-select" on:change=move |e| rating.set(event_target_value(&e))>
                    <option value="" selected=move || rating.get().is_empty()>"The case's own rating"</option>
                    {RATINGS.iter().map(|r| {
                        let r = *r;
                        view! { <option value=r selected=move || rating.get() == r>{rating_words(Some(r)).unwrap_or(r)}</option> }
                    }).collect_view()}
                </select>
                {move || problem.get().map(|why| view! { <p class="appeals-problem" role="alert">{why}</p> })}
                <div class="appeals-actions">
                    <button type="button" class="btn btn-primary" disabled=move || sending.get() || !pending on:click=move |_| decide(Decision::Overturned)>
                        "Overturn: list it"
                    </button>
                    <button type="button" class="btn btn-secondary" disabled=move || sending.get() || !pending on:click=move |_| decide(Decision::Upheld)>
                        "Uphold: the decision stands"
                    </button>
                </div>
            </section>
        </article>
    }
}

// -----------------------------------------------------------------------------
// 5. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, at: &str) -> QueueItem {
        QueueItem { sim_id: id.to_string(), at: at.to_string(), status: "appealed".to_string(), ..Default::default() }
    }

    #[test]
    fn the_queue_is_worked_oldest_first_and_ties_keep_the_workers_order() {
        let sorted = oldest_first(vec![item("c", "2026-11-14T10:00:00Z"), item("a", "2026-11-12T09:00:00Z"), item("b", "2026-11-14T10:00:00Z")]);
        assert_eq!(sorted.iter().map(|i| i.sim_id.as_str()).collect::<Vec<_>>(), ["a", "c", "b"]);
    }

    #[test]
    fn a_rationale_needs_twenty_characters_counted_as_the_worker_counts_them() {
        assert!(rationale_problem("   too short   ").is_some());
        assert!(rationale_problem(&"x".repeat(19)).unwrap().contains("19 so far"));
        assert_eq!(rationale_problem(&format!("  {}  ", "x".repeat(20))), None);
        // Ten emoji are twenty UTF-16 units, as JavaScript counts them.
        assert_eq!(rationale_problem(&"\u{1F680}".repeat(10)), None);
    }

    #[test]
    fn the_decision_body_is_the_workers_tool_call_and_a_rating_goes_only_with_an_overturn() {
        let over = resolve_body("07d0", Decision::Overturned, "  T4 evidence shows the tide tables.  ", Some("ages_10"));
        assert_eq!(over, serde_json::json!({ "name": "moderation_resolve_appeal", "args": { "sim_id": "07d0", "decision": "overturned", "rationale": "T4 evidence shows the tide tables.", "rating": "ages_10" } }));
        let up = resolve_body("07d0", Decision::Upheld, "The T3 lesson is not shown.", Some("ages_10"));
        assert!(up["args"].get("rating").is_none(), "an uphold sends no rating");
        assert_eq!(up["args"]["decision"], "upheld");
        let unknown = resolve_body("07d0", Decision::Overturned, "The review misread the statement.", Some("everyone"));
        assert!(unknown["args"].get("rating").is_none(), "a rating the Worker does not know is never sent");
        assert!(resolve_body("07d0", Decision::Overturned, "The review misread the statement.", None)["args"].get("rating").is_none());
    }

    #[test]
    fn a_case_a_newer_review_decided_is_named_before_anyone_upholds_over_it() {
        assert_eq!(superseded_words("appealed", Some("rejected")), None);
        assert_eq!(superseded_words("", None), None);
        let words = superseded_words("approved", Some("changes_requested")).unwrap();
        assert!(words.contains("now approved"), "{words}");
        assert!(words.contains("puts it back to changes requested"), "{words}");
        assert!(superseded_words("pending", None).unwrap().contains("in review"));
    }

    #[test]
    fn reasons_and_test_scores_read_from_the_case_json() {
        let reason = serde_json::json!({ "code": "test_real_world_fiction", "test": "T4", "action": "reject", "p": 0.873 });
        assert_eq!(reason_line(&reason), "test_real_world_fiction \u{00b7} T4 \u{00b7} reject \u{00b7} p 0.87");
        assert_eq!(reason_line(&serde_json::json!({})), "{}");
        let judge = serde_json::json!({ "tests": { "T3": { "score": 1, "evidence": "no lesson shown" }, "T1": { "score": 3, "evidence": "paddles" }, "T4": {} } });
        let rows = test_rows(&judge);
        assert_eq!(rows.iter().map(|r| (r.0.as_str(), r.1)).collect::<Vec<_>>(), [("T1", Some(3)), ("T3", Some(1)), ("T4", None)]);
        assert_eq!(rows[1].2, "no lesson shown");
        assert!(test_rows(&Value::Null).is_empty());
    }

    #[test]
    fn times_statuses_and_failures_are_said_in_words() {
        assert_eq!(short_time("2026-11-14T23:05:41.120Z"), "2026-11-14 23:05 UTC");
        assert_eq!(short_time("yesterday"), "yesterday");
        assert_eq!(status_words("rejected"), "not listed");
        assert_eq!(status_words("some_new_state"), "some new state");
        let audit = ApiError::Server { status: 503, message: r#"{"error":"audit_unavailable","code":"audit_unavailable"}"#.into() };
        assert!(failure_words(&audit).contains("audit log"));
        let refused = ApiError::Server { status: 400, message: r#"{"ok":false,"error":"no pending appeal"}"#.into() };
        assert_eq!(failure_words(&refused), "no pending appeal");
        assert!(denied(&ApiError::Unauthorized));
        assert!(denied(&ApiError::Server { status: 403, message: String::new() }));
        assert!(!denied(&refused));
    }
}
