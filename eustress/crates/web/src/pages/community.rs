// =============================================================================
// Eustress Web - Community Page (Industrial Design)
// =============================================================================
// Community dashboard with leaderboards, user search, and social features
// Work Hard, Play Hard - celebrating both creators and players
// =============================================================================

use leptos::prelude::*;
use crate::api::{self, ApiClient, PublicUser, LeaderboardEntry as ApiLeaderboardEntry, CommunityStats};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

// Helper function to trigger search (avoids closure move issues)
fn trigger_search(
    search_query: RwSignal<String>,
    search_results: RwSignal<Vec<UserResult>>,
    is_searching: RwSignal<bool>,
    api_url: String,
) {
    let query = search_query.get();
    if query.is_empty() {
        is_searching.set(false);
        search_results.set(vec![]);
        return;
    }
    
    is_searching.set(true);
    
    wasm_bindgen_futures::spawn_local(async move {
        let client = ApiClient::new(&api_url);
        
        match api::community::search_users(&client, &query, None, Some(10)).await {
            Ok(response) => {
                search_results.set(response.users);
            }
            Err(e) => {
                log::warn!("Search failed: {:?}", e);
            }
        }
    });
}

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Leaderboard entry for display.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub username: String,
    pub avatar_url: Option<String>,
    pub score: u64,
    pub score_label: String,
    pub badge: Option<String>,
}

impl From<ApiLeaderboardEntry> for LeaderboardEntry {
    fn from(entry: ApiLeaderboardEntry) -> Self {
        let badge = match entry.rank {
            1 => Some("🏆".to_string()),
            2 => Some("🥈".to_string()),
            3 => Some("🥉".to_string()),
            _ => None,
        };
        Self {
            rank: entry.rank,
            username: entry.user.username,
            avatar_url: entry.user.avatar_url,
            score: entry.score,
            score_label: entry.score_label,
            badge,
        }
    }
}

