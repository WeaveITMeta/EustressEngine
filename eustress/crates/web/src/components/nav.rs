// =============================================================================
// Eustress Web - Central Navigation Component
// =============================================================================
// Reusable navigation bar for all pages
// Shows user info and Bliss balance when logged in
// Mobile-responsive with hamburger menu and slide-out drawer
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use crate::components::sign_out::{use_sign_out, EVERYWHERE_HELP, THIS_DEVICE_HELP};
use crate::api::{get_delta_balance, ApiClient};
use crate::state::{AppState, AuthState, DeltaState};
use crate::components::action_menu::move_focus;
use crate::utils::{after_event, format_bliss_exact, format_count};

/// The two downloads, Studio first: (name, page, what it is for). The top bar's Downloads menu lists them, and so
/// does the mobile drawer.
pub(crate) const DOWNLOADS: [(&str, &str, &str); 2] = [
    ("Eustress Studio", "/download", "Build and publish"),
    ("Eustress Player", "/downloads/player", "Play and join"),
];

/// What a key does to the Downloads menu. `open` says whether the menu is open, `focused` which item has the focus
/// (none while the button has it), `len` how many items there are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MenuKey {
    /// Open the menu and focus this item.
    Open(usize),
    /// Move the focus to this item.
    Focus(usize),
    /// Close the menu; `true` puts the focus back on the button.
    Close(bool),
    /// Not a key the menu answers.
    Ignore,
}

/// Enter and Space are the button's own click (it opens the menu with the first item focused), so they are not here.
/// Down or Up on the closed button opens the menu at the first or the last item; once open, Down, Up, Home and End
/// move and wrap, Escape closes and returns the focus to the button, and Tab closes and lets the focus go on.
pub(crate) fn menu_key(open: bool, focused: Option<usize>, key: &str, len: usize) -> MenuKey {
    match (open, key) {
        (false, "ArrowDown") if len > 0 => MenuKey::Open(0),
        (false, "ArrowUp") if len > 0 => MenuKey::Open(len - 1),
        (false, _) => MenuKey::Ignore,
        (true, "Escape") => MenuKey::Close(true),
        (true, "Tab") => MenuKey::Close(false),
        (true, _) => move_focus(focused, len, key).map_or(MenuKey::Ignore, MenuKey::Focus),
    }
}

