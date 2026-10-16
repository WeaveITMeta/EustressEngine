// =============================================================================
// Eustress Web - Verify an existing account (/verify-account)
// =============================================================================
// An account made before identity verification was required can sign in and
// read, and the Worker refuses every change with 403 `kyc_required` until the
// account is linked to one verified KYC session. This page runs the same KYC
// session the sign-up runs (the upload and submit helpers of pages/login.rs, the
// phone hand-off of pages/verify.rs), and when the session is `verified` it
// calls POST /api/kyc/link { kyc_session_id }, then reads the account again
// (GET /api/auth/me) so `kyc_required` clears and the banner goes away.
//
// The Worker's routes used here:
//   POST /api/kyc/start         -> { session_id, claim_secret, upload_token } (through login.rs ensure_kyc_session)
//   POST /api/kyc/upload        multipart: document, side, id_type, session_id, upload_token
//   POST /api/kyc/submit        { session_id, upload_token, needs_back, full_name }
//   GET  /api/kyc/status/<id>   (through the login.rs helper)
//   POST /api/kyc/handoff       { session_id, upload_token, id_type, needs_back } -> verify_url (/verify?s=<id>#t=<token>)
//   GET  /api/kyc/session/<id>  progress of a phone capture
//   POST /api/kyc/link          { kyc_session_id, kyc_claim_secret } (the account's session)
//
// The claim secret is in memory only: a signal in this page, set by ensure_kyc_session, sent in the body of the
// link call and nowhere else, and cleared once the link worked. The phone gets the upload token, never the secret.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{self, ApiClient, ApiError};
use crate::components::result_card::{Glyph, ResultCard, ResultPage, Tone};
use crate::components::QrCodeSvg;
use crate::pages::login::{ensure_kyc_session, submit_kyc_verification, upload_id_document};
use crate::state::{AppState, AuthState, User};

/// The route.
pub const PATH: &str = "/verify-account";

/// The Worker's link route.
pub const LINK_PATH: &str = "/api/kyc/link";

// CONTRACT: claim secret, confirm with the KYC builder
// The one place the claim secret's field name is written. The Worker hands the secret out when the KYC session
// starts (POST /api/kyc/start) and asks for it again on the link call, next to `kyc_session_id`. It is held in
// memory only (a signal in this page): never stored, never logged, never put in a URL, never given to the phone.
pub const CLAIM_SECRET_FIELD: &str = "kyc_claim_secret";

// -----------------------------------------------------------------------------
// What the page shows
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The sign-in has not been looked up yet.
    Checking,
    /// Nobody is signed in.
    SignIn,
    /// Signed in, and the Worker does not ask this account to verify.
    Verified,
    /// This visit just verified the account.
    Done,
    /// Signed in, and the account must verify: the KYC session.
    Flow,
}

pub fn screen_for(auth: &AuthState, linked: bool) -> Screen {
    match auth {
        AuthState::Unknown => Screen::Checking,
        AuthState::Unauthenticated => Screen::SignIn,
        AuthState::Authenticated(_) if linked => Screen::Done,
        AuthState::Authenticated(user) if user.kyc_required => Screen::Flow,
        AuthState::Authenticated(_) => Screen::Verified,
    }
}

/// The ID types offered: the value sent to the Worker, and the words.
pub const ID_TYPES: [(&str, &str); 3] = [
    ("passport", "Passport"),
    ("drivers_license", "Driver's licence"),
    ("national_id", "National ID card"),
];

/// A passport is one photo; every other document is a front and a back.
pub fn needs_back(id_type: &str) -> bool {
    id_type != "passport"
}

// -----------------------------------------------------------------------------
// The link call
// -----------------------------------------------------------------------------

/// What the Worker answers when the link worked.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LinkReply {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub already_verified: bool,
}

/// The body of the link call.
pub fn link_body(session_id: &str, claim_secret: &str) -> Value {
    let mut body = json!({ "kyc_session_id": session_id });
    body[CLAIM_SECRET_FIELD] = Value::String(claim_secret.to_string());
    body
}

/// Link the verified session to the signed-in account.
pub async fn link_session(client: &ApiClient, session_id: &str, claim_secret: &str) -> Result<LinkReply, ApiError> {
    client.post(LINK_PATH, &link_body(session_id, claim_secret)).await
}

/// The account as the page shows it once the Worker has linked it: nothing left to verify.
pub fn cleared(mut user: User) -> User {
    user.kyc_required = false;
    user
}

/// Link, then read the account again so `kyc_required` clears and the banner goes. `linked` is set the
/// moment the Worker says yes, before the account is read, so the page never shows "already verified"
/// between the two.
async fn link_and_refresh(state: AppState, session: String, claim_secret: String, linked: RwSignal<bool>) -> Result<(), ApiError> {
    let client = ApiClient::new(&state.api_url);
    link_session(&client, &session, &claim_secret).await?;
    linked.set(true);
    match api::get_current_user(&client).await {
        Ok(user) => state.login(user),
        // The Worker said yes; if it cannot be asked again right now, the account is still the one it verified.
        Err(_) => {
            if let AuthState::Authenticated(user) = state.auth.get_untracked() {
                state.login(cleared(user));
            }
        }
    }
    Ok(())
}

/// What a failed link call means for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkProblem {
    /// The same session may work in a moment.
    TryAgain(String),
    /// This session cannot be used; a new one can.
    StartOver(String),
    /// Nothing the person can do here.
    Stop(String),
}

impl LinkProblem {
    pub fn sentence(&self) -> &str {
        match self {
            LinkProblem::TryAgain(s) | LinkProblem::StartOver(s) | LinkProblem::Stop(s) => s,
        }
    }
}

