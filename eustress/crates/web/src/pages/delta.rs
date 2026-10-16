// =============================================================================
// Eustress Web - Delta (/delta)
// =============================================================================
// What Delta is, how an account gets it, when it expires, what it is not, and the packs for sale. The numbers are the Worker's
// (infrastructure/cloudflare/api/src/delta.mjs): the trial, the plan grants, the 12-month life of a credit. The packs, their
// sizes and their prices are Stripe's, read through GET /api/delta/packs; a buy opens Stripe Checkout (POST /api/delta/checkout)
// and Stripe returns the buyer to /delta/thanks (or back here when the payment is cancelled). Where the packs are not on sale
// (a deployment without them, or Stripe not set up), the page says buying is not open: it never shows a price it cannot sell at.
// The account's balance is the one the top bar already holds (AppState::delta).
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::ApiClient;
use crate::components::currency_ui::{access_of, begin_checkout, buy_label, delta_pack_name, release_on_return, return_banner, state_of_packs, Banner, Currency, CurrencyShell, Figures, Message, Pack, PackSection, Packs};
use crate::state::{AppState, DeltaState};
use crate::utils::format_count;

/// Delta granted once to an account that passes identity verification.
pub const TRIAL: u64 = 1_000;
/// How long a trial or a pack lasts, in months.
pub const LIFE_MONTHS: u32 = 12;

/// The monthly Delta each plan grants, by plan name: (plan, monthly Delta, a line about it).
pub const PLAN_GRANTS: [(&str, u64, &str); 3] = [
    ("Creator", 1_000, "monthly"),
    ("Professional", 16_000, "monthly; 192,000 with yearly billing"),
    ("Bundle", 900, "monthly, with 400 Tickets"),
];

/// How it works, in the page's words. The numbers are the Worker's (delta.mjs): the 12-month life of a credit, and a plan's grant
/// lasting up to 31 days past the end of its month (PLAN_ROLLOVER_MAX_MS).
pub fn rules() -> [String; 5] {
    [
        format!("Every credit expires {LIFE_MONTHS} months after you receive it. A plan's monthly Delta lasts up to 31 days past the end of its month."),
        "When you spend, the credit that expires first is used first.".to_string(),
        "Delta never converts to Bliss or to Tickets, and it can never be cashed out.".to_string(),
        "A refund or a chargeback takes back the Delta it paid for.".to_string(),
        "Free tools never cost Delta.".to_string(),
    ]
}

/// A pack as the Worker sends it: the Stripe lookup key, the Delta it buys, its price in dollars.
#[derive(Debug, Clone, Deserialize)]
struct WirePack {
    id: String,
    delta: u64,
    usd: f64,
}

/// The packs in the shared card form. A pack the Worker sent with no Delta in it, or no price, is not offered.
pub fn packs_of(wire: Vec<(String, u64, f64)>) -> Vec<Pack> {
    wire.into_iter()
        .filter(|(_, delta, usd)| *delta > 0 && *usd > 0.0)
        .map(|(id, delta, usd)| Pack { name: delta_pack_name(&id), id, amount: delta, bonus: 0, usd })
        .collect()
}

/// The words for the plan the Worker names: "creator" is "Creator", "player_plus" is "Player Plus". No plan, or the free one, has none.
pub fn plan_label(plan: &str) -> Option<String> {
    if plan.is_empty() || plan == "free" {
        return None;
    }
    let words: Vec<String> = plan
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect();
    Some(words.join(" ")).filter(|s| !s.is_empty())
}

/// What the Buy section says when the packs are not on sale. Studio has no public release yet (2026-10-05): this sentence, and the note
/// under the packs that says the same, change when it ships.
fn not_open() -> impl IntoView {
    view! {
        <div class="g-glass g-none">
            <span class="g-chip warn">"Not open yet"</span>
            <b>"Delta packs are not on sale yet"</b>
            <p>"Studio has no public release yet, so there is nothing to spend Delta on. When packs open, they appear here with their prices."</p>
        </div>
    }
}

