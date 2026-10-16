// =============================================================================
// Eustress Web - Projects Page (Industrial Redesign)
// =============================================================================
// User's creative hub - manage all spaces, games, and experiences
// =============================================================================

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::api::{ApiClient};
use crate::api::{commerce_error_text, get_commerce_account, CommerceMode, SellerSimulation};
use crate::components::action_menu::{Action, ActionMenu, Item};
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::utils::after_event;
use super::review_panel::ReviewPanel;
use super::project_sell::{seller_for, SellButton, SellChip, SellNote};
use super::project_tags::TagEditor;
use crate::tags::{query_matches_tag, suggested_label};

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
    /// Review decided against listing it.
    NotListed,
    /// Review asked for changes before it can be listed.
    ChangesRequested,
    /// Passed review, and the author keeps it private.
    Private,
}

impl SpaceStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Archived => "archived",
            Self::UnderReview => "review",
            Self::NotListed => "not_listed",
            Self::ChangesRequested => "changes_requested",
            Self::Private => "private",
        }
    }

    pub(crate) fn display_name(&self) -> &'static str {
        match self {
            Self::Published => "Published",
            Self::Draft => "Draft",
            Self::Archived => "Archived",
            Self::UnderReview => "Under Review",
            Self::NotListed => "Not listed",
            Self::ChangesRequested => "Changes requested",
            Self::Private => "Private",
        }
    }

    /// The status the API reports for a project. Held and quarantined cases
    /// both arrive as `in_review` and are not told apart here.
    pub(crate) fn from_api(status: &str) -> Self {
        match status {
            "published" => Self::Published,
            "draft" | "rejected" => Self::Draft,
            "archived" => Self::Archived,
            "not_listed" => Self::NotListed,
            "changes_requested" => Self::ChangesRequested,
            "approved_private" => Self::Private,
            "in_review" | "appeal_in_review" | "unreviewed" | "review"
            | "pending" | "classifying" | "held" | "appealed" | "quarantined" => Self::UnderReview,
            _ => Self::Draft,
        }
    }

    /// Whether the review panel has something to say about this project.
    fn has_review(&self) -> bool {
        matches!(self, Self::UnderReview | Self::NotListed | Self::ChangesRequested | Self::Private)
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
    /// The tags that describe it, and who chose them (`"author"` or `"judge"`; absent means the author).
    pub tags: Vec<String>,
    pub tags_source: Option<String>,
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
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub tags_source: Option<String>,
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
    // The project whose settings dialog is open, the one an Archive is waiting on, and the line that says what just happened.
    let settings_open = RwSignal::new(None::<Place>);
    let archive_ask = RwSignal::new(None::<Place>);
    let toast = RwSignal::new(None::<Toast>);
    // The project whose review panel is open. A review email links to
    // /projects?review=<id>, which opens it.
    let review_open = RwSignal::new(review_id_from_query(&current_query()));
    let panel_api = app_state.api_url.clone();

    // API data state
    let spaces = RwSignal::new(Vec::<Place>::new());
    let is_loading = RwSignal::new(true);
    
    // What each published project can sell, from the account's commerce record. A project with no entry here has
    // nothing to show: no chip and no Sell button. The record is asked for once per sign-in.
    let sellers = RwSignal::new(Vec::<SellerSimulation>::new());
    let auth = app_state.auth;
    let signed_in = Memo::new(move |_| matches!(auth.get(), AuthState::Authenticated(_)));
    {
        let api_url = app_state.api_url.clone();
        Effect::new(move |_| {
            if !signed_in.get() {
                sellers.set(Vec::new());
                return;
            }
            let api_url = api_url.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match get_commerce_account(&api_url, CommerceMode::Test).await {
                    Ok(account) => sellers.set(account.simulations),
                    Err(e) => log::warn!("Could not read what each project can sell: {}", commerce_error_text(&e)),
                }
            });
        });
    }

    // Fetch projects from API on mount, and again after an archive or unarchive.
    let reload = {
        let api_url = app_state.api_url.clone();
        move || {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            // Fetch user's projects
            match client.get::<ProjectsResponse>("/api/projects").await {
                Ok(response) => {
                    let places: Vec<Place> = response.projects.into_iter().map(|p| {
                        // The API reports the review state for anything not yet
                        // listed (see SpaceStatus::from_api).
                        let status = SpaceStatus::from_api(&p.status);
                        Place {
                            id: p.id,
                            name: p.name,
                            description: p.description.unwrap_or_default(),
                            thumbnail_url: p.thumbnail_url,
                            status,
                            visits: p.play_count,
                            favorites: p.favorite_count,
                            max_players: p.max_players,
                            tags: p.tags.unwrap_or_default(),
                            tags_source: p.tags_source,
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
        }
    };
    let first_load = reload.clone();
    Effect::new(move |_| first_load());

    // Archive and unarchive go through the Worker; the list is read again afterwards, so it shows what the Worker kept.
    let archive_api = app_state.api_url.clone();
    let set_archived = {
        let reload = reload.clone();
        move |place: Place, archive: bool| {
            let api_url = archive_api.clone();
            let reload = reload.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let client = ApiClient::new(&api_url);
                let route = if archive { "archive" } else { "unarchive" };
                let sent = client.post::<serde_json::Value, _>(&format!("/api/simulations/{}/{route}", place.id), &serde_json::json!({})).await;
                match sent {
                    Ok(_) => {
                        reload();
                        toast.set(Some(if archive {
                            Toast { message: format!("Archived {}. It is out of the Gallery, and only you can see it.", place.name), undo: Some(place.id.clone()), failed: false }
                        } else {
                            Toast { message: format!("{} is back in the Gallery, as it was.", place.name), undo: None, failed: false }
                        }));
                    }
                    Err(e) => toast.set(Some(Toast { message: format!("Could not {} {}: {}", route, place.name, crate::api::failure_sentence(&e)), undo: None, failed: true })),
                }
            });
        }
    };
    let archive_now = {
        let set_archived = set_archived.clone();
        Callback::new(move |place: Place| set_archived(place, true))
    };
    let unarchive_now = {
        let set_archived = set_archived.clone();
        Callback::new(move |place: Place| set_archived(place, false))
    };
    // A card's Archive asks first; Unarchive undoes it at once.
    let on_archive = Callback::new(move |place: Place| {
        if place.status == SpaceStatus::Archived {
            unarchive_now.run(place);
        } else {
            archive_ask.set(Some(place));
        }
    });
    let on_configure = Callback::new(move |place: Place| settings_open.set(Some(place)));
    // The tag editor saved: close the dialog, read the list again, and say that the Space goes back to review.
    let after_tags = {
        let reload = reload.clone();
        Callback::new(move |message: String| {
            settings_open.set(None);
            reload();
            toast.set(Some(Toast { message, undo: None, failed: false }));
        })
    };
    // "Undo" under the line that says it was archived: the listing is relisted by its id.
    let undo_archive = {
        let set_archived = set_archived.clone();
        move |id: String| {
            let place = Place { id, name: "the Space".to_string(), description: String::new(), thumbnail_url: None, status: SpaceStatus::Archived, visits: 0, favorites: 0, max_players: 0, tags: Vec::new(), tags_source: None, updated_at: String::new(), created_at: String::new(), is_public: false };
            set_archived(place, false);
        }
    };

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
                    || query_matches_tag(&query, &place.tags);
                
                let matches_status = status == "all"
                    || place.status.as_str() == status;
                
                matches_search && matches_status
            })
            .collect::<Vec<_>>()
    };
    
    // Stats
    let total_visits = move || spaces.get().iter().map(|p| p.visits).sum::<u64>();
    let total_favorites = move || spaces.get().iter().map(|p| p.favorites).sum::<u64>();
    // Published means approved and public. A Space still in review is counted apart, so the number does not read as "nothing is there".
    let published_count = move || spaces.get().iter().filter(|p| p.status == SpaceStatus::Published).count();
    let in_review_count = move || spaces.get().iter().filter(|p| p.status == SpaceStatus::UnderReview).count();
    
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
                <SellNote />
                
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
                            {move || (in_review_count() > 0).then(|| view! { <span class="stat-sub">{format!("{} in review", in_review_count())}</span> })}
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
                            class:active=move || filter_status.get() == "review"
                            on:click=move |_| filter_status.set("review".to_string())
                        >"In review"</button>
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
                        </div>
                    }
                >
                    <div class="spaces-grid" class:list-view=move || view_mode.get() == "list">
                        <For
                            each=filtered_spaces
                            key=|place| place.id.clone()
                            children=move |place| {
                                view! { <PlaceCard place=place review_open=review_open on_configure=on_configure on_archive=on_archive sellers=sellers /> }
                            }
                        />
                    </div>
                </Show>
            </section>
            
            // Eustress Engine Protocol — AI Training Data Export
            <section class="ai-training-section">
                <div class="ai-section-header-centered">
                    <h2>"Eustress Engine Protocol"</h2>
                    <p class="ai-subtitle">"Spatial AI Training Data Export"</p>
                    <p class="ai-description">"Export consented spatial data from your published spaces to train AI models. One API key indexes all your spaces and entities marked with "<code>"AI=true"</code>"."</p>
                </div>

                <div class="ai-protocol-grid">
                    <div class="ai-protocol-card">
                        <div class="ai-protocol-card-icon">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <circle cx="12" cy="12" r="10"/>
                                <polyline points="12 6 12 12 16 14"/>
                            </svg>
                        </div>
                        <h3>"Real-time Streams"</h3>
                        <p>"Subscribe to live ECS deltas via EustressStream for spatial reasoning agents. Zero-copy rkyv serialization with sub-microsecond dispatch."</p>
                        <div class="ai-benchmark-table">
                            <div class="ai-bench-row ai-bench-header">
                                <span class="ai-bench-label">"Transport"</span>
                                <span class="ai-bench-value">"Latency"</span>
                                <span class="ai-bench-value">"Throughput"</span>
                            </div>
                            <div class="ai-bench-row ai-bench-highlight">
                                <span class="ai-bench-label">"In-process"</span>
                                <span class="ai-bench-value">"< 1 \u{00B5}s"</span>
                                <span class="ai-bench-value">"~85M msg/s"</span>
                            </div>
                            <div class="ai-bench-row">
                                <span class="ai-bench-label">"TCP"</span>
                                <span class="ai-bench-value">"103.6 \u{00B5}s"</span>
                                <span class="ai-bench-value">"9,651 msg/s"</span>
                            </div>
                            <div class="ai-bench-row">
                                <span class="ai-bench-label">"QUIC"</span>
                                <span class="ai-bench-value">"182.4 \u{00B5}s"</span>
                                <span class="ai-bench-value">"5,483 msg/s"</span>
                            </div>
                        </div>
                    </div>
                    <div class="ai-protocol-card">
                        <div class="ai-protocol-card-icon">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/>
                                <circle cx="9" cy="7" r="4"/>
                                <path d="M23 21v-2a4 4 0 0 0-3-3.87"/>
                                <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
                            </svg>
                        </div>
                        <h3>"Team Aggregation"</h3>
                        <p>"Aggregate spatial data across team members and spaces. Shared training datasets with per-entity consent controls."</p>
                        <ul class="ai-feature-list">
                            <li>"Merge scene graphs from multiple contributors"</li>
                            <li>"Per-entity AI opt-in via Instance.AI property"</li>
                            <li>"Automatic PII redaction for player data"</li>
                            <li>"Organization-scoped dataset namespaces"</li>
                        </ul>
                    </div>
                    <div class="ai-protocol-card">
                        <div class="ai-protocol-card-icon">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/>
                                <line x1="8" y1="21" x2="16" y2="21"/>
                                <line x1="12" y1="17" x2="12" y2="21"/>
                            </svg>
                        </div>
                        <h3>"Scene Export"</h3>
                        <p>"Export full scene graphs as content-addressed .echk chunks. Human-readable TOML files preserved for direct AI ingestion."</p>
                        <ul class="ai-feature-list">
                            <li>"Transforms, materials, physics, scripting context"</li>
                            <li>"TOML files readable without custom deserializers"</li>
                            <li>"Zstd-compressed .pak archives for R2 storage"</li>
                            <li>"Incremental exports via SceneDelta stream"</li>
                        </ul>
                    </div>
                </div>

                <div class="ai-btn-wrap">
                    <a href="/ai" class="ai-portal-btn">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                            <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                        </svg>
                        "Manage API Keys"
                    </a>
                </div>
            </section>

            // A line that says what an archive just did, with the way back
            {move || toast.get().map(|t| {
                let undo = t.undo.clone();
                let undo_archive = undo_archive.clone();
                view! {
                    <div class=if t.failed { "projects-toast failed" } else { "projects-toast" } role=if t.failed { "alert" } else { "status" }>
                        <span>{t.message.clone()}</span>
                        {undo.map(|id| view! {
                            <button class="projects-toast-action" on:click=move |_| { let id = id.clone(); let undo_archive = undo_archive.clone(); after_event(move || { toast.set(None); undo_archive(id); }); }>"Undo"</button>
                        })}
                        <button class="projects-toast-close" aria-label="Dismiss" on:click=move |_| after_event(move || toast.set(None))>"x"</button>
                    </div>
                }
            })}

            // Archive asks first
            {move || archive_ask.get().map(|place| {
                let confirm_place = place.clone();
                view! {
                    <div class="modal-overlay" on:click=move |_| after_event(move || archive_ask.set(None))>
                        <div class="modal-content archive-dialog" role="alertdialog" aria-modal="true" aria-labelledby="archive-title" on:click=|e| e.stop_propagation()>
                            <h3 id="archive-title">{format!("Archive {}?", place.name)}</h3>
                            <p>"It leaves the Gallery and only you can see it. Its review, its world and its counts stay as they are. You can unarchive it any time, and the line that appears after this has an Undo."</p>
                            <div class="modal-actions">
                                <button class="btn btn-secondary" on:click=move |_| after_event(move || archive_ask.set(None))>"Cancel"</button>
                                <button class="btn btn-primary" on:click=move |_| { let p = confirm_place.clone(); after_event(move || { archive_ask.set(None); archive_now.run(p); }); }>"Archive"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // The listing's settings: what they are now, and where each one is changed
            {move || settings_open.get().map(|place| {
                let studio = format!("eustress://edit/{}", place.id);
                let id = place.id.clone();
                let has_review = place.status.has_review();
                let archived = place.status == SpaceStatus::Archived;
                let place_for_archive = place.clone();
                let tags_now = place.tags.clone();
                let tags_source = place.tags_source.clone();
                let tags_name = place.name.clone();
                let tags_locked = place.status == SpaceStatus::UnderReview;
                view! {
                    <div class="modal-overlay" on:click=move |_| after_event(move || settings_open.set(None))>
                        <div class="modal-content settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title" on:click=|e| e.stop_propagation()>
                            <h3 id="settings-title">{format!("Settings: {}", place.name)}</h3>
                            <dl class="settings-facts">
                                <dt>"Status"</dt><dd>{place.status.display_name()}</dd>
                                <dt>"Visibility"</dt><dd>{if place.is_public { "Public once approved" } else { "Private: only you" }}</dd>
                                <dt>"Max players"</dt><dd>{format!("{} (from the last publish)", place.max_players)}</dd>
                                <dt>"Description"</dt><dd>{if place.description.is_empty() { "None yet".to_string() } else { place.description.clone() }}</dd>
                            </dl>
                            <h4>"Tags"</h4>
                            <TagEditor place_id=id.clone() place_name=tags_name tags=tags_now tags_source=tags_source locked=tags_locked on_saved=after_tags />
                            <h4>"Where each one is changed"</h4>
                            <ul class="settings-where">
                                <li><strong>"Title and description"</strong>": the Publish dialog in Studio. Publishing again updates the listing, and a change to the words goes back to review."</li>
                                <li><strong>"Tags"</strong>": here, or in the Publish dialog in Studio. A change to the tags goes back to review."</li>
                                <li><strong>"Max players"</strong>": the Space's Players service in Studio (Properties, MaxPlayers). It reaches this page the next time you publish."</li>
                                <li><strong>"Visibility"</strong>": Archive and Unarchive here, or the public setting in the Publish dialog."</li>
                            </ul>
                            <div class="modal-actions">
                                <a href=studio class="btn btn-secondary">"Edit in Studio"</a>
                                {has_review.then(|| { let id = id.clone(); view! { <button class="btn btn-secondary" on:click=move |_| { let id = id.clone(); after_event(move || { settings_open.set(None); review_open.set(Some(id)); }); }>"Open the review"</button> } })}
                                <button class="btn btn-secondary" on:click=move |_| { let p = place_for_archive.clone(); after_event(move || { settings_open.set(None); on_archive.run(p); }); }>{if archived { "Unarchive" } else { "Archive" }}</button>
                                <button class="btn btn-primary" on:click=move |_| after_event(move || settings_open.set(None))>"Close"</button>
                            </div>
                        </div>
                    </div>
                }
            })}

            // What review decided about a project, and the appeal
            {move || review_open.get().map(|id| view! {
                <ReviewPanel
                    sim_id=id
                    api_url=panel_api.clone()
                    on_close=move || review_open.set(None)
                />
            })}

            <Footer />
        </div>
    }
}

/// The query string of the page in front of the visitor. The prerender has no
/// page.
#[cfg(not(feature = "ssr"))]
fn current_query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

#[cfg(feature = "ssr")]
fn current_query() -> String {
    String::new()
}

/// The project id in `?review=<id>`. Only a plausible id passes, since it goes
/// into an API path: lowercase hex and dashes, 1 to 64 characters.
fn review_id_from_query(search: &str) -> Option<String> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == "review")
        .map(|(_, value)| value)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-'))
        .map(str::to_string)
}