/// The top bar's one Downloads button: a download arrow that opens a small menu, Eustress Studio and Eustress Player.
/// It is in the bar for signed-in and signed-out visitors, before the currency pills; on a phone the drawer lists both.
/// A click outside the menu closes it.
#[component]
fn NavDownloads() -> impl IntoView {
    let open = RwSignal::new(false);
    let root = NodeRef::<leptos::html::Div>::new();
    let button = NodeRef::<leptos::html::Button>::new();
    let items: [NodeRef<leptos::html::A>; 2] = [NodeRef::new(), NodeRef::new()];

    let focus_item = move |i: usize| {
        if let Some(item) = items.get(i).and_then(|n| n.get_untracked()) {
            let _ = item.focus();
        }
    };
    let open_at = move |i: usize| {
        open.set(true);
        // The menu is visible once this event has finished; the item takes the focus then.
        after_event(move || focus_item(i));
    };
    let close = move |return_focus: bool| {
        open.set(false);
        if return_focus {
            if let Some(b) = button.get_untracked() {
                let _ = b.focus();
            }
        }
    };
    // Which item has the focus, if one does.
    let focused = move || -> Option<usize> {
        let active = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element())?;
        items.iter().position(|n| {
            n.get_untracked().is_some_and(|item| {
                let el: &web_sys::Element = item.as_ref();
                *el == active
            })
        })
    };
    let on_key = move |ev: leptos::ev::KeyboardEvent| match menu_key(open.get_untracked(), focused(), &ev.key(), DOWNLOADS.len()) {
        MenuKey::Open(i) => {
            ev.prevent_default();
            open_at(i);
        }
        MenuKey::Focus(i) => {
            ev.prevent_default();
            focus_item(i);
        }
        MenuKey::Close(return_focus) => {
            if return_focus {
                ev.prevent_default();
            }
            close(return_focus);
        }
        MenuKey::Ignore => {}
    };

    #[cfg(not(feature = "ssr"))]
    {
        use wasm_bindgen::JsCast;
        let handle = window_event_listener(leptos::ev::mousedown, move |ev: leptos::ev::MouseEvent| {
            if !open.get_untracked() {
                return;
            }
            let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok());
            let inside = match (root.get_untracked(), target) {
                (Some(root), Some(node)) => root.contains(Some(&node)),
                _ => false,
            };
            if !inside {
                open.set(false);
            }
        });
        on_cleanup(move || handle.remove());
    }

    view! {
        <div class="nav-dl" node_ref=root on:keydown=on_key>
            <button
                type="button"
                class="nav-dl-btn"
                node_ref=button
                title="Downloads"
                aria-label="Downloads"
                aria-haspopup="menu"
                aria-controls="nav-dl-menu"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| if open.get_untracked() { close(false) } else { open_at(0) }
            >
                <svg class="nav-dl-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                    <polyline points="7 10 12 15 17 10"></polyline>
                    <line x1="12" x2="12" y1="15" y2="3"></line>
                </svg>
            </button>
            <div
                id="nav-dl-menu"
                class=move || if open.get() { "nav-dropdown-menu nav-dl-menu is-open" } else { "nav-dropdown-menu nav-dl-menu" }
                role="menu"
                aria-label="Downloads"
            >
                {DOWNLOADS.into_iter().zip(items).map(|((name, href, hint), item)| view! {
                    <a href=href class="dropdown-item nav-dl-item" role="menuitem" tabindex="-1" node_ref=item on:click=move |_| open.set(false)>
                        <span class="nav-dl-name">{name}</span>
                        <span class="nav-dl-hint">{hint}</span>
                    </a>
                }).collect_view()}
            </div>
        </div>
    }
}

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
    let delta = app_state.delta;

    // The Delta balance for the top bar, asked for once per sign-in by whichever
    // page's nav mounts first. No Delta route yet (a 404) or a failed request
    // leaves no chip.
    {
        let api_url = app_state.api_url.clone();
        Effect::new(move |_| match auth.get() {
            AuthState::Authenticated(_) => {
                if delta.get_untracked() != DeltaState::Unknown {
                    return;
                }
                delta.set(DeltaState::Loading);
                let api_url = api_url.clone();
                spawn_local(async move {
                    let client = ApiClient::new(&api_url);
                    delta.set(match get_delta_balance(&client).await {
                        Ok(Some(balance)) => DeltaState::Balance(balance),
                        _ => DeltaState::Absent,
                    });
                });
            }
            _ => delta.set(DeltaState::Unknown),
        });
    }
    
    // Mobile menu state
    let menu_open = RwSignal::new(false);
    
    // Close menu when clicking a link
    let close_menu = move || menu_open.set(false);
    // The drawer takes the focus when it opens (its close button, at the top left where the menu button was) and gives
    // it back to the menu button when it is closed from inside (the close button, the backdrop, Escape). A link in it
    // closes it and goes to its page.
    let hamburger = NodeRef::<leptos::html::Button>::new();
    let drawer_close = NodeRef::<leptos::html::Button>::new();
    let open_drawer = move || {
        menu_open.set(true);
        after_event(move || {
            if let Some(b) = drawer_close.get_untracked() {
                let _ = b.focus();
            }
        });
    };
    let dismiss_drawer = move || {
        menu_open.set(false);
        after_event(move || {
            if let Some(b) = hamburger.get_untracked() {
                let _ = b.focus();
            }
        });
    };
    // Sign out has two buttons (components/sign_out.rs): this device, and everywhere.
    let go_home = StoredValue::new_local(use_navigate());
    let sign_out_desktop = use_sign_out(Callback::new(move |_| go_home.with_value(|go| go("/", Default::default()))));
    let sign_out_mobile = use_sign_out(Callback::new(move |_| {
        close_menu();
        go_home.with_value(|go| go("/", Default::default()));
    }));
    
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
        // Signed in, the right side holds the pills and the account button; style/topbar.css keeps room for them.
        <nav class=move || if matches!(auth.get(), AuthState::Authenticated(_)) { "central-nav signed-in" } else { "central-nav" }>
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
                <NavDownloads />
                {move || {
                    
                    match auth.get() {
                        AuthState::Authenticated(user) => {
                            let profile_url = format!("/profile/{}", user.username);
                            let username = user.username.clone();
                            let avatar_url = user.avatar_url.clone();

                            view! {
                                <div class="nav-user-section">
                                    // Tickets, Bliss and Delta: each pill is as wide as its amount, and its title has the exact amount.
                                    {tickets_pill(user.ticket_balance)}
                                    // Bliss: the chip still navigates to the overview; hover/focus reveals the menu.
                                    {bliss_pill(user.bliss_balance)}
                                    // Delta, to the right of Bliss: a usage currency for premium
                                    // Studio tools. No chip until the Worker has answered.
                                    {move || match delta.get() {
                                        DeltaState::Balance(balance) => Some(delta_pill(balance.whole())),
                                        _ => None,
                                    }}

                                    // User Menu with Dropdown
                                    <div class="nav-user-dropdown">
                                        <button class="nav-user-trigger">
                                            <div class="nav-avatar">
                                                {match avatar_url {
                                                    Some(url) => view! { <img src=url alt="Avatar" /> }.into_any(),
                                                    None => view! { <img src="/assets/icons/noob-head.svg" alt="Avatar" /> }.into_any(),
                                                }}
                                            </div>
                                            <span class="nav-username" title=username.clone()>{username.clone()}</span>
                                            <img src="/assets/icons/chevron-down.svg" alt="Menu" class="dropdown-chevron" />
                                        </button>
                                        
                                        <div class="nav-dropdown-menu">
                                            <a href=profile_url class="dropdown-item">
                                                <img src="/assets/icons/user.svg" alt="Profile" />
                                                "Profile"
                                            </a>
                                            <a href="/purchases" class="dropdown-item">
                                                <img src="/assets/icons/receipt.svg" alt="Purchases" />
                                                "Purchases"
                                            </a>
                                            <a href="/account/cases" class="dropdown-item">
                                                <img src="/assets/icons/folder.svg" alt="" />
                                                "Cases"
                                            </a>
                                            <a href="/settings" class="dropdown-item">
                                                <img src="/assets/icons/settings.svg" alt="Settings" />
                                                "Settings"
                                            </a>
                                            <div class="dropdown-divider"></div>
                                            <button class="dropdown-item logout" title=THIS_DEVICE_HELP on:click=move |_| sign_out_desktop.this_device.run(())>
                                                <img src="/assets/icons/logout.svg" alt="" />
                                                "Sign out of this device"
                                            </button>
                                            <div class="sign-out-divider" role="separator"></div>
                                            <button class="dropdown-item logout" title=EVERYWHERE_HELP disabled=move || sign_out_desktop.busy.get() on:click=move |_| sign_out_desktop.everywhere.run(())>
                                                <img src="/assets/icons/logout.svg" alt="" />
                                                "Sign out everywhere"
                                            </button>
                                            {move || sign_out_desktop.error.get().map(|e| view! { <p class="sign-out-error" role="alert">{e}</p> })}
                                        </div>
                                    </div>
                                </div>
                            }.into_any()
                        }
                        _ => {
                            view! {
                                <div class="nav-user-section">
                                    <a href="/bliss" class="nav-bliss">
                                        <img src="/assets/icons/bliss.svg" alt="Bliss" class="bliss-icon" />
                                        <span class="bliss-label">"Bliss"</span>
                                    </a>
                                    <a href="/login" class="central-nav-link nav-cta">"Sign In"</a>
                                </div>
                            }.into_any()
                        }
                    }
                }}
            </div>
            
            // Hamburger button (mobile only)
            <button
                class=move || if menu_open.get() { "hamburger-btn mobile-only is-open" } else { "hamburger-btn mobile-only" }
                node_ref=hamburger
                aria-label="Toggle menu"
                aria-controls="mobile-drawer"
                aria-expanded=move || menu_open.get().to_string()
                on:click=move |_| if menu_open.get_untracked() { dismiss_drawer() } else { open_drawer() }
            >
                <span class=move || if menu_open.get() { "hamburger-line open line-1" } else { "hamburger-line line-1" }></span>
                <span class=move || if menu_open.get() { "hamburger-line open line-2" } else { "hamburger-line line-2" }></span>
                <span class=move || if menu_open.get() { "hamburger-line open line-3" } else { "hamburger-line line-3" }></span>
            </button>
            
            // Mobile backdrop (closes menu on tap)
            <div 
                class=move || if menu_open.get() { "mobile-backdrop visible" } else { "mobile-backdrop" }
                on:click=move |_| dismiss_drawer()
            ></div>
            
            // Mobile drawer: it slides in from the left, under the menu button (style/topbar.css). Escape closes it.
            <div
                id="mobile-drawer"
                class=move || if menu_open.get() { "mobile-drawer open" } else { "mobile-drawer" }
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Escape" && menu_open.get_untracked() {
                        ev.prevent_default();
                        dismiss_drawer();
                    }
                }
            >
                // Drawer header: the close button first, where the menu button was, then the logo
                <div class="drawer-header">
                    <button
                        class="drawer-close"
                        node_ref=drawer_close
                        aria-label="Close menu"
                        on:click=move |_| dismiss_drawer()
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M18 6L6 18"></path>
                            <path d="M6 6l12 12"></path>
                        </svg>
                    </button>
                    <a href=home_url class="drawer-logo" on:click=move |_| close_menu()>
                        <img src="/assets/logo.svg" alt="Eustress" />
                    </a>
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
                        
                        match auth.get() {
                            AuthState::Authenticated(user) => {
                                let profile_url = format!("/profile/{}", user.username);
                                let bliss_display = format_short(user.bliss_balance);
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
                                            {move || match delta.get() {
                                                DeltaState::Balance(balance) => Some(view! {
                                                    <span class="drawer-bliss drawer-delta">
                                                        <img src="/assets/icons/delta.svg" alt="Delta" />
                                                        {format_count(balance.whole())}" Delta"
                                                    </span>
                                                }),
                                                _ => None,
                                            }}
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
                                        <a href="/purchases" class="drawer-user-link" on:click=move |_| close_menu()>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z"></path>
                                                <path d="M8 8h8"></path>
                                                <path d="M8 12h8"></path>
                                                <path d="M8 16h5"></path>
                                            </svg>
                                            "Purchases"
                                        </a>
                                        <a href="/account/cases" class="drawer-user-link" on:click=move |_| close_menu()>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"></path>
                                            </svg>
                                            "Cases"
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
                                        <button class="drawer-user-link logout" title=THIS_DEVICE_HELP on:click=move |_| sign_out_mobile.this_device.run(())>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"></path>
                                                <polyline points="16 17 21 12 16 7"></polyline>
                                                <line x1="21" x2="9" y1="12" y2="12"></line>
                                            </svg>
                                            "Sign out of this device"
                                        </button>
                                        <div class="sign-out-divider" role="separator"></div>
                                        <button class="drawer-user-link logout" title=EVERYWHERE_HELP disabled=move || sign_out_mobile.busy.get() on:click=move |_| sign_out_mobile.everywhere.run(())>
                                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"></path>
                                                <polyline points="16 17 21 12 16 7"></polyline>
                                                <line x1="21" x2="9" y1="12" y2="12"></line>
                                            </svg>
                                            "Sign out everywhere"
                                        </button>
                                        {move || sign_out_mobile.error.get().map(|e| view! { <p class="sign-out-error" role="alert">{e}</p> })}
                                    </nav>
                                }.into_any()
                            }
                            _ => {
                                view! {
                                    <div class="drawer-auth-buttons">
                                        <a href="/login" class="drawer-btn primary" on:click=move |_| close_menu()>
                                            "Sign In"
                                        </a>
                                        <a href="/login?register=1" class="drawer-btn secondary" on:click=move |_| close_menu()>
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
                    {DOWNLOADS.into_iter().map(|(name, href, hint)| view! {
                        <a href=href class="drawer-download-btn" title=hint on:click=move |_| close_menu()>
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                                <polyline points="7 10 12 15 17 10"></polyline>
                                <line x1="12" x2="12" y1="15" y2="3"></line>
                            </svg>
                            {format!("Download {name}")}
                        </a>
                    }).collect_view()}
                </div>
            </div>
        </nav>
    }
}

