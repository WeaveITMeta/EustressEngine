// =============================================================================
// Eustress Web - The currency pages' shared parts (Tickets, Bliss, Delta)
// =============================================================================
// One look for the three pages that talk about money: the same header, a strip of tabs to move between the currencies, the same
// balance strip, the same pack cards and the same buy bar. Tickets and Delta sell packs through Stripe Checkout; the Bliss page
// funds the treasury the same way. What the shared parts promise:
//
//   * A pack's price, size and rate come from the Worker (`/api/tickets/packages`, `/api/delta/packs`). Nothing here is a price.
//   * "Best rate" is computed (the most units per dollar), never asserted; "most popular" is not claimed anywhere.
//   * A buy goes to Stripe's own page. The address the Worker returns is used only when it is Stripe Checkout's (https on
//     checkout.stripe.com); anything else is refused and nothing is charged. Eustress never sees a card.
//   * A buy that cannot start says why in words, on the page, instead of an alert box.
//
// Table of Contents:
// 1. The currencies and what is shown (pure, tested)
// 2. The shell
// 3. Balance, packs, buy bar, the pack section
// 4. Starting a checkout
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{self, ApiClient, ApiError};
use crate::components::{CentralNav, Footer};
use crate::state::AuthState;
use crate::utils::format_count;

// -----------------------------------------------------------------------------
// 1. The currencies and what is shown
// -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    Tickets,
    Bliss,
    Delta,
}

impl Currency {
    pub const ALL: [Currency; 3] = [Currency::Tickets, Currency::Bliss, Currency::Delta];

    pub fn name(self) -> &'static str {
        match self {
            Currency::Tickets => "Tickets",
            Currency::Bliss => "Bliss",
            Currency::Delta => "Delta",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Currency::Tickets => "/tickets",
            Currency::Bliss => "/bliss",
            Currency::Delta => "/delta",
        }
    }
}

/// One thing for sale: a pack of Tickets or of Delta. `amount` is what the buyer ends up with (bonus included).
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub amount: u64,
    /// Of `amount`, how much is bonus (0 when none).
    pub bonus: u64,
    pub usd: f64,
}

/// "$25", "$49.99": a price with the cents only when there are some.
pub fn price_words(usd: f64) -> String {
    if (usd - usd.round()).abs() < 0.005 { format!("${}", usd.round() as u64) } else { format!("${usd:.2}") }
}

/// How many units a dollar buys in this pack, rounded: "104 per $1". Empty for a price that is not positive.
pub fn rate_words(pack: &Pack) -> String {
    if pack.usd <= 0.0 {
        return String::new();
    }
    format!("{} per $1", format_count((pack.amount as f64 / pack.usd).round() as u64))
}

/// The pack with the best rate, when one is strictly better than every other and there is more than one to choose from.
pub fn best_rate(packs: &[Pack]) -> Option<String> {
    if packs.len() < 2 {
        return None;
    }
    let rate = |p: &Pack| if p.usd > 0.0 { p.amount as f64 / p.usd } else { 0.0 };
    let best = packs.iter().max_by(|a, b| rate(a).partial_cmp(&rate(b)).unwrap_or(std::cmp::Ordering::Equal))?;
    let top = rate(best);
    (top > 0.0 && packs.iter().filter(|p| (rate(p) - top).abs() < 1e-9).count() == 1).then(|| best.id.clone())
}

/// The name of a Delta pack from its Stripe lookup key: "eustress_delta_week" is "Week".
pub fn delta_pack_name(key: &str) -> String {
    let word = key.rsplit('_').next().unwrap_or(key);
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

/// Whether the visitor can buy yet: signed in, signed out, or not known (the page has not heard from the account).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Checking,
    SignedIn,
    SignedOut,
}

pub fn access_of(auth: &AuthState) -> Access {
    match auth {
        AuthState::Authenticated(_) => Access::SignedIn,
        AuthState::Unauthenticated => Access::SignedOut,
        AuthState::Unknown => Access::Checking,
    }
}

