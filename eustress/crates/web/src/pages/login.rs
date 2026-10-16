// =============================================================================
// Eustress Web - Login Page
// =============================================================================
// Two tabs: Login (load identity.toml) and Register (create new identity)
// Authentication: Eustress Identity (Ed25519 challenge-response)
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use crate::api::{self, ApiClient};
use crate::components::{CentralNav, QrCodeSvg};
use crate::state::{AppState, User};
use crate::webauthn;

/// Global minimum age. Bliss pays out real USD, so accounts are gated on age
/// before an identity is ever created. The server enforces this again against
/// the date of birth read off the document; this copy only spares the applicant
/// a pointless round trip.
const MIN_AGE: u32 = 18;

/// Whole years from a `YYYY-MM-DD` date to today.
///
/// Returns `None` for an absent, malformed, or impossible date so callers can
/// distinguish "not filled in yet" from "too young".
/// The query string of the page in front of the visitor. The prerender has no
/// page.
#[cfg(not(feature = "ssr"))]
fn current_query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

#[cfg(feature = "ssr")]
fn current_query() -> String {
    String::new()
}

/// Whether the query asks for the Create Account tab: `?register=1`.
fn wants_register(search: &str) -> bool {
    search.trim_start_matches('?').split('&').any(|pair| pair == "register=1")
}

fn age_from_dob(dob: &str) -> Option<u32> {
    let d = dob.trim();
    let parts: Vec<&str> = d.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    use chrono::Datelike;

    let birth = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let today = chrono::Local::now().date_naive();
    if birth > today {
        return None; // born in the future
    }

    let mut age = today.year() - year;
    // Subtract a year when this year's birthday has not happened yet.
    if (today.month(), today.day()) < (month, day) {
        age -= 1;
    }
    u32::try_from(age.max(0)).ok()
}

/// The account as a passkey sign-in left it. The answer of `/api/passkeys/login` is the same shape as
/// the identity-file sign-in's, so this session is marked here; `/api/auth/me` says the same on the
/// next page load. A passkey session cannot change keys (Settings, Security).
pub(crate) fn as_passkey_session(mut user: User) -> User {
    user.auth_method = Some("passkey".to_string());
    user
}

/// The button's words. It signs in with a passkey the device holds for this site.
const PASSKEY_BUTTON: &str = "Sign in with a passkey";

