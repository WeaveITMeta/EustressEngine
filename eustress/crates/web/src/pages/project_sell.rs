// =============================================================================
// Eustress Web - Sell (/projects/:sim_id)
// =============================================================================
// What a creator sells in one of their published projects: the products, the
// sales they made, what happened to them (events), and where the API posts
// those events (webhooks). The same data `eustress commerce` works on, in the
// same two modes: test shapes drafts and moves nothing; live is what players
// see and buy.
//
// The projects page (/projects) is the one list. Each published project's card
// shows whether it can sell and links here with its Sell button. The older
// creator addresses redirect to these pages (CreatorRedirect below, app.rs and
// the host's _redirects file).
//
// The API answers only for the signed-in account's own simulations, so this
// page cannot be opened onto someone else's sales.
//
// Table of Contents:
// 1. Formatting and the card's sell chip
// 2. Page
// 3. One simulation
// 4. Products
// 5. Sales
// 6. Events
// 7. Webhooks
// 8. The projects page's parts
// 9. The older address
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
// 1. Formatting and the card's sell chip
// -----------------------------------------------------------------------------

/// A Worker timestamp (seconds) in the reader's zone and clock.
fn when(secs: i64) -> String {
    crate::time_words::when_secs(secs)
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

/// What a project's card says about selling, in words. A project that can sell
/// says how many products it has; one that cannot says why, in the Worker's own
/// sentence.
pub(crate) fn sell_chip_text(can_sell: bool, products: Option<u64>, reason: Option<&str>) -> String {
    if can_sell {
        match products {
            Some(1) => "Sells to players, 1 product".to_string(),
            Some(n) => format!("Sells to players, {} products", format_count(n)),
            None => "Sells to players".to_string(),
        }
    } else {
        match reason.map(str::trim).filter(|r| !r.is_empty()) {
            Some(why) => why.to_string(),
            None => "Cannot sell yet".to_string(),
        }
    }
}

/// The chip's style and words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SellChipLook {
    pub class: &'static str,
    pub text: String,
}

pub(crate) fn sell_chip(sim: &SellerSimulation) -> SellChipLook {
    SellChipLook {
        class: if sim.can_sell { "sell-chip is-ok" } else { "sell-chip is-held" },
        text: sell_chip_text(sim.can_sell, sim.products, sim.reason.as_deref()),
    }
}

/// Where a project's Sell button goes.
pub(crate) fn sell_href(sim_id: &str) -> String {
    format!("/projects/{sim_id}")
}

/// The account's selling entry for a project, when it has one. Only published
/// projects have one; the others show no chip and no Sell button.
pub(crate) fn seller_for<'a>(sims: &'a [SellerSimulation], project_id: &str) -> Option<&'a SellerSimulation> {
    sims.iter().find(|s| s.id == project_id)
}

// -----------------------------------------------------------------------------
// 2. Page
// -----------------------------------------------------------------------------

/// The selling dashboard for one project.
#[component]
pub fn ProjectSellPage() -> impl IntoView {
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
            <CentralNav active="projects".to_string() />
            <div class="creator-wrap">
                {move || {
                    if session.get() == Session::SignedOut {
                        return view! {
                            <div class="creator-card creator-center">
                                <p>"Sign in to sell in your projects."</p>
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
                            <div class="creator-card"><p class="creator-hint">"Loading your project…"</p></div>
                        }.into_any();
                    };
                    let mine = sim_id.get().and_then(|id| seller_for(&acct.simulations, &id).cloned());
                    match mine {
                        Some(sim) => view! { <SimulationCommerce sim=sim api_url=api_url.clone() /> }.into_any(),
                        None => view! { <NotYours /> }.into_any(),
                    }
                }}
            </div>
            <Footer />
        </div>
    }
}

/// The way back to the list: the crumb over a project's name, and the way out of a page that is not yours.
#[component]
fn AllProjects(class: &'static str) -> impl IntoView {
    view! { <a href="/projects" class=class>"All projects"</a> }
}

