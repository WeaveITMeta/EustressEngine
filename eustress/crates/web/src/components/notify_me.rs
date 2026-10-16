// =============================================================================
// Eustress Web - Get notified: one email when a product's first release is out
// =============================================================================
// The button under "first release coming soon". It uses the account's own verified email (api/release_notify.rs): signed out, the way
// in is to sign in; with no verified email, the way in is Settings; otherwise it is one press to ask and one to take it back. The
// line under the button always says what will happen, and what it says is only what the Worker does: one email, once.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::release_notify::{self, Notify, Product};
use crate::api::ApiClient;
use crate::state::{AppState, AuthState};

#[component]
pub fn NotifyMe(product: Product) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());
    let state = RwSignal::new(Notify::Checking);

    // Follows the account: signed out, checking, then what the Worker says. An Effect, so it runs only in the browser.
    Effect::new(move |_| match auth.get() {
        AuthState::Authenticated(_) => {
            state.set(Notify::Checking);
            spawn_local(async move {
                let client = ApiClient::new(&api_url.get_value());
                state.set(release_notify::state_of_status(&release_notify::status(&client, product).await));
            });
        }
        AuthState::Unauthenticated => state.set(Notify::SignedOut),
        _ => state.set(Notify::Checking),
    });

    let change = move |wanted: bool| {
        if state.get_untracked() == Notify::Saving {
            return;
        }
        state.set(Notify::Saving);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let answer = if wanted { release_notify::subscribe(&client, product).await } else { release_notify::unsubscribe(&client, product).await };
            state.set(release_notify::state_of_change(wanted, &answer));
        });
    };

    let name = product.name();
    view! {
        <div class="dl-notify" aria-live="polite">
            {move || match state.get() {
                Notify::Checking => view! { <span class="g-skel w40 btn"></span> }.into_any(),
                Notify::SignedOut => view! {
                    <a class="g-btn g-btn-primary" href="/login">"Sign in to get notified"</a>
                    <p class="g-hint">"We write to the verified email on your account, once."</p>
                }.into_any(),
                Notify::NeedsVerifiedEmail => view! {
                    <a class="g-btn g-btn-primary" href="/settings">"Verify your email"</a>
                    <p class="g-hint">"We only write to a verified email. Verify the one on your account, then come back to turn this on."</p>
                }.into_any(),
                Notify::Off => view! {
                    <button type="button" class="g-btn g-btn-primary" on:click=move |_| change(true)>"Get notified"</button>
                    <p class="g-hint">{format!("One email to your verified address when {name} is released. Nothing else.")}</p>
                }.into_any(),
                Notify::On => view! {
                    <p class="dl-on"><span class="g-dot" aria-hidden="true"></span>{format!("You will get one email when {name} is released.")}</p>
                    <button type="button" class="g-btn" on:click=move |_| change(false)>"Turn off"</button>
                }.into_any(),
                Notify::Saving => view! { <button type="button" class="g-btn g-btn-primary" disabled=true>"Saving..."</button> }.into_any(),
                Notify::Unavailable => view! { <p class="g-hint">"Email notices are not open yet. The Gallery shows new Spaces the moment they are approved."</p> }.into_any(),
                Notify::Failed(why) => view! {
                    <p class="dl-error" role="alert">{why}</p>
                    <button type="button" class="g-btn" on:click=move |_| change(true)>"Try again"</button>
                }.into_any(),
            }}
        </div>
    }
}