/// A whole amount short enough for a pill: digits with commas up to 9,999, then K, M and B with at most one decimal
/// (12.3K, 123K, 1.2M, 12.3M). It rounds down, so a pill never shows more than the account holds; the pill's title has
/// the exact amount.
pub(crate) fn format_short_count(n: u64) -> String {
    if n < 10_000 {
        return format_count(n);
    }
    for (size, unit) in [(1_000_000_000u64, "B"), (1_000_000, "M"), (1_000, "K")] {
        if n >= size {
            let tenths = n / (size / 10);
            return if tenths >= 1_000 || tenths % 10 == 0 {
                format!("{}{unit}", format_count(tenths / 10))
            } else {
                format!("{}.{}{unit}", tenths / 10, tenths % 10)
            };
        }
    }
    format_count(n)
}

/// A Bliss balance for its pill: [`format_short_count`] of the whole Bliss, and below 1 the hundredths (a new account's
/// first fraction before the daily distribution still shows), rounded down too.
pub(crate) fn format_short(amount: f64) -> String {
    if !amount.is_finite() || amount <= 0.0 {
        return "0".to_string();
    }
    if amount < 1.0 {
        let hundredths = (amount * 100.0).floor() as u64;
        return if hundredths == 0 { "0".to_string() } else { format!("0.{hundredths:02}") };
    }
    format_short_count(amount.floor() as u64)
}