/// User search result.
pub type UserResult = PublicUser;

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Community page - leaderboards, user search, and social hub.
#[component]
pub fn CommunityPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    
    // State
    let search_query = RwSignal::new(String::new());
    let is_searching = RwSignal::new(false);
    
    // API data state
    let leaderboard = RwSignal::new(Vec::<LeaderboardEntry>::new());
    let search_results = RwSignal::new(Vec::<UserResult>::new());
    let stats = RwSignal::new(Option::<CommunityStats>::None);
    let is_loading = RwSignal::new(true);
    
    // Fetch community data on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            // Fetch leaderboard
            match api::community::get_leaderboard(&client, "creators", None, None, Some(10)).await {
                Ok(response) => {
                    let entries: Vec<LeaderboardEntry> = response.entries.into_iter().map(Into::into).collect();
                    leaderboard.set(entries);
                }
                Err(e) => {
                    log::warn!("Failed to fetch leaderboard: {:?}", e);
                }
            }
            
            // Fetch community stats
            match api::community::get_community_stats(&client).await {
                Ok(response) => {
                    stats.set(Some(response));
                }
                Err(e) => {
                    log::warn!("Failed to fetch stats: {:?}", e);
                }
            }
            
            is_loading.set(false);
        });
    });
    
    // Handle search - clone for each closure
    let api_url_search1 = app_state.api_url.clone();
    let api_url_search2 = app_state.api_url.clone();
    
    // Get stats for display
    let total_users = move || stats.get().map(|s| s.total_users).unwrap_or(0);
    let total_experiences = move || stats.get().map(|s| s.total_experiences).unwrap_or(0);
    let total_plays = move || stats.get().map(|s| s.total_plays).unwrap_or(0);
    
    view! {
        <div class="page page-community-industrial">
            <CentralNav active="community".to_string() />
            
            // Background
            <div class="community-bg">
                <div class="community-grid-overlay"></div>
                <div class="community-glow glow-1"></div>
                <div class="community-glow glow-2"></div>
                <div class="community-glow glow-3"></div>
            </div>
            
            // Hero Section
            <section class="community-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"COMMUNITY"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="community-title">"Work Hard. Play Hard."</h1>
                <p class="community-subtitle">"Join thousands of creators and players pushing the limits"</p>
                
                // Stats Banner
                <div class="community-stats">
                    <div class="stat-item">
                        <span class="stat-number">{move || format_number(total_users())}</span>
                        <span class="stat-label">"Users"</span>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="stat-item">
                        <span class="stat-number">{move || format_number(total_experiences())}</span>
                        <span class="stat-label">"Experiences"</span>
                    </div>
                    <div class="stat-divider"></div>
                    <div class="stat-item">
                        <span class="stat-number">{move || format_number(total_plays())}</span>
                        <span class="stat-label">"Total Plays"</span>
                    </div>
                </div>
            </section>
            
            // User Search Section
            <section class="search-section">
                <div class="search-container">
                    <div class="search-box large">
                        <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="11" cy="11" r="8"></circle>
                            <path d="m21 21-4.3-4.3"></path>
                        </svg>
                        <input 
                            type="text"
                            class="search-input-industrial"
                            placeholder="Search by username, email, or Discord..."
                            prop:value=move || search_query.get()
                            on:input=move |e| {
                                search_query.set(event_target_value(&e));
                            }
                            on:keydown=move |e: web_sys::KeyboardEvent| {
                                if e.key() == "Enter" {
                                    trigger_search(search_query, search_results, is_searching, api_url_search1.clone());
                                }
                            }
                        />
                        <button class="search-btn" on:click=move |_| trigger_search(search_query, search_results, is_searching, api_url_search2.clone())>
                            "Search"
                        </button>
                    </div>
                    <p class="search-hint">"Supports Eustress usernames, email addresses, and Discord usernames"</p>
                </div>
                
                // Search Results
                <Show when=move || is_searching.get() && !search_query.get().is_empty()>
                    <div class="search-results">
                        <h3 class="results-title">"Search Results"</h3>
                        <div class="results-grid">
                            <For
                                each=move || search_results.get()
                                key=|user| user.username.clone()
                                children=move |user| {
                                    let profile_url = format!("/profile/{}", user.username);
                                    view! {
                                        <a href=profile_url class="user-card">
                                            <div class="user-avatar">
                                                <img src="/assets/icons/user.svg" alt="Avatar" />
                                            </div>
                                            <div class="user-info">
                                                <div class="user-name-row">
                                                    <span class="user-display-name">{user.display_name}</span>
                                                    {user.is_verified.then(|| view! {
                                                        <img src="/assets/icons/check.svg" alt="Verified" class="verified-icon" />
                                                    })}
                                                </div>
                                                <span class="user-username">"@"{user.username}</span>
                                                <span class="user-followers">{format_number(user.follower_count)}" followers"</span>
                                            </div>
                                            <img src="/assets/icons/arrow-right.svg" alt="View" class="user-arrow" />
                                        </a>
                                    }
                                }
                            />
                        </div>
                    </div>
                </Show>
            </section>
            
            // Leaderboard Section
            <section class="leaderboard-section">
                <div class="leaderboard-header">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/trophy.svg" alt="Leaderboard" class="section-icon" />
                        <h2>"Leaderboard"</h2>
                    </div>
                    <p class="leaderboard-subtitle">"Top contributors by hours spent on Eustress"</p>
                </div>
                
                // Leaderboard Table - Top Creators
                <div class="leaderboard-container">
                    <div class="leaderboard-table">
                        <div class="table-header">
                            <span class="col-rank">"Rank"</span>
                            <span class="col-player">"Creator"</span>
                            <span class="col-hours">"Total Visits"</span>
                        </div>
                        <For
                            each=move || leaderboard.get()
                            key=|entry| entry.rank
                            children=move |entry| {
                                let profile_url = format!("/profile/{}", entry.username);
                                let rank_class = match entry.rank {
                                    1 => "rank gold",
                                    2 => "rank silver",
                                    3 => "rank bronze",
                                    _ => "rank",
                                };
                                view! {
                                    <a href=profile_url class="table-row">
                                        <span class=rank_class>
                                            {entry.badge.clone().unwrap_or_else(|| format!("#{}", entry.rank))}
                                        </span>
                                        <div class="player-cell">
                                            <div class="player-avatar">
                                                <img src="/assets/icons/user.svg" alt="Avatar" />
                                            </div>
                                            <span class="player-name">{entry.username.clone()}</span>
                                        </div>
                                        <span class="hours-cell">{format_number(entry.score)}</span>
                                    </a>
                                }
                            }
                        />
                    </div>
                </div>
                
                <div class="leaderboard-footer">
                    <a href="/leaderboard" class="btn-view-all">
                        "View Full Leaderboard"
                        <img src="/assets/icons/arrow-right.svg" alt="Arrow" />
                    </a>
                </div>
            </section>
            
            // Community Highlights
            <section class="highlights-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/star.svg" alt="Highlights" class="section-icon" />
                    <h2>"Community Highlights"</h2>
                </div>
                
                <div class="highlights-grid">
                    <div class="highlight-card featured">
                        <div class="highlight-badge">"FEATURED CREATOR"</div>
                        <div class="highlight-content">
                            <div class="creator-avatar large">
                                <img src="/assets/icons/user.svg" alt="Creator" />
                            </div>
                            <h3>"BuilderPro"</h3>
                            <p>"3,156 hours creating amazing worlds"</p>
                            <a href="/profile/BuilderPro" class="highlight-link">"View Profile"</a>
                        </div>
                    </div>
                    
                    <div class="highlight-card">
                        <div class="highlight-icon">
                            <img src="/assets/icons/trending.svg" alt="Trending" />
                        </div>
                        <h3>"Trending This Week"</h3>
                        <p>"Racing games are up 45% in plays"</p>
                        <a href="/gallery?category=racing" class="highlight-link">"Explore Racing"</a>
                    </div>
                    
                    <div class="highlight-card">
                        <div class="highlight-icon">
                            <img src="/assets/icons/calendar.svg" alt="Event" />
                        </div>
                        <h3>"Build Jam 2025"</h3>
                        <p>"48-hour game jam starting Feb 15"</p>
                        <a href="/events/build-jam-2025" class="highlight-link">"Join Event"</a>
                    </div>
                    
                    <div class="highlight-card">
                        <div class="highlight-icon">
                            <img src="/assets/icons/gift.svg" alt="Rewards" />
                        </div>
                        <h3>"Creator Rewards"</h3>
                        <p>"Earn credits for popular spaces"</p>
                        <a href="/rewards" class="highlight-link">"Learn More"</a>
                    </div>
                </div>
            </section>
            
            // Join CTA
            <section class="join-cta">
                <div class="join-card">
                    <div class="join-content">
                        <h2>"Ready to Join?"</h2>
                        <p>"Connect with creators, compete on leaderboards, and be part of something amazing"</p>
                    </div>
                    <div class="join-actions">
                        <a href="/login" class="btn-join primary">"Sign Up Free"</a>
                        <a href="/learn" class="btn-join secondary">"Learn More"</a>
                    </div>
                </div>
            </section>
            
            <Footer />
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
