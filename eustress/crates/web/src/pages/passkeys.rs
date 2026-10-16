// =============================================================================
// Eustress Web - Passkeys (Settings, Security)
// =============================================================================
// A passkey is a second key on the account. It signs in to this website on one
// device with no identity file; the identity file stays the master key, and
// adding or removing a passkey needs it. A session that signed in with a
// passkey can do everything except change keys, so here it only reads the list.
// Contract: docs/architecture/PASSKEYS.md.
//
// Adding: the Worker's "begin" answer gives a challenge and the options; the
// device makes the passkey; the identity file signs the new passkey's id and
// the challenge (so the id is known before it signs); the Worker checks both.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient, ApiError, PasskeyInfo};
use crate::components::identity_file_picker::IdentityFilePicker;
use crate::identity_file::IdentityFile;
use crate::pages::login::sign_challenge_ed25519;
use crate::state::{AppState, User};
use crate::webauthn;

/// What the group says in a passkey session, and what the Worker's own refusal says too.
pub const PASSKEY_SESSION_SENTENCE: &str =
    "You signed in with a passkey. Sign in with your identity file to add or remove passkeys or replace your identity file.";

/// What the email change and the API key controls say in a passkey session.
pub const EMAIL_AND_KEYS_SENTENCE: &str = "Sign in with your identity file to change your email or create API keys.";

/// The Worker's refusal of a passkey session (403, code identity_file_required), in its own words;
/// `None` for any other failure, which the caller words as it always did.
pub fn identity_file_required_words(error: &ApiError) -> Option<String> {
    let ApiError::Server { status: 403, message } = error else { return None };
    let body: serde_json::Value = serde_json::from_str(message).ok()?;
    if body.get("code").and_then(|c| c.as_str()) != Some("identity_file_required") {
        return None;
    }
    Some(body.get("error").and_then(|e| e.as_str()).filter(|e| !e.is_empty()).unwrap_or("Sign in with your identity file to do this.").to_string())
}

/// A session that signed in with a passkey cannot change keys. `None` (nobody signed in, or an
/// answer from before passkeys) is not blocked here; the Worker is the gate either way.
pub fn key_change_blocked(user: Option<&User>) -> bool {
    user.map(User::signed_in_with_passkey).unwrap_or(false)
}

/// Whether "Add a passkey" can be pressed.
pub fn can_add(webauthn_available: bool, passkey_session: bool, file_chosen: bool, busy: bool) -> bool {
    webauthn_available && !passkey_session && file_chosen && !busy
}

/// Whether a "Remove passkey" button can be pressed.
pub fn can_remove(passkey_session: bool, file_chosen: bool, busy: bool) -> bool {
    !passkey_session && file_chosen && !busy
}

/// The account id inside the user handle the Worker sends (the id's UTF-8 bytes, base64url).
pub fn account_id_from_handle(handle: &str) -> Result<String, String> {
    let bytes = webauthn::b64url_decode(handle)?;
    String::from_utf8(bytes).map_err(|_| "Eustress sent an account id that could not be read.".to_string())
}

/// The chosen file must be for the account that is signed in, when the file says which account it is.
pub fn file_fits_account(file: &IdentityFile, account_id: &str) -> Result<(), String> {
    match &file.account_id {
        Some(id) if !id.eq_ignore_ascii_case(account_id) => {
            Err("This file is for a different account than the one you are signed in to. Pick the identity file for this account.".to_string())
        }
        _ => Ok(()),
    }
}

/// When the Worker takes the passkey the device made, the sentence for it.
fn added_sentence(name: &str) -> String {
    let name = if name.is_empty() { "Passkey" } else { name };
    format!("Added the passkey \"{name}\". You can sign in with it on this device.")
}

/// When the device made a passkey and Eustress then refused it, the sentence says the passkey on
/// the device is not linked to the account, so it can be deleted from the device.
pub fn refused_after_made(why: &str) -> String {
    format!("{why} The passkey your device just made is not linked to your account. You can delete it in your device's passkey settings.")
}

