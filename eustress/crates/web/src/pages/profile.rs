// =============================================================================
// Eustress Web - Profile Page (Industrial Design)
// =============================================================================
// Social profile with avatar display and user content
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use serde::Deserialize;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

const API_URL: &str = "https://api.eustress.dev";

/// User profile data.
#[derive(Clone, Debug, PartialEq)]
pub struct UserProfile {
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub join_date: String,
    pub follower_count: u64,
    pub following_count: u64,
    pub experience_count: u32,
    pub total_plays: u64,
    pub badges: Vec<Badge>,
    pub is_verified: bool,
    pub is_following: bool,
    pub discord_linked: bool,
}

/// User badge/achievement.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
}

/// API response for user profile.
#[derive(Clone, Debug, Deserialize)]
struct ProfileApiResponse {
    username: String,
    display_name: String,
    bio: String,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    join_date: String,
    follower_count: u64,
    following_count: u64,
    friend_count: Option<u64>,
    simulation_count: u32,
    total_plays: u64,
    favorite_count: Option<u64>,
    inventory_count: Option<u64>,
    badges: Vec<Badge>,
    is_verified: bool,
    is_following: Option<bool>,
    discord_linked: bool,
}

/// User's published experience.
#[derive(Clone, Debug, PartialEq)]
pub struct UserExperience {
    pub id: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub play_count: u64,
}