pub fn link_problem_of(error: &ApiError) -> LinkProblem {
    if let ApiError::Server { status, message } = error {
        let body = serde_json::from_str::<Value>(message).ok();
        let said = |key: &str| {
            body.as_ref()
                .and_then(|v| v.get(key))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        match (*status, api::worker_code(message).as_deref()) {
            (409, Some("kyc_id_in_use")) => return LinkProblem::Stop(api::KYC_ID_IN_USE_SENTENCE.to_string()),
            (409, Some("kyc_session_used")) => {
                return LinkProblem::StartOver(said("error").unwrap_or_else(|| "This identity verification was already used. Verify your ID again.".to_string()))
            }
            // The Worker says why in `reason` ("more than 24 hours old", "has not passed").
            (403, Some("kyc_required")) => {
                return LinkProblem::StartOver(said("reason").or_else(|| said("error")).unwrap_or_else(|| "Verify your ID again.".to_string()))
            }
            // The Worker has no pepper secret set: nothing the person can do, and nothing a retry fixes.
            (503, Some("kyc_unconfigured")) => {
                return LinkProblem::Stop(said("error").unwrap_or_else(|| "Identity verification is not set up yet. Contact support@eustress.dev.".to_string()))
            }
            (500..=599, _) => return LinkProblem::TryAgain(api::failure_sentence(error)),
            _ => {}
        }
    }
    match error {
        ApiError::Network(_) => LinkProblem::TryAgain(api::failure_sentence(error)),
        other => LinkProblem::Stop(api::failure_sentence(other)),
    }
}

// -----------------------------------------------------------------------------
// Words for the upload and the phone
// -----------------------------------------------------------------------------

/// The sentence for a failed upload. The login.rs helper reports `Upload error (415): {json}`; the Worker's
/// own sentence is inside the JSON.
pub fn upload_problem(raw: &str) -> String {
    raw.find('{')
        .and_then(|at| api::worker_sentence(&raw[at..]))
        .unwrap_or_else(|| raw.to_string())
}

/// The body that starts a phone hand-off for this session: the upload token, never the claim secret.
pub fn handoff_payload(session_id: &str, upload_token: &str, id_type: &str) -> Value {
    json!({ "session_id": session_id, "upload_token": upload_token, "id_type": id_type, "needs_back": needs_back(id_type) })
}

/// The address the phone opens, from the Worker's hand-off answer.
pub fn verify_url_of(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.get("verify_url")?.as_str().filter(|u| !u.is_empty()).map(str::to_string)
}

/// How far the phone has got, from `GET /api/kyc/session/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    Seen { front: bool, back: bool },
    Verified(String),
    Rejected(String),
}

pub fn session_progress(v: &Value) -> Progress {
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    match v.get("status").and_then(Value::as_str).unwrap_or("") {
        "verified" => Progress::Verified(v.get("ocr_name").and_then(Value::as_str).unwrap_or("").to_string()),
        "rejected" => Progress::Rejected(v.get("reason").and_then(Value::as_str).unwrap_or("").to_string()),
        _ if flag("front_uploaded") || flag("back_uploaded") => Progress::Seen { front: flag("front_uploaded"), back: flag("back_uploaded") },
        _ => Progress::Waiting,
    }
}

// -----------------------------------------------------------------------------
// The session, as signals
// -----------------------------------------------------------------------------

/// Everything one verification session holds. Every field is a signal, so the struct is `Copy`.
#[derive(Clone, Copy)]
struct Flow {
    /// The session id, the claim secret and the upload token come from POST /api/kyc/start on first use
    /// (ensure_kyc_session) and are empty until then. Memory only.
    session: RwSignal<String>,
    claim: RwSignal<String>,
    upload_token: RwSignal<String>,
    id_type: RwSignal<String>,
    front_name: RwSignal<String>,
    front_done: RwSignal<bool>,
    back_name: RwSignal<String>,
    back_done: RwSignal<bool>,
    status: RwSignal<String>,
    verified_name: RwSignal<String>,
    verified: RwSignal<bool>,
    rejected: RwSignal<bool>,
    reject_reason: RwSignal<String>,
    unavailable: RwSignal<String>,
    problem: RwSignal<String>,
    link_started: RwSignal<bool>,
    link_busy: RwSignal<bool>,
    link_problem: RwSignal<Option<LinkProblem>>,
    linked: RwSignal<bool>,
    qr_open: RwSignal<bool>,
    qr_url: RwSignal<String>,
    qr_error: RwSignal<String>,
    qr_generation: RwSignal<u32>,
}

impl Flow {
    fn new() -> Self {
        Self {
            session: RwSignal::new(String::new()),
            claim: RwSignal::new(String::new()),
            upload_token: RwSignal::new(String::new()),
            id_type: RwSignal::new("passport".to_string()),
            front_name: RwSignal::new(String::new()),
            front_done: RwSignal::new(false),
            back_name: RwSignal::new(String::new()),
            back_done: RwSignal::new(false),
            status: RwSignal::new(String::new()),
            verified_name: RwSignal::new(String::new()),
            verified: RwSignal::new(false),
            rejected: RwSignal::new(false),
            reject_reason: RwSignal::new(String::new()),
            unavailable: RwSignal::new(String::new()),
            problem: RwSignal::new(String::new()),
            link_started: RwSignal::new(false),
            link_busy: RwSignal::new(false),
            link_problem: RwSignal::new(None),
            linked: RwSignal::new(false),
            qr_open: RwSignal::new(false),
            qr_url: RwSignal::new(String::new()),
            qr_error: RwSignal::new(String::new()),
            qr_generation: RwSignal::new(0),
        }
    }

