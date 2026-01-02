// =============================================================================
// Eustress Web - Experiences Page
// =============================================================================
// Table of Contents:
// 1. Imports and Types
// 2. Mock Data
// 3. Main Component
// 4. Experience Card Component
// 5. Server List Component
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use wasm_bindgen::prelude::*;
use web_sys::window;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};
use crate::api::friends::{
    FriendPrivateServer, FriendInServer, get_friend_private_servers, get_friends_in_experience,
    join_friend_server, Friend,
};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Experience/game data structure.
#[derive(Clone, Debug)]
pub struct Experience {
    pub id: String,
    pub title: String,
    pub description: String,
    pub thumbnail: String,
    pub creator_id: String,
    pub creator_name: String,
    pub tags: Vec<String>,
    pub player_count: u32,
    pub max_players: u32,
    pub rating: f32,
    pub visits: u64,
    pub twitter_url: Option<String>,
    pub discord_url: Option<String>,
}

/// Server instance data.
#[derive(Clone, Debug)]
pub struct GameServer {
    pub id: String,
    pub experience_id: String,
    pub name: String,
    pub player_count: u32,
    pub max_players: u32,
    pub ping: u32,
    pub region: String,
    pub is_friend_server: bool,
    pub friend_names: Vec<String>,
}

// -----------------------------------------------------------------------------
// 2. Mock Data
// -----------------------------------------------------------------------------

fn get_mock_experiences() -> Vec<Experience> {
    vec![
        Experience {
            id: "1".to_string(),
            title: "Obby Tower Challenge".to_string(),
            description: "Race to the top of the ultimate obstacle course! 500 stages of pure parkour madness.".to_string(),
            thumbnail: "/assets/thumbnails/obby.jpg".to_string(),
            creator_id: "parkourmaster".to_string(),
            creator_name: "ParkourMaster".to_string(),
            tags: vec!["Obby".to_string(), "Parkour".to_string(), "Challenge".to_string()],
            player_count: 12453,
            max_players: 50,
            rating: 4.7,
            visits: 45_000_000,
            twitter_url: Some("https://x.com/ParkourMaster".to_string()),
            discord_url: Some("https://discord.gg/obbymaster".to_string()),
        },
        Experience {
            id: "2".to_string(),
            title: "Eustress Royale".to_string(),
            description: "100 players drop onto an island. Only one survives. Battle royale action!".to_string(),
            thumbnail: "/assets/thumbnails/royale.jpg".to_string(),
            creator_id: "epicgamesstudio".to_string(),
            creator_name: "EpicGamesStudio".to_string(),
            tags: vec!["Battle Royale".to_string(), "Shooter".to_string(), "PvP".to_string()],
            player_count: 8921,
            max_players: 100,
            rating: 4.5,
            visits: 120_000_000,
            twitter_url: Some("https://x.com/EustressRoyale".to_string()),
            discord_url: Some("https://discord.gg/eustressroyale".to_string()),
        },
        Experience {
            id: "3".to_string(),
            title: "Business Tycoon Empire".to_string(),
            description: "Build your business empire from scratch. Manage resources, hire employees, dominate the market!".to_string(),
            thumbnail: "/assets/thumbnails/tycoon.jpg".to_string(),
            creator_id: "tycoondevs".to_string(),
            creator_name: "TycoonDevs".to_string(),
            tags: vec!["Tycoon".to_string(), "Simulation".to_string(), "Building".to_string()],
            player_count: 5632,
            max_players: 30,
            rating: 4.8,
            visits: 89_000_000,
            twitter_url: None,
            discord_url: Some("https://discord.gg/tycoonempire".to_string()),
        },
        Experience {
            id: "4".to_string(),
            title: "The Haunted Mansion".to_string(),
            description: "Explore the terrifying mansion with friends. Solve puzzles, escape the ghost!".to_string(),
            thumbnail: "/assets/thumbnails/horror.jpg".to_string(),
            creator_id: "spookystudios".to_string(),
            creator_name: "SpookyStudios".to_string(),
            tags: vec!["Horror".to_string(), "Co-op".to_string(), "Puzzle".to_string()],
            player_count: 3421,
            max_players: 8,
            rating: 4.6,
            visits: 34_000_000,
            twitter_url: Some("https://x.com/SpookyStudios".to_string()),
            discord_url: None,
        },
        Experience {
            id: "5".to_string(),
            title: "Racing Legends".to_string(),
            description: "High-speed racing with customizable cars. Compete in tournaments worldwide!".to_string(),
            thumbnail: "/assets/thumbnails/racing.jpg".to_string(),
            creator_id: "speeddemon".to_string(),
            creator_name: "SpeedDemon".to_string(),
            tags: vec!["Racing".to_string(), "Cars".to_string(), "Competitive".to_string()],
            player_count: 7845,
            max_players: 20,
            rating: 4.4,
            visits: 67_000_000,
            twitter_url: None,
            discord_url: None,
        },
        Experience {
            id: "6".to_string(),
            title: "Legends of Eustress".to_string(),
            description: "Epic RPG adventure with quests, dungeons, and legendary loot. Join guilds and raid bosses!".to_string(),
            thumbnail: "/assets/thumbnails/rpg.jpg".to_string(),
            creator_id: "rpgmasters".to_string(),
            creator_name: "RPGMasters".to_string(),
            tags: vec!["RPG".to_string(), "Adventure".to_string(), "MMO".to_string()],
            player_count: 15234,
            max_players: 200,
            rating: 4.9,
            visits: 230_000_000,
            twitter_url: Some("https://x.com/RPGMasters".to_string()),
            discord_url: Some("https://discord.gg/legendsofeustress".to_string()),
        },
    ]
}

