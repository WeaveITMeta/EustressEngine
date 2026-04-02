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

    // Jurisdiction detection (from Cloudflare cdn-cgi/trace)
    let detected_country = RwSignal::new("US".to_string());
    let accepted_ids = RwSignal::new(vec![
        "Passport".to_string(),
        "Driver's licence".to_string(),
        "National ID".to_string(),
    ]);
    let jurisdiction_name = RwSignal::new("Detecting...".to_string());

    // Detect jurisdiction on load
    spawn_local(async move {
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
                                                "Choose identity.toml or drag here"
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
                                        disabled=move || reg_username.get().trim().is_empty() || reg_birthday.get().is_empty()
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
                                                            let sess = kyc_session_id.get();
                                                            spawn_local(async move {
                                                                match upload_id_document(&file, "front", &id_type, &sess).await {
                                                                    Ok(_) => reg_id_front_uploaded.set(true),
                                                                    Err(e) => {
                                                                        error.set(Some(format!("Front upload failed: {}", e)));
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
                                                            let sess = kyc_session_id.get();
                                                            spawn_local(async move {
                                                                match upload_id_document(&file, "back", &id_type, &sess).await {
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
                                            }
                                            on:click=move |_| reg_step.set(3)
                                        >"Continue"</button>
                                    </div>
                                </div>
                            })}

                            // Step 3: Generate Identity
                            {move || (reg_step.get() == 3).then(|| {
                                let username = reg_username.get();
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
                                                let bday = birthday.clone();
                                                let idt = id_type.clone();
                                                let idn = id_number.clone();

                                                match generate_identity_toml(&uname, &bday, &idt, &idn) {
                                                    Ok((toml_content, pub_key, priv_key)) => {
                                                        // Download the TOML file
                                                        download_file("identity.toml", &toml_content);

                                                        let pub_key_clone = pub_key.clone();
                                                        let uname_clone = uname.clone();
                                                        let bday_clone = bday.clone();
                                                        let idt_clone = idt.clone();
                                                        let id_hash = sha256_hex(&format!("{}:{}:{}", idt, idn, bday));

                                                        // Set local state
                                                        identity_public_key.set(pub_key);
                                                        identity_private_key.set(priv_key);
                                                        identity_username.set(uname_clone.clone());
                                                        identity_file_loaded.set(true);

                                                        // Register with the node API
                                                        loading.set(true);
                                                        error.set(None);
                                                        identity_status.set("Registering with node...".to_string());

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
                                                                    loading.set(false);
                                                                    identity_status.set("Registered and signed in!".to_string());
                                                                    app_state_c.login_with_token(response.token, response.user);
                                                                    nav("/dashboard", Default::default());
                                                                }
                                                                Err(e) => {
                                                                    loading.set(false);
                                                                    identity_status.set("Identity created — save your file to cloud or desktop.".to_string());
                                                                    error.set(Some(format!("Registration: {} — you can still sign in with your downloaded identity.toml", e)));
                                                                    is_register.set(false);
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
async fn upload_id_document(
    file: &web_sys::File,
    side: &str,
    id_type: &str,
    session_id: &str,
) -> Result<String, String> {
    let form_data = web_sys::FormData::new()
        .map_err(|_| "Failed to create FormData".to_string())?;
    form_data
        .append_with_blob_and_filename("document", file, &file.name())
        .map_err(|_| "Failed to append file".to_string())?;
    form_data
        .append_with_str("side", side)
        .map_err(|_| "Failed to append side".to_string())?;
    form_data
        .append_with_str("id_type", id_type)
        .map_err(|_| "Failed to append id_type".to_string())?;
    form_data
        .append_with_str("session_id", session_id)
        .map_err(|_| "Failed to append session_id".to_string())?;

    // KYC uploads always go to Cloudflare Worker (R2 is only there)
    let api_url = "https://api.eustress.dev".to_string();

    // Get auth token if available
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
        Ok(text)
    } else {
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Upload error ({}): {}", resp.status(), text))
    }
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
    let priv_b64 = base64_encode(&keypair.private_key().to_bytes());

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
private_key = "{priv_b64}"
issued_at = "{now}"

[profile]
birthday = "{birthday}"

[verification]
id_type = "{id_type}"
id_hash = "{id_hash}"
verified = false
"#);

    Ok((toml_content, pub_hex, priv_b64))
}

fn sha256_hex(input: &str) -> String {
    let hash = bliss_crypto::sha256(input.as_bytes());
    hash.to_hex()
}

fn sign_challenge_ed25519(private_key_b64: &str, challenge: &str) -> Result<String, String> {
    use bliss_crypto::{PrivateKey, Signature};

    let seed_bytes = base64_decode(private_key_b64)
        .map_err(|e| format!("Invalid private key encoding: {}", e))?;

    let private_key = PrivateKey::from_bytes(&seed_bytes)
        .map_err(|e| format!("Invalid private key: {}", e))?;

    let signature = Signature::sign(challenge.as_bytes(), &private_key)
        .map_err(|e| format!("Signing failed: {}", e))?;

    Ok(base64_encode(&signature.to_bytes()))
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