/// The text on the buy button for the chosen pack.
pub fn buy_label(pack: &Pack, unit: &str) -> String {
    format!("Buy {} {} for {}", format_count(pack.amount), unit, price_words(pack.usd))
}

/// The only address a payment may be sent to: Stripe Checkout, over https.
pub fn safe_checkout_url(url: &str) -> bool {
    match url.strip_prefix("https://checkout.stripe.com/") {
        Some(rest) => !rest.is_empty() && !url.contains(char::is_whitespace) && !url.contains('\\'),
        None => false,
    }
}

/// The `error` and `code` of a Worker answer's JSON text, when it has them.
pub fn worker_words(text: &str) -> (Option<String>, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return (None, None) };
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    (get("error"), get("code"))
}

/// What a purchase that could not start says. Nothing was charged in any of these.
pub fn checkout_message(err: &ApiError) -> String {
    match err {
        ApiError::Unauthorized => "Sign in to buy. Nothing was charged.".to_string(),
        ApiError::NotFound => "That pack is not on sale right now. Nothing was charged.".to_string(),
        ApiError::Server { status: 503, .. } => "Buying is not open yet. Nothing was charged.".to_string(),
        ApiError::Server { status: 502, .. } => "Stripe could not open its payment page. Try again in a moment. Nothing was charged.".to_string(),
        ApiError::Server { message, .. } => match worker_words(message).0 {
            Some(words) => format!("{}. Nothing was charged.", words.trim_end_matches('.')),
            None => format!("{} Nothing was charged.", api::failure_sentence(err)),
        },
        other => format!("{} Nothing was charged.", api::failure_sentence(other)),
    }
}

/// How long to wait between reads of an account's balance while a payment settles, and how many reads to make: about a minute in
/// all. The credit is made when Stripe tells the Worker the payment cleared, a moment after the buyer lands back on the site.
pub const BALANCE_REFRESH_EVERY_MS: u32 = 5_000;
pub const BALANCE_REFRESHES: u32 = 12;

/// Whether the address says a Tickets purchase just went through, so the balance is still settling.
pub fn paid_return(query: &str) -> bool {
    crate::utils::query_value(query, "purchased").as_deref() == Some("true")
}

/// A line on the page about what just happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Info(String),
    Error(String),
}

/// What the address says about a payment that just ended: `?purchased=true` and `?purchased=false` (Tickets), `?cancelled=1`
/// (Delta) and `?funded=true` and `?funded=false` (the Bliss treasury).
pub fn return_banner(query: &str) -> Option<Message> {
    use crate::utils::query_value;
    let said = |name: &str| query_value(query, name);
    if said("purchased").as_deref() == Some("true") {
        return Some(Message::Info("Thank you. Your purchase went through; your balance updates in a moment.".to_string()));
    }
    if said("funded").as_deref() == Some("true") {
        return Some(Message::Info("Thank you. Your payment went through; all of it goes to the treasury.".to_string()));
    }
    if said("purchased").as_deref() == Some("false") || said("funded").as_deref() == Some("false") || said("cancelled").as_deref() == Some("1") {
        return Some(Message::Info("The payment was cancelled. Nothing was charged.".to_string()));
    }
    None
}

// -----------------------------------------------------------------------------
// 2. The shell
// -----------------------------------------------------------------------------

