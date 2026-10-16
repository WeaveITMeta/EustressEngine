// =============================================================================
// Eustress Web - A pending change of email address
// =============================================================================
// Shown across the signed-in site while a change of the account's email address
// is waiting to take effect, because an account with no address on file has
// nobody to warn: the owner sees it here. It offers the one thing that stops it.
// Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified email".
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient};
use crate::state::{AppState, AuthState};

/// A banner for a pending change of address.
#[component]
pub fn EmailBanner() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let hidden = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let state_for_cancel = app_state.clone();

    let pending = move || match auth.get() {
        AuthState::Authenticated(user) if !hidden.get() => user.email_change_pending.clone(),
        _ => None,
    };

    let cancel = Callback::new(move |_: ()| {
        busy.set(true);
        let state = state_for_cancel.clone();
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            if api::email_cancel_session(&client).await.is_ok() {
                hidden.set(true);
                if let Ok(user) = api::get_current_user(&client).await {
                    state.login(user);
                }
            }
            busy.set(false);
        });
    });

    view! {
        {move || pending().map(|p| view! {
            <div class="email-banner" role="status">
                <span>{format!("A change of your email address to {} is waiting, taking effect on {}.", p.address, api::iso_to_words(&p.effective_at))}</span>
                <a href="/settings" class="btn btn-secondary">"See it in Settings"</a>
                <button class="btn btn-secondary" disabled=move || busy.get() on:click=move |_| cancel.run(())>"This was not me: cancel it"</button>
            </div>
        })}
    }
}
