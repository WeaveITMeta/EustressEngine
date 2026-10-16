// =============================================================================
// Eustress Web - Replace my key (Settings, Security)
// =============================================================================
// A member replaces the identity key their account signs in with. They load
// their current identity file (to sign with it: a stolen session alone cannot
// take the account), see what the key owns, choose what to revoke, and get a
// new identity file. The new key pair is made here in the browser; the private
// key never leaves it. Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient, ApiError};
use crate::components::identity_file_picker::IdentityFilePicker;
use crate::identity_file::{check_against_account, IdentityFile};
use crate::pages::login::{download_file, generate_identity_toml_for, sign_challenge_ed25519};
use crate::pages::passkeys::{key_change_blocked, PASSKEY_SESSION_SENTENCE};
use crate::state::{AppState, AuthState};

/// The words for a failed call: the Worker's own sentence when it sent one.
fn failure_text(error: &ApiError) -> String {
    api::failure_sentence(error)
}

/// A time given as milliseconds since 1970, in words, or `None` when it is not
/// in the future (nothing is frozen).
fn future_time(ms: f64, now_ms: f64) -> Option<String> {
    if !ms.is_finite() || ms <= now_ms {
        return None;
    }
    Some(crate::time_words::when_ms(ms)).filter(|t| !t.is_empty())
}

fn now_ms() -> f64 {
    #[cfg(not(feature = "ssr"))]
    {
        js_sys::Date::now()
    }
    #[cfg(feature = "ssr")]
    {
        0.0
    }
}

/// The passkeys ticked when the list first appears: all of them. They sign in to the website on
/// their own, so a replacement that leaves them behind leaves a way in the old key's owner may still hold.
fn default_revoke_passkeys(credentials: &api::Credentials) -> Vec<String> {
    credentials.passkeys.iter().map(|p| p.id.clone()).collect()
}

/// What was revoked, in words, for the page that follows a replacement.
fn revoked_summary(revoke: &api::RevokeList) -> String {
    format!(
        "{} API key(s), {} webhook endpoint(s), {} passkey(s){}",
        revoke.api_keys.len(),
        revoke.webhooks.len(),
        revoke.passkeys.len(),
        if revoke.discord { ", the Discord link" } else { "" }
    )
}

/// What the page shows after a replacement.
#[derive(Clone)]
struct Finished {
    username: String,
    /// The new identity file, kept in memory so it can be downloaded again.
    new_file: String,
    emailed_to: Option<String>,
    frozen_until: Option<String>,
    pinned_until: Option<String>,
    revoked: String,
}