/// The page every currency lives in: nav, backdrop, the heading, the tabs, the content, the footer.
#[component]
pub fn CurrencyShell(
    /// The currency whose tab is lit; none on the overview.
    #[prop(optional)]
    current: Option<Currency>,
    title: &'static str,
    lead: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class="page g-page dl-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main dl-main cur-main">
                <div class="g-wrap">
                    <header class="dl-head">
                        <div class="g-kicker"><span class="g-kick-rule"></span>"Currency"</div>
                        <h1 class="dl-title">{title}</h1>
                        <p class="dl-lead">{lead}</p>
                    </header>
                    <nav class="cur-tabs" aria-label="Currencies">
                        <a href="/currency" class="cur-tab" aria-current=if current.is_none() { Some("page") } else { None }>"Overview"</a>
                        {Currency::ALL.iter().map(|c| {
                            let c = *c;
                            view! { <a href=c.path() class="cur-tab" aria-current=if current == Some(c) { Some("page") } else { None }>{c.name()}</a> }
                        }).collect_view()}
                    </nav>
                    {children()}
                </div>
            </main>
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Balance, packs, buy bar
// -----------------------------------------------------------------------------

/// A strip of labelled figures, in the same mono white as the Gallery's stats.
#[component]
pub fn Figures(items: Vec<(&'static str, String)>) -> impl IntoView {
    (!items.is_empty()).then(|| {
        view! {
            <dl class="g-spec cur-figures">
                {items.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()}
            </dl>
        }
    })
}

/// The line about what just happened, in the page's own words.
#[component]
pub fn Banner(message: RwSignal<Option<Message>>) -> impl IntoView {
    view! {
        {move || message.get().map(|m| match m {
            Message::Info(t) => view! { <p class="cur-banner" role="status">{t}</p> }.into_any(),
            Message::Error(t) => view! { <p class="cur-banner bad" role="alert">{t}</p> }.into_any(),
        })}
    }
}

/// The packs as cards to choose from. One is chosen at a time; the best rate is marked when it is plain. Each card is a label around a
/// native radio input, so the group has one tab stop, the arrow keys move between cards and every card has a name.
#[component]
pub fn PackPicker(packs: Vec<Pack>, selected: RwSignal<Option<String>>, unit: &'static str) -> impl IntoView {
    let best = best_rate(&packs);
    let group = format!("{}-pack", unit.to_lowercase());
    view! {
        <div class="pk-grid" role="radiogroup" aria-label=format!("{unit} packs")>
            {packs.into_iter().map(|p| {
                let id = p.id.clone();
                let pick = p.id.clone();
                let on = {
                    let id = id.clone();
                    move || selected.get().as_deref() == Some(id.as_str())
                };
                let is_best = best.as_deref() == Some(p.id.as_str());
                let base = p.amount.saturating_sub(p.bonus);
                view! {
                    <label class="pk-card" class:on=on.clone()>
                        <input type="radio" class="g-sr" name=group.clone() prop:checked=on.clone() on:change=move |_| selected.set(Some(pick.clone())) />
                        {is_best.then(|| view! { <span class="g-chip live pk-badge">"Best rate"</span> })}
                        <span class="pk-name">{p.name.clone()}</span>
                        <span class="pk-amount">{format_count(p.amount)}<small>{format!(" {unit}")}</small></span>
                        {(p.bonus > 0).then(|| view! { <span class="pk-bonus">{format!("{} + {} bonus", format_count(base), format_count(p.bonus))}</span> })}
                        <span class="pk-price">{price_words(p.usd)}</span>
                        <span class="pk-rate">{rate_words(&p)}</span>
                    </label>
                }
            }).collect_view()}
        </div>
    }
}

/// The button that buys the chosen pack, or the way to sign in first. Until the account is known the button waits, so a signed-in
/// visitor is never shown "Sign in to buy".
#[component]
pub fn BuyBar(
    /// What the button says once a pack is chosen.
    label: Signal<Option<String>>,
    busy: RwSignal<bool>,
    access: Signal<Access>,
    message: RwSignal<Option<Message>>,
    on_buy: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="pk-bar">
            {move || if access.get() == Access::SignedOut {
                view! { <a class="g-btn g-btn-primary g-btn-lg" href="/login">"Sign in to buy"</a> }.into_any()
            } else {
                let text = label.get();
                let ready = text.is_some() && !busy.get() && access.get() == Access::SignedIn;
                view! {
                    <button type="button" class="g-btn g-btn-primary g-btn-lg" disabled=!ready on:click=move |_| on_buy.run(())>
                        {if busy.get() { "Opening Stripe...".to_string() } else { text.unwrap_or_else(|| "Choose a pack".to_string()) }}
                    </button>
                }.into_any()
            }}
            <Banner message=message />
            <p class="g-hint pk-note">"You pay on Stripe's own page. Eustress never sees your card."</p>
        </div>
    }
}

/// Whether a currency's packs can be bought, as far as the page can tell.
#[derive(Clone, Debug, PartialEq)]
pub enum Packs {
    Loading,
    Open(Vec<Pack>),
    /// Not on sale: no route, Stripe not set up, or nothing priced. The page says so and shows no price.
    Closed,
    Failed(String),
}

/// What the answer to "which packs are on sale" means for the page.
pub fn state_of_packs(answer: Result<Vec<Pack>, ApiError>) -> Packs {
    match answer {
        Ok(list) if list.is_empty() => Packs::Closed,
        Ok(list) => Packs::Open(list),
        Err(ApiError::NotFound) | Err(ApiError::Server { status: 503, .. }) => Packs::Closed,
        Err(e) => Packs::Failed(api::failure_sentence(&e)),
    }
}

/// The body of a "Buy" section: skeletons while the packs load, why they could not load, what to say when they are not on sale, or
/// the packs with the button that buys the chosen one.
#[component]
pub fn PackSection(
    packs: Packs,
    unit: &'static str,
    /// How many placeholders to show while the packs load.
    placeholders: usize,
    /// What the section says when the packs are not on sale.
    #[prop(into)]
    closed: ViewFn,
    selected: RwSignal<Option<String>>,
    label: Signal<Option<String>>,
    busy: RwSignal<bool>,
    access: Signal<Access>,
    message: RwSignal<Option<Message>>,
    on_buy: Callback<()>,
    /// A line under the button, when the page has one.
    #[prop(optional, into)]
    note: Option<String>,
) -> impl IntoView {
    match packs {
        Packs::Loading => view! {
            <div class="pk-grid" role="status" aria-busy="true" aria-label="Loading the packs">
                {(0..placeholders).map(|_| view! { <div class="pk-card g-skel-card"><span class="g-skel w60"></span><span class="g-skel w40 tall"></span></div> }).collect_view()}
            </div>
        }
        .into_any(),
        Packs::Failed(why) => view! { <div class="g-glass g-none" role="alert"><b>"The packs could not load"</b><p>{why}</p></div> }.into_any(),
        Packs::Closed => closed.run(),
        Packs::Open(list) => view! {
            <PackPicker packs=list selected=selected unit=unit />
            <BuyBar label=label busy=busy access=access message=message on_buy=on_buy />
            {note.map(|n| view! { <p class="g-hint dl-note">{n}</p> })}
        }
        .into_any(),
    }
}

// -----------------------------------------------------------------------------
// 4. Starting a checkout
// -----------------------------------------------------------------------------

/// A page that comes back from Stripe by the browser's Back button is often restored exactly as it was left, with its buy button still
/// reading "Opening Stripe...". When the browser shows the page from its back/forward cache (the `pageshow` event with `persisted`
/// set), the button is released. Call once, where the page sets up.
pub fn release_on_return(busy: RwSignal<bool>) {
    #[cfg(not(feature = "ssr"))]
    {
        let handle = leptos::leptos_dom::helpers::window_event_listener_untyped("pageshow", move |event| {
            let restored = js_sys::Reflect::get(&event, &wasm_bindgen::JsValue::from_str("persisted")).ok().and_then(|v| v.as_bool()).unwrap_or(false);
            if restored {
                busy.set(false);
            }
        });
        on_cleanup(move || handle.remove());
    }
    #[cfg(feature = "ssr")]
    let _ = busy;
}

/// Ask the Worker for a Stripe Checkout page and go to it. The address is used only when it is Stripe Checkout's.
pub fn begin_checkout(api_url: String, path: &'static str, body: serde_json::Value, busy: RwSignal<bool>, message: RwSignal<Option<Message>>) {
    busy.set(true);
    message.set(None);
    spawn_local(async move {
        let client = ApiClient::new(&api_url);
        match client.post::<serde_json::Value, _>(path, &body).await {
            Ok(answer) => match answer.get("url").and_then(|u| u.as_str()) {
                Some(url) if safe_checkout_url(url) => {
                    if let Some(window) = web_sys::window() {
                        let _ = window.location().set_href(url);
                    }
                }
                _ => {
                    busy.set(false);
                    message.set(Some(Message::Error("Stripe did not give a payment page. Nothing was charged.".to_string())));
                }
            },
            Err(e) => {
                busy.set(false);
                message.set(Some(Message::Error(checkout_message(&e))));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn pack(id: &str, amount: u64, bonus: u64, usd: f64) -> Pack {
        Pack { id: id.to_string(), name: id.to_string(), amount, bonus, usd }
    }

    /// A view as the page's HTML, the way the prerender writes it.
    fn html<V: RenderHtml>(build: impl FnOnce() -> V) -> String {
        let owner = Owner::new();
        owner.with(|| build().to_html())
    }

    fn delta_packs() -> Vec<Pack> {
        vec![pack("session", 1_000, 0, 10.0), pack("project", 2_600, 0, 25.0), pack("studio", 60_000, 0, 500.0)]
    }

    fn bar(access: Access, label: Option<&str>, busy: bool) -> String {
        html(|| {
            let label = Signal::derive({
                let l = label.map(str::to_string);
                move || l.clone()
            });
            let busy = RwSignal::new(busy);
            let access = Signal::derive(move || access);
            let message = RwSignal::new(None::<Message>);
            view! { <BuyBar label=label busy=busy access=access message=message on_buy=Callback::new(|_: ()| {}) /> }
        })
    }

    fn section(packs: Packs, access: Access, chosen: Option<&str>) -> String {
        html(|| {
            let selected = RwSignal::new(chosen.map(str::to_string));
            let label = Signal::derive(|| None::<String>);
            let access = Signal::derive(move || access);
            view! {
                <PackSection packs=packs unit="Delta" placeholders=4 closed=|| view! { <p class="closed-marker">"Not open yet"</p> }
                    selected=selected label=label busy=RwSignal::new(false) access=access message=RwSignal::new(None::<Message>)
                    on_buy=Callback::new(|_: ()| {}) note="A line under the button." />
            }
        })
    }

    #[test]
    fn numbers_and_prices_read_the_way_a_person_writes_them() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(60_000), "60,000");
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(price_words(25.0), "$25");
        assert_eq!(price_words(500.0), "$500");
        assert_eq!(price_words(4.99), "$4.99");
        assert_eq!(price_words(99.99), "$99.99");
    }

    #[test]
    fn the_rate_is_what_a_dollar_buys_and_the_best_one_is_named_only_when_it_is_plain() {
        let delta = vec![pack("session", 1_000, 0, 10.0), pack("project", 2_600, 0, 25.0), pack("studio", 60_000, 0, 500.0)];
        assert_eq!(rate_words(&delta[0]), "100 per $1");
        assert_eq!(rate_words(&delta[1]), "104 per $1");
        assert_eq!(rate_words(&delta[2]), "120 per $1");
        assert_eq!(best_rate(&delta), Some("studio".to_string()));
        // Equal rates have no best; one pack has nothing to be best against; a free pack is never best.
        assert_eq!(best_rate(&[pack("a", 100, 0, 1.0), pack("b", 200, 0, 2.0)]), None);
        assert_eq!(best_rate(&[pack("a", 100, 0, 1.0)]), None);
        assert_eq!(best_rate(&[pack("a", 100, 0, 0.0), pack("b", 100, 0, 0.0)]), None);
        assert_eq!(rate_words(&pack("a", 100, 0, 0.0)), "");
    }

    #[test]
    fn the_tickets_packs_rank_by_the_numbers_the_worker_sends() {
        let tickets = vec![
            pack("starter", 400, 0, 4.99),
            pack("standard", 880, 80, 9.99),
            pack("mega", 1_840, 240, 19.99),
            pack("super", 5_000, 1_000, 49.99),
            pack("ultra", 10_800, 2_800, 99.99),
        ];
        assert_eq!(best_rate(&tickets), Some("ultra".to_string()));
    }

    #[test]
    fn a_delta_pack_is_named_from_its_lookup_key() {
        assert_eq!(delta_pack_name("eustress_delta_week"), "Week");
        assert_eq!(delta_pack_name("eustress_delta_studio"), "Studio");
        assert_eq!(delta_pack_name("solo"), "Solo");
        assert_eq!(delta_pack_name(""), "");
    }

    #[test]
    fn the_button_names_the_amount_and_the_price() {
        assert_eq!(buy_label(&pack("w", 5_400, 0, 50.0), "Delta"), "Buy 5,400 Delta for $50");
        assert_eq!(buy_label(&pack("s", 880, 80, 9.99), "Tickets"), "Buy 880 Tickets for $9.99");
    }

    #[test]
    fn a_payment_goes_only_to_stripe_checkout_over_https() {
        assert!(safe_checkout_url("https://checkout.stripe.com/c/pay/cs_test_a1B2"));
        for bad in [
            "",
            "http://checkout.stripe.com/c/pay/x",
            "https://checkout.stripe.com",
            "https://checkout.stripe.com/",
            "https://checkout.stripe.com.evil.example/c/pay/x",
            "https://evil.example/https://checkout.stripe.com/c",
            "https://stripe.com/c/pay/x",
            "javascript:alert(1)",
            "https://checkout.stripe.com/c/pay/x y",
            "https://checkout.stripe.com/\\evil.example",
            "//checkout.stripe.com/c/pay/x",
        ] {
            assert!(!safe_checkout_url(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_purchase_that_cannot_start_says_why_and_that_nothing_was_charged() {
        let all = [
            checkout_message(&ApiError::Unauthorized),
            checkout_message(&ApiError::NotFound),
            checkout_message(&ApiError::Server { status: 503, message: String::new() }),
            checkout_message(&ApiError::Server { status: 502, message: String::new() }),
            checkout_message(&ApiError::Server { status: 400, message: "{\"error\":\"Invalid package\",\"code\":\"invalid_request\"}".to_string() }),
            checkout_message(&ApiError::Network("offline".to_string())),
        ];
        for m in &all {
            assert!(m.ends_with("Nothing was charged."), "{m}");
            assert!(!m.contains(".."), "{m}");
        }
        assert!(all[0].contains("Sign in"));
        assert!(all[2].contains("not open yet"));
        assert!(all[4].starts_with("Invalid package."), "{}", all[4]);
    }

    #[test]
    fn the_workers_error_and_code_are_read_from_its_json_and_nothing_else_is_trusted() {
        assert_eq!(worker_words("{\"error\":\"Nope\",\"code\":\"x\"}"), (Some("Nope".to_string()), Some("x".to_string())));
        assert_eq!(worker_words("not json"), (None, None));
        assert_eq!(worker_words("{\"error\":5}"), (None, None));
    }

    #[test]
    fn a_balance_is_read_for_about_a_minute_while_a_payment_settles() {
        let seconds = BALANCE_REFRESH_EVERY_MS as u64 * (BALANCE_REFRESHES as u64 - 1) / 1_000;
        assert!((45..=75).contains(&seconds), "{seconds}s");
    }

    #[test]
    fn only_a_tickets_purchase_that_went_through_keeps_the_balance_reading() {
        assert!(paid_return("?purchased=true"));
        for no in ["?purchased=false", "", "?funded=true", "?cancelled=1", "?purchased=truee"] {
            assert!(!paid_return(no), "{no:?}");
        }
    }

    #[test]
    fn the_address_says_what_just_happened_to_a_payment() {
        assert!(matches!(return_banner("?purchased=true"), Some(Message::Info(t)) if t.contains("went through")));
        assert!(matches!(return_banner("?purchased=false"), Some(Message::Info(t)) if t.contains("cancelled")));
        assert!(matches!(return_banner("?cancelled=1"), Some(Message::Info(t)) if t.contains("cancelled")));
        assert!(matches!(return_banner("?funded=true"), Some(Message::Info(t)) if t.contains("treasury")));
        assert!(matches!(return_banner("?funded=false"), Some(Message::Info(t)) if t.contains("cancelled")));
        assert_eq!(return_banner("?purchased=maybe"), None);
        assert_eq!(return_banner("?funded=1"), None);
        assert_eq!(return_banner(""), None);
    }

    #[test]
    fn the_buy_button_waits_for_the_account_and_sends_a_stranger_to_sign_in() {
        let user: crate::state::User = serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000001","username":"a","created_at":"2026-01-01T00:00:00Z"}"#).unwrap();
        assert_eq!(access_of(&AuthState::Unknown), Access::Checking);
        assert_eq!(access_of(&AuthState::Unauthenticated), Access::SignedOut);
        assert_eq!(access_of(&AuthState::Authenticated(user)), Access::SignedIn);
    }

    #[test]
    fn the_cards_show_each_packs_amount_price_and_rate_and_mark_only_the_chosen_one() {
        let page = html(|| {
            let selected = RwSignal::new(Some("project".to_string()));
            view! { <PackPicker packs=delta_packs() selected=selected unit="Delta" /> }
        });
        assert_eq!(page.matches(r#"class="pk-card on""#).count(), 1, "{page}");
        assert_eq!(page.matches(r#"class="pk-card""#).count(), 2, "{page}");
        assert_eq!(page.matches(r#"type="radio""#).count(), 3, "one native radio per pack: {page}");
        assert_eq!(page.matches(r#"name="delta-pack""#).count(), 3, "all in one group: {page}");
        for want in ["1,000", "2,600", "60,000", "$10", "$25", "$500", "100 per $1", "104 per $1", "120 per $1", "Delta packs"] {
            assert!(page.contains(want), "{want}: {page}");
        }
        assert_eq!(page.matches("Best rate").count(), 1, "only the biggest pack has the best rate: {page}");
        let none = html(|| view! { <PackPicker packs=delta_packs() selected=RwSignal::new(None::<String>) unit="Delta" /> });
        assert_eq!(none.matches(r#"class="pk-card on""#).count(), 0);
    }

    #[test]
    fn a_bonus_is_shown_inside_the_total_and_only_for_a_pack_that_has_one() {
        let page = html(|| view! { <PackPicker packs=vec![pack("a", 400, 0, 4.99), pack("b", 880, 80, 9.99)] selected=RwSignal::new(None::<String>) unit="Tickets" /> });
        assert_eq!(page.matches(r#"class="pk-bonus""#).count(), 1, "{page}");
        assert!(page.contains("800 + 80 bonus") && page.contains("880") && page.contains("$9.99"), "{page}");
    }

    #[test]
    fn the_figures_strip_shows_its_labels_and_values_and_is_not_drawn_when_empty() {
        let full = html(|| view! { <Figures items=vec![("Your balance", "1,240 Tickets".to_string()), ("Lasts", "12 months".to_string())] /> });
        assert!(full.contains("Your balance") && full.contains("1,240 Tickets") && full.contains("12 months") && full.contains("cur-figures"), "{full}");
        let empty = html(|| view! { <Figures items=vec![] /> });
        assert!(!empty.contains("cur-figures") && !empty.contains("<dl"), "{empty}");
    }

    #[test]
    fn a_message_is_announced_as_a_status_and_an_error_as_an_alert() {
        let info = html(|| view! { <Banner message=RwSignal::new(Some(Message::Info("Thank you.".to_string()))) /> });
        assert!(info.contains(r#"role="status""#) && info.contains("Thank you.") && !info.contains("bad"), "{info}");
        let bad = html(|| view! { <Banner message=RwSignal::new(Some(Message::Error("Nothing was charged.".to_string()))) /> });
        assert!(bad.contains(r#"role="alert""#) && bad.contains("cur-banner bad"), "{bad}");
        let none = html(|| view! { <Banner message=RwSignal::new(None::<Message>) /> });
        assert!(!none.contains("cur-banner"), "{none}");
    }

    #[test]
    fn a_stranger_is_sent_to_sign_in_and_the_account_is_waited_for() {
        let out = bar(Access::SignedOut, Some("Buy 1,000 Delta for $10"), false);
        assert!(out.contains(r#"href="/login""#) && out.contains("Sign in to buy") && !out.contains("<button"), "{out}");
        let waiting = bar(Access::Checking, Some("Buy 1,000 Delta for $10"), false);
        assert!(waiting.contains("<button") && waiting.contains("disabled") && !waiting.contains("Sign in to buy"), "{waiting}");
    }

    #[test]
    fn a_signed_in_buyer_can_buy_only_once_a_pack_is_chosen_and_not_twice() {
        let ready = bar(Access::SignedIn, Some("Buy 1,000 Delta for $10"), false);
        assert!(ready.contains("Buy 1,000 Delta for $10") && !ready.contains("disabled"), "{ready}");
        let none = bar(Access::SignedIn, None, false);
        assert!(none.contains("Choose a pack") && none.contains("disabled"), "{none}");
        let busy = bar(Access::SignedIn, Some("Buy 1,000 Delta for $10"), true);
        assert!(busy.contains("Opening Stripe...") && busy.contains("disabled") && !busy.contains("Buy 1,000"), "{busy}");
        assert!(ready.contains("never sees your card"), "{ready}");
    }

    #[test]
    fn the_section_shows_what_the_packs_are_doing_and_never_a_price_it_cannot_sell_at() {
        let loading = section(Packs::Loading, Access::SignedIn, None);
        assert!(loading.contains(r#"aria-busy="true""#) && loading.matches("g-skel-card").count() == 4 && !loading.contains('$'), "{loading}");
        let failed = section(Packs::Failed("The server answered 500. Try again in a minute.".to_string()), Access::SignedIn, None);
        assert!(failed.contains(r#"role="alert""#) && failed.contains("could not load") && failed.contains("answered 500") && !failed.contains('$'), "{failed}");
        let closed = section(Packs::Closed, Access::SignedIn, None);
        assert!(closed.contains("closed-marker") && !closed.contains('$') && !closed.contains("pk-card") && !closed.contains("<button"), "{closed}");
        let open = section(Packs::Open(delta_packs()), Access::SignedIn, Some("studio"));
        assert!(open.contains("$500") && open.contains("pk-bar") && open.contains("A line under the button.") && !open.contains("closed-marker"), "{open}");
        assert_eq!(open.matches(r#"class="pk-card on""#).count(), 1, "{open}");
    }

    #[test]
    fn what_the_workers_answer_means_for_the_packs() {
        let one = vec![pack("x", 1, 0, 1.0)];
        assert_eq!(state_of_packs(Ok(one.clone())), Packs::Open(one));
        assert_eq!(state_of_packs(Ok(vec![])), Packs::Closed, "no price set up means no pack");
        assert_eq!(state_of_packs(Err(ApiError::NotFound)), Packs::Closed, "a Worker without the route");
        assert_eq!(state_of_packs(Err(ApiError::Server { status: 503, message: String::new() })), Packs::Closed, "Stripe not configured");
        assert!(matches!(state_of_packs(Err(ApiError::Network("offline".to_string()))), Packs::Failed(_)));
        assert!(matches!(state_of_packs(Err(ApiError::Server { status: 502, message: "x".to_string() })), Packs::Failed(_)));
    }

    #[test]
    fn the_three_currencies_have_their_own_pages() {
        assert_eq!(Currency::ALL.map(|c| c.path()), ["/tickets", "/bliss", "/delta"]);
        assert_eq!(Currency::ALL.map(|c| c.name()), ["Tickets", "Bliss", "Delta"]);
    }
}
