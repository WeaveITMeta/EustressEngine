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

/// Full leaderboard page.
#[component]
pub fn LeaderboardPage() -> impl IntoView {
    // State
    let active_tab = RwSignal::new("work".to_string());
    let time_period = RwSignal::new("all".to_string());
    let search_query = RwSignal::new(String::new());
    
    // Sample Play leaderboard data (extended)
    let play_leaderboard = vec![
        LeaderboardEntry { rank: 1, username: "NightRider".to_string(), avatar_url: None, hours: 2847.5, level: 99, spaces_created: 0, total_visits: 0, streak_days: 365 },
        LeaderboardEntry { rank: 2, username: "SkyWalker99".to_string(), avatar_url: None, hours: 2654.2, level: 95, spaces_created: 0, total_visits: 0, streak_days: 280 },
        LeaderboardEntry { rank: 3, username: "PixelMaster".to_string(), avatar_url: None, hours: 2501.8, level: 92, spaces_created: 0, total_visits: 0, streak_days: 245 },
        LeaderboardEntry { rank: 4, username: "GameLord".to_string(), avatar_url: None, hours: 2389.3, level: 88, spaces_created: 0, total_visits: 0, streak_days: 200 },
        LeaderboardEntry { rank: 5, username: "AdventureSeeker".to_string(), avatar_url: None, hours: 2156.7, level: 85, spaces_created: 0, total_visits: 0, streak_days: 180 },
        LeaderboardEntry { rank: 6, username: "SpeedDemon".to_string(), avatar_url: None, hours: 2034.1, level: 82, spaces_created: 0, total_visits: 0, streak_days: 165 },
        LeaderboardEntry { rank: 7, username: "CosmicPlayer".to_string(), avatar_url: None, hours: 1987.4, level: 80, spaces_created: 0, total_visits: 0, streak_days: 150 },
        LeaderboardEntry { rank: 8, username: "VirtualHero".to_string(), avatar_url: None, hours: 1876.9, level: 78, spaces_created: 0, total_visits: 0, streak_days: 140 },
        LeaderboardEntry { rank: 9, username: "QuestRunner".to_string(), avatar_url: None, hours: 1765.2, level: 75, spaces_created: 0, total_visits: 0, streak_days: 130 },
        LeaderboardEntry { rank: 10, username: "EpicGamer".to_string(), avatar_url: None, hours: 1654.8, level: 72, spaces_created: 0, total_visits: 0, streak_days: 120 },
        LeaderboardEntry { rank: 11, username: "StarChaser".to_string(), avatar_url: None, hours: 1543.2, level: 70, spaces_created: 0, total_visits: 0, streak_days: 110 },
        LeaderboardEntry { rank: 12, username: "NeonKnight".to_string(), avatar_url: None, hours: 1432.6, level: 68, spaces_created: 0, total_visits: 0, streak_days: 100 },
        LeaderboardEntry { rank: 13, username: "PhantomX".to_string(), avatar_url: None, hours: 1321.9, level: 65, spaces_created: 0, total_visits: 0, streak_days: 95 },
        LeaderboardEntry { rank: 14, username: "BlazeFury".to_string(), avatar_url: None, hours: 1210.3, level: 62, spaces_created: 0, total_visits: 0, streak_days: 88 },
        LeaderboardEntry { rank: 15, username: "ShadowStrike".to_string(), avatar_url: None, hours: 1098.7, level: 60, spaces_created: 0, total_visits: 0, streak_days: 80 },
        LeaderboardEntry { rank: 16, username: "ThunderBolt".to_string(), avatar_url: None, hours: 987.1, level: 58, spaces_created: 0, total_visits: 0, streak_days: 75 },
        LeaderboardEntry { rank: 17, username: "IceStorm".to_string(), avatar_url: None, hours: 876.5, level: 55, spaces_created: 0, total_visits: 0, streak_days: 70 },
        LeaderboardEntry { rank: 18, username: "FireDragon".to_string(), avatar_url: None, hours: 765.8, level: 52, spaces_created: 0, total_visits: 0, streak_days: 65 },
        LeaderboardEntry { rank: 19, username: "WindWalker".to_string(), avatar_url: None, hours: 654.2, level: 50, spaces_created: 0, total_visits: 0, streak_days: 60 },
        LeaderboardEntry { rank: 20, username: "EarthShaker".to_string(), avatar_url: None, hours: 543.6, level: 48, spaces_created: 0, total_visits: 0, streak_days: 55 },
    ];
    
    // Sample Work leaderboard data (extended)
    let work_leaderboard = vec![
        LeaderboardEntry { rank: 1, username: "BuilderPro".to_string(), avatar_url: None, hours: 3156.2, level: 99, spaces_created: 47, total_visits: 15_000_000, streak_days: 400 },
        LeaderboardEntry { rank: 2, username: "CodeMaster".to_string(), avatar_url: None, hours: 2987.5, level: 97, spaces_created: 38, total_visits: 12_500_000, streak_days: 350 },
        LeaderboardEntry { rank: 3, username: "DesignWizard".to_string(), avatar_url: None, hours: 2845.1, level: 94, spaces_created: 32, total_visits: 10_800_000, streak_days: 320 },
        LeaderboardEntry { rank: 4, username: "ScriptNinja".to_string(), avatar_url: None, hours: 2654.8, level: 90, spaces_created: 28, total_visits: 8_900_000, streak_days: 290 },
        LeaderboardEntry { rank: 5, username: "WorldCreator".to_string(), avatar_url: None, hours: 2543.2, level: 87, spaces_created: 25, total_visits: 7_500_000, streak_days: 260 },
        LeaderboardEntry { rank: 6, username: "AssetForge".to_string(), avatar_url: None, hours: 2387.6, level: 84, spaces_created: 22, total_visits: 6_200_000, streak_days: 240 },
        LeaderboardEntry { rank: 7, username: "LevelArchitect".to_string(), avatar_url: None, hours: 2256.3, level: 81, spaces_created: 19, total_visits: 5_100_000, streak_days: 220 },
        LeaderboardEntry { rank: 8, username: "PluginDev".to_string(), avatar_url: None, hours: 2134.7, level: 78, spaces_created: 16, total_visits: 4_200_000, streak_days: 200 },
        LeaderboardEntry { rank: 9, username: "StudioPro".to_string(), avatar_url: None, hours: 1998.4, level: 75, spaces_created: 14, total_visits: 3_500_000, streak_days: 180 },
        LeaderboardEntry { rank: 10, username: "CreativeForce".to_string(), avatar_url: None, hours: 1876.1, level: 72, spaces_created: 12, total_visits: 2_900_000, streak_days: 165 },
        LeaderboardEntry { rank: 11, username: "ArtisanMaker".to_string(), avatar_url: None, hours: 1754.8, level: 70, spaces_created: 11, total_visits: 2_400_000, streak_days: 150 },
        LeaderboardEntry { rank: 12, username: "VoxelKing".to_string(), avatar_url: None, hours: 1632.5, level: 67, spaces_created: 10, total_visits: 2_000_000, streak_days: 140 },
        LeaderboardEntry { rank: 13, username: "TerrainMaster".to_string(), avatar_url: None, hours: 1510.2, level: 64, spaces_created: 9, total_visits: 1_700_000, streak_days: 130 },
        LeaderboardEntry { rank: 14, username: "ShaderGuru".to_string(), avatar_url: None, hours: 1387.9, level: 61, spaces_created: 8, total_visits: 1_400_000, streak_days: 120 },
        LeaderboardEntry { rank: 15, username: "PhysicsWiz".to_string(), avatar_url: None, hours: 1265.6, level: 58, spaces_created: 7, total_visits: 1_200_000, streak_days: 110 },
        LeaderboardEntry { rank: 16, username: "AudioEngineer".to_string(), avatar_url: None, hours: 1143.3, level: 55, spaces_created: 6, total_visits: 1_000_000, streak_days: 100 },
        LeaderboardEntry { rank: 17, username: "UIDesigner".to_string(), avatar_url: None, hours: 1021.0, level: 52, spaces_created: 5, total_visits: 850_000, streak_days: 90 },
        LeaderboardEntry { rank: 18, username: "NetworkPro".to_string(), avatar_url: None, hours: 898.7, level: 49, spaces_created: 4, total_visits: 700_000, streak_days: 80 },
        LeaderboardEntry { rank: 19, username: "AnimationAce".to_string(), avatar_url: None, hours: 776.4, level: 46, spaces_created: 3, total_visits: 550_000, streak_days: 70 },
        LeaderboardEntry { rank: 20, username: "ModelMaker".to_string(), avatar_url: None, hours: 654.1, level: 43, spaces_created: 2, total_visits: 400_000, streak_days: 60 },
    ];
    
    // Filter entries by search
    let filter_entries = move |entries: Vec<LeaderboardEntry>| {
        let query = search_query.get().to_lowercase();
        if query.is_empty() {
            entries
        } else {
            entries.into_iter()
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