/// The "Replace my key" group of the Security tab.
#[component]
pub fn ReplaceKeySection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;

    let current = RwSignal::new(Option::<IdentityFile>::None);
    let problem = RwSignal::new(String::new());
    let begun = RwSignal::new(Option::<api::RotateBegin>::None);
    let revoke_keys = RwSignal::new(Vec::<String>::new());
    let revoke_hooks = RwSignal::new(Vec::<String>::new());
    let revoke_discord = RwSignal::new(false);
    let revoke_passkeys = RwSignal::new(Vec::<String>::new());
    let email_it = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let finished = RwSignal::new(Option::<Finished>::None);

    let passkey_session = move || auth.with(|a| key_change_blocked(a.user()));

    let account_email = move || match auth.get() {
        AuthState::Authenticated(user) => user.email.clone().filter(|e| !e.is_empty()),
        _ => None,
    };

    // Step 1 is the picker: the current file, read into memory. Step 2: a challenge, and what the key owns. The file must be the one the
    // account signs in with now.
    let check_file = Callback::new(move |_: ()| {
        let Some(file) = current.get_untracked() else { return };
        busy.set(true);
        problem.set(String::new());
        spawn_local(async move {
            let url = api_url.get_value();
            let client = ApiClient::new(&url);
            match api::rotate_begin(&client).await {
                Ok(b) => {
                    if let Err(why) = check_against_account(&file, &b.public_key, &b.account_id, &api::api_origin(&url)) {
                        problem.set(why);
                    } else if b.origin != api::api_origin(&url) {
                        problem.set("The address of Eustress did not match this page. Nothing was changed.".to_string());
                    } else {
                        revoke_keys.set(Vec::new());
                        revoke_hooks.set(Vec::new());
                        revoke_discord.set(false);
                        revoke_passkeys.set(default_revoke_passkeys(&b.credentials));
                        begun.set(Some(b));
                    }
                }
                Err(e) => problem.set(failure_text(&e)),
            }
            busy.set(false);
        });
    });

    // Step 3: make the new key pair, save its file, sign twice, replace.
    let replace = Callback::new(move |_: ()| {
        let (Some(old), Some(b)) = (current.get_untracked(), begun.get_untracked()) else { return };
        let (username, email) = match auth.get_untracked() {
            AuthState::Authenticated(u) => (u.username.clone(), u.email.clone().filter(|e| !e.is_empty())),
            _ => {
                problem.set("Sign in first.".to_string());
                return;
            }
        };
        let revoke = api::RevokeList {
            api_keys: revoke_keys.get_untracked(),
            webhooks: revoke_hooks.get_untracked(),
            discord: revoke_discord.get_untracked(),
            passkeys: revoke_passkeys.get_untracked(),
        };
        let email_new_file = email_it.get_untracked();
        let state = app_state.clone();
        busy.set(true);
        problem.set(String::new());
        spawn_local(async move {
            let url = api_url.get_value();
            let client = ApiClient::new(&url);

            // The new pair, as an identity file that names the account itself.
            let (toml, new_public, new_private) = match generate_identity_toml_for(&username, Some(&b.account_id)) {
                Ok(t) => t,
                Err(e) => {
                    problem.set(format!("Could not make a new key: {e}"));
                    busy.set(false);
                    return;
                }
            };
            let message = api::rotation_message(&api::api_origin(&url), &b.account_id, &old.public_key, &new_public, &b.challenge);
            let (old_signature, new_signature) = match (sign_challenge_ed25519(&old.private_key, &message), sign_challenge_ed25519(&new_private, &message)) {
                (Ok(o), Ok(n)) => (o, n),
                _ => {
                    problem.set("Could not sign. Check the identity file and try again.".to_string());
                    busy.set(false);
                    return;
                }
            };

            // The new file is saved BEFORE the key is replaced, so a blocked
            // download cannot leave the account with a key nobody holds. A key
            // that is never used is harmless.
            let file_name = format!("eustress-{username}.toml");
            download_file(&file_name, &toml);

            let request = api::RotateRequest { new_public_key: new_public, challenge: b.challenge.clone(), old_signature, new_signature, revoke };
            match api::rotate_key(&client, &request).await {
                Ok(reply) => {
                    let mut emailed_to = None;
                    if email_new_file {
                        if let Some(address) = email.clone() {
                            let body = serde_json::json!({ "email": address, "username": username, "toml_content": toml });
                            let mailed: Result<serde_json::Value, ApiError> = ApiClient::new(&url).post("/api/identity/email-backup", &body).await;
                            if mailed.is_ok() {
                                emailed_to = Some(address);
                            }
                        }
                    }
                    let now = now_ms();
                    let revoked = revoked_summary(&request.revoke);
                    let frozen_until = future_time(reply.value_frozen_until, now);
                    let pinned_until = future_time(reply.payout_pinned_until, now);
                    state.login_with_token(reply.token, reply.user);
                    finished.set(Some(Finished { username, new_file: toml, emailed_to, frozen_until, pinned_until, revoked }));
                    current.set(None);
                    begun.set(None);
                }
                Err(e) => problem.set(failure_text(&e)),
            }
            busy.set(false);
        });
    });

    view! {
        <div class="settings-group" id="replace-key">
            <h2 class="group-title">"Identity file"</h2>

            {move || finished.get().map(|done| {
                let file_name = format!("eustress-{}.toml", done.username);
                let download_again = {
                    let (name, content) = (file_name.clone(), done.new_file.clone());
                    move |_| download_file(&name, &content)
                };
                view! {
                    <div class="alert alert-success" role="status">
                        <h3>"Your key was replaced"</h3>
                        <ul class="replace-key-list">
                            <li>{format!("Your new identity file was saved as {file_name}.")}</li>
                            {done.emailed_to.clone().map(|a| view! { <li>{format!("A copy was emailed to {a}, as a plain file.")}</li> })}
                            <li>"Your old identity file no longer signs in to Eustress, and every other sign-in was ended. Copy the new file to every computer you use."</li>
                            {done.frozen_until.clone().map(|t| view! { <li>{format!("As a safety step, payouts, withdrawals and transfers are paused until {t}.")}</li> })}
                            {done.pinned_until.clone().map(|t| view! { <li>{format!("The payout bank account cannot be changed until {t}.")}</li> })}
                            <li>{format!("Revoked at your request: {}.", done.revoked)}</li>
                            <li>"Delete old copies of your earlier files, including the emails that carried them."</li>
                        </ul>
                        <button class="btn btn-secondary" on:click=download_again>"Download the new file again"</button>
                    </div>
                }
            })}

            <Show when=move || finished.get().is_none()>
                <p class="info-text">
                    "There is no password: your identity file signs you in. It holds the private key to your account. Replace it if a copy may have been seen, for example in an old email. "
                    "You need your current identity file. You get a new one; the old one stops working and every other sign-in ends."
                </p>

                {move || passkey_session().then(|| view! { <p class="info-text" role="status">{PASSKEY_SESSION_SENTENCE}</p> })}

                <IdentityFilePicker current=current id="replace-key-file" on_pick=Callback::new(move |_: ()| { begun.set(None); problem.set(String::new()); }) />

                {move || (!problem.get().is_empty()).then(|| view! { <div class="alert alert-error" role="alert">{problem.get()}</div> })}

                <Show when=move || current.get().is_some() && begun.get().is_none()>
                    <button class="btn btn-primary" disabled=move || busy.get() || passkey_session() on:click=move |_| check_file.run(())>
                        {move || if busy.get() { "Checking..." } else { "Check my file" }}
                    </button>
                </Show>

                {move || begun.get().map(|b| {
                    let creds = b.credentials.clone();
                    let any = !creds.api_keys.is_empty() || !creds.webhooks.is_empty() || creds.discord_linked || !creds.passkeys.is_empty();
                    view! {
                        <div class="replace-key-step">
                            <p class="file-ok" role="status">"This file is the key your account signs in with. Choose what to revoke below, then press Replace my key."</p>
                            {any.then(|| view! {
                                <h3>"Revoke what the old key made?"</h3>
                                <p class="info-text">"These act as your account and do not end when you replace the key. Tick the ones to revoke now."</p>
                            })}
                            {creds.api_keys.into_iter().map(|k| {
                                let id = k.id.clone();
                                let id_for_change = id.clone();
                                view! {
                                    <label class="checkbox-row">
                                        <input type="checkbox" on:change=move |ev| {
                                            let on = event_target_checked(&ev);
                                            revoke_keys.update(|v| { v.retain(|x| x != &id_for_change); if on { v.push(id_for_change.clone()); } });
                                        } />
                                        <span>{format!("API key {} ({}...{}, {})", k.name, k.key_prefix, k.last4, k.mode)}</span>
                                    </label>
                                }
                            }).collect_view()}
                            {creds.webhooks.into_iter().map(|w| {
                                let id = w.id.clone();
                                view! {
                                    <label class="checkbox-row">
                                        <input type="checkbox" on:change=move |ev| {
                                            let on = event_target_checked(&ev);
                                            revoke_hooks.update(|v| { v.retain(|x| x != &id); if on { v.push(id.clone()); } });
                                        } />
                                        <span>{format!("Webhook endpoint at {} ({})", w.host, if w.livemode { "live" } else { "test" })}</span>
                                    </label>
                                }
                            }).collect_view()}
                            {(!creds.passkeys.is_empty()).then(|| view! {
                                <p class="info-text">"Passkeys sign in to this website with no identity file, so they keep working after a replacement unless you remove them. They are ticked so they are removed; untick one to keep it."</p>
                            })}
                            {creds.passkeys.into_iter().map(|p| {
                                let id = p.id.clone();
                                let id_for_change = p.id.clone();
                                let shown_name = if p.name.is_empty() { "Passkey".to_string() } else { p.name.clone() };
                                view! {
                                    <label class="checkbox-row">
                                        <input type="checkbox" prop:checked=move || revoke_passkeys.with(|v| v.contains(&id)) on:change=move |ev| {
                                            let on = event_target_checked(&ev);
                                            revoke_passkeys.update(|v| { v.retain(|x| x != &id_for_change); if on { v.push(id_for_change.clone()); } });
                                        } />
                                        <span>{format!("Passkey {shown_name} (signs in to this website with no file)")}</span>
                                    </label>
                                }
                            }).collect_view()}
                            {creds.discord_linked.then(|| view! {
                                <label class="checkbox-row">
                                    <input type="checkbox" on:change=move |ev| revoke_discord.set(event_target_checked(&ev)) />
                                    <span>"The linked Discord account"</span>
                                </label>
                            })}
                            {(!creds.complete).then(|| view! { <p class="info-text">"Some items could not be listed. You can revoke them from your project settings afterwards."</p> })}
                            {creds.payout_connected.then(|| view! { <p class="info-text">"Your payout bank account stays connected; it cannot be changed for a while after a replacement."</p> })}

                            {move || account_email().map(|address| view! {
                                <label class="checkbox-row">
                                    <input type="checkbox" prop:checked=move || email_it.get() on:change=move |ev| email_it.set(event_target_checked(&ev)) />
                                    <span>{format!("Also email the new file to {address} (a plain file, as when you signed up)")}</span>
                                </label>
                            })}

                            <p class="info-text">
                                "When you click the button your browser makes a new key, saves the new identity file to your computer, and replaces the old key. "
                                "Do not skip saving the file: it is the only copy that signs in from now on."
                            </p>
                            <button class="btn btn-primary" disabled=move || busy.get() || passkey_session() on:click=move |_| replace.run(())>
                                {move || if busy.get() { "Replacing..." } else { "Replace my key" }}
                            </button>
                        </div>
                    }
                })}
            </Show>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_file_is_first_on_the_security_tab_and_there_is_no_password_card() {
        let settings = include_str!("settings.rs");
        let security = &settings[settings.find(r#"active_tab.get() == "security""#).expect("a Security tab")..];
        let replace = security.find("<ReplaceKeySection />").expect("the identity file is on the Security tab");
        assert!(replace < security.find("<EmailSection />").unwrap(), "and comes first");
        assert!(!settings.contains(r#""Password"</h2>"#) && !settings.contains("Change Password"), "no password card");
        assert!(include_str!("replace_key.rs").contains(r#"<h2 class="group-title">"Identity file"</h2>"#));
    }

    #[test]
    fn a_time_in_the_future_is_shown_and_a_past_or_missing_one_is_not() {
        let now = 1_800_000_000_000.0;
        assert_eq!(future_time(now + 86_400_000.0, now).as_deref(), Some("Jan 16, 2027, 8:00 AM UTC"));
        assert_eq!(future_time(0.0, now), None);
        assert_eq!(future_time(now - 1.0, now), None);
        assert_eq!(future_time(f64::NAN, now), None);
    }

    fn credentials_with_passkeys() -> api::Credentials {
        serde_json::from_value(serde_json::json!({
            "api_keys": [{ "id": "k1", "name": "CI" }],
            "passkeys": [{ "id": "AQID", "name": "Laptop" }, { "id": "BAUG", "name": "Phone" }]
        }))
        .expect("the credentials parse")
    }

    #[test]
    fn every_passkey_is_ticked_by_default_and_the_api_keys_are_not() {
        let creds = credentials_with_passkeys();
        assert_eq!(default_revoke_passkeys(&creds), vec!["AQID".to_string(), "BAUG".to_string()]);
        assert!(default_revoke_passkeys(&api::Credentials::default()).is_empty());
        // An answer from before passkeys has none to list.
        let old: api::Credentials = serde_json::from_str(r#"{ "api_keys": [] }"#).unwrap();
        assert!(old.passkeys.is_empty());
    }

    #[test]
    fn the_request_names_the_passkeys_to_revoke_under_revoke_passkeys() {
        let revoke = api::RevokeList { passkeys: vec!["AQID".into()], ..Default::default() };
        let body = serde_json::to_value(api::RotateRequest {
            new_public_key: "n".into(),
            challenge: "c".into(),
            old_signature: "o".into(),
            new_signature: "s".into(),
            revoke,
        })
        .unwrap();
        assert_eq!(body["revoke"]["passkeys"], serde_json::json!(["AQID"]));
        assert_eq!(body["revoke"]["api_keys"], serde_json::json!([]));
    }

    #[test]
    fn the_page_after_a_replacement_counts_the_passkeys_removed() {
        let revoke = api::RevokeList { api_keys: vec!["a".into()], webhooks: vec![], discord: true, passkeys: vec!["x".into(), "y".into()] };
        assert_eq!(revoked_summary(&revoke), "1 API key(s), 0 webhook endpoint(s), 2 passkey(s), the Discord link");
        assert_eq!(revoked_summary(&api::RevokeList::default()), "0 API key(s), 0 webhook endpoint(s), 0 passkey(s)");
    }

    #[test]
    fn the_replace_group_lists_passkeys_ticked_and_sends_them() {
        let source = include_str!("replace_key.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap()];
        assert!(source.contains("revoke_passkeys.set(default_revoke_passkeys(&b.credentials))"), "ticked when the list appears");
        assert!(source.contains("passkeys: revoke_passkeys.get_untracked()"), "and sent");
        assert!(source.contains("prop:checked=move || revoke_passkeys.with("), "each box shows whether it is ticked");
    }

    #[test]
    fn a_failure_is_in_the_workers_words_when_it_has_some() {
        let e = ApiError::Server { status: 401, message: "{\"error\":\"Signature verification failed\",\"code\":\"signature_invalid\"}".to_string() };
        assert_eq!(failure_text(&e), "Signature verification failed");
        let plain = ApiError::Server { status: 502, message: "bad gateway".to_string() };
        assert!(failure_text(&plain).contains("502"));
        assert!(failure_text(&ApiError::Unauthorized).contains("Sign in again"));
    }
}