/// The line under a passkey's name for when it was added.
pub fn added_line(info: &PasskeyInfo) -> String {
    match info.created_at.as_ref().map(api::Stamp::words).filter(|w| !w.is_empty()) {
        Some(when) => format!("Added {when}"),
        None => "Added at a time that is not on record".to_string(),
    }
}

/// The line under a passkey's name for when it last signed in.
pub fn last_used_line(info: &PasskeyInfo) -> String {
    match info.last_used_at.as_ref().map(api::Stamp::words).filter(|w| !w.is_empty()) {
        Some(when) => format!("Last used {when}"),
        None => "Not used yet".to_string(),
    }
}

/// The text the identity file signs to add a passkey. The challenge is the Worker's base64url text;
/// the message carries its 32 bytes as hex.
pub fn add_message_to_sign(origin: &str, account_id: &str, credential_id: &str, challenge: &str) -> Result<String, String> {
    Ok(api::passkey_add_message(origin, account_id, credential_id, &webauthn::challenge_hex(challenge)?))
}

/// The text the identity file signs to remove a passkey; see [`add_message_to_sign`].
pub fn remove_message_to_sign(origin: &str, account_id: &str, credential_id: &str, challenge: &str) -> Result<String, String> {
    Ok(api::passkey_remove_message(origin, account_id, credential_id, &webauthn::challenge_hex(challenge)?))
}

/// Make the passkey and add it. The device is asked after the Worker has given a challenge and
/// before the identity file signs, because the signature names the new passkey's id.
async fn add_passkey(url: String, file: IdentityFile, name: String) -> Result<PasskeyInfo, String> {
    let client = ApiClient::new(&url);
    let origin = api::api_origin(&url);
    let begin = api::passkey_add_begin(&client).await.map_err(|e| api::failure_sentence(&e))?;
    if api::api_origin(&begin.api_origin) != origin {
        return Err("The address of Eustress did not match this page. Nothing was changed.".to_string());
    }
    let account_id = account_id_from_handle(&begin.user.id)?;
    file_fits_account(&file, &account_id)?;
    let options = webauthn::CreationOptions::from_begin(&begin)?;

    let made = webauthn::create_credential(&options).await?;
    let message = add_message_to_sign(&origin, &account_id, &made.id(), &begin.challenge)
        .map_err(|why| refused_after_made(&why))?;
    let signature = sign_challenge_ed25519(&file.private_key, &message)
        .map_err(|_| refused_after_made("Could not sign. Check the identity file and try again."))?;
    let body = webauthn::add_body(&begin.challenge, &name, &made, &signature);
    api::passkey_add(&client, &body).await.map_err(|e| refused_after_made(&api::failure_sentence(&e)))
}

/// Remove one passkey, signed by the identity file.
async fn remove_passkey(url: String, file: IdentityFile, id: String, account_id: String) -> Result<(), String> {
    let client = ApiClient::new(&url);
    let origin = api::api_origin(&url);
    file_fits_account(&file, &account_id)?;
    let begin = api::passkey_remove_begin(&client, &id).await.map_err(|e| api::failure_sentence(&e))?;
    if api::api_origin(&begin.api_origin) != origin {
        return Err("The address of Eustress did not match this page. Nothing was changed.".to_string());
    }
    // The Worker names the account it will check the signature for; its word wins over the page's.
    let account_id = begin.account_id.clone().filter(|a| !a.is_empty()).unwrap_or(account_id);
    file_fits_account(&file, &account_id)?;
    let message = remove_message_to_sign(&origin, &account_id, &id, &begin.challenge)?;
    let signature = sign_challenge_ed25519(&file.private_key, &message).map_err(|_| "Could not sign. Check the identity file and try again.".to_string())?;
    api::passkey_remove(&client, &id, &begin.challenge, &signature).await.map_err(|e| api::failure_sentence(&e))
}

#[derive(Clone)]
enum ListState {
    Loading,
    Ready(Vec<PasskeyInfo>),
    Failed(String),
}