    /// A new session: nothing uploaded, nothing held from the last one. The next upload starts a fresh one.
    fn start_over(self, id_type: &str) {
        self.qr_generation.update(|g| *g += 1);
        self.session.set(String::new());
        self.upload_token.set(String::new());
        self.claim.set(String::new());
        self.id_type.set(id_type.to_string());
        self.front_name.set(String::new());
        self.front_done.set(false);
        self.back_name.set(String::new());
        self.back_done.set(false);
        self.status.set(String::new());
        self.verified_name.set(String::new());
        self.verified.set(false);
        self.rejected.set(false);
        self.reject_reason.set(String::new());
        self.unavailable.set(String::new());
        self.problem.set(String::new());
        self.link_started.set(false);
        self.link_busy.set(false);
        self.link_problem.set(None);
        self.qr_open.set(false);
        self.qr_url.set(String::new());
        self.qr_error.set(String::new());
    }

    /// Upload one side with the sign-up's helper, and submit when every side the document needs is in.
    fn upload(self, side: &'static str, file: web_sys::File) {
        self.problem.set(String::new());
        self.status.set("Uploading document...".to_string());
        self.verified.set(false);
        self.rejected.set(false);
        self.unavailable.set(String::new());
        if side == "front" {
            self.front_name.set(file.name());
        } else {
            self.back_name.set(file.name());
        }
        let id_type = self.id_type.get_untracked();
        let back_needed = needs_back(&id_type);
        spawn_local(async move {
            // The session's id and upload token, from /api/kyc/start on first use (the sign-up's helper).
            let (session, upload_token) = match ensure_kyc_session(self.session, self.claim, self.upload_token).await {
                Ok(started) => started,
                Err(e) => {
                    self.problem.set(e);
                    self.status.set(String::new());
                    return;
                }
            };
            match upload_id_document(&file, side, &id_type, "", "", "", &session, &upload_token).await {
                Ok(_) => {
                    if side == "front" {
                        self.front_done.set(true);
                    } else {
                        self.back_done.set(true);
                    }
                    if self.front_done.get_untracked() && (!back_needed || self.back_done.get_untracked()) {
                        self.status.set("Submitting for verification...".to_string());
                        self.submit().await;
                    } else {
                        self.status.set(if side == "front" { "Front uploaded. Now upload the back.".to_string() } else { "Back uploaded. Now upload the front.".to_string() });
                    }
                }
                Err(e) => {
                    self.problem.set(upload_problem(&e));
                    self.status.set(String::new());
                    if side == "front" {
                        self.front_done.set(false);
                    } else {
                        self.back_done.set(false);
                    }
                }
            }
        });
    }

    /// Ask the Worker to check the session. "Try again" after an outage calls this alone: the Worker kept the uploads.
    async fn submit(self) {
        submit_kyc_verification(
            self.session.get_untracked(),
            self.upload_token.get_untracked(),
            needs_back(&self.id_type.get_untracked()),
            String::new(),
            self.status,
            self.verified_name,
            self.verified,
            self.rejected,
            self.reject_reason,
            self.unavailable,
        )
        .await;
    }

    /// Link the verified session to the account.
    fn run_link(self, state: AppState) {
        self.link_busy.set(true);
        self.link_problem.set(None);
        let session = self.session.get_untracked();
        let claim_secret = self.claim.get_untracked();
        spawn_local(async move {
            match link_and_refresh(state, session, claim_secret, self.linked).await {
                // Used: the secrets are not kept.
                Ok(()) => {
                    self.claim.set(String::new());
                    self.upload_token.set(String::new());
                }
                Err(e) => self.link_problem.set(Some(link_problem_of(&e))),
            }
            self.link_busy.set(false);
        });
    }

    /// Show a QR code for the phone, and watch the session until the phone finishes.
    fn start_handoff(self) {
        self.qr_error.set(String::new());
        self.qr_generation.update(|g| *g += 1);
        let generation = self.qr_generation.get_untracked();
        let id_type = self.id_type.get_untracked();
        spawn_local(async move {
            let (session, upload_token) = match ensure_kyc_session(self.session, self.claim, self.upload_token).await {
                Ok(started) => started,
                Err(e) => {
                    self.qr_error.set(e);
                    return;
                }
            };
            let payload = handoff_payload(&session, &upload_token, &id_type);
            let url = format!("{}/api/kyc/handoff", crate::state::api_base());
            let Ok(request) = gloo_net::http::Request::post(&url).header("Content-Type", "application/json").body(payload.to_string()) else {
                self.qr_error.set("Could not prepare the phone link.".to_string());
                return;
            };
            match request.send().await {
                Ok(resp) if resp.status() == 200 => {
                    let text = resp.text().await.unwrap_or_default();
                    match verify_url_of(&text) {
                        Some(link) => {
                            self.qr_url.set(link);
                            self.qr_open.set(true);
                            self.watch_phone(generation).await;
                        }
                        None => self.qr_error.set("The server did not return a phone link.".to_string()),
                    }
                }
                _ => self.qr_error.set("Could not create a phone link. Check your connection.".to_string()),
            }
        });
    }