/// Login/Register page with tabs.
#[component]
pub fn LoginPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let navigate = use_navigate();

    // Tab state. `/login?register=1` opens on Create Account.
    let is_register = RwSignal::new(wants_register(&current_query()));

    let loading = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);

    // Login: identity auth state
    let identity_public_key = RwSignal::new(String::new());
    let identity_private_key = RwSignal::new(String::new());
    let identity_username = RwSignal::new(String::new());
    let identity_file_loaded = RwSignal::new(false);
    let identity_status = RwSignal::new(String::new());

    // Register: form state
    let reg_username = RwSignal::new(String::new());
    let reg_email = RwSignal::new(String::new());
    let reg_birthday = RwSignal::new(String::new());
    let reg_id_type = RwSignal::new("passport".to_string());
    let reg_id_number = RwSignal::new(String::new());
    let reg_id_front_name = RwSignal::new(String::new());
    let reg_id_front_uploaded = RwSignal::new(false);
    let reg_id_back_name = RwSignal::new(String::new());
    let reg_id_back_uploaded = RwSignal::new(false);
    let reg_id_needs_back = RwSignal::new(false); // passport = no back needed
    let reg_step = RwSignal::new(1_u32); // 1=form, 2=verify, 3=done
    // The verification session, from POST /api/kyc/start on first use: a
    // server-chosen id, its claim secret and its upload-only token. Memory only:
    // never storage, never a log, never a URL. The upload token uploads, submits
    // and starts the phone hand-off; the claim secret goes to registration and
    // nowhere else, so the phone link can never make an account.
    let kyc_session_id = RwSignal::new(String::new());
    let kyc_claim_secret = RwSignal::new(String::new());
    let kyc_upload_token = RwSignal::new(String::new());
    let kyc_status_text = RwSignal::new(String::new()); // "Verifying...", "Verified as John Doe", "Rejected"
    let kyc_verified = RwSignal::new(false);
    let kyc_rejected = RwSignal::new(false);
    let kyc_verified_name = RwSignal::new(String::new()); // name extracted by Grok
    let kyc_reject_reason = RwSignal::new(String::new());
    // The Worker's sentence when verification could not run (503 kyc_unavailable):
    // not a verdict. The uploads are kept, and "Try again" submits the same session.
    let kyc_unavailable = RwSignal::new(String::new());

    // Desktop -> mobile handoff. Desktops rarely have a camera good enough to
    // resolve the small print on an ID, so the applicant can scan a QR code and
    // finish capture on their phone while this tab polls for the result.
    let qr_open = RwSignal::new(false);
    let qr_url = RwSignal::new(String::new());
    let qr_error = RwSignal::new(String::new());
    // Drives the polling Effect; bumped once the handoff is live.
    let qr_polling = RwSignal::new(false);

    // Register the handoff, then show the QR. The applicant's name and birthday
    // are stored server-side against the session id so they never travel
    // through the QR payload or the phone's URL bar.
    let start_handoff = move || {
        qr_error.set(String::new());
        let id_type = reg_id_type.get_untracked();
        let needs_back = reg_id_needs_back.get_untracked();
        let full_name = reg_username.get_untracked();
        let birthday = reg_birthday.get_untracked();

        spawn_local(async move {
            let (session, upload_token) = match ensure_kyc_session(kyc_session_id, kyc_claim_secret, kyc_upload_token).await {
                Ok(started) => started,
                Err(e) => {
                    qr_error.set(e);
                    return;
                }
            };
            // The upload token, never the claim secret: the phone can upload, never register.
            let payload = serde_json::json!({
                "session_id": session,
                "upload_token": upload_token,
                "id_type": id_type,
                "needs_back": needs_back,
                "full_name": full_name,
                "birthday": birthday,
                "username": full_name,
            });
            let result = gloo_net::http::Request::post(&format!("{}/api/kyc/handoff", crate::state::api_base()))
                .header("Content-Type", "application/json")
                .body(payload.to_string())
                .map_err(|_| ())
                .ok();

            let Some(req) = result else {
                qr_error.set("Could not prepare the phone link.".into());
                return;
            };

            match req.send().await {
                Ok(resp) if resp.status() == 200 => {
                    let text = resp.text().await.unwrap_or_default();
                    let url = serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|v| v.get("verify_url").and_then(|s| s.as_str()).map(String::from));
                    match url {
                        Some(u) => {
                            qr_url.set(u);
                            qr_open.set(true);
                            qr_polling.set(true);
                        }
                        None => qr_error.set("The server did not return a phone link.".into()),
                    }
                }
                _ => qr_error.set("Could not create a phone link. Check your connection.".into()),
            }
        });
    };

    // Watch the handoff session until the phone finishes. Polling (rather than
    // a socket) keeps this working through the same CDN as every other request.
    Effect::new(move |_| {
        if !qr_polling.get() {
            return;
        }
        let session = kyc_session_id.get_untracked();
        // The session's details need its token, sent in a header (never the URL).
        let token = kyc_upload_token.get_untracked();

        spawn_local(async move {
            // 30 minutes at 3s intervals matches the handoff TTL, so the loop
            // cannot outlive the session it is watching.
            for _ in 0..600 {
                gloo_timers::future::TimeoutFuture::new(3000).await;

                // Stop if the applicant closed the panel or moved on.
                if !qr_polling.get_untracked() {
                    return;
                }

                let url = format!("{}/api/kyc/session/{}", crate::state::api_base(), session);
                let Ok(resp) = gloo_net::http::Request::get(&url).header("X-KYC-Token", &token).send().await else {
                    continue;
                };
                if resp.status() != 200 {
                    continue;
                }
                let Ok(text) = resp.text().await else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };

                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                let front = v.get("front_uploaded").and_then(|b| b.as_bool()).unwrap_or(false);
                let back = v.get("back_uploaded").and_then(|b| b.as_bool()).unwrap_or(false);

                // Mirror the phone's progress into the desktop form so the
                // step-2 gating sees the same state as a desktop upload would.
                if front {
                    reg_id_front_uploaded.set(true);
                    if reg_id_front_name.get_untracked().is_empty() {
                        reg_id_front_name.set("Captured on phone".into());
                    }
                }
                if back {
                    reg_id_back_uploaded.set(true);
                    if reg_id_back_name.get_untracked().is_empty() {
                        reg_id_back_name.set("Captured on phone".into());
                    }
                }

                match status {
                    "verified" => {
                        kyc_verified.set(true);
                        kyc_rejected.set(false);
                        kyc_verified_name.set(
                            v.get("ocr_name").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        );
                        kyc_status_text.set(String::new());
                        qr_polling.set(false);
                        qr_open.set(false);
                        return;
                    }
                    "rejected" => {
                        kyc_verified.set(false);
                        kyc_rejected.set(true);
                        kyc_reject_reason.set(
                            v.get("reason").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        );
                        kyc_status_text.set(String::new());
                        qr_polling.set(false);
                        return;
                    }
                    _ if front => {
                        kyc_status_text.set(if back {
                            "Both sides received. Verifying...".into()
                        } else {
                            "Front received on your phone...".into()
                        });
                    }
                    _ => {}
                }
            }
            qr_polling.set(false);
        });
    });

    // Jurisdiction detection (from Cloudflare cdn-cgi/trace)
    let detected_country = RwSignal::new("US".to_string());
    let accepted_ids = RwSignal::new(vec![
        "Passport".to_string(),
        "Driver's licence".to_string(),
        "National ID".to_string(),
    ]);
    let jurisdiction_name = RwSignal::new("Detecting...".to_string());

    // Detect jurisdiction on load — try Cloudflare trace, fallback to API
    spawn_local(async move {
        // Try the API endpoint first (works everywhere, has cf-ipcountry)
        if let Ok(resp) = gloo_net::http::Request::get(&format!("{}/api/kyc/jurisdiction", crate::state::api_base())).send().await {
            if resp.ok() {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(iso2) = data.get("iso2").and_then(|v| v.as_str()) {
                        detected_country.set(iso2.to_string());
                        if let Some(name) = data.get("name").and_then(|v| v.as_str()) {
                            jurisdiction_name.set(name.to_string());
                        } else {
                            jurisdiction_name.set(iso2.to_string());
                        }
                        if let Some(ids_arr) = data.get("accepted_ids").and_then(|v| v.as_array()) {
                            let ids: Vec<String> = ids_arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                            if !ids.is_empty() {
                                accepted_ids.set(ids);
                            }
                        }
                        return; // Got jurisdiction from API
                    }
                }
            }
        }

        // Fallback: try cdn-cgi/trace (only works when served from Cloudflare)
        if let Ok(resp) = gloo_net::http::Request::get("/cdn-cgi/trace").send().await {
            if let Ok(text) = resp.text().await {
                for line in text.lines() {
                    if let Some(loc) = line.strip_prefix("loc=") {
                        detected_country.set(loc.to_string());
                        // Load jurisdiction-specific IDs
                        let ids = match loc {
                            "CA" => vec!["Passport", "Provincial driver's licence", "Canadian citizenship/PR card", "Health card"],
                            "US" => vec!["Passport", "State driver's licence", "State ID", "Military ID"],
                            "GB" => vec!["Passport", "National ID", "Armed Forces ID", "Driving licence"],
                            "AU" => vec!["Passport", "Driving licence", "Birth cert (under 21)"],
                            "DE" => vec!["Passport", "National ID (Personalausweis)", "Driving licence"],
                            "FR" => vec!["Passport", "National ID", "Driving licence"],
                            "JP" => vec!["Passport", "Driving licence", "My Number card", "Residence card"],
                            "IN" => vec!["Passport", "National ID", "Driving licence", "PAN", "Aadhaar"],
                            "SG" => vec!["Passport", "National ID", "Driving licence"],
                            "CH" => vec!["Passport", "National ID", "Swiss ID card"],
                            "NZ" => vec!["Passport", "Driver's licence", "18+ photo ID"],
                            "FI" | "SE" | "NO" | "DK" => vec!["Passport", "Driving licence", "National ID"],
                            _ => vec!["Passport", "National ID", "Driver's licence"],
                        };
                        accepted_ids.set(ids.iter().map(|s| s.to_string()).collect());
                        jurisdiction_name.set(loc.to_string());
                        break;
                    }
                }
            }
        }
    });

    // Stored values for closure access
    let api_url_stored = StoredValue::new(app_state.api_url.clone());
    let app_state_stored = StoredValue::new(app_state.clone());
    let nav_fn = StoredValue::new(navigate.clone());

    // Passkeys: the button shows only where the browser has WebAuthn, and that is known only in the
    // browser, so the prerendered page never shows it.
    let passkey_supported = RwSignal::new(false);
    Effect::new(move |_| passkey_supported.set(webauthn::webauthn_available()));
    let passkey_sign_in = move |_| {
        loading.set(true);
        error.set(None);
        let api_url = api_url_stored.get_value();
        let app_state_c = app_state_stored.get_value();
        let nav = nav_fn.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let outcome: Result<api::AuthResponse, String> = async {
                let begin = api::passkey_login_begin(&client).await.map_err(|e| api::failure_sentence(&e))?;
                let options = webauthn::RequestOptions::from_begin(&begin)?;
                let asserted = webauthn::get_credential(&options).await?;
                let body = webauthn::login_body(&begin.challenge, &asserted);
                api::passkey_login(&client, &body).await.map_err(|e| api::failure_sentence(&e))
            }
            .await;
            loading.set(false);
            match outcome {
                Ok(response) => {
                    app_state_c.login_with_token(response.token, as_passkey_session(response.user));
                    nav("/dashboard", Default::default());
                }
                Err(why) => error.set(Some(why)),
            }
        });
    };

    view! {
        <div class="page page-login">
            <CentralNav active="".to_string() />

            <div class="login-background">
                <div class="bg-grid"></div>
                <div class="bg-glow bg-glow-1"></div>
                <div class="bg-glow bg-glow-2"></div>
            </div>

            <div class="login-wrapper">
                // Left side - showcase: the build plate, nine pillars around the core; style/login.css draws and animates it
                <div class="login-showcase ls-showcase">
                    <div class="ls-stage" aria-hidden="true">
                        <div class="ls-plate">
                            <div class="ls-pillar" style="--i:0;--d:0;--h:2"><span class="ls-top"><img src="/assets/icons/physics.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:1;--d:1;--h:1"><span class="ls-top"><img src="/assets/icons/code.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:2;--d:2;--h:3"><span class="ls-top"><img src="/assets/icons/network.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:7;--d:1;--h:1"><span class="ls-top"><img src="/assets/icons/brain.svg" alt="" /></span></div>
                            <div class="ls-pillar core" style="--i:8;--d:2;--h:4"><span class="ls-top"><img src="/assets/icons/eustress-gear.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:3;--d:3;--h:2"><span class="ls-top"><img src="/assets/icons/perspective.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:6;--d:2;--h:3"><span class="ls-top"><img src="/assets/icons/cube.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:5;--d:3;--h:1"><span class="ls-top"><img src="/assets/icons/audio.svg" alt="" /></span></div>
                            <div class="ls-pillar" style="--i:4;--d:4;--h:2"><span class="ls-top"><img src="/assets/icons/animation.svg" alt="" /></span></div>
                        </div>
                    </div>
                    <ul class="ls-readout" aria-label="What a Space is built from">
                        <li style="--i:0"><b>"01"</b>" Physics"</li>
                        <li style="--i:1"><b>"02"</b>" Scripting"</li>
                        <li style="--i:2"><b>"03"</b>" Multiplayer"</li>
                        <li style="--i:3"><b>"04"</b>" 2D and 3D"</li>
                        <li style="--i:4"><b>"05"</b>" Animation"</li>
                        <li style="--i:5"><b>"06"</b>" Audio"</li>
                        <li style="--i:6"><b>"07"</b>" Building"</li>
                        <li style="--i:7"><b>"08"</b>" AI"</li>
                    </ul>
                    <div class="showcase-tagline">
                        <h2>"Create Excellence"</h2>
                        <p>"Build simulations, connect real data, and run them in the engine."</p>
                    </div>
                </div>

                // Right side - login card
                <div class="login-container">
                    <div class="login-card">
                        <div class="login-header">
                            <img src="/assets/logo.svg" alt="Eustress Engine" class="login-logo" />
                        </div>

                        // Tab switcher
                        <div class="login-tabs">
                            <button
                                type="button"
                                class=move || if !is_register.get() { "login-tab active" } else { "login-tab" }
                                on:click=move |_| { is_register.set(false); error.set(None); }
                            >"Sign In"</button>
                            <button
                                type="button"
                                class=move || if is_register.get() { "login-tab active" } else { "login-tab" }
                                on:click=move |_| { is_register.set(true); error.set(None); }
                            >"Register"</button>
                        </div>

                        // Error banner
                        {move || error.get().map(|e| view! {
                            <div class="form-error-banner">{e}</div>
                        })}

                        // ── SIGN IN TAB ──
                        <div class="auth-panel" class:hidden=move || is_register.get()>
                            <p class="identity-desc">
                                "Load your identity file to sign in. There is no password."
                            </p>

                            // File picker
                            <div class="form-field">
                                <label class="form-label">"Identity File"</label>
                                <div class="identity-file-drop">
                                    <input
                                        type="file"
                                        accept=".toml"
                                        class="identity-file-input"
                                        on:change=move |e| {
                                            use wasm_bindgen::JsCast;
                                            let input: web_sys::HtmlInputElement = e.target().unwrap().unchecked_into();
                                            if let Some(files) = input.files() {
                                                if let Some(file) = files.get(0) {
                                                    let reader = web_sys::FileReader::new().unwrap();
                                                    let reader_clone = reader.clone();
                                                    let onload = wasm_bindgen::closure::Closure::wrap(Box::new(move |_: web_sys::Event| {
                                                        if let Ok(result) = reader_clone.result() {
                                                            let text = result.as_string().unwrap_or_default();
                                                            let mut pub_key = String::new();
                                                            let mut priv_key = String::new();
                                                            let mut uname = String::new();

                                                            for line in text.lines() {
                                                                let trimmed = line.trim();
                                                                if trimmed.starts_with("public_key") {
                                                                    if let Some(val) = extract_toml_value(trimmed) {
                                                                        pub_key = val;
                                                                    }
                                                                } else if trimmed.starts_with("private_key") || trimmed.starts_with("secret_key") {
                                                                    if let Some(val) = extract_toml_value(trimmed) {
                                                                        priv_key = val;
                                                                    }
                                                                } else if trimmed.starts_with("username") || trimmed.starts_with("display_name") {
                                                                    if let Some(val) = extract_toml_value(trimmed) {
                                                                        uname = val;
                                                                    }
                                                                }
                                                            }

                                                            if !pub_key.is_empty() && !priv_key.is_empty() {
                                                                identity_public_key.set(pub_key);
                                                                identity_private_key.set(priv_key);
                                                                identity_username.set(uname);
                                                                identity_file_loaded.set(true);
                                                                identity_status.set("Identity loaded".to_string());
                                                                error.set(None);
                                                            } else {
                                                                error.set(Some("Invalid identity.toml: missing public_key or private_key.".to_string()));
                                                                identity_file_loaded.set(false);
                                                            }
                                                        }
                                                    }) as Box<dyn FnMut(_)>);
                                                    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                                                    onload.forget();
                                                    let _ = reader.read_as_text(&file);
                                                }
                                            }
                                        }
                                    />
                                    <div class="file-drop-label">
                                        {move || {
                                            if identity_file_loaded.get() {
                                                "Identity loaded. Ready to sign in."
                                            } else {
                                                "Choose eustress-username.toml or drag here"
                                            }
                                        }}
                                    </div>
                                </div>
                                <p class="form-hint"><a href="/replacement-key">"Lost your identity file?"</a></p>
                            </div>

                            // Identity info
                            {move || identity_file_loaded.get().then(|| {
                                let pk = identity_public_key.get();
                                let pk_short = if pk.len() > 16 {
                                    format!("{}...{}", &pk[..8], &pk[pk.len()-8..])
                                } else {
                                    pk.clone()
                                };
                                let uname = identity_username.get();
                                view! {
                                    <div class="identity-info">
                                        <div class="identity-field">
                                            <span class="field-label">"Public Key"</span>
                                            <span class="field-value mono">{pk_short}</span>
                                        </div>
                                        {if !uname.is_empty() {
                                            Some(view! {
                                                <div class="identity-field">
                                                    <span class="field-label">"Username"</span>
                                                    <span class="field-value">{uname}</span>
                                                </div>
                                            })
                                        } else {
                                            None
                                        }}
                                        <div class="identity-field">
                                            <span class="field-label">"Status"</span>
                                            <span class="field-value identity-status-ok">{identity_status.get()}</span>
                                        </div>
                                    </div>
                                }
                            })}

                            // Sign in button
                            <button
                                type="button"
                                class="btn btn-primary"
                                disabled=move || loading.get() || !identity_file_loaded.get()
                                on:click=move |_| {
                                    let pub_key = identity_public_key.get();
                                    let priv_key = identity_private_key.get();

                                    loading.set(true);
                                    error.set(None);
                                    identity_status.set("Requesting challenge...".to_string());

                                    let api_url = api_url_stored.get_value();
                                    let app_state_c = app_state_stored.get_value();
                                    let nav = nav_fn.get_value();
                                    spawn_local(async move {
                                        let client = ApiClient::new(&api_url);

                                        let challenge_resp = match api::request_challenge(&client, &pub_key).await {
                                            Ok(c) => c,
                                            Err(e) => {
                                                loading.set(false);
                                                error.set(Some(format!("Challenge failed: {}", e)));
                                                identity_status.set("Challenge failed".to_string());
                                                return;
                                            }
                                        };

                                        identity_status.set("Signing challenge...".to_string());

                                        // The signature covers the login message, which names
                                        // the API this sign-in is for; the Worker checks the
                                        // origin the request actually reached.
                                        let message = api::login_message(&api::api_origin(&api_url), &challenge_resp.challenge);
                                        let signature = match sign_challenge_ed25519(&priv_key, &message) {
                                            Ok(sig) => sig,
                                            Err(e) => {
                                                loading.set(false);
                                                error.set(Some(format!("Signing failed: {}", e)));
                                                identity_status.set("Signing failed".to_string());
                                                return;
                                            }
                                        };

                                        identity_status.set("Verifying...".to_string());

                                        match api::verify_challenge(&client, &pub_key, &challenge_resp.challenge, &signature).await {
                                            Ok(response) => {
                                                loading.set(false);
                                                identity_status.set("Authenticated".to_string());
                                                app_state_c.login_with_token(response.token, response.user);
                                                nav("/dashboard", Default::default());
                                            }
                                            Err(e) => {
                                                loading.set(false);
                                                error.set(Some(format!("Verification failed: {}", e)));
                                                identity_status.set("Verification failed".to_string());
                                            }
                                        }
                                    });
                                }
                            >
                                {move || if loading.get() { "Authenticating..." } else { "Sign In" }}
                            </button>

                            <Show when=move || passkey_supported.get()>
                                <div class="passkey-signin">
                                    <p class="form-hint">"Added a passkey to your account on this device? Use it instead of the file."</p>
                                    <button
                                        type="button"
                                        class="btn btn-secondary"
                                        disabled=move || loading.get()
                                        on:click=passkey_sign_in
                                    >
                                        {PASSKEY_BUTTON}
                                    </button>
                                </div>
                            </Show>

                            <div class="identity-help">
                                <p>
                                    "Your private key never leaves the browser. "
                                    "The server sends a random challenge; you sign it locally."
                                </p>
                            </div>
                        </div>

                        // ── REGISTER TAB ──
                        <div class="auth-panel" class:hidden=move || !is_register.get()>
                            // Step 1: Basic info
                            {move || (reg_step.get() == 1).then(|| view! {
                                <div class="register-step">
                                    <p class="step-indicator">"Step 1 of 3: Basic Info"</p>

                                    <div class="form-field">
                                        <label class="form-label">
                                            "Username"
                                            <span class="required">"*"</span>
                                        </label>
                                        <input
                                            type="text"
                                            class="form-input"
                                            placeholder="Choose a unique username"
                                            prop:value=move || reg_username.get()
                                            on:input=move |e| reg_username.set(event_target_value(&e))
                                        />
                                    </div>

                                    <div class="form-field">
                                        <label class="form-label">
                                            "Email"
                                            <span class="required">"*"</span>
                                        </label>
                                        <input
                                            type="email"
                                            class="form-input"
                                            placeholder="you@example.com"
                                            prop:value=move || reg_email.get()
                                            on:input=move |e| reg_email.set(event_target_value(&e))
                                        />
                                        <p class="form-hint">"A backup copy of your identity file will be emailed to you."</p>
                                    </div>

                                    <div class="form-field">
                                        <label class="form-label">
                                            "Date of Birth"
                                            <span class="required">"*"</span>
                                        </label>
                                        <input
                                            type="date"
                                            class="form-input"
                                            prop:value=move || reg_birthday.get()
                                            on:input=move |e| reg_birthday.set(event_target_value(&e))
                                        />
                                        // Age is a hard gate, not a warning: Bliss pays
                                        // out real money, so say so at the point of entry
                                        // rather than after documents are uploaded.
                                        <Show
                                            when=move || {
                                                let b = reg_birthday.get();
                                                !b.is_empty() && age_from_dob(&b).map_or(true, |a| a < MIN_AGE)
                                            }
                                            fallback=|| view! {
                                                <p class="form-hint">
                                                    "You must be "{MIN_AGE}" or older. This is checked against your ID."
                                                </p>
                                            }
                                        >
                                            <p class="form-error-inline">
                                                {move || match age_from_dob(&reg_birthday.get()) {
                                                    Some(_) => format!(
                                                        "You must be at least {} years old to create an account.",
                                                        MIN_AGE
                                                    ),
                                                    None => "Enter a valid date of birth.".to_string(),
                                                }}
                                            </p>
                                        </Show>
                                    </div>

                                    <button
                                        type="button"
                                        class="btn btn-primary"
                                        disabled=move || {
                                            reg_username.get().trim().is_empty()
                                            || reg_email.get().trim().is_empty()
                                            || !reg_email.get().contains('@')
                                            || age_from_dob(&reg_birthday.get()).map_or(true, |a| a < MIN_AGE)
                                        }
                                        on:click=move |_| reg_step.set(2)
                                    >"Continue"</button>
                                </div>
                            })}

                            // Step 2: ID Verification
                            {move || (reg_step.get() == 2).then(|| view! {
                                <div class="register-step">
                                    <p class="step-indicator">"Step 2 of 3: Identity Verification"</p>

                                    <div class="form-field">
                                        <label class="form-label">
                                            "ID Type"
                                            <span class="required">"*"</span>
                                        </label>
                                        <p class="form-hint" style="margin-bottom: 6px;">
                                            "Detected jurisdiction: "
                                            <strong>{move || jurisdiction_name.get()}</strong>
                                        </p>
                                        <select
                                            class="form-input"
                                            prop:value=move || reg_id_type.get()
                                            on:change=move |e| {
                                                let val = event_target_value(&e);
                                                // Passport = single side. Everything else needs front + back.
                                                let needs_back = !val.contains("passport");
                                                reg_id_needs_back.set(needs_back);
                                                reg_id_type.set(val);
                                            }
                                        >
                                            {move || accepted_ids.get().into_iter().map(|id| {
                                                let val = id.to_lowercase().replace(' ', "_").replace("'", "");
                                                view! {
                                                    <option value={val}>{id}</option>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </select>
                                    </div>

                                    <div class="form-field">
                                        <label class="form-label">
                                            "ID Number"
                                            <span class="required">"*"</span>
                                        </label>
                                        <input
                                            type="text"
                                            class="form-input"
                                            placeholder="Enter your ID number"
                                            prop:value=move || reg_id_number.get()
                                            on:input=move |e| reg_id_number.set(event_target_value(&e))
                                        />
                                    </div>

                                    // ID Document Upload — Front
                                    <div class="form-field">
                                        <label class="form-label">
                                            {move || {
                                                if reg_id_needs_back.get() {
                                                    "Upload ID: front"
                                                } else {
                                                    "Upload ID Document"
                                                }
                                            }}
                                            <span class="required">"*"</span>
                                        </label>
                                        <div class="identity-file-drop">
                                            <input
                                                type="file"
                                                accept="image/jpeg,image/png"
                                                class="identity-file-input"
                                                on:change=move |e| {
                                                    use wasm_bindgen::JsCast;
                                                    let input: web_sys::HtmlInputElement = e.target().unwrap().unchecked_into();
                                                    if let Some(files) = input.files() {
                                                        if let Some(file) = files.get(0) {
                                                            let name = file.name();
                                                            reg_id_front_name.set(name.clone());
                                                            let id_type = reg_id_type.get();
                                                            let id_number = reg_id_number.get();
                                                            let birthday = reg_birthday.get();
                                                            let uname = reg_username.get();
                                                            let needs_back = reg_id_needs_back.get();
                                                            kyc_status_text.set("Uploading document...".to_string());
                                                            kyc_verified.set(false);
                                                            kyc_rejected.set(false);
                                                            kyc_unavailable.set(String::new());
                                                            spawn_local(async move {
                                                                // The session's id and upload token, from /api/kyc/start on first use.
                                                                let (sess, upload_token) = match ensure_kyc_session(kyc_session_id, kyc_claim_secret, kyc_upload_token).await {
                                                                    Ok(started) => started,
                                                                    Err(e) => {
                                                                        error.set(Some(e));
                                                                        kyc_status_text.set(String::new());
                                                                        return;
                                                                    }
                                                                };
                                                                match upload_id_document(&file, "front", &id_type, &id_number, &birthday, &uname, &sess, &upload_token).await {
                                                                    Ok(_resp) => {
                                                                        reg_id_front_uploaded.set(true);
                                                                        // If no back needed (passport), submit for verification now
                                                                        if !needs_back {
                                                                            kyc_status_text.set("Submitting for verification...".to_string());
                                                                            submit_kyc_verification(
                                                                                sess.clone(), upload_token.clone(), false, uname.clone(),
                                                                                kyc_status_text, kyc_verified_name, kyc_verified, kyc_rejected, kyc_reject_reason, kyc_unavailable,
                                                                            ).await;
                                                                        } else {
                                                                            kyc_status_text.set("Front uploaded. Now upload the back.".to_string());
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        error.set(Some(format!("Upload failed: {}", e)));
                                                                        kyc_status_text.set(String::new());
                                                                        reg_id_front_uploaded.set(false);
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    }
                                                }
                                            />
                                            <div class="file-drop-label">
                                                {move || {
                                                    if reg_id_front_uploaded.get() {
                                                        let name = reg_id_front_name.get();
                                                        format!("Front: {}", name)
                                                    } else if reg_id_needs_back.get() {
                                                        "Upload photo of front side".to_string()
                                                    } else {
                                                        "Upload photo of your ID".to_string()
                                                    }
                                                }}
                                            </div>
                                        </div>
                                    </div>

                                    // ID Document Upload — Back (only for non-passport IDs)
                                    <div class="form-field" class:hidden=move || !reg_id_needs_back.get()>
                                        <label class="form-label">
                                            "Upload ID: back"
                                            <span class="required">"*"</span>
                                        </label>
                                        <div class="identity-file-drop">
                                            <input
                                                type="file"
                                                accept="image/jpeg,image/png"
                                                class="identity-file-input"
                                                on:change=move |e| {
                                                    use wasm_bindgen::JsCast;
                                                    let input: web_sys::HtmlInputElement = e.target().unwrap().unchecked_into();
                                                    if let Some(files) = input.files() {
                                                        if let Some(file) = files.get(0) {
                                                            let name = file.name();
                                                            reg_id_back_name.set(name.clone());
                                                            let id_type = reg_id_type.get();
                                                            let id_number = reg_id_number.get();
                                                            let birthday = reg_birthday.get();
                                                            let uname = reg_username.get();
                                                            spawn_local(async move {
                                                                // The session's id and upload token, from /api/kyc/start on first use.
                                                                let (sess, upload_token) = match ensure_kyc_session(kyc_session_id, kyc_claim_secret, kyc_upload_token).await {
                                                                    Ok(started) => started,
                                                                    Err(e) => {
                                                                        error.set(Some(e));
                                                                        kyc_status_text.set(String::new());
                                                                        return;
                                                                    }
                                                                };
                                                                match upload_id_document(&file, "back", &id_type, &id_number, &birthday, &uname, &sess, &upload_token).await {
                                                                    Ok(_) => {
                                                                        reg_id_back_uploaded.set(true);
                                                                        // Both sides uploaded — submit for verification
                                                                        kyc_status_text.set("Submitting for verification...".to_string());
                                                                        submit_kyc_verification(
                                                                            sess.clone(), upload_token.clone(), true, uname.clone(),
                                                                            kyc_status_text, kyc_verified_name, kyc_verified, kyc_rejected, kyc_reject_reason, kyc_unavailable,
                                                                        ).await;
                                                                    }
                                                                    Err(e) => {
                                                                        error.set(Some(format!("Back upload failed: {}", e)));
                                                                        reg_id_back_uploaded.set(false);
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    }
                                                }
                                            />
                                            <div class="file-drop-label">
                                                {move || {
                                                    if reg_id_back_uploaded.get() {
                                                        let name = reg_id_back_name.get();
                                                        format!("Back: {}", name)
                                                    } else {
                                                        "Upload photo of back side".to_string()
                                                    }
                                                }}
                                            </div>
                                        </div>
                                    </div>

                                    // ── Continue on phone (QR handoff) ──
                                    // A webcam rarely resolves the small print on
                                    // an ID. Scanning a code hands capture to the
                                    // phone camera while this tab keeps polling.
                                    <div class="kyc-handoff">
                                        <Show
                                            when=move || !qr_open.get()
                                            fallback=move || view! {
                                                <div class="kyc-qr-panel">
                                                    <p class="kyc-qr-title">"Scan with your phone"</p>
                                                    <QrCodeSvg data=qr_url.get() size_px=200 />
                                                    <p class="kyc-qr-hint">
                                                        "Point your phone camera at the code, then photograph your ID. "
                                                        "This page updates on its own when you finish."
                                                    </p>
                                                    <Show when=move || qr_polling.get()>
                                                        <p class="kyc-qr-waiting">
                                                            <span class="kyc-spinner"></span>
                                                            "Waiting for your phone..."
                                                        </p>
                                                    </Show>
                                                    <button
                                                        type="button"
                                                        class="btn-link"
                                                        on:click=move |_| { qr_open.set(false); qr_polling.set(false); }
                                                    >"Cancel and upload here instead"</button>
                                                </div>
                                            }
                                        >
                                            <button
                                                type="button"
                                                class="btn btn-secondary kyc-phone-btn"
                                                on:click=move |_| start_handoff()
                                            >"Use my phone camera instead"</button>
                                            <p class="form-hint">
                                                "Recommended. Phone cameras read ID text far more reliably than a webcam."
                                            </p>
                                        </Show>
                                        <Show when=move || !qr_error.get().is_empty()>
                                            <p class="kyc-qr-error">{move || qr_error.get()}</p>
                                        </Show>
                                    </div>

                                    <p class="form-hint">
                                        "Your ID images are encrypted and stored securely. "
                                        "They are only used for one-time verification."
                                    </p>

                                    // ── Verification Status Feedback ──
                                    <div class="kyc-feedback-area">
                                        <Show when=move || kyc_verified.get()>
                                            <div class="kyc-status kyc-verified">
                                                <span class="kyc-icon">"✓"</span>
                                                <div>
                                                    <strong>"Identity Verified"</strong>
                                                    <p>{move || {
                                                        let name = kyc_verified_name.get();
                                                        format!("Verified as {}", if name.is_empty() { "you".to_string() } else { name })
                                                    }}</p>
                                                </div>
                                            </div>
                                        </Show>
                                        <Show when=move || kyc_rejected.get()>
                                            <div class="kyc-status kyc-rejected">
                                                <span class="kyc-icon">"✗"</span>
                                                <div>
                                                    <strong>"Verification Failed"</strong>
                                                    <p>{move || {
                                                        let reason = kyc_reject_reason.get();
                                                        if reason.is_empty() { "Please try again with a clearer photo".to_string() } else { reason }
                                                    }}</p>
                                                </div>
                                            </div>
                                        </Show>
                                        <Show when=move || !kyc_unavailable.get().is_empty()>
                                            <div class="kyc-status kyc-unavailable" role="alert">
                                                <span class="kyc-icon">"!"</span>
                                                <div>
                                                    <strong>"Verification is temporarily unavailable"</strong>
                                                    <p>{move || kyc_unavailable.get()}</p>
                                                    <button
                                                        type="button"
                                                        class="btn btn-secondary kyc-retry-btn"
                                                        on:click=move |_| {
                                                            // The same session: the Worker kept its uploads, so nothing is uploaded again.
                                                            let sess = kyc_session_id.get_untracked();
                                                            let upload_token = kyc_upload_token.get_untracked();
                                                            let needs_back = reg_id_needs_back.get_untracked();
                                                            let uname = reg_username.get_untracked();
                                                            kyc_unavailable.set(String::new());
                                                            kyc_status_text.set("Submitting for verification...".to_string());
                                                            spawn_local(async move {
                                                                submit_kyc_verification(
                                                                    sess, upload_token, needs_back, uname,
                                                                    kyc_status_text, kyc_verified_name, kyc_verified, kyc_rejected, kyc_reject_reason, kyc_unavailable,
                                                                ).await;
                                                            });
                                                        }
                                                    >"Try again"</button>
                                                </div>
                                            </div>
                                        </Show>
                                        <Show when=move || !kyc_status_text.get().is_empty() && !kyc_verified.get() && !kyc_rejected.get()>
                                            <div class="kyc-status kyc-processing">
                                                <span class="kyc-spinner"></span>
                                                <span>{move || kyc_status_text.get()}</span>
                                            </div>
                                        </Show>
                                    </div>

                                    <div class="register-nav-buttons">
                                        <button
                                            type="button"
                                            class="btn btn-secondary"
                                            on:click=move |_| reg_step.set(1)
                                        >"Back"</button>
                                        <button
                                            type="button"
                                            class="btn btn-primary"
                                            disabled=move || {
                                                reg_id_number.get().trim().is_empty()
                                                || !reg_id_front_uploaded.get()
                                                || (reg_id_needs_back.get() && !reg_id_back_uploaded.get())
                                                || !kyc_verified.get()
                                            }
                                            on:click=move |_| reg_step.set(3)
                                        >{move || if kyc_verified.get() { "Continue" } else { "Waiting for verification..." }}</button>
                                    </div>
                                </div>
                            })}

                            // Step 3: Generate Identity
                            {move || (reg_step.get() == 3).then(|| {
                                let username = reg_username.get();
                                let email = reg_email.get();
                                let birthday = reg_birthday.get();
                                let id_type = reg_id_type.get();
                                let id_number = reg_id_number.get();
                                view! {
                                    <div class="register-step">
                                        <p class="step-indicator">"Step 3 of 3: Create Your Identity"</p>

                                        <div class="identity-info">
                                            <div class="identity-field">
                                                <span class="field-label">"Username"</span>
                                                <span class="field-value">{username.clone()}</span>
                                            </div>
                                            <div class="identity-field">
                                                <span class="field-label">"Email"</span>
                                                <span class="field-value">{email.clone()}</span>
                                            </div>
                                            <div class="identity-field">
                                                <span class="field-label">"Date of Birth"</span>
                                                <span class="field-value">{birthday.clone()}</span>
                                            </div>
                                            <div class="identity-field">
                                                <span class="field-label">"ID Document"</span>
                                                <span class="field-value identity-status-ok">
                                                    {if reg_id_needs_back.get() { "Front + Back uploaded" } else { "Uploaded" }}
                                                </span>
                                            </div>
                                        </div>

                                        <button
                                            type="button"
                                            class="btn btn-primary"
                                            disabled=loading.get()
                                            on:click=move |_| {
                                                let uname = username.clone();
                                                let email_addr = email.clone();
                                                let bday = birthday.clone();
                                                let idt = id_type.clone();
                                                let idn = id_number.clone();

                                                match generate_identity_toml(&uname) {
                                                    Ok((toml_content, pub_key, priv_key)) => {
                                                        let pub_key_clone = pub_key.clone();
                                                        let uname_clone = uname.clone();
                                                        let email_clone = email_addr.clone();
                                                        let bday_clone = bday.clone();
                                                        let idt_clone = idt.clone();
                                                        let toml_for_email = toml_content.clone();
                                                        let id_hash = sha256_hex(&format!("{}:{}:{}", idt, idn, bday));

                                                        // Register first — only download TOML on success
                                                        loading.set(true);
                                                        error.set(None);
                                                        identity_status.set("Registering...".to_string());

                                                        let api_url = api_url_stored.get_value();
                                                        let app_state_c = app_state_stored.get_value();
                                                        let nav = nav_fn.get_value();
                                                        spawn_local(async move {
                                                            let client = ApiClient::new(&api_url);
                                                            let sess = kyc_session_id.get();
                                                            // The claim secret goes to registration and nowhere else.
                                                            let claim = kyc_claim_secret.get_untracked();
                                                            // Pass `email` + `toml_content` into the
                                                            // register request so the Worker auto-relays
                                                            // the welcome email in-line. Saves a round
                                                            // trip vs. the previous /api/identity/email-
                                                            // backup follow-up call AND closes the
                                                            // window where registration succeeded but
                                                            // the email post failed silently.
                                                            let email_for_register = if email_clone.is_empty() {
                                                                None
                                                            } else {
                                                                Some(email_clone.as_str())
                                                            };
                                                            let toml_for_register = if email_for_register.is_some() {
                                                                Some(toml_for_email.as_str())
                                                            } else {
                                                                None
                                                            };
                                                            match api::register_identity(
                                                                &client,
                                                                &uname_clone,
                                                                &pub_key_clone,
                                                                Some(&bday_clone),
                                                                Some(&idt_clone),
                                                                Some(&id_hash),
                                                                Some(&sess),
                                                                Some(&claim),
                                                                email_for_register,
                                                                toml_for_register,
                                                            ).await {
                                                                Ok(response) => {
                                                                    // Used: the claim secret is not kept.
                                                                    kyc_claim_secret.set(String::new());
                                                                    // Registration succeeded — download both files
                                                                    let toml_filename = format!("eustress-{}.toml", uname_clone);
                                                                    download_file(&toml_filename, &toml_content);
                                                                    download_file("README - Eustress Identity.txt", &identity_readme(&uname_clone));

                                                                    // Fallback: if the in-line auto-email
                                                                    // didn't go through (transient relay
                                                                    // hiccup, sender unverified mid-deploy,
                                                                    // etc.), retry via the explicit
                                                                    // `/api/identity/email-backup` endpoint.
                                                                    // The user supplied email but server
                                                                    // didn't emit it — this is the only
                                                                    // path that recovers without re-asking.
                                                                    let auto_emailed = response
                                                                        .backup_emailed
                                                                        .unwrap_or(false);
                                                                    if !auto_emailed && !email_clone.is_empty() {
                                                                        let email_client = ApiClient::new(&api_url);
                                                                        let email_to = email_clone.clone();
                                                                        let email_toml = toml_for_email.clone();
                                                                        let email_user = uname_clone.clone();
                                                                        spawn_local(async move {
                                                                            let _: Result<serde_json::Value, _> = email_client.post(
                                                                                "/api/identity/email-backup",
                                                                                &serde_json::json!({
                                                                                    "email": email_to,
                                                                                    "username": email_user,
                                                                                    "toml_content": email_toml,
                                                                                }),
                                                                            ).await;
                                                                        });
                                                                    }

                                                                    identity_public_key.set(pub_key);
                                                                    identity_private_key.set(priv_key);
                                                                    identity_username.set(uname_clone.clone());
                                                                    identity_file_loaded.set(true);

                                                                    loading.set(false);
                                                                    identity_status.set("Registered. Save your Eustress identity file.".to_string());
                                                                    app_state_c.login_with_token(response.token, response.user);
                                                                    nav("/dashboard", Default::default());
                                                                }
                                                                Err(e) => {
                                                                    loading.set(false);
                                                                    error.set(Some(format!("Registration failed: {}", e)));
                                                                }
                                                            }
                                                        });
                                                    }
                                                    Err(e) => {
                                                        error.set(Some(format!("Failed: {}", e)));
                                                    }
                                                }
                                            }
                                        >
                                            {move || if loading.get() { "Registering..." } else { "Create Identity & Sign In" }}
                                        </button>

                                        <p class="identity-create-hint">
                                            "Your Ed25519 keypair is generated in the browser. "
                                            "The downloaded "
                                            <code>"identity.toml"</code>
                                            " is your account. Keep it safe."
                                        </p>

                                        <button
                                            type="button"
                                            class="btn btn-secondary"
                                            style="margin-top: 8px;"
                                            on:click=move |_| reg_step.set(2)
                                        >"Back"</button>
                                    </div>
                                }
                            })}
                        </div>

                    </div>
                </div>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// Upload an ID document to Cloudflare R2 via the KYC API.
/// Called immediately when user selects a file.
/// Upload response from KYC worker
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct KycUploadResponse {
    verification_id: String,
    status: String,
}

/// Status poll response from KYC worker
#[derive(Clone, Debug, serde::Deserialize)]
struct KycStatusResponse {
    status: String,
    verified: Option<bool>,
    ocr_name: Option<String>,
    decision: Option<serde_json::Value>,
}

pub(crate) async fn upload_id_document(
    file: &web_sys::File,
    side: &str,
    id_type: &str,
    id_number: &str,
    birthday: &str,
    username: &str,
    session_id: &str,
    upload_token: &str,
) -> Result<KycUploadResponse, String> {
    let form_data = web_sys::FormData::new()
        .map_err(|_| "Failed to create FormData".to_string())?;
    form_data
        .append_with_blob_and_filename("document", file, &file.name())
        .map_err(|_| "Failed to append file".to_string())?;
    form_data.append_with_str("side", side).map_err(|_| "append side".to_string())?;
    form_data.append_with_str("id_type", id_type).map_err(|_| "append id_type".to_string())?;
    form_data.append_with_str("id_number", id_number).map_err(|_| "append id_number".to_string())?;
    form_data.append_with_str("birthday", birthday).map_err(|_| "append birthday".to_string())?;
    form_data.append_with_str("username", username).map_err(|_| "append username".to_string())?;
    form_data.append_with_str("session_id", session_id).map_err(|_| "append session_id".to_string())?;
    form_data.append_with_str("upload_token", upload_token).map_err(|_| "append upload_token".to_string())?;

    let api_url = crate::state::api_base();

    let token: Option<String> = {
        use gloo_storage::Storage;
        gloo_storage::LocalStorage::get("auth_token").ok()
    };

    let mut request = gloo_net::http::Request::post(&format!("{}/api/kyc/upload", api_url));
    if let Some(ref t) = token {
        request = request.header("Authorization", &format!("Bearer {}", t));
    }

    let resp = request
        .body(form_data)
        .map_err(|e| format!("Request build failed: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Upload failed: {}", e))?;

    if resp.status() == 200 || resp.status() == 201 {
        let text = resp.text().await.unwrap_or_default();
        serde_json::from_str::<KycUploadResponse>(&text)
            .map_err(|e| format!("Parse error: {}", e))
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Upload error ({}): {}", resp.status(), text))
    }
}

/// What POST /api/kyc/start returns. No Debug: it holds the claim secret.
#[derive(serde::Deserialize)]
struct KycStarted {
    session_id: String,
    claim_secret: String,
    upload_token: String,
}

/// The registration's verification session, started on first use: `(session id,
/// upload token)`. The claim secret goes into its signal (memory) and nowhere
/// else until registration sends it.
pub(crate) async fn ensure_kyc_session(
    session: RwSignal<String>,
    claim: RwSignal<String>,
    upload: RwSignal<String>,
) -> Result<(String, String), String> {
    let existing = session.get_untracked();
    if !existing.is_empty() {
        return Ok((existing, upload.get_untracked()));
    }
    let resp = gloo_net::http::Request::post(&format!("{}/api/kyc/start", crate::state::api_base()))
        .send()
        .await
        .map_err(|_| "Could not reach the verification service. Check your connection.".to_string())?;
    if resp.status() != 200 {
        return Err(format!("Could not start verification ({}). Try again in a minute.", resp.status()));
    }
    let started: KycStarted = resp
        .json()
        .await
        .map_err(|_| "Unexpected answer from the verification service.".to_string())?;
    session.set(started.session_id.clone());
    claim.set(started.claim_secret);
    upload.set(started.upload_token.clone());
    Ok((started.session_id, started.upload_token))
}

/// Submit uploaded documents for verification and handle the result.
/// Calls /api/kyc/submit which checks documents are present and marks verified.
pub(crate) async fn submit_kyc_verification(
    session_id: String,
    upload_token: String,
    needs_back: bool,
    full_name: String,
    status_signal: RwSignal<String>,
    verified_name_signal: RwSignal<String>,
    kyc_verified_signal: RwSignal<bool>,
    kyc_rejected_signal: RwSignal<bool>,
    reject_reason_signal: RwSignal<String>,
    unavailable_signal: RwSignal<String>,
) {
    unavailable_signal.set(String::new());
    let api_url = crate::state::api_base();
    let url = format!("{}/api/kyc/submit", api_url);

    let body = serde_json::json!({
        "session_id": session_id,
        "upload_token": &upload_token,
        "needs_back": needs_back,
        "full_name": full_name,
    });

    match gloo_net::http::Request::post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .unwrap()
        .send()
        .await
    {
        Ok(resp) => {
            let text = resp.text().await.unwrap_or_default();
            if resp.status() == 200 {
                if let Ok(result) = serde_json::from_str::<KycStatusResponse>(&text) {
                    if result.status == "verified" {
                        let name = result.ocr_name.unwrap_or_default();
                        let display = if name.is_empty() { "you".to_string() } else { name.clone() };
                        status_signal.set(format!("Verified as {}", display));
                        verified_name_signal.set(name);
                        kyc_verified_signal.set(true);
                        return;
                    }
                }
                // Unexpected status — fall back to polling
                let vid = format!("kyc-{}-front", session_id);
                poll_kyc_status(vid, upload_token.clone(), status_signal, verified_name_signal, kyc_verified_signal, kyc_rejected_signal, reject_reason_signal).await;
            } else if let Some(message) = crate::pages::verify::kyc_unavailable_message(resp.status(), &text) {
                // Verification could not run: not a verdict. Nothing is rejected, the
                // uploads are kept, and "Try again" submits this session again.
                status_signal.set(String::new());
                unavailable_signal.set(message);
            } else {
                // The Worker explains a refusal in `error`; show that, not its JSON.
                let said = crate::api::worker_sentence(&text).unwrap_or(text);
                status_signal.set(said.clone());
                reject_reason_signal.set(said);
                kyc_rejected_signal.set(true);
            }
        }
        Err(e) => {
            status_signal.set(format!("Network error: {}", e));
            kyc_rejected_signal.set(true);
        }
    }
}

/// Poll KYC verification status until resolved (verified/rejected/error)
async fn poll_kyc_status(
    verification_id: String,
    upload_token: String,
    status_signal: RwSignal<String>,
    verified_name_signal: RwSignal<String>,
    kyc_verified_signal: RwSignal<bool>,
    kyc_rejected_signal: RwSignal<bool>,
    reject_reason_signal: RwSignal<String>,
) {
    let api_url = crate::state::api_base();
    let url = format!("{}/api/kyc/status/{}", api_url, verification_id);

    // Poll every 2 seconds, max 60 attempts (2 minutes)
    for _ in 0..60 {
        gloo_timers::future::TimeoutFuture::new(2_000).await;

        let resp = match gloo_net::http::Request::get(&url).header("X-KYC-Token", &upload_token).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let text = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        let result: KycStatusResponse = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(_) => continue,
        };

        match result.status.as_str() {
            "verified" => {
                let name = result.ocr_name.unwrap_or_default();
                status_signal.set(format!("Verified as {}", if name.is_empty() { "you" } else { &name }));
                verified_name_signal.set(name);
                kyc_verified_signal.set(true);
                return;
            }
            "rejected" => {
                let reasons = result.decision
                    .and_then(|d| d.get("reasons").cloned())
                    .and_then(|r| r.as_array().map(|a| {
                        a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>().join(", ")
                    }))
                    .unwrap_or_else(|| "Verification failed".to_string());
                status_signal.set("Rejected".to_string());
                reject_reason_signal.set(reasons);
                kyc_rejected_signal.set(true);
                return;
            }
            "error" => {
                status_signal.set("Verification error. Please try again.".to_string());
                kyc_rejected_signal.set(true);
                return;
            }
            _ => {
                // Still processing
                status_signal.set("Verifying your identity...".to_string());
            }
        }
    }

    // Timeout
    status_signal.set("Verification timed out. Please try again.".to_string());
    kyc_rejected_signal.set(true);
}

/// Generate the README file that accompanies identity.toml downloads.
fn identity_readme(username: &str) -> String {
    format!(r#"========================================================
  EUSTRESS IDENTITY — {username}
========================================================

WHAT IS THIS FILE?

  The "eustress-{username}.toml" file in this folder IS your Eustress
  account. It contains your Ed25519 keypair — your private
  key is your password, your public key is your username's
  cryptographic proof. There is no email, no password, no
  "forgot password" button. This file is everything.

HOW TO KEEP IT SAFE

  1. RECOMMENDED: Save this folder to a cloud drive
     (OneDrive, iCloud Drive, Google Drive, Dropbox).
     This way your identity syncs across all your
     computers automatically.

     Suggested locations:
       Windows:  OneDrive/Documents/Eustress/
       macOS:    iCloud Drive/Eustress/
       Linux:    Google Drive/Eustress/

  2. Make a backup on a USB drive or external storage.
     If you lose this file and have no backup, your
     account is gone forever. No one can recover it.

  3. NEVER share identity.toml with anyone. Anyone who
     has this file can sign in as you.

SIGN IN ON ANOTHER COMPUTER

  You can sign in on as many computers as you want,
  at the same time. Just copy identity.toml to each
  machine (or use cloud sync). Go to eustress.dev/login,
  click "Sign In", and load your identity.toml file.

  Each session gets its own token. There is no limit
  on simultaneous sessions.

HOW IT WORKS

  When you sign in, the server sends a random challenge.
  Your browser signs it with your private key (which
  never leaves your device). The server verifies the
  signature matches your public key on file. Done.

  No passwords are transmitted. No credentials are
  stored on any server. Your private key never touches
  the network.

BENEFICIARIES

  You can add beneficiaries to your identity.toml to
  designate who receives your BLS balance and assets
  if your account becomes inactive. Add this section
  to the bottom of your identity.toml:

  [beneficiaries]
  primary = "their_public_key_hex"
  secondary = "backup_public_key_hex"
  inactive_days = 365

  After 365 days of inactivity, your assets transfer
  to your primary beneficiary automatically.

SUPPORT

  Website:  https://eustress.dev
  Login:    https://eustress.dev/login
  Help:     support@eustress.dev

========================================================
  Keep this file safe. It is your account.
========================================================
"#)
}

fn extract_toml_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() == 2 {
        let val = parts[1].trim().trim_matches('"').trim_matches('\'');
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}

// The Worker mails this file from identity@eustress.dev, so it accepts only a
// file in this shape (validIdentityFile in
// infrastructure/cloudflare/api/src/index.js): these sections and keys, these
// two header comments, no other text. A new key or comment here needs the same
// change there, or the backup email stops being sent.
//
// The file carries the account, the keys and whether the account is verified,
// and nothing about the person: no birthday, no ID type, no ID hash. Anyone who
// reads a leaked file would otherwise hold everything but the ID number, which
// is a small space to search. Sign-in and Studio read only the keys.
fn generate_identity_toml(username: &str) -> Result<(String, String, String), String> {
    generate_identity_toml_for(username, None)
}

/// [`generate_identity_toml`] for a known account: the file names the account's
/// own id (a replacement key's file), or a fresh one when `user_id` is `None`.
pub(crate) fn generate_identity_toml_for(username: &str, user_id: Option<&str>) -> Result<(String, String, String), String> {
    use bliss_crypto::KeyPair;

    let keypair = KeyPair::generate();
    let pub_hex = keypair.public_key().to_hex();
    let priv_hex = keypair.private_key().to_hex();

    let now = chrono::Utc::now().to_rfc3339();
    let user_id = user_id.map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let toml_content = format!(
r#"# Eustress Identity — generated {now}
# This file IS your account. Keep it safe. Do not share your private key.

[identity]
version = "1.0"
user_id = "{user_id}"
username = "{username}"
public_key = "{pub_hex}"
private_key = "{priv_hex}"
issued_at = "{now}"

[verification]
verified = false
"#);

    Ok((toml_content, pub_hex, priv_hex))
}

fn sha256_hex(input: &str) -> String {
    let hash = bliss_crypto::sha256(input.as_bytes());
    hash.to_hex()
}

/// Signs `message` (the login message, see `api::login_message`) with the key.
pub(crate) fn sign_challenge_ed25519(private_key_hex: &str, message: &str) -> Result<String, String> {
    use bliss_crypto::{PrivateKey, Signature};

    // Support both hex and base64 encoded keys for backwards compatibility
    let private_key = if private_key_hex.len() == 64 && private_key_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        // Hex-encoded (new format)
        PrivateKey::from_hex(private_key_hex)
            .map_err(|e| format!("Invalid private key: {}", e))?
    } else {
        // Base64-encoded (old format)
        let seed_bytes = base64_decode(private_key_hex)
            .map_err(|e| format!("Invalid private key encoding: {}", e))?;
        PrivateKey::from_bytes(&seed_bytes)
            .map_err(|e| format!("Invalid private key: {}", e))?
    };

    let signature = Signature::sign(message.as_bytes(), &private_key)
        .map_err(|e| format!("Signing failed: {}", e))?;

    // Return signature as hex (matches what the Worker expects)
    Ok(private_key_to_hex_sig(&signature.to_bytes()))
}

fn private_key_to_hex_sig(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub(crate) fn download_file(filename: &str, content: &str) {
    use wasm_bindgen::JsCast;
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            let blob_parts = js_sys::Array::new();
            blob_parts.push(&wasm_bindgen::JsValue::from_str(content));
            if let Ok(blob) = web_sys::Blob::new_with_str_sequence(&blob_parts) {
                if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                    if let Ok(el) = doc.create_element("a") {
                        let a: web_sys::HtmlAnchorElement = el.unchecked_into();
                        a.set_href(&url);
                        a.set_download(filename);
                        a.click();
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }
            }
        }
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("No window")?;
    let decoded = window
        .atob(input)
        .map_err(|_| "base64 decode failed".to_string())?;
    Ok(decoded.bytes().collect())
}

fn base64_encode(input: &[u8]) -> String {
    let binary: String = input.iter().map(|&b| b as char).collect();
    web_sys::window()
        .and_then(|w| w.btoa(&binary).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod login_signing_tests {
    use super::*;

    fn a_user() -> User {
        serde_json::from_value(serde_json::json!({
            "id": "11111111-2222-4333-8444-555555555555", "username": "ada", "email": null,
            "avatar_url": null, "discord_id": null, "created_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap()
    }

    #[test]
    fn a_passkey_sign_in_marks_the_session_so_key_changes_are_off() {
        let marked = as_passkey_session(a_user());
        assert_eq!(marked.auth_method.as_deref(), Some("passkey"));
        assert!(marked.signed_in_with_passkey());
        assert!(!a_user().signed_in_with_passkey(), "an identity-file sign-in is not marked");
        assert_eq!(marked.username, "ada", "nothing else changes");
    }

    #[test]
    fn the_passkey_button_is_shown_only_when_the_browser_has_webauthn_and_signs_in_like_the_file_does() {
        let source = include_str!("login.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap()];
        let shown = source.find("<Show when=move || passkey_supported.get()>").expect("the button sits inside a Show on the WebAuthn flag");
        let button = source[shown..].find("{PASSKEY_BUTTON}").expect("and the button is inside it");
        assert!(button < 900, "the Show wraps the button");
        assert_eq!(PASSKEY_BUTTON, "Sign in with a passkey");
        assert!(source.contains("passkey_supported.set(webauthn::webauthn_available())"), "the flag is the browser's own answer");
        assert!(source.contains("let passkey_supported = RwSignal::new(false);"), "hidden until the browser says yes");
        // The same finish as the identity file: store the token and go to the dashboard.
        let handler = &source[source.find("let passkey_sign_in").unwrap()..];
        let handler = &handler[..handler.find("view! {").unwrap()];
        assert!(handler.contains("login_with_token(response.token, as_passkey_session(response.user))"));
        assert!(handler.contains("nav(\"/dashboard\", Default::default())"));
        assert!(handler.contains("api::passkey_login_begin") && handler.contains("webauthn::get_credential") && handler.contains("api::passkey_login("));
    }

    const VECTORS: &str = include_str!("../../../../../docs/architecture/login-v1-vectors.json");

    #[test]
    fn signing_the_login_message_gives_the_shared_signatures() {
        let v: serde_json::Value = serde_json::from_str(VECTORS).expect("the shared vectors are JSON");
        let secret = v["secret_key"].as_str().unwrap();
        for s in v["signatures"].as_array().unwrap() {
            let message = api::login_message(s["origin"].as_str().unwrap(), s["challenge"].as_str().unwrap());
            assert_eq!(sign_challenge_ed25519(secret, &message).unwrap(), s["signature"].as_str().unwrap());
        }
    }

    #[test]
    fn the_identity_file_carries_no_id_data() {
        let (toml, public, private) = generate_identity_toml("someone").unwrap();
        for leaked in ["birthday", "id_type", "id_hash", "[profile]"] {
            assert!(!toml.contains(leaked), "the file mentions {leaked}");
        }
        assert!(toml.contains("[identity]"));
        assert!(toml.contains("username = \"someone\""));
        assert!(toml.contains(&format!("public_key = \"{public}\"")));
        assert!(toml.contains(&format!("private_key = \"{private}\"")));
        assert!(toml.contains("verified = false"));
    }

    #[test]
    fn try_again_on_the_registration_page_resubmits_without_uploading() {
        let src = include_str!("login.rs").replace("\r\n", "\n");
        let button = src.split("class=\"btn btn-secondary kyc-retry-btn\"").nth(1).unwrap().split(">\"Try again\"<").next().unwrap();
        assert!(button.contains("kyc_session_id.get_untracked()"));
        assert!(button.contains("kyc_upload_token.get_untracked()"));
        assert!(button.contains("submit_kyc_verification("));
        assert!(!button.contains("upload_id_document"));
        let submit = src.split("async fn submit_kyc_verification(").nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(submit.contains("kyc_unavailable_message(resp.status(), &text)"));
        assert!(submit.contains("unavailable_signal.set(message)"));
    }

    #[test]
    fn the_claim_secret_goes_to_registration_only_and_stays_in_memory() {
        let src = include_str!("login.rs").replace("\r\n", "\n");
        let code = src.split("#[cfg(test)]").next().unwrap();
        // Read once, for registration; cleared once used.
        assert_eq!(code.matches("kyc_claim_secret.get").count(), 1);
        let register = code.split("let claim = kyc_claim_secret.get_untracked();").nth(1).unwrap();
        assert!(register.split("email_for_register,").next().unwrap().contains("Some(&claim),"));
        assert!(code.contains("kyc_claim_secret.set(String::new());"));
        // The phone hand-off sends the upload token and nothing that can register.
        let handoff = code.split("let start_handoff = move || {").nth(1).unwrap().split("\n    };\n").next().unwrap();
        assert!(handoff.contains("\"upload_token\": upload_token"));
        let payload = handoff.split("serde_json::json!({").nth(1).unwrap().split("});").next().unwrap();
        assert!(payload.contains("\"upload_token\": upload_token"));
        assert!(!payload.contains("claim"));
        // No line that touches a claim secret logs it or stores it.
        for line in code.lines().filter(|l| l.contains("claim")) {
            for bad in ["log::", "console", "Storage", "storage", "format!("] {
                assert!(!line.contains(bad), "{bad} next to the claim secret: {line}");
            }
        }
        // The struct that carries it has no Debug.
        assert!(code.contains("#[derive(serde::Deserialize)]\nstruct KycStarted {"));
    }

    #[test]
    fn the_claim_secret_lives_in_page_memory_only_never_a_url_storage_or_a_log() {
        let code = |src: &str| src.replace("\r\n", "\n").split("#[cfg(test)]").next().unwrap().to_string();
        let login = code(include_str!("login.rs"));
        let auth = code(include_str!("../api/auth.rs"));
        let client = code(include_str!("../api/mod.rs"));
        // Held in a signal (page memory), set from /api/kyc/start, read once for registration.
        assert!(login.contains("let kyc_claim_secret = RwSignal::new(String::new());"));
        assert!(login.contains("claim.set(started.claim_secret);"));
        // No line that touches it builds a URL, stores it, or logs it, on the page or in the API client.
        for (file, text) in [("pages/login.rs", &login), ("api/auth.rs", &auth)] {
            for line in text.lines().filter(|l| l.to_lowercase().contains("claim")) {
                for bad in ["log::", "console", "Storage", "storage", "format!", "url", "Url", "URL", "href", "location", "history", "query", "#t=", "?s="] {
                    assert!(!line.contains(bad), "{file}: {bad} next to the claim secret: {line}");
                }
            }
            assert!(!text.contains("SessionStorage") || !text.lines().any(|l| l.contains("SessionStorage") && l.to_lowercase().contains("claim")));
        }
        // It travels only in the JSON body of the sign-up POST, which the client does not log.
        assert!(auth.contains("client.post(\"/api/auth/register\", &request)"));
        let post = client.split("pub async fn post<").nth(1).unwrap().split("\n    }\n").next().unwrap();
        assert!(post.contains(".json(body)") && !post.contains("log::") && !post.contains("console"));
        // Its Debug never prints it.
        assert!(auth.contains(".field(\"kyc_claim_secret\", &self.kyc_claim_secret.as_ref().map(|_| \"[redacted]\"))"));
        assert!(!auth.contains("#[derive(Debug, Serialize)]\npub struct IdentityRegisterRequest"));
    }

    #[test]
    fn the_session_reads_send_the_token_in_a_header_never_the_url() {
        let src = include_str!("login.rs").replace("\r\n", "\n");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for route in ["/api/kyc/session/", "/api/kyc/status/"] {
            let after = code.split(route).nth(1).unwrap_or_else(|| panic!("{route} is read"));
            let read = after.split(".send()").next().unwrap();
            assert!(read.contains(".header(\"X-KYC-Token\", "), "{route} sends X-KYC-Token");
        }
        for line in code.lines().filter(|l| l.contains("/api/kyc/")) {
            assert!(!line.contains("?token") && !line.contains("&token") && !line.contains("?t="), "a token in a URL: {line}");
        }
    }
}

#[cfg(test)]
mod visible_copy_tests {
    /// The string literals of this file's code (its tests left out), skipping comments and the raw strings (r#"..."#),
    /// which hold the downloaded identity file and its README rather than words on the page. The showcase (the build
    /// plate, its readout and the tagline) is scanned like the rest.
    fn page_strings() -> Vec<String> {
        let src = include_str!("login.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let code: Vec<char> = code.chars().collect();
        let (mut out, mut i) = (Vec::new(), 0);
        while i < code.len() {
            match code[i] {
                '/' if code.get(i + 1) == Some(&'/') => {
                    while i < code.len() && code[i] != '\n' {
                        i += 1;
                    }
                }
                'r' if code.get(i + 1) == Some(&'#') && code.get(i + 2) == Some(&'"') => {
                    i += 3;
                    while i + 1 < code.len() && !(code[i] == '"' && code[i + 1] == '#') {
                        i += 1;
                    }
                    i += 2;
                }
                '\'' if code.get(i + 1) == Some(&'\\') => i += 4,
                '\'' if code.get(i + 2) == Some(&'\'') => i += 3,
                '"' => {
                    let mut s = String::new();
                    i += 1;
                    while i < code.len() && code[i] != '"' {
                        if code[i] == '\\' {
                            s.push(code[i]);
                            i += 1;
                        }
                        s.push(code[i]);
                        i += 1;
                    }
                    i += 1;
                    out.push(s);
                }
                _ => i += 1,
            }
        }
        out
    }

    #[test]
    fn the_words_on_the_login_page_carry_no_dashes_and_no_games_outside_the_showcase() {
        let strings = page_strings();
        assert!(strings.len() > 200, "the scan found the page's strings: {}", strings.len());
        for step in ["Step 1 of 3: Basic Info", "Step 2 of 3: Identity Verification", "Step 3 of 3: Create Your Identity"] {
            assert!(strings.iter().any(|s| s == step), "{step}");
        }
        for s in &strings {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "a dash in words on the page: {s}");
            assert!(!s.to_lowercase().contains("games"), "Eustress is a simulation and data platform, not a game engine: {s}");
        }
        // the showcase's heading and tagline
        assert!(strings.iter().any(|s| s == "Create Excellence"));
        assert!(strings.iter().any(|s| s == "Build simulations, connect real data, and run them in the engine."));
        // the sign-in line says what to do in plain words
        assert!(strings.iter().any(|s| s == "Load your identity file to sign in. There is no password."));
        assert!(!strings.iter().any(|s| s.contains("challenge-response")), "no jargon on the page");
    }
}