/// The "Passkeys" group of the Security tab.
#[component]
pub fn PasskeysSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;

    let current = RwSignal::new(Option::<IdentityFile>::None);
    let name = RwSignal::new(String::new());
    let list = RwSignal::new(ListState::Loading);
    let problem = RwSignal::new(String::new());
    let done = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let supported = RwSignal::new(false);

    let passkey_session = move || auth.with(|a| key_change_blocked(a.user()));

    let reload = Callback::new(move |_: ()| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::passkeys_list(&client).await {
                Ok(items) => list.set(ListState::Ready(items)),
                Err(e) => list.set(ListState::Failed(api::failure_sentence(&e))),
            }
        });
    });

    // Whether this browser has passkeys is known only in the browser. The list is read again when
    // the sign-in changes (a key replacement can remove passkeys).
    Effect::new(move |_| supported.set(webauthn::webauthn_available()));
    Effect::new(move |_| {
        if auth.with(|a| a.user().map(|u| (u.id, u.key_epoch))).is_some() {
            reload.run(());
        }
    });

    let add = Callback::new(move |_: ()| {
        let Some(file) = current.get_untracked() else { return };
        let label = webauthn::clean_name(&name.get_untracked());
        busy.set(true);
        problem.set(String::new());
        done.set(String::new());
        spawn_local(async move {
            match add_passkey(api_url.get_value(), file, label).await {
                Ok(info) => {
                    done.set(added_sentence(&info.name));
                    name.set(String::new());
                    reload.run(());
                }
                Err(why) => problem.set(why),
            }
            busy.set(false);
        });
    });

    let remove = Callback::new(move |id: String| {
        let Some(file) = current.get_untracked() else { return };
        let Some(account_id) = auth.with_untracked(|a| a.user().map(|u| u.id.to_string())) else { return };
        busy.set(true);
        problem.set(String::new());
        done.set(String::new());
        spawn_local(async move {
            match remove_passkey(api_url.get_value(), file, id, account_id).await {
                Ok(()) => {
                    done.set("Removed the passkey. It no longer signs in to Eustress.".to_string());
                    reload.run(());
                }
                Err(why) => problem.set(why),
            }
            busy.set(false);
        });
    });

    view! {
        <div class="settings-group" id="passkeys">
            <h2 class="group-title">"Passkeys"</h2>
            <p class="info-text">
                "A passkey signs you in to this website on one device with your fingerprint, face or screen lock, and needs no file. "
                "Your identity file stays your master key: Studio and the Player still sign in with it, and you need it to add or remove a passkey."
            </p>

            {move || match list.get() {
                ListState::Loading => view! { <p class="info-text" role="status">"Loading your passkeys..."</p> }.into_any(),
                ListState::Failed(why) => view! { <div class="alert alert-error" role="alert">{why}</div> }.into_any(),
                ListState::Ready(items) if items.is_empty() => view! { <p class="info-text" role="status">"You have no passkeys yet."</p> }.into_any(),
                ListState::Ready(items) => view! {
                    <ul class="passkey-list">
                        {items.into_iter().map(|info| {
                            let id = info.id.clone();
                            let shown_name = if info.name.is_empty() { "Passkey".to_string() } else { info.name.clone() };
                            let (added, used) = (added_line(&info), last_used_line(&info));
                            view! {
                                <li class="passkey-row">
                                    <div class="passkey-info">
                                        <span class="setting-value">{shown_name}</span>
                                        <span class="setting-label">{added}</span>
                                        <span class="setting-label">{used}</span>
                                    </div>
                                    <button
                                        type="button"
                                        class="btn-edit"
                                        disabled=move || !can_remove(passkey_session(), current.with(|c| c.is_some()), busy.get())
                                        on:click=move |_| remove.run(id.clone())
                                    >
                                        "Remove passkey"
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}

            {move || (!problem.get().is_empty()).then(|| view! { <div class="alert alert-error" role="alert">{problem.get()}</div> })}
            {move || (!done.get().is_empty()).then(|| view! { <div class="alert alert-success" role="status">{done.get()}</div> })}

            {move || if passkey_session() {
                view! { <p class="info-text" role="status">{PASSKEY_SESSION_SENTENCE}</p> }.into_any()
            } else {
                view! {
                    <div class="passkey-add">
                        <div class="form-field">
                            <label class="form-label" for="passkey-name">"Name for this passkey (optional)"</label>
                            <input
                                id="passkey-name"
                                type="text"
                                class="form-input"
                                maxlength="64"
                                placeholder="Work laptop"
                                prop:value=move || name.get()
                                on:input=move |e| name.set(event_target_value(&e))
                            />
                        </div>
                        <IdentityFilePicker current=current id="passkeys-file" />
                        {move || (!supported.get()).then(|| view! { <p class="info-text">{webauthn::NO_WEBAUTHN}</p> })}
                        <button
                            type="button"
                            class="btn btn-primary"
                            disabled=move || !can_add(supported.get(), passkey_session(), current.with(|c| c.is_some()), busy.get())
                            on:click=move |_| add.run(())
                        >
                            {move || if busy.get() { "Working..." } else { "Add a passkey" }}
                        </button>
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "11111111-2222-4333-8444-555555555555";

    fn user(auth_method: Option<&str>) -> User {
        let mut v = serde_json::json!({
            "id": ACCOUNT, "username": "ada", "email": null, "avatar_url": null, "discord_id": null,
            "created_at": "2026-01-01T00:00:00Z"
        });
        if let Some(m) = auth_method {
            v["auth_method"] = m.into();
        }
        serde_json::from_value(v).expect("a user parses")
    }

    fn file_for(account: Option<&str>) -> IdentityFile {
        IdentityFile { public_key: "a".repeat(64), private_key: "b".repeat(64), username: "ada".into(), account_id: account.map(str::to_string) }
    }

    #[test]
    fn the_contracts_sentence_is_shown_word_for_word() {
        assert_eq!(
            PASSKEY_SESSION_SENTENCE,
            "You signed in with a passkey. Sign in with your identity file to add or remove passkeys or replace your identity file."
        );
    }

    #[test]
    fn a_passkey_session_cannot_change_keys_and_any_other_session_can_try() {
        assert!(key_change_blocked(Some(&user(Some("passkey")))));
        assert!(!key_change_blocked(Some(&user(Some("key")))));
        assert!(!key_change_blocked(Some(&user(None))), "an answer from before passkeys is an identity-file session");
        assert!(!key_change_blocked(None));
    }

    #[test]
    fn an_older_me_answer_without_auth_method_still_parses() {
        assert_eq!(user(None).auth_method, None);
        assert_eq!(user(Some("passkey")).auth_method.as_deref(), Some("passkey"));
    }

    #[test]
    fn the_buttons_are_on_only_when_the_action_can_happen() {
        // can_add(webauthn, passkey session, file chosen, busy)
        assert!(can_add(true, false, true, false));
        assert!(!can_add(false, false, true, false), "no WebAuthn in the browser");
        assert!(!can_add(true, true, true, false), "a passkey session cannot add");
        assert!(!can_add(true, false, false, false), "no identity file chosen");
        assert!(!can_add(true, false, true, true), "already working");
        // can_remove(passkey session, file chosen, busy)
        assert!(can_remove(false, true, false));
        assert!(!can_remove(true, true, false), "a passkey session cannot remove");
        assert!(!can_remove(false, false, false));
        assert!(!can_remove(false, true, true));
    }

    #[test]
    fn the_account_id_comes_out_of_the_user_handle() {
        let handle = webauthn::b64url_encode(ACCOUNT.as_bytes());
        assert_eq!(account_id_from_handle(&handle).unwrap(), ACCOUNT);
        assert!(account_id_from_handle("a+b/").is_err());
        assert!(account_id_from_handle(&webauthn::b64url_encode(&[0xff, 0xfe, 0xfd])).is_err(), "not text");
    }

    #[test]
    fn a_file_for_another_account_is_refused_before_the_device_is_asked() {
        assert!(file_fits_account(&file_for(Some(ACCOUNT)), ACCOUNT).is_ok());
        assert!(file_fits_account(&file_for(Some(&ACCOUNT.to_uppercase())), ACCOUNT).is_ok(), "case does not matter");
        assert!(file_fits_account(&file_for(None), ACCOUNT).is_ok(), "an older file names no account; the Worker judges its key");
        let wrong = file_fits_account(&file_for(Some("99999999-9999-4999-8999-999999999999")), ACCOUNT).unwrap_err();
        assert!(wrong.starts_with("This file is for a different account"), "{wrong}");
    }

    #[test]
    fn the_list_says_when_a_passkey_was_added_and_last_used() {
        let info: PasskeyInfo = serde_json::from_value(serde_json::json!({
            "id": "AQID", "name": "Laptop", "created_at": 1800000000000u64, "last_used_at": "2026-10-08T12:00:00Z"
        }))
        .unwrap();
        assert_eq!(added_line(&info), "Added Jan 15, 2027, 8:00 AM UTC");
        assert_eq!(last_used_line(&info), "Last used Oct 8, 2026, 12:00 PM UTC");
        let fresh: PasskeyInfo = serde_json::from_value(serde_json::json!({ "id": "AQID", "last_used_at": null })).unwrap();
        assert_eq!(last_used_line(&fresh), "Not used yet");
        assert_eq!(added_line(&fresh), "Added at a time that is not on record");
    }

    #[test]
    fn a_passkey_the_device_made_but_the_account_refused_is_said_to_be_unlinked() {
        let s = refused_after_made("That passkey is already on an account.");
        assert!(s.starts_with("That passkey is already on an account."));
        assert!(s.contains("not linked to your account") && s.contains("delete it"));
    }

    #[test]
    fn the_section_sits_right_after_the_identity_file_group_and_before_the_address_group() {
        let settings = include_str!("settings.rs");
        let security = &settings[settings.find(r#"active_tab.get() == "security""#).expect("a Security tab")..];
        let replace = security.find("<ReplaceKeySection />").expect("the identity file group");
        let passkeys = security.find("<PasskeysSection />").expect("the passkeys group");
        let email = security.find("<EmailSection />").expect("the address group");
        assert!(replace < passkeys && passkeys < email, "Identity file, then Passkeys, then the address");
        assert!(security[replace..passkeys].matches("Section />").count() == 1, "nothing between the two groups");
        assert!(include_str!("mod.rs").contains("pub mod passkeys;"));
    }

    #[test]
    fn both_key_change_surfaces_turn_their_buttons_off_in_a_passkey_session() {
        let mine = include_str!("passkeys.rs");
        let mine = &mine[..mine.find("#[cfg(test)]").unwrap()];
        assert!(mine.contains("disabled=move || !can_add(supported.get(), passkey_session()"), "Add a passkey");
        assert!(mine.contains("disabled=move || !can_remove(passkey_session()"), "Remove passkey");
        let replace = include_str!("replace_key.rs");
        let replace = &replace[..replace.find("#[cfg(test)]").unwrap()];
        let check = replace.find(r#"else { "Check my file" }"#).expect("the check button");
        let replace_button = replace.find(r#"else { "Replace my key" }"#).expect("the replace button");
        for (label, at) in [("Check my file", check), ("Replace my key", replace_button)] {
            let before = &replace[..at];
            let button = &before[before.rfind("<button").unwrap()..];
            assert!(button.contains("passkey_session()"), "the {label} button is off in a passkey session: {button}");
        }
        assert!(replace.contains("PASSKEY_SESSION_SENTENCE"), "and the group says why");
    }

    /// One real exchange with the Worker (infrastructure/cloudflare/api/tests/passkeys.test.mjs, its
    /// software authenticator): the begin answers, and the exact messages the Worker verified.
    const WORKER_ADD_BEGIN: &str = r#"{"challenge":"cvysYD6JJR6k_cTpDEWWitdMW5IgH3esHrNnGKUHlZU","rp":{"id":"eustress.dev","name":"Eustress"},"user":{"id":"MTMxMzEzMTMtMTMxMy00MzEzLTgzMTMtMTMxMzEzMTMxMzEz","name":"owner","display_name":"owner"},"pub_key_cred_params":[-8,-7,-257],"exclude_credentials":[],"timeout_ms":300000,"authenticator_selection":{"resident_key":"required","user_verification":"required"},"attestation":"none","api_origin":"https://api.eustress.dev"}"#;
    const WORKER_REMOVE_BEGIN: &str = r#"{"challenge":"-_Xon2QLB7X30gib7tUiiDISO-nlPjBL_NeIunCIBVY","api_origin":"https://api.eustress.dev","account_id":"13131313-1313-4313-8313-131313131313"}"#;
    const WORKER_CREDENTIAL_ID: &str = "JcwEUs4ZXP0WbWWJT-L4E-bb5BA6Q1mFcSM8hPd31Bw";
    const WORKER_ADD_MESSAGE: &str = "eustress-passkey-add-v1\nhttps://api.eustress.dev\n13131313-1313-4313-8313-131313131313\nJcwEUs4ZXP0WbWWJT-L4E-bb5BA6Q1mFcSM8hPd31Bw\n72fcac603e89251ea4fdc4e90c45968ad74c5b92201f77ac1eb36718a5079595";
    const WORKER_REMOVE_MESSAGE: &str = "eustress-passkey-remove-v1\nhttps://api.eustress.dev\n13131313-1313-4313-8313-131313131313\nJcwEUs4ZXP0WbWWJT-L4E-bb5BA6Q1mFcSM8hPd31Bw\nfbf5e89f640b07b5f7d2089beed5228832123be9e53e304bfcd788ba70880556";

    #[test]
    fn the_site_signs_exactly_the_bytes_the_worker_verifies_for_add() {
        let begin: api::PasskeyAddBegin = serde_json::from_str(WORKER_ADD_BEGIN).expect("the Worker's begin answer parses");
        let options = webauthn::CreationOptions::from_begin(&begin).expect("and the site accepts it");
        assert_eq!(options.challenge.len(), 32);
        let account_id = account_id_from_handle(&begin.user.id).unwrap();
        assert_eq!(account_id, "13131313-1313-4313-8313-131313131313");
        let message = add_message_to_sign(&api::api_origin("https://api.eustress.dev"), &account_id, WORKER_CREDENTIAL_ID, &begin.challenge).unwrap();
        assert_eq!(message, WORKER_ADD_MESSAGE);
        assert_ne!(message, api::passkey_add_message("https://api.eustress.dev", &account_id, WORKER_CREDENTIAL_ID, &begin.challenge), "the base64url text is not what is signed");
    }

    #[test]
    fn the_site_signs_exactly_the_bytes_the_worker_verifies_for_remove() {
        let begin: api::PasskeyRemoveBegin = serde_json::from_str(WORKER_REMOVE_BEGIN).expect("the Worker's remove answer parses");
        let account = begin.account_id.clone().unwrap();
        let message = remove_message_to_sign("https://api.eustress.dev", &account, WORKER_CREDENTIAL_ID, &begin.challenge).unwrap();
        assert_eq!(message, WORKER_REMOVE_MESSAGE);
        // A challenge that is not 32 bytes of base64url signs nothing.
        assert!(remove_message_to_sign("https://api.eustress.dev", &account, WORKER_CREDENTIAL_ID, "abc").is_err());
        assert!(add_message_to_sign("https://api.eustress.dev", &account, WORKER_CREDENTIAL_ID, &webauthn::challenge_hex(&begin.challenge).unwrap()).is_err(), "the hex form is not a challenge");
    }

    #[test]
    fn the_remove_answer_may_or_may_not_name_the_account() {
        let without: api::PasskeyRemoveBegin = serde_json::from_str(r#"{"challenge":"-_Xon2QLB7X30gib7tUiiDISO-nlPjBL_NeIunCIBVY","api_origin":"https://api.eustress.dev"}"#).unwrap();
        assert_eq!(without.account_id, None);
        let source = include_str!("passkeys.rs");
        let source = &source[..source.find("#[cfg(test)]").unwrap()];
        assert!(source.contains("begin.account_id.clone().filter(|a| !a.is_empty()).unwrap_or(account_id)"), "the Worker's account id wins when it sends one");
    }

    fn server_error(status: u16, body: &str) -> ApiError {
        ApiError::Server { status, message: body.to_string() }
    }

    #[test]
    fn the_sentence_for_email_and_api_keys_is_the_one_mckale_chose() {
        assert_eq!(EMAIL_AND_KEYS_SENTENCE, "Sign in with your identity file to change your email or create API keys.");
    }

    #[test]
    fn a_403_identity_file_required_is_shown_in_the_workers_words_and_nothing_else_is() {
        let refusal = server_error(403, r#"{"code":"identity_file_required","error":"Sign in with your identity file to change your email or create API keys."}"#);
        assert_eq!(identity_file_required_words(&refusal).as_deref(), Some(EMAIL_AND_KEYS_SENTENCE));
        let bare = server_error(403, r#"{"code":"identity_file_required"}"#);
        assert!(identity_file_required_words(&bare).unwrap().contains("identity file"));
        assert_eq!(identity_file_required_words(&server_error(403, r#"{"code":"banned","error":"No."}"#)), None);
        assert_eq!(identity_file_required_words(&server_error(400, r#"{"code":"identity_file_required"}"#)), None, "only a 403");
        assert_eq!(identity_file_required_words(&server_error(403, "not json")), None);
        assert_eq!(identity_file_required_words(&ApiError::Unauthorized), None);
    }

    #[test]
    fn the_email_change_and_create_key_controls_are_off_in_a_passkey_session_and_reading_and_revoking_are_not() {
        fn cut(s: &str) -> &str {
            &s[..s.find("#[cfg(test)]").unwrap_or(s.len())]
        }
        let email = cut(include_str!("email_settings.rs"));
        for label in ["\"Change my address\"", "{move || if busy.get() { \"Asking...\" }"] {
            let at = email.find(label).unwrap_or_else(|| panic!("{label} is in the email group"));
            let before = &email[..at];
            let button = &before[before.rfind("<button").unwrap()..];
            assert!(button.contains("passkey_session()"), "{label} is off in a passkey session: {button}");
        }
        assert!(email.contains("EMAIL_AND_KEYS_SENTENCE"), "and the group says why");
        // Cancelling a change and sending the verification link stay on.
        for label in ["\"Cancel the change\"", "\"Send me a link\""] {
            let at = email.find(label).unwrap();
            let before = &email[..at];
            assert!(!before[before.rfind("<button").unwrap()..].contains("passkey_session()"), "{label} stays on");
        }
        assert!(email.contains("identity_file_required_words(&e)"), "a 403 is worded when the change is asked for");

        let projects = cut(include_str!("projects.rs"));
        let keys = &projects[projects.find("pub fn ApiKeysSection").expect("the API keys section")..];
        let create = keys.find("\"Create Key\"").expect("the Create Key button");
        let before = &keys[..create];
        let button = &before[before.rfind("<button").unwrap()..];
        assert!(button.contains("passkey_session()"), "Create Key is off in a passkey session: {button}");
        assert!(keys.contains("EMAIL_AND_KEYS_SENTENCE") && keys.contains("identity_file_required_words(&e)"));
        let revoke = &keys[keys.find("let revoke_key").unwrap()..keys.find("let copy_to_clipboard").unwrap()];
        assert!(!revoke.contains("passkey_session"), "revoking a key stays on");
        // The /ai page shows this same section; no other file creates a key.
        assert!(cut(include_str!("ai.rs")).contains("ApiKeysSection"));
        for other in [include_str!("dashboard.rs"), include_str!("project_sell.rs"), include_str!("settings.rs")] {
            assert!(!other.contains("\"/api/keys\""), "a second place that creates keys needs the same rule");
        }
    }

    #[test]
    fn no_text_in_this_file_uses_a_long_dash() {
        let source = include_str!("passkeys.rs");
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "a long dash in pages/passkeys.rs");
        }
    }
}
