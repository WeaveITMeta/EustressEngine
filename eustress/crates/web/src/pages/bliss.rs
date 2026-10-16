// =============================================================================
// Eustress Web - Bliss (/bliss)
// =============================================================================
// Bliss (BLS): the Proof-of-Contribution currency. Contributors earn it; the treasury that pays them is funded here (through
// Stripe Checkout), and the live KPIs come from the witness. The look, the tabs, the pack cards and the buy bar are the ones the
// Tickets and Delta pages use (components/currency_ui.rs); the words on the page are the ones this page has always carried.
//
// Table of Contents:
// 1. Data and pure helpers (tested)
// 2. The page
// 3. Sections that are text
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::ApiClient;
use crate::components::currency_ui::{begin_checkout, release_on_return, return_banner, Banner, Currency, CurrencyShell, Figures, Message};
use crate::state::{AppState, AuthState};
use crate::utils::{format_bliss_exact, format_count};

// -----------------------------------------------------------------------------
// 1. Data and pure helpers
// -----------------------------------------------------------------------------

/// Live KPI data fetched from the Bliss API.
#[derive(Clone, Debug, PartialEq)]
pub struct BlissKpi {
    pub total_supply: String,
    pub circulating_supply: String,
    pub treasury_balance: String,
    pub active_nodes: u64,
    pub total_contributors: u64,
    pub avg_bliss_earned: String,
    /// Annual emission rate, live from the ledger (e.g. "5.0% / yr").
    /// Bliss is Proof-of-Contribution: there is no mining or hashrate.
    pub emission_rate: String,
    pub daily_distribution: String,
}

impl Default for BlissKpi {
    fn default() -> Self {
        Self {
            total_supply: "Loading...".to_string(),
            circulating_supply: "Loading...".to_string(),
            treasury_balance: "Loading...".to_string(),
            active_nodes: 0,
            total_contributors: 0,
            avg_bliss_earned: "Loading...".to_string(),
            emission_rate: "Loading...".to_string(),
            daily_distribution: "Loading...".to_string(),
        }
    }
}

/// A treasury funding tier.
#[derive(Clone, Debug, PartialEq)]
pub struct InvestmentTier {
    pub id: &'static str,
    pub name: &'static str,
    pub amount_usd: &'static str,
    pub description: &'static str,
}

/// The four tiers, in the order they are offered. The ids are the Worker's.
pub const TIERS: [InvestmentTier; 4] = [
    InvestmentTier { id: "seed", name: "Seed", amount_usd: "$25", description: "Support the ecosystem at ground level" },
    InvestmentTier { id: "growth", name: "Growth", amount_usd: "$100", description: "Meaningful treasury investment" },
    InvestmentTier { id: "sustainer", name: "Sustainer", amount_usd: "$500", description: "Significant contribution to network sustainability" },
    InvestmentTier { id: "patron", name: "Patron", amount_usd: "$1,000+", description: "Major backer powering the Bliss economy" },
];

/// The note under the treasury button. A contribution goes whole to the treasury: the Worker records its full amount with no
/// platform fee (index.js, handleStripeWebhook: fee_cents 0, platform_cents 0).
pub const TREASURY_NOTE: &str = "Secure payment via Stripe. The whole amount is credited to the treasury. You pay on Stripe's own page; Eustress never sees your card.";

/// The smallest and the largest custom amount the Worker takes, in dollars.
pub const MIN_CUSTOM_USD: f64 = 5.0;
pub const MAX_CUSTOM_USD: f64 = 100_000.0;

/// The body of a treasury checkout: the chosen tier or a custom amount, and whether it repeats monthly. `None` when neither a tier
/// nor a custom amount within the Worker's limits has been chosen.
pub fn invest_body(tier: Option<usize>, custom: &str, recurring: bool) -> Option<serde_json::Value> {
    let tier_id = tier.and_then(|i| TIERS.get(i)).map(|t| t.id);
    let custom_amount = custom.trim().parse::<f64>().ok().filter(|a| a.is_finite() && (MIN_CUSTOM_USD..=MAX_CUSTOM_USD).contains(a));
    if tier_id.is_none() && custom_amount.is_none() {
        return None;
    }
    Some(serde_json::json!({
        "tier": tier_id,
        "mode": if recurring { "recurring" } else { "one_time" },
        "custom_amount": custom_amount,
    }))
}