/// An address that names no project of the signed-in account.
#[component]
fn NotYours() -> impl IntoView {
    view! {
        <div class="creator-card creator-center">
            <p>"This is not one of your published projects."</p>
            <AllProjects class="creator-link" />
        </div>
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
                <AllProjects class="creator-crumb" />
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

// -----------------------------------------------------------------------------
// 8. The projects page's parts
// -----------------------------------------------------------------------------

/// The line under the projects page's heading that points to selling.
#[component]
pub fn SellNote() -> impl IntoView {
    view! {
        <p class="projects-sell-note">
            "A published project can sell products and passes to players. Use the Sell button on its card. "
            <a href="/docs/earning">"How selling works"</a>
        </p>
    }
}

/// A card's line about selling: that the project sells and how many products it has, or why it cannot yet.
#[component]
pub fn SellChip(sim: SellerSimulation) -> impl IntoView {
    let look = sell_chip(&sim);
    view! { <p class=look.class>{look.text}</p> }
}

/// A card's Sell button: the project's products, sales, events and webhooks.
#[component]
pub fn SellButton(sim_id: String) -> impl IntoView {
    view! {
        <a href=sell_href(&sim_id) class="action-btn secondary" title="Products, sales, events and webhooks for this project">
            <img src="/assets/icons/tag.svg" alt="" />
            "Sell"
        </a>
    }
}

// -----------------------------------------------------------------------------
// 9. The older address
// -----------------------------------------------------------------------------

/// A project id that is safe to put in an address: letters, digits, dashes and
/// underscores. Anything else goes to the list.
fn is_plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Where the older creator addresses go: the list, or the project's Sell page.
pub(crate) fn creator_redirect_target(sim_id: Option<&str>) -> String {
    match sim_id.filter(|id| is_plain_id(id)) {
        Some(id) => sell_href(id),
        None => "/projects".to_string(),
    }
}

/// The in-app side of the redirect. The host redirects the address itself; a
/// link inside the app can land here first.
#[component]
pub fn CreatorRedirect() -> impl IntoView {
    let params = use_params_map();
    let target = creator_redirect_target(params.get_untracked().get("sim_id").as_deref());
    view! { <leptos_router::components::Redirect path=target options=leptos_router::NavigateOptions { replace: true, ..Default::default() } /> }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn sim(id: &str, can_sell: bool, products: Option<u64>, reason: Option<&str>) -> SellerSimulation {
        SellerSimulation {
            id: id.to_string(),
            name: "Orbit".to_string(),
            spaces: Vec::new(),
            listed: true,
            can_sell,
            reason: reason.map(str::to_string),
            products,
            updated_at: None,
        }
    }

    /// The older creator address, built so that this file never holds it as a literal.
    fn older() -> String {
        ["/", "creator"].concat()
    }

    /// The words a reader sees in some markup, with the tags removed.
    fn visible(html: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => {
                    in_tag = false;
                    out.push(' ');
                }
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    }

    fn html_of(view: impl RenderHtml) -> String {
        let owner = Owner::new();
        owner.with(|| view.to_html())
    }

    /// This file's code, without its tests.
    fn code() -> &'static str {
        include_str!("project_sell.rs").split("#[cfg(test)]").next().unwrap()
    }

    #[test]
    fn a_project_that_sells_says_how_many_products_it_has() {
        assert_eq!(sell_chip_text(true, Some(3), None), "Sells to players, 3 products");
        assert_eq!(sell_chip_text(true, Some(1), None), "Sells to players, 1 product");
        assert_eq!(sell_chip_text(true, Some(0), None), "Sells to players, 0 products");
        assert_eq!(sell_chip_text(true, Some(1200), None), "Sells to players, 1,200 products");
        assert_eq!(sell_chip_text(true, None, None), "Sells to players");
        // a reason does not matter to a project that sells
        assert_eq!(sell_chip_text(true, Some(2), Some("stale")), "Sells to players, 2 products");
    }

    #[test]
    fn a_project_that_cannot_sell_says_why_and_never_says_it_sells() {
        assert_eq!(sell_chip_text(false, Some(2), Some("Not listed in the Gallery yet")), "Not listed in the Gallery yet");
        assert_eq!(sell_chip_text(false, None, Some("  Under review  ")), "Under review");
        assert_eq!(sell_chip_text(false, None, None), "Cannot sell yet");
        assert_eq!(sell_chip_text(false, Some(5), Some("   ")), "Cannot sell yet");
        assert!(!sell_chip_text(false, Some(5), None).contains("Sells"));
    }

    #[test]
    fn the_chip_is_green_when_it_sells_and_amber_when_it_cannot() {
        let ok = sell_chip(&sim("a", true, Some(4), None));
        assert_eq!((ok.class, ok.text.as_str()), ("sell-chip is-ok", "Sells to players, 4 products"));
        let held = sell_chip(&sim("a", false, Some(4), Some("Unlisted")));
        assert_eq!((held.class, held.text.as_str()), ("sell-chip is-held", "Unlisted"));
    }

    #[test]
    fn a_card_draws_the_chip_and_the_sell_button_with_their_words_and_address() {
        let ok = html_of(SellChip(SellChipProps { sim: sim("a1", true, Some(2), None) }));
        assert!(ok.contains("sell-chip is-ok") && visible(&ok).contains("Sells to players, 2 products"), "{ok}");
        let held = html_of(SellChip(SellChipProps { sim: sim("a1", false, None, Some("Not listed in the Gallery yet")) }));
        assert!(held.contains("sell-chip is-held") && visible(&held).contains("Not listed in the Gallery yet"), "{held}");
        assert!(!visible(&held).contains("Sells to players"), "{held}");

        let button = html_of(SellButton(SellButtonProps { sim_id: "a1-b2".to_string() }));
        assert!(button.contains("href=\"/projects/a1-b2\""), "{button}");
        assert_eq!(visible(&button).trim(), "Sell");
    }

    #[test]
    fn a_card_finds_its_own_entry_and_a_project_without_one_gets_nothing() {
        let list = vec![sim("first", true, Some(1), None), sim("second", false, None, Some("Unlisted"))];
        assert_eq!(seller_for(&list, "second").map(|s| s.id.as_str()), Some("second"));
        assert_eq!(seller_for(&list, "first").map(|s| s.can_sell), Some(true));
        assert!(seller_for(&list, "a draft with no entry").is_none());
        assert!(seller_for(&[], "first").is_none());
    }

    #[test]
    fn the_projects_page_wires_the_chip_the_button_and_the_note_to_the_commerce_account() {
        let projects = include_str!("projects.rs");
        for needle in [
            "get_commerce_account(&api_url, CommerceMode::Test)",
            "<SellNote />",
            "<SellChip sim=sim />",
            "<SellButton sim_id=sim.id />",
            "seller_for(list, &seller_id)",
            "sellers=sellers",
        ] {
            assert!(projects.contains(needle), "projects.rs does not hold {needle}");
        }
        // the chip and the button show only for a project that has an entry
        assert!(projects.contains("seller.get().map(|sim| view! { <SellChip"), "the chip waits for an entry");
        assert!(projects.contains("seller.get().map(|sim| view! { <SellButton"), "the button waits for an entry");
    }

    #[test]
    fn the_header_line_points_to_how_selling_works() {
        let html = html_of(SellNote());
        assert!(html.contains("href=\"/docs/earning\""), "{html}");
        assert!(visible(&html).contains("How selling works"), "{html}");
        assert!(include_str!("../app.rs").contains("path!(\"/docs/earning\")"), "the link goes to a page the app has");
    }

    #[test]
    fn the_dashboard_moved_to_the_project_address_and_the_older_ones_redirect() {
        let app = include_str!("../app.rs");
        assert!(app.contains("<Route path=path!(\"/projects/:sim_id\") view=ProjectSellPage />"), "no route for a project's Sell page");
        assert!(app.contains("<Route path=path!(\"/projects\") view=ProjectsPage />"), "the list is still there");
        let older = older();
        assert!(app.contains(&format!("<Route path=path!(\"{older}\") view=CreatorRedirect />")), "the older list address is not redirected in the app");
        assert!(app.contains(&format!("<Route path=path!(\"{older}/:sim_id\") view=CreatorRedirect />")), "the older project address is not redirected in the app");
        assert!(!app.contains("CreatorPage") && !include_str!("mod.rs").contains(&format!("pub mod {};", &older[1..])), "the old page is gone");
        assert!(include_str!("mod.rs").contains("pub mod project_sell;"));
        // and the host redirects both, permanently
        let redirects = include_str!("../../_redirects");
        assert!(redirects.lines().any(|l| l.trim() == format!("{older} /projects 301")), "the host does not redirect the list");
        assert!(redirects.lines().any(|l| l.trim() == format!("{older}/:id /projects/:id 301")), "the host does not redirect a project");
    }

    #[test]
    fn the_older_address_goes_to_the_list_or_to_the_same_project() {
        assert_eq!(creator_redirect_target(None), "/projects");
        assert_eq!(creator_redirect_target(Some("")), "/projects");
        assert_eq!(creator_redirect_target(Some("3f2a-9c")), "/projects/3f2a-9c");
        assert_eq!(creator_redirect_target(Some("sim_7")), "/projects/sim_7");
        // anything that could change where the address goes falls back to the list
        let too_long = "x".repeat(65);
        for bad in ["../admin", "a/b", "a?b=1", "a#b", "a b", "a\\b", "//evil.example", too_long.as_str()] {
            assert_eq!(creator_redirect_target(Some(bad)), "/projects", "{bad}");
        }
        assert_eq!(creator_redirect_target(Some(&"x".repeat(64))), format!("/projects/{}", "x".repeat(64)));
    }

    #[test]
    fn no_link_in_the_site_goes_to_the_older_address_except_the_two_redirect_routes() {
        let older = older();
        let files: [(&str, &str); 16] = [
            ("app.rs", include_str!("../app.rs")),
            ("components/nav.rs", include_str!("../components/nav.rs")),
            ("components/footer.rs", include_str!("../components/footer.rs")),
            ("components/layout.rs", include_str!("../components/layout.rs")),
            ("components/pass_store.rs", include_str!("../components/pass_store.rs")),
            ("components/play_chooser.rs", include_str!("../components/play_chooser.rs")),
            ("pages/dashboard.rs", include_str!("dashboard.rs")),
            ("pages/docs_earning.rs", include_str!("docs_earning.rs")),
            ("pages/docs_publishing.rs", include_str!("docs_publishing.rs")),
            ("pages/listing.rs", include_str!("listing.rs")),
            ("pages/mod.rs", include_str!("mod.rs")),
            ("pages/profile.rs", include_str!("profile.rs")),
            ("pages/projects.rs", include_str!("projects.rs")),
            ("pages/purchases.rs", include_str!("purchases.rs")),
            ("pages/settings.rs", include_str!("settings.rs")),
            ("pages/project_sell.rs", code()),
        ];
        for (name, text) in files {
            let text = if name == "app.rs" {
                // the two routes that redirect are the only place the older address may appear
                text.replace(&format!("path!(\"{older}\")"), "").replace(&format!("path!(\"{older}/:sim_id\")"), "")
            } else {
                text.to_string()
            };
            assert!(!text.contains(&older), "{name} still holds the older creator address");
        }
        for (name, text) in [("sitemap.xml", include_str!("../../sitemap.xml")), ("llms.txt", include_str!("../../llms.txt"))] {
            assert!(!text.contains(&older), "{name} lists the older creator address");
        }
    }

    #[test]
    fn the_sell_page_links_back_to_all_projects_in_the_crumb_and_when_the_project_is_not_yours() {
        let crumb = html_of(AllProjects(AllProjectsProps { class: "creator-crumb" }));
        assert!(crumb.contains("href=\"/projects\"") && crumb.contains("creator-crumb"), "{crumb}");
        assert_eq!(visible(&crumb).trim(), "All projects");
        let not_yours = html_of(NotYours());
        assert!(not_yours.contains("href=\"/projects\"") && visible(&not_yours).contains("This is not one of your published projects."), "{not_yours}");
        // the page itself uses them, and says to sign in when signed out
        let code = code();
        assert!(code.contains("<AllProjects class=\"creator-crumb\" />"), "the crumb");
        assert!(code.contains("Sign in to sell in your projects.") && code.contains("href=\"/login\""), "the signed-out words");
        assert!(code.contains("CentralNav active=\"projects\""), "the nav marks Projects");
    }

    #[test]
    fn the_words_this_page_adds_use_no_dashes() {
        let pieces = [
            html_of(SellNote()),
            html_of(NotYours()),
            html_of(SellChip(SellChipProps { sim: sim("a", true, Some(12), None) })),
            html_of(SellButton(SellButtonProps { sim_id: "a".to_string() })),
        ];
        for html in pieces {
            let words = visible(&html);
            assert!(!words.contains('\u{2014}') && !words.contains('\u{2013}'), "{words}");
        }
    }
}
