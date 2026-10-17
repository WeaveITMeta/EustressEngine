// =============================================================================
// Eustress Web - The Marketplace
// =============================================================================
// Models, scripts, plugins, audio, textures, templates and avatar items from creators, in the Gallery's look: glass panels,
// chips, mono numbers and the Gallery's card. Every item, name, price and count on this page is something the Worker said, read
// once when the page opens:
//
//   GET /api/marketplace?tab=creator&limit=50&   the items (api/marketplace.rs get_marketplace, the same call as before)
//
// The Worker answers every address under /api/marketplace with a stub today: an empty list. So the page shows its empty state,
// and the band under it says where creators sell now: the products and passes in their own Spaces, from Projects. Nothing
// stands in for the missing items: no sample cards, no star ratings, no download counts and no equity offers.
//
// A seller is shown by @username (pages/profile.rs handle_of) and links to their profile. An item's picture is used only when
// it is an https address or a path on this site; anything else draws its category's icon. An item's own page is
// pages/marketplace_item.rs, which also holds the purchase.
//
// Table of Contents:
// 1. What the Worker says, and the pure rules (tested)
// 2. The page
// 3. The items: controls, grid and card
// 4. Loading, failed and empty states
// 5. Tests
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::catalog::published_words;
use crate::api::marketplace::{get_marketplace, MarketplaceItem, MarketplaceQuery};
use crate::api::{self, ApiClient};
use crate::components::play_chooser::now_ms;
use crate::components::{CentralNav, Footer};
use crate::pages::profile::{handle_of, monogram};
use crate::state::AppState;
use crate::utils::format_count;

// -----------------------------------------------------------------------------
// 1. What the Worker says, and the pure rules
// -----------------------------------------------------------------------------

/// One section's data: still loading, here, or its route failed (with the words for why).
#[derive(Clone, Debug, PartialEq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// The two shelves an item sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shelf {
    Creator,
    Avatar,
}

impl Shelf {
    pub const ALL: [Shelf; 2] = [Shelf::Creator, Shelf::Avatar];

    pub fn label(self) -> &'static str {
        match self {
            Shelf::Creator => "Creator content",
            Shelf::Avatar => "Avatar items",
        }
    }
}

/// Every category the Worker names: its key, its words, its shelf and its icon under /assets/icons.
pub const CATEGORIES: [(&str, &str, Shelf, &str); 13] = [
    ("models", "3D models", Shelf::Creator, "cube.svg"),
    ("scripts", "Scripts", Shelf::Creator, "code.svg"),
    ("plugins", "Plugins", Shelf::Creator, "puzzle.svg"),
    ("audio", "Audio", Shelf::Creator, "audio.svg"),
    ("textures", "Textures", Shelf::Creator, "image.svg"),
    ("templates", "Templates", Shelf::Creator, "template.svg"),
    ("spaces", "Spaces", Shelf::Creator, "cube.svg"),
    ("clothing", "Clothing", Shelf::Avatar, "shirt.svg"),
    ("accessories", "Accessories", Shelf::Avatar, "sparkles.svg"),
    ("animations", "Animations", Shelf::Avatar, "animation.svg"),
    ("emotes", "Emotes", Shelf::Avatar, "smile.svg"),
    ("faces", "Faces", Shelf::Avatar, "face.svg"),
    ("bodies", "Bodies", Shelf::Avatar, "body.svg"),
];

fn category_of(category: &str) -> Option<&'static (&'static str, &'static str, Shelf, &'static str)> {
    let key = category.trim().to_ascii_lowercase();
    CATEGORIES.iter().find(|c| c.0 == key)
}

/// "3D models"; a category this page does not know reads "Other".
pub fn category_words(category: &str) -> &'static str {
    category_of(category).map(|c| c.1).unwrap_or("Other")
}

/// The shelf an item sits on; an unknown category sits with the creator content.
pub fn shelf_of(category: &str) -> Shelf {
    category_of(category).map(|c| c.2).unwrap_or(Shelf::Creator)
}

/// The category's icon, drawn when an item has no picture.
pub fn category_icon(category: &str) -> String {
    format!("/assets/icons/{}", category_of(category).map(|c| c.3).unwrap_or("cube.svg"))
}

/// How the items are ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Popular,
    Recent,
    PriceLow,
    PriceHigh,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::Popular, Sort::Recent, Sort::PriceLow, Sort::PriceHigh];

    pub fn key(self) -> &'static str {
        match self {
            Sort::Popular => "popular",
            Sort::Recent => "recent",
            Sort::PriceLow => "price-low",
            Sort::PriceHigh => "price-high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Popular => "Most sales",
            Sort::Recent => "Newest",
            Sort::PriceLow => "Price, low to high",
            Sort::PriceHigh => "Price, high to low",
        }
    }

    pub fn from_key(key: &str) -> Sort {
        Sort::ALL.into_iter().find(|s| s.key() == key).unwrap_or(Sort::Popular)
    }
}

