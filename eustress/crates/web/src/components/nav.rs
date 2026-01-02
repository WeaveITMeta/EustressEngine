// =============================================================================
// Eustress Web - Central Navigation Component
// =============================================================================
// Reusable navigation bar for all pages
// Shows user info and Bliss balance when logged in
// Mobile-responsive with hamburger menu and slide-out drawer
// =============================================================================

use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use crate::state::{AppState, AuthState};

/// Central navigation bar component.
/// 
/// # Arguments
/// * `active` - The currently active page (e.g., "home", "gallery", "create")
#[component]
pub fn CentralNav(
    #[prop(default = "home".to_string())]
    active: String,
) -> impl IntoView {
    // Get app state from context
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    
    // Mobile menu state
    let menu_open = RwSignal::new(false);
    
    // Close menu when clicking a link
    let close_menu = move || menu_open.set(false);
    
    // Clone active for each closure
    let active_desktop = active.clone();
    let active_mobile = active.clone();
    
    let is_active = move |page: &str| {
        if page == active_desktop { "central-nav-link active" } else { "central-nav-link" }
    };
    
    // Mobile link class includes close handler
    let mobile_link_class = move |page: &str| {
        if page == active_mobile { "mobile-nav-link active" } else { "mobile-nav-link" }
    };
    
    let home_class = is_active("home");
    let gallery_class = is_active("gallery");
    let projects_class = is_active("projects");
    let marketplace_class = is_active("marketplace");
    let learn_class = is_active("learn");
    let community_class = is_active("community");
    
    // Mobile classes
    let mobile_home_class = mobile_link_class("home");
    let mobile_gallery_class = mobile_link_class("gallery");
    let mobile_projects_class = mobile_link_class("projects");
    let mobile_marketplace_class = mobile_link_class("marketplace");
    let mobile_learn_class = mobile_link_class("learn");
    let mobile_community_class = mobile_link_class("community");
    
    // Determine home link based on auth state
    let home_url = move || {
        match auth.get() {
            AuthState::Authenticated(_) => "/dashboard",
            _ => "/",
        }
    };
    
    view! {
        <nav class="central-nav">
            // Logo (always visible)
            <a href=home_url class="nav-logo">
                <img src="/assets/icons/eustress-gear.svg" alt="" class="nav-logo-gear" />
                <img src="/assets/logo.svg" alt="Eustress" class="nav-logo-svg" />
            </a>
            
            // Desktop nav links (hidden on mobile)
            <div class="nav-links desktop-only">
                <a href=home_url class=home_class>"Home"</a>
                <a href="/gallery" class=gallery_class>"Gallery"</a>
                <a href="/community" class=community_class>"Community"</a>
                <a href="/marketplace" class=marketplace_class>"Marketplace"</a>
                <a href="/projects" class=projects_class>"Projects"</a>
                <a href="/learn" class=learn_class>"Learn"</a>
            </div>
            
            // Desktop right side - user info or sign in (hidden on mobile)
            <div class="nav-right desktop-only">
                {move || {
                    let navigate = use_navigate();
                    let app_state = expect_context::<AppState>();
                    
                    match auth.get() {
                        AuthState::Authenticated(user) => {
                            let profile_url = format!("/profile/{}", user.username);
                            let bliss_display = format_bliss(user.bliss_balance);
                            let username = user.username.clone();
                            let avatar_url = user.avatar_url.clone();
                            
                            view! {
                                <div class="nav-user-section">
                                    // Bliss Balance
                                    <a href="/bliss" class="nav-bliss">
                                        <img src="/assets/icons/bliss.svg" alt="Bliss" class="bliss-icon" />
                                        <span class="bliss-amount">{bliss_display}</span>
                                    </a>
                                    
                                    // User Menu with Dropdown
                                    <div class="nav-user-dropdown">
                                        <button class="nav-user-trigger">
                                            <div class="nav-avatar">
                                                {match avatar_url {
                                                    Some(url) => view! { <img src=url alt="Avatar" /> }.into_any(),
                                                    None => view! { <img src="/assets/icons/noob-head.svg" alt="Avatar" /> }.into_any(),
                                                }}
                                            </div>
                                            <span class="nav-username">{username.clone()}</span>
                                            <img src="/assets/icons/chevron-down.svg" alt="Menu" class="dropdown-chevron" />
                                        </button>
                                        
                                        <div class="nav-dropdown-menu">
                                            <a href=profile_url class="dropdown-item">
                                                <img src="/assets/icons/user.svg" alt="Profile" />
                                                "Profile"
                                            </a>
                                            <a href="/settings" class="dropdown-item">
                                                <img src="/assets/icons/settings.svg" alt="Settings" />
                                                "Settings"
                                            </a>
                                            <div class="dropdown-divider"></div>
                                            <button 
                                                class="dropdown-item logout"
                                                on:click=move |_| {
                                                    app_state.logout();
                                                    navigate("/", Default::default());
                                                }
                                            >
                                                <img src="/assets/icons/logout.svg" alt="Logout" />
                                                "Sign Out"
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        }
                        _ => {
                            view! {
                                <a href="/login" class="central-nav-link nav-cta">"Sign In"</a>
                            }.into_any()
                        }
                    }
                }}
            </div>
            
            // Hamburger button (mobile only)
            <button 
                class="hamburger-btn mobile-only"
                aria-label="Toggle menu"
                aria-expanded=move || menu_open.get().to_string()
                on:click=move |_| menu_open.update(|v| *v = !*v)
            >
                <span class=move || if menu_open.get() { "hamburger-line open line-1" } else { "hamburger-line line-1" }></span>
                <span class=move || if menu_open.get() { "hamburger-line open line-2" } else { "hamburger-line line-2" }></span>
                <span class=move || if menu_open.get() { "hamburger-line open line-3" } else { "hamburger-line line-3" }></span>
            </button>
            
            // Mobile backdrop (closes menu on tap)
            <div 
                class=move || if menu_open.get() { "mobile-backdrop visible" } else { "mobile-backdrop" }
                on:click=move |_| menu_open.set(false)
            ></div>
            
            // Mobile drawer
            <div class=move || if menu_open.get() { "mobile-drawer open" } else { "mobile-drawer" }>
                // Drawer header
                <div class="drawer-header">
                    <a href=home_url class="drawer-logo" on:click=move |_| close_menu()>
                        <img src="/assets/logo.svg" alt="Eustress" />
                    </a>
                    <button 
                        class="drawer-close"
                        aria-label="Close menu"
                        on:click=move |_| menu_open.set(false)
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M18 6L6 18"></path>
                            <path d="M6 6l12 12"></path>
                        </svg>
                    </button>
                </div>
                
                // Mobile nav links
                <nav class="drawer-nav">
                    <a href=home_url class=mobile_home_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path>
                            <polyline points="9 22 9 12 15 12 15 22"></polyline>
                        </svg>
                        "Home"
                    </a>
                    <a href="/gallery" class=mobile_gallery_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <rect width="18" height="18" x="3" y="3" rx="2" ry="2"></rect>
                            <circle cx="9" cy="9" r="2"></circle>
                            <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"></path>
                        </svg>
                        "Gallery"
                    </a>
                    <a href="/community" class=mobile_community_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"></path>
                            <circle cx="9" cy="7" r="4"></circle>
                            <path d="M22 21v-2a4 4 0 0 0-3-3.87"></path>
                            <path d="M16 3.13a4 4 0 0 1 0 7.75"></path>
                        </svg>
                        "Community"
                    </a>
                    <a href="/marketplace" class=mobile_marketplace_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <circle cx="8" cy="21" r="1"></circle>
                            <circle cx="19" cy="21" r="1"></circle>
                            <path d="M2.05 2.05h2l2.66 12.42a2 2 0 0 0 2 1.58h9.78a2 2 0 0 0 1.95-1.57l1.65-7.43H5.12"></path>
                        </svg>
                        "Marketplace"
                    </a>
                    <a href="/projects" class=mobile_projects_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
                        </svg>
                        "Projects"
                    </a>
                    <a href="/learn" class=mobile_learn_class on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"></path>
                        </svg>
                        "Learn"
                    </a>
                </nav>
                
                // Drawer divider
                <div class="drawer-divider"></div>
                
                // Mobile user section
                <div class="drawer-user-section">
                    {move || {
                        let navigate = use_navigate();
                        let app_state = expect_context::<AppState>();
                        
                        match auth.get() {
                            AuthState::Authenticated(user) => {
                                let profile_url = format!("/profile/{}", user.username);
                                let bliss_display = format_bliss(user.bliss_balance);
                                let username = user.username.clone();
                                let avatar_url = user.avatar_url.clone();
                                
                                view! {
                                    <div class="drawer-user-info">
                                        <div class="drawer-avatar">
                                            {match avatar_url {
                                                Some(url) => view! { <img src=url alt="Avatar" /> }.into_any(),
                                                None => view! { <img src="/assets/icons/noob-head.svg" alt="Avatar" /> }.into_any(),
                                            }}
                                        </div>
                                        <div class="drawer-user-details">
                                            <span class="drawer-username">{username}</span>
                                            <span class="drawer-bliss">
                                                <img src="/assets/icons/bliss.svg" alt="Bliss" />
                                                {bliss_display}" Bliss"
                                            </span>
                                        </div>
                                    </div>
                                    
                                    <nav class="drawer-user-nav">
                                        <a href=profile_url class="drawer-user-link" on:click=move |_| close_menu()>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"></path>
                                                <circle cx="12" cy="7" r="4"></circle>
                                            </svg>
                                            "Profile"
                                        </a>
                                        <a href="/friends" class="drawer-user-link" on:click=move |_| close_menu()>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"></path>
                                                <circle cx="9" cy="7" r="4"></circle>
                                                <line x1="19" x2="19" y1="8" y2="14"></line>
                                                <line x1="22" x2="16" y1="11" y2="11"></line>
                                            </svg>
                                            "Friends"
                                        </a>
                                        <a href="/settings" class="drawer-user-link" on:click=move |_| close_menu()>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"></path>
                                                <circle cx="12" cy="12" r="3"></circle>
                                            </svg>
                                            "Settings"
                                        </a>
                                        <button 
                                            class="drawer-user-link logout"
                                            on:click=move |_| {
                                                app_state.logout();
                                                close_menu();
                                                navigate("/", Default::default());
                                            }
                                        >
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"></path>
                                                <polyline points="16 17 21 12 16 7"></polyline>
                                                <line x1="21" x2="9" y1="12" y2="12"></line>
                                            </svg>
                                            "Sign Out"
                                        </button>
                                    </nav>
                                }.into_any()
                            }
                            _ => {
                                view! {
                                    <div class="drawer-auth-buttons">
                                        <a href="/login" class="drawer-btn primary" on:click=move |_| close_menu()>
                                            "Sign In"
                                        </a>
                                        <a href="/register" class="drawer-btn secondary" on:click=move |_| close_menu()>
                                            "Create Account"
                                        </a>
                                    </div>
                                }.into_any()
                            }
                        }
                    }}
                </div>
                
                // Drawer footer
                <div class="drawer-footer">
                    <a href="/download" class="drawer-download-btn" on:click=move |_| close_menu()>
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                            <polyline points="7 10 12 15 17 10"></polyline>
                            <line x1="12" x2="12" y1="15" y2="3"></line>
                        </svg>
                        "Download Studio"
                    </a>
                </div>
            </div>
        </nav>
    }
}

/// Format Bliss balance for display.
fn format_bliss(amount: u64) -> String {
    if amount >= 1_000_000 {
        format!("{:.1}M", amount as f64 / 1_000_000.0)
    } else if amount >= 1_000 {
        format!("{:.1}K", amount as f64 / 1_000.0)
    } else {
        amount.to_string()
    }
}
