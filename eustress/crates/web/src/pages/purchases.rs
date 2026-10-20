// =============================================================================
// Eustress Web - Purchases
// =============================================================================
// Everything the signed-in account has bought with Tickets or Bliss. Each
// purchase shows its title, icon and amount, and the same spend is totalled
// per creator, with a search, and per simulation. Choosing a creator or a
// simulation narrows the purchase list to it.
//
// Reads /api/purchases, which answers only for the bearer's own account, so
// there is no way to open this page onto someone else's spending.
//
// Table of Contents:
// 1. Formatting
// 2. Page
// 3. Pieces
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    get_purchases, ApiClient, ApiError, CreatorSpend, Purchase, PurchaseHistory, SimulationSpend,
};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::utils::{format_bliss_exact, format_count};

/// Purchases rendered before "Show more". A long history renders in steps so
/// the first paint does not build thousands of rows.
const PAGE_ROWS: usize = 100;

// -----------------------------------------------------------------------------
// 1. Formatting
// -----------------------------------------------------------------------------

/// Local date for the list, and the full date and time for its tooltip.
fn when(ts: &str) -> (String, String) {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(t) => {
            let local = t.with_timezone(&chrono::Local);
            (
                local.format("%b %-d, %Y").to_string(),
                local.format("%B %-d, %Y at %H:%M").to_string(),
            )
        }
        Err(_) => (ts.to_string(), ts.to_string()),
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", format_count(n), if n == 1 { one } else { many })
}

fn creator_label(c: &CreatorSpend) -> String {
    match &c.creator_id {
        Some(_) if !c.creator_name.is_empty() => c.creator_name.clone(),
        Some(_) => "Unnamed creator".to_string(),
        None => "No creator".to_string(),
    }
}

fn simulation_label(s: &SimulationSpend) -> String {
    match &s.simulation_id {
        Some(_) if !s.simulation_name.is_empty() => s.simulation_name.clone(),
        Some(_) => "Untitled simulation".to_string(),
        None => "Outside a simulation".to_string(),
    }
}

fn thumbnail_url(api_url: &str, simulation_id: &str) -> String {
    format!("{api_url}/api/simulations/{simulation_id}/thumbnail")
}

/// What the purchase list is narrowed to. A `None` id is the bucket with no
/// creator, or the one outside any simulation. The label is what the list
/// header says while the narrowing is on.
#[derive(Clone, Debug, PartialEq)]
enum Focus {
    Creator(Option<String>, String),
    Simulation(Option<String>, String),
}

impl Focus {
    fn matches(&self, p: &Purchase) -> bool {
        match self {
            Focus::Creator(id, _) => &p.creator_id == id,
            Focus::Simulation(id, _) => &p.simulation_id == id,
        }
    }

    fn label(&self) -> &str {
        match self {
            Focus::Creator(_, label) | Focus::Simulation(_, label) => label,
        }
    }
}

/// Where the account's session stands. A memo over this, rather than the auth
/// signal itself, is what the fetch follows: the app re-sets `auth` every
/// minute to refresh balances, and each of those would otherwise reload the
/// whole purchase history.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Session {
    Unknown,
    SignedOut,
    SignedIn(uuid::Uuid),
}

// -----------------------------------------------------------------------------
// 2. Page
// -----------------------------------------------------------------------------

