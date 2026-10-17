// =============================================================================
// Eustress Web - A Marketplace item's page (/marketplace/:id)
// =============================================================================
// One item in the layout of a Space's page (gallery.css): its picture in the frame, its name, its seller by @username, the
// price and the purchase beside it, and what the seller wrote under it. Every word and number is the Worker's:
//
//   GET  /api/marketplace/{id}            the item (api/marketplace.rs get_marketplace_item)
//   POST /api/marketplace/{id}/purchase   the purchase, for a signed-in account (purchase_item)
//
// The reading of the address, the item, the purchase and the words the purchase shows are the page's own and are not changed
// here; only the layout around them is. The equity panel this page used to show is gone: it offered a share of an item's sales
// that no route sells. An item with no picture draws its category's icon; the seller is a letter, never a picture from another
// host.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use crate::api::{ApiClient, marketplace::{MarketplaceItem, get_marketplace_item, purchase_item}};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::utils::format_count;
use super::marketplace::{art_of, category_icon, category_words, item_name, seller_of};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Marketplace item detail page: shows full item info with purchase flow.
#[component]
pub fn MarketplaceItemPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let params = use_params_map();

    // Item state
    let item = RwSignal::new(Option::<MarketplaceItem>::None);
    let is_loading = RwSignal::new(true);
    let error_message = RwSignal::new(Option::<String>::None);
    let purchase_loading = RwSignal::new(false);
    let purchase_success = RwSignal::new(false);

    // Fetch item on mount
    {
        let api_url = app_state.api_url.clone();
        spawn_local(async move {
            let item_id = params.get_untracked().get("item_id").unwrap_or_default();
            if item_id.is_empty() {
                error_message.set(Some("No item ID provided".to_string()));
                is_loading.set(false);
                return;
            }
            let client = ApiClient::new(&api_url);
            match get_marketplace_item(&client, &item_id).await {
                Ok(fetched) => {
                    item.set(Some(fetched));
                    is_loading.set(false);
                }
                Err(error) => {
                    error_message.set(Some(format!("Failed to load item: {}", error)));
                    is_loading.set(false);
                }
            }
        });
    }

    // Store api_url in a StoredValue so closures capture only Copy types
    let api_url_stored = StoredValue::new(app_state.api_url.clone());

    // Purchase handler
    let handle_purchase = move |_| {
        let api_url = api_url_stored.get_value();
        let auth_clone = auth;
        purchase_loading.set(true);
        spawn_local(async move {
            if let AuthState::Authenticated(_user) = auth_clone.get_untracked() {
                if let Some(current_item) = item.get_untracked() {
                    let client = ApiClient::new(&api_url);
                    match purchase_item(&client, &current_item.id).await {
                        Ok(_) => {
                            purchase_success.set(true);
                            purchase_loading.set(false);
                        }
                        Err(error) => {
                            error_message.set(Some(format!("Purchase failed: {}", error)));
                            purchase_loading.set(false);
                        }
                    }
                }
            }
        });
    };

    view! {
        <div class="page g-page mk-page mk-item-page">
            <CentralNav active="marketplace".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                <div class="g-wrap mk-item">
                    <nav class="g-crumbs" aria-label="Breadcrumb">
                        <a href="/marketplace">"Marketplace"</a>
                        <span aria-hidden="true">"/"</span>
                        <span class="g-crumb-now">
                            {move || item.get().map(|i| i.name.clone()).unwrap_or_else(|| "Item".to_string())}
                        </span>
                    </nav>

                    {move || {
                        if is_loading.get() {
                            item_loading().into_any()
                        } else if let Some(error) = error_message.get() {
                            item_failed(item.with(|i| i.is_some()), error).into_any()
                        } else if let Some(current_item) = item.get() {
                            let price_display = if current_item.is_free {
                                "Free".to_string()
                            } else {
                                format!("{:.0} BLS", current_item.price_bliss)
                            };

                            view! {
                                <ItemDetail item=current_item price=price_display>
                                    {move || {
                                        if purchase_success.get() {
                                            view! {
                                                <div class="purchase-success mk-bought" role="status">
                                                    <span class="success-icon" aria-hidden="true">"+"</span>
                                                    <span>"Purchased successfully! Item added to your inventory."</span>
                                                </div>
                                            }.into_any()
                                        } else {
                                            let is_authed = matches!(auth.get(), AuthState::Authenticated(_));
                                            let is_buying = purchase_loading.get();
                                            view! {
                                                <div class="purchase-actions mk-buy-actions">
                                                    {if is_authed {
                                                        view! {
                                                            <button
                                                                class="g-btn g-btn-primary g-btn-lg purchase-btn"
                                                                on:click=handle_purchase
                                                                disabled=is_buying
                                                            >
                                                                {if is_buying { "Processing..." } else { "Purchase" }}
                                                            </button>
                                                        }.into_any()
                                                    } else {
                                                        view! {
                                                            <a href="/login" class="g-btn g-btn-primary g-btn-lg purchase-btn">
                                                                "Sign In to Purchase"
                                                            </a>
                                                        }.into_any()
                                                    }}
                                                </div>
                                            }.into_any()
                                        }
                                    }}
                                </ItemDetail>
                            }.into_any()
                        } else {
                            item_missing().into_any()
                        }
                    }}
                </div>
            </main>

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// The item
// -----------------------------------------------------------------------------

/// The item's picture in the frame, or its category's icon when it has none. The Worker answers 404 for art it will not serve
/// (animated, transparent, several images, not an image, too large), so a picture that fails to load clears art_ok and the frame
/// draws the same icon as an item with no picture, never the browser's broken-image icon.
fn frame_picture(art: Option<String>, icon: String, art_ok: RwSignal<bool>) -> impl IntoView {
    move || match art.clone().filter(|_| art_ok.get()) {
        Some(src) => view! { <img class="mk-frame-img" src=src alt="" decoding="async" on:error=move |_| art_ok.set(false) /> }.into_any(),
        None => view! { <span class="mk-art-empty mk-art-big" aria-hidden="true"><img src=icon.clone() alt="" /></span> }.into_any(),
    }
}

/// The item in the layout of a Space's page. `price` is the page's own price line and `children` its purchase block.
#[component]
pub fn ItemDetail(item: MarketplaceItem, price: String, children: Children) -> impl IntoView {
    let name = item_name(&item);
    let art = art_of(&item);
    let icon = category_icon(&item.category);
    let category = category_words(&item.category);
    let seller = seller_of(&item);
    let about = item.description.trim().to_string();
    let listed = crate::time_words::when_iso(item.created_at.trim());
    let currency = item.currency.trim().to_string();
    view! {
        <div class="g-lhead mk-lhead">
            <div class="g-lmedia">
                <div class="g-frame">
                    <span class="g-bracket tl"></span><span class="g-bracket tr"></span><span class="g-bracket bl"></span><span class="g-bracket br"></span>
                    {frame_picture(art, icon, RwSignal::new(true))}
                </div>
            </div>
            <aside class="g-lside">
                <div class="g-chips">
                    <span class="g-chip">{category}</span>
                    {item.is_verified.then(|| view! { <span class="g-chip live">"Verified"</span> })}
                </div>
                <h1 class="g-ltitle">{name}</h1>
                {seller.map(|(handle, href, letter)| view! {
                    <a class="g-maker" href=href><span class="g-av" aria-hidden="true">{letter}</span>{format!("by {handle}")}</a>
                })}
                <div class="g-glass g-lpanel mk-buy">
                    <span class="mk-buy-label">"Price"</span>
                    <span class="mk-buy-price" class:free=item.is_free>{price}</span>
                    {children()}
                </div>
            </aside>
            <div class="g-lcopy">
                <section class="g-csec" aria-labelledby="mk-about-h">
                    <h2 id="mk-about-h">"About this item"</h2>
                    {if about.is_empty() {
                        view! { <p class="g-about">"The seller wrote no description."</p> }.into_any()
                    } else {
                        view! { <p class="g-about">{about}</p> }.into_any()
                    }}
                </section>
                <section class="g-csec" aria-labelledby="mk-details-h">
                    <h2 id="mk-details-h">"Details"</h2>
                    <dl class="g-glass g-kv">
                        <dt>"Sales"</dt><dd class="g-mono">{format_count(item.sales_count)}</dd>
                        {(!currency.is_empty()).then(|| view! { <dt>"Currency"</dt><dd class="g-mono">{currency}</dd> })}
                        {(!listed.is_empty()).then(|| view! { <dt>"Listed"</dt><dd>{listed}</dd> })}
                        <dt>"Item ID"</dt><dd class="g-mono">{item.id.clone()}</dd>
                    </dl>
                </section>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Loading, failed and missing
// -----------------------------------------------------------------------------

const FAILED_GLYPH: &str = "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM12 7.5v5.5M12 16.5v.01";

/// The shape of the page while the item loads.
fn item_loading() -> impl IntoView {
    view! {
        <div class="g-lhead" role="status" aria-busy="true" aria-label="Loading the item">
            <h1 class="g-sr">"Marketplace item"</h1>
            <div class="g-lmedia"><div class="g-skel g-skel-frame"></div></div>
            <div class="g-lside"><span class="g-skel w80 tall"></span><span class="g-skel w40"></span><div class="g-skel g-skel-panel"></div></div>
        </div>
    }
}

/// What went wrong: the item could not be read, or (with the item on the page) the purchase did not go through. The page's
/// own sentence is shown as it is.
fn item_failed(had_item: bool, error: String) -> impl IntoView {
    let title = if had_item { "The purchase did not go through" } else { "This item could not be loaded" };
    view! {
        <div class="g-glass mk-state mk-failed" role="alert">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=FAILED_GLYPH/></svg></span>
            <div>
                <h1 class="mk-state-title">{title}</h1>
                <p>{error}</p>
                <a href="/marketplace" class="g-btn g-btn-sm">"Back to the Marketplace"</a>
            </div>
        </div>
    }
}

/// The Worker answered with no item.
fn item_missing() -> impl IntoView {
    view! {
        <div class="g-glass mk-state mk-empty">
            <span class="g-glyph" aria-hidden="true"><svg viewBox="0 0 24 24"><path d=FAILED_GLYPH/></svg></span>
            <div>
                <h1 class="mk-state-title">"Item not found"</h1>
                <p>"This item may have been removed or is no longer available."</p>
                <a href="/marketplace" class="g-btn g-btn-sm">"Back to the Marketplace"</a>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    fn item(seller: &str, thumb: Option<&str>) -> MarketplaceItem {
        MarketplaceItem {
            id: "a1".to_string(),
            name: "Crane Rig".to_string(),
            description: "A rigged harbour crane.".to_string(),
            thumbnail_url: thumb.map(str::to_string),
            price_bliss: 120.0,
            currency: "BLS".to_string(),
            creator_name: seller.to_string(),
            creator_id: "0f3c".to_string(),
            category: "models".to_string(),
            sales_count: 1204,
            rating: 4.5,
            is_verified: true,
            is_free: false,
            equity_available: Some(12.0),
            equity_price_per_percent: Some(90.0),
            created_at: "2026-10-01T12:00:00Z".to_string(),
        }
    }

    fn render(i: MarketplaceItem) -> String {
        let owner = Owner::new();
        owner.with(|| view! { <ItemDetail item=i price="120 BLS".to_string()><button class="probe">"Purchase"</button></ItemDetail> }.to_html())
    }

    fn html_of(view: impl RenderHtml) -> String {
        let owner = Owner::new();
        owner.with(|| view.to_html())
    }

    #[test]
    fn the_item_page_shows_the_seller_by_username_and_a_letter_never_a_picture_from_elsewhere() {
        let html = render(item("ada", Some("https://evil.example/x.png\" onerror=\"x")));
        assert!(html.contains("href=\"/profile/ada\"") && html.contains("by @ada"), "{html}");
        assert!(html.contains("class=\"g-av\">A</span>") && !html.contains("<img class=\"g-av\""), "the seller is a letter: {html}");
        assert!(html.contains("class=\"probe\""), "the page's purchase block sits in the panel");
        assert!(html.contains("<h1 class=\"g-ltitle\">Crane Rig</h1>") && html.contains("1,204"));
        let bad = render(item("ada", Some("//evil.example/x.png")));
        assert!(!bad.contains("evil.example") && bad.contains("src=\"/assets/icons/cube.svg\""), "{bad}");
        let ok = render(item("ada", Some("/assets/home/hero-city.webp")));
        assert!(ok.contains("src=\"/assets/home/hero-city.webp\""));
    }

    /// What the frame draws for this picture, rendered on the server. art_ok is the signal the img's on:error clears.
    fn art_html(art: Option<&str>, art_ok: bool) -> String {
        let owner = Owner::new();
        owner.with(|| frame_picture(art.map(str::to_string), category_icon("models"), RwSignal::new(art_ok)).to_html())
    }

    #[test]
    fn a_frame_picture_that_will_not_load_is_the_icon_an_item_without_one_draws() {
        let url = "https://api.example.dev/a/thumbnail";
        let loading = art_html(Some(url), true);
        assert!(loading.contains("class=\"mk-frame-img\"") && loading.contains(url) && !loading.contains("mk-art-empty"), "{loading}");
        let none = art_html(None, true);
        assert!(none.contains("class=\"mk-art-empty mk-art-big\"") && none.contains("src=\"/assets/icons/cube.svg\""), "{none}");
        // art_ok is false once the img's on:error has run: the frame then draws exactly what it draws for an item with no picture.
        let failed = art_html(Some(url), false);
        assert_eq!(failed, none, "a picture that failed shows the category icon, not the browser's broken-image icon");
        assert!(!failed.contains("mk-frame-img") && !failed.contains(url));
        // The same through the page's own component.
        assert!(render(item("ada", Some(url))).contains("class=\"mk-frame-img\""));
    }

    #[test]
    fn the_frame_picture_has_an_error_handler_that_clears_its_own_signal() {
        let page = include_str!("marketplace_item.rs").split(concat!("#[cfg", "(test)]")).next().unwrap();
        let at = page.find("class=\"mk-frame-img\"").expect("the frame's picture");
        let start = page[..at].rfind("<img").expect("an img tag");
        let tag = &page[start..start + page[start..].find("/>").expect("the tag ends")];
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(page.contains("move || match art.clone().filter(|_| art_ok.get())"), "the picture is drawn only while art_ok");
        assert!(page.contains("{frame_picture(art, icon, RwSignal::new(true))}"));
        assert_eq!(page.matches("class=\"mk-frame-img\"").count(), 1, "one picture tag in the frame");
    }

    #[test]
    fn the_item_page_shows_no_rating_and_no_equity_offer() {
        let html = render(item("ada", None));
        for gone in ["4.5", "Rating", "Equity", "equity", "12.0%", "90 BLS"] {
            assert!(!html.contains(gone), "the item page shows {gone:?}");
        }
        assert!(!html.contains('\u{2014}') && !html.contains('\u{2013}'));
    }

    #[test]
    fn each_state_of_the_item_page_says_what_it_is() {
        let loading = html_of(item_loading());
        assert!(loading.contains("aria-busy=\"true\"") && !loading.contains("not"));
        let failed = html_of(item_failed(false, "No item ID provided".to_string()));
        assert!(failed.contains("This item could not be loaded") && failed.contains("No item ID provided") && failed.contains("role=\"alert\""));
        let bought = html_of(item_failed(true, "Purchase failed: Unauthorized".to_string()));
        assert!(bought.contains("The purchase did not go through") && bought.contains("Purchase failed: Unauthorized"));
        let missing = html_of(item_missing());
        assert!(missing.contains("Item not found") && missing.contains("href=\"/marketplace\""));
        for html in [loading, failed, bought, missing] {
            assert!(html.contains("<h1"), "every state names the page for the tab title");
            assert!(!html.contains('\u{2014}') && !html.contains('\u{2013}'));
        }
    }
}
