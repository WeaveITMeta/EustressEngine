// =============================================================================
// Eustress Web - Creator
// =============================================================================
// What a creator sells in their published simulations: the products, the
// sales they made, what happened to them (events), and where the API posts
// those events (webhooks). The same data `eustress commerce` works on, in the
// same two modes: test shapes drafts and moves nothing; live is what players
// see and buy.
//
// /creator            every simulation the account published, and whether it sells
// /creator/:sim_id    one simulation's products, sales, events and webhooks
//
// The API answers only for the signed-in account's own simulations, so this
// page cannot be opened onto someone else's sales.
//
// Table of Contents:
// 1. Formatting
// 2. Page
// 3. One simulation
// 4. Products
// 5. Sales
// 6. Events
// 7. Webhooks
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde_json::{json, Value};

use crate::api::{
    commerce_error_text, create_product, create_webhook, delete_webhook, get_commerce_account, list_events,
    list_products, list_sales, list_webhooks, refund_sale, update_product, CommerceAccount, CommerceEvent,
    CommerceMode, CommerceProduct, CommerceSale, SellerSimulation, WebhookEndpoint,
};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::utils::format_count;

// -----------------------------------------------------------------------------
// 1. Formatting
// -----------------------------------------------------------------------------

/// A Worker timestamp (seconds) in local time.
fn when(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %Y %H:%M").to_string())
        .unwrap_or_default()
}

fn tickets(n: u64) -> String {
    format!("{} TKT", format_count(n))
}

fn kind_label(kind: &str) -> &'static str {
    if kind == "pass" {
        "Pass"
    } else {
        "Product"
    }
}

/// The browser's own yes/no box, for changes that cannot be undone.
fn confirmed(text: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(text).ok()).unwrap_or(false)
}

/// Where the account's session stands. The page follows this memo rather
/// than the auth signal, which the app re-sets every minute to refresh
/// balances and would otherwise reload everything.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Session {
    Unknown,
    SignedOut,
    SignedIn(uuid::Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Products,
    Sales,
    Events,
    Webhooks,
}

// -----------------------------------------------------------------------------
// 2. Page
// -----------------------------------------------------------------------------

/// The creator dashboard: every simulation, or one simulation's commerce.
#[component]
pub fn CreatorPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let api_url = app_state.api_url.clone();
    let params = use_params_map();
    let sim_id = Memo::new(move |_| params.read().get("sim_id").filter(|id| !id.is_empty()));

    let account = RwSignal::new(Option::<CommerceAccount>::None);
    let error = RwSignal::new(Option::<String>::None);
    let session = Memo::new(move |_| match auth.get() {
        AuthState::Authenticated(user) => Session::SignedIn(user.id),
        AuthState::Unauthenticated => Session::SignedOut,
        AuthState::Unknown => Session::Unknown,
    });

    {
        let api_url = api_url.clone();
        Effect::new(move |_| {
            if let Session::SignedIn(_) = session.get() {
                let api_url = api_url.clone();
                spawn_local(async move {
                    // The simulation list is the same in both modes.
                    match get_commerce_account(&api_url, CommerceMode::Test).await {
                        Ok(a) => {
                            account.set(Some(a));
                            error.set(None);
                        }
                        Err(e) => error.set(Some(commerce_error_text(&e))),
                    }
                });
            }
        });
    }

    view! {
        <div class="page page-creator">
            <CentralNav active="".to_string() />
            <div class="creator-wrap">
                {move || {
                    if session.get() == Session::SignedOut {
                        return view! {
                            <div class="creator-card creator-center">
                                <p>"Sign in to sell in your simulations."</p>
                                <a href="/login" class="btn btn-primary">"Sign in"</a>
                            </div>
                        }.into_any();
                    }
                    if let Some(msg) = error.get() {
                        return view! {
                            <div class="creator-card creator-error" role="alert"><p>{msg}</p></div>
                        }.into_any();
                    }
                    let Some(acct) = account.get() else {
                        return view! {
                            <div class="creator-card"><p class="creator-hint">"Loading your simulations…"</p></div>
                        }.into_any();
                    };
                    match sim_id.get() {
                        None => view! { <SimulationList account=acct /> }.into_any(),
                        Some(id) => match acct.simulations.iter().find(|s| s.id == id).cloned() {
                            Some(sim) => view! { <SimulationCommerce sim=sim api_url=api_url.clone() /> }.into_any(),
                            None => view! {
                                <div class="creator-card creator-center">
                                    <p>"This is not one of your published simulations."</p>
                                    <a href="/creator" class="creator-link">"Your simulations"</a>
                                </div>
                            }.into_any(),
                        },
                    }
                }}
            </div>
            <Footer />
        </div>
    }
}