/// The Tickets pill.
fn tickets_pill(balance: u64) -> impl IntoView {
    view! {
        <a href="/tickets" class="nav-tickets" title=format!("{} Tickets", format_count(balance))>
            <img src="/assets/icons/ticket.svg" alt="Tickets" class="ticket-icon" />
            <span class="ticket-amount">{format_short_count(balance)}</span>
        </a>
    }
}

/// The Bliss pill and its menu (Overview, History).
fn bliss_pill(balance: f64) -> impl IntoView {
    view! {
        <div class="nav-bliss-dropdown">
            <a href="/bliss" class="nav-bliss" title=format!("{} Bliss", format_bliss_exact(balance))>
                <img src="/assets/icons/bliss.svg" alt="Bliss" class="bliss-icon" />
                <span class="bliss-amount">{format_short(balance)}</span>
                <img src="/assets/icons/chevron-down.svg" alt=""
                     class="dropdown-chevron bliss-chevron" />
            </a>
            <div class="nav-dropdown-menu nav-bliss-menu">
                <a href="/bliss" class="dropdown-item">
                    <img src="/assets/icons/bliss.svg" alt="" />
                    "Overview"
                </a>
                <a href="/bliss/history" class="dropdown-item">
                    <img src="/assets/icons/trending.svg" alt="" />
                    "History"
                </a>
            </div>
        </div>
    }
}