/// What an item costs, in the words its own page uses: "Free", or the price in whole Bliss ("120 BLS").
pub fn price_words(item: &MarketplaceItem) -> String {
    if item.is_free { "Free".to_string() } else { format!("{:.0} BLS", item.price_bliss) }
}

/// The price an item sorts by: a free item costs nothing.
fn price_of(item: &MarketplaceItem) -> f64 {
    if item.is_free || !item.price_bliss.is_finite() { 0.0 } else { item.price_bliss }
}

/// "1 sale", "1,204 sales".
pub fn sales_words(n: u64) -> String {
    if n == 1 { "1 sale".to_string() } else { format!("{} sales", format_count(n)) }
}

/// "listed 3 days ago", from the item's creation time; nothing when the Worker sent no time.
pub fn listed_words(created_at: &str, now_ms: f64) -> Option<String> {
    published_words(created_at.trim(), now_ms).map(|w| w.replacen("published", "listed", 1))
}

/// The item's name, or "Untitled item".
pub fn item_name(item: &MarketplaceItem) -> String {
    if item.name.trim().is_empty() { "Untitled item".to_string() } else { item.name.trim().to_string() }
}

/// `/marketplace/a1`.
pub fn item_href(id: &str) -> String {
    format!("/marketplace/{}", urlencoding::encode(id))
}

/// The picture an item shows: an https address or a path on this site. Anything else, a protocol-relative address included,
/// never goes into an `<img>`.
pub fn art_of(item: &MarketplaceItem) -> Option<String> {
    item.thumbnail_url
        .as_deref()
        .map(str::trim)
        .filter(|u| u.starts_with("https://") || (u.starts_with('/') && !u.starts_with("//")))
        .map(str::to_string)
}

/// The seller: their @username, the address of their profile and the letter that stands for them. Nothing when the Worker
/// named nobody.
pub fn seller_of(item: &MarketplaceItem) -> Option<(String, String, String)> {
    let name = item.creator_name.trim();
    (!name.is_empty()).then(|| (handle_of(name), format!("/profile/{}", urlencoding::encode(name)), monogram(name)))
}

/// How many items sit on a shelf.
pub fn count_on(items: &[MarketplaceItem], shelf: Shelf) -> usize {
    items.iter().filter(|i| shelf_of(&i.category) == shelf).count()
}

/// The shelf the page opens on: the creator content, unless only avatar items are listed.
pub fn first_shelf(items: &[MarketplaceItem]) -> Shelf {
    if count_on(items, Shelf::Creator) == 0 && count_on(items, Shelf::Avatar) > 0 { Shelf::Avatar } else { Shelf::Creator }
}

/// The categories of a shelf that have items, in the page's order, with how many each has.
pub fn category_counts(items: &[MarketplaceItem], shelf: Shelf) -> Vec<(&'static str, &'static str, usize)> {
    CATEGORIES
        .iter()
        .filter(|c| c.2 == shelf)
        .map(|c| (c.0, c.1, items.iter().filter(|i| category_of(&i.category).is_some_and(|k| k.0 == c.0)).count()))
        .filter(|c| c.2 > 0)
        .collect()
}

/// The items the grid shows: those on the shelf, in the category ("all" for every one), whose name, words or seller hold the
/// search text (a leading @ is dropped), in the order asked for. Ties keep the Worker's order.
pub fn shown(items: &[MarketplaceItem], shelf: Shelf, category: &str, query: &str, sort: Sort) -> Vec<MarketplaceItem> {
    let q = query.trim().to_lowercase();
    let q = q.strip_prefix('@').unwrap_or(&q).trim().to_string();
    let mut out: Vec<MarketplaceItem> = items
        .iter()
        .filter(|i| shelf_of(&i.category) == shelf)
        .filter(|i| category == "all" || category_of(&i.category).is_some_and(|k| k.0 == category))
        .filter(|i| q.is_empty() || i.name.to_lowercase().contains(&q) || i.description.to_lowercase().contains(&q) || i.creator_name.to_lowercase().contains(&q))
        .cloned()
        .collect();
    match sort {
        Sort::Popular => out.sort_by(|a, b| b.sales_count.cmp(&a.sales_count)),
        Sort::Recent => out.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        Sort::PriceLow => out.sort_by(|a, b| price_of(a).total_cmp(&price_of(b))),
        Sort::PriceHigh => out.sort_by(|a, b| price_of(b).total_cmp(&price_of(a))),
    }
    out
}

/// The figures in the header, from the items the Worker sent: how many, from how many sellers, and how many are free. Nothing
/// at all when there are no items.
pub fn figures_of(items: &[MarketplaceItem]) -> Vec<(&'static str, String)> {
    if items.is_empty() {
        return Vec::new();
    }
    let mut sellers: Vec<String> = items.iter().map(|i| i.creator_name.trim().to_lowercase()).filter(|n| !n.is_empty()).collect();
    sellers.sort();
    sellers.dedup();
    vec![
        ("Items", format_count(items.len() as u64)),
        ("Sellers", format_count(sellers.len() as u64)),
        ("Free", format_count(items.iter().filter(|i| i.is_free).count() as u64)),
    ]
}

