// =============================================================================
// Eustress Web - Sponsor the engine (/sponsor)
// =============================================================================
// Money given to build the engine, once or every month. The whole amount, after the payment fee, goes to Eustress's side, where the
// engine's costs are paid (the Worker's src/sponsor.mjs: POST /api/sponsor/checkout, GET /api/sponsors). A sponsor receives nothing
// that can be spent, traded or cashed out, and no share of the company. A signed-in sponsor can ask for their username on the
// sponsor list below. Same look as the currency pages (components/currency_ui.rs).
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::ApiClient;
use crate::components::currency_ui::{begin_checkout, release_on_return, Banner, CurrencyShell, Message};
use crate::state::{AppState, AuthState};

/// A sponsor tier: the Worker's id, its name, its price in words, and what it is for.
pub struct SponsorTier {
    pub id: &'static str,
    pub name: &'static str,
    pub amount_usd: &'static str,
    pub description: &'static str,
}

/// The four tiers, in the order they are offered. The ids and amounts are the Worker's (SPONSOR_TIERS, src/sponsor.mjs).
pub const TIERS: [SponsorTier; 4] = [
    SponsorTier { id: "supporter", name: "Supporter", amount_usd: "$10", description: "Keeps the lights on for a builder" },
    SponsorTier { id: "builder", name: "Builder", amount_usd: "$50", description: "A week of build servers" },
    SponsorTier { id: "champion", name: "Champion", amount_usd: "$250", description: "A month of hosting for the platform" },
    SponsorTier { id: "founding", name: "Founding Sponsor", amount_usd: "$1,000", description: "Backs a whole feature from design to release" },
];

/// What a sponsorship is and is not, in the page's words.
pub const RULES: [&str; 4] = [
    "The whole amount, after the payment fee, pays for building and running the engine.",
    "A sponsorship is a gift. It is not an investment: it buys no share of the company and no claim on revenue or profit.",
    "It grants no Tickets, Delta or Bliss. Bliss is earned by contributing and is never sold.",
    "To pay contributors directly instead, fund the Treasury on the Bliss page.",
];

/// The smallest and the largest custom amount the Worker takes, in dollars.
pub const MIN_CUSTOM_USD: f64 = 5.0;
pub const MAX_CUSTOM_USD: f64 = 100_000.0;

/// The body of a sponsorship checkout: the chosen tier or a custom amount, whether it repeats monthly, and whether the sponsor asked to
/// be listed. `None` when neither a tier nor an amount the Worker takes is chosen.
pub fn sponsor_body(tier: Option<usize>, custom: &str, monthly: bool, listed: bool) -> Option<serde_json::Value> {
    let tier_id = tier.and_then(|i| TIERS.get(i)).map(|t| t.id);
    let custom_amount = custom.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= MIN_CUSTOM_USD && *v <= MAX_CUSTOM_USD);
    if tier_id.is_none() && custom_amount.is_none() {
        return None;
    }
    Some(serde_json::json!({
        "tier": tier_id,
        "custom_amount": if tier_id.is_some() { None } else { custom_amount },
        "mode": if monthly { "recurring" } else { "one_time" },
        "listed": listed,
    }))
}

/// What the address says about the payment that just ended: `?sponsored=true` or `?sponsored=false`.
pub fn sponsor_banner(query: &str) -> Option<Message> {
    match crate::utils::query_value(query, "sponsored").as_deref() {
        Some("true") => Some(Message::Info("Thank you. Your sponsorship went through; all of it goes to building the engine.".to_string())),
        Some("false") => Some(Message::Info("The payment was cancelled. Nothing was charged.".to_string())),
        _ => None,
    }
}

/// A listed sponsor as the Worker sends it: the account's username, the month they first sponsored (YYYY-MM), and whether monthly.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Sponsor {
    pub username: String,
    #[serde(default)]
    pub since: String,
    #[serde(default)]
    pub monthly: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct SponsorsResponse {
    #[serde(default)]
    sponsors: Vec<Sponsor>,
}

