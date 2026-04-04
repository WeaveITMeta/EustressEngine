// =============================================================================
// Eustress Web - Leaderboard Page (Industrial Design)
// =============================================================================
// Full leaderboard experience - Work Hard, Play Hard
// Detailed rankings for players and creators
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Leaderboard entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub username: String,
    pub avatar_url: Option<String>,
    pub hours: f64,
    pub level: u32,
    pub spaces_created: u32,
    pub total_visits: u64,
    pub streak_days: u32,
}

/// Time period filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimePeriod {
    AllTime,
    ThisMonth,
    ThisWeek,
    Today,
}

impl TimePeriod {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AllTime => "all",
            Self::ThisMonth => "month",
            Self::ThisWeek => "week",
            Self::Today => "today",
        }
    }
    
    fn display_name(&self) -> &'static str {
        match self {
            Self::AllTime => "All Time",
            Self::ThisMonth => "This Month",
            Self::ThisWeek => "This Week",
            Self::Today => "Today",
        }
    }
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Full leaderboard page — pulls real data from api.eustress.dev.
#[component]
pub fn LeaderboardPage() -> impl IntoView {
    let app_state = expect_context::<crate::state::AppState>();
    let time_period = RwSignal::new("alltime".to_string());
    let search_query = RwSignal::new(String::new());
    let entries = RwSignal::new(Vec::<LeaderboardEntry>::new());
    let featured = RwSignal::new(Option::<LeaderboardEntry>::None);
    let loading = RwSignal::new(true);

    // Fetch leaderboard from API whenever period changes
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let period = time_period.get();
        let url = format!("{}/api/community/leaderboard?period={}", api_url, period);
        loading.set(true);

        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(resp) = gloo_net::http::Request::get(&url).send().await {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    let mut parsed = Vec::new();
                    if let Some(arr) = data.get("entries").and_then(|v| v.as_array()) {
                        for (i, e) in arr.iter().enumerate() {
                            parsed.push(LeaderboardEntry {
                                rank: e.get("rank").and_then(|v| v.as_u64()).unwrap_or(i as u64 + 1) as u32,
                                username: e.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                avatar_url: e.get("avatar_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                hours: e.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                level: 0,
                                spaces_created: e.get("spaces_created").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                total_visits: e.get("total_visits").and_then(|v| v.as_u64()).unwrap_or(0),
                                streak_days: 0,
                            });
                        }
                    }
                    entries.set(parsed);

                    // Featured creator (random from top 20, rotates weekly)
                    if let Some(f) = data.get("featured") {
                        if f.is_object() {
                            featured.set(Some(LeaderboardEntry {
                                rank: 0,
                                username: f.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                avatar_url: f.get("avatar_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                hours: f.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                level: 0,
                                spaces_created: f.get("spaces_created").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                                total_visits: f.get("total_visits").and_then(|v| v.as_u64()).unwrap_or(0),
                                streak_days: 0,
                            }));
                        }
                    }
                }
            }
            loading.set(false);
        });
    });

    // Filter entries by search
    let filtered_entries = move || {
        let query = search_query.get().to_lowercase();
        let all = entries.get();
        if query.is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|e| e.username.to_lowercase().contains(&query))
                .collect()
        }
    };
    
    view! {
        <div class="page page-leaderboard-industrial">
            <CentralNav active="community".to_string() />
            
            // Background
            <div class="leaderboard-page-bg">
                <div class="leaderboard-grid-overlay"></div>
                <div class="leaderboard-glow glow-1"></div>
                <div class="leaderboard-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="leaderboard-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"LEADERBOARD"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="leaderboard-title">"Top Creators"</h1>
                <p class="leaderboard-subtitle">"The legends who define the Eustress community"</p>
                <a href="/leaderboard" class="btn-secondary-steel leaderboard-cta">
                    "View Full Leaderboard"
                </a>
            </section>
            
            // Controls
            <section class="leaderboard-controls">
                // Filters Row
                <div class="filters-row">
                    <div class="search-box">
                        <img src="/assets/icons/search.svg" alt="Search" class="search-icon" />
                        <input 
                            type="text"
                            class="search-input-industrial"
                            placeholder="Search users..."
                            prop:value=move || search_query.get()
                            on:input=move |e| search_query.set(event_target_value(&e))
                        />
                    </div>
                    
                    <div class="time-filters">
                        <button 
                            class="time-btn"
                            class:active=move || time_period.get() == "all"
                            on:click=move |_| time_period.set("all".to_string())
                        >"All Time"</button>
                        <button 
                            class="time-btn"
                            class:active=move || time_period.get() == "month"
                            on:click=move |_| time_period.set("month".to_string())
                        >"This Month"</button>
                        <button 
                            class="time-btn"
                            class:active=move || time_period.get() == "week"
                            on:click=move |_| time_period.set("week".to_string())
                        >"This Week"</button>
                        <button 
                            class="time-btn"
                            class:active=move || time_period.get() == "today"
                            on:click=move |_| time_period.set("today".to_string())
                        >"Today"</button>
                    </div>
                </div>
            </section>
            
            // Leaderboard Content
            <section class="leaderboard-content">
                // Play Leaderboard
                <Show when=move || active_tab.get() == "play">
                    <div class="full-leaderboard">
                        // Top 3 Podium
                        <div class="podium">
                            {play_leaderboard.get(1).map(|e| view! {
                                <div class="podium-place second">
                                    <div class="podium-avatar">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"2"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                            {play_leaderboard.first().map(|e| view! {
                                <div class="podium-place first">
                                    <div class="podium-crown">
                                        <img src="/assets/icons/trophy.svg" alt="Crown" />
                                    </div>
                                    <div class="podium-avatar gold">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"1"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                            {play_leaderboard.get(2).map(|e| view! {
                                <div class="podium-place third">
                                    <div class="podium-avatar">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"3"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                        </div>
                        
                        // Full Table
                        <div class="leaderboard-table-full">
                            <div class="table-header-full">
                                <span class="col-rank">"Rank"</span>
                                <span class="col-player">"Player"</span>
                                <span class="col-level">"Level"</span>
                                <span class="col-streak">"Streak"</span>
                                <span class="col-hours">"Hours"</span>
                            </div>
                            {filter_entries(play_leaderboard.clone()).into_iter().map(|entry| {
                                let profile_url = format!("/profile/{}", entry.username);
                                let rank_class = match entry.rank {
                                    1 => "rank-cell gold",
                                    2 => "rank-cell silver",
                                    3 => "rank-cell bronze",
                                    _ => "rank-cell",
                                };
                                view! {
                                    <a href=profile_url class="table-row-full">
                                        <span class=rank_class>{"#"}{entry.rank}</span>
                                        <div class="player-cell">
                                            <div class="player-avatar-sm">
                                                <img src="/assets/icons/user.svg" alt="Avatar" />
                                            </div>
                                            <span class="player-name">{entry.username}</span>
                                        </div>
                                        <span class="level-cell">
                                            <span class="level-badge">"Lv "{entry.level}</span>
                                        </span>
                                        <span class="streak-cell">
                                            <img src="/assets/icons/fire.svg" alt="Streak" class="streak-icon" />
                                            {entry.streak_days}" days"
                                        </span>
                                        <span class="hours-cell">{format!("{:.1}h", entry.hours)}</span>
                                    </a>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                </Show>
                
                // Work Leaderboard
                <Show when=move || active_tab.get() == "work">
                    <div class="full-leaderboard">
                        // Top 3 Podium
                        <div class="podium">
                            {work_leaderboard.get(1).map(|e| view! {
                                <div class="podium-place second">
                                    <div class="podium-avatar">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"2"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                            {work_leaderboard.first().map(|e| view! {
                                <div class="podium-place first">
                                    <div class="podium-crown">
                                        <img src="/assets/icons/trophy.svg" alt="Crown" />
                                    </div>
                                    <div class="podium-avatar gold">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"1"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                            {work_leaderboard.get(2).map(|e| view! {
                                <div class="podium-place third">
                                    <div class="podium-avatar">
                                        <img src="/assets/icons/user.svg" alt="Avatar" />
                                    </div>
                                    <span class="podium-rank">"3"</span>
                                    <span class="podium-name">{e.username.clone()}</span>
                                    <span class="podium-hours">{format!("{:.0}h", e.hours)}</span>
                                </div>
                            })}
                        </div>
                        
                        // Full Table
                        <div class="leaderboard-table-full">
                            <div class="table-header-full">
                                <span class="col-rank">"Rank"</span>
                                <span class="col-player">"Creator"</span>
                                <span class="col-spaces">"Spaces"</span>
                                <span class="col-visits">"Visits"</span>
                                <span class="col-hours">"Hours"</span>
                            </div>
                            {filter_entries(work_leaderboard.clone()).into_iter().map(|entry| {
                                let profile_url = format!("/profile/{}", entry.username);
                                let rank_class = match entry.rank {
                                    1 => "rank-cell gold",
                                    2 => "rank-cell silver",
                                    3 => "rank-cell bronze",
                                    _ => "rank-cell",
                                };
                                view! {
                                    <a href=profile_url class="table-row-full">
                                        <span class=rank_class>{"#"}{entry.rank}</span>
                                        <div class="player-cell">
                                            <div class="player-avatar-sm">
                                                <img src="/assets/icons/user.svg" alt="Avatar" />
                                            </div>
                                            <span class="player-name">{entry.username}</span>
                                        </div>
                                        <span class="spaces-cell">{entry.spaces_created}</span>
                                        <span class="visits-cell">{format_number(entry.total_visits)}</span>
                                        <span class="hours-cell">{format!("{:.1}h", entry.hours)}</span>
                                    </a>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                </Show>
            </section>
            
            // Your Rank Section
            <section class="your-rank-section">
                <div class="your-rank-card">
                    <div class="your-rank-content">
                        <h3>"Your Ranking"</h3>
                        <p>"Sign in to see where you stand on the leaderboard"</p>
                    </div>
                    <a href="/login" class="btn-sign-in">"Sign In"</a>
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
