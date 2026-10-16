// =============================================================================
// Eustress Web - Tickets (/tickets)
// =============================================================================
// Buy Tickets for the marketplace, game passes and simulation features. The packs, their prices and their bonuses are the Worker's
// (GET /api/tickets/packages); a buy opens Stripe Checkout (POST /api/tickets/checkout). Half of each sale, after the payment fee,
// funds the Bliss contributor treasury. Same look as the Bliss and Delta pages (components/currency_ui.rs).
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::ApiClient;
use crate::components::currency_ui::{
    access_of, begin_checkout, buy_label, paid_return, release_on_return, return_banner, state_of_packs, Banner, Currency, CurrencyShell, Figures, Message, Pack, PackSection, Packs, BALANCE_REFRESHES,
    BALANCE_REFRESH_EVERY_MS,
};
use crate::state::{AppState, AuthState};
use crate::utils::format_count;

/// Where the money goes, in the page's words. The Worker takes the payment fee first and splits the rest (treasury.mjs splitSale).
pub const RULES: [&str; 3] = [
    "Half of each sale, after the payment fee, funds the Bliss contributor treasury.",
    "Tickets are for spending in Eustress: marketplace items, game passes and simulation features.",
    "Tickets do not convert to Bliss or Delta.",
];

/// A pack as the Worker sends it.
#[derive(Debug, Clone, Deserialize)]
struct TicketPackage {
    id: String,
    name: String,
    usd: f64,
    #[serde(default)]
    bonus: u64,
    total: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct PackagesResponse {
    packages: Vec<TicketPackage>,
}

#[derive(Debug, Clone, Deserialize)]
struct BalanceResponse {
    tickets: u64,
}

/// The packs in the shared card form: the buyer ends up with the total, bonus included.
fn packs_of(response: PackagesResponse) -> Vec<Pack> {
    response.packages.into_iter().map(|p| Pack { id: p.id, name: p.name, amount: p.total, bonus: p.bonus, usd: p.usd }).collect()
}

/// What the Packs section says when there are none on sale.
fn none_on_sale() -> impl IntoView {
    view! { <div class="g-glass g-none"><b>"No packs are on sale right now"</b><p>"Check back soon."</p></div> }
}

#[component]
pub fn TicketsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());

    let packs = RwSignal::new(Packs::Loading);
    let selected = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    release_on_return(busy);
    // What the address says about the payment that just ended (at the top), and what a buy that could not start says (by the button).
    let notice = RwSignal::new(return_banner(&crate::utils::current_query()));
    let message = RwSignal::new(None::<Message>);
    let balance = RwSignal::new(None::<u64>);

    // In the browser only: the packs, and the account's balance once it is known to be signed in.
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            packs.set(state_of_packs(client.get::<PackagesResponse>("/api/tickets/packages").await.map(packs_of)));
        });
    });
    // The balance is read once; after a purchase that just went through it is read again for about a minute, until the credit lands.
    let just_bought = paid_return(&crate::utils::current_query());
    let reading = StoredValue::new(false);
    Effect::new(move |_| {
        if !matches!(auth.get(), AuthState::Authenticated(_)) || reading.get_value() {
            return;
        }
        reading.set_value(true);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let reads = if just_bought { BALANCE_REFRESHES } else { 1 };
            for round in 0..reads {
                if let Ok(b) = client.get::<BalanceResponse>("/api/tickets/balance").await {
                    balance.set(Some(b.tickets));
                }
                if round + 1 < reads {
                    TimeoutFuture::new(BALANCE_REFRESH_EVERY_MS).await;
                }
            }
        });
    });

    let access = Signal::derive(move || access_of(&auth.get()));
    let chosen = move || -> Option<Pack> {
        let id = selected.get()?;
        match packs.get() {
            Packs::Open(list) => list.into_iter().find(|p| p.id == id),
            _ => None,
        }
    };
    let label = Signal::derive(move || chosen().map(|p| buy_label(&p, "Tickets")));
    let on_buy = Callback::new(move |_| {
        if let Some(p) = chosen() {
            begin_checkout(api_url.get_value(), "/api/tickets/checkout", serde_json::json!({ "package": p.id }), busy, message);
        }
    });

    view! {
        <CurrencyShell current=Currency::Tickets title="Tickets" lead="Spend Tickets in the marketplace, on game passes and on simulation features.">
            <Banner message=notice />
            {move || match auth.get() {
                AuthState::Authenticated(user) => {
                    let shown = balance.get().unwrap_or(user.ticket_balance);
                    view! { <Figures items=vec![("Your balance", format!("{} Tickets", format_count(shown)))] /> }.into_any()
                }
                _ => ().into_any(),
            }}

            <section class="g-section" aria-labelledby="tk-packs">
                <div class="g-head"><h2 id="tk-packs">"Packs"</h2><span class="g-rule"></span></div>
                {move || view! {
                    <PackSection packs=packs.get() unit="Tickets" placeholders=5 closed=none_on_sale
                        selected=selected label=label busy=busy access=access message=message on_buy=on_buy />
                }}
            </section>

            <section class="g-section" aria-labelledby="tk-where">
                <div class="g-head"><h2 id="tk-where">"Where the money goes"</h2><span class="g-rule"></span></div>
                <div class="g-glass dl-table-wrap">
                    <ul class="dl-points dl-rules">{RULES.iter().map(|r| view! { <li>{*r}</li> }).collect_view()}</ul>
                </div>
            </section>
        </CurrencyShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_is_shown_as_the_total_the_buyer_ends_up_with_and_the_bonus_inside_it() {
        let r: PackagesResponse = serde_json::from_str(
            r#"{"packages":[{"id":"starter","name":"Starter","usd":4.99,"base":400,"bonus":0,"total":400},{"id":"standard","name":"Standard","usd":9.99,"base":800,"bonus":80,"total":880}]}"#,
        )
        .unwrap();
        let packs = packs_of(r);
        assert_eq!(packs[0], Pack { id: "starter".to_string(), name: "Starter".to_string(), amount: 400, bonus: 0, usd: 4.99 });
        assert_eq!((packs[1].amount, packs[1].bonus), (880, 80));
    }

    #[test]
    fn the_page_says_the_treasury_gets_half_after_the_payment_fee_and_that_tickets_do_not_convert() {
        assert!(RULES[0].contains("Half of each sale") && RULES[0].contains("after the payment fee"), "{}", RULES[0]);
        assert!(!RULES[0].contains("Half of what Tickets bring in"));
        assert!(RULES[2].contains("do not convert to Bliss or Delta"));
    }

    #[test]
    fn a_list_with_no_packs_is_closed_and_the_page_says_nothing_is_on_sale() {
        use leptos::tachys::view::RenderHtml;
        let r: PackagesResponse = serde_json::from_str(r#"{"packages":[]}"#).unwrap();
        assert_eq!(state_of_packs(Ok(packs_of(r))), Packs::Closed);
        let owner = Owner::new();
        let html = owner.with(|| none_on_sale().to_html());
        assert!(html.contains("No packs are on sale right now") && !html.contains('$'), "{html}");
    }

    #[test]
    fn a_pack_with_no_bonus_field_has_no_bonus() {
        let r: PackagesResponse = serde_json::from_str(r#"{"packages":[{"id":"a","name":"A","usd":1.0,"total":10}]}"#).unwrap();
        assert_eq!(packs_of(r)[0].bonus, 0);
    }
}
