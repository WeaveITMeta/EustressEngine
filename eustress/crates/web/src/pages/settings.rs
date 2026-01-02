// =============================================================================
// Eustress Web - Settings Page (Roblox-style)
// =============================================================================
// Account settings, privacy, notifications, and preferences
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use web_sys::NotificationPermission;
use crate::components::{CentralNav, Footer};
use crate::services::notifications;
use crate::state::{AppState, AuthState};

/// Settings page component.
#[component]
pub fn SettingsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let navigate = use_navigate();
    let auth = app_state.auth;
    
    // Active settings tab
    let active_tab = RwSignal::new("account".to_string());
    
    // Form states
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let confirm_password = RwSignal::new(String::new());
    let loading = RwSignal::new(false);
    let success_msg = RwSignal::new(Option::<String>::None);
    let error_msg = RwSignal::new(Option::<String>::None);
    
    // Toggle states for privacy settings
    let inventory_visible = RwSignal::new(true);
    let allow_messages = RwSignal::new(true);
    let allow_friend_requests = RwSignal::new(true);
    let show_online_status = RwSignal::new(true);
    
    // Get current user info
    let user_email = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.email.clone(),
            _ => String::new(),
        }
    };
    
    let has_email = move || !user_email().is_empty();
    
    
    let has_discord = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.discord_id.is_some(),
            _ => false,
        }
    };
    
    let username = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.username.clone(),
            _ => "User".to_string(),
        }
    };
    
    let avatar_url = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.avatar_url.clone(),
            _ => None,
        }
    };
    
    // API URLs for account linking
    let api_url = app_state.api_url.clone();
    let discord_link_url = RwSignal::new(format!("{}/api/auth/discord/link", api_url));
    
    // Clone for use in closures
    let app_state_for_form = RwSignal::new(app_state.clone());
    let app_state_for_logout = app_state.clone();
    
    view! {
        <div class="page page-settings-roblox">
            <CentralNav active="".to_string() />
            
            // Background
            <div class="settings-bg">
                <div class="settings-grid-overlay"></div>
            </div>
            
            <div class="settings-layout">
                // Sidebar
                <aside class="settings-sidebar">
                    <div class="sidebar-header">
                        <div class="sidebar-avatar">
                            {move || match avatar_url() {
                                Some(url) => view! { <img src=url alt="Avatar" /> }.into_any(),
                                None => view! { <img src="/assets/icons/noob-head.svg" alt="Avatar" /> }.into_any(),
                            }}
                        </div>
                        <div class="sidebar-user-info">
                            <span class="sidebar-username">{username}</span>
                            <span class="sidebar-label">"Settings"</span>
                        </div>
                    </div>
                    
                    <nav class="settings-nav">
                        <button 
                            class="settings-nav-item"
                            class:active=move || active_tab.get() == "account"
                            on:click=move |_| active_tab.set("account".to_string())
                        >
                            <img src="/assets/icons/user.svg" alt="Account" />
                            "Account Info"
                        </button>
                        <button 
                            class="settings-nav-item"
                            class:active=move || active_tab.get() == "security"
                            on:click=move |_| active_tab.set("security".to_string())
                        >
                            <img src="/assets/icons/shield.svg" alt="Security" />
                            "Security"
                        </button>
                        <button 
                            class="settings-nav-item"
                            class:active=move || active_tab.get() == "privacy"
                            on:click=move |_| active_tab.set("privacy".to_string())
                        >
                            <img src="/assets/icons/eye.svg" alt="Privacy" />
                            "Privacy"
                        </button>
                        <button 
                            class="settings-nav-item"
                            class:active=move || active_tab.get() == "notifications"
                            on:click=move |_| active_tab.set("notifications".to_string())
                        >
                            <img src="/assets/icons/bell.svg" alt="Notifications" />
                            "Notifications"
                        </button>
                        <button 
                            class="settings-nav-item"
                            class:active=move || active_tab.get() == "billing"
                            on:click=move |_| active_tab.set("billing".to_string())
                        >
                            <img src="/assets/icons/bliss.svg" alt="Billing" />
                            "Billing"
                        </button>
                    </nav>
                    
                    <div class="sidebar-footer">
                        <button 
                            class="settings-nav-item logout"
                            on:click=move |_| {
                                app_state_for_logout.logout();
                                navigate("/", Default::default());
                            }
                        >
                            <img src="/assets/icons/logout.svg" alt="Logout" />
                            "Sign Out"
                        </button>
                    </div>
                </aside>
                
                // Main content
                <main class="settings-content">
                    // Account Info Tab
                    <Show when=move || active_tab.get() == "account">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Account Info"</h1>
                            <p class="panel-description">"Manage your account information and personal details"</p>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Personal"</h2>
                                
                                <div class="setting-row">
                                    <div class="setting-info">
                                        <span class="setting-label">"Username"</span>
                                        <span class="setting-value">{username}</span>
                                    </div>
                                    <button class="btn-edit" disabled>"Edit"</button>
                                </div>
                                
                                <div class="setting-row">
                                    <div class="setting-info">
                                        <span class="setting-label">"Email Address"</span>
                                        <span class="setting-value">
                                            {move || if has_email() { user_email() } else { "Not set".to_string() }}
                                        </span>
                                    </div>
                                    <button class="btn-edit">{move || if has_email() { "Change" } else { "Add" }}</button>
                                </div>
                                
                                <div class="setting-row">
                                    <div class="setting-info">
                                        <span class="setting-label">"Display Name"</span>
                                        <span class="setting-value">{username}</span>
                                    </div>
                                    <button class="btn-edit" disabled>"Edit"</button>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Social Accounts"</h2>
                                
                                <div class="social-account-row">
                                    <div class="social-icon discord">
                                        <img src="/assets/icons/discord.svg" alt="Discord" />
                                    </div>
                                    <div class="social-info">
                                        <span class="social-name">"Discord"</span>
                                        <span class="social-status">
                                            {move || if has_discord() { "Connected" } else { "Not connected" }}
                                        </span>
                                    </div>
                                    {move || if has_discord() {
                                        view! {
                                            <span class="connected-badge">
                                                <img src="/assets/icons/check.svg" alt="Connected" />
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <a href=discord_link_url.get() class="btn-connect">"Connect"</a>
                                        }.into_any()
                                    }}
                                </div>
                            </div>
                        </div>
                    </Show>
                    
                    // Security Tab
                    <Show when=move || active_tab.get() == "security">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Security"</h1>
                            <p class="panel-description">"Manage your password and account security"</p>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Password"</h2>
                                
                                {move || {
                                    let app_state_inner = app_state_for_form.get();
                                    if has_email() {
                                        view! {
                                            <div class="setting-row">
                                                <div class="setting-info">
                                                    <span class="setting-label">"Password"</span>
                                                    <span class="setting-value">"••••••••"</span>
                                                </div>
                                                <button class="btn-edit">"Change Password"</button>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div class="add-password-section">
                                                <p class="info-text">"Add a password to enable email login as a backup to Discord."</p>
                                                
                                                {move || success_msg.get().map(|msg| view! {
                                                    <div class="alert alert-success">
                                                        <img src="/assets/icons/check.svg" alt="Success" />
                                                        {msg}
                                                    </div>
                                                })}
                                                
                                                {move || error_msg.get().map(|msg| view! {
                                                    <div class="alert alert-error">
                                                        <img src="/assets/icons/x.svg" alt="Error" />
                                                        {msg}
                                                    </div>
                                                })}
                                                
                                                <div class="form-grid">
                                                    <div class="form-field">
                                                        <label>"Email"</label>
                                                        <input
                                                            type="email"
                                                            placeholder="you@example.com"
                                                            prop:value=move || email.get()
                                                            on:input=move |e| email.set(event_target_value(&e))
                                                        />
                                                    </div>
                                                    <div class="form-field">
                                                        <label>"Password"</label>
                                                        <input
                                                            type="password"
                                                            placeholder="••••••••"
                                                            prop:value=move || password.get()
                                                            on:input=move |e| password.set(event_target_value(&e))
                                                        />
                                                    </div>
                                                    <div class="form-field">
                                                        <label>"Confirm Password"</label>
                                                        <input
                                                            type="password"
                                                            placeholder="••••••••"
                                                            prop:value=move || confirm_password.get()
                                                            on:input=move |e| confirm_password.set(event_target_value(&e))
                                                        />
                                                    </div>
                                                </div>
                                                
                                                <button
                                                    class="btn-primary"
                                                    disabled=loading.get()
                                                    on:click=move |_| {
                                                        let email_val = email.get();
                                                        let password_val = password.get();
                                                        let confirm_val = confirm_password.get();
                                                        
                                                        if email_val.is_empty() || password_val.is_empty() {
                                                            error_msg.set(Some("Please fill in all fields".to_string()));
                                                            return;
                                                        }
                                                        
                                                        if password_val != confirm_val {
                                                            error_msg.set(Some("Passwords do not match".to_string()));
                                                            return;
                                                        }
                                                        
                                                        if password_val.len() < 8 {
                                                            error_msg.set(Some("Password must be at least 8 characters".to_string()));
                                                            return;
                                                        }
                                                        
                                                        loading.set(true);
                                                        error_msg.set(None);
                                                        success_msg.set(None);
                                                        
                                                        let app_state_clone = app_state_inner.clone();
                                                        spawn_local(async move {
                                                            let client = crate::api::ApiClient::new(&app_state_clone.api_url);
                                                            
                                                            match crate::api::add_email_password(&client, &email_val, &password_val).await {
                                                                Ok(user) => {
                                                                    success_msg.set(Some("Email and password added!".to_string()));
                                                                    app_state_clone.login(user);
                                                                    email.set(String::new());
                                                                    password.set(String::new());
                                                                    confirm_password.set(String::new());
                                                                }
                                                                Err(e) => {
                                                                    error_msg.set(Some(e.to_string()));
                                                                }
                                                            }
                                                            
                                                            loading.set(false);
                                                        });
                                                    }
                                                >
                                                    {move || if loading.get() { "Adding..." } else { "Add Email & Password" }}
                                                </button>
                                            </div>
                                        }.into_any()
                                    }
                                }}
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Two-Factor Authentication"</h2>
                                <div class="setting-row">
                                    <div class="setting-info">
                                        <span class="setting-label">"2FA Status"</span>
                                        <span class="setting-value status-disabled">"Not enabled"</span>
                                    </div>
                                    <button class="btn-edit" disabled>"Enable"</button>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Active Sessions"</h2>
                                <div class="session-item current">
                                    <div class="session-icon">
                                        <img src="/assets/icons/monitor.svg" alt="Device" />
                                    </div>
                                    <div class="session-info">
                                        <span class="session-device">"Current Session"</span>
                                        <span class="session-details">"Windows • Chrome • Active now"</span>
                                    </div>
                                    <span class="current-badge">"Current"</span>
                                </div>
                            </div>
                        </div>
                    </Show>
                    
                    // Privacy Tab
                    <Show when=move || active_tab.get() == "privacy">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Privacy"</h1>
                            <p class="panel-description">"Control who can see your information and interact with you"</p>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Profile Visibility"</h2>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Show Online Status"</span>
                                        <span class="toggle-description">"Let others see when you're online"</span>
                                    </div>
                                    <button 
                                        class="toggle-switch"
                                        class:active=move || show_online_status.get()
                                        on:click=move |_| show_online_status.update(|v| *v = !*v)
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Inventory Visible"</span>
                                        <span class="toggle-description">"Allow others to see your inventory"</span>
                                    </div>
                                    <button 
                                        class="toggle-switch"
                                        class:active=move || inventory_visible.get()
                                        on:click=move |_| inventory_visible.update(|v| *v = !*v)
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Contact Settings"</h2>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Allow Messages"</span>
                                        <span class="toggle-description">"Receive messages from other users"</span>
                                    </div>
                                    <button 
                                        class="toggle-switch"
                                        class:active=move || allow_messages.get()
                                        on:click=move |_| allow_messages.update(|v| *v = !*v)
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Allow Friend Requests"</span>
                                        <span class="toggle-description">"Receive friend requests from others"</span>
                                    </div>
                                    <button 
                                        class="toggle-switch"
                                        class:active=move || allow_friend_requests.get()
                                        on:click=move |_| allow_friend_requests.update(|v| *v = !*v)
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                            </div>
                        </div>
                    </Show>
                    
                    // Notifications Tab
                    <Show when=move || active_tab.get() == "notifications">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Notifications"</h1>
                            <p class="panel-description">"Choose what notifications you want to receive"</p>
                            
                            // Desktop Notifications Permission
                            <div class="settings-group">
                                <h2 class="group-title">"Desktop Notifications"</h2>
                                
                                <div class="notification-permission-card">
                                    <div class="permission-info">
                                        <img src="/assets/icons/bell.svg" alt="Notifications" class="permission-icon" />
                                        <div class="permission-text">
                                            <span class="permission-title">"Browser Notifications"</span>
                                            <span class="permission-description">
                                                "Get notified when your favorite experiences are updated"
                                            </span>
                                        </div>
                                    </div>
                                    <NotificationPermissionButton />
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Favorite Updates"</h2>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Experience Updates"</span>
                                        <span class="toggle-description">"When a favorited experience gets a new version"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"New Content"</span>
                                        <span class="toggle-description">"When favorites add new levels, items, or features"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Special Events"</span>
                                        <span class="toggle-description">"Limited-time events in your favorite experiences"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Social Notifications"</h2>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Messages"</span>
                                        <span class="toggle-description">"When you receive a new message"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Friend Requests"</span>
                                        <span class="toggle-description">"When someone sends you a friend request"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Friend Activity"</span>
                                        <span class="toggle-description">"When friends publish new experiences"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Email Notifications"</h2>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Weekly Digest"</span>
                                        <span class="toggle-description">"Summary of updates from your favorites"</span>
                                    </div>
                                    <button class="toggle-switch active">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                
                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Marketing Emails"</span>
                                        <span class="toggle-description">"News, updates, and promotions"</span>
                                    </div>
                                    <button class="toggle-switch">
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                            </div>
                        </div>
                    </Show>
                    
                    // Billing Tab
                    <Show when=move || active_tab.get() == "billing">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Billing"</h1>
                            <p class="panel-description">"Manage your Bliss balance and payment methods"</p>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Bliss Balance"</h2>
                                
                                <div class="bliss-balance-card">
                                    <div class="balance-icon">
                                        <img src="/assets/icons/bliss.svg" alt="Bliss" />
                                    </div>
                                    <div class="balance-info">
                                        <span class="balance-amount">"0"</span>
                                        <span class="balance-label">"Bliss"</span>
                                    </div>
                                    <a href="/bliss" class="btn-primary">"Get Bliss"</a>
                                </div>
                            </div>
                            
                            <div class="settings-group">
                                <h2 class="group-title">"Transaction History"</h2>
                                
                                <div class="empty-state-small">
                                    <img src="/assets/icons/archive.svg" alt="No transactions" />
                                    <span>"No transactions yet"</span>
                                </div>
                            </div>
                        </div>
                    </Show>
                </main>
            </div>
            
            <Footer />
        </div>
    }
}

