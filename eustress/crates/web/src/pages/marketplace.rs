// =============================================================================
// Eustress Web - Marketplace Page (Industrial Design)
// =============================================================================
// Browse and purchase creator content and avatar items
// Categories: Creator Content (assets, scripts, plugins) and Avatar Items
// =============================================================================

use leptos::prelude::*;
use crate::api::{self, ApiClient, MarketplaceItem as ApiMarketplaceItem, MarketplaceQuery};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Marketplace item category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemCategory {
    // Creator Content
    Models,
    Scripts,
    Plugins,
    Audio,
    Textures,
    Templates,
    Spaces,      // Open-sourced spaces that can be redistributed
    // Avatar Items
    Clothing,
    Accessories,
    Animations,
    Emotes,
    Faces,
    Bodies,
}

impl ItemCategory {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Scripts => "scripts",
            Self::Plugins => "plugins",
            Self::Audio => "audio",
            Self::Textures => "textures",
            Self::Templates => "templates",
            Self::Spaces => "spaces",
            Self::Clothing => "clothing",
            Self::Accessories => "accessories",
            Self::Animations => "animations",
            Self::Emotes => "emotes",
            Self::Faces => "faces",
            Self::Bodies => "bodies",
        }
    }
    
    fn display_name(&self) -> &'static str {
        match self {
            Self::Models => "3D Models",
            Self::Scripts => "Scripts",
            Self::Plugins => "Plugins",
            Self::Audio => "Audio",
            Self::Textures => "Textures",
            Self::Templates => "Templates",
            Self::Spaces => "Spaces",
            Self::Clothing => "Clothing",
            Self::Accessories => "Accessories",
            Self::Animations => "Animations",
            Self::Emotes => "Emotes",
            Self::Faces => "Faces",
            Self::Bodies => "Bodies",
        }
    }
    
    fn icon_path(&self) -> &'static str {
        match self {
            Self::Models => "/assets/icons/cube.svg",
            Self::Scripts => "/assets/icons/code.svg",
            Self::Plugins => "/assets/icons/puzzle.svg",
            Self::Audio => "/assets/icons/audio.svg",
            Self::Textures => "/assets/icons/image.svg",
            Self::Templates => "/assets/icons/template.svg",
            Self::Spaces => "/assets/icons/gamepad.svg",
            Self::Clothing => "/assets/icons/shirt.svg",
            Self::Accessories => "/assets/icons/sparkles.svg",
            Self::Animations => "/assets/icons/animation.svg",
            Self::Emotes => "/assets/icons/smile.svg",
            Self::Faces => "/assets/icons/face.svg",
            Self::Bodies => "/assets/icons/body.svg",
        }
    }
    
    fn is_avatar_item(&self) -> bool {
        matches!(self, 
            Self::Clothing | Self::Accessories | Self::Animations | 
            Self::Emotes | Self::Faces | Self::Bodies
        )
    }
}

/// Marketplace item.
#[derive(Clone, Debug, PartialEq)]
pub struct MarketplaceItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: ItemCategory,
    pub price: u64,  // In Bliss, 0 = free
    pub creator_name: String,
    pub thumbnail_url: Option<String>,
    pub downloads: u64,
    pub rating: f32,
    pub is_verified: bool,
    pub is_open_source: bool,        // For Spaces: allows viewing/copying source
    pub equity_available: Option<f32>, // For Spaces: percentage of equity available for sale (0.0-100.0)
    pub equity_price_per_percent: Option<u64>, // Price in Bliss per 1% equity
}