    /// Poll the session every three seconds, for as long as the phone link lives (thirty minutes).
    async fn watch_phone(self, generation: u32) {
        let session = self.session.get_untracked();
        // The session's progress needs its upload token, in a header and never in the address.
        let token = self.upload_token.get_untracked();
        for _ in 0..600 {
            gloo_timers::future::TimeoutFuture::new(3000).await;
            if self.qr_generation.get_untracked() != generation || !self.qr_open.get_untracked() {
                return;
            }
            let url = format!("{}/api/kyc/session/{}", crate::state::api_base(), session);
            let Ok(resp) = gloo_net::http::Request::get(&url).header("X-KYC-Token", &token).send().await else { continue };
            if resp.status() != 200 {
                continue;
            }
            let Ok(text) = resp.text().await else { continue };
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            match session_progress(&v) {
                Progress::Waiting => {}
                Progress::Seen { front, back } => {
                    self.front_done.set(front);
                    self.back_done.set(back);
                    self.status.set(if back { "Both sides received. Verifying...".to_string() } else { "Front received on your phone...".to_string() });
                }
                Progress::Verified(name) => {
                    self.verified_name.set(name);
                    self.rejected.set(false);
                    self.status.set(String::new());
                    self.verified.set(true);
                    self.qr_open.set(false);
                    return;
                }
                Progress::Rejected(reason) => {
                    self.reject_reason.set(reason);
                    self.verified.set(false);
                    self.rejected.set(true);
                    self.status.set(String::new());
                    self.qr_open.set(false);
                    return;
                }
            }
        }
        self.qr_open.set(false);
    }
}

/// The first file a file input holds.
fn picked_file(ev: &web_sys::Event) -> Option<web_sys::File> {
    use wasm_bindgen::JsCast;
    let input: web_sys::HtmlInputElement = ev.target()?.dyn_into().ok()?;
    input.files()?.get(0)
}

// -----------------------------------------------------------------------------
// The page
// -----------------------------------------------------------------------------

/// The page: sign in first, already verified, or the KYC session.
#[component]
pub fn VerifyAccountPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let flow = Flow::new();
    let linked = flow.linked;
    let screen = Memo::new(move |_| screen_for(&auth.get(), linked.get()));

    // This page is for one signed-in account and is nobody's search result.
    #[cfg(not(feature = "ssr"))]
    {
        let held = StoredValue::new_local(None::<web_sys::Element>);
        Effect::new(move |_| {
            let doc = document();
            if let Ok(meta) = doc.create_element("meta") {
                let _ = meta.set_attribute("name", "robots");
                let _ = meta.set_attribute("content", "noindex, nofollow");
                if let Ok(Some(head)) = doc.query_selector("head") {
                    let _ = head.append_child(&meta);
                    held.set_value(Some(meta));
                }
            }
        });
        on_cleanup(move || {
            held.update_value(|slot| {
                if let Some(meta) = slot.take() {
                    meta.remove();
                }
            });
        });
    }

    // What the browser remembers can be older than the Worker's answer: read the account once when the page opens.
    {
        let state = app_state.clone();
        let asked = StoredValue::new(false);
        Effect::new(move |_| {
            if auth.get().is_authenticated() && !asked.get_value() {
                asked.set_value(true);
                let state = state.clone();
                spawn_local(async move {
                    let client = ApiClient::new(&state.api_url);
                    if let Ok(user) = api::get_current_user(&client).await {
                        if state.auth.get_untracked().is_authenticated() {
                            state.login(user);
                        }
                    }
                });
            }
        });
    }

    // The moment the session is verified, link it to the account.
    {
        let state = app_state.clone();
        Effect::new(move |_| {
            if flow.verified.get() && !flow.link_started.get_untracked() {
                flow.link_started.set(true);
                flow.run_link(state.clone());
            }
        });
    }

    let state_for_retry = app_state.clone();

    view! {
        <ResultPage>
            {move || match screen.get() {
                Screen::Checking => view! { <CheckingCard /> }.into_any(),
                Screen::SignIn => view! { <SignInCard /> }.into_any(),
                Screen::Verified => view! { <AlreadyVerifiedCard /> }.into_any(),
                Screen::Done => view! { <DoneCard /> }.into_any(),
                Screen::Flow => {
                    let state = state_for_retry.clone();
                    view! { <FlowCard flow=flow state=state /> }.into_any()
                }
            }}
        </ResultPage>
    }
}

#[component]
fn CheckingCard() -> impl IntoView {
    view! {
        <ResultCard tone=Tone::Plain glyph=Glyph::Busy title="Checking your account">
            <p>"One moment."</p>
        </ResultCard>
    }
}

/// Signed out.
#[component]
pub fn SignInCard() -> impl IntoView {
    view! {
        <ResultCard tone=Tone::Info glyph=Glyph::Shield title="Sign in first">
            <p>"Sign in to the account you want to verify, then come back to this page."</p>
            <div class="result-actions">
                <a class="btn btn-primary" href="/login">"Sign in"</a>
            </div>
        </ResultCard>
    }
}

/// Signed in and nothing left to verify.
#[component]
pub fn AlreadyVerifiedCard() -> impl IntoView {
    view! {
        <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Your identity is already verified">
            <p>"There is nothing more to do. Publishing, buying and changing settings are open."</p>
            <div class="result-actions">
                <a class="btn btn-primary" href="/dashboard">"Go to your dashboard"</a>
            </div>
        </ResultCard>
    }
}

/// This visit verified the account.
#[component]
pub fn DoneCard() -> impl IntoView {
    view! {
        <ResultCard tone=Tone::Ok glyph=Glyph::Check title="You are verified">
            <p>"Your identity is verified. Publishing, buying and changing settings are open again."</p>
            <div class="result-actions">
                <a class="btn btn-primary" href="/dashboard">"Go to your dashboard"</a>
            </div>
        </ResultCard>
    }
}

