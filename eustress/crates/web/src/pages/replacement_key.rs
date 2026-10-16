// =============================================================================
// Eustress Web - A replacement key, for someone who lost their identity file
// =============================================================================
// Without the identity file nobody can sign in, and no email can fix that: the
// file is the key. After a person at Eustress has checked who the owner is, an
// administrator gives the account a new public key (docs/architecture/
// IDENTITY_KEY_ROTATION_AND_RECOVERY.md). This page makes that key: the browser
// generates the pair and the identity file, saves the file, and shows only the
// PUBLIC half, which is what is sent to support. The private key never leaves
// the browser. Nothing here touches the account; until support replaces the old
// key, the old key is the only one that works.
// =============================================================================

use leptos::prelude::*;

use crate::api;
use crate::components::result_card::{Glyph, ResultCard, ResultPage, Tone};
use crate::pages::login::{download_file, generate_identity_toml_for, sign_challenge_ed25519};
use crate::state::AppState;

/// What the page shows after a key was made.
#[derive(Clone)]
struct Made {
    username: String,
    case_ref: String,
    public_key: String,
    /// The new key's signature over the username and the case: it shows support
    /// that this key is held by whoever sent it.
    signature: String,
}

/// A page to make a replacement key, for the lost-file case.
#[component]
pub fn ReplacementKeyPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let username = RwSignal::new(String::new());
    let case_ref = RwSignal::new(String::new());
    let problem = RwSignal::new(String::new());
    let made = RwSignal::new(Option::<Made>::None);

    let make = Callback::new(move |_: ()| {
        let name = username.get_untracked().trim().to_string();
        let case = case_ref.get_untracked().trim().to_string();
        if name.is_empty() {
            problem.set("Type your Eustress username first.".to_string());
            return;
        }
        if case.len() < 3 || !case.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')) {
            problem.set("Type the case number support gave you (letters, digits and _ . : - only).".to_string());
            return;
        }
        match generate_identity_toml_for(&name, None) {
            Ok((toml, public_key, private_key)) => {
                let message = api::admin_replace_key_message(&api::api_origin(&api_url.get_value()), &name, &case, &public_key);
                let Ok(signature) = sign_challenge_ed25519(&private_key, &message) else {
                    problem.set("Could not sign with the new key.".to_string());
                    return;
                };
                // Saved first, so a blocked download is seen before anything is sent.
                download_file(&format!("eustress-{name}.toml"), &toml);
                problem.set(String::new());
                made.set(Some(Made { username: name, case_ref: case, public_key, signature }));
            }
            Err(e) => problem.set(format!("Could not make a new key: {e}")),
        }
    });

    view! {
        <ResultPage>
            {move || match made.get() {
                None => view! {
                    <ResultCard tone=Tone::Info glyph=Glyph::Key title="Lost your identity file?" wide=true>
                        <p>"Open a case with support first, then make a new key here. Support replaces your old one after checking who you are."</p>
                        <p class="result-why">
                            "Your identity file is the key to your account, so an email cannot sign you back in. "
                            "If you can still sign in on another computer, use Settings, Security, Identity key instead."
                        </p>
                        {move || (!problem.get().is_empty()).then(|| view! { <p class="result-problem" role="alert">{problem.get()}</p> })}
                        <div class="form-field">
                            <label class="form-label" for="replacement-username">"Your Eustress username"</label>
                            <input id="replacement-username" type="text" class="form-input" autocomplete="username" prop:value=move || username.get()
                                on:input=move |ev| username.set(event_target_value(&ev)) />
                        </div>
                        <div class="form-field">
                            <label class="form-label" for="replacement-case">"Case number from support"</label>
                            <input id="replacement-case" type="text" class="form-input" prop:value=move || case_ref.get()
                                on:input=move |ev| case_ref.set(event_target_value(&ev)) />
                        </div>
                        <div class="result-actions">
                            <button class="btn btn-primary" on:click=move |_| make.run(())>"Make a new key"</button>
                        </div>
                        <p class="result-why">
                            "Your browser makes the key and saves a new identity file. Nothing is sent to Eustress, and your account is not changed yet."
                        </p>
                    </ResultCard>
                }.into_any(),
                Some(m) => {
                    let message = format!(
                        "Username: {}\nCase: {}\nNew public key: {}\nSignature: {}\n",
                        m.username, m.case_ref, m.public_key, m.signature
                    );
                    view! {
                        <ResultCard tone=Tone::Ok glyph=Glyph::Check title="Your new identity file was saved" wide=true>
                            <p>{format!("It is called eustress-{}.toml. Keep it: it is the only file that will work once support replaces your key.", m.username)}</p>
                            <p>"Now write to support@eustress.dev from the email address on your account, and send ONLY this (the signature shows support that the key is yours):"</p>
                            <pre class="replacement-key-public">{message}</pre>
                            <p class="result-warning">"Never send the file itself."</p>
                            <p class="result-why">
                                "Support will check who you are, then replace your key. Until then your old key is the one that works, "
                                "and after the replacement payouts and transfers are paused for a few days as a safety step."
                            </p>
                        </ResultCard>
                    }.into_any()
                }
            }}
        </ResultPage>
    }
}