/// The only address bank onboarding may be sent to: Stripe Connect, over https.
pub fn safe_connect_url(url: &str) -> bool {
    url.strip_prefix("https://connect.stripe.com/").is_some_and(|rest| !rest.is_empty() && !url.contains(char::is_whitespace) && !url.contains('\\'))
}

/// What a bank onboarding that did not start says, and whether the reason is a missing plan (the page then offers the plans).
pub fn onboarding_words(body: &str) -> (String, bool) {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let message = text("error").unwrap_or_else(|| "Unknown error".to_string());
    match text("code").as_deref() {
        Some("plan_required") => {
            let reason = text("reason").unwrap_or_default();
            (if reason.is_empty() { message } else { format!("{}. {reason}", message.trim_end_matches('.')) }, true)
        }
        // The Worker's own sentence: the identity key was just replaced, or the session ended.
        Some("payout_pinned" | "account_frozen" | "session_revoked" | "account_unavailable") => (message, false),
        _ => (format!("Stripe: {message}"), false),
    }
}

/// Group an integer-valued f64 with thousands separators (100000000 reads "100,000,000").
fn fmt_thousands(n: f64) -> String {
    let rounded = n.round() as i64;
    let digits = format_count(rounded.unsigned_abs());
    if rounded < 0 { format!("-{digits}") } else { digits }
}