/// The KYC session for a signed-in account that must verify.
#[component]
fn FlowCard(flow: Flow, state: AppState) -> impl IntoView {
    let on_type = move |ev: web_sys::Event| flow.start_over(&event_target_value(&ev));
    let retry_state = state.clone();
    let try_link_again = move |_| flow.run_link(retry_state.clone());

    view! {
        <ResultCard tone=Tone::Info glyph=Glyph::Shield title="Verify your identity" wide=true>
            <p>"Your account was made before identity verification was required. Verify once with a government photo ID and publishing, buying and changing settings open again."</p>
            <p class="result-why">"Your ID images are encrypted and stored securely. They are only used for one-time verification."</p>

            {move || (!flow.problem.get().is_empty()).then(|| view! { <p class="result-problem" role="alert">{flow.problem.get()}</p> })}

            <div class="form-field">
                <label class="form-label" for="verify-account-id-type">"ID type"</label>
                <select id="verify-account-id-type" class="form-input" prop:value=move || flow.id_type.get() on:change=on_type>
                    {ID_TYPES.iter().map(|(value, words)| view! { <option value=*value>{*words}</option> }).collect_view()}
                </select>
            </div>

            <div class="form-field">
                <label class="form-label">
                    {move || if needs_back(&flow.id_type.get()) { "Upload ID: front" } else { "Upload ID document" }}
                </label>
                <div class="identity-file-drop">
                    <input
                        type="file"
                        accept="image/jpeg,image/png"
                        class="identity-file-input"
                        on:change=move |ev| {
                            if let Some(file) = picked_file(&ev) {
                                flow.upload("front", file);
                            }
                        }
                    />
                    <div class="file-drop-label">
                        {move || if flow.front_done.get() {
                            format!("Front: {}", flow.front_name.get())
                        } else if needs_back(&flow.id_type.get()) {
                            "Upload photo of front side".to_string()
                        } else {
                            "Upload photo of your ID".to_string()
                        }}
                    </div>
                </div>
            </div>

            <Show when=move || needs_back(&flow.id_type.get())>
                <div class="form-field">
                    <label class="form-label">"Upload ID: back"</label>
                    <div class="identity-file-drop">
                        <input
                            type="file"
                            accept="image/jpeg,image/png"
                            class="identity-file-input"
                            on:change=move |ev| {
                                if let Some(file) = picked_file(&ev) {
                                    flow.upload("back", file);
                                }
                            }
                        />
                        <div class="file-drop-label">
                            {move || if flow.back_done.get() {
                                format!("Back: {}", flow.back_name.get())
                            } else {
                                "Upload photo of back side".to_string()
                            }}
                        </div>
                    </div>
                </div>
            </Show>

            // A webcam rarely resolves the small print on an ID: the phone's camera does.
            <div class="kyc-handoff">
                <Show
                    when=move || !flow.qr_open.get()
                    fallback=move || view! {
                        <div class="kyc-qr-panel">
                            <p class="kyc-qr-title">"Scan with your phone"</p>
                            <QrCodeSvg data=flow.qr_url.get() size_px=200 />
                            <p class="kyc-qr-hint">"Point your phone camera at the code, then photograph your ID. This page updates on its own when you finish."</p>
                            <p class="kyc-qr-waiting"><span class="kyc-spinner"></span>"Waiting for your phone..."</p>
                            <button type="button" class="btn-link" on:click=move |_| { flow.qr_generation.update(|g| *g += 1); flow.qr_open.set(false); }>"Cancel and upload here instead"</button>
                        </div>
                    }
                >
                    <button type="button" class="btn btn-secondary kyc-phone-btn" on:click=move |_| flow.start_handoff()>"Use my phone camera instead"</button>
                    <p class="form-hint">"Recommended. Phone cameras read ID text far more reliably than a webcam."</p>
                </Show>
                <Show when=move || !flow.qr_error.get().is_empty()>
                    <p class="kyc-qr-error">{move || flow.qr_error.get()}</p>
                </Show>
            </div>

            <div class="kyc-feedback-area">
                <Show when=move || flow.verified.get()>
                    <div class="kyc-status kyc-verified">
                        <span class="kyc-icon">"✓"</span>
                        <div>
                            <strong>"Identity verified"</strong>
                            <p>{move || if flow.link_busy.get() { "Opening your account...".to_string() } else { String::new() }}</p>
                        </div>
                    </div>
                </Show>
                <Show when=move || flow.rejected.get()>
                    <div class="kyc-status kyc-rejected">
                        <span class="kyc-icon">"✗"</span>
                        <div>
                            <strong>"Verification failed"</strong>
                            <p>{move || {
                                let reason = flow.reject_reason.get();
                                if reason.is_empty() { "Please try again with a clearer photo.".to_string() } else { reason }
                            }}</p>
                        </div>
                    </div>
                </Show>
                <Show when=move || !flow.unavailable.get().is_empty()>
                    <div class="kyc-status kyc-unavailable" role="alert">
                        <span class="kyc-icon">"!"</span>
                        <div>
                            <strong>"Verification is temporarily unavailable"</strong>
                            <p>{move || flow.unavailable.get()}</p>
                            <button
                                type="button"
                                class="btn btn-secondary kyc-retry-btn"
                                on:click=move |_| {
                                    // The same session: the Worker kept its uploads, so nothing is uploaded again.
                                    flow.unavailable.set(String::new());
                                    flow.status.set("Submitting for verification...".to_string());
                                    spawn_local(flow.submit());
                                }
                            >"Try again"</button>
                        </div>
                    </div>
                </Show>
                {move || flow.link_problem.get().map(|problem| {
                    let sentence = problem.sentence().to_string();
                    let again = try_link_again.clone();
                    view! {
                        <div class="kyc-status kyc-rejected" role="alert">
                            <span class="kyc-icon">"!"</span>
                            <div>
                                <strong>"Your account could not be verified yet"</strong>
                                <p>{sentence}</p>
                                {match problem {
                                    LinkProblem::TryAgain(_) => view! { <button type="button" class="btn btn-secondary kyc-retry-btn" disabled=move || flow.link_busy.get() on:click=again>"Try again"</button> }.into_any(),
                                    LinkProblem::StartOver(_) => view! { <button type="button" class="btn btn-secondary kyc-retry-btn" on:click=move |_| flow.start_over(&flow.id_type.get_untracked())>"Start over"</button> }.into_any(),
                                    LinkProblem::Stop(_) => ().into_any(),
                                }}
                            </div>
                        </div>
                    }
                })}
                <Show when=move || !flow.status.get().is_empty() && !flow.verified.get() && !flow.rejected.get()>
                    <div class="kyc-status kyc-processing">
                        <span class="kyc-spinner"></span>
                        <span>{move || flow.status.get()}</span>
                    </div>
                </Show>
            </div>
        </ResultCard>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::kyc_banner::{banner_visible, KycBannerBar, KycBannerBarProps, BANNER_BUTTON, BANNER_TEXT};
    use leptos::tachys::view::RenderHtml;

    const USER: &str = r#"{"id":"11111111-1111-4111-8111-111111111111","username":"old_timer","email":null,"created_at":"2026-01-01T00:00:00.000Z"}"#;

    fn user(kyc_required: bool) -> User {
        let mut u: User = serde_json::from_str(USER).unwrap();
        u.kyc_required = kyc_required;
        u
    }

    fn html<V: RenderHtml>(build: impl FnOnce() -> V) -> String {
        let owner = Owner::new();
        owner.with(|| build().to_html())
    }

    /// The part of this file that ships, without the tests.
    fn shipped() -> String {
        let src = include_str!("verify_account.rs").replace("\r\n", "\n");
        src.split("#[cfg(test)]").next().unwrap().to_string()
    }

    fn server(status: u16, body: &str) -> ApiError {
        ApiError::Server { status, message: body.to_string() }
    }

    #[test]
    fn kyc_required_is_read_when_present_and_false_when_absent() {
        let present: User = serde_json::from_str(&USER.replace("\"email\":null", "\"email\":null,\"kyc_required\":true")).unwrap();
        assert!(present.kyc_required);
        let absent: User = serde_json::from_str(USER).unwrap();
        assert!(!absent.kyc_required, "an account the Worker does not flag has nothing to verify");
        let said_false: User = serde_json::from_str(&USER.replace("\"email\":null", "\"email\":null,\"kyc_required\":false")).unwrap();
        assert!(!said_false.kyc_required);
        // the browser keeps the user in localStorage and reads it back
        let kept: User = serde_json::from_str(&serde_json::to_string(&present).unwrap()).unwrap();
        assert!(kept.kyc_required);
    }

    #[test]
    fn the_banner_shows_only_for_a_signed_in_account_that_must_verify() {
        let must = AuthState::Authenticated(user(true));
        let done = AuthState::Authenticated(user(false));
        assert!(banner_visible(&must, false, "/gallery"));
        assert!(!banner_visible(&done, false, "/gallery"), "a verified account sees no banner");
        assert!(!banner_visible(&AuthState::Unauthenticated, false, "/gallery"));
        assert!(!banner_visible(&AuthState::Unknown, false, "/gallery"));
        assert!(!banner_visible(&must, true, "/gallery"), "hidden for the visit");
        assert!(!banner_visible(&must, false, "/verify-account"), "not on the page that verifies");
        assert!(!banner_visible(&must, false, "/verify-account/"));
        assert!(banner_visible(&must, false, "/verify"), "the phone page is another page");
    }

    #[test]
    fn the_banner_says_what_the_account_may_do_and_goes_to_the_verifying_page() {
        let out = html(|| KycBannerBar(KycBannerBarProps { on_dismiss: Callback::new(|_: ()| {}) }));
        assert_eq!(BANNER_TEXT, "Verify your identity to keep using Eustress. You can still sign in and look around, but publishing, buying and changing settings need a verified account.");
        assert!(out.contains(BANNER_TEXT) && out.contains(BANNER_BUTTON));
        assert_eq!(BANNER_BUTTON, "Verify now");
        assert!(out.contains("href=\"/verify-account\""), "{out}");
        for t in [BANNER_TEXT, BANNER_BUTTON] {
            assert!(!t.contains('\u{2014}') && !t.contains('\u{2013}'));
        }
        // mounted once for the whole site, next to the other banner
        let app = include_str!("../app.rs");
        assert!(app.contains("<KycBanner />") && app.contains("use crate::components::kyc_banner::KycBanner;"));
        // hidden for the visit only: the choice is a signal in the component, never stored
        let banner = include_str!("../components/kyc_banner.rs");
        assert!(banner.contains("RwSignal::new(false)") && !banner.contains("Storage"));
    }

    #[test]
    fn the_page_shows_what_the_account_needs() {
        assert_eq!(screen_for(&AuthState::Unknown, false), Screen::Checking);
        assert_eq!(screen_for(&AuthState::Unauthenticated, false), Screen::SignIn);
        assert_eq!(screen_for(&AuthState::Authenticated(user(false)), false), Screen::Verified);
        assert_eq!(screen_for(&AuthState::Authenticated(user(true)), false), Screen::Flow);
        assert_eq!(screen_for(&AuthState::Authenticated(user(false)), true), Screen::Done);
        assert_eq!(screen_for(&AuthState::Authenticated(user(true)), true), Screen::Done);
        assert!(html(|| SignInCard()).contains("href=\"/login\""));
        assert!(html(|| AlreadyVerifiedCard()).contains("already verified"));
        assert!(html(|| DoneCard()).contains("You are verified"));
    }

    #[test]
    fn the_link_call_sends_the_session_to_the_workers_link_route() {
        assert_eq!(LINK_PATH, "/api/kyc/link");
        assert_eq!(link_body("abcd-1234", "s3cr3t"), json!({ "kyc_session_id": "abcd-1234", "kyc_claim_secret": "s3cr3t" }));
        assert!(shipped().contains("client.post(LINK_PATH, &link_body(session_id, claim_secret))"));
        let reply: LinkReply = serde_json::from_str(r#"{"ok":true,"already_verified":true}"#).unwrap();
        assert!(reply.ok && reply.already_verified);
    }

    #[test]
    fn the_account_is_read_again_after_the_link_so_the_banner_clears() {
        assert!(!cleared(user(true)).kyc_required);
        let src = shipped();
        let body = src.split("async fn link_and_refresh(").nth(1).unwrap().split("\n}\n").next().unwrap();
        let link = body.find("link_session(").expect("it links");
        let set = body.find("linked.set(true)").expect("it marks the page done");
        let read = body.find("api::get_current_user(").expect("it reads /api/auth/me again");
        let login = body.find("state.login(user)").expect("it stores the fresh account");
        assert!(link < set && set < read && read < login);
        assert!(body.contains("state.login(cleared(user))"), "and if that read fails, the account is still the one the Worker verified");
        // the page links the moment the session is verified, and only once
        assert!(src.contains("flow.verified.get() && !flow.link_started.get_untracked()"));
    }

    #[test]
    fn the_claim_secret_lives_in_memory_and_goes_only_to_the_link_call() {
        assert_eq!(link_body("abcd-1234", "s3cr3t-value")[CLAIM_SECRET_FIELD], "s3cr3t-value");
        assert_eq!(link_body("abcd-1234", "s3cr3t-value")["kyc_session_id"], "abcd-1234");
        let src = shipped();
        for banned in ["LocalStorage", "local_storage", "session_storage", "SessionStorage", "log::", "console", "println!", "dbg!", "set_href", "history"] {
            assert!(!src.contains(banned), "the page must not use {banned}");
        }
        // no line that touches the claim secret logs it, formats it into text or stores it
        for line in src.lines().filter(|l| l.contains("claim")) {
            for bad in ["log::", "console", "Storage", "storage", "format!(", "{:?}", "{claim"] {
                assert!(!line.contains(bad), "{bad} next to the claim secret: {line}");
            }
        }
        // it is read once, for the link call, and cleared once the link worked
        assert_eq!(src.matches("self.claim.get_untracked()").count(), 1);
        let after = src.split("Ok(()) => {").nth(1).unwrap().split("Err(e) => self.link_problem").next().unwrap();
        assert!(after.contains("self.claim.set(String::new())") && after.contains("self.upload_token.set(String::new())"));
        // the phone's hand-off body carries the upload token and no claim secret
        let hand = handoff_payload("abcd-1234", "tok", "passport");
        assert!(hand.get(CLAIM_SECRET_FIELD).is_none() && hand["upload_token"] == "tok");
        let handoff_code = src.split("fn start_handoff(self) {").nth(1).unwrap().split("async fn watch_phone").next().unwrap();
        assert!(!handoff_code.contains("self.claim.get") && !handoff_code.contains("CLAIM_SECRET_FIELD"), "the hand-off never reads the claim secret");
        // the phone's progress is read with the upload token in a header, never in the address
        let watch = src.split("async fn watch_phone").nth(1).unwrap().split("\n    }\n").next().unwrap();
        assert!(watch.contains(".header(\"X-KYC-Token\", &token)") && !watch.contains("?token") && !watch.contains("?t="));
        // the field name is written once, under the marker the KYC builder looks for
        assert_eq!(src.matches("\"kyc_claim_secret\"").count(), 1);
        assert!(src.contains("// CONTRACT: claim secret, confirm with the KYC builder"));
    }

    #[test]
    fn a_kyc_required_refusal_says_to_verify_and_the_ids_conflict_says_who_to_ask() {
        let gate = server(403, r#"{"error":"Verify your identity to keep using your account","code":"kyc_required","reason":"Your account was created before identity verification was required."}"#);
        assert_eq!(api::failure_sentence(&gate), "Verify your identity to do this.");
        assert!(api::is_kyc_required(&gate));
        let other = server(403, r#"{"error":"Admin access required"}"#);
        assert_eq!(api::failure_sentence(&other), "Admin access required", "any other refusal keeps the Worker's words");
        assert!(!api::is_kyc_required(&other));
        assert!(!api::is_kyc_required(&server(500, r#"{"code":"kyc_required"}"#)));
        let in_use = server(409, r#"{"error":"This ID is already verified on another account","code":"kyc_id_in_use"}"#);
        assert_eq!(api::failure_sentence(&in_use), "This ID is already linked to another Eustress account. Contact support@eustress.dev.");
        assert_eq!(api::kyc_sentence(409, r#"{"code":"kyc_session_used"}"#), None);
        // the commerce calls unwrap the body themselves; they ask the same helper first
        let commerce = include_str!("../api/commerce.rs");
        assert!(commerce.contains("crate::api::kyc_sentence(status, &text)") && commerce.contains("let message = identity_words.unwrap_or(message);"), "and use its answer in place of the unwrapped body");
        assert!(api::is_kyc_required(&server(403, "Verify your identity to do this.")));
        for t in [api::KYC_REQUIRED_SENTENCE, api::KYC_ID_IN_USE_SENTENCE] {
            assert!(!t.contains('\u{2014}') && !t.contains('\u{2013}'));
        }
    }

    #[test]
    fn every_link_refusal_is_in_the_workers_words_and_says_what_to_do() {
        let used = link_problem_of(&server(409, r#"{"error":"This ID is already verified on another account","code":"kyc_id_in_use"}"#));
        assert_eq!(used, LinkProblem::Stop("This ID is already linked to another Eustress account. Contact support@eustress.dev.".to_string()));
        let old = link_problem_of(&server(403, r#"{"error":"Identity verification is required","code":"kyc_required","reason":"Your identity verification is more than 24 hours old. Verify your ID again."}"#));
        assert_eq!(old, LinkProblem::StartOver("Your identity verification is more than 24 hours old. Verify your ID again.".to_string()));
        let spent = link_problem_of(&server(409, r#"{"error":"This identity verification was already used","code":"kyc_session_used"}"#));
        assert_eq!(spent, LinkProblem::StartOver("This identity verification was already used".to_string()));
        let down = link_problem_of(&server(503, r#"{"error":"Verification is temporarily unavailable. Try again in a minute.","code":"kyc_unavailable"}"#));
        assert_eq!(down, LinkProblem::TryAgain("Verification is temporarily unavailable. Try again in a minute.".to_string()));
        let unset = link_problem_of(&server(503, r#"{"error":"Identity verification is not set up yet.","code":"kyc_unconfigured"}"#));
        assert_eq!(unset, LinkProblem::Stop("Identity verification is not set up yet.".to_string()), "a retry does not fix a missing secret");
        assert!(matches!(link_problem_of(&ApiError::Network("offline".to_string())), LinkProblem::TryAgain(_)));
        assert!(matches!(link_problem_of(&ApiError::Unauthorized), LinkProblem::Stop(s) if s.contains("Sign in again")));
    }

    #[test]
    fn the_upload_and_the_phone_use_the_workers_shapes() {
        assert_eq!(
            upload_problem(r#"Upload error (415): {"error":"Please upload a JPEG or PNG photo of your ID.","received":"image/webp"}"#),
            "Please upload a JPEG or PNG photo of your ID."
        );
        assert_eq!(upload_problem("Upload failed: network"), "Upload failed: network");
        assert_eq!(handoff_payload("abcd-1234", "tok", "passport"), json!({"session_id":"abcd-1234","upload_token":"tok","id_type":"passport","needs_back":false}));
        assert_eq!(handoff_payload("abcd-1234", "tok", "national_id")["needs_back"], true);
        assert_eq!(verify_url_of(r#"{"verify_url":"https://eustress.dev/verify?s=abcd-1234","expires_in":1800}"#).as_deref(), Some("https://eustress.dev/verify?s=abcd-1234"));
        assert_eq!(verify_url_of(r#"{"verify_url":""}"#), None);
        assert_eq!(session_progress(&json!({"status":"pending","front_uploaded":false,"back_uploaded":false})), Progress::Waiting);
        assert_eq!(session_progress(&json!({"status":"uploaded","front_uploaded":true,"back_uploaded":false})), Progress::Seen { front: true, back: false });
        assert_eq!(session_progress(&json!({"status":"verified","ocr_name":"Old Timer"})), Progress::Verified("Old Timer".to_string()));
        assert_eq!(session_progress(&json!({"status":"rejected","reason":"Blurry"})), Progress::Rejected("Blurry".to_string()));
        assert!(needs_back("national_id") && needs_back("drivers_license") && !needs_back("passport"));
    }

    #[test]
    fn the_page_reuses_the_sign_up_upload_and_submit_and_writes_no_upload_of_its_own() {
        let src = shipped();
        assert!(src.contains("upload_id_document(&file, side,") && src.contains("submit_kyc_verification("));
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join(" ");
        assert!(!code.contains("/api/kyc/upload") && !code.contains("/api/kyc/submit") && !code.contains("append_with_blob"));
        let login = include_str!("login.rs");
        assert!(login.contains("pub(crate) async fn upload_id_document(") && login.contains("pub(crate) async fn submit_kyc_verification("));
        // and its session start: the page asks the Worker for one through the same helper, and writes no /api/kyc/start call of its own
        assert!(login.contains("pub(crate) async fn ensure_kyc_session(") && src.matches("ensure_kyc_session(self.session, self.claim, self.upload_token)").count() == 2);
        assert!(!code.contains("/api/kyc/start"));
        // "Try again" after an outage submits the same session and uploads nothing
        let retry = src.split("kyc-retry-btn\"\n                                on:click").nth(1).unwrap().split(">\"Try again\"").next().unwrap();
        assert!(retry.contains("flow.submit()") && !retry.contains("upload("));
    }

    #[test]
    fn the_route_the_module_and_the_search_header_exist() {
        let app = include_str!("../app.rs");
        assert!(app.contains(r#"<Route path=path!("/verify-account") view=VerifyAccountPage />"#));
        assert!(app.contains("verify_account::VerifyAccountPage,"));
        assert!(include_str!("mod.rs").contains("pub mod verify_account;"));
        assert!(include_str!("../components/mod.rs").contains("pub mod kyc_banner;"));
        let headers = include_str!("../../_headers").replace("\r\n", "\n");
        let at = headers.find("\n/verify-account\n").expect("a rule for /verify-account") + 1;
        assert!(headers[at..].lines().take(3).any(|l| l.trim().eq_ignore_ascii_case("X-Robots-Tag: noindex, nofollow")));
        // and the page asks crawlers that run scripts the same
        assert!(shipped().contains("\"noindex, nofollow\""));
    }
}