/// The words under a sponsor's name.
pub fn sponsor_line(s: &Sponsor) -> String {
    match (s.monthly, s.since.is_empty()) {
        (true, false) => format!("Monthly sponsor since {}", s.since),
        (false, false) => format!("Sponsor since {}", s.since),
        (true, true) => "Monthly sponsor".to_string(),
        (false, true) => "Sponsor".to_string(),
    }
}

#[component]
pub fn SponsorPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());

    let selected_tier = RwSignal::new(None::<usize>);
    let custom_amount = RwSignal::new(String::new());
    let monthly = RwSignal::new(false);
    let listed = RwSignal::new(false);
    let busy = RwSignal::new(false);
    release_on_return(busy);
    let notice = RwSignal::new(sponsor_banner(&crate::utils::current_query()));
    let message = RwSignal::new(None::<Message>);
    let sponsors = RwSignal::new(None::<Vec<Sponsor>>);

    // In the browser only: the sponsors who asked to be listed.
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let list = client.get::<SponsorsResponse>("/api/sponsors").await.map(|r| r.sponsors).unwrap_or_default();
            sponsors.set(Some(list));
        });
    });

    let signed_in = Signal::derive(move || matches!(auth.get(), AuthState::Authenticated(_)));
    // Only an account can be listed: the list shows its username.
    let body = move || sponsor_body(selected_tier.get(), &custom_amount.get(), monthly.get(), listed.get() && signed_in.get());

    view! {
        <CurrencyShell title="Sponsor the engine" lead="Eustress is built by a small team and paid for by the people who use it. A sponsorship pays for the engine itself: build servers, hosting and the work of making it.">
            <Banner message=notice />

            <section class="g-section" aria-labelledby="sp-give">
                <div class="g-head"><h2 id="sp-give">"Choose a sponsorship"</h2><span class="g-rule"></span></div>
                <div class="cur-toggle" role="group" aria-label="How often">
                    <button type="button" class="cur-toggle-btn" class:on=move || !monthly.get() aria-pressed=move || (!monthly.get()).to_string() on:click=move |_| monthly.set(false)>"One-time"</button>
                    <button type="button" class="cur-toggle-btn" class:on=move || monthly.get() aria-pressed=move || monthly.get().to_string() on:click=move |_| monthly.set(true)>"Monthly"</button>
                </div>
                <div class="pk-grid" role="radiogroup" aria-label="Sponsor tiers">
                    {TIERS.iter().enumerate().map(|(i, tier)| {
                        let on = move || selected_tier.get() == Some(i);
                        view! {
                            <label class="pk-card pk-tier" class:on=on>
                                <input type="radio" class="g-sr" name="sponsor-tier" prop:checked=on on:change=move |_| { selected_tier.set(Some(i)); custom_amount.set(String::new()); } />
                                <span class="pk-name">{tier.name}</span>
                                <span class="pk-price">{tier.amount_usd}<small>{move || if monthly.get() { " /mo" } else { "" }}</small></span>
                                <span class="pk-desc">{tier.description}</span>
                            </label>
                        }
                    }).collect_view()}
                </div>
                <div class="g-glass cur-custom">
                    <label for="sponsor-custom">"Or enter a custom amount"</label>
                    <div class="cur-custom-input">
                        <span aria-hidden="true">"$"</span>
                        <input id="sponsor-custom" type="number" min="5" step="1" inputmode="decimal" placeholder="0"
                            prop:value=move || custom_amount.get()
                            on:input=move |e| { custom_amount.set(event_target_value(&e)); selected_tier.set(None); } />
                        <span aria-hidden="true">"USD"</span>
                    </div>
                </div>
                <div class="pk-bar">
                    {move || if signed_in.get() {
                        view! {
                            <label class="g-hint">
                                <input type="checkbox" prop:checked=move || listed.get() on:change=move |e| listed.set(event_target_checked(&e)) />
                                " Show my username on the sponsor list"
                            </label>
                        }.into_any()
                    } else {
                        view! { <p class="g-hint">"You can sponsor without signing in. Sign in first if you want your username on the sponsor list."</p> }.into_any()
                    }}
                    <button type="button" class="g-btn g-btn-primary g-btn-lg" disabled=move || body().is_none() || busy.get()
                        on:click=move |_| {
                            if let Some(b) = body() {
                                begin_checkout(api_url.get_value(), "/api/sponsor/checkout", b, busy, message);
                            }
                        }>
                        {move || if busy.get() { "Opening Stripe...".to_string() } else if monthly.get() { "Sponsor monthly".to_string() } else { "Sponsor the engine".to_string() }}
                    </button>
                    <Banner message=message />
                    <p class="g-hint pk-note">"Secure payment via Stripe. You pay on Stripe's own page; Eustress never sees your card."</p>
                </div>
            </section>

            <section class="g-section" aria-labelledby="sp-where">
                <div class="g-head"><h2 id="sp-where">"Where the money goes"</h2><span class="g-rule"></span></div>
                <div class="g-glass dl-table-wrap">
                    <ul class="dl-points dl-rules">{RULES.iter().map(|r| view! { <li>{*r}</li> }).collect_view()}</ul>
                </div>
                <p class="g-hint">"Every sponsorship is counted on the "<a href="/economics">"Economics"</a>" page. To pay contributors directly, "<a href="/bliss">"fund the Treasury"</a>"."</p>
            </section>

            <section class="g-section" aria-labelledby="sp-list">
                <div class="g-head"><h2 id="sp-list">"Sponsors"</h2><span class="g-rule"></span></div>
                {move || match sponsors.get() {
                    None => view! { <p class="g-hint">"Loading the sponsor list..."</p> }.into_any(),
                    Some(list) if list.is_empty() => view! { <div class="g-glass g-none"><b>"No sponsors are listed yet"</b><p>"Be the first."</p></div> }.into_any(),
                    Some(list) => view! {
                        <ul class="dl-points">
                            {list.into_iter().map(|s| {
                                let line = sponsor_line(&s);
                                let href = format!("/profile/{}", s.username);
                                view! { <li><a href=href>{s.username.clone()}</a>" \u{b7} "{line}</li> }
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                }}
            </section>
        </CurrencyShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_needs_a_tier_or_a_custom_amount_the_worker_takes() {
        assert_eq!(sponsor_body(None, "", false, false), None);
        assert_eq!(sponsor_body(None, "4.99", false, false), None);
        assert_eq!(sponsor_body(None, "100000.01", false, false), None);
        assert_eq!(sponsor_body(Some(9), "", false, false), None, "no such tier");
        let tier = sponsor_body(Some(1), "75", true, true).unwrap();
        assert_eq!((tier["tier"].as_str(), tier["mode"].as_str(), tier["listed"].as_bool()), (Some("builder"), Some("recurring"), Some(true)));
        assert!(tier["custom_amount"].is_null(), "a tier wins over a custom amount");
        let custom = sponsor_body(None, " 75 ", false, false).unwrap();
        assert!(custom["tier"].is_null());
        assert_eq!((custom["custom_amount"].as_f64(), custom["mode"].as_str()), (Some(75.0), Some("one_time")));
    }

    #[test]
    fn the_tiers_keep_the_workers_ids_in_order() {
        assert_eq!(TIERS.map(|t| t.id), ["supporter", "builder", "champion", "founding"]);
    }

    #[test]
    fn the_page_says_a_sponsorship_is_a_gift_and_not_an_investment() {
        assert!(RULES[1].contains("not an investment") && RULES[1].contains("no share of the company"), "{}", RULES[1]);
        assert!(RULES[2].contains("never sold"));
    }

    #[test]
    fn the_address_says_whether_the_sponsorship_went_through() {
        assert!(matches!(sponsor_banner("?sponsored=true"), Some(Message::Info(t)) if t.contains("went through")));
        assert!(matches!(sponsor_banner("?sponsored=false"), Some(Message::Info(t)) if t.contains("cancelled")));
        assert_eq!(sponsor_banner("?sponsored=maybe"), None);
        assert_eq!(sponsor_banner(""), None);
    }

    #[test]
    fn a_sponsor_line_says_monthly_and_since_when_known() {
        let s = |monthly: bool, since: &str| Sponsor { username: "ada".to_string(), since: since.to_string(), monthly };
        assert_eq!(sponsor_line(&s(true, "2026-11")), "Monthly sponsor since 2026-11");
        assert_eq!(sponsor_line(&s(false, "")), "Sponsor");
    }
}