/// Profile page component.
#[component]
pub fn ProfilePage() -> impl IntoView {
    let params = use_params_map();
    let app_state = expect_context::<AppState>();
    
    // Get username from URL or current user
    let username = move || {
        params.get().get("username").unwrap_or_else(|| {
            match app_state.auth.get() {
                AuthState::Authenticated(user) => user.username,
                _ => "guest".to_string(),
            }
        })
    };

    let profile = RwSignal::new(UserProfile {
        username: String::new(),
        display_name: String::new(),
        bio: String::new(),
        avatar_url: None,
        banner_url: None,
        join_date: String::new(),
        follower_count: 0,
        following_count: 0,
        experience_count: 0,
        total_plays: 0,
        badges: vec![],
        is_verified: false,
        is_following: false,
        discord_linked: false,
    });

    let experiences = RwSignal::new(Vec::<UserExperience>::new());
    let profile_loading = RwSignal::new(true);
    let profile_error = RwSignal::new(Option::<String>::None);

    // Fetch profile from Cloudflare Worker
    let uname = username();
    spawn_local(async move {
        let url = format!("{}/api/community/users/{}", API_URL, urlencoding::encode(&uname));
        match gloo_net::http::Request::get(&url).send().await {
            Ok(resp) if resp.ok() => {
                if let Ok(data) = resp.json::<ProfileApiResponse>().await {
                    profile.set(UserProfile {
                        username: data.username.clone(),
                        display_name: data.display_name,
                        bio: data.bio,
                        avatar_url: data.avatar_url,
                        banner_url: data.banner_url,
                        join_date: data.join_date,
                        follower_count: data.follower_count,
                        following_count: data.following_count,
                        experience_count: data.simulation_count,
                        total_plays: data.total_plays,
                        badges: data.badges,
                        is_verified: data.is_verified,
                        is_following: data.is_following.unwrap_or(false),
                        discord_linked: data.discord_linked,
                    });
                }
            }
            Ok(resp) => {
                let status = resp.status();
                profile_error.set(Some(format!("User not found ({})", status)));
            }
            Err(e) => {
                profile_error.set(Some(format!("Failed to load profile: {}", e)));
            }
        }
        profile_loading.set(false);
    });
    
    // Determine if this is the owner viewing their own profile
    let is_own_profile = move || {
        let auth = app_state.auth.get();
        match auth {
            AuthState::Authenticated(user) => {
                let p = profile.get();
                !p.username.is_empty() && user.username == p.username
            }
            _ => false,
        }
    };

    // Default tab: Simulations for public view, Avatar for own profile
    let active_tab = RwSignal::new("simulations".to_string());

    // Follow state
    let is_following = RwSignal::new(false);
    
    view! {
        <div class="page page-profile-industrial">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="profile-bg">
                <div class="profile-grid-overlay"></div>
                <div class="profile-glow glow-1"></div>
                <div class="profile-glow glow-2"></div>
            </div>
            
            // Profile header with banner
            <div class="profile-header-industrial">
                <div class="profile-banner">
                    <div class="banner-gradient"></div>
                </div>
                
                <div class="profile-header-content">
                    // Avatar with hover overlay
                    <div class="avatar-container">
                        <div class="avatar-frame">
                            <img src="/assets/icons/noob-head.svg" alt="Avatar" class="avatar-img" />
                            <div class="avatar-hover-overlay">
                                <span>"Customize Avatar"</span>
                            </div>
                        </div>
                        <button class="customize-avatar-btn" title="Customize Avatar">
                            <img src="/assets/icons/edit.svg" alt="Edit" />
                        </button>
                    </div>
                    
                    // Profile info
                    <div class="profile-info">
                        <div class="profile-name-row">
                            <div class="name-with-links">
                                <h1 class="display-name">
                                    {move || {
                                        let p = profile.get();
                                        if p.display_name.is_empty() { p.username.clone() } else { p.display_name.clone() }
                                    }}
                                </h1>
                                // Linked account icons
                                <div class="linked-accounts-icons">
                                    {move || profile.get().discord_linked.then(|| view! {
                                        <a href="#" class="linked-icon" title="Discord Linked">
                                            <img src="/assets/icons/discord.svg" alt="Discord" />
                                        </a>
                                    })}
                                </div>
                            </div>
                            <span class="username">"@" {move || profile.get().username}</span>
                        </div>
                        
                        <p class="bio">{move || profile.get().bio}</p>
                        
                        <div class="profile-meta">
                            <span class="join-date">
                                <img src="/assets/icons/calendar.svg" alt="Joined" />
                                "Joined " {move || profile.get().join_date}
                            </span>
                        </div>
                    </div>
                </div>
            </div>
            
            <div class="profile-container">
                // Stats and Actions Row
                <div class="profile-stats-row">
                    <div class="profile-stats">
                        <div class="stat-card">
                            <span class="stat-value">{move || format_number(profile.get().follower_count)}</span>
                            <span class="stat-label">"Followers"</span>
                        </div>
                        <div class="stat-card">
                            <span class="stat-value">{move || format_number(profile.get().following_count)}</span>
                            <span class="stat-label">"Following"</span>
                        </div>
                        <div class="stat-card">
                            <span class="stat-value">{move || profile.get().experience_count.to_string()}</span>
                            <span class="stat-label">"Simulations"</span>
                        </div>
                        <div class="stat-card">
                            <span class="stat-value">{move || format_number(profile.get().total_plays)}</span>
                            <span class="stat-label">"Total Plays"</span>
                        </div>
                    </div>
                    
                    <div class="profile-actions">
                        {move || {
                            let auth = app_state.auth.get();
                            let viewing_self = match &auth {
                                AuthState::Authenticated(user) => user.username == profile.get().username,
                                _ => false,
                            };
                            let is_authed = auth.is_authenticated();

                            if is_authed && !viewing_self && !profile.get().username.is_empty() {
                                Some(view! {
                                    <button
                                        class="btn-follow"
                                        class:following=move || is_following.get()
                                        on:click=move |_| is_following.update(|f| *f = !*f)
                                    >
                                        <img src="/assets/icons/user.svg" alt="Follow" />
                                        {move || if is_following.get() { "Following" } else { "Follow" }}
                                    </button>
                                })
                            } else {
                                None
                            }
                        }}
                        <button class="btn-icon" title="Share">
                            <img src="/assets/icons/upload.svg" alt="Share" />
                        </button>
                    </div>
                </div>
                
                // Badges section
                <section class="profile-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/trophy.svg" alt="Badges" class="section-icon" />
                        <h2>"Badges"</h2>
                    </div>
                    <div class="badges-grid">
                        <For
                            each=move || profile.get().badges
                            key=|badge| badge.id.clone()
                            children=move |badge| view! {
                                <div class="badge-card" title=badge.description.clone()>
                                    <div class="badge-icon-wrapper">
                                        <img src="/assets/icons/star.svg" alt=badge.name.clone() />
                                    </div>
                                    <span class="badge-name">{badge.name}</span>
                                </div>
                            }
                        />
                    </div>
                </section>
                
                // Content tabs — public shows Simulations only, owner sees all
                <div class="profile-tabs">
                    // Simulations — always visible (public)
                    <button
                        class="profile-tab"
                        class:active=move || active_tab.get() == "simulations"
                        on:click=move |_| active_tab.set("simulations".to_string())
                    >
                        <img src="/assets/icons/gamepad.svg" alt="Simulations" />
                        "Simulations"
                    </button>

                    // Private tabs — only visible to profile owner
                    {move || is_own_profile().then(|| view! {
                        <button
                            class="profile-tab"
                            class:active=move || active_tab.get() == "avatar"
                            on:click=move |_| active_tab.set("avatar".to_string())
                        >
                            <img src="/assets/icons/user.svg" alt="Avatar" />
                            "Avatar"
                        </button>
                        <button
                            class="profile-tab"
                            class:active=move || active_tab.get() == "favorites"
                            on:click=move |_| active_tab.set("favorites".to_string())
                        >
                            <img src="/assets/icons/heart.svg" alt="Favorites" />
                            "Favorites"
                        </button>
                        <button
                            class="profile-tab"
                            class:active=move || active_tab.get() == "friends"
                            on:click=move |_| active_tab.set("friends".to_string())
                        >
                            <img src="/assets/icons/users.svg" alt="Friends" />
                            "Friends"
                        </button>
                        <button
                            class="profile-tab"
                            class:active=move || active_tab.get() == "inventory"
                            on:click=move |_| active_tab.set("inventory".to_string())
                        >
                            <img src="/assets/icons/archive.svg" alt="Inventory" />
                            "Inventory"
                        </button>
                    })}
                </div>

                // Tab content
                <div class="profile-tab-content">
                    // Avatar — private only
                    <Show when=move || active_tab.get() == "avatar" && is_own_profile()>
                        <div class="avatar-customizer">
                            <div class="avatar-preview-section">
                                // 3D character preview (placeholder — connects to engine's character system)
                                <div class="avatar-viewport">
                                    <div class="avatar-3d-placeholder">
                                        <img src="/assets/icons/user.svg" alt="Avatar" class="avatar-placeholder-icon" />
                                        <p class="avatar-placeholder-text">"3D Preview"</p>
                                    </div>
                                    <div class="avatar-rotate-hint">"Drag to rotate"</div>
                                </div>
                            </div>

                            <div class="avatar-controls-section">
                                <h3 class="avatar-section-title">"Customize Character"</h3>

                                // Body
                                <div class="avatar-category">
                                    <h4 class="avatar-category-title">"Body"</h4>
                                    <div class="avatar-options">
                                        <div class="avatar-option">
                                            <label>"Height"</label>
                                            <input type="range" min="0" max="100" value="50" class="avatar-slider" />
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Build"</label>
                                            <input type="range" min="0" max="100" value="50" class="avatar-slider" />
                                        </div>
                                    </div>
                                </div>

                                // Head
                                <div class="avatar-category">
                                    <h4 class="avatar-category-title">"Head"</h4>
                                    <div class="avatar-options">
                                        <div class="avatar-option">
                                            <label>"Face Shape"</label>
                                            <select class="form-input avatar-select">
                                                <option>"Round"</option>
                                                <option>"Square"</option>
                                                <option>"Oval"</option>
                                                <option>"Diamond"</option>
                                            </select>
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Skin Tone"</label>
                                            <div class="color-swatches">
                                                <button class="swatch" style="background: #f5d0a9"></button>
                                                <button class="swatch" style="background: #d4a574"></button>
                                                <button class="swatch" style="background: #a0754a"></button>
                                                <button class="swatch" style="background: #6b4226"></button>
                                                <button class="swatch" style="background: #3d2314"></button>
                                                <button class="swatch" style="background: #f0c8c8"></button>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                // Hair
                                <div class="avatar-category">
                                    <h4 class="avatar-category-title">"Hair"</h4>
                                    <div class="avatar-options">
                                        <div class="avatar-option">
                                            <label>"Style"</label>
                                            <select class="form-input avatar-select">
                                                <option>"Short"</option>
                                                <option>"Medium"</option>
                                                <option>"Long"</option>
                                                <option>"Buzz"</option>
                                                <option>"Mohawk"</option>
                                                <option>"Ponytail"</option>
                                                <option>"Bald"</option>
                                            </select>
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Color"</label>
                                            <div class="color-swatches">
                                                <button class="swatch" style="background: #1a1a1a"></button>
                                                <button class="swatch" style="background: #3b2a1a"></button>
                                                <button class="swatch" style="background: #5a3825"></button>
                                                <button class="swatch" style="background: #8b6914"></button>
                                                <button class="swatch" style="background: #c4892a"></button>
                                                <button class="swatch" style="background: #d4a04a"></button>
                                                <button class="swatch" style="background: #c4500f"></button>
                                                <button class="swatch" style="background: #a0a0a0"></button>
                                                <button class="swatch" style="background: #e0e0e0"></button>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                // Clothing
                                <div class="avatar-category">
                                    <h4 class="avatar-category-title">"Clothing"</h4>
                                    <div class="avatar-options">
                                        <div class="avatar-option">
                                            <label>"Top"</label>
                                            <select class="form-input avatar-select">
                                                <option>"T-Shirt"</option>
                                                <option>"Hoodie"</option>
                                                <option>"Jacket"</option>
                                                <option>"Tank Top"</option>
                                                <option>"Suit"</option>
                                            </select>
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Bottom"</label>
                                            <select class="form-input avatar-select">
                                                <option>"Jeans"</option>
                                                <option>"Shorts"</option>
                                                <option>"Pants"</option>
                                                <option>"Skirt"</option>
                                                <option>"Suit Pants"</option>
                                            </select>
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Shirt Color"</label>
                                            <div class="color-swatches">
                                                <button class="swatch" style="background: #ffffff"></button>
                                                <button class="swatch" style="background: #1a1a1a"></button>
                                                <button class="swatch" style="background: #3366ff"></button>
                                                <button class="swatch" style="background: #ff3333"></button>
                                                <button class="swatch" style="background: #33cc33"></button>
                                                <button class="swatch" style="background: #ffcc00"></button>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                // Accessories
                                <div class="avatar-category">
                                    <h4 class="avatar-category-title">"Accessories"</h4>
                                    <div class="avatar-options">
                                        <div class="avatar-option">
                                            <label>"Hat"</label>
                                            <select class="form-input avatar-select">
                                                <option>"None"</option>
                                                <option>"Cap"</option>
                                                <option>"Beanie"</option>
                                                <option>"Top Hat"</option>
                                                <option>"Headband"</option>
                                            </select>
                                        </div>
                                        <div class="avatar-option">
                                            <label>"Glasses"</label>
                                            <select class="form-input avatar-select">
                                                <option>"None"</option>
                                                <option>"Round"</option>
                                                <option>"Square"</option>
                                                <option>"Aviator"</option>
                                                <option>"Sunglasses"</option>
                                            </select>
                                        </div>
                                    </div>
                                </div>

                                <button class="btn btn-primary avatar-save-btn">
                                    "Save Avatar"
                                </button>
                            </div>
                        </div>
                    </Show>

                    <Show when=move || active_tab.get() == "simulations">
                        <div class="experiences-grid">
                            <For
                                each=move || experiences.get()
                                key=|exp| exp.id.clone()
                                children=move |exp| view! {
                                    <ProfileExperienceCard experience=exp />
                                }
                            />
                        </div>
                    </Show>
                    
                    <Show when=move || active_tab.get() == "favorites" && is_own_profile()>
                        <div class="empty-state">
                            <img src="/assets/icons/heart.svg" alt="No favorites" class="empty-icon" />
                            <h3>"No favorites yet"</h3>
                            <p>"Experiences you favorite will appear here"</p>
                        </div>
                    </Show>
                    
                    <Show when=move || active_tab.get() == "friends" && is_own_profile()>
                        <div class="friends-grid">
                            <FriendCard name="GamerPro" status="online" />
                            <FriendCard name="PixelArtist" status="in-game" />
                            <FriendCard name="BuilderBob" status="offline" />
                            <FriendCard name="SpeedRunner" status="online" />
                        </div>
                    </Show>
                    
                    <Show when=move || active_tab.get() == "inventory" && is_own_profile()>
                        <div class="inventory-grid">
                            <InventoryItem name="Golden Sword" rarity="legendary" />
                            <InventoryItem name="Space Helmet" rarity="rare" />
                            <InventoryItem name="Neon Wings" rarity="epic" />
                            <InventoryItem name="Basic Hat" rarity="common" />
                        </div>
                    </Show>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}

/// Experience card for profile.
#[component]
fn ProfileExperienceCard(experience: UserExperience) -> impl IntoView {
    let exp_url = format!("/simulation/{}", experience.id);
    
    view! {
        <a href=exp_url class="profile-experience-card">
            <div class="experience-thumbnail">
                <img src="/assets/icons/gamepad.svg" alt="Experience" class="thumbnail-icon" />
            </div>
            <div class="experience-info">
                <h3 class="experience-name">{experience.name}</h3>
                <span class="play-count">
                    <img src="/assets/icons/play.svg" alt="Plays" />
                    {format_number(experience.play_count)}
                </span>
            </div>
        </a>
    }
}

/// Friend card component.
#[component]
fn FriendCard(name: &'static str, status: &'static str) -> impl IntoView {
    let status_class = match status {
        "online" => "status-online",
        "in-game" => "status-ingame",
        _ => "status-offline",
    };
    
    let status_text = match status {
        "online" => "Online",
        "in-game" => "In Game",
        _ => "Offline",
    };
    
    view! {
        <a href=format!("/profile/{}", name) class="friend-card">
            <div class="friend-avatar">
                <img src="/assets/icons/noob-head.svg" alt="Avatar" />
                <span class=format!("status-dot {}", status_class)></span>
            </div>
            <div class="friend-info">
                <span class="friend-name">{name}</span>
                <span class=format!("friend-status {}", status_class)>{status_text}</span>
            </div>
        </a>
    }
}

/// Inventory item component.
#[component]
fn InventoryItem(name: &'static str, rarity: &'static str) -> impl IntoView {
    let rarity_class = format!("rarity-{}", rarity);
    
    view! {
        <div class=format!("inventory-item {}", rarity_class)>
            <div class="item-icon">
                <img src="/assets/icons/cube.svg" alt="Item" />
            </div>
            <div class="item-info">
                <span class="item-name">{name}</span>
                <span class="item-rarity">{rarity}</span>
            </div>
        </div>
    }
}

/// Format large numbers.
fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