/// Pull the live Bliss KPIs from the witness and update the signal. Reads three endpoints (the economics and rate view, community
/// stats, and node stats) and maps their authoritative fields onto the KPI cards. Any endpoint that fails leaves its prior value in
/// place (no flicker to zero).
#[cfg(not(feature = "ssr"))]
async fn refresh_bliss_kpis(kpi: RwSignal<BlissKpi>) {
    // Economics: treasury, live emission, supply, distributed, drip rate.
    if let Ok(resp) = gloo_net::http::Request::get(&format!("{}/api/payouts/rate", crate::state::api_base())).send().await {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            let num = |k: &str| data.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let treasury = num("treasury_usd");
            let daily_emission = num("daily_bls_emission");
            let current_supply = num("current_supply");
            let total_distributed = num("total_distributed");
            let annual_rate = num("annual_emission_rate");
            let rate_display = data.get("rate_display").and_then(|v| v.as_str()).unwrap_or("-").to_string();

            kpi.update(|k| {
                k.treasury_balance = format!("${:.2} USD", treasury);
                // Live emission (decays every 4 years), not a Year-1 constant.
                k.daily_distribution = format!("{} BLS", fmt_thousands(daily_emission));
                // Supply grows with each mint; circulating = minted-to-date.
                k.total_supply = format!("{} BLS", fmt_thousands(current_supply));
                k.circulating_supply = format!("{} BLS", fmt_thousands(total_distributed));
                k.avg_bliss_earned = rate_display;
                k.emission_rate = format!("{:.1}% / yr", annual_rate * 100.0);
            });
        }
    }

    // Community: contributor count.
    if let Ok(resp) = gloo_net::http::Request::get(&format!("{}/api/community/stats", crate::state::api_base())).send().await {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            let users = data.get("total_users").and_then(|v| v.as_u64()).unwrap_or(0);
            kpi.update(|k| k.total_contributors = users);
        }
    }

    // Nodes: live active-node count from heartbeats.
    if let Ok(resp) = gloo_net::http::Request::get(&format!("{}/api/node/stats", crate::state::api_base())).send().await {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            let nodes = data.get("active_nodes").and_then(|v| v.as_u64()).unwrap_or(0);
            kpi.update(|k| k.active_nodes = nodes);
        }
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
pub fn BlissPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = StoredValue::new(app_state.api_url.clone());

    // Treasury funding state.
    let selected_tier = RwSignal::new(None::<usize>);
    let custom_amount = RwSignal::new(String::new());
    let is_recurring = RwSignal::new(false);
    let busy = RwSignal::new(false);
    release_on_return(busy);
    // What the address says about the payment that just ended (at the top), and what a funding that could not start says (by the button).
    let notice = RwSignal::new(return_banner(&crate::utils::current_query()));
    let message = RwSignal::new(None::<Message>);

    // Bank onboarding state.
    let payout_busy = RwSignal::new(false);
    release_on_return(payout_busy);
    let payout_message = RwSignal::new(None::<Message>);
    let needs_plan = RwSignal::new(false);

    // KPI data: fetched live from the witness and refreshed every 20 seconds so the page stays near-real-time without a websocket.
    // Every value is authoritative ledger state; nothing on this page is hardcoded. Browser only: the prerender renders the
    // empty frame and the live app fills it the moment it mounts.
    let kpi = RwSignal::new(BlissKpi::default());
    #[cfg(not(feature = "ssr"))]
    {
        spawn_local(refresh_bliss_kpis(kpi));
        // Every 20 seconds, for as long as the page is open: leaving it ends the polling.
        if let Ok(poll) = leptos::leptos_dom::helpers::set_interval_with_handle(move || spawn_local(refresh_bliss_kpis(kpi)), std::time::Duration::from_secs(20)) {
            on_cleanup(move || poll.clear());
        }
    }

    let connect_bank = move |_| {
        if payout_busy.get_untracked() {
            return;
        }
        payout_busy.set(true);
        payout_message.set(None);
        needs_plan.set(false);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            match client.post::<serde_json::Value, _>("/api/stripe/connect/onboard", &serde_json::json!({})).await {
                Ok(answer) => match answer.get("url").and_then(|u| u.as_str()) {
                    Some(url) if safe_connect_url(url) => {
                        if let Some(window) = web_sys::window() {
                            let _ = window.location().set_href(url);
                        }
                    }
                    _ => {
                        payout_busy.set(false);
                        payout_message.set(Some(Message::Error("Stripe did not give a page to connect a bank account.".to_string())));
                    }
                },
                Err(crate::api::ApiError::Server { message, .. }) => {
                    let (words, plan) = onboarding_words(&message);
                    payout_busy.set(false);
                    needs_plan.set(plan);
                    payout_message.set(Some(Message::Error(words)));
                }
                Err(e) => {
                    payout_busy.set(false);
                    payout_message.set(Some(Message::Error(crate::api::failure_sentence(&e))));
                }
            }
        });
    };

    let signed_in = Signal::derive(move || matches!(auth.get(), AuthState::Authenticated(_)));
    let body = move || invest_body(selected_tier.get(), &custom_amount.get(), is_recurring.get());

    view! {
        <CurrencyShell current=Currency::Bliss title="Bliss (BLS)" lead="Invest in the Future of Digital Labor. Bliss is a Proof-of-Contribution cryptocurrency where your creative work earns real value. Fund the treasury to grow the ecosystem, run a node to strengthen the network, or simply build and earn.">
            <Banner message=notice />

            // The account: balance and payouts, or the way in.
            {move || match auth.get() {
                AuthState::Authenticated(user) => view! {
                    <section class="cur-account" aria-label="Your Bliss">
                        <Figures items=vec![("Your balance", format!("{} BLS", format_bliss_exact(user.bliss_balance)))] />
                        <div class="g-glass dl-card">
                            <h2 class="cur-name">"Daily payouts"</h2>
                            <p class="dl-purpose">"Connect your bank to receive USD daily."</p>
                            <p class="g-hint">"Stripe Connect and 1099s. Cash-out needs identity verification and a Creator, Professional or Bundle plan."</p>
                            <div class="dl-card-action">
                                <button type="button" class="g-btn g-btn-primary" disabled=move || payout_busy.get() on:click=connect_bank>
                                    {move || if payout_busy.get() { "Opening Stripe..." } else { "Connect bank account" }}
                                </button>
                                <Banner message=payout_message />
                                {move || needs_plan.get().then(|| view! { <a class="g-btn" href="/premium">"See the plans"</a> })}
                            </div>
                        </div>
                    </section>
                }.into_any(),
                _ => view! {
                    <section class="g-glass cur-wallet" aria-label="Sign in">
                        <div>
                            <h2 class="cur-name">"Your identity.toml is your wallet"</h2>
                            <p class="dl-purpose">"Sign in to view your BLS balance."</p>
                        </div>
                        <a href="/login" class="g-btn g-btn-primary g-btn-lg">"Sign in"</a>
                    </section>
                }.into_any(),
            }}

            // Live KPIs.
            <section class="g-section" aria-labelledby="bliss-kpi">
                <div class="g-head"><h2 id="bliss-kpi">"Live KPIs"</h2><span class="g-chip live"><i class="g-dot"></i>"Live"</span><span class="g-rule"></span></div>
                {move || {
                    let k = kpi.get();
                    let cards = vec![
                        ("Treasury balance", k.treasury_balance.clone()),
                        ("Current supply", k.total_supply.clone()),
                        ("Circulating", k.circulating_supply.clone()),
                        ("Active nodes", k.active_nodes.to_string()),
                        ("Contributors", k.total_contributors.to_string()),
                        ("USD drip per BLS", k.avg_bliss_earned.clone()),
                        ("Emission rate", k.emission_rate.clone()),
                        ("Daily distribution", k.daily_distribution.clone()),
                    ];
                    view! {
                        <dl class="cur-kpis">
                            {cards.into_iter().map(|(label, value)| view! { <div class="g-glass cur-kpi"><dt>{label}</dt><dd>{value}</dd></div> }).collect_view()}
                        </dl>
                    }
                }}
            </section>

            // Fund the treasury.
            <section class="g-section" aria-labelledby="bliss-fund">
                <div class="g-head"><h2 id="bliss-fund">"Fund the treasury"</h2><span class="g-rule"></span></div>
                <p class="dl-purpose">"100% goes to the treasury. No middlemen, no VC extraction. The treasury drips daily to active contributors; your investment directly rewards builders."</p>
                <div class="cur-toggle" role="group" aria-label="How often">
                    <button type="button" class="cur-toggle-btn" class:on=move || !is_recurring.get() aria-pressed=move || (!is_recurring.get()).to_string() on:click=move |_| is_recurring.set(false)>"One-time"</button>
                    <button type="button" class="cur-toggle-btn" class:on=move || is_recurring.get() aria-pressed=move || is_recurring.get().to_string() on:click=move |_| is_recurring.set(true)>"Monthly"</button>
                </div>
                <TierCards selected=selected_tier custom=custom_amount recurring=is_recurring />
                <div class="g-glass cur-custom">
                    <label for="bliss-custom">"Or enter a custom amount"</label>
                    <div class="cur-custom-input">
                        <span aria-hidden="true">"$"</span>
                        <input id="bliss-custom" type="number" min="5" step="1" inputmode="decimal" placeholder="0"
                            prop:value=move || custom_amount.get()
                            on:input=move |e| { custom_amount.set(event_target_value(&e)); selected_tier.set(None); } />
                        <span aria-hidden="true">"USD"</span>
                    </div>
                </div>
                <div class="pk-bar">
                    <button type="button" class="g-btn g-btn-primary g-btn-lg" disabled=move || body().is_none() || busy.get()
                        on:click=move |_| {
                            if let Some(b) = body() {
                                begin_checkout(api_url.get_value(), "/api/stripe/checkout", b, busy, message);
                            }
                        }>
                        {move || if busy.get() { "Opening Stripe...".to_string() } else if is_recurring.get() { "Subscribe monthly".to_string() } else { "Fund treasury".to_string() }}
                    </button>
                    {move || (!signed_in.get()).then(|| view! { <p class="g-hint">"You can fund the treasury without signing in. Signed in, the payment is recorded to your account."</p> })}
                    <Banner message=message />
                    <p class="g-hint pk-note">{TREASURY_NOTE}</p>
                </div>
            </section>

            <TextSections />
        </CurrencyShell>
    }
}

