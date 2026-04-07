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
use crate::components::CentralNav;
use crate::state::AppState;

/// Login/Register page with tabs.
#[component]
pub fn LoginPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let navigate = use_navigate();

    // Tab state
    let is_register = RwSignal::new(false);

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
    let kyc_session_id = RwSignal::new(uuid::Uuid::new_v4().to_string()); // links uploads to registration
    let kyc_status_text = RwSignal::new(String::new()); // "Verifying...", "Verified as John Doe", "Rejected"
    let kyc_verified = RwSignal::new(false);
    let kyc_rejected = RwSignal::new(false);
    let kyc_verified_name = RwSignal::new(String::new()); // name extracted by Grok
    let kyc_reject_reason = RwSignal::new(String::new());

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
        if let Ok(resp) = gloo_net::http::Request::get("https://api.eustress.dev/api/kyc/jurisdiction").send().await {
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

    view! {
        <div class="page page-login">
            <CentralNav active="".to_string() />

            <div class="login-background">
                <div class="bg-grid"></div>
                <div class="bg-glow bg-glow-1"></div>
                <div class="bg-glow bg-glow-2"></div>
            </div>

            <div class="login-wrapper">
                // Left side - showcase
                <div class="login-showcase">
                    <div class="blueprint-grid">
                        <div class="blueprint-icon tilt-1">
                            <img src="/assets/icons/cube.svg" alt="3D Objects" />
                        </div>
                        <div class="blueprint-icon tilt-2">
                            <img src="/assets/icons/dog.svg" alt="Dog" />
                        </div>
                        <div class="blueprint-icon tilt-3">
                            <img src="/assets/icons/sparkles.svg" alt="Effects" />
                        </div>
                        <div class="blueprint-icon tilt-4">
                            <img src="/assets/icons/gamepad.svg" alt="Games" />
                        </div>
                        <div class="blueprint-icon tilt-5">
                            <img src="/assets/icons/rocket.svg" alt="Launch" />
                        </div>
                        <div class="blueprint-icon tilt-6">
                            <img src="/assets/icons/users.svg" alt="Multiplayer" />
                        </div>
                        <div class="blueprint-icon tilt-7">
                            <img src="/assets/icons/network.svg" alt="Connect" />
                        </div>
                        <div class="blueprint-icon tilt-8">
                            <img src="/assets/icons/settings.svg" alt="Tools" />
                        </div>
                        <div class="blueprint-icon tilt-9">
                            <img src="/assets/icons/trending.svg" alt="Growth" />
                        </div>
                    </div>
                    <div class="showcase-tagline">
                        <h2>"Create Without Limits"</h2>
                        <p>"Build games, simulations, and experience the power of Eustress"</p>
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
                                "Load your "
                                <code>"identity.toml"</code>
                                " to sign in with Ed25519 challenge-response."
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
                                                                error.set(Some("Invalid identity.toml — missing public_key or private_key".to_string()));
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
                                                "Identity loaded — ready to sign in"
                                            } else {
                                                "Choose eustress-username.toml or drag here"
                                            }
                                        }}
                                    </div>
                                </div>
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

                                        let signature = match sign_challenge_ed25519(&priv_key, &challenge_resp.challenge) {
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
                                    <p class="step-indicator">"Step 1 of 3 — Basic Info"</p>

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
                                    </div>

                                    <button
                                        type="button"
                                        class="btn btn-primary"
                                        disabled=move || reg_username.get().trim().is_empty() || reg_email.get().trim().is_empty() || !reg_email.get().contains('@') || reg_birthday.get().is_empty()
                                        on:click=move |_| reg_step.set(2)
                                    >"Continue"</button>
                                </div>
                            })}

                            // Step 2: ID Verification
                            {move || (reg_step.get() == 2).then(|| view! {
                                <div class="register-step">
                                    <p class="step-indicator">"Step 2 of 3 — Identity Verification"</p>

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
                                                    "Upload ID — Front"
                                                } else {
                                                    "Upload ID Document"
                                                }
                                            }}
                                            <span class="required">"*"</span>
                                        </label>
                                        <div class="identity-file-drop">
                                            <input
                                                type="file"
                                                accept="image/*,.pdf"
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
                                                            let sess = kyc_session_id.get();
                                                            kyc_status_text.set("Uploading document...".to_string());
                                                            kyc_verified.set(false);
                                                            kyc_rejected.set(false);
                                                            spawn_local(async move {
                                                                match upload_id_document(&file, "front", &id_type, &id_number, &birthday, &uname, &sess).await {
                                                                    Ok(resp) => {
                                                                        reg_id_front_uploaded.set(true);
                                                                        kyc_status_text.set("Verifying your identity...".to_string());
                                                                        // Start polling for verification result
                                                                        poll_kyc_status(
                                                                            resp.verification_id,
                                                                            kyc_status_text,
                                                                            kyc_verified_name,
                                                                            kyc_verified,
                                                                            kyc_rejected,
                                                                            kyc_reject_reason,
                                                                        ).await;
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
                                            "Upload ID — Back"
                                            <span class="required">"*"</span>
                                        </label>
                                        <div class="identity-file-drop">
                                            <input
                                                type="file"
                                                accept="image/*,.pdf"
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
                                                            let sess = kyc_session_id.get();
                                                            spawn_local(async move {
                                                                match upload_id_document(&file, "back", &id_type, &id_number, &birthday, &uname, &sess).await {
                                                                    Ok(_) => reg_id_back_uploaded.set(true),
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

                                    <p class="form-hint">
                                        "Your ID images are encrypted and stored securely. "
                                        "They are only used for one-time verification."
                                    </p>

                                    // ── Verification Status Feedback ──
                                    {move || {
                                        let status = kyc_status_text.get();
                                        let verified = kyc_verified.get();
                                        let rejected = kyc_rejected.get();
                                        let name = kyc_verified_name.get();
                                        let reason = kyc_reject_reason.get();

                                        if status.is_empty() {
                                            None
                                        } else if verified {
                                            Some(view! {
                                                <div class="kyc-status kyc-verified">
                                                    <span class="kyc-icon">"✓"</span>
                                                    <div>
                                                        <strong>"Identity Verified"</strong>
                                                        <p>{format!("Verified as {}", if name.is_empty() { "you".to_string() } else { name })}</p>
                                                    </div>
                                                </div>
                                            })
                                        } else if rejected {
                                            Some(view! {
                                                <div class="kyc-status kyc-rejected">
                                                    <span class="kyc-icon">"✗"</span>
                                                    <div>
                                                        <strong>"Verification Failed"</strong>
                                                        <p>{if reason.is_empty() { "Please try again with a clearer photo".to_string() } else { reason }}</p>
                                                    </div>
                                                </div>
                                            })
                                        } else {
                                            Some(view! {
                                                <div class="kyc-status kyc-processing">
                                                    <span class="kyc-spinner"></span>
                                                    <span>{status}</span>
                                                </div>
                                            })
                                        }
                                    }}

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
                                        <p class="step-indicator">"Step 3 of 3 — Create Your Identity"</p>

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

                                                match generate_identity_toml(&uname, &bday, &idt, &idn) {
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
                                                            match api::register_identity(
                                                                &client,
                                                                &uname_clone,
                                                                &pub_key_clone,
                                                                Some(&bday_clone),
                                                                Some(&idt_clone),
                                                                Some(&id_hash),
                                                                Some(&sess),
                                                            ).await {
                                                                Ok(response) => {
                                                                    // Registration succeeded — download both files
                                                                    let toml_filename = format!("eustress-{}.toml", uname_clone);
                                                                    download_file(&toml_filename, &toml_content);
                                                                    download_file("README - Eustress Identity.txt", &identity_readme(&uname_clone));

                                                                    // Email a backup copy of identity.toml (fire-and-forget)
                                                                    if !email_clone.is_empty() {
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
                                                                    identity_status.set("Registered — save your eustress identity file!".to_string());
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
                                            " is your account — keep it safe."
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
struct KycUploadResponse {
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

async fn upload_id_document(
    file: &web_sys::File,
    side: &str,
    id_type: &str,
    id_number: &str,
    birthday: &str,
    username: &str,
    session_id: &str,
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

    let api_url = "https://api.eustress.dev".to_string();

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

/// Poll KYC verification status until resolved (verified/rejected/error)
async fn poll_kyc_status(
    verification_id: String,
    status_signal: RwSignal<String>,
    verified_name_signal: RwSignal<String>,
    kyc_verified_signal: RwSignal<bool>,
    kyc_rejected_signal: RwSignal<bool>,
    reject_reason_signal: RwSignal<String>,
) {
    let api_url = "https://api.eustress.dev";
    let url = format!("{}/api/kyc/status/{}", api_url, verification_id);

    // Poll every 2 seconds, max 60 attempts (2 minutes)
    for _ in 0..60 {
        gloo_timers::future::TimeoutFuture::new(2_000).await;

        let resp = match gloo_net::http::Request::get(&url).send().await {
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
                status_signal.set("Verification error — please retry".to_string());
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
    status_signal.set("Verification timed out — please retry".to_string());
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

fn generate_identity_toml(
    username: &str,
    birthday: &str,
    id_type: &str,
    id_number: &str,
) -> Result<(String, String, String), String> {
    use bliss_crypto::KeyPair;

    let keypair = KeyPair::generate();
    let pub_hex = keypair.public_key().to_hex();
    let priv_hex = keypair.private_key().to_hex();

    let now = chrono::Utc::now().to_rfc3339();
    let user_id = uuid::Uuid::new_v4().to_string();

    let id_hash = sha256_hex(&format!("{}:{}:{}", id_type, id_number, birthday));

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

[profile]
birthday = "{birthday}"

[verification]
id_type = "{id_type}"
id_hash = "{id_hash}"
verified = false
"#);

    Ok((toml_content, pub_hex, priv_hex))
}

fn sha256_hex(input: &str) -> String {
    let hash = bliss_crypto::sha256(input.as_bytes());
    hash.to_hex()
}

fn sign_challenge_ed25519(private_key_hex: &str, challenge: &str) -> Result<String, String> {
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

    let signature = Signature::sign(challenge.as_bytes(), &private_key)
        .map_err(|e| format!("Signing failed: {}", e))?;

    // Return signature as hex (matches what the Worker expects)
    Ok(private_key_to_hex_sig(&signature.to_bytes()))
}

fn private_key_to_hex_sig(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn download_file(filename: &str, content: &str) {
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