fn items_words(n: usize) -> String {
    if n == 1 { "1 item".to_string() } else { format!("{} items", format_count(n as u64)) }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

#[component]
pub fn MarketplacePage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let items = RwSignal::new(Load::<Vec<MarketplaceItem>>::Loading);

    // The one read: the creator tab, 50 items, as the page has always asked.
    let load = move || {
        items.set(Load::Loading);
        spawn_local(async move {
            let client = ApiClient::new(&api_url.get_value());
            let query = MarketplaceQuery {
                tab: Some("creator".to_string()),
                limit: Some(50),
                ..Default::default()
            };
            match get_marketplace(&client, &query).await {
                Ok(response) => items.set(Load::Ready(response.items)),
                Err(e) => {
                    log::warn!("Failed to fetch marketplace: {:?}", e);
                    items.set(Load::Failed(api::failure_sentence(&e)));
                }
            }
        });
    };
    Effect::new(move |_| load());

    view! {
        <div class="page g-page mk-page">
            <CentralNav active="marketplace".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <MarketBody items=items retry=Callback::new(move |_| load()) />
            </main>
            <Footer />
        </div>
    }
}

/// Everything under the top bar.
#[component]
pub fn MarketBody(items: RwSignal<Load<Vec<MarketplaceItem>>>, retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="g-wrap mk-wrap">
            <header class="g-glass mk-hero" aria-labelledby="mk-title">
                <div class="mk-hero-copy">
                    <div class="g-kicker"><span class="g-kick-rule"></span>"Creator content and avatar items"</div>
                    <h1 id="mk-title" class="mk-title">"Marketplace"</h1>
                    <p class="mk-lede">"Models, scripts, plugins, audio, textures, templates and avatar items from creators, each with who made it and what it costs."</p>
                </div>
                {move || {
                    let figures = match items.get() {
                        Load::Ready(list) => figures_of(&list),
                        _ => Vec::new(),
                    };
                    (!figures.is_empty()).then(|| view! {
                        <dl class="mk-figures">
                            {figures.into_iter().map(|(k, v)| view! { <div><dt>{k}</dt><dd>{v}</dd></div> }).collect_view()}
                        </dl>
                    })
                }}
            </header>

            <section class="g-section mk-section" aria-labelledby="mk-items-h">
                <div class="g-head"><h2 id="mk-items-h">"Items"</h2><span class="g-rule"></span></div>
                {move || match items.get() {
                    Load::Loading => loading_cards().into_any(),
                    Load::Failed(why) => failed_state(why, retry).into_any(),
                    Load::Ready(list) if list.is_empty() => empty_state().into_any(),
                    Load::Ready(list) => view! { <Shelves items=list /> }.into_any(),
                }}
            </section>

            <div class="g-glass g-band mk-band">
                <div>
                    <h2>"Sell what you build"</h2>
                    <p>"Today a creator sells inside their own Space: products and passes that players buy with Tickets. Set them up from the Sell button on a published Space in Projects."</p>
                </div>
                <div class="g-actions">
                    <a class="g-btn g-btn-primary g-btn-lg" href="/projects">"Open Projects"</a>
                    <a class="g-btn g-btn-lg" href="/docs/earning">"How selling works"</a>
                </div>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The items: controls, grid and card
// -----------------------------------------------------------------------------

const SEARCH_GLYPH: &str = "M11 4a7 7 0 1 0 0 14a7 7 0 0 0 0-14zM20 20l-3.6-3.6";

/// The shelves, the categories, search and order, and the grid of what passes.
#[component]
fn Shelves(items: Vec<MarketplaceItem>) -> impl IntoView {
    let items = StoredValue::new(items);
    let shelf = RwSignal::new(items.with_value(|l| first_shelf(l)));
    let category = RwSignal::new("all".to_string());
    let query = RwSignal::new(String::new());
    let sort = RwSignal::new(Sort::Popular);
    let rows = move || items.with_value(|l| shown(l, shelf.get(), &category.get(), &query.get(), sort.get()));

    view! {
        <div class="mk-controls">
            <div class="g-chips mk-shelves" role="group" aria-label="Shelf">
                {Shelf::ALL.into_iter().map(|s| {
                    let n = items.with_value(|l| count_on(l, s));
                    view! {
                        <button type="button" class="g-chip g-chip-btn" class:on=move || shelf.get() == s aria-pressed=move || (shelf.get() == s).to_string()
                            on:click=move |_| { shelf.set(s); category.set("all".to_string()); }>
                            {s.label()}<span class="mk-n">{n}</span>
                        </button>
                    }
                }).collect_view()}
            </div>
            <div class="mk-row">
                <form class="mk-search" role="search" on:submit=move |ev: leptos::ev::SubmitEvent| ev.prevent_default()>
                    <label class="g-sr" for="mk-q">"Search items and sellers"</label>
                    <svg class="mk-search-icon" viewBox="0 0 24 24" aria-hidden="true"><path d=SEARCH_GLYPH/></svg>
                    <input id="mk-q" class="mk-q" type="search" placeholder="Search items and sellers" autocomplete="off" spellcheck="false" maxlength="80"
                        prop:value=move || query.get() on:input=move |e| query.set(event_target_value(&e)) />
                </form>
                <label class="mk-sort">
                    <span>"Sort"</span>
                    <select on:change=move |e| sort.set(Sort::from_key(&event_target_value(&e)))>
                        {Sort::ALL.into_iter().map(|s| view! { <option value=s.key() selected=move || sort.get() == s>{s.label()}</option> }).collect_view()}
                    </select>
                </label>
            </div>
            {move || {
                let cats = items.with_value(|l| category_counts(l, shelf.get()));
                (!cats.is_empty()).then(|| view! {
                    <div class="g-chips mk-cats" role="group" aria-label="Category">
                        <button type="button" class="g-chip g-chip-btn" class:on=move || category.get() == "all" aria-pressed=move || (category.get() == "all").to_string()
                            on:click=move |_| category.set("all".to_string())>"All"</button>
                        {cats.into_iter().map(|(key, label, n)| view! {
                            <button type="button" class="g-chip g-chip-btn" class:on=move || category.get() == key aria-pressed=move || (category.get() == key).to_string()
                                on:click=move |_| category.set(key.to_string())>
                                {label}<span class="mk-n">{n}</span>
                            </button>
                        }).collect_view()}
                    </div>
                })
            }}
        </div>
        {move || {
            let list = rows();
            if list.is_empty() {
                let q = query.get();
                let hint = if q.trim().is_empty() { "Nothing on this shelf matches.".to_string() } else { format!("Nothing here matches \u{201c}{}\u{201d}.", q.trim()) };
                view! {
                    <div class="g-glass g-none mk-none">
                        <b>"No item matches"</b>
                        <p>{hint}</p>
                        <div class="g-actions">
                            <button type="button" class="g-btn" on:click=move |_| { query.set(String::new()); category.set("all".to_string()); }>"Show everything"</button>
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <p class="mk-count" role="status">{items_words(list.len())}</p>
                    <div class="g-grid mk-grid">
                        {list.into_iter().map(|i| view! { <ItemCard item=i /> }).collect_view()}
                    </div>
                }.into_any()
            }
        }}
    }
}

/// An item's picture, or its category's icon when it has none. The Worker answers 404 for art it will not serve (animated,
/// transparent, several images, not an image, too large), so a picture that fails to load clears art_ok and the card draws the
/// same icon as an item with no picture, never the browser's broken-image icon. Each card makes its own signal.
fn item_picture(art: Option<String>, icon: String, art_ok: RwSignal<bool>) -> impl IntoView {
    move || match art.clone().filter(|_| art_ok.get()) {
        Some(src) => view! { <img class="mk-img" src=src alt="" loading="lazy" decoding="async" on:error=move |_| art_ok.set(false) /> }.into_any(),
        None => view! { <span class="mk-art-empty" aria-hidden="true"><img src=icon.clone() alt="" /></span> }.into_any(),
    }
}

/// An item in the grid, built from the Gallery's card: its picture or its category's icon, the price in the corner, the name
/// (the whole card opens the item's page), the seller by @username, the category, and the sales and listing day.
#[component]
pub fn ItemCard(item: MarketplaceItem) -> impl IntoView {
    let name = item_name(&item);
    let href = item_href(&item.id);
    let art = art_of(&item);
    let icon = category_icon(&item.category);
    let price = price_words(&item);
    let free = item.is_free;
    let seller = seller_of(&item);
    let category = category_words(&item.category);
    let sales = item.sales_count;
    let listed = listed_words(&item.created_at, now_ms());
    let about = item.description.trim().to_string();
    view! {
        <article class="g-card mk-card">
            <div class="g-media">
                {item_picture(art, icon, RwSignal::new(true))}
                {item.is_verified.then(|| view! { <span class="g-chip g-badge mk-verified">"Verified"</span> })}
                <span class="g-count mk-price" class:free=free>{price}</span>
            </div>
            <div class="g-body">
                <h3 class="g-name"><a class="g-link" href=href>{name}</a></h3>
                {seller.map(|(handle, href, _)| view! { <div class="g-by">"by "<a href=href>{handle}</a></div> })}
                {(!about.is_empty()).then(|| view! { <p class="mk-desc">{about}</p> })}
                <div class="g-chips"><span class="g-chip">{category}</span></div>
                <div class="g-facts">
                    <span><b>{format_count(sales)}</b>{if sales == 1 { " sale" } else { " sales" }}</span>
                    {listed.map(|l| view! { <span>{l}</span> })}
                </div>
            </div>
        </article>
    }
}

// -----------------------------------------------------------------------------
// 4. Loading, failed and empty states
// -----------------------------------------------------------------------------

const EMPTY_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM8 12h8";
const FAILED_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM12 7.5v5.5M12 16.5v.01";

/// The shape of the grid while the items load, not made-up items.
fn loading_cards() -> impl IntoView {
    view! {
        <div class="g-grid mk-grid" role="status" aria-busy="true" aria-label="Loading the Marketplace">
            {(0..3).map(|_| view! { <div class="g-card g-skel-card"><div class="g-skel g-skel-media"></div><div class="g-body"><span class="g-skel w70"></span><span class="g-skel w40"></span></div></div> }).collect_view()}
        </div>
    }
}

/// The route did not answer: said plainly, never shown as an empty Marketplace.
fn failed_state(why: String, retry: Callback<()>) -> impl IntoView {
    view! {
        <div class="g-glass mk-state mk-failed" role="alert">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=FAILED_GLYPH/></svg></span>
            <div>
                <b>"The Marketplace could not load"</b>
                <p>{why}</p>
                <button type="button" class="g-btn g-btn-sm" on:click=move |_| retry.run(())>"Try again"</button>
            </div>
        </div>
    }
}

/// The Worker answered with no items: what is missing, and where to go.
fn empty_state() -> impl IntoView {
    view! {
        <div class="g-glass mk-state mk-empty">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=EMPTY_GLYPH/></svg></span>
            <div>
                <b>"Nothing is listed in the Marketplace yet"</b>
                <p>"Items show here, each with its seller and price, as soon as the Marketplace has any. The Gallery has every published Space."</p>
                <a class="g-btn g-btn-sm" href="/gallery">"Open the Gallery"</a>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 5. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = include_str!("marketplace.rs");
    const ITEM_FILE: &str = include_str!("marketplace_item.rs");
    const API_FILE: &str = include_str!("../api/marketplace.rs");
    const CSS: &str = include_str!("../../style/marketplace.css");
    const MAIN_CSS: &str = include_str!("../../style/main.css");
    const INDEX: &str = include_str!("../../index.html");
    const SEO: &str = include_str!("../seo.rs");

    /// A file's code, without its tests.
    fn code(file: &'static str) -> &'static str {
        file.split(concat!("#[cfg", "(test)]")).next().unwrap()
    }

    fn item(id: &str, name: &str, category: &str, seller: &str, price: f64, sales: u64) -> MarketplaceItem {
        MarketplaceItem {
            id: id.to_string(),
            name: name.to_string(),
            description: format!("What {name} is."),
            thumbnail_url: None,
            price_bliss: price,
            currency: "BLS".to_string(),
            creator_name: seller.to_string(),
            creator_id: format!("u-{seller}"),
            category: category.to_string(),
            sales_count: sales,
            rating: 0.0,
            is_verified: false,
            is_free: price == 0.0,
            equity_available: None,
            equity_price_per_percent: None,
            created_at: "2026-10-01T12:00:00Z".to_string(),
        }
    }

    fn sample() -> Vec<MarketplaceItem> {
        let mut a = item("a1", "Crane Rig", "models", "ada", 120.0, 31);
        a.thumbnail_url = Some("/assets/home/hero-city.webp".to_string());
        let mut b = item("b2", "Tide Script", "scripts", "ohm_lab", 0.0, 4);
        b.thumbnail_url = Some("//evil.example/x.png".to_string());
        let c = item("c3", "Hard Hat", "accessories", "kestrel", 40.0, 9);
        vec![a, b, c]
    }

    fn render(state: Load<Vec<MarketplaceItem>>) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| {
            let items = RwSignal::new(state);
            view! { <MarketBody items=items retry=Callback::new(|_| ()) /> }.to_html()
        })
    }

    fn render_card(i: MarketplaceItem) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| view! { <ItemCard item=i /> }.to_html())
    }

    /// What the old page showed that no route said.
    const STAND_INS: [&str; 10] = [
        concat!("Equity ", "Available"),
        concat!("equity in ", "this item"),
        concat!("Space", "EquityOffer"),
        concat!("projected_", "revenue"),
        concat!("star", ".svg"),
        concat!("Highest ", "Rated"),
        concat!("download", ".svg"),
        concat!("from our ", "community"),
        concat!("soul ", "scripts"),
        concat!("page-marketplace-", "industrial"),
    ];

    #[test]
    fn nothing_the_old_page_invented_comes_back() {
        for gone in STAND_INS {
            assert!(!code(FILE).contains(gone), "marketplace.rs shows {gone:?}");
            assert!(!code(ITEM_FILE).contains(gone), "marketplace_item.rs shows {gone:?}");
        }
        for (what, html) in [("full", render(Load::Ready(sample()))), ("empty", render(Load::Ready(vec![]))), ("failed", render(Load::Failed("x".into())))] {
            for gone in STAND_INS {
                assert!(!html.contains(gone), "the {what} page shows {gone:?}");
            }
            assert!(!html.to_lowercase().contains("rating"), "the {what} page shows a rating no route gives");
            assert!(!html.contains("0.0"), "the {what} page shows a made-up score");
        }
        // The rating and equity fields stay in the API shape, and this page reads neither.
        for field in [".rating", ".equity_available", ".equity_price_per_percent"] {
            assert!(!code(FILE).contains(field) && !code(ITEM_FILE).contains(field), "{field} is read");
        }
    }

    #[test]
    fn the_empty_marketplace_says_so_and_draws_nothing() {
        let html = render(Load::Ready(vec![]));
        assert!(html.contains("Nothing is listed in the Marketplace yet"), "{html}");
        assert!(html.contains("href=\"/gallery\""));
        assert_eq!(html.matches("class=\"g-glass mk-state mk-empty\"").count(), 1);
        for drawn in ["class=\"g-card", "mk-figures", "mk-controls", "<dt>", "items</p>"] {
            assert!(!html.contains(drawn), "nothing is drawn for nothing: {drawn}");
        }
        // The band says where selling happens today.
        assert!(html.contains("href=\"/projects\"") && html.contains("href=\"/docs/earning\""));
    }

    #[test]
    fn a_failed_read_says_so_and_a_loading_one_claims_nothing() {
        let failed = render(Load::Failed("Could not reach Eustress. Check your connection and try again.".into()));
        assert!(failed.contains("The Marketplace could not load") && failed.contains("Could not reach Eustress"), "{failed}");
        assert!(failed.contains("Try again") && failed.contains("role=\"alert\""));
        assert!(!failed.contains("Nothing is listed"), "a failed read is not an empty Marketplace");
        let loading = render(Load::Loading);
        assert!(loading.contains("aria-busy=\"true\"") && loading.contains("g-skel-card"));
        for claim in ["Nothing is listed", "could not load", "class=\"g-card mk-card\"", "<dt>"] {
            assert!(!loading.contains(claim), "loading claims {claim}");
        }
    }

    #[test]
    fn the_full_page_draws_the_gallerys_card_for_each_item_with_its_seller_by_username() {
        let html = render(Load::Ready(sample()));
        assert_eq!(html.matches("class=\"g-card mk-card\"").count(), 2, "the creator shelf opens first: two items");
        assert!(html.contains("<dt>Items</dt><dd>3</dd>") && html.contains("<dt>Sellers</dt><dd>3</dd>") && html.contains("<dt>Free</dt><dd>1</dd>"), "{html}");
        for (name, handle) in [("Crane Rig", ">@ada<"), ("Tide Script", ">@ohm_lab<")] {
            let at = html.find(name).unwrap_or_else(|| panic!("{name} is shown"));
            let end = at + html[at..].find("</article>").unwrap();
            assert!(html[at..end].contains(handle), "{handle} is beside {name}");
        }
        assert!(!html.contains(">ada<") && !html.contains(">ohm_lab<"), "no seller stands without the @");
        assert!(html.contains("href=\"/profile/ada\"") && html.contains("href=\"/marketplace/a1\""));
        assert!(html.contains("120 BLS") && html.contains(">Free<"));
        assert!(html.contains("31</b> sales") && html.contains("4</b> sales"));
    }

    #[test]
    fn a_picture_is_used_only_from_https_or_a_path_here_and_a_seller_is_a_letter() {
        let html = render(Load::Ready(sample()));
        assert!(html.contains("src=\"/assets/home/hero-city.webp\""));
        assert!(!html.contains("javascript:") && !html.contains("evil.example"), "{html}");
        let mut i = item("x", "X", "audio", "ada", 1.0, 1);
        for (url, ok) in [("https://cdn.eustress.dev/a.png", true), ("/assets/a.png", true), ("//evil.example/a.png", false), ("http://a.example/a.png", false), ("data:image/png;base64,AA", false), (" ", false)] {
            i.thumbnail_url = Some(url.to_string());
            assert_eq!(art_of(&i).is_some(), ok, "{url}");
        }
        i.thumbnail_url = None;
        let card = render_card(i.clone());
        assert!(card.contains("src=\"/assets/icons/audio.svg\""), "no picture: the category's icon {card}");
        // Every img on a card is an https picture or a path under /assets.
        for chunk in render(Load::Ready(sample())).split("<img ").skip(1) {
            let tag = &chunk[..chunk.find('>').unwrap()];
            assert!(tag.contains("src=\"https://") || tag.contains("src=\"/assets/"), "{tag}");
        }
        // The seller has no picture of their own here: the item page draws their first letter.
        assert_eq!(seller_of(&i).map(|s| s.2), Some("A".to_string()));
        assert_eq!(seller_of(&item("y", "Y", "audio", "  ", 1.0, 1)), None);
        // A Space or an unknown kind draws the neutral cube; no card on these pages draws a game controller.
        assert_eq!(category_icon("spaces"), "/assets/icons/cube.svg");
        assert_eq!(category_icon("holograms"), "/assets/icons/cube.svg");
        for code_part in [code(FILE), code(ITEM_FILE)] {
            assert!(!code_part.contains(concat!("game", "pad")), "a gamepad icon is named");
        }
    }

    /// What an item card's picture area draws for this picture, rendered on the server. art_ok is the signal the img's on:error clears.
    fn art_html(art: Option<&str>, art_ok: bool) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| item_picture(art.map(str::to_string), category_icon("audio"), RwSignal::new(art_ok)).to_html())
    }

    /// The img tag in a page's code that carries this attribute.
    fn art_tag(code: &str, attr: &str) -> String {
        let at = code.find(attr).unwrap_or_else(|| panic!("{attr} is not in the page"));
        let start = code[..at].rfind("<img").expect("an img tag");
        let end = start + code[start..].find("/>").expect("the tag ends");
        code[start..end].to_string()
    }

    #[test]
    fn an_item_picture_that_will_not_load_is_the_icon_an_item_without_one_draws() {
        let url = "https://api.example.dev/a/thumbnail";
        let loading = art_html(Some(url), true);
        assert!(loading.contains("class=\"mk-img\"") && loading.contains(url) && !loading.contains("mk-art-empty"), "{loading}");
        let none = art_html(None, true);
        assert!(none.contains("class=\"mk-art-empty\"") && none.contains("src=\"/assets/icons/audio.svg\""), "{none}");
        // art_ok is false once the img's on:error has run: the card then draws exactly what it draws for an item with no picture.
        let failed = art_html(Some(url), false);
        assert_eq!(failed, none, "a picture that failed shows the category icon, not the browser's broken-image icon");
        assert!(!failed.contains("mk-img") && !failed.contains(url));
        // The same through the card.
        let mut i = item("x", "X", "audio", "ada", 1.0, 1);
        i.thumbnail_url = Some(url.to_string());
        assert!(render_card(i).contains("class=\"mk-img\""));
    }

    #[test]
    fn the_item_pictures_have_an_error_handler_that_clears_their_own_signal() {
        let card = code(FILE);
        let tag = art_tag(card, "class=\"mk-img\"");
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(card.contains("move || match art.clone().filter(|_| art_ok.get())"), "the picture is drawn only while art_ok");
        // One signal for each card: a signal shared by the cards would blank every card at the first failure.
        assert!(card.contains("{item_picture(art, icon, RwSignal::new(true))}"));
        assert_eq!(card.matches("class=\"mk-img\"").count(), 1, "one picture tag on the card");
        let page = code(ITEM_FILE);
        let tag = art_tag(page, "class=\"mk-frame-img\"");
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(page.contains("{frame_picture(art, icon, RwSignal::new(true))}"));
    }

    #[test]
    fn the_shelves_categories_search_and_order_do_what_they_say() {
        let mut list = sample();
        list.push(item("d4", "Gear Pack", "models", "kestrel", 300.0, 2));
        list.push(item("e5", "Mystery", "holograms", "ada", 5.0, 50));
        let ids = |v: Vec<MarketplaceItem>| v.into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(shown(&list, Shelf::Creator, "all", "", Sort::Popular)), ["e5", "a1", "b2", "d4"]);
        assert_eq!(ids(shown(&list, Shelf::Creator, "models", "", Sort::PriceHigh)), ["d4", "a1"]);
        assert_eq!(ids(shown(&list, Shelf::Creator, "all", "", Sort::PriceLow)), ["b2", "e5", "a1", "d4"]);
        assert_eq!(ids(shown(&list, Shelf::Avatar, "all", "", Sort::Popular)), ["c3"]);
        assert_eq!(ids(shown(&list, Shelf::Creator, "all", "@KEST", Sort::Popular)), ["d4"]);
        assert_eq!(ids(shown(&list, Shelf::Creator, "all", "tide", Sort::Popular)), ["b2"]);
        assert!(shown(&list, Shelf::Avatar, "models", "", Sort::Popular).is_empty());
        assert_eq!(category_words("holograms"), "Other");
        assert_eq!(category_counts(&list, Shelf::Creator), vec![("models", "3D models", 2), ("scripts", "Scripts", 1)]);
        assert_eq!(first_shelf(&[item("z", "Z", "faces", "a", 1.0, 1)]), Shelf::Avatar);
        assert_eq!(first_shelf(&[]), Shelf::Creator);
        for s in Sort::ALL {
            assert_eq!(Sort::from_key(s.key()), s);
        }
        // The order chosen is the one drawn.
        assert!(code(FILE).contains("shown(l, shelf.get(), &category.get(), &query.get(), sort.get())"));
    }

    #[test]
    fn the_card_and_the_item_page_say_the_same_price() {
        let mut i = item("a", "A", "models", "ada", 119.6, 1);
        assert_eq!(price_words(&i), "120 BLS");
        i.is_free = true;
        assert_eq!(price_words(&i), "Free");
        assert!(code(ITEM_FILE).contains("format!(\"{:.0} BLS\", current_item.price_bliss)"), "the item page's own words");
        assert!(code(FILE).contains("format!(\"{:.0} BLS\", item.price_bliss)"));
        assert_eq!(sales_words(1), "1 sale");
        assert_eq!(sales_words(1204), "1,204 sales");
        assert_eq!(listed_words("2026-10-01T12:00:00Z", 1_791_000_000_000.0 + 86_400_000.0 * 30.0).as_deref(), Some("listed 4 weeks ago"));
        assert_eq!(listed_words("", 0.0), None);
    }

    #[test]
    fn the_money_and_identity_calls_are_the_ones_the_page_made_before() {
        // The list: the same route and the same query.
        let page = code(FILE);
        assert_eq!(page.matches("get_marketplace(&client, &query)").count(), 1);
        assert!(page.contains("tab: Some(\"creator\".to_string()),") && page.contains("limit: Some(50),") && page.contains("..Default::default()"));
        // The item and the purchase: the same calls, made only for a signed-in account.
        let item = code(ITEM_FILE);
        for needle in [
            "get_marketplace_item(&client, &item_id).await",
            "purchase_item(&client, &current_item.id).await",
            "if let AuthState::Authenticated(_user) = auth_clone.get_untracked() {",
            "params.get_untracked().get(\"item_id\").unwrap_or_default()",
            "on:click=handle_purchase",
            "disabled=is_buying",
            "<a href=\"/login\"",
            "\"Purchased successfully! Item added to your inventory.\"",
        ] {
            assert_eq!(item.matches(needle).count(), 1, "marketplace_item.rs: {needle}");
        }
        // The routes and payloads in the API file are untouched.
        for needle in [
            "let mut endpoint = \"/api/marketplace?\".to_string();",
            "let endpoint = format!(\"/api/marketplace/{}\", item_id);",
            "let endpoint = format!(\"/api/marketplace/{}/purchase\", item_id);",
            "client.post(&endpoint, &()).await",
        ] {
            assert!(API_FILE.contains(needle), "api/marketplace.rs: {needle}");
        }
        assert!(!page.contains("purchase_item") && !page.contains(".post("), "the list page buys nothing");
        assert!(!page.contains("/api/simulations"), "no Gallery listing is read here, so the Adult content setting governs nothing new");
    }

    #[test]
    fn the_copy_has_no_dashes_and_names_the_product_plainly() {
        let pages = [render(Load::Ready(sample())), render(Load::Ready(vec![])), render(Load::Failed("x".into())), render(Load::Loading)];
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!code(FILE).contains(dash) && !code(ITEM_FILE).contains(dash), "a long dash in the code");
            assert!(!CSS.contains(dash), "marketplace.css");
            for html in &pages {
                assert!(!html.contains(dash), "the rendered page");
            }
        }
        for text in pages.iter().map(|h| h.to_lowercase()).chain([code(FILE).to_lowercase(), code(ITEM_FILE).to_lowercase()]) {
            assert!(!text.contains("game engine"), "Eustress is not called a game engine");
            assert!(!text.contains("experience"), "the product noun is Simulation or Space");
        }
        let line = SEO.lines().find(|l| l.contains("(\"/marketplace\",")).expect("the Marketplace's description");
        assert!(line.contains("models, scripts, audio, textures and avatar items from creators"), "{line}");
    }

    #[test]
    fn marketplace_css_is_linked_once_and_before_topbar_css() {
        let link = "<link data-trunk rel=\"css\" href=\"style/marketplace.css\">";
        assert_eq!(INDEX.matches(link).count(), 1, "marketplace.css is linked once");
        let at = INDEX.find(link).unwrap();
        let topbar = INDEX.find("<link data-trunk rel=\"css\" href=\"style/topbar.css\">").expect("topbar.css is linked");
        assert!(at < topbar, "marketplace.css comes before topbar.css, which stays last");
        assert!(CSS.contains(".mk-hero") && CSS.contains(".mk-state") && CSS.contains("@media (max-width: 860px)"));
    }

    #[test]
    fn the_old_pages_rules_are_gone_from_main_css() {
        for gone in [concat!(".page-marketplace-", "industrial"), concat!(".marketplace-", "glow"), concat!(".marketplace-", "sidebar"), concat!(".marketplace-", "card")] {
            assert!(!MAIN_CSS.contains(&format!("{gone} ")), "{gone} is still in main.css");
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn the_marketplace_address_shows_this_page() {
        use hydration_context::{SharedContext, SsrSharedContext};
        use leptos::tachys::view::RenderHtml;
        use leptos_router::location::RequestUrl;
        let _ = leptos::task::Executor::init_futures_executor();
        let shared: std::sync::Arc<dyn SharedContext + Send + Sync> = std::sync::Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new("/marketplace"));
            crate::app::App().to_html()
        });
        owner.cleanup();
        assert!(html.contains("mk-page") && html.contains(">Marketplace</h1>"), "{html}");
        assert!(html.contains("aria-busy=\"true\""), "the prerender shows the loading shape, not an empty claim");
    }
}
