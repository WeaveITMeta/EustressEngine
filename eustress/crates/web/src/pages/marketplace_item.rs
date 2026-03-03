// =============================================================================
// Eustress Web - Marketplace Item Detail Page (Industrial Design)
// =============================================================================
// Individual item view with thumbnail, description, creator info,
// purchase/download button, ratings, and related items.
// Route: /marketplace/:item_id
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use crate::api::{ApiClient, marketplace::{MarketplaceItem, get_marketplace_item, purchase_item}};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Marketplace item detail page — shows full item info with purchase flow.
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
        <div class="page page-marketplace-item-industrial">
            <CentralNav active="marketplace".to_string() />

            // Background
            <div class="item-bg">
                <div class="item-grid-overlay"></div>
                <div class="item-glow glow-1"></div>
            </div>

            // Breadcrumb
            <nav class="item-breadcrumb">
                <a href="/marketplace">"Marketplace"</a>
                <span class="separator">"/"</span>
                <span class="current">
                    {move || item.get().map(|i| i.name.clone()).unwrap_or_else(|| "Item".to_string())}
                </span>
            </nav>

            // Loading state
            {move || {
                if is_loading.get() {
                    view! {
                        <div class="item-loading">
                            <div class="loading-spinner"></div>
                            <p>"Loading item details..."</p>
                        </div>
                    }.into_any()
                } else if let Some(error) = error_message.get() {
                    view! {
                        <div class="item-error">
                            <h2>"Error"</h2>
                            <p>{error}</p>
                            <a href="/marketplace" class="btn-secondary-steel">"Back to Marketplace"</a>
                        </div>
                    }.into_any()
                } else if let Some(current_item) = item.get() {
                    let price_display = if current_item.is_free {
                        "Free".to_string()
                    } else {
                        format!("{:.0} BLS", current_item.price_bliss)
                    };
                    let rating_stars = format!("{:.1}", current_item.rating);
                    let has_equity = current_item.equity_available.is_some();

                    let item_name = current_item.name.clone();
                    let item_category = current_item.category.clone();
                    let creator_name = current_item.creator_name.clone();
                    let creator_link = format!("/profile/{}", current_item.creator_id);
                    let sales_display = format!("{}", current_item.sales_count);
                    let currency_display = current_item.currency.clone();
                    let description = current_item.description.clone();
                    let item_id_display = current_item.id.clone();
                    let created_display = current_item.created_at.clone();
                    let thumbnail = current_item.thumbnail_url.clone();
                    let is_verified = current_item.is_verified;

                    view! {
                        <div class="item-detail">
                            // Main content layout
                            <div class="item-layout">
                                // Left column — preview
                                <div class="item-preview">
                                    <div class="preview-container">
                                        {if let Some(thumb_url) = thumbnail {
                                            view! {
                                                <img src={thumb_url} alt={item_name.clone()} class="preview-image" />
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class="preview-placeholder">
                                                    <div class="placeholder-icon">"3D"</div>
                                                </div>
                                            }.into_any()
                                        }}
                                    </div>
                                </div>

                                // Right column — info and purchase
                                <div class="item-info">
                                    // Title and badges
                                    <div class="info-header">
                                        <h1 class="item-name">{item_name}</h1>
                                        <div class="item-badges">
                                            {if is_verified {
                                                view! { <span class="badge verified">"Verified"</span> }.into_any()
                                            } else {
                                                view! { <span></span> }.into_any()
                                            }}
                                            <span class="badge category">{item_category}</span>
                                        </div>
                                    </div>

                                    // Creator
                                    <div class="item-creator">
                                        <span class="creator-label">"By"</span>
                                        <a href=creator_link class="creator-name">
                                            {creator_name}
                                        </a>
                                    </div>

                                    // Stats row
                                    <div class="item-stats">
                                        <div class="stat">
                                            <span class="stat-value">{rating_stars}</span>
                                            <span class="stat-label">"Rating"</span>
                                        </div>
                                        <div class="stat-sep"></div>
                                        <div class="stat">
                                            <span class="stat-value">{sales_display}</span>
                                            <span class="stat-label">"Sales"</span>
                                        </div>
                                        <div class="stat-sep"></div>
                                        <div class="stat">
                                            <span class="stat-value">{currency_display}</span>
                                            <span class="stat-label">"Currency"</span>
                                        </div>
                                    </div>

                                    // Description
                                    <div class="item-description">
                                        <h3>"Description"</h3>
                                        <p>{description}</p>
                                    </div>

                                    // Equity section (if available)
                                    {if has_equity {
                                        let equity_pct = current_item.equity_available.unwrap_or(0.0);
                                        let equity_price = current_item.equity_price_per_percent.unwrap_or(0.0);
                                        view! {
                                            <div class="item-equity">
                                                <h3>"Equity Available"</h3>
                                                <div class="equity-info">
                                                    <span class="equity-pct">{format!("{:.1}%", equity_pct)}</span>
                                                    <span class="equity-price">{format!("{:.0} BLS per %", equity_price)}</span>
                                                </div>
                                                <p class="equity-desc">"Purchase equity in this item to earn a share of future sales revenue."</p>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }}

                                    // Purchase section
                                    <div class="purchase-section">
                                        <div class="price-display">
                                            <span class="price-value">{price_display}</span>
                                        </div>

                                        {move || {
                                            if purchase_success.get() {
                                                view! {
                                                    <div class="purchase-success">
                                                        <span class="success-icon">"+"</span>
                                                        <span>"Purchased successfully! Item added to your inventory."</span>
                                                    </div>
                                                }.into_any()
                                            } else {
                                                let is_authed = matches!(auth.get(), AuthState::Authenticated(_));
                                                let is_buying = purchase_loading.get();
                                                view! {
                                                    <div class="purchase-actions">
                                                        {if is_authed {
                                                            view! {
                                                                <button
                                                                    class="btn-primary-steel purchase-btn"
                                                                    on:click=handle_purchase
                                                                    disabled=is_buying
                                                                >
                                                                    {if is_buying { "Processing..." } else { "Purchase" }}
                                                                </button>
                                                            }.into_any()
                                                        } else {
                                                            view! {
                                                                <a href="/login" class="btn-primary-steel purchase-btn">
                                                                    "Sign In to Purchase"
                                                                </a>
                                                            }.into_any()
                                                        }}
                                                    </div>
                                                }.into_any()
                                            }
                                        }}
                                    </div>

                                    // Metadata
                                    <div class="item-metadata">
                                        <div class="meta-row">
                                            <span class="meta-label">"Created"</span>
                                            <span class="meta-value">{created_display}</span>
                                        </div>
                                        <div class="meta-row">
                                            <span class="meta-label">"Item ID"</span>
                                            <span class="meta-value mono">{item_id_display}</span>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="item-error">
                            <h2>"Item Not Found"</h2>
                            <p>"This item may have been removed or is no longer available."</p>
                            <a href="/marketplace" class="btn-secondary-steel">"Back to Marketplace"</a>
                        </div>
                    }.into_any()
                }
            }}

            <Footer />
        </div>
    }
}