fn get_mock_servers(experience_id: &str) -> Vec<GameServer> {
    vec![
        GameServer {
            id: "srv-1".to_string(),
            experience_id: experience_id.to_string(),
            name: "US East #1".to_string(),
            player_count: 47,
            max_players: 50,
            ping: 23,
            region: "US East".to_string(),
            is_friend_server: true,
            friend_names: vec!["Alex".to_string(), "Jordan".to_string()],
        },
        GameServer {
            id: "srv-2".to_string(),
            experience_id: experience_id.to_string(),
            name: "US West #3".to_string(),
            player_count: 38,
            max_players: 50,
            ping: 45,
            region: "US West".to_string(),
            is_friend_server: true,
            friend_names: vec!["Sam".to_string()],
        },
        GameServer {
            id: "srv-3".to_string(),
            experience_id: experience_id.to_string(),
            name: "EU Central #2".to_string(),
            player_count: 50,
            max_players: 50,
            ping: 89,
            region: "EU".to_string(),
            is_friend_server: false,
            friend_names: vec![],
        },
        GameServer {
            id: "srv-4".to_string(),
            experience_id: experience_id.to_string(),
            name: "Asia #1".to_string(),
            player_count: 42,
            max_players: 50,
            ping: 156,
            region: "Asia".to_string(),
            is_friend_server: false,
            friend_names: vec![],
        },
        GameServer {
            id: "srv-5".to_string(),
            experience_id: experience_id.to_string(),
            name: "US East #2".to_string(),
            player_count: 31,
            max_players: 50,
            ping: 28,
            region: "US East".to_string(),
            is_friend_server: false,
            friend_names: vec![],
        },
    ]
}

fn get_experience_by_id(id: &str) -> Option<Experience> {
    get_mock_experiences().into_iter().find(|e| e.id == id)
}

// -----------------------------------------------------------------------------
// 3. Main Component
// -----------------------------------------------------------------------------