/// Purchases: every Tickets and Bliss spend of the signed-in account.
#[component]
pub fn PurchasesPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = app_state.api_url.clone();

    let history = RwSignal::new(Option::<PurchaseHistory>::None);
    let error = RwSignal::new(Option::<String>::None);
    let creator_query = RwSignal::new(String::new());
    let focus = RwSignal::new(Option::<Focus>::None);
    let shown = RwSignal::new(PAGE_ROWS);

    let session = Memo::new(move |_| match auth.get() {
        AuthState::Authenticated(user) => Session::SignedIn(user.id),
        AuthState::Unauthenticated => Session::SignedOut,
        AuthState::Unknown => Session::Unknown,
    });

    // `auth` starts Unknown while the session is restored from localStorage,
    // so the fetch waits for a signed-in account instead of going out bare.
    {
        let api_url = api_url.clone();
        Effect::new(move |_| {
            if let Session::SignedIn(_) = session.get() {
                let api_url = api_url.clone();
                spawn_local(async move {
                    match get_purchases(&ApiClient::new(api_url)).await {
                        Ok(h) => {
                            history.set(Some(h));
                            error.set(None);
                        }
                        Err(ApiError::Unauthorized) => error.set(Some(
                            "Your session has ended. Sign in again to see your purchases.".to_string(),
                        )),
                        Err(e) => error.set(Some(format!("Your purchases could not be loaded. {e}"))),
                    }
                });
            }
        });
    }

    // How many purchases the current narrowing leaves, for the list header
    // and the "Show more" button.
    let visible_count = Memo::new(move |_| {
        let f = focus.get();
        history.with(|h| {
            h.as_ref().map_or(0, |h| {
                h.purchases.iter().filter(|p| f.as_ref().map_or(true, |f| f.matches(p))).count()
            })
        })
    });

    view! {
        <div class="page page-purchases">
            <CentralNav active="".to_string() />

            <div class="purchases-wrap">
                <header class="purchases-head">
                    <div>
                        <h1>"Purchases"</h1>
                        <p class="purchases-sub">
                            "Everything you have bought with Tickets and Bliss, and who it supported. "
                            "Only you can see this page."
                        </p>
                    </div>
                    <a href="/bliss/history" class="purchases-back">"Wallet ledger"</a>
                </header>

                {move || {
                    if session.get() == Session::SignedOut {
                        return view! {
                            <div class="purchases-card purchases-center">
                                <p>"Sign in to see your purchases."</p>
                                <a href="/login" class="btn btn-primary">"Sign in"</a>
                            </div>
                        }.into_any();
                    }
                    if let Some(msg) = error.get() {
                        return view! {
                            <div class="purchases-card purchases-error" role="alert">
                                <p>{msg}</p>
                            </div>
                        }.into_any();
                    }
                    let Some((totals, truncated)) =
                        history.with(|h| h.as_ref().map(|h| (h.totals.clone(), h.truncated)))
                    else {
                        return view! {
                            <div class="purchases-card">
                                <p class="purchases-hint">"Loading your purchases…"</p>
                            </div>
                        }.into_any();
                    };

                    let hero = view! {
                        <section class="purchases-hero" aria-label="Totals">
                            <div class="purchases-totals">
                                <div class="purchases-total">
                                    <span class="purchases-label">"Tickets spent"</span>
                                    <span class="purchases-total-value">
                                        {format_count(totals.tickets)}
                                        <span class="purchases-unit">"TKT"</span>
                                    </span>
                                </div>
                                <div class="purchases-total purchases-total-bls">
                                    <span class="purchases-label">"Bliss spent"</span>
                                    <span class="purchases-total-value">
                                        {format_bliss_exact(totals.bliss)}
                                        <span class="purchases-unit">"BLS"</span>
                                    </span>
                                </div>
                            </div>
                            <div class="purchases-stats">
                                <div class="purchases-stat">
                                    <span class="purchases-label">"Purchases"</span>
                                    <span class="purchases-stat-value">{format_count(totals.purchases)}</span>
                                </div>
                                <div class="purchases-stat">
                                    <span class="purchases-label">"Creators supported"</span>
                                    <span class="purchases-stat-value">{format_count(totals.creators)}</span>
                                </div>
                                <div class="purchases-stat">
                                    <span class="purchases-label">"Simulations"</span>
                                    <span class="purchases-stat-value">{format_count(totals.simulations)}</span>
                                </div>
                            </div>
                        </section>
                    };

                    if totals.purchases == 0 {
                        return view! {
                            {hero}
                            <div class="purchases-card purchases-empty">
                                <img src="/assets/icons/receipt.svg" alt="" class="purchases-empty-icon" />
                                <h2>"No purchases yet"</h2>
                                <p>
                                    "When you buy something inside a simulation with Tickets or Bliss, it "
                                    "shows up here with the creator and the simulation it supported."
                                </p>
                                <a href="/tickets" class="purchases-back">"Get Tickets"</a>
                            </div>
                        }.into_any();
                    }

                    let api_for_sims = api_url.clone();
                    let api_for_rows = api_url.clone();
                    view! {
                        {hero}
                        {truncated.then(|| view! {
                            <p class="purchases-note">
                                "This shows your most recent 50,000 purchases; the totals cover those."
                            </p>
                        })}

                        <div class="purchases-grid">
                            <section class="purchases-card" aria-labelledby="purchases-creators-h">
                                <div class="purchases-card-head">
                                    <h2 class="purchases-h2" id="purchases-creators-h">"Spend by creator"</h2>
                                    <span class="purchases-count">{plural(totals.creators, "creator", "creators")}</span>
                                </div>
                                <label class="purchases-search">
                                    <img src="/assets/icons/search.svg" alt="" />
                                    <input
                                        type="search"
                                        placeholder="Search creators"
                                        aria-label="Search creators"
                                        autocomplete="off"
                                        prop:value=move || creator_query.get()
                                        on:input=move |ev| creator_query.set(event_target_value(&ev))
                                    />
                                </label>
                                <ul class="purchases-groups">
                                    {move || {
                                        let query = creator_query.get();
                                        let needle = query.trim().to_lowercase();
                                        let rows: Vec<CreatorSpend> = history.with(|h| {
                                            h.as_ref().map(|h| {
                                                h.by_creator
                                                    .iter()
                                                    .filter(|c| needle.is_empty() || creator_label(c).to_lowercase().contains(&needle))
                                                    .cloned()
                                                    .collect()
                                            })
                                            .unwrap_or_default()
                                        });
                                        if rows.is_empty() {
                                            return view! {
                                                <li class="purchases-none">
                                                    {format!("No creator matches \u{201c}{}\u{201d}.", query.trim())}
                                                </li>
                                            }.into_any();
                                        }
                                        rows.into_iter()
                                            .map(|c| view! { <CreatorRow spend=c focus=focus shown=shown /> })
                                            .collect_view()
                                            .into_any()
                                    }}
                                </ul>
                            </section>

                            <section class="purchases-card" aria-labelledby="purchases-sims-h">
                                <div class="purchases-card-head">
                                    <h2 class="purchases-h2" id="purchases-sims-h">"Spend by simulation"</h2>
                                    <span class="purchases-count">{plural(totals.simulations, "simulation", "simulations")}</span>
                                </div>
                                <ul class="purchases-groups">
                                    {move || {
                                        let rows: Vec<SimulationSpend> = history.with(|h| {
                                            h.as_ref().map(|h| h.by_simulation.clone()).unwrap_or_default()
                                        });
                                        rows.into_iter()
                                            .map(|s| view! {
                                                <SimulationRow spend=s api_url=api_for_sims.clone() focus=focus shown=shown />
                                            })
                                            .collect_view()
                                    }}
                                </ul>
                            </section>
                        </div>

                        <section class="purchases-card" aria-labelledby="purchases-list-h">
                            <div class="purchases-card-head">
                                <h2 class="purchases-h2" id="purchases-list-h">"Every purchase"</h2>
                                <span class="purchases-count">
                                    {move || plural(visible_count.get() as u64, "purchase", "purchases")}
                                </span>
                            </div>
                            {move || focus.get().map(|f| view! {
                                <div class="purchases-focus">
                                    <span>{f.label().to_string()}</span>
                                    <button
                                        type="button"
                                        class="purchases-focus-clear"
                                        on:click=move |_| {
                                            focus.set(None);
                                            shown.set(PAGE_ROWS);
                                        }
                                    >
                                        "Show all"
                                    </button>
                                </div>
                            })}
                            <ul class="purchases-list">
                                {move || {
                                    let f = focus.get();
                                    let limit = shown.get();
                                    let rows: Vec<Purchase> = history.with(|h| {
                                        h.as_ref().map(|h| {
                                            h.purchases
                                                .iter()
                                                .filter(|p| f.as_ref().map_or(true, |f| f.matches(p)))
                                                .take(limit)
                                                .cloned()
                                                .collect()
                                        })
                                        .unwrap_or_default()
                                    });
                                    rows.into_iter()
                                        .map(|p| view! { <PurchaseRow purchase=p api_url=api_for_rows.clone() /> })
                                        .collect_view()
                                }}
                            </ul>
                            {move || {
                                let left = visible_count.get().saturating_sub(shown.get());
                                (left > 0).then(|| view! {
                                    <button
                                        type="button"
                                        class="purchases-more"
                                        on:click=move |_| shown.update(|n| *n += PAGE_ROWS)
                                    >
                                        {format!("Show more ({} left)", format_count(left as u64))}
                                    </button>
                                })
                            }}
                        </section>
                    }.into_any()
                }}
            </div>

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. Pieces
// -----------------------------------------------------------------------------

/// Tickets and Bliss side by side, each only when there is some. They are
/// separate currencies with no fixed rate, so they are never added up.
#[component]
fn Amounts(tickets: u64, bliss: f64) -> impl IntoView {
    let has_tickets = tickets > 0;
    let has_bliss = bliss > 0.0;
    view! {
        <span class="purchases-amounts">
            {has_tickets.then(|| view! {
                <span class="purchases-amount purchases-amount-tkt">
                    {format_count(tickets)}<span class="purchases-unit">"TKT"</span>
                </span>
            })}
            {has_bliss.then(|| view! {
                <span class="purchases-amount purchases-amount-bls">
                    {format_bliss_exact(bliss)}<span class="purchases-unit">"BLS"</span>
                </span>
            })}
            {(!has_tickets && !has_bliss).then(|| view! { <span class="purchases-amount">"0"</span> })}
        </span>
    }
}

/// A picture that falls through its sources: each is tried only when the one
/// before it fails to load, and the last resort is a local mark that always
/// exists, so a missing thumbnail never leaves a broken image.
#[component]
fn FallbackIcon(sources: Vec<String>, fallback: &'static str) -> impl IntoView {
    let step = RwSignal::new(0usize);
    view! {
        <span class="purchases-icon">
            {move || {
                let i = step.get();
                match sources.get(i).cloned() {
                    Some(src) => view! {
                        <img src=src alt="" loading="lazy" decoding="async" on:error=move |_| step.set(i + 1) />
                    }.into_any(),
                    None => view! { <img src=fallback alt="" class="purchases-icon-mark" /> }.into_any(),
                }
            }}
        </span>
    }
}

/// Toggle a narrowing on or off, and start the list from the top.
fn toggle(focus: RwSignal<Option<Focus>>, shown: RwSignal<usize>, target: Focus) {
    focus.update(|f| *f = if f.as_ref() == Some(&target) { None } else { Some(target) });
    shown.set(PAGE_ROWS);
}

#[component]
fn CreatorRow(
    spend: CreatorSpend,
    focus: RwSignal<Option<Focus>>,
    shown: RwSignal<usize>,
) -> impl IntoView {
    let label = creator_label(&spend);
    let initial = label.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let target = Focus::Creator(
        spend.creator_id.clone(),
        match &spend.creator_id {
            Some(_) => format!("Purchases from {label}"),
            None => "Purchases with no creator".to_string(),
        },
    );
    let active = {
        let target = target.clone();
        move || focus.with(|f| f.as_ref() == Some(&target))
    };
    let is_none = spend.creator_id.is_none();
    view! {
        <li>
            <button
                type="button"
                class="purchases-group"
                class:is-active=active.clone()
                aria-pressed=move || active().to_string()
                on:click=move |_| toggle(focus, shown, target.clone())
            >
                // The bucket with no creator gets a mark, not an initial: an
                // "N" for "No creator" reads as somebody's name.
                <span class="purchases-avatar" class:is-none=is_none>
                    {if is_none {
                        view! { <img src="/assets/icons/tag.svg" alt="" /> }.into_any()
                    } else {
                        initial.into_any()
                    }}
                </span>
                <span class="purchases-group-text">
                    <span class="purchases-group-name">{label}</span>
                    <span class="purchases-group-meta">{plural(spend.purchases, "purchase", "purchases")}</span>
                </span>
                <Amounts tickets=spend.tickets bliss=spend.bliss />
            </button>
        </li>
    }
}

#[component]
fn SimulationRow(
    spend: SimulationSpend,
    api_url: String,
    focus: RwSignal<Option<Focus>>,
    shown: RwSignal<usize>,
) -> impl IntoView {
    let label = simulation_label(&spend);
    let target = Focus::Simulation(
        spend.simulation_id.clone(),
        match &spend.simulation_id {
            Some(_) => format!("Purchases in {label}"),
            None => "Purchases outside a simulation".to_string(),
        },
    );
    let active = {
        let target = target.clone();
        move || focus.with(|f| f.as_ref() == Some(&target))
    };
    let sources: Vec<String> = spend.simulation_id.iter().map(|id| thumbnail_url(&api_url, id)).collect();
    let fallback = if spend.simulation_id.is_some() { "/assets/icons/gamepad.svg" } else { "/assets/icons/receipt.svg" };
    let meta = match (&spend.simulation_id, spend.creator_name.is_empty()) {
        (Some(_), false) => format!("by {} \u{00b7} {}", spend.creator_name, plural(spend.purchases, "purchase", "purchases")),
        _ => plural(spend.purchases, "purchase", "purchases"),
    };
    view! {
        <li>
            <button
                type="button"
                class="purchases-group"
                class:is-active=active.clone()
                aria-pressed=move || active().to_string()
                on:click=move |_| toggle(focus, shown, target.clone())
            >
                <FallbackIcon sources=sources fallback=fallback />
                <span class="purchases-group-text">
                    <span class="purchases-group-name">{label}</span>
                    <span class="purchases-group-meta">{meta}</span>
                </span>
                <Amounts tickets=spend.tickets bliss=spend.bliss />
            </button>
        </li>
    }
}

#[component]
fn PurchaseRow(purchase: Purchase, api_url: String) -> impl IntoView {
    let (day, full) = when(&purchase.ts);

    // The purchase's own icon first, then its simulation's thumbnail, then
    // the mark of the currency it was paid in.
    let mut sources = Vec::new();
    if !purchase.icon.is_empty() {
        sources.push(purchase.icon.clone());
    }
    if let Some(id) = &purchase.simulation_id {
        sources.push(thumbnail_url(&api_url, id));
    }
    let fallback = if purchase.is_bliss() { "/assets/icons/bliss.svg" } else { "/assets/icons/ticket.svg" };

    let title = if purchase.title.is_empty() { "Untitled purchase".to_string() } else { purchase.title.clone() };
    let (tickets, bliss) = if purchase.is_bliss() {
        (0, purchase.amount)
    } else {
        (purchase.amount.round().max(0.0) as u64, 0.0)
    };

    let simulation = purchase.simulation_id.clone().map(|id| {
        let name = if purchase.simulation_name.is_empty() {
            "Untitled simulation".to_string()
        } else {
            purchase.simulation_name.clone()
        };
        view! { <a href=format!("/simulation/{id}") class="purchases-link">{name}</a> }
    });
    let creator = (purchase.creator_id.is_some() && !purchase.creator_name.is_empty()).then(|| {
        let name = purchase.creator_name.clone();
        view! {
            <span>
                "by "
                <a href=format!("/profile/{}", urlencoding::encode(&name)) class="purchases-link">{name.clone()}</a>
            </span>
        }
    });
    let separator = (simulation.is_some() && creator.is_some())
        .then(|| view! { <span class="purchases-dot" aria-hidden="true">"\u{00b7}"</span> });
    let neither = (simulation.is_none() && creator.is_none())
        .then(|| view! { <span>"No creator"</span> });

    view! {
        <li class="purchases-item">
            <FallbackIcon sources=sources fallback=fallback />
            <div class="purchases-item-main">
                <span class="purchases-item-title">{title}</span>
                <span class="purchases-item-sub">{simulation}{separator}{creator}{neither}</span>
            </div>
            <div class="purchases-item-side">
                <Amounts tickets=tickets bliss=bliss />
                <time class="purchases-item-date" datetime=purchase.ts.clone() title=full>{day}</time>
            </div>
        </li>
    }
}
