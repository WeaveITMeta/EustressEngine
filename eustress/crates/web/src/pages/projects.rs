// =============================================================================
// Eustress Web - Projects Page (Industrial Redesign)
// =============================================================================
// User's creative hub - manage all spaces, games, and experiences
// =============================================================================

use leptos::prelude::*;
use crate::api::{ApiClient};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Project/Place status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpaceStatus {
    Published,
    Draft,
    Archived,
    UnderReview,
}

impl SpaceStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Archived => "archived",
            Self::UnderReview => "review",
        }
    }
    
    fn display_name(&self) -> &'static str {
        match self {
            Self::Published => "Published",
            Self::Draft => "Draft",
            Self::Archived => "Archived",
            Self::UnderReview => "Under Review",
        }
    }
}

/// User's place/project.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    pub status: SpaceStatus,
    pub visits: u64,
    pub favorites: u64,
    pub max_players: u32,
    pub genre: String,
    pub updated_at: String,
    pub created_at: String,
    pub is_public: bool,
}

/// API response types
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<ProjectData>,
    pub total: u64,
    pub page: u32,
    pub limit: u32,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ProjectData {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub status: String,
    pub genre: String,
    pub max_players: u32,
    pub is_public: bool,
    pub version: u32,
    pub play_count: u64,
    pub favorite_count: u64,
    pub last_edited: String,
    pub created_at: String,
    pub published_at: Option<String>,
    pub storage_url: Option<String>,
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Projects page - manage all user's spaces and creations.
#[component]
pub fn ProjectsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    
    // State
    let search_query = RwSignal::new(String::new());
    let filter_status = RwSignal::new("all".to_string());
    let sort_by = RwSignal::new("updated".to_string());
    let view_mode = RwSignal::new("grid".to_string()); // "grid" or "list"
    let active_menu = RwSignal::new(None::<String>); // Track which context menu is open
    
    // API data state
    let spaces = RwSignal::new(Vec::<Place>::new());
    let is_loading = RwSignal::new(true);
    
    // Fetch projects from API on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            // Fetch user's projects
            match client.get::<ProjectsResponse>("/api/projects").await {
                Ok(response) => {
                    let places: Vec<Place> = response.projects.into_iter().map(|p| {
                        let status = match p.status.as_str() {
                            "published" => SpaceStatus::Published,
                            "draft" => SpaceStatus::Draft,
                            "archived" => SpaceStatus::Archived,
                            _ => SpaceStatus::Draft,
                        };
                        Place {
                            id: p.id,
                            name: p.name,
                            description: p.description.unwrap_or_default(),
                            thumbnail_url: p.thumbnail_url,
                            status,
                            visits: p.play_count,
                            favorites: p.favorite_count,
                            max_players: p.max_players,
                            genre: p.genre,
                            updated_at: p.last_edited,
                            created_at: p.created_at,
                            is_public: p.is_public,
                        }
                    }).collect();
                    spaces.set(places);
                }
                Err(e) => {
                    log::warn!("Failed to fetch projects: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    });
    
    // Filter spaces
    let filtered_spaces = move || {
        let query = search_query.get().to_lowercase();
        let status = filter_status.get();
        
        spaces.get()
            .into_iter()
            .filter(|place| {
                let matches_search = query.is_empty()
                    || place.name.to_lowercase().contains(&query)
                    || place.description.to_lowercase().contains(&query)
                    || place.genre.to_lowercase().contains(&query);
                
                let matches_status = status == "all"
                    || place.status.as_str() == status;
                
                matches_search && matches_status
            })
            .collect::<Vec<_>>()
    };
    
    // Stats
    let total_visits = move || spaces.get().iter().map(|p| p.visits).sum::<u64>();
    let total_favorites = move || spaces.get().iter().map(|p| p.favorites).sum::<u64>();
    let published_count = move || spaces.get().iter().filter(|p| p.status == SpaceStatus::Published).count();
    
    view! {
        <div class="page page-projects-industrial">
            <CentralNav active="projects".to_string() />
            
            // Background
            <div class="projects-bg">
                <div class="projects-grid-overlay"></div>
                <div class="projects-glow glow-1"></div>
                <div class="projects-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="projects-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"MY PROJECTS"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="projects-title">"Your Creations"</h1>
                <p class="projects-subtitle">"Manage, customize, and publish your spaces"</p>
                
                // Stats Row
                <div class="stats-row">
                    <div class="stat-box">
                        <img src="/assets/icons/gamepad.svg" alt="Spaces" class="stat-icon" />
                        <div class="stat-content">
                            <span class="stat-value">{move || spaces.get().len()}</span>
                            <span class="stat-label">"Total Spaces"</span>
                        </div>
                    </div>
                    <div class="stat-box">
                        <img src="/assets/icons/play.svg" alt="Visits" class="stat-icon" />
                        <div class="stat-content">
                            <span class="stat-value">{move || format_number(total_visits())}</span>
                            <span class="stat-label">"Total Visits"</span>
                        </div>
                    </div>
                    <div class="stat-box">
                        <img src="/assets/icons/heart.svg" alt="Favorites" class="stat-icon" />
                        <div class="stat-content">
                            <span class="stat-value">{move || format_number(total_favorites())}</span>
                            <span class="stat-label">"Favorites"</span>
                        </div>
                    </div>
                    <div class="stat-box">
                        <img src="/assets/icons/check.svg" alt="Published" class="stat-icon" />
                        <div class="stat-content">
                            <span class="stat-value">{published_count}</span>
                            <span class="stat-label">"Published"</span>
                        </div>
                    </div>
                </div>
                
                // New Project Button - Links to download page
                <a href="/download" class="btn-create-new">
                    <img src="/assets/icons/download.svg" alt="Download" class="btn-icon" />
                    "Download Studio"
                </a>
            </section>
            
            // Toolbar
            <section class="projects-toolbar">
                <div class="toolbar-left">
                    <div class="search-box">
                        <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="11" cy="11" r="8"></circle>
                            <path d="m21 21-4.3-4.3"></path>
                        </svg>
                        <input 
                            type="text"
                            class="search-input-industrial"
                            placeholder="Search your spaces..."
                            prop:value=move || search_query.get()
                            on:input=move |e| search_query.set(event_target_value(&e))
                        />
                    </div>
                    
                    <div class="filter-chips">
                        <button 
                            class="chip"
                            class:active=move || filter_status.get() == "all"
                            on:click=move |_| filter_status.set("all".to_string())
                        >"All"</button>
                        <button 
                            class="chip"
                            class:active=move || filter_status.get() == "published"
                            on:click=move |_| filter_status.set("published".to_string())
                        >"Published"</button>
                        <button 
                            class="chip"
                            class:active=move || filter_status.get() == "draft"
                            on:click=move |_| filter_status.set("draft".to_string())
                        >"Drafts"</button>
                        <button 
                            class="chip"
                            class:active=move || filter_status.get() == "archived"
                            on:click=move |_| filter_status.set("archived".to_string())
                        >"Archived"</button>
                    </div>
                </div>
                
                <div class="toolbar-right">
                    <select 
                        class="sort-select-industrial"
                        prop:value=move || sort_by.get()
                        on:change=move |e| sort_by.set(event_target_value(&e))
                    >
                        <option value="updated">"Last Updated"</option>
                        <option value="created">"Date Created"</option>
                        <option value="name">"Name"</option>
                        <option value="visits">"Most Visits"</option>
                    </select>
                    
                    <div class="view-toggle">
                        <button 
                            class="view-btn"
                            class:active=move || view_mode.get() == "grid"
                            on:click=move |_| view_mode.set("grid".to_string())
                            title="Grid View"
                        >
                            <img src="/assets/icons/grid.svg" alt="Grid" />
                        </button>
                        <button 
                            class="view-btn"
                            class:active=move || view_mode.get() == "list"
                            on:click=move |_| view_mode.set("list".to_string())
                            title="List View"
                        >
                            <img src="/assets/icons/list.svg" alt="List" />
                        </button>
                    </div>
                </div>
            </section>
            
            // Spaces Grid/List
            <section class="projects-content">
                <Show
                    when=move || !filtered_spaces().is_empty()
                    fallback=|| view! {
                        <div class="empty-state-industrial">
                            <img src="/assets/icons/folder.svg" alt="No spaces" class="empty-icon" />
                            <h3>"No spaces found"</h3>
                            <p>"Download Eustress Engine to create your first space"</p>
                            <a href="/download" class="btn-create-new">
                                <img src="/assets/icons/download.svg" alt="Download" class="btn-icon" />
                                "Download Studio"
                            </a>
                        </div>
                    }
                >
                    <div class="spaces-grid" class:list-view=move || view_mode.get() == "list">
                        <For
                            each=filtered_spaces
                            key=|place| place.id.clone()
                            children=move |place| {
                                let place_id = place.id.clone();
                                view! { <PlaceCard place=place active_menu=active_menu place_id=place_id /> }
                            }
                        />
                    </div>
                </Show>
            </section>
            
            // AI Training Portal Section
            <section class="ai-training-section">
                <div class="ai-section-header">
                    <div class="ai-icon-wrap">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M12 2L2 7l10 5 10-5-10-5z"/>
                            <path d="M2 17l10 5 10-5"/>
                            <path d="M2 12l10 5 10-5"/>
                        </svg>
                    </div>
                    <div class="ai-section-text">
                        <h2>"AI Training Portal"</h2>
                        <p>"Export consented spatial data from your spaces to train AI models. One API key indexes all your spaces and entities marked with AI=true."</p>
                    </div>
                </div>
                
                <div class="ai-features">
                    <div class="ai-feature">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
                        </svg>
                        <span>"50% less tokens with TOON format"</span>
                    </div>
                    <div class="ai-feature">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <circle cx="12" cy="12" r="10"/>
                            <polyline points="12 6 12 12 16 14"/>
                        </svg>
                        <span>"Real-time WebTransport streaming"</span>
                    </div>
                    <div class="ai-feature">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/>
                            <circle cx="9" cy="7" r="4"/>
                            <path d="M23 21v-2a4 4 0 0 0-3-3.87"/>
                            <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
                        </svg>
                        <span>"Team data aggregation"</span>
                    </div>
                </div>
                
                <a href="/ai" class="ai-portal-btn">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                    </svg>
                    "Manage API Keys"
                </a>
            </section>
            
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Place Card Component
// -----------------------------------------------------------------------------

#[component]
fn PlaceCard(
    place: Place,
    active_menu: RwSignal<Option<String>>,
    place_id: String,
) -> impl IntoView {
    let play_url = format!("/play/{}", place.id);
    let play_url2 = play_url.clone();
    // Edit opens in desktop app via custom protocol
    let edit_url = format!("eustress://edit/{}", place.id);
    let edit_url2 = edit_url.clone();
    let configure_url = format!("/configure/{}", place.id);
    
    let status_class = format!("status-badge {}", place.status.as_str());
    
    let place_id_check = place_id.clone();
    let is_menu_open = move || active_menu.get() == Some(place_id_check.clone());
    
    let place_id_toggle = place_id.clone();
    let toggle_menu = move |_| {
        let current = active_menu.get();
        if current == Some(place_id_toggle.clone()) {
            active_menu.set(None);
        } else {
            active_menu.set(Some(place_id_toggle.clone()));
        }
    };
    
    view! {
        <div class="place-card">
            <div class="card-thumbnail">
                <img src="/assets/icons/gamepad.svg" alt="Place" class="thumbnail-icon" />
                <div class=status_class>{place.status.display_name()}</div>
            </div>
            
            <div class="card-body">
                <div class="card-header">
                    <h3 class="card-title">{place.name.clone()}</h3>
                    <div class="menu-container">
                        <button class="menu-trigger" on:click=toggle_menu title="More options">
                            <img src="/assets/icons/more.svg" alt="Menu" />
                        </button>
                        
                        <Show when=is_menu_open>
                            <div class="context-menu">
                                <a href=play_url.clone() class="menu-item">
                                    <img src="/assets/icons/play.svg" alt="Play" />
                                    "Play"
                                </a>
                                <a href=edit_url.clone() class="menu-item">
                                    <img src="/assets/icons/edit.svg" alt="Edit" />
                                    "Edit"
                                </a>
                                <a href=configure_url.clone() class="menu-item">
                                    <img src="/assets/icons/settings.svg" alt="Configure" />
                                    "Configure"
                                </a>
                                <div class="menu-divider"></div>
                                <button class="menu-item">
                                    <img src="/assets/icons/copy.svg" alt="Duplicate" />
                                    "Duplicate"
                                </button>
                                <button class="menu-item">
                                    <img src="/assets/icons/archive.svg" alt="Archive" />
                                    "Archive"
                                </button>
                                <div class="menu-divider"></div>
                                <button class="menu-item danger">
                                    <img src="/assets/icons/trash.svg" alt="Delete" />
                                    "Delete"
                                </button>
                            </div>
                        </Show>
                    </div>
                </div>
                
                <p class="card-description">{place.description.clone()}</p>
                
                <div class="card-meta">
                    <span class="meta-item">
                        <img src="/assets/icons/users.svg" alt="Players" class="meta-icon" />
                        {format!("Max {}", place.max_players)}
                    </span>
                    <span class="meta-item">
                        <img src="/assets/icons/tag.svg" alt="Genre" class="meta-icon" />
                        {place.genre.clone()}
                    </span>
                </div>
                
                <div class="card-stats">
                    <span class="stat">
                        <img src="/assets/icons/play.svg" alt="Visits" class="stat-icon-sm" />
                        {format_number(place.visits)}
                    </span>
                    <span class="stat">
                        <img src="/assets/icons/heart.svg" alt="Favorites" class="stat-icon-sm" />
                        {format_number(place.favorites)}
                    </span>
                    <span class="stat updated">
                        {place.updated_at.clone()}
                    </span>
                </div>
                
                <div class="card-actions">
                    <a href=play_url2 class="action-btn primary">
                        <img src="/assets/icons/play.svg" alt="Play" />
                        "Play"
                    </a>
                    <a href=edit_url2 class="action-btn secondary">
                        <img src="/assets/icons/edit.svg" alt="Edit" />
                        "Edit"
                    </a>
                </div>
            </div>
        </div>
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