/// Button to request notification permission.
#[component]
fn NotificationPermissionButton() -> impl IntoView {
    let permission = RwSignal::new(notifications::get_permission());
    let requesting = RwSignal::new(false);
    
    let request_permission = move |_| {
        requesting.set(true);
        spawn_local(async move {
            match notifications::request_permission().await {
                Ok(perm) => {
                    permission.set(perm);
                    
                    // Show a test notification if granted
                    if perm == NotificationPermission::Granted {
                        let _ = notifications::show_notification(
                            "Notifications Enabled!",
                            "You'll now receive updates when your favorites are updated.",
                            None,
                        );
                    }
                }
                Err(_) => {
                    // Permission request failed
                }
            }
            requesting.set(false);
        });
    };
    
    view! {
        {move || {
            let perm = permission.get();
            match perm {
                NotificationPermission::Granted => {
                    view! {
                        <span class="permission-status granted">
                            <img src="/assets/icons/check.svg" alt="Enabled" />
                            "Enabled"
                        </span>
                    }.into_any()
                }
                NotificationPermission::Denied => {
                    view! {
                        <span class="permission-status denied">
                            <img src="/assets/icons/x.svg" alt="Blocked" />
                            "Blocked"
                        </span>
                    }.into_any()
                }
                _ => {
                    view! {
                        <button 
                            class="btn-enable-notifications"
                            on:click=request_permission
                            disabled=requesting.get()
                        >
                            {move || if requesting.get() { "Requesting..." } else { "Enable" }}
                        </button>
                    }.into_any()
                }
            }
        }}
    }
}