#[component]
pub fn DeltaPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let balance = app_state.delta;
    let api_url = StoredValue::new(app_state.api_url.clone());

    let packs = RwSignal::new(Packs::Loading);
    let selected = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    release_on_return(busy);
    // What the address says about the payment that just ended (at the top), and what a buy that could not start says (by the button).
    let notice = RwSignal::new(return_banner(&crate::utils::current_query()));
    let message = RwSignal::new(None::<Message>);

    // In the browser only: which packs are on sale.
    Effect::new(move |_| {
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let answer = client.get::<Vec<WirePack>>("/api/delta/packs").await.map(|w| packs_of(w.into_iter().map(|p| (p.id, p.delta, p.usd)).collect()));
            packs.set(state_of_packs(answer));
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
    let label = Signal::derive(move || chosen().map(|p| buy_label(&p, "Delta")));
    let on_buy = Callback::new(move |_| {
        if let Some(p) = chosen() {
            begin_checkout(api_url.get_value(), "/api/delta/checkout", serde_json::json!({ "pack": p.id }), busy, message);
        }
    });

    view! {
        <CurrencyShell current=Currency::Delta title="Delta" lead="Delta pays for premium Studio tools. Most tools are free and never use it.">
            <Banner message=notice />
            {move || {
                let mut items = Vec::new();
                if let DeltaState::Balance(b) = balance.get() {
                    items.push(("Your balance", format!("{} Delta", format_count(b.whole()))));
                    if let Some(plan) = b.plan.as_deref().and_then(plan_label) {
                        items.push(("Your plan", plan));
                    }
                }
                items.push(("Trial", format_count(TRIAL)));
                items.push(("Lasts", format!("{LIFE_MONTHS} months")));
                items.push(("Cash-out", "Never".to_string()));
                view! { <Figures items=items /> }
            }}

            <section class="g-section" aria-labelledby="delta-buy">
                <div class="g-head"><h2 id="delta-buy">"Buy Delta"</h2><span class="g-rule"></span></div>
                {move || view! {
                    <PackSection packs=packs.get() unit="Delta" placeholders=6 closed=not_open
                        selected=selected label=label busy=busy access=access message=message on_buy=on_buy
                        note=format!("Delta expires {LIFE_MONTHS} months after you buy it, and Studio has no public release yet.") />
                }}
            </section>

            <section class="g-section" aria-labelledby="delta-get">
                <div class="g-head"><h2 id="delta-get">"How you get Delta"</h2><span class="g-rule"></span></div>
                <div class="cur-grid">
                    <article class="g-glass dl-card">
                        <h3 class="cur-name">"A trial"</h3>
                        <p class="dl-purpose">{format!("{} Delta, once, when your identity is verified.", format_count(TRIAL))}</p>
                        <p class="g-hint">"Verification is the same check Bliss cash-out uses. It happens once per account."</p>
                    </article>
                    <article class="g-glass dl-card">
                        <h3 class="cur-name">"A plan"</h3>
                        <p class="dl-purpose">"Some plans add Delta every month."</p>
                        <ul class="dl-points">
                            {PLAN_GRANTS.iter().map(|(plan, n, line)| view! { <li>{format!("{plan}: {} {line}", format_count(*n))}</li> }).collect_view()}
                        </ul>
                        <div class="dl-card-action"><a class="g-btn" href="/premium">"See the plans"</a></div>
                    </article>
                    <article class="g-glass dl-card">
                        <h3 class="cur-name">"A pack"</h3>
                        <p class="dl-purpose">"Buy more whenever you need it."</p>
                        <p class="g-hint">"Sizes and prices are above when packs are on sale. Payment goes through Stripe."</p>
                    </article>
                </div>
            </section>

            <section class="g-section" aria-labelledby="delta-rules">
                <div class="g-head"><h2 id="delta-rules">"How it works"</h2><span class="g-rule"></span></div>
                <div class="g-glass dl-table-wrap">
                    <ul class="dl-points dl-rules">{rules().into_iter().map(|r| view! { <li>{r}</li> }).collect_view()}</ul>
                </div>
            </section>
        </CurrencyShell>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_match_the_workers_and_the_premium_page() {
        assert_eq!(TRIAL, 1_000);
        assert_eq!(LIFE_MONTHS, 12);
        assert_eq!(PLAN_GRANTS.map(|(p, n, _)| (p, n)), [("Creator", 1_000), ("Professional", 16_000), ("Bundle", 900)]);
    }

    #[test]
    fn the_packs_come_in_the_workers_order_with_names_from_their_keys() {
        let wire = vec![
            ("eustress_delta_session".to_string(), 1_000, 10.0),
            ("eustress_delta_project".to_string(), 2_600, 25.0),
            ("eustress_delta_studio".to_string(), 60_000, 500.0),
        ];
        let packs = packs_of(wire);
        assert_eq!(packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Session", "Project", "Studio"]);
        assert_eq!((packs[2].amount, packs[2].usd, packs[2].bonus), (60_000, 500.0, 0));
    }

    #[test]
    fn a_pack_with_no_delta_or_no_price_is_never_offered() {
        let packs = packs_of(vec![("a".to_string(), 0, 10.0), ("b".to_string(), 100, 0.0), ("c".to_string(), 100, 5.0)]);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "c");
    }

    #[test]
    fn the_rules_state_the_workers_terms_and_that_delta_never_converts_or_cashes_out() {
        let rules = rules();
        assert!(rules[0].contains("12 months") && rules[0].contains("31 days"), "{}", rules[0]);
        assert!(rules[1].contains("expires first is used first"));
        assert!(rules[2].contains("never converts to Bliss or to Tickets") && rules[2].contains("never be cashed out"), "{}", rules[2]);
        assert!(rules[3].contains("refund or a chargeback takes back"));
        assert!(rules[4].contains("Free tools never cost Delta"));
    }

    #[test]
    fn the_wire_form_is_what_the_worker_sends() {
        let w: Vec<WirePack> = serde_json::from_str(r#"[{"id":"eustress_delta_week","delta":5400,"usd":50}]"#).unwrap();
        assert_eq!((w[0].id.as_str(), w[0].delta, w[0].usd), ("eustress_delta_week", 5_400, 50.0));
    }

    #[test]
    fn the_checkout_body_names_the_pack_by_the_key_the_worker_listed() {
        // The Worker's PACK_KEYS are the lookup keys it lists as `id`; the page sends that same id back, not the display name.
        let packs = packs_of(vec![("eustress_delta_week".to_string(), 5_400, 50.0)]);
        assert_eq!(serde_json::json!({ "pack": packs[0].id }), serde_json::json!({ "pack": "eustress_delta_week" }));
    }

    #[test]
    fn when_the_packs_are_not_on_sale_the_page_says_so_and_names_no_price() {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        let html = owner.with(|| not_open().to_html());
        assert!(html.contains("Not open yet") && html.contains("not on sale yet"), "{html}");
        assert!(!html.contains('$'), "{html}");
        // A Worker with no Delta route, or with Stripe not set up, leaves the packs closed rather than failed.
        assert_eq!(state_of_packs(Err(crate::api::ApiError::NotFound)), Packs::Closed);
    }

    #[test]
    fn the_plan_is_named_the_way_a_person_says_it() {
        assert_eq!(plan_label("creator").as_deref(), Some("Creator"));
        assert_eq!(plan_label("player_plus").as_deref(), Some("Player Plus"));
        assert_eq!(plan_label("professional").as_deref(), Some("Professional"));
        assert_eq!(plan_label("free"), None);
        assert_eq!(plan_label(""), None);
        assert_eq!(plan_label("__"), None);
    }
}
