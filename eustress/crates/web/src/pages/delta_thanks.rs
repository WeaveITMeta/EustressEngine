// =============================================================================
// Eustress Web - Delta Purchase Return Page
// =============================================================================
// Stripe returns buyers here after a Delta pack purchase (/delta/thanks). The Delta is added when Stripe tells the Worker the payment
// went through, a moment after the buyer lands here, so a signed-in visitor sees their balance read again a few times, here and in
// the top bar, until it settles.
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{get_delta_balance, ApiClient};
use crate::components::currency_ui::{Currency, CurrencyShell, Figures, BALANCE_REFRESHES, BALANCE_REFRESH_EVERY_MS};
use crate::state::{AppState, AuthState, DeltaState};
use crate::utils::format_count;

/// Shown after a Delta pack purchase.
#[component]
pub fn DeltaThanksPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let balance = app_state.delta;
    let api_url = StoredValue::new(app_state.api_url.clone());
    let started = StoredValue::new(false);

    // In the browser only, once the account is known to be signed in: read the balance now and again every few seconds, and put
    // each reading where the top bar looks for it too.
    Effect::new(move |_| {
        if !matches!(auth.get(), AuthState::Authenticated(_)) || started.get_value() {
            return;
        }
        started.set_value(true);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            for round in 0..BALANCE_REFRESHES {
                if let Ok(Some(b)) = get_delta_balance(&client).await {
                    balance.set(DeltaState::Balance(b));
                }
                if round + 1 < BALANCE_REFRESHES {
                    TimeoutFuture::new(BALANCE_REFRESH_EVERY_MS).await;
                }
            }
        });
    });

    view! {
        <CurrencyShell current=Currency::Delta title="Thank you" lead="Your Delta is on its way.">
            {move || match balance.get() {
                DeltaState::Balance(b) => view! { <Figures items=vec![("Your balance", format!("{} Delta", format_count(b.whole())))] /> }.into_any(),
                _ => ().into_any(),
            }}
            <p class="dl-purpose dl-note">"Stripe tells us when the payment clears, and the Delta is added then. This page reads your balance again every few seconds for about a minute."</p>
            <div class="cur-actions">
                <a href="/delta" class="g-btn g-btn-primary">"Back to Delta"</a>
                <a href="/dashboard" class="g-btn">"Go to my dashboard"</a>
                <a href="/premium" class="g-btn">"See plans"</a>
            </div>
            <p class="g-hint dl-note">"If the balance has not changed after a few minutes, tell us on the "<a href="/support">"support page"</a>"."</p>
        </CurrencyShell>
    }
}