/// Space equity investment offer
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceEquityOffer {
    pub space_id: String,
    pub space_name: String,
    pub creator_name: String,
    pub total_equity_offered: f32,    // Total % being sold
    pub equity_remaining: f32,        // % still available
    pub price_per_percent: u64,       // Bliss per 1%
    pub min_investment: f32,          // Minimum % to purchase
    pub description: String,
    pub projected_revenue: Option<u64>, // Monthly projected revenue
    pub current_players: u64,
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Marketplace page - browse creator content and avatar items.
#[component]
pub fn MarketplacePage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    
    // State
    let search_query = RwSignal::new(String::new());
    let active_tab = RwSignal::new("creator".to_string()); // "creator" or "avatar"
    let selected_category = RwSignal::new("all".to_string());
    let sort_by = RwSignal::new("popular".to_string());
    
    // API data state
    let items = RwSignal::new(Vec::<MarketplaceItem>::new());
    let is_loading = RwSignal::new(true);
    
    // Convert API item to local MarketplaceItem
    fn api_to_local(item: ApiMarketplaceItem) -> MarketplaceItem {
        let category = match item.category.as_str() {
            "models" => ItemCategory::Models,
            "scripts" => ItemCategory::Scripts,
            "plugins" => ItemCategory::Plugins,
            "audio" => ItemCategory::Audio,
            "textures" => ItemCategory::Textures,
            "templates" => ItemCategory::Templates,
            "spaces" => ItemCategory::Spaces,
            "clothing" => ItemCategory::Clothing,
            "accessories" => ItemCategory::Accessories,
            "animations" => ItemCategory::Animations,
            "emotes" => ItemCategory::Emotes,
            "faces" => ItemCategory::Faces,
            "bodies" => ItemCategory::Bodies,
            _ => ItemCategory::Models,
        };
        
        MarketplaceItem {
            id: item.id,
            name: item.name,
            description: item.description,
            category,
            price: item.price_bliss as u64,
            creator_name: item.creator_name,
            thumbnail_url: item.thumbnail_url,
            downloads: item.sales_count,
            rating: item.rating,
            is_verified: item.is_verified,
            is_open_source: item.is_free,
            equity_available: item.equity_available,
            equity_price_per_percent: item.equity_price_per_percent.map(|p| p as u64),
        }
    }
    
    // Fetch marketplace data on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            let query = MarketplaceQuery {
                tab: Some("creator".to_string()),
                limit: Some(50),
                ..Default::default()
            };
            
            match api::marketplace::get_marketplace(&client, &query).await {
                Ok(response) => {
                    let local_items: Vec<MarketplaceItem> = response.items.into_iter().map(api_to_local).collect();
                    items.set(local_items);
                }
                Err(e) => {
                    log::warn!("Failed to fetch marketplace: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    });
    
    // Re-fetch when tab changes
    let api_url_tab = app_state.api_url.clone();
    let fetch_items = move |tab: String| {
        let api_url = api_url_tab.clone();
        is_loading.set(true);
        
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            let query = MarketplaceQuery {
                tab: Some(tab),
                limit: Some(50),
                ..Default::default()
            };
            
            match api::marketplace::get_marketplace(&client, &query).await {
                Ok(response) => {
                    let local_items: Vec<MarketplaceItem> = response.items.into_iter().map(api_to_local).collect();
                    items.set(local_items);
                }
                Err(e) => {
                    log::warn!("Failed to fetch marketplace: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    };
    
    // Filter items based on tab, category, and search (local filtering for instant feedback)
    let filtered_items = move || {
        let query = search_query.get().to_lowercase();
        let tab = active_tab.get();
        let category = selected_category.get();
        
        items.get()
            .into_iter()
            .filter(|item| {
                // Filter by tab (creator vs avatar)
                let matches_tab = if tab == "creator" {
                    !item.category.is_avatar_item()
                } else {
                    item.category.is_avatar_item()
                };
                
                // Filter by category
                let matches_category = category == "all" 
                    || item.category.as_str() == category;
                
                // Filter by search
                let matches_search = query.is_empty()
                    || item.name.to_lowercase().contains(&query)
                    || item.description.to_lowercase().contains(&query)
                    || item.creator_name.to_lowercase().contains(&query);
                
                matches_tab && matches_category && matches_search
            })
            .collect::<Vec<_>>()
    };
    
    // Categories are defined inline in the view to avoid closure issues
    
    view! {
        <div class="page page-marketplace-industrial">
            <CentralNav active="marketplace".to_string() />
            
            // Background
            <div class="marketplace-bg">
                <div class="marketplace-grid-overlay"></div>
                <div class="marketplace-glow glow-1"></div>
                <div class="marketplace-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="marketplace-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"MARKETPLACE"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="marketplace-title">"Creator Marketplace"</h1>
                <p class="marketplace-subtitle">"Assets, soul scripts, and avatar items from our community"</p>
                
                // Tab Switcher
                <div class="tab-switcher">
                    <button 
                        class="tab-btn"
                        class:active=move || active_tab.get() == "creator"
                        on:click=move |_| {
                            active_tab.set("creator".to_string());
                            selected_category.set("all".to_string());
                        }
                    >
                        <img src="/assets/icons/cube.svg" alt="Creator" class="tab-icon" />
                        "Creator Content"
                    </button>
                    <button 
                        class="tab-btn"
                        class:active=move || active_tab.get() == "avatar"
                        on:click=move |_| {
                            active_tab.set("avatar".to_string());
                            selected_category.set("all".to_string());
                        }
                    >
                        <img src="/assets/icons/user.svg" alt="Avatar" class="tab-icon" />
                        "Avatar Items"
                    </button>
                </div>
                
                // Search Box
                <div class="search-box">
                    <img src="/assets/icons/search.svg" alt="Search" class="search-icon" />
                    <input 
                        type="text"
                        class="search-input-industrial"
                        placeholder="Search items, creators..."
                        prop:value=move || search_query.get()
                        on:input=move |e| search_query.set(event_target_value(&e))
                    />
                </div>
            </section>
            
            // Main Content
            <div class="marketplace-content">
                // Sidebar Categories
                <aside class="marketplace-sidebar">
                    <div class="sidebar-section">
                        <h3 class="sidebar-title">"Categories"</h3>
                        <button 
                            class="category-btn"
                            class:active=move || selected_category.get() == "all"
                            on:click=move |_| selected_category.set("all".to_string())
                        >
                            <img src="/assets/icons/grid.svg" alt="All" class="cat-icon" />
                            "All Items"
                        </button>
                        
                        // Creator Content Categories
                        <Show when=move || active_tab.get() == "creator">
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "models"
                                on:click=move |_| selected_category.set("models".to_string())
                            >
                                <img src="/assets/icons/cube.svg" alt="Models" class="cat-icon" />
                                "3D Models"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "scripts"
                                on:click=move |_| selected_category.set("scripts".to_string())
                            >
                                <img src="/assets/icons/code.svg" alt="Scripts" class="cat-icon" />
                                "Scripts"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "plugins"
                                on:click=move |_| selected_category.set("plugins".to_string())
                            >
                                <img src="/assets/icons/puzzle.svg" alt="Plugins" class="cat-icon" />
                                "Plugins"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "audio"
                                on:click=move |_| selected_category.set("audio".to_string())
                            >
                                <img src="/assets/icons/audio.svg" alt="Audio" class="cat-icon" />
                                "Audio"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "textures"
                                on:click=move |_| selected_category.set("textures".to_string())
                            >
                                <img src="/assets/icons/image.svg" alt="Textures" class="cat-icon" />
                                "Textures"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "templates"
                                on:click=move |_| selected_category.set("templates".to_string())
                            >
                                <img src="/assets/icons/template.svg" alt="Templates" class="cat-icon" />
                                "Templates"
                            </button>
                        </Show>
                        
                        // Avatar Items Categories
                        <Show when=move || active_tab.get() == "avatar">
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "clothing"
                                on:click=move |_| selected_category.set("clothing".to_string())
                            >
                                <img src="/assets/icons/shirt.svg" alt="Clothing" class="cat-icon" />
                                "Clothing"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "accessories"
                                on:click=move |_| selected_category.set("accessories".to_string())
                            >
                                <img src="/assets/icons/sparkles.svg" alt="Accessories" class="cat-icon" />
                                "Accessories"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "animations"
                                on:click=move |_| selected_category.set("animations".to_string())
                            >
                                <img src="/assets/icons/animation.svg" alt="Animations" class="cat-icon" />
                                "Animations"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "emotes"
                                on:click=move |_| selected_category.set("emotes".to_string())
                            >
                                <img src="/assets/icons/smile.svg" alt="Emotes" class="cat-icon" />
                                "Emotes"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "faces"
                                on:click=move |_| selected_category.set("faces".to_string())
                            >
                                <img src="/assets/icons/face.svg" alt="Faces" class="cat-icon" />
                                "Faces"
                            </button>
                            <button 
                                class="category-btn"
                                class:active=move || selected_category.get() == "bodies"
                                on:click=move |_| selected_category.set("bodies".to_string())
                            >
                                <img src="/assets/icons/body.svg" alt="Bodies" class="cat-icon" />
                                "Bodies"
                            </button>
                        </Show>
                    </div>
                    
                    <div class="sidebar-section">
                        <h3 class="sidebar-title">"Sort By"</h3>
                        <select 
                            class="sort-select-industrial"
                            prop:value=move || sort_by.get()
                            on:change=move |e| sort_by.set(event_target_value(&e))
                        >
                            <option value="popular">"Most Popular"</option>
                            <option value="recent">"Recently Added"</option>
                            <option value="price-low">"Price: Low to High"</option>
                            <option value="price-high">"Price: High to Low"</option>
                            <option value="rating">"Highest Rated"</option>
                        </select>
                    </div>
                </aside>
                
                // Items Grid
                <main class="marketplace-main">
                    <div class="results-header">
                        <span class="results-count">
                            {move || format!("{} items", filtered_items().len())}
                        </span>
                    </div>
                    
                    <Show
                        when=move || !filtered_items().is_empty()
                        fallback=|| view! {
                            <div class="no-results-industrial">
                                <img src="/assets/icons/search.svg" alt="No results" class="no-results-icon" />
                                <h3>"No items found"</h3>
                                <p>"Try adjusting your search or filters"</p>
                            </div>
                        }
                    >
                        <div class="items-grid">
                            <For
                                each=filtered_items
                                key=|item| item.id.clone()
                                children=move |item| view! { <MarketplaceCard item=item /> }
                            />
                        </div>
                    </Show>
                </main>
            </div>
            
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Item Card Component
// -----------------------------------------------------------------------------

#[component]
fn MarketplaceCard(item: MarketplaceItem) -> impl IntoView {
    let item_url = format!("/marketplace/{}", item.id);
    let price_display = if item.price == 0 {
        "Free".to_string()
    } else {
        format!("{} Bliss", item.price)
    };
    
    view! {
        <a href=item_url class="marketplace-card">
            <div class="card-thumbnail">
                <img src=item.category.icon_path() alt="Item" class="thumbnail-icon" />
                {if item.is_verified {
                    Some(view! {
                        <div class="verified-badge">
                            <img src="/assets/icons/check.svg" alt="Verified" class="badge-icon" />
                        </div>
                    })
                } else {
                    None
                }}
            </div>
            <div class="card-body">
                <span class="card-category">{item.category.display_name()}</span>
                <h3 class="card-name">{item.name}</h3>
                <p class="card-description">{item.description}</p>
                <div class="card-meta">
                    <div class="creator-info">
                        <img src="/assets/icons/user.svg" alt="Creator" class="creator-icon-sm" />
                        <span>{item.creator_name}</span>
                    </div>
                    <div class="card-rating">
                        <img src="/assets/icons/star.svg" alt="Rating" class="rating-icon" />
                        <span>{format!("{:.1}", item.rating)}</span>
                    </div>
                </div>
                <div class="card-footer">
                    <span class="card-downloads">
                        <img src="/assets/icons/download.svg" alt="Downloads" class="stat-icon-sm" />
                        {format_number(item.downloads)}
                    </span>
                    <span class="card-price" class:free=item.price == 0>
                        {price_display}
                    </span>
                </div>
            </div>
        </a>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