#[component]
fn SimulationList(account: CommerceAccount) -> impl IntoView {
    let empty = account.simulations.is_empty();
    view! {
        <header class="creator-head">
            <div>
                <h1>"Creator"</h1>
                <p class="creator-sub">
                    "Sell products and passes in the simulations you published: create them, put them on "
                    "sale, and follow every sale. The same data "
                    <code>"eustress commerce"</code>
                    " works on. Only you can see this page."
                </p>
            </div>
            <a href="/docs/earning" class="creator-pill">"How selling works"</a>
        </header>
        {if empty {
            view! {
                <div class="creator-card creator-center">
                    <h2>"No published simulations yet"</h2>
                    <p>
                        "Products belong to a published simulation. Publish a Universe from Studio, then "
                        "come back here to sell in it."
                    </p>
                </div>
            }.into_any()
        } else {
            view! {
                <ul class="creator-sims">
                    {account.simulations.into_iter().map(|s| {
                        let href = format!("/creator/{}", s.id);
                        let products = s.products.map(|n| {
                            format!("{} {}", format_count(n), if n == 1 { "product" } else { "products" })
                        });
                        let status = if s.can_sell {
                            ("creator-chip is-ok", "Sells to players".to_string())
                        } else {
                            ("creator-chip is-held", s.reason.clone().unwrap_or_else(|| "Cannot sell yet".to_string()))
                        };
                        view! {
                            <li>
                                <a href=href class="creator-sim">
                                    <span class="creator-sim-name">
                                        {if s.name.is_empty() { "Untitled simulation".to_string() } else { s.name.clone() }}
                                    </span>
                                    <span class="creator-sim-meta">
                                        {products.unwrap_or_default()}
                                        {(!s.listed).then(|| " \u{00b7} not listed in the gallery")}
                                    </span>
                                    <span class=status.0>{status.1}</span>
                                </a>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any()
        }}
    }
}

// -----------------------------------------------------------------------------
// 3. One simulation
// -----------------------------------------------------------------------------

#[component]
fn SimulationCommerce(sim: SellerSimulation, api_url: String) -> impl IntoView {
    let mode = RwSignal::new(CommerceMode::Test);
    let tab = RwSignal::new(Tab::Products);
    let name = if sim.name.is_empty() { "Untitled simulation".to_string() } else { sim.name.clone() };
    let status = if sim.can_sell {
        "Players can buy what you put on sale here.".to_string()
    } else {
        format!("Players cannot buy here yet: {}", sim.reason.clone().unwrap_or_else(|| "it is not sellable".to_string()))
    };
    let tab_button = move |t: Tab, label: &'static str| {
        view! {
            <button
                type="button"
                class="creator-tab"
                class:is-active=move || tab.get() == t
                aria-pressed=move || (tab.get() == t).to_string()
                on:click=move |_| tab.set(t)
            >
                {label}
            </button>
        }
    };
    view! {
        <header class="creator-head">
            <div>
                <a href="/creator" class="creator-crumb">"All simulations"</a>
                <h1>{name}</h1>
                <p class="creator-sub">{status}</p>
            </div>
            <div class="creator-mode" role="group" aria-label="Mode">
                <button
                    type="button"
                    class:is-active=move || mode.get() == CommerceMode::Test
                    on:click=move |_| mode.set(CommerceMode::Test)
                >
                    "Test"
                </button>
                <button
                    type="button"
                    class="is-live"
                    class:is-active=move || mode.get() == CommerceMode::Live
                    on:click=move |_| mode.set(CommerceMode::Live)
                >
                    "Live"
                </button>
            </div>
        </header>
        <p class="creator-mode-note" class:is-live=move || mode.get() == CommerceMode::Live>
            {move || match mode.get() {
                CommerceMode::Test => "Test mode: drafts and Studio's test purchases. Nothing here moves Tickets.",
                CommerceMode::Live => "Live mode: what players see and buy. Changes here are real.",
            }}
        </p>
        <nav class="creator-tabs" aria-label="Sections">
            {tab_button(Tab::Products, "Products")}
            {tab_button(Tab::Sales, "Sales")}
            {tab_button(Tab::Events, "Events")}
            {tab_button(Tab::Webhooks, "Webhooks")}
        </nav>
        {move || match tab.get() {
            Tab::Products => view! { <ProductsPanel sim=sim.clone() mode=mode api_url=api_url.clone() /> }.into_any(),
            Tab::Sales => view! { <SalesPanel sim_id=sim.id.clone() mode=mode api_url=api_url.clone() /> }.into_any(),
            Tab::Events => view! { <EventsPanel sim_id=sim.id.clone() mode=mode api_url=api_url.clone() /> }.into_any(),
            Tab::Webhooks => view! { <WebhooksPanel mode=mode api_url=api_url.clone() /> }.into_any(),
        }}
    }
}

// -----------------------------------------------------------------------------
// 4. Products
// -----------------------------------------------------------------------------

#[component]
fn ProductsPanel(sim: SellerSimulation, mode: RwSignal<CommerceMode>, api_url: String) -> impl IntoView {
    let products = RwSignal::new(Option::<Vec<CommerceProduct>>::None);
    let error = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let editing = RwSignal::new(Option::<String>::None);

    let api = StoredValue::new(api_url);
    let sim_id = StoredValue::new(sim.id.clone());

    Effect::new(move |_| {
        let m = mode.get();
        reload.track();
        products.set(None);
        let (api_url, id) = (api.get_value(), sim_id.get_value());
        spawn_local(async move {
            let result = list_products(&api_url, m, &id).await;
            // A mode switched while this was out answers for the new mode.
            if mode.get_untracked() != m {
                return;
            }
            match result {
                Ok(list) => {
                    products.set(Some(list));
                    error.set(None);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
        });
    });

    // The new-product form.
    let name = RwSignal::new(String::new());
    let kind = RwSignal::new("consumable".to_string());
    let price = RwSignal::new(String::new());
    let space = RwSignal::new(sim.spaces.first().cloned().unwrap_or_default());
    let description = RwSignal::new(String::new());
    let spaces = sim.spaces.clone();

    let create = move |_| {
        let Ok(tkt) = price.get_untracked().trim().parse::<u64>() else {
            error.set(Some("Price is a whole number of Tickets.".to_string()));
            return;
        };
        let (api_url, id) = (api.get_value(), sim_id.get_value());
        let (n, k, s, d, m) = (name.get_untracked(), kind.get_untracked(), space.get_untracked(), description.get_untracked(), mode.get_untracked());
        busy.set(true);
        spawn_local(async move {
            match create_product(&api_url, m, &id, &s, n.trim(), &k, tkt, d.trim()).await {
                Ok(p) => {
                    notice.set(Some(format!(
                        "{} is #{}, a draft. Scripts sell it with PromptProductPurchase({}); put it on sale in live mode.",
                        p.name, p.number, p.number
                    )));
                    error.set(None);
                    name.set(String::new());
                    price.set(String::new());
                    description.set(String::new());
                    reload.update(|r| *r += 1);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
            busy.set(false);
        });
    };

    let change = move |id: String, patch: Value, done: String| {
        let (api_url, m) = (api.get_value(), mode.get_untracked());
        busy.set(true);
        spawn_local(async move {
            match update_product(&api_url, m, &id, patch).await {
                Ok(_) => {
                    notice.set(Some(done));
                    error.set(None);
                    editing.set(None);
                    reload.update(|r| *r += 1);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="creator-card" aria-labelledby="creator-products-h">
            <div class="creator-card-head">
                <h2 class="creator-h2" id="creator-products-h">"Products"</h2>
                <span class="creator-count">
                    {move || products.with(|p| p.as_ref().map(|p| format!("{} in all", p.len())).unwrap_or_default())}
                </span>
            </div>
            {move || notice.get().map(|n| view! { <p class="creator-notice">{n}</p> })}
            {move || error.get().map(|e| view! { <p class="creator-error-line" role="alert">{e}</p> })}
            {move || match products.get() {
                None => view! { <p class="creator-hint">"Loading products…"</p> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <p class="creator-hint">"No products yet. Create the first one below."</p>
                }.into_any(),
                Some(list) => view! {
                    <ul class="creator-rows">
                        {list.into_iter().map(|p| {
                            let id = p.id.clone();
                            let is_editing = {
                                let id = id.clone();
                                move || editing.with(|e| e.as_deref() == Some(id.as_str()))
                            };
                            let toggle_label = if p.active { "Take off sale" } else { "Put on sale" };
                            let toggle = {
                                let (id, active, name) = (id.clone(), p.active, p.name.clone());
                                move |_| {
                                    let done = if active { format!("{name} is off sale.") } else { format!("{name} is on sale.") };
                                    change(id.clone(), json!({ "active": !active }), done);
                                }
                            };
                            let open_edit = {
                                let id = id.clone();
                                move |_| editing.set(Some(id.clone()))
                            };
                            view! {
                                <li class="creator-row">
                                    <span class="creator-num">{format!("#{}", p.number)}</span>
                                    <span class="creator-row-main">
                                        <span class="creator-row-title">{p.name.clone()}</span>
                                        <span class="creator-row-sub">
                                            {kind_label(&p.kind)}
                                            {p.space.clone().map(|s| format!(" \u{00b7} {s}"))}
                                            {(!p.description.is_empty()).then(|| format!(" \u{00b7} {}", p.description))}
                                        </span>
                                    </span>
                                    <span class="creator-amount">{tickets(p.price)}</span>
                                    <span class=if p.active { "creator-chip is-ok" } else { "creator-chip" }>
                                        {if p.active { "On sale" } else { "Draft" }}
                                    </span>
                                    <span class="creator-actions">
                                        <button type="button" class="creator-btn" on:click=open_edit disabled=move || busy.get()>"Edit"</button>
                                        <button
                                            type="button"
                                            class="creator-btn"
                                            title=move || (mode.get() == CommerceMode::Test).then(|| "Switch to Live: what players see is a live change".to_string())
                                            disabled=move || busy.get() || mode.get() == CommerceMode::Test
                                            on:click=toggle
                                        >
                                            {toggle_label}
                                        </button>
                                    </span>
                                    {move || is_editing().then(|| view! { <ProductEditor product=p.clone() busy=busy on_save=change on_cancel=move || editing.set(None) /> })}
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </section>

        <section class="creator-card" aria-labelledby="creator-new-h">
            <div class="creator-card-head">
                <h2 class="creator-h2" id="creator-new-h">"New product"</h2>
            </div>
            <div class="creator-form">
                <label>
                    <span>"Name"</span>
                    <input type="text" maxlength="80" placeholder="100 Coins" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
                </label>
                <label>
                    <span>"Kind"</span>
                    <select on:change=move |ev| kind.set(event_target_value(&ev))>
                        <option value="consumable" selected=move || kind.get() == "consumable">"Product: granted each purchase"</option>
                        <option value="pass" selected=move || kind.get() == "pass">"Pass: owned once"</option>
                    </select>
                </label>
                <label>
                    <span>"Price (Tickets)"</span>
                    <input type="number" min="0" step="1" placeholder="50" prop:value=move || price.get() on:input=move |ev| price.set(event_target_value(&ev)) />
                </label>
                <label>
                    <span>"Sold in Space"</span>
                    <select on:change=move |ev| space.set(event_target_value(&ev))>
                        {spaces.into_iter().map(|s| {
                            let (value, this) = (s.clone(), s.clone());
                            view! { <option value=value selected=move || space.get() == this>{s}</option> }
                        }).collect_view()}
                    </select>
                </label>
                <label class="creator-form-wide">
                    <span>"Description"</span>
                    <input type="text" maxlength="500" placeholder="What the player gets" prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev)) />
                </label>
            </div>
            <div class="creator-form-foot">
                <p class="creator-hint">"A new product is a draft: testable in Studio, invisible to players until you put it on sale in live mode."</p>
                <button
                    type="button"
                    class="btn btn-primary"
                    disabled=move || busy.get() || name.get().trim().is_empty() || price.get().trim().is_empty() || space.get().is_empty()
                    on:click=create
                >
                    "Create draft"
                </button>
            </div>
        </section>
    }
}

/// Change a product's name, price and description.
#[component]
fn ProductEditor(
    product: CommerceProduct,
    busy: RwSignal<bool>,
    on_save: impl Fn(String, Value, String) + Copy + 'static,
    on_cancel: impl Fn() + Copy + 'static,
) -> impl IntoView {
    let name = RwSignal::new(product.name.clone());
    let price = RwSignal::new(product.price.to_string());
    let description = RwSignal::new(product.description.clone());
    let id = StoredValue::new(product.id.clone());
    let save = move |_| {
        let Ok(tkt) = price.get_untracked().trim().parse::<u64>() else { return };
        let patch = json!({
            "name": name.get_untracked().trim(),
            "price": tkt,
            "description": description.get_untracked().trim(),
        });
        on_save(id.get_value(), patch, format!("Saved {}.", name.get_untracked().trim()));
    };
    view! {
        <div class="creator-editor">
            <label>
                <span>"Name"</span>
                <input type="text" maxlength="80" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
            </label>
            <label>
                <span>"Price (Tickets)"</span>
                <input type="number" min="0" step="1" prop:value=move || price.get() on:input=move |ev| price.set(event_target_value(&ev)) />
            </label>
            <label class="creator-form-wide">
                <span>"Description"</span>
                <input type="text" maxlength="500" prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev)) />
            </label>
            <div class="creator-editor-foot">
                <p class="creator-hint">"A product on sale changes in live mode only."</p>
                <button type="button" class="creator-btn" on:click=move |_| on_cancel()>"Cancel"</button>
                <button type="button" class="btn btn-primary" disabled=move || busy.get() on:click=save>"Save"</button>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 5. Sales
// -----------------------------------------------------------------------------

#[component]
fn SalesPanel(sim_id: String, mode: RwSignal<CommerceMode>, api_url: String) -> impl IntoView {
    let sales = RwSignal::new(Option::<(Vec<CommerceSale>, bool)>::None);
    let error = RwSignal::new(Option::<String>::None);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let api = StoredValue::new(api_url);
    let sim = StoredValue::new(sim_id);

    Effect::new(move |_| {
        let m = mode.get();
        reload.track();
        sales.set(None);
        let (api_url, id) = (api.get_value(), sim.get_value());
        spawn_local(async move {
            let result = list_sales(&api_url, m, &id).await;
            if mode.get_untracked() != m {
                return;
            }
            match result {
                Ok(list) => {
                    sales.set(Some((list.data, list.has_more)));
                    error.set(None);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
        });
    });

    let refund = move |id: String, what: String| {
        if !confirmed(&format!("Refund {what}? The buyer gets its Tickets back and your share is taken back.")) {
            return;
        }
        let (api_url, m) = (api.get_value(), mode.get_untracked());
        busy.set(true);
        spawn_local(async move {
            match refund_sale(&api_url, m, &id).await {
                Ok(_) => reload.update(|r| *r += 1),
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="creator-card" aria-labelledby="creator-sales-h">
            <div class="creator-card-head">
                <h2 class="creator-h2" id="creator-sales-h">"Sales"</h2>
                <button type="button" class="creator-btn" on:click=move |_| reload.update(|r| *r += 1)>"Refresh"</button>
            </div>
            {move || error.get().map(|e| view! { <p class="creator-error-line" role="alert">{e}</p> })}
            {move || match sales.get() {
                None => view! { <p class="creator-hint">"Loading sales…"</p> }.into_any(),
                Some((list, _)) if list.is_empty() => view! {
                    <p class="creator-hint">
                        {move || match mode.get() {
                            CommerceMode::Test => "No test purchases yet. Buy something in Play, in Studio, to see one here.",
                            CommerceMode::Live => "No sales yet.",
                        }}
                    </p>
                }.into_any(),
                Some((list, has_more)) => {
                    let kept: Vec<&CommerceSale> = list.iter().filter(|s| s.status == "succeeded").collect();
                    let gross: u64 = kept.iter().map(|s| s.amount).sum();
                    let earned: u64 = kept.iter().map(|s| s.creator_amount).sum();
                    let waiting = kept.iter().filter(|s| s.product.kind != "pass" && !s.fulfilled).count() as u64;
                    let count = list.len() as u64;
                    view! {
                        <div class="creator-totals">
                            <div><span class="creator-label">"Sales"</span><span class="creator-total">{format_count(count)}</span></div>
                            <div><span class="creator-label">"Paid by players"</span><span class="creator-total">{tickets(gross)}</span></div>
                            <div><span class="creator-label">"Your share"</span><span class="creator-total">{tickets(earned)}</span></div>
                            <div><span class="creator-label">"Waiting to be granted"</span><span class="creator-total">{format_count(waiting)}</span></div>
                        </div>
                        {has_more.then(|| view! { <p class="creator-hint">"The newest 100 sales. The CLI lists them all: eustress commerce purchases list."</p> })}
                        <ul class="creator-rows">
                            {list.into_iter().map(|s| {
                                let what = format!("#{} {} ({})", s.product.number, s.product.name, tickets(s.amount));
                                let (chip_class, chip) = match (s.status.as_str(), s.product.kind.as_str(), s.fulfilled) {
                                    ("refunded", _, _) => ("creator-chip is-held", "Refunded"),
                                    (_, "pass", _) => ("creator-chip is-ok", "Owned"),
                                    (_, _, true) => ("creator-chip is-ok", "Granted"),
                                    _ => ("creator-chip is-wait", "Waiting for ProcessReceipt"),
                                };
                                let can_refund = s.status == "succeeded";
                                let on_refund = {
                                    let (id, what) = (s.id.clone(), what.clone());
                                    move |_| refund(id.clone(), what.clone())
                                };
                                view! {
                                    <li class="creator-row">
                                        <span class="creator-num">{format!("#{}", s.product.number)}</span>
                                        <span class="creator-row-main">
                                            <span class="creator-row-title">{s.product.name.clone()}</span>
                                            <span class="creator-row-sub">
                                                {when(s.created)}
                                                " \u{00b7} buyer "
                                                <code>{s.buyer_id.chars().take(8).collect::<String>()}</code>
                                                {s.space.clone().map(|sp| format!(" \u{00b7} {sp}"))}
                                                {s.synthetic.then(|| " \u{00b7} test helper")}
                                            </span>
                                        </span>
                                        <span class="creator-amount">
                                            {tickets(s.amount)}
                                            <span class="creator-row-sub">{format!("{} yours", format_count(s.creator_amount))}</span>
                                        </span>
                                        <span class=chip_class>{chip}</span>
                                        <span class="creator-actions">
                                            {can_refund.then(|| view! {
                                                <button type="button" class="creator-btn" disabled=move || busy.get() on:click=on_refund>"Refund"</button>
                                            })}
                                        </span>
                                    </li>
                                }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            }}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 6. Events
// -----------------------------------------------------------------------------

/// What an event is about, in a line.
fn event_detail(event: &CommerceEvent) -> String {
    let object = event.data.get("object").cloned().unwrap_or(Value::Null);
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let product = object.get("product").cloned().unwrap_or_else(|| object.clone());
    let name = text(&product, "name");
    let number = product.get("number").and_then(Value::as_u64);
    let mut line = match number {
        Some(n) if !name.is_empty() => format!("#{n} {name}"),
        _ => name,
    };
    if let Some(amount) = object.get("amount").and_then(Value::as_u64) {
        line.push_str(&format!(" \u{00b7} {}", tickets(amount)));
    }
    let failure = text(&object, "failure_message");
    if !failure.is_empty() {
        line.push_str(&format!(" \u{00b7} {failure}"));
    }
    line
}

#[component]
fn EventsPanel(sim_id: String, mode: RwSignal<CommerceMode>, api_url: String) -> impl IntoView {
    let events = RwSignal::new(Option::<Vec<CommerceEvent>>::None);
    let error = RwSignal::new(Option::<String>::None);
    let reload = RwSignal::new(0u32);
    let api = StoredValue::new(api_url);
    let sim = StoredValue::new(sim_id);

    Effect::new(move |_| {
        let m = mode.get();
        reload.track();
        events.set(None);
        let api_url = api.get_value();
        spawn_local(async move {
            let result = list_events(&api_url, m).await;
            if mode.get_untracked() != m {
                return;
            }
            match result {
                Ok(list) => {
                    // The account's events cover all its simulations.
                    let here = sim.get_value();
                    events.set(Some(list.into_iter().filter(|e| e.sim_id.as_deref().map_or(true, |s| s == here)).collect()));
                    error.set(None);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
        });
    });

    view! {
        <section class="creator-card" aria-labelledby="creator-events-h">
            <div class="creator-card-head">
                <h2 class="creator-h2" id="creator-events-h">"Events"</h2>
                <button type="button" class="creator-btn" on:click=move |_| reload.update(|r| *r += 1)>"Refresh"</button>
            </div>
            <p class="creator-hint">
                "What happened to your products and sales, newest first. "
                <code>"eustress commerce listen"</code>
                " streams the same events as they happen."
            </p>
            {move || error.get().map(|e| view! { <p class="creator-error-line" role="alert">{e}</p> })}
            {move || match events.get() {
                None => view! { <p class="creator-hint">"Loading events…"</p> }.into_any(),
                Some(list) if list.is_empty() => view! { <p class="creator-hint">"No events yet."</p> }.into_any(),
                Some(list) => view! {
                    <ul class="creator-rows">
                        {list.into_iter().map(|e| view! {
                            <li class="creator-row">
                                <span class="creator-chip">{e.kind.clone()}</span>
                                <span class="creator-row-main">
                                    <span class="creator-row-title">{event_detail(&e)}</span>
                                    <span class="creator-row-sub">{when(e.created)}" \u{00b7} "<code>{e.id.clone()}</code></span>
                                </span>
                            </li>
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </section>
    }
}

// -----------------------------------------------------------------------------
// 7. Webhooks
// -----------------------------------------------------------------------------

#[component]
fn WebhooksPanel(mode: RwSignal<CommerceMode>, api_url: String) -> impl IntoView {
    let endpoints = RwSignal::new(Option::<Vec<WebhookEndpoint>>::None);
    let error = RwSignal::new(Option::<String>::None);
    let secret = RwSignal::new(Option::<(String, String)>::None);
    let reload = RwSignal::new(0u32);
    let busy = RwSignal::new(false);
    let url = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let api = StoredValue::new(api_url);

    Effect::new(move |_| {
        let m = mode.get();
        reload.track();
        endpoints.set(None);
        let api_url = api.get_value();
        spawn_local(async move {
            let result = list_webhooks(&api_url, m).await;
            if mode.get_untracked() != m {
                return;
            }
            match result {
                Ok(list) => {
                    endpoints.set(Some(list));
                    error.set(None);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
        });
    });

    let add = move |_| {
        let (api_url, m, u, d) = (api.get_value(), mode.get_untracked(), url.get_untracked(), description.get_untracked());
        busy.set(true);
        spawn_local(async move {
            match create_webhook(&api_url, m, u.trim(), d.trim()).await {
                Ok(endpoint) => {
                    secret.set(endpoint.secret.clone().map(|s| (endpoint.url.clone(), s)));
                    url.set(String::new());
                    description.set(String::new());
                    error.set(None);
                    reload.update(|r| *r += 1);
                }
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
            busy.set(false);
        });
    };

    let remove = move |id: String, target: String| {
        if !confirmed(&format!("Stop sending events to {target}?")) {
            return;
        }
        let (api_url, m) = (api.get_value(), mode.get_untracked());
        busy.set(true);
        spawn_local(async move {
            match delete_webhook(&api_url, m, &id).await {
                Ok(_) => reload.update(|r| *r += 1),
                Err(e) => error.set(Some(commerce_error_text(&e))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="creator-card" aria-labelledby="creator-hooks-h">
            <div class="creator-card-head">
                <h2 class="creator-h2" id="creator-hooks-h">"Webhooks"</h2>
            </div>
            <p class="creator-hint">
                "Every event of this mode, posted to your server and signed with the endpoint's secret. "
                "Each mode keeps its own endpoints."
            </p>
            {move || error.get().map(|e| view! { <p class="creator-error-line" role="alert">{e}</p> })}
            {move || secret.get().map(|(target, s)| view! {
                <div class="creator-secret" role="status">
                    <p>{format!("Signing secret for {target}. Copy it now: it is not shown again.")}</p>
                    <code class="creator-secret-value">{s}</code>
                    <button type="button" class="creator-btn" on:click=move |_| secret.set(None)>"Done"</button>
                </div>
            })}
            {move || match endpoints.get() {
                None => view! { <p class="creator-hint">"Loading endpoints…"</p> }.into_any(),
                Some(list) if list.is_empty() => view! { <p class="creator-hint">"No endpoints in this mode."</p> }.into_any(),
                Some(list) => view! {
                    <ul class="creator-rows">
                        {list.into_iter().map(|w| {
                            let on_remove = {
                                let (id, target) = (w.id.clone(), w.url.clone());
                                move |_| remove(id.clone(), target.clone())
                            };
                            let last = w.last_delivery.as_ref().map(|d| {
                                let ok = d.get("ok").and_then(Value::as_bool).unwrap_or(false);
                                let at = d.get("at").and_then(Value::as_i64).map(when).unwrap_or_default();
                                format!("last delivery {} {at}", if ok { "succeeded" } else { "failed" })
                            });
                            view! {
                                <li class="creator-row">
                                    <span class="creator-row-main">
                                        <span class="creator-row-title">{w.url.clone()}</span>
                                        <span class="creator-row-sub">
                                            {if w.description.is_empty() { w.enabled_events.join(", ") } else { w.description.clone() }}
                                            {last.map(|l| format!(" \u{00b7} {l}"))}
                                        </span>
                                    </span>
                                    <span class=if w.status == "enabled" { "creator-chip is-ok" } else { "creator-chip is-held" }>{w.status.clone()}</span>
                                    <span class="creator-actions">
                                        <button type="button" class="creator-btn" disabled=move || busy.get() on:click=on_remove>"Remove"</button>
                                    </span>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
            }}
            <div class="creator-form">
                <label class="creator-form-wide">
                    <span>"Endpoint URL"</span>
                    <input type="url" placeholder="https://example.com/eustress/webhook" prop:value=move || url.get() on:input=move |ev| url.set(event_target_value(&ev)) />
                </label>
                <label class="creator-form-wide">
                    <span>"Description"</span>
                    <input type="text" maxlength="200" placeholder="Order fulfillment" prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev)) />
                </label>
            </div>
            <div class="creator-form-foot">
                <p class="creator-hint">"An https address on a public host. For a local server, use eustress commerce listen --forward-to."</p>
                <button type="button" class="btn btn-primary" disabled=move || busy.get() || url.get().trim().is_empty() on:click=add>
                    "Add endpoint"
                </button>
            </div>
        </section>
    }
}
