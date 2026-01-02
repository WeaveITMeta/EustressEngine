// =============================================================================
// Eustress Web - Profile Page (Industrial Design)
// =============================================================================
// Social profile with avatar display and user content
// =============================================================================

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use crate::components::{CentralNav, Footer};
use crate::state::{AppState, AuthState};

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
#[derive(Clone, Debug, PartialEq)]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
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
    
    // Get username from URL or use current user
    let _username = move || {
        params.get().get("username").unwrap_or_else(|| {
            match app_state.auth.get() {
                AuthState::Authenticated(user) => user.username,
                _ => "guest".to_string(),
            }
        })
    };
    // TODO: Use _username to fetch profile from API
    
    // Sample profile data (would come from API)
    let profile = RwSignal::new(UserProfile {
        username: "PlayerOne".to_string(),
        display_name: "Player One".to_string(),
        bio: "Game developer and explorer. Creating awesome experiences since 2023. 🎮✨".to_string(),
        avatar_url: None,
        banner_url: None,
        join_date: "January 2023".to_string(),
        follower_count: 12500,
        following_count: 342,
        experience_count: 8,
        total_plays: 450000,
        badges: vec![
            Badge {
                id: "creator".to_string(),
                name: "Creator".to_string(),
                icon: "🎨".to_string(),
                description: "Published 5+ experiences".to_string(),
            },
            Badge {
                id: "popular".to_string(),
                name: "Popular".to_string(),
                icon: "⭐".to_string(),
                description: "100K+ total plays".to_string(),
            },
            Badge {
                id: "veteran".to_string(),
                name: "Veteran".to_string(),
                icon: "🏆".to_string(),
                description: "Member for 1+ year".to_string(),
            },
            Badge {
                id: "social".to_string(),
                name: "Social Butterfly".to_string(),
                icon: "🦋".to_string(),
                description: "1K+ followers".to_string(),
            },
        ],
        is_verified: true,
        is_following: false,
        discord_linked: true,
    });
    
    // User's experiences
    let experiences = RwSignal::new(vec![
        UserExperience {
            id: "1".to_string(),
            name: "Adventure World".to_string(),
            thumbnail_url: None,
            play_count: 125000,
        },
        UserExperience {
            id: "2".to_string(),
            name: "Puzzle Paradise".to_string(),
            thumbnail_url: None,
            play_count: 89000,
        },
        UserExperience {
            id: "3".to_string(),
            name: "Racing Rivals".to_string(),
            thumbnail_url: None,
            play_count: 156000,
        },
        UserExperience {
            id: "4".to_string(),
            name: "Space Station".to_string(),
            thumbnail_url: None,
            play_count: 80000,
        },
    ]);
    
    // Tab state
    let active_tab = RwSignal::new("experiences".to_string());
    
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
                                    {move || profile.get().display_name}
                                    {move || profile.get().is_verified.then(|| view! {
                                        <span class="verified-badge" title="Verified Creator">
                                            <img src="/assets/icons/check.svg" alt="Verified" />
                                        </span>
                                    })}
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
                            <span class="stat-label">"Experiences"</span>
                        </div>
                        <div class="stat-card">
                            <span class="stat-value">{move || format_number(profile.get().total_plays)}</span>
                            <span class="stat-label">"Total Plays"</span>
                        </div>
                    </div>
                    
                    <div class="profile-actions">
                        <button 
                            class="btn-follow"
                            class:following=move || is_following.get()
                            on:click=move |_| is_following.update(|f| *f = !*f)
                        >
                            <img src="/assets/icons/user.svg" alt="Follow" />
                            {move || if is_following.get() { "Following" } else { "Follow" }}
                        </button>
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
                
                // Content tabs
                <div class="profile-tabs">
                    <button 
                        class="profile-tab"
                        class:active=move || active_tab.get() == "experiences"
                        on:click=move |_| active_tab.set("experiences".to_string())
                    >
                        <img src="/assets/icons/gamepad.svg" alt="Experiences" />
                        "Experiences"
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
                </div>
                
                // Tab content
                <div class="profile-tab-content">
                    <Show when=move || active_tab.get() == "experiences">
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
                    
                    <Show when=move || active_tab.get() == "favorites">
                        <div class="empty-state">
                            <img src="/assets/icons/heart.svg" alt="No favorites" class="empty-icon" />
                            <h3>"No favorites yet"</h3>
                            <p>"Experiences you favorite will appear here"</p>
                        </div>
                    </Show>
                    
                    <Show when=move || active_tab.get() == "friends">
                        <div class="friends-grid">
                            <FriendCard name="GamerPro" status="online" />
                            <FriendCard name="PixelArtist" status="in-game" />
                            <FriendCard name="BuilderBob" status="offline" />
                            <FriendCard name="SpeedRunner" status="online" />
                        </div>
                    </Show>
                    
                    <Show when=move || active_tab.get() == "inventory">
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
    let exp_url = format!("/experience/{}", experience.id);
    
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