/// Experiences discovery page.
#[component]
pub fn ExperiencesPage() -> impl IntoView {
    // Get experiences sorted by player count
    let experiences = {
        let mut exp = get_mock_experiences();
        exp.sort_by(|a, b| b.player_count.cmp(&a.player_count));
        exp
    };
    
    // Search and filter state
    let search_query = RwSignal::new(String::new());
    let selected_tag = RwSignal::new(Option::<String>::None);
    
    // All unique tags
    let all_tags: Vec<String> = {
        let mut tags: Vec<String> = experiences.iter()
            .flat_map(|e| e.tags.clone())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    };
    
    view! {
        <div class="page page-experiences">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="experiences-bg">
                <div class="bg-grid"></div>
                <div class="bg-glow bg-glow-1"></div>
                <div class="bg-glow bg-glow-2"></div>
            </div>
            
            // Hero Section
            <section class="experiences-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"DISCOVER"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="experiences-title">"Experiences"</h1>
                <p class="experiences-subtitle">"Explore millions of player-created worlds"</p>
            </section>
            
            // Search and Filters
            <section class="experiences-filters">
                <div class="search-bar">
                    <img src="/assets/icons/search.svg" alt="Search" class="search-icon" />
                    <input 
                        type="text" 
                        placeholder="Search experiences..."
                        class="search-input"
                        prop:value=move || search_query.get()
                        on:input=move |e| search_query.set(event_target_value(&e))
                    />
                </div>
                
                <div class="tag-filters">
                    <button 
                        class=move || if selected_tag.get().is_none() { "tag-btn active" } else { "tag-btn" }
                        on:click=move |_| selected_tag.set(None)
                    >
                        "All"
                    </button>
                    {all_tags.into_iter().map(|tag| {
                        let tag_clone = tag.clone();
                        let tag_for_check = tag.clone();
                        view! {
                            <button 
                                class=move || {
                                    if selected_tag.get().as_ref() == Some(&tag_for_check) { 
                                        "tag-btn active" 
                                    } else { 
                                        "tag-btn" 
                                    }
                                }
                                on:click=move |_| selected_tag.set(Some(tag_clone.clone()))
                            >
                                {tag}
                            </button>
                        }
                    }).collect::<Vec<_>>()}
                </div>
            </section>
            
            // Experience Grid
            <section class="experiences-grid">
                {experiences.into_iter().map(|exp| {
                    view! {
                        <ExperienceCard experience=exp />
                    }
                }).collect::<Vec<_>>()}
            </section>
            
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 4. Experience Card Component
// -----------------------------------------------------------------------------

#[component]
fn ExperienceCard(
    experience: Experience,
) -> impl IntoView {
    let id = experience.id.clone();
    let title = experience.title.clone();
    let creator_name = experience.creator_name.clone();
    let creator_id = experience.creator_id.clone();
    let player_count = format_player_count(experience.player_count);
    let rating = format!("{:.1}", experience.rating);
    let visits = format_visits(experience.visits);
    let tags: Vec<String> = experience.tags.iter().take(2).cloned().collect();
    let href = format!("/experience/{}", id);
    let creator_href = format!("/profile/{}", creator_id);
    
    view! {
        <a href=href class="experience-card">
            <div class="card-thumbnail">
                <div class="thumbnail-placeholder">
                    <img src="/assets/icons/gamepad.svg" alt="Game" />
                </div>
                <div class="card-player-count">
                    <img src="/assets/icons/users.svg" alt="Players" />
                    <span>{player_count}</span>
                </div>
            </div>
            
            <div class="card-content">
                <h3 class="card-title">{title}</h3>
                <p class="card-creator">
                    "by "
                    <a href=creator_href class="creator-link" on:click=|e| e.stop_propagation()>
                        {creator_name}
                    </a>
                </p>
                
                <div class="card-tags">
                    {tags.into_iter().map(|tag| {
                        let tag_encoded = tag.replace(' ', "+");
                        let tag_url = format!("/gallery?tag={}", tag_encoded);
                        view! { <a href=tag_url class="card-tag clickable">{tag}</a> }
                    }).collect::<Vec<_>>()}
                </div>
                
                <div class="card-stats">
                    <div class="stat">
                        <img src="/assets/icons/star.svg" alt="Rating" />
                        <span>{rating}</span>
                    </div>
                    <div class="stat">
                        <img src="/assets/icons/trending.svg" alt="Visits" />
                        <span>{visits}</span>
                    </div>
                </div>
            </div>
        </a>
    }
}

// -----------------------------------------------------------------------------
// 5. Experience Detail Modal
// -----------------------------------------------------------------------------

#[component]
fn ExperienceDetailModal(
    experience: Experience,
    on_close: impl Fn() + 'static + Clone,
) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let experience_id = experience.id.clone();
    
    // Loading states
    let is_loading_friends = RwSignal::new(true);
    let friend_servers = RwSignal::new(Vec::<FriendPrivateServer>::new());
    let friends_in_experience = RwSignal::new(Vec::<Friend>::new());
    
    // Load friend servers and friends in this experience from API
    Effect::new(move |_| {
        let api_url = app_state.api_url.clone();
        let token = app_state.get_token();
        let exp_id = experience_id.clone();
        
        spawn_local(async move {
            if let Some(t) = token {
                // Fetch friend private servers for this experience
                if let Ok(servers) = get_friend_private_servers(&api_url, &t, &exp_id).await {
                    friend_servers.set(servers);
                }
                
                // Fetch friends currently playing this experience
                if let Ok(friends) = get_friends_in_experience(&api_url, &t, &exp_id).await {
                    friends_in_experience.set(friends);
                }
            }
            is_loading_friends.set(false);
        });
    });
    
    // Mock servers for non-friend servers (would come from a different API)
    let servers = get_mock_servers(&experience.id);
    let other_servers: Vec<GameServer> = servers.iter()
        .filter(|s| !s.is_friend_server)
        .cloned()
        .collect();
    
    let on_close_bg = on_close.clone();
    let on_close_btn = on_close.clone();
    
    let title = experience.title.clone();
    let creator_name = experience.creator_name.clone();
    let creator_id = experience.creator_id.clone();
    let creator_href = format!("/profile/{}", creator_id);
    let description = experience.description.clone();
    let player_count = format_player_count(experience.player_count);
    let rating = format!("{:.1}", experience.rating);
    let visits = format_visits(experience.visits);
    let tags = experience.tags.clone();
    
    view! {
        <div class="modal-overlay" on:click=move |_| on_close_bg()>
            <div class="experience-modal" on:click=|e| e.stop_propagation()>
                <button class="modal-close" on:click=move |_| on_close_btn()>
                    "×"
                </button>
                
                // Header with thumbnail
                <div class="modal-header">
                    <div class="modal-thumbnail">
                        <div class="thumbnail-placeholder large">
                            <img src="/assets/icons/gamepad.svg" alt="Game" />
                        </div>
                    </div>
                    
                    <div class="modal-info">
                        <h2 class="modal-title">{title}</h2>
                        <p class="modal-creator">
                            "by "
                            <a href=creator_href class="creator-link">{creator_name}</a>
                        </p>
                        
                        <div class="modal-stats">
                            <div class="stat-box">
                                <span class="stat-value">{player_count}</span>
                                <span class="stat-label">"Playing"</span>
                            </div>
                            <div class="stat-box">
                                <span class="stat-value">{rating}</span>
                                <span class="stat-label">"Rating"</span>
                            </div>
                            <div class="stat-box">
                                <span class="stat-value">{visits}</span>
                                <span class="stat-label">"Visits"</span>
                            </div>
                        </div>
                        
                        <div class="modal-tags">
                            {tags.into_iter().map(|tag| {
                                let tag_encoded = tag.replace(' ', "+");
                                let tag_url = format!("/gallery?tag={}", tag_encoded);
                                view! { <a href=tag_url class="modal-tag clickable">{tag}</a> }
                            }).collect::<Vec<_>>()}
                        </div>
                    </div>
                </div>
                
                // Description
                <div class="modal-description">
                    <p>{description}</p>
                </div>
                
                // Play Button
                <div class="modal-actions">
                    <button class="btn-play">
                        <img src="/assets/icons/play.svg" alt="Play" />
                        "Play Now"
                    </button>
                    <button class="btn-favorite">
                        <img src="/assets/icons/heart.svg" alt="Favorite" />
                    </button>
                </div>
                
                // Friend Servers (from real API)
                {move || {
                    let servers = friend_servers.get();
                    let friends = friends_in_experience.get();
                    let loading = is_loading_friends.get();
                    
                    if loading {
                        view! {
                            <div class="server-section">
                                <h3 class="section-title">
                                    <img src="/assets/icons/users.svg" alt="Friends" />
                                    "Friends Playing"
                                </h3>
                                <div class="loading-inline">
                                    <div class="spinner-small"></div>
                                    <span>"Loading friend servers..."</span>
                                </div>
                            </div>
                        }.into_any()
                    } else if servers.is_empty() && friends.is_empty() {
                        view! { <div></div> }.into_any()
                    } else {
                        view! {
                            <div class="server-section">
                                <h3 class="section-title">
                                    <img src="/assets/icons/users.svg" alt="Friends" />
                                    "Friends Playing"
                                </h3>
                                
                                // Friends in public servers
                                {if !friends.is_empty() {
                                    view! {
                                        <div class="friends-playing">
                                            {friends.into_iter().map(|friend| {
                                                let display_name = friend.display_name.clone();
                                                let server_id = friend.current_server_id.clone();
                                                let has_server = server_id.is_some();
                                                view! {
                                                    <div class="friend-playing-row">
                                                        <div class="friend-avatar-small">
                                                            {display_name.chars().next().unwrap_or('?').to_string()}
                                                        </div>
                                                        <span class="friend-name">{display_name}</span>
                                                        {if has_server {
                                                            view! {
                                                                <button class="btn-join-small">"Join"</button>
                                                            }.into_any()
                                                        } else {
                                                            view! { <span></span> }.into_any()
                                                        }}
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}
                                
                                // Friend private servers
                                {if !servers.is_empty() {
                                    view! {
                                        <div class="friend-private-servers">
                                            <h4 class="subsection-title">"Private Servers"</h4>
                                            {servers.into_iter().map(|server| {
                                                view! { <FriendServerRow server=server /> }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                } else {
                                    view! { <div></div> }.into_any()
                                }}
                            </div>
                        }.into_any()
                    }
                }}
                
                // All Servers
                <div class="server-section">
                    <h3 class="section-title">
                        <img src="/assets/icons/network.svg" alt="Servers" />
                        "All Servers"
                    </h3>
                    <div class="server-list">
                        {other_servers.into_iter().map(|server| {
                            view! { <ServerRow server=server /> }
                        }).collect::<Vec<_>>()}
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Friend private server row component
#[component]
fn FriendServerRow(server: FriendPrivateServer) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let server_id = server.server_id.clone();
    let owner_username = server.owner_username.clone();
    let player_count = server.player_count;
    let max_players = server.max_players;
    let can_join = server.can_join && player_count < max_players;
    
    let capacity_percent = (player_count as f32 / max_players as f32 * 100.0) as u32;
    let capacity_class = if capacity_percent >= 90 { "full" } 
        else if capacity_percent >= 70 { "busy" } 
        else { "available" };
    
    let friends_list: String = if server.friends_in_server.len() <= 2 {
        server.friends_in_server.iter()
            .map(|f| f.display_name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let first: Vec<_> = server.friends_in_server.iter()
            .take(2)
            .map(|f| f.display_name.clone())
            .collect();
        format!("{} +{}", first.join(", "), server.friends_in_server.len() - 2)
    };
    
    let capacity = format!("{}/{}", player_count, max_players);
    let capacity_cls = format!("server-capacity {}", capacity_class);
    
    let is_joining = RwSignal::new(false);
    let join_error = RwSignal::new(Option::<String>::None);
    
    let handle_join = move |_| {
        let api_url = app_state.api_url.clone();
        let token = app_state.get_token();
        let srv_id = server_id.clone();
        
        is_joining.set(true);
        join_error.set(None);
        
        spawn_local(async move {
            if let Some(t) = token {
                match join_friend_server(&api_url, &t, &srv_id).await {
                    Ok(response) => {
                        // In production, this would trigger the game client to connect
                        // For now, log the connection info
                        web_sys::console::log_1(&format!(
                            "Joining server at {}:{} with token {}",
                            response.server_address, response.server_port, response.join_token
                        ).into());
                    }
                    Err(e) => {
                        join_error.set(Some(e));
                    }
                }
            }
            is_joining.set(false);
        });
    };
    
    view! {
        <div class="friend-server-row">
            <div class="server-info">
                <span class="server-owner">"Hosted by "{owner_username}</span>
                <span class="server-friends-list">{friends_list}</span>
            </div>
            
            <div class="server-stats">
                <span class=capacity_cls>{capacity}</span>
            </div>
            
            {move || {
                if let Some(err) = join_error.get() {
                    view! { <span class="join-error">{err}</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }
            }}
            
            <button 
                class="btn-join"
                disabled=move || !can_join || is_joining.get()
                on:click=handle_join
            >
                {move || if is_joining.get() { "Joining..." } else { "Join" }}
            </button>
        </div>
    }
}

#[component]
fn ServerRow(server: GameServer) -> impl IntoView {
    let capacity_percent = (server.player_count as f32 / server.max_players as f32 * 100.0) as u32;
    let capacity_class = if capacity_percent >= 90 { "full" } 
        else if capacity_percent >= 70 { "busy" } 
        else { "available" };
    
    let name = server.name.clone();
    let region = server.region.clone();
    let friends = server.friend_names.join(", ");
    let has_friends = !server.friend_names.is_empty();
    let capacity = format!("{}/{}", server.player_count, server.max_players);
    let ping = format!("{} ms", server.ping);
    let capacity_cls = format!("server-capacity {}", capacity_class);
    
    view! {
        <div class="server-row">
            <div class="server-info">
                <span class="server-name">{name}</span>
                <span class="server-region">{region}</span>
                {if has_friends {
                    view! {
                        <span class="server-friends">{friends}</span>
                    }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>
            
            <div class="server-stats">
                <span class=capacity_cls>{capacity}</span>
                <span class="server-ping">{ping}</span>
            </div>
            
            <button class="btn-join">"Join"</button>
        </div>
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn format_player_count(count: u32) -> String {
    if count >= 1000 {
        format!("{:.1}K", count as f32 / 1000.0)
    } else {
        count.to_string()
    }
}

fn format_visits(visits: u64) -> String {
    if visits >= 1_000_000_000 {
        format!("{:.1}B", visits as f64 / 1_000_000_000.0)
    } else if visits >= 1_000_000 {
        format!("{:.1}M", visits as f64 / 1_000_000.0)
    } else if visits >= 1000 {
        format!("{:.1}K", visits as f64 / 1000.0)
    } else {
        visits.to_string()
    }
}

// -----------------------------------------------------------------------------
// Platform Detection
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Platform {
    Windows,
    MacOS,
    Linux,
    Android,
    IOS,
    Unknown,
}

impl Platform {
    fn detect() -> Self {
        let user_agent = window()
            .and_then(|w| w.navigator().user_agent().ok())
            .unwrap_or_default()
            .to_lowercase();
        
        if user_agent.contains("android") {
            Platform::Android
        } else if user_agent.contains("iphone") || user_agent.contains("ipad") {
            Platform::IOS
        } else if user_agent.contains("win") {
            Platform::Windows
        } else if user_agent.contains("mac") {
            Platform::MacOS
        } else if user_agent.contains("linux") {
            Platform::Linux
        } else {
            Platform::Unknown
        }
    }
    
    fn display_name(&self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::MacOS => "macOS",
            Platform::Linux => "Linux",
            Platform::Android => "Android",
            Platform::IOS => "iOS",
            Platform::Unknown => "your platform",
        }
    }
    
    fn download_url(&self) -> &'static str {
        match self {
            Platform::Windows => "https://downloads.eustress.dev/player/windows/EustressPlayer-Setup.exe",
            Platform::MacOS => "https://downloads.eustress.dev/player/mac/EustressPlayer.dmg",
            Platform::Linux => "https://downloads.eustress.dev/player/linux/EustressPlayer.AppImage",
            Platform::Android => "https://downloads.eustress.dev/player/android/EustressPlayer.apk",
            Platform::IOS => "https://apps.apple.com/app/eustress-player/id123456789",
            Platform::Unknown => "https://downloads.eustress.dev/",
        }
    }
    
    fn icon_path(&self) -> &'static str {
        match self {
            Platform::Windows => "/assets/icons/windows.svg",
            Platform::MacOS => "/assets/icons/macos.svg",
            Platform::Linux => "/assets/icons/linux.svg",
            Platform::Android => "/assets/icons/android.svg",
            Platform::IOS => "/assets/icons/ios.svg",
            Platform::Unknown => "/assets/icons/download.svg",
        }
    }
}

// -----------------------------------------------------------------------------
// Play Modal Component
// -----------------------------------------------------------------------------

#[component]
fn PlayModal(
    experience_id: String,
    on_close: impl Fn() + 'static + Clone,
) -> impl IntoView {
    let platform = Platform::detect();
    let platform_name = platform.display_name().to_string();
    let download_url = platform.download_url().to_string();
    let icon_path = platform.icon_path().to_string();
    let exp_id = experience_id.clone();
    
    let on_close_bg = on_close.clone();
    let on_close_btn = on_close.clone();
    
    // Try to launch via protocol
    let try_launch = move |_| {
        if let Some(win) = window() {
            let launch_url = format!("eustress://play/{}", exp_id);
            let _ = win.location().set_href(&launch_url);
        }
    };
    
    view! {
        <div class="modal-overlay play-modal-overlay" on:click=move |_| on_close_bg()>
            <div class="play-modal" on:click=|e| e.stop_propagation()>
                <button class="modal-close" on:click=move |_| on_close_btn()>
                    "×"
                </button>
                
                <div class="play-modal-content">
                    <div class="play-icon">
                        <img src="/assets/icons/download.svg" alt="Download" />
                    </div>
                    <h2>"Eustress Player Required"</h2>
                    <p class="play-subtitle">
                        "To play this experience, you need to install Eustress Player."
                    </p>
                    
                    <div class="play-actions">
                        <a href=download_url.clone() class="btn-download-player">
                            <img src=icon_path alt="Platform" />
                            "Download for " {platform_name}
                        </a>
                    </div>
                    
                    <div class="play-alternative">
                        <p>"Already have Eustress Player installed?"</p>
                        <button class="btn-retry" on:click=try_launch>
                            "Try Again"
                        </button>
                    </div>
                    
                    <div class="play-info">
                        <h3>"Other Platforms"</h3>
                        <div class="platform-links">
                            <a href="https://downloads.eustress.dev/player/windows/EustressPlayer-Setup.exe" class="platform-link">
                                <img src="/assets/icons/windows.svg" alt="Windows" />
                                "Windows"
                            </a>
                            <a href="https://downloads.eustress.dev/player/mac/EustressPlayer.dmg" class="platform-link">
                                <img src="/assets/icons/macos.svg" alt="macOS" />
                                "macOS"
                            </a>
                            <a href="https://downloads.eustress.dev/player/linux/EustressPlayer.AppImage" class="platform-link">
                                <img src="/assets/icons/linux.svg" alt="Linux" />
                                "Linux"
                            </a>
                            <a href="https://downloads.eustress.dev/player/redox/EustressPlayer" class="platform-link">
                                <img src="/assets/icons/redox.svg" alt="Redox" />
                                "Redox"
                            </a>
                            <a href="https://downloads.eustress.dev/player/android/EustressPlayer.apk" class="platform-link">
                                <img src="/assets/icons/android.svg" alt="Android" />
                                "Android"
                            </a>
                            <a href="https://apps.apple.com/app/eustress-player/id123456789" class="platform-link">
                                <img src="/assets/icons/ios.svg" alt="iOS" />
                                "iOS"
                            </a>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 6. Experience Detail Page (standalone page with URL param)
// -----------------------------------------------------------------------------

/// Standalone experience detail page - accessed via /experience/:id
#[component]
pub fn ExperienceDetailPage() -> impl IntoView {
    let params = use_params_map();
    let experience_id = move || params.read().get("id").unwrap_or_default();
    let show_play_modal = RwSignal::new(false);
    
    view! {
        <div class="page page-experience-detail">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="experiences-bg">
                <div class="bg-grid"></div>
                <div class="bg-glow bg-glow-1"></div>
                <div class="bg-glow bg-glow-2"></div>
            </div>
            
            {move || {
                let id = experience_id();
                match get_experience_by_id(&id) {
                    Some(experience) => {
                        let servers = get_mock_servers(&experience.id);
                        let friend_servers: Vec<GameServer> = servers.iter()
                            .filter(|s| s.is_friend_server)
                            .cloned()
                            .collect();
                        let other_servers: Vec<GameServer> = servers.iter()
                            .filter(|s| !s.is_friend_server)
                            .cloned()
                            .collect();
                        
                        let title = experience.title.clone();
                        let creator_name = experience.creator_name.clone();
                        let creator_id = experience.creator_id.clone();
                        let creator_href = format!("/profile/{}", creator_id);
                        let description = experience.description.clone();
                        let player_count = format_player_count(experience.player_count);
                        let rating = format!("{:.1}", experience.rating);
                        let visits = format_visits(experience.visits);
                        let tags = experience.tags.clone();
                        let has_friend_servers = !friend_servers.is_empty();
                        let exp_id_for_modal = experience.id.clone();
                        let twitter_url = experience.twitter_url.clone();
                        let discord_url = experience.discord_url.clone();
                        let has_links = twitter_url.is_some() || discord_url.is_some();
                        
                        // Tab state
                        let active_tab = RwSignal::new("about".to_string());
                        
                        view! {
                            <div class="experience-detail-container">
                                // Header with thumbnail
                                <div class="detail-header">
                                    <div class="detail-thumbnail">
                                        <div class="thumbnail-placeholder large">
                                            <img src="/assets/icons/gamepad.svg" alt="Game" />
                                        </div>
                                    </div>
                                    
                                    <div class="detail-info">
                                        <h1 class="detail-title">{title}</h1>
                                        <p class="detail-creator">
                                            "by "
                                            <a href=creator_href class="creator-link">{creator_name}</a>
                                        </p>
                                        
                                        <div class="detail-stats">
                                            <div class="stat-box">
                                                <span class="stat-value">{player_count}</span>
                                                <span class="stat-label">"Playing"</span>
                                            </div>
                                            <div class="stat-box">
                                                <span class="stat-value">{rating}</span>
                                                <span class="stat-label">"Rating"</span>
                                            </div>
                                            <div class="stat-box">
                                                <span class="stat-value">{visits}</span>
                                                <span class="stat-label">"Visits"</span>
                                            </div>
                                        </div>
                                        
                                        <div class="detail-tags">
                                            {tags.into_iter().map(|tag| {
                                                view! { <span class="modal-tag">{tag}</span> }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    </div>
                                </div>
                                
                                // Tabs: About / Links
                                <div class="detail-tabs-section">
                                    <div class="detail-tabs">
                                        <button 
                                            class="detail-tab"
                                            class:active=move || active_tab.get() == "about"
                                            on:click=move |_| active_tab.set("about".to_string())
                                        >
                                            "About"
                                        </button>
                                        <button 
                                            class="detail-tab"
                                            class:active=move || active_tab.get() == "links"
                                            on:click=move |_| active_tab.set("links".to_string())
                                        >
                                            "Links"
                                        </button>
                                    </div>
                                    
                                    // Tab Content
                                    <div class="detail-tab-content">
                                        <Show when=move || active_tab.get() == "about">
                                            <div class="tab-about">
                                                <p>{description.clone()}</p>
                                            </div>
                                        </Show>
                                        
                                        <Show when=move || active_tab.get() == "links">
                                            <div class="tab-links">
                                                {if has_links {
                                                    view! {
                                                        <div class="links-list">
                                                            {twitter_url.clone().map(|url| view! {
                                                                <a href=url target="_blank" rel="noopener" class="social-link twitter">
                                                                    <img src="/assets/icons/twitter-x.svg" alt="X" />
                                                                    <span>"X (Twitter)"</span>
                                                                </a>
                                                            })}
                                                            {discord_url.clone().map(|url| view! {
                                                                <a href=url target="_blank" rel="noopener" class="social-link discord">
                                                                    <img src="/assets/icons/discord.svg" alt="Discord" />
                                                                    <span>"Discord"</span>
                                                                </a>
                                                            })}
                                                        </div>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <p class="no-links">"No social links available for this experience."</p>
                                                    }.into_any()
                                                }}
                                            </div>
                                        </Show>
                                    </div>
                                </div>
                                
                                // Play Button
                                <div class="detail-actions">
                                    <button class="btn-play" on:click=move |_| show_play_modal.set(true)>
                                        <img src="/assets/icons/play.svg" alt="Play" />
                                        "Play Now"
                                    </button>
                                    <button class="btn-favorite">
                                        <img src="/assets/icons/heart.svg" alt="Favorite" />
                                    </button>
                                    <button class="btn-share">
                                        <img src="/assets/icons/copy.svg" alt="Share" />
                                        "Share"
                                    </button>
                                </div>
                                
                                // Servers Section
                                <div class="detail-servers">
                                    // Friend Servers
                                    {if has_friend_servers {
                                        view! {
                                            <div class="server-section">
                                                <h3 class="section-title">
                                                    <img src="/assets/icons/users.svg" alt="Friends" />
                                                    "Friends Playing"
                                                </h3>
                                                <div class="server-list">
                                                    {friend_servers.into_iter().map(|server| {
                                                        view! { <ServerRow server=server /> }
                                                    }).collect::<Vec<_>>()}
                                                </div>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }}
                                    
                                    // All Servers
                                    <div class="server-section">
                                        <h3 class="section-title">
                                            <img src="/assets/icons/network.svg" alt="Servers" />
                                            "All Servers"
                                        </h3>
                                        <div class="server-list">
                                            {other_servers.into_iter().map(|server| {
                                                view! { <ServerRow server=server /> }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    </div>
                                </div>
                                
                                // Play Modal
                                <Show when=move || show_play_modal.get()>
                                    <PlayModal 
                                        experience_id=exp_id_for_modal.clone()
                                        on_close=move || show_play_modal.set(false)
                                    />
                                </Show>
                            </div>
                        }.into_any()
                    }
                    None => {
                        view! {
                            <div class="experience-not-found">
                                <h1>"Experience Not Found"</h1>
                                <p>"The experience you're looking for doesn't exist or has been removed."</p>
                                <a href="/experiences" class="btn btn-primary">"Browse Experiences"</a>
                            </div>
                        }.into_any()
                    }
                }
            }}
            
            <Footer />
        </div>
    }
}