// -----------------------------------------------------------------------------
// Place Card Component
// -----------------------------------------------------------------------------

/// What the page says after an archive or unarchive.
#[derive(Clone, Debug, PartialEq)]
struct Toast {
    message: String,
    /// The listing to relist when "Undo" is pressed.
    undo: Option<String>,
    failed: bool,
}

#[component]
fn PlaceCard(
    place: Place,
    review_open: RwSignal<Option<String>>,
    on_configure: Callback<Place>,
    on_archive: Callback<Place>,
    sellers: RwSignal<Vec<SellerSimulation>>,
) -> impl IntoView {
    let playable = matches!(place.status, SpaceStatus::Published | SpaceStatus::Private);
    // This page lists the signed-in person's own Spaces, so the reader is always the author: tags the review chose say so.
    let card_tags = place.tags.clone();
    let tags_note = suggested_label(&place.tags, place.tags_source.as_deref(), true);
    let archived = place.status == SpaceStatus::Archived;
    let has_review = place.status.has_review();
    // Play opens the Play chooser in place, like every Play button on the site.
    let chooser = expect_context::<crate::components::play_chooser::PlayChooserSlot>();
    let play_id = place.id.clone();
    // Edit opens Studio through its own link; Studio downloads the listing's world and opens it.
    let edit_url = format!("eustress://edit/{}", place.id);
    let status_class = format!("status-badge {}", place.status.as_str());
    let thumbnail = place.thumbnail_url.clone().filter(|u| u.starts_with("https://") || u.starts_with('/'));
    // This project's selling entry, once the account's commerce record has loaded. No entry: no chip and no Sell button.
    let seller_id = place.id.clone();
    let seller = Memo::new(move |_| sellers.with(|list| seller_for(list, &seller_id).cloned()));

    // Every item does something. Play only where the listing can be played; Review only where review has something to say.
    let mut items = Vec::new();
    if playable {
        let id = play_id.clone();
        items.push(Item::new("Play", "play.svg", Action::Call(Callback::new(move |_| chooser.open(&id)))));
    }
    items.push(Item::new("Edit in Studio", "edit.svg", Action::Open(edit_url.clone())));
    if has_review {
        let id = place.id.clone();
        items.push(Item::new("Review", "shield.svg", Action::Call(Callback::new(move |_| review_open.set(Some(id.clone()))))));
    }
    let for_settings = place.clone();
    items.push(Item::new("Settings", "settings.svg", Action::Call(Callback::new(move |_| on_configure.run(for_settings.clone())))));
    let for_archive = place.clone();
    items.push(
        Item::new(if archived { "Unarchive" } else { "Archive" }, "archive.svg", Action::Call(Callback::new(move |_| on_archive.run(for_archive.clone()))))
            .divided(),
    );

    view! {
        <div class="place-card">
            <div class="card-thumbnail">
                {match thumbnail {
                    Some(src) => view! { <img src=src alt="" class="thumbnail-image" loading="lazy" /> }.into_any(),
                    None => view! { <img src="/assets/icons/gamepad.svg" alt="" class="thumbnail-icon" /> }.into_any(),
                }}
                <div class=status_class>{place.status.display_name()}</div>
            </div>

            <div class="card-body">
                <div class="card-header">
                    <h3 class="card-title">{place.name.clone()}</h3>
                    <div class="menu-container">
                        <ActionMenu label=format!("Options for {}", place.name) items=items />
                    </div>
                </div>

                <p class="card-description">{place.description.clone()}</p>

                <div class="card-meta">
                    // The number is the one the last publish stored, so it can lag until the next publish from Studio.
                    <span class="meta-item" title="From the last publish. It updates the next time you publish from Studio.">
                        <img src="/assets/icons/users.svg" alt="Players" class="meta-icon" />
                        {format!("Max {}", place.max_players)}
                    </span>
                    <span class="meta-item card-tags">
                        <img src="/assets/icons/tag.svg" alt="Tags" class="meta-icon" />
                        {if card_tags.is_empty() {
                            view! { <span class="card-tag-note">"No tags yet"</span> }.into_any()
                        } else {
                            card_tags.iter().map(|t| view! { <span class="card-tag">{format!("#{t}")}</span> }).collect_view().into_any()
                        }}
                        {tags_note.map(|words| view! { <span class="card-tag-note">{words}</span> })}
                    </span>
                </div>

                {move || seller.get().map(|sim| view! { <SellChip sim=sim /> })}

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
                    {playable.then(|| view! {
                        <button type="button" class="action-btn primary" on:click=move |_| chooser.open(&play_id)>
                            <img src="/assets/icons/play.svg" alt="Play" />
                            "Play"
                        </button>
                    })}
                    <a href=edit_url.clone() class="action-btn secondary">
                        <img src="/assets/icons/edit.svg" alt="Edit" />
                        "Edit"
                    </a>
                    {has_review.then(|| {
                        let id = place.id.clone();
                        view! {
                            <button class="action-btn secondary" on:click=move |_| review_open.set(Some(id.clone()))>
                                <img src="/assets/icons/shield.svg" alt="" />
                                "Review"
                            </button>
                        }
                    })}
                    {move || seller.get().map(|sim| view! { <SellButton sim_id=sim.id /> })}
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

// -----------------------------------------------------------------------------
// API Keys Section Component
// -----------------------------------------------------------------------------

/// API key data from backend
#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApiKeyData {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub key_type: String,
    /// "test" or "live", for a commerce key.
    #[serde(default)]
    pub mode: String,
    pub created_at: String,
    pub last_used: Option<String>,
    pub usage_count: u64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApiKeysResponse {
    pub keys: Vec<ApiKeyData>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct CreateKeyResponse {
    pub id: String,
    pub name: String,
    pub key: String,
    pub key_type: String,
}

/// API Keys management section for signed-in users.
#[component]
pub fn ApiKeysSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    
    // State
    let api_keys = RwSignal::new(Vec::<ApiKeyData>::new());
    let is_loading = RwSignal::new(true);
    let show_create_modal = RwSignal::new(false);
    let new_key_name = RwSignal::new(String::new());
    let new_key_type = RwSignal::new("datastore".to_string());
    // A commerce key is a test key or a live key, as in `eustress commerce`.
    let new_key_mode = RwSignal::new("test".to_string());
    let newly_created_key = RwSignal::new(None::<String>);
    let copied_key_id = RwSignal::new(None::<String>);
    let key_error = RwSignal::new(None::<String>);
    // A passkey session cannot create keys (revoking stays on).
    let auth = app_state.auth;
    let passkey_session = move || auth.with(|a| crate::pages::passkeys::key_change_blocked(a.user()));
    
    // Fetch API keys on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.get::<ApiKeysResponse>("/api/keys").await {
                Ok(response) => {
                    api_keys.set(response.keys);
                }
                Err(e) => {
                    log::warn!("Failed to fetch API keys: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    });
    
    // Create new key handler
    let api_url_stored = StoredValue::new(app_state.api_url.clone());
    let create_key = move |_| {
        let api_url = api_url_stored.get_value();
        let name = new_key_name.get();
        let key_type = new_key_type.get();
        let mode = new_key_mode.get();

        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let body = serde_json::json!({
                "name": name,
                "key_type": key_type,
                "mode": mode
            });

            match client.post::<CreateKeyResponse, _>("/api/keys", &body).await {
                Ok(response) => {
                    newly_created_key.set(Some(response.key.clone()));
                    key_error.set(None);
                    // Refresh keys list
                    if let Ok(keys_response) = client.get::<ApiKeysResponse>("/api/keys").await {
                        api_keys.set(keys_response.keys);
                    }
                }
                Err(e) => {
                    log::error!("Failed to create API key: {:?}", e);
                    key_error.set(Some(crate::pages::passkeys::identity_file_required_words(&e).unwrap_or_else(|| format!("The key was not created: {e}"))));
                }
            }
        });
    };

    // Revoke: the key stops working at once, everywhere it is used.
    let revoke_key = move |key_id: String, key_name: String| {
        let sure = web_sys::window()
            .and_then(|w| w.confirm_with_message(&format!("Revoke \u{201c}{key_name}\u{201d}? Anything using it stops working.")).ok())
            .unwrap_or(false);
        if !sure {
            return;
        }
        let api_url = api_url_stored.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.delete::<serde_json::Value>(&format!("/api/keys/{key_id}")).await {
                Ok(_) => {
                    api_keys.update(|keys| keys.retain(|k| k.id != key_id));
                    key_error.set(None);
                }
                Err(e) => key_error.set(Some(format!("The key was not revoked: {e}"))),
            }
        });
    };
    
    // Copy key to clipboard via navigator.clipboard JS API
    let copy_to_clipboard = move |key: String, key_id: String| {
        let js_code = format!(
            "navigator.clipboard.writeText('{}')",
            key.replace('\'', "\\'")
        );
        let _ = js_sys::eval(&js_code);

        copied_key_id.set(Some(key_id));
        let copied_signal = copied_key_id;
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(2000).await;
            copied_signal.set(None);
        });
    };
    
    view! {
        <section class="api-keys-section">
            <div class="section-header">
                <div class="header-content">
                    <svg class="section-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                    </svg>
                    <div>
                        <h2>"API Keys"</h2>
                        <p>"Manage your API keys for DataStoreService, HttpService, AI Training, and selling (eustress commerce)"</p>
                    </div>
                </div>
                <button 
                    class="btn-create-key"
                    disabled=move || passkey_session()
                    on:click=move |_| {
                        new_key_name.set(String::new());
                        newly_created_key.set(None);
                        show_create_modal.set(true);
                    }
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <line x1="12" y1="5" x2="12" y2="19"/>
                        <line x1="5" y1="12" x2="19" y2="12"/>
                    </svg>
                    "Create Key"
                </button>
            </div>
            
            {move || passkey_session().then(|| view! { <p class="keys-note" role="status">{crate::pages::passkeys::EMAIL_AND_KEYS_SENTENCE}</p> })}
            {move || key_error.get().map(|e| view! { <p class="keys-error" role="alert">{e}</p> })}

            <Show when=move || is_loading.get()>
                <div class="keys-loading">
                    <div class="spinner"></div>
                    "Loading API keys..."
                </div>
            </Show>
            
            <Show when=move || !is_loading.get() && api_keys.get().is_empty()>
                <div class="keys-empty">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/>
                    </svg>
                    <p>"No API keys yet. Create one to use DataStoreService or HttpService in your scripts."</p>
                </div>
            </Show>
            
            <Show when=move || !is_loading.get() && !api_keys.get().is_empty()>
                <div class="keys-table">
                    <div class="keys-header">
                        <span class="col-name">"Name"</span>
                        <span class="col-type">"Type"</span>
                        <span class="col-key">"Key"</span>
                        <span class="col-usage">"Usage"</span>
                        <span class="col-created">"Created"</span>
                        <span class="col-actions">"Actions"</span>
                    </div>
                    <For
                        each=move || api_keys.get()
                        key=|key| key.id.clone()
                        children=move |key| {
                            let key_prefix = key.key_prefix.clone();
                            let on_revoke = {
                                let (id, name) = (key.id.clone(), key.name.clone());
                                move |_| revoke_key(id.clone(), name.clone())
                            };
                            let type_label = if key.key_type == "commerce" && !key.mode.is_empty() {
                                format!("commerce \u{00b7} {}", key.mode)
                            } else {
                                key.key_type.clone()
                            };
                            view! {
                                <div class="key-row">
                                    <span class="col-name">{key.name.clone()}</span>
                                    <span class="col-type">
                                        <span class="key-type-badge" class:datastore=key.key_type == "datastore" class:http=key.key_type == "http" class:ai=key.key_type == "ai" class:commerce=key.key_type == "commerce">
                                            {type_label}
                                        </span>
                                    </span>
                                    <span class="col-key">
                                        <code class="key-preview">{format!("{}...", key_prefix)}</code>
                                    </span>
                                    <span class="col-usage">{format_number(key.usage_count)}</span>
                                    <span class="col-created">{key.created_at.clone()}</span>
                                    <span class="col-actions">
                                        <button
                                            class="action-icon"
                                            title="Revoke Key"
                                            on:click=on_revoke
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <polyline points="3 6 5 6 21 6"/>
                                                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                                            </svg>
                                        </button>
                                    </span>
                                </div>
                            }
                        }
                    />
                </div>
            </Show>
            
            // Create Key Modal
            <Show when=move || show_create_modal.get()>
                <div class="modal-overlay" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                    <div class="modal-content" on:click=|e| e.stop_propagation()>
                        <div class="modal-header">
                            <h3>"Create API Key"</h3>
                            <button class="modal-close" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                    <line x1="18" y1="6" x2="6" y2="18"/>
                                    <line x1="6" y1="6" x2="18" y2="18"/>
                                </svg>
                            </button>
                        </div>
                        
                        <Show when=move || newly_created_key.get().is_some()>
                            <div class="key-created-success">
                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                    <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/>
                                    <polyline points="22 4 12 14.01 9 11.01"/>
                                </svg>
                                <p>"Key created! Copy it now — you won't see it again."</p>
                                <div class="key-display">
                                    <code>{move || newly_created_key.get().unwrap_or_default()}</code>
                                    <button 
                                        class="copy-btn"
                                        on:click=move |_| {
                                            if let Some(key) = newly_created_key.get() {
                                                copy_to_clipboard(key, "new".to_string());
                                            }
                                        }
                                    >
                                        <Show 
                                            when=move || copied_key_id.get() == Some("new".to_string())
                                            fallback=|| view! {
                                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                    <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
                                                    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
                                                </svg>
                                            }
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <polyline points="20 6 9 17 4 12"/>
                                            </svg>
                                        </Show>
                                    </button>
                                </div>
                                <button class="btn-done" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                    "Done"
                                </button>
                            </div>
                        </Show>
                        
                        <Show when=move || newly_created_key.get().is_none()>
                            <div class="modal-body">
                                <div class="form-group">
                                    <label>"Key Name"</label>
                                    <input 
                                        type="text"
                                        placeholder="e.g., Production DataStore"
                                        prop:value=move || new_key_name.get()
                                        on:input=move |e| new_key_name.set(event_target_value(&e))
                                    />
                                </div>
                                
                                <div class="form-group">
                                    <label>"Key Type"</label>
                                    <div class="key-type-options">
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "datastore"
                                            on:click=move |_| new_key_type.set("datastore".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <ellipse cx="12" cy="5" rx="9" ry="3"/>
                                                <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/>
                                                <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>
                                            </svg>
                                            <span>"DataStore"</span>
                                            <small>"AWS DynamoDB persistence"</small>
                                        </button>
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "http"
                                            on:click=move |_| new_key_type.set("http".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <circle cx="12" cy="12" r="10"/>
                                                <line x1="2" y1="12" x2="22" y2="12"/>
                                                <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
                                            </svg>
                                            <span>"HttpService"</span>
                                            <small>"External API requests"</small>
                                        </button>
                                        <button 
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "ai"
                                            on:click=move |_| new_key_type.set("ai".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <path d="M12 2L2 7l10 5 10-5-10-5z"/>
                                                <path d="M2 17l10 5 10-5"/>
                                                <path d="M2 12l10 5 10-5"/>
                                            </svg>
                                            <span>"AI Training"</span>
                                            <small>"Spatial data export"</small>
                                        </button>
                                        <button
                                            class="type-option"
                                            class:selected=move || new_key_type.get() == "commerce"
                                            on:click=move |_| new_key_type.set("commerce".to_string())
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                                <path d="M12 2H2v10l9.29 9.29a1 1 0 0 0 1.41 0l8.59-8.59a1 1 0 0 0 0-1.41Z"/>
                                                <circle cx="7" cy="7" r="1.5"/>
                                            </svg>
                                            <span>"Commerce"</span>
                                            <small>"Sell products (eustress commerce)"</small>
                                        </button>
                                    </div>
                                </div>

                                <Show when=move || new_key_type.get() == "commerce">
                                    <div class="form-group">
                                        <label>"Mode"</label>
                                        <div class="key-type-options">
                                            <button
                                                class="type-option"
                                                class:selected=move || new_key_mode.get() == "test"
                                                on:click=move |_| new_key_mode.set("test".to_string())
                                            >
                                                <span>"Test"</span>
                                                <small>"Drafts and test purchases; moves no Tickets"</small>
                                            </button>
                                            <button
                                                class="type-option"
                                                class:selected=move || new_key_mode.get() == "live"
                                                on:click=move |_| new_key_mode.set("live".to_string())
                                            >
                                                <span>"Live"</span>
                                                <small>"Real sales, prices and refunds"</small>
                                            </button>
                                        </div>
                                    </div>
                                </Show>
                            </div>
                            
                            <div class="modal-footer">
                                <button class="btn-cancel" on:click=move |_| crate::utils::after_event(move || show_create_modal.set(false))>
                                    "Cancel"
                                </button>
                                <button 
                                    class="btn-create"
                                    disabled=move || new_key_name.get().is_empty()
                                    on:click=create_key
                                >
                                    "Create Key"
                                </button>
                            </div>
                        </Show>
                    </div>
                </div>
            </Show>
        </section>
    }
}