/// The Delta pill, from the whole Delta.
fn delta_pill(whole: u64) -> impl IntoView {
    view! {
        <a href="/delta" class="nav-delta" title=format!("{} Delta: usage credit for premium Studio tools", format_count(whole))>
            <img src="/assets/icons/delta.svg" alt="Delta" class="delta-icon" />
            <span class="delta-amount">{format_short_count(whole)}</span>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::tachys::view::RenderHtml;

    /// This file's code, without its tests.
    fn code() -> &'static str {
        include_str!("nav.rs").split("#[cfg(test)]").next().unwrap()
    }

    const TOPBAR_CSS: &str = include_str!("../../style/topbar.css");

    /// The declarations of the first rule whose selector list is exactly `selector`.
    fn rule(css: &'static str, selector: &str) -> &'static str {
        let at = css
            .match_indices('{')
            .find(|(i, _)| {
                let start = css[..*i].rfind(['}', '/']).map_or(0, |j| j + 1);
                css[start..*i].trim() == selector
            })
            .map(|(i, _)| i)
            .unwrap_or_else(|| panic!("no rule for {selector}"));
        &css[at + 1..at + css[at..].find('}').unwrap()]
    }

    #[test]
    fn the_top_bar_has_one_downloads_button_with_studio_then_player() {
        assert_eq!(DOWNLOADS, [
            ("Eustress Studio", "/download", "Build and publish"),
            ("Eustress Player", "/downloads/player", "Play and join"),
        ]);
        let owner = Owner::new();
        let html = owner.with(|| NavDownloads().to_html());
        assert_eq!(html.matches("<button").count(), 1, "one button: {html}");
        assert_eq!(html.matches("class=\"nav-dl-btn\"").count(), 1, "{html}");
        assert!(html.contains("aria-label=\"Downloads\""), "the button is called Downloads: {html}");
        let studio = html.find("href=\"/download\"").expect("the Studio item");
        let player = html.find("href=\"/downloads/player\"").expect("the Player item");
        assert!(studio < player, "Studio first, then Player: {html}");
        for words in ["Eustress Studio", "Build and publish", "Eustress Player", "Play and join"] {
            assert_eq!(html.matches(words).count(), 1, "{words} in {html}");
        }
        // the old pair of buttons is gone
        assert!(!code().contains("nav-download-btn") && !code().contains("DOWNLOAD_LINKS"));
    }

    #[test]
    fn the_downloads_menu_says_what_it_is_to_a_screen_reader() {
        let owner = Owner::new();
        let html = owner.with(|| NavDownloads().to_html());
        let button = &html[html.find("<button").unwrap()..html.find("</button>").unwrap()];
        for attr in ["aria-haspopup=\"menu\"", "aria-expanded=\"false\"", "aria-controls=\"nav-dl-menu\"", "type=\"button\""] {
            assert!(button.contains(attr), "{attr} on the button: {button}");
        }
        assert!(button.contains("aria-hidden=\"true\""), "the arrow is decoration: {button}");
        let menu = &html[html.find("id=\"nav-dl-menu\"").expect("the menu the button controls")..];
        assert!(menu.contains("role=\"menu\""), "{menu}");
        assert_eq!(menu.matches("role=\"menuitem\"").count(), 2, "{menu}");
        assert_eq!(menu.matches("tabindex=\"-1\"").count(), 2, "items are reached with the arrow keys: {menu}");
        assert!(!menu.contains("is-open"), "closed until asked: {menu}");
    }

    #[test]
    fn the_downloads_menu_answers_the_keyboard() {
        // closed: Down opens at the first item, Up at the last; the rest is the button's own (Enter and Space click it)
        assert_eq!(menu_key(false, None, "ArrowDown", 2), MenuKey::Open(0));
        assert_eq!(menu_key(false, None, "ArrowUp", 2), MenuKey::Open(1));
        for key in ["Enter", " ", "Escape", "Tab", "a"] {
            assert_eq!(menu_key(false, None, key, 2), MenuKey::Ignore, "{key} on the closed button");
        }
        // open: the arrows move and wrap, Home and End jump
        assert_eq!(menu_key(true, Some(0), "ArrowDown", 2), MenuKey::Focus(1));
        assert_eq!(menu_key(true, Some(1), "ArrowDown", 2), MenuKey::Focus(0));
        assert_eq!(menu_key(true, Some(0), "ArrowUp", 2), MenuKey::Focus(1));
        assert_eq!(menu_key(true, None, "ArrowDown", 2), MenuKey::Focus(0), "from the button into the menu");
        assert_eq!(menu_key(true, Some(1), "Home", 2), MenuKey::Focus(0));
        assert_eq!(menu_key(true, Some(0), "End", 2), MenuKey::Focus(1));
        // Escape closes and gives the focus back to the button; Tab closes and lets it go on
        assert_eq!(menu_key(true, Some(1), "Escape", 2), MenuKey::Close(true));
        assert_eq!(menu_key(true, Some(0), "Tab", 2), MenuKey::Close(false));
        assert_eq!(menu_key(true, Some(0), "x", 2), MenuKey::Ignore);
        // the component wires it: one key handler on the menu and its button, and a click outside closes it
        let code = code();
        assert!(code.contains("<div class=\"nav-dl\" node_ref=root on:keydown=on_key>"));
        assert!(code.contains("window_event_listener(leptos::ev::mousedown") && code.contains("root.contains(Some(&node))"));
    }

    #[test]
    fn the_downloads_button_is_in_the_bar_for_everyone_and_before_the_currency_pills() {
        let code = code();
        let right = code.find("<div class=\"nav-right desktop-only\">").expect("the bar's right side");
        let at = |what: &str| code[right..].find(what).map(|i| i + right).unwrap_or_else(|| panic!("{what} in the bar"));
        let button = at("<NavDownloads />");
        let who = at("match auth.get()");
        let tickets = at("{tickets_pill(user.ticket_balance)}");
        let bliss = at("{bliss_pill(user.bliss_balance)}");
        let delta = at("Some(delta_pill(balance.whole()))");
        assert!(button < who && button < tickets && button < bliss, "the button comes before the pills and outside the auth split");
        assert!(tickets < bliss && bliss < delta, "Tickets, Bliss, Delta");
        assert_eq!(code.matches("<NavDownloads />").count(), 1);
    }

    #[test]
    fn the_account_menu_and_the_drawer_link_cases_right_after_purchases() {
        let code = code();
        let dropdown = code.find("<a href=\"/purchases\" class=\"dropdown-item\">").expect("Purchases in the dropdown");
        let drawer = code.find("<a href=\"/purchases\" class=\"drawer-user-link\"").expect("Purchases in the drawer");
        for (menu, at, class) in [("dropdown", dropdown, "dropdown-item"), ("drawer", drawer, "drawer-user-link")] {
            let next = code[at + 10..].find("<a href=").map(|i| i + at + 10).unwrap();
            assert!(code[next..].starts_with(&format!("<a href=\"/account/cases\" class=\"{class}\"")), "{menu}: Cases comes right after Purchases");
        }
        assert_eq!(code.matches("href=\"/account/cases\"").count(), 2);
    }

    #[test]
    fn the_drawer_lists_both_downloads() {
        let code = code();
        let footer = code.find("<div class=\"drawer-footer\">").expect("the drawer footer");
        let list = &code[footer..];
        assert!(list.contains("{DOWNLOADS.into_iter().map(|(name, href, hint)| view! {") && list.contains("class=\"drawer-download-btn\""), "the drawer lists every download");
        assert!(list.contains("{format!(\"Download {name}\")}"), "each says what it downloads");
        assert!(list.contains("close_menu()"), "choosing one closes the drawer");
        // the single old Studio-only button is gone
        assert!(!code.contains("href=\"/download\" class=\"drawer-download-btn\""));
    }

    #[test]
    fn the_account_menu_and_the_drawer_no_longer_link_the_creator_page() {
        let code = code();
        let older = ["/", "creator"].concat();
        assert!(!code.contains(&older), "a link to the older address is still here");
        assert!(!code.contains("Creator"), "a Creator entry is still here");
        // Projects is where selling lives now, in the bar and in the drawer
        assert_eq!(code.matches("href=\"/projects\"").count(), 2);
    }

    #[test]
    fn a_pill_amount_is_short_and_rounds_down() {
        for (n, shown) in [
            (0, "0"), (7, "7"), (999, "999"), (1_000, "1,000"), (1_400, "1,400"), (9_999, "9,999"),
            (10_000, "10K"), (12_345, "12.3K"), (12_399, "12.3K"), (99_999, "99.9K"), (123_456, "123K"), (999_999, "999K"),
            (1_000_000, "1M"), (1_250_000, "1.2M"), (12_300_000, "12.3M"), (999_999_999, "999M"),
            (1_500_000_000, "1.5B"), (2_000_000_000_000, "2,000B"),
        ] {
            assert_eq!(format_short_count(n), shown, "{n}");
        }
        for (bliss, shown) in [
            (0.0, "0"), (-3.0, "0"), (f64::NAN, "0"), (0.004, "0"), (0.25, "0.25"), (0.999, "0.99"),
            (1.0, "1"), (999.99, "999"), (1_400.5, "1,400"), (12_345.0, "12.3K"), (12_300_000.0, "12.3M"),
        ] {
            assert_eq!(format_short(bliss), shown, "{bliss}");
        }
        // never longer than a pill holds
        for n in [9_999u64, 99_999, 999_999, 9_999_999, 99_999_999, 999_999_999, 999_999_999_999] {
            assert!(format_short_count(n).len() <= 5, "{n}: {}", format_short_count(n));
        }
    }

    #[test]
    fn each_pill_shows_the_short_amount_and_has_the_exact_one_in_its_title() {
        let owner = Owner::new();
        let tickets = owner.with(|| tickets_pill(12_345).to_html());
        assert!(tickets.contains("title=\"12,345 Tickets\"") && tickets.contains(">12.3K</span>"), "{tickets}");
        let bliss = owner.with(|| bliss_pill(1_234_567.5).to_html());
        assert!(bliss.contains("title=\"1,234,567.50 Bliss\"") && bliss.contains(">1.2M</span>"), "{bliss}");
        assert!(bliss.contains("href=\"/bliss/history\""), "the Bliss menu stays: {bliss}");
        let delta = owner.with(|| delta_pill(1_000).to_html());
        assert!(delta.contains("title=\"1,000 Delta: usage credit for premium Studio tools\"") && delta.contains(">1,000</span>"), "{delta}");
        // the account name is in its title too, for when it truncates
        assert!(code().contains("<span class=\"nav-username\" title=username.clone()>"));
    }

    #[test]
    fn the_avatar_is_a_fixed_square_that_never_shrinks() {
        let avatar = rule(TOPBAR_CSS, ".nav-avatar");
        for decl in ["flex-shrink: 0;", "width: 28px;", "height: 28px;", "min-width: 28px;", "overflow: hidden;"] {
            assert!(avatar.contains(decl), "{decl} in .nav-avatar: {avatar}");
        }
        let img = rule(TOPBAR_CSS, ".nav-avatar img");
        for decl in ["object-fit: cover;", "width: 28px;", "height: 28px;"] {
            assert!(img.contains(decl), "{decl} in .nav-avatar img: {img}");
        }
        // the name gives way instead
        let name = rule(TOPBAR_CSS, ".nav-username");
        for decl in ["min-width: 0;", "overflow: hidden;", "text-overflow: ellipsis;", "white-space: nowrap;"] {
            assert!(name.contains(decl), "{decl} in .nav-username: {name}");
        }
    }

    #[test]
    fn the_pills_grow_with_their_amounts() {
        let pill = rule(TOPBAR_CSS, ".nav-right .nav-tickets,\n.nav-right .nav-bliss,\n.nav-right .nav-delta".replace('\n', if TOPBAR_CSS.contains("\r\n") { "\r\n" } else { "\n" }).as_str());
        for decl in ["white-space: nowrap;", "flex: none;", "min-width: 58px;", "padding: 0 12px 0 9px;"] {
            assert!(pill.contains(decl), "{decl} in the pill rule: {pill}");
        }
        let fixed = pill.lines().map(str::trim).any(|l| l.starts_with("width:") || l.starts_with("max-width:"));
        assert!(!fixed, "no fixed width on a pill: {pill}");
        let amount = rule(TOPBAR_CSS, ".nav-right .ticket-amount,\n.nav-right .nav-bliss .bliss-amount,\n.nav-right .delta-amount".replace('\n', if TOPBAR_CSS.contains("\r\n") { "\r\n" } else { "\n" }).as_str());
        assert!(amount.contains("font-variant-numeric: tabular-nums;") && amount.contains("white-space: nowrap;"), "{amount}");
    }

    #[test]
    fn the_bar_keeps_room_for_the_right_side_and_the_name_gives_way_first() {
        assert!(code().contains("<nav class=move || if matches!(auth.get(), AuthState::Authenticated(_)) { \"central-nav signed-in\" } else { \"central-nav\" }>"));
        assert!(TOPBAR_CSS.contains("grid-template-columns: minmax(max-content, 1fr) max-content minmax(var(--tb-right-min), 1fr);"));
        assert!(rule(TOPBAR_CSS, ".central-nav.signed-in").contains("--tb-right-min: 470px;"));
        let right = rule(TOPBAR_CSS, ".nav-right");
        assert!(right.contains("min-width: 0;") && right.contains("justify-self: stretch;"), "the right side may shrink to its column: {right}");
        // the pills and the Downloads button never shrink; the account button does
        assert!(rule(TOPBAR_CSS, ".nav-dl").contains("flex: none;"));
        assert!(rule(TOPBAR_CSS, ".nav-right .nav-user-dropdown").contains("min-width: 0;"));
    }

    #[test]
    fn the_drawer_slides_in_from_the_left_and_its_backdrop_covers_the_page() {
        let drawer = rule(TOPBAR_CSS, ".central-nav .mobile-drawer");
        for decl in ["left: 0;", "right: auto;", "transform: translateX(-100%);", "visibility: hidden;", "border-right: 1px solid"] {
            assert!(drawer.contains(decl), "{decl} in the closed drawer: {drawer}");
        }
        let open = rule(TOPBAR_CSS, ".central-nav .mobile-drawer.open");
        assert!(open.contains("transform: translateX(0);") && open.contains("visibility: visible;"), "{open}");
        let backdrop = rule(TOPBAR_CSS, ".central-nav .mobile-backdrop");
        assert!(backdrop.contains("bottom: auto;") && backdrop.contains("height: 100vh;"), "the backdrop covers the page: {backdrop}");
        // the menu button stays at the left of the bar on a phone
        assert!(include_str!("../../style/main.css").contains(".central-nav .hamburger-btn { order: -1;"));
    }

    #[test]
    fn the_drawer_takes_the_focus_and_gives_it_back() {
        let code = code();
        let header = code.find("<div class=\"drawer-header\">").expect("the drawer header");
        let close = code[header..].find("class=\"drawer-close\"").expect("the close button");
        let logo = code[header..].find("class=\"drawer-logo\"").expect("the logo");
        assert!(close < logo, "the close button comes first, at the left");
        assert!(code.contains("aria-controls=\"mobile-drawer\"") && code.contains("id=\"mobile-drawer\""));
        assert!(code.contains("node_ref=hamburger") && code.contains("node_ref=drawer_close"));
        // opening focuses the close button; closing from inside gives the focus back to the menu button
        let open = &code[code.find("let open_drawer = move || {").unwrap()..code.find("let dismiss_drawer = move || {").unwrap()];
        assert!(open.contains("menu_open.set(true)") && open.contains("drawer_close.get_untracked()"), "{open}");
        let dismiss = &code[code.find("let dismiss_drawer = move || {").unwrap()..];
        let dismiss = &dismiss[..dismiss.find("};").unwrap()];
        assert!(dismiss.contains("menu_open.set(false)") && dismiss.contains("hamburger.get_untracked()"), "{dismiss}");
        assert!(code.contains("if ev.key() == \"Escape\" && menu_open.get_untracked() {"), "Escape closes it");
        assert_eq!(code.matches("dismiss_drawer()").count(), 4, "the menu button, the backdrop, the close button and Escape");
    }

    #[test]
    fn the_top_bar_sheet_is_loaded_last_and_the_old_download_steps_are_gone() {
        let index = include_str!("../../index.html");
        let main = index.find("href=\"style/main.css\"").expect("main.css");
        let topbar = index.find("<link data-trunk rel=\"css\" href=\"style/topbar.css\">").expect("topbar.css is linked");
        let last_css = index.rfind("<link data-trunk rel=\"css\"").unwrap();
        assert!(main < topbar && topbar == last_css, "topbar.css comes after every other sheet");
        let sell = include_str!("../../style/projects_sell.css");
        for gone in [".nav-download-btn", ".nav-dl-verb", ".nav-downloads", "@media (max-width: 1279px)", "@media (max-width: 1699px)"] {
            assert!(!sell.contains(gone), "{gone} is still in projects_sell.css");
        }
    }
}
