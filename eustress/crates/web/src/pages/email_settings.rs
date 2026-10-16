// =============================================================================
// Eustress Web - Email address (Settings, Security)
// =============================================================================
// Verify the address on file, and change it. The first verification link goes
// only to the address on file. A change is signed with the current identity
// file (a stolen session alone cannot take over the address), waits seven days,
// and every address the account already had is told, with a link that cancels
// it. Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified
// email".
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient};
use crate::components::identity_file_picker::IdentityFilePicker;
use crate::identity_file::{check_against_account, IdentityFile};
use crate::pages::login::sign_challenge_ed25519;
use crate::pages::passkeys::{identity_file_required_words, key_change_blocked, EMAIL_AND_KEYS_SENTENCE};
use crate::state::AppState;

/// The "Email address" group of the Security tab.
#[component]
pub fn EmailSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;

    let view_state = RwSignal::new(Option::<api::EmailView>::None);
    let note = RwSignal::new(String::new());
    let problem = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let new_address = RwSignal::new(String::new());
    let current = RwSignal::new(Option::<IdentityFile>::None);
    let changing = RwSignal::new(false);
    let passkey_session = move || auth.with(|a| key_change_blocked(a.user()));

    let load = Callback::new(move |_: ()| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            if let Ok(v) = api::email_status(&client).await {
                view_state.set(Some(v));
            }
        });
    });
    Effect::new(move |_| {
        if auth.get().is_authenticated() {
            load.run(());
        }
    });

    let send_link = Callback::new(move |_: ()| {
        busy.set(true);
        problem.set(String::new());
        note.set(String::new());
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_start(&client).await {
                Ok(r) if r.already_verified => note.set("This address is already verified.".to_string()),
                Ok(r) => note.set(format!(
                    "A link was sent to {}. Open it and press the button on the page it opens.",
                    r.sent_to.unwrap_or_else(|| "your address".to_string())
                )),
                Err(e) => problem.set(api::failure_sentence(&e)),
            }
            busy.set(false);
            load.run(());
        });
    });

    let cancel = Callback::new(move |_: ()| {
        busy.set(true);
        problem.set(String::new());
        note.set(String::new());
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_cancel_session(&client).await {
                Ok(()) => note.set("The change was cancelled.".to_string()),
                Err(e) => problem.set(api::failure_sentence(&e)),
            }
            busy.set(false);
            load.run(());
        });
    });

    let state_for_apply = app_state.clone();
    let apply = Callback::new(move |_: ()| {
        busy.set(true);
        problem.set(String::new());
        note.set(String::new());
        let state = state_for_apply.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match api::email_apply(&client).await {
                Ok(()) => {
                    note.set("Your address was changed. The new address has to be verified again.".to_string());
                    if let Ok(user) = api::get_current_user(&client).await {
                        state.login(user);
                    }
                }
                Err(e) => problem.set(api::failure_sentence(&e)),
            }
            busy.set(false);
            load.run(());
        });
    });

    // Ask for the change: a challenge, the current key's signature over the
    // address, and the request.
    let ask = Callback::new(move |_: ()| {
        let Some(file) = current.get_untracked() else {
            problem.set("Choose your current identity file first.".to_string());
            return;
        };
        let Some(address) = api::clean_address(&new_address.get_untracked()) else {
            problem.set("That is not an email address.".to_string());
            return;
        };
        busy.set(true);
        problem.set(String::new());
        note.set(String::new());
        spawn_local(async move {
            let url = api_url.get_value();
            let client = ApiClient::new(&url);
            let begun = match api::email_change_begin(&client).await {
                Ok(b) => b,
                Err(e) => {
                    problem.set(identity_file_required_words(&e).unwrap_or_else(|| api::failure_sentence(&e)));
                    busy.set(false);
                    return;
                }
            };
            if let Err(why) = check_against_account(&file, &begun.public_key, &begun.account_id, &api::api_origin(&url)) {
                problem.set(why);
            } else if begun.origin != api::api_origin(&url) {
                problem.set("The address of Eustress did not match this page. Nothing was changed.".to_string());
            } else {
                let message = api::email_change_message(&begun.origin, &begun.account_id, &file.public_key, &address, &begun.challenge);
                match sign_challenge_ed25519(&file.private_key, &message) {
                    Ok(signature) => match api::email_change(&client, &begun.challenge, &address, &signature).await {
                        Ok(pending) => {
                            note.set(format!(
                                "Asked. The change takes effect on {}, and you confirm it here then. The addresses your account already has were told, with a link to cancel it.",
                                api::iso_to_words(&pending.effective_at)
                            ));
                            new_address.set(String::new());
                            current.set(None);
                            changing.set(false);
                        }
                        Err(e) => problem.set(identity_file_required_words(&e).unwrap_or_else(|| api::failure_sentence(&e))),
                    },
                    Err(_) => problem.set("Could not sign. Check the identity file and try again.".to_string()),
                }
            }
            busy.set(false);
            load.run(());
        });
    });

    view! {
        <div class="settings-group" id="email-address">
            <h2 class="group-title">"Email address"</h2>

            {move || (!note.get().is_empty()).then(|| view! { <div class="alert alert-success" role="status">{note.get()}</div> })}
            {move || (!problem.get().is_empty()).then(|| view! { <div class="alert alert-error" role="alert">{problem.get()}</div> })}

            {move || passkey_session().then(|| view! { <p class="info-text" role="status">{EMAIL_AND_KEYS_SENTENCE}</p> })}

            {move || match view_state.get() {
                None => view! { <p class="info-text">"Loading..."</p> }.into_any(),
                Some(v) => {
                    let on_file = v.on_file.clone();
                    let verified = v.verified;
                    let aged = v.aged;
                    let aged_at = v.aged_at.clone();
                    let pending = v.pending.clone();
                    view! {
                        <div class="email-status">
                            {match on_file.clone() {
                                Some(address) => view! {
                                    <p>
                                        <strong>{address}</strong>
                                        {if verified { " (verified)" } else { " (not verified)" }}
                                    </p>
                                }.into_any(),
                                None => view! { <p>"No email address is on your account."</p> }.into_any(),
                            }}
                            {(verified && !aged).then(|| view! {
                                <p class="info-text">
                                    {format!("A verified address counts for account recovery once it is thirty days old{}.",
                                        aged_at.clone().map(|t| format!(", from {}", api::iso_to_words(&t))).unwrap_or_default())}
                                </p>
                            })}
                            {(on_file.is_some() && !verified).then(|| view! {
                                <p class="info-text">
                                    "Verify it so Eustress can reach you if your account is ever at risk. The link goes to this address only."
                                </p>
                                <button class="btn btn-secondary" disabled=move || busy.get() on:click=move |_| send_link.run(())>
                                    "Send me a link"
                                </button>
                            })}
                        </div>

                        {pending.map(|p| {
                            let ready = p.ready;
                            view! {
                                <div class="alert alert-warning email-pending" role="status">
                                    <h3>"A change of address is waiting"</h3>
                                    <p>{format!("To {}, taking effect on {}.", p.address, api::iso_to_words(&p.effective_at))}</p>
                                    <p class="info-text">"If you did not ask for this, cancel it now and replace your identity key."</p>
                                    <div class="email-pending-actions">
                                        <button class="btn btn-secondary" disabled=move || busy.get() on:click=move |_| cancel.run(())>"Cancel the change"</button>
                                        {ready.then(|| view! {
                                            <button class="btn btn-primary" disabled=move || busy.get() on:click=move |_| apply.run(())>"Make it my address"</button>
                                        })}
                                    </div>
                                </div>
                            }
                        })}
                    }.into_any()
                }
            }}

            <Show when=move || view_state.get().map(|v| v.pending.is_none()).unwrap_or(false)>
                <Show when=move || !changing.get()>
                    <button class="btn btn-secondary" disabled=move || passkey_session() on:click=move |_| changing.set(true)>"Change my address"</button>
                </Show>
                <Show when=move || changing.get()>
                    <div class="replace-key-step">
                        <p class="info-text">
                            "A change needs your current identity file, waits seven days, and is announced to the addresses you have now, with a link to cancel it."
                        </p>
                        <div class="form-field">
                            <label class="form-label">"New address"</label>
                            <input type="email" class="form-input" prop:value=move || new_address.get()
                                on:input=move |ev| new_address.set(event_target_value(&ev)) />
                        </div>
                        <IdentityFilePicker current=current id="email-change-file" on_pick=Callback::new(move |_: ()| problem.set(String::new())) />
                        <button class="btn btn-primary" disabled=move || busy.get() || passkey_session() on:click=move |_| ask.run(())>
                            {move || if busy.get() { "Asking..." } else { "Ask for the change" }}
                        </button>
                    </div>
                </Show>
            </Show>
        </div>
    }
}