/// The four tiers as cards to choose from. Choosing one clears the custom amount; each card is a label around a native radio input.
#[component]
fn TierCards(selected: RwSignal<Option<usize>>, custom: RwSignal<String>, recurring: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="pk-grid" role="radiogroup" aria-label="Treasury tiers">
            {TIERS.iter().enumerate().map(|(i, tier)| {
                let on = move || selected.get() == Some(i);
                view! {
                    <label class="pk-card pk-tier" class:on=on>
                        <input type="radio" class="g-sr" name="bliss-tier" prop:checked=on on:change=move |_| { selected.set(Some(i)); custom.set(String::new()); } />
                        <span class="pk-name">{tier.name}</span>
                        <span class="pk-price">{tier.amount_usd}<small>{move || if recurring.get() { " /mo" } else { "" }}</small></span>
                        <span class="pk-desc">{tier.description}</span>
                    </label>
                }
            }).collect_view()}
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Sections that are text
// -----------------------------------------------------------------------------

const WHY: [(&str, &str); 8] = [
    ("You Earn by Creating", "Traditional money requires selling your time to someone else. Bliss rewards you directly for building, scripting, teaching, and collaborating. Your creative output has intrinsic value."),
    ("Predictable Tail Emission", "No unlimited printing. Bliss uses a halving schedule with a permanent tail emission floor: enough to reward contributors forever, never enough to devalue holdings."),
    ("Fork-Portable", "Your identity and earnings travel with you. If a server shuts down or you disagree with governance, take your TOML identity file to any fork. No lock-in, ever."),
    ("Fully Auditable Source", "Every line of the blockchain, wallet, and distribution system is source-available. Anyone can audit the code, run a fork, or propose changes. No black-box tokenomics."),
    ("True Self-Custody", "Ed25519 keypairs generated on your machine. Your keys never leave your device. No custodial wallets, no exchanges holding your funds, no \"freeze\" buttons."),
    ("Community Governed", "The Online Trust Registry lets the community flag forks that deviate from fair rates. No central authority decides who earns what: the network self-regulates."),
    ("Daily Payouts", "Get paid every day at UTC midnight, not monthly, not quarterly. Yesterday's contributions become today's earnings. The fastest feedback loop in crypto. Cash-out needs identity verification and a Creator, Professional or Bundle plan."),
    ("KYC Compliant", "72 IRS QI-approved jurisdictions. Identity verified once, recognized everywhere. Sybil-proof by design: one person, one account, real contributions only."),
];

const INNOVATIONS: [(&str, &str); 6] = [
    ("Dual-Authority Proof", "Every contribution requires two signatures: yours (proving you did the work) and the Cloudflare witness (proving it happened legitimately). Neither party can forge alone. AI can fake user signatures but cannot fake the independent server co-signature."),
    ("Contribution Hash Chains", "Your work history is a signed, append-only chain. Each entry includes a hash of the previous entry. Tampering with any past contribution breaks the chain, detectable by any verifier."),
    ("Sovereign Portable Identity", "One TOML file on your Desktop = your entire identity. Ed25519 keypair, contribution history, server attestations, succession plan. Take it anywhere. No accounts, no emails, no passwords."),
    ("Online Trust Registry", "Every fork publishes its issuance rate. The network computes the median and flags outliers. Forks issuing too much or too little are visible to all. No central authority needed, just math."),
    ("Non-Coercive by Design", "Light Node is opt-out, not opt-in. You can earn without running any infrastructure. Full Node is opt-in for those who want to contribute more. The system works for everyone, not just power users."),
    ("Surgical Fork Revocation", "If a fork is caught gaming the system, only the BLS it issued gets revoked. Your direct contributions and earnings from other forks are never touched. Per-fork ledger tracking makes revocation precise, not punitive."),
];

const FAQ: [(&str, &str); 6] = [
    ("How do I earn BLS?", "Use Eustress Engine. Your building, scripting, design, and collaboration activities are automatically tracked. The Cloudflare witness co-signs your contributions and BLS is distributed daily at UTC midnight based on your score."),
    ("Where does treasury funding go?", "100% goes to contributors. The treasury drips daily to active builders using exponential decay: large deposits taper naturally over months, not cliff-drop. Your investment directly rewards the people building on Eustress. This is not a token purchase; it's funding the people who create value."),
    ("What happens if a server goes down?", "Your identity.toml file contains everything you need. Take it to any other Eustress fork and your contribution history, wallet, and identity carry over. Fork portability is a core design principle."),
    ("Do I need to run a Full Node?", "No. Light Node is the default and runs automatically when you use the engine. Full Node is opt-in for users who want to store blockchain data and produce blocks, earning a +10% BLS bonus."),
    ("How is Bliss different from mining?", "Mining rewards hardware investment. Bliss rewards creative contribution. You earn by building things, not by running GPUs. Proof-of-Contribution values human work over electricity consumption."),
    ("Can AI-generated contributions game the system?", "The dual-authority model makes this hard. The Cloudflare witness independently validates that contribution events actually occurred in the engine. AI can't forge the server co-signature. Rate limiting and the Online Trust Registry catch statistical anomalies."),
];

#[component]
fn TextSections() -> impl IntoView {
    view! {
        <section class="g-section" aria-labelledby="bliss-why">
            <div class="g-head"><h2 id="bliss-why">"Why Bliss?"</h2><span class="g-rule"></span></div>
            <p class="dl-purpose">"What makes Bliss different from traditional money and other cryptocurrencies."</p>
            <div class="cur-cards">
                {WHY.iter().map(|(t, p)| view! { <article class="g-glass dl-card"><h3 class="cur-sub">{*t}</h3><p class="dl-purpose">{*p}</p></article> }).collect_view()}
            </div>
        </section>

        <section class="g-section" aria-labelledby="bliss-nodes">
            <div class="g-head"><h2 id="bliss-nodes">"Node participation"</h2><span class="g-rule"></span></div>
            <p class="dl-purpose">"Every Eustress Engine instance can contribute to the Bliss network. Choose your participation level."</p>
            <div class="dl-compare">
                <article class="g-glass dl-card">
                    <span class="g-chip live">"Default"</span>
                    <h3 class="cur-name">"Light Node"</h3>
                    <p class="dl-purpose">"Earn by creating: zero setup."</p>
                    <ul class="dl-points">
                        <li>"Automatic: runs when you use Eustress Engine"</li>
                        <li>"Contributions co-signed by Cloudflare witness"</li>
                        <li>"Daily BLS distribution (UTC midnight) based on contribution score"</li>
                        <li>"No port forwarding, no server setup"</li>
                        <li>"Opt-out at any time in settings"</li>
                        <li>"Minimal bandwidth: only sends contribution hashes"</li>
                    </ul>
                    <p class="dl-best"><b>"Requirements"</b>"Internet connection (for co-signing only)"</p>
                </article>
                <article class="g-glass dl-card">
                    <span class="g-chip">"Opt-in"</span>
                    <h3 class="cur-name">"Full Node"</h3>
                    <p class="dl-purpose">"Strengthen the network, earn more."</p>
                    <ul class="dl-points">
                        <li>"Everything in Light Node"</li>
                        <li>"+10% bonus on all BLS rewards"</li>
                        <li>"Store and validate blockchain data"</li>
                        <li>"Participate in block production"</li>
                        <li>"Serve as a peer for other nodes"</li>
                        <li>"Cloudflare Tunnel: no port forwarding needed"</li>
                    </ul>
                    <p class="dl-best"><b>"Requirements"</b>"2GB+ RAM, 10GB+ disk, stable internet"</p>
                </article>
            </div>
        </section>

        <section class="g-section" aria-labelledby="bliss-innovates">
            <div class="g-head"><h2 id="bliss-innovates">"How Bliss innovates"</h2><span class="g-rule"></span></div>
            <ol class="cur-steps">
                {INNOVATIONS.iter().enumerate().map(|(i, (t, p))| view! {
                    <li class="g-glass cur-step"><span class="cur-num">{format!("{:02}", i + 1)}</span><div><h3 class="cur-sub">{*t}</h3><p class="dl-purpose">{*p}</p></div></li>
                }).collect_view()}
            </ol>
        </section>

        <section class="g-section" aria-labelledby="bliss-faq">
            <div class="g-head"><h2 id="bliss-faq">"Frequently asked questions"</h2><span class="g-rule"></span></div>
            <div class="cur-faq">
                {FAQ.iter().map(|(q, a)| view! { <details class="g-glass"><summary>{*q}</summary><p>{*a}</p></details> }).collect_view()}
            </div>
        </section>

        <section class="g-glass cur-wallet" aria-label="Documentation">
            <div>
                <h2 class="cur-name">"Bliss documentation"</h2>
                <p class="dl-purpose">"How contributions are scored, how the daily BLS emission and the USD drip work, and what runs today."</p>
            </div>
            <a href="/docs/earning" class="g-btn g-btn-lg">"View documentation"</a>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_treasury_checkout_needs_a_tier_or_a_custom_amount_of_at_least_the_minimum() {
        assert_eq!(invest_body(None, "", false), None);
        assert_eq!(invest_body(None, "4.99", false), None, "under the minimum");
        assert_eq!(invest_body(None, "100000.01", false), None, "over the Worker's limit");
        assert!(invest_body(None, "5", false).is_some() && invest_body(None, "100000", true).is_some(), "both limits are allowed");
        assert_eq!(invest_body(None, "abc", false), None);
        assert_eq!(invest_body(None, "inf", false), None);
        assert_eq!(invest_body(Some(9), "", false), None, "no such tier");
        let seed = invest_body(Some(0), "", false).unwrap();
        assert_eq!(seed["tier"], "seed");
        assert_eq!(seed["mode"], "one_time");
        assert!(seed["custom_amount"].is_null());
        let monthly = invest_body(Some(3), "", true).unwrap();
        assert_eq!((monthly["tier"].as_str(), monthly["mode"].as_str()), (Some("patron"), Some("recurring")));
        let custom = invest_body(None, " 75 ", false).unwrap();
        assert!(custom["tier"].is_null());
        assert_eq!(custom["custom_amount"], 75.0);
    }

    #[test]
    fn the_note_says_the_whole_amount_goes_to_the_treasury_and_names_no_fee_the_worker_does_not_take() {
        assert!(TREASURY_NOTE.contains("whole amount is credited to the treasury"));
        assert!(!TREASURY_NOTE.contains("fee"), "{TREASURY_NOTE}");
        assert!(TREASURY_NOTE.contains("never sees your card"));
    }

    fn tiers_html(chosen: Option<usize>, monthly: bool) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            let (selected, custom, recurring) = (RwSignal::new(chosen), RwSignal::new(String::new()), RwSignal::new(monthly));
            view! { <TierCards selected=selected custom=custom recurring=recurring /> }.to_html()
        })
    }

    #[test]
    fn the_tier_cards_show_every_tier_with_its_price_and_say_monthly_only_when_it_is() {
        let once = tiers_html(None, false);
        for want in ["Seed", "$25", "Growth", "$100", "Sustainer", "$500", "Patron", "$1,000+", "Support the ecosystem at ground level"] {
            assert!(once.contains(want), "{want}: {once}");
        }
        assert_eq!(once.matches(r#"type="radio""#).count(), 4, "{once}");
        assert_eq!(once.matches(r#"name="bliss-tier""#).count(), 4, "one group: {once}");
        assert!(!once.contains("/mo"), "{once}");
        let monthly = tiers_html(None, true);
        assert_eq!(monthly.matches("/mo").count(), 4, "{monthly}");
    }

    #[test]
    fn only_the_chosen_tier_is_marked() {
        assert_eq!(tiers_html(None, false).matches(r#"class="pk-card pk-tier on""#).count(), 0);
        let one = tiers_html(Some(2), false);
        assert_eq!(one.matches(r#"class="pk-card pk-tier on""#).count(), 1, "{one}");
        assert_eq!(one.matches(r#"class="pk-card pk-tier""#).count(), 3, "{one}");
    }

    #[test]
    fn the_tiers_keep_the_workers_ids_in_order() {
        assert_eq!(TIERS.map(|t| t.id), ["seed", "growth", "sustainer", "patron"]);
    }

    #[test]
    fn bank_onboarding_goes_only_to_stripe_connect_over_https() {
        assert!(safe_connect_url("https://connect.stripe.com/setup/e/acct_1/abc"));
        for bad in ["", "http://connect.stripe.com/x", "https://connect.stripe.com/", "https://connect.stripe.com.evil.example/x", "https://checkout.stripe.com/x", "https://connect.stripe.com/a b"] {
            assert!(!safe_connect_url(bad), "{bad:?}");
        }
    }

    #[test]
    fn an_onboarding_that_did_not_start_says_what_the_worker_said() {
        let (plan, needs) = onboarding_words(r#"{"error":"A plan is required","code":"plan_required","reason":"Cash-out needs a Creator plan"}"#);
        assert!(needs);
        assert_eq!(plan, "A plan is required. Cash-out needs a Creator plan");
        let (frozen, needs) = onboarding_words(r#"{"error":"Payouts are paused after a key change","code":"payout_pinned"}"#);
        assert!(!needs);
        assert_eq!(frozen, "Payouts are paused after a key change");
        let (other, needs) = onboarding_words(r#"{"error":"Account restricted"}"#);
        assert!(!needs);
        assert_eq!(other, "Stripe: Account restricted");
        assert_eq!(onboarding_words("not json").0, "Stripe: Unknown error");
    }

    #[test]
    fn numbers_are_grouped_in_threes() {
        assert_eq!(fmt_thousands(100_000_000.0), "100,000,000");
        assert_eq!(fmt_thousands(999.4), "999");
        assert_eq!(fmt_thousands(-1_500.0), "-1,500");
    }
}
