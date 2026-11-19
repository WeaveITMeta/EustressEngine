// =============================================================================
// Eustress Web - Settings Page (Roblox-style)
// =============================================================================
// Account settings, privacy, notifications, and preferences
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use web_sys::NotificationPermission;
use crate::api::ApiClient;
use crate::components::{CentralNav, Footer};
use crate::components::nav::account_avatar;
use crate::components::adult_content::AdultContentSetting;
use crate::pages::agents_settings::AgentsSection;
use crate::pages::email_settings::EmailSection;
use crate::pages::passkeys::PasskeysSection;
use crate::pages::replace_key::ReplaceKeySection;
use crate::services::notifications;
use crate::state::{AppState, AuthState};

/// `GET` and `PUT /api/notifications/prefs`.
#[derive(Clone, Debug, serde::Deserialize)]
struct NotificationPrefs {
    #[serde(default)]
    moderation_email: bool,
    /// Off unless the account chose it (sign-up box or this page).
    #[serde(default)]
    marketing_email: bool,
}

/// Settings page component.
#[component]
pub fn SettingsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let navigate = use_navigate();
    let auth = app_state.auth;
    
    // Active settings tab
    // `/settings?replace_key=1` (from the one-time prompt) opens on Security; `/settings?tab=privacy` (from an adult listing's notice)
    // opens on Privacy, where Adult content is the first setting.
    let query = crate::utils::current_query();
    let active_tab = RwSignal::new(if query.contains("replace_key=1") { "security" } else if query.contains("tab=privacy") { "privacy" } else { "account" }.to_string());
    
    
    // Toggle states for privacy settings
    let inventory_visible = RwSignal::new(true);
    let allow_messages = RwSignal::new(true);
    let allow_friend_requests = RwSignal::new(true);
    let show_online_status = RwSignal::new(true);

    // Whether review results are emailed. `None` until the server has said.
    let review_email = RwSignal::new(None::<bool>);
    let review_email_error = RwSignal::new(String::new());
    // Whether marketing emails are sent. `None` until the server has said.
    let marketing_email = RwSignal::new(None::<bool>);
    let marketing_email_error = RwSignal::new(String::new());
    {
        let api_url = app_state.api_url.clone();
        Effect::new(move |_| {
            let api_url = api_url.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url);
                if let Ok(prefs) = client.get::<NotificationPrefs>("/api/notifications/prefs").await {
                    review_email.set(Some(prefs.moderation_email));
                    marketing_email.set(Some(prefs.marketing_email));
                }
            });
        });
    }
    let toggle_review_email = {
        let api_url = app_state.api_url.clone();
        move |_| {
            let Some(current) = review_email.get_untracked() else { return };
            let next = !current;
            // Shown at once; put back if the server does not take it.
            review_email.set(Some(next));
            review_email_error.set(String::new());
            let api_url = api_url.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url);
                let saved = client
                    .put::<serde_json::Value, _>("/api/notifications/prefs", &serde_json::json!({ "moderation_email": next }))
                    .await;
                if saved.is_err() {
                    review_email.set(Some(current));
                    review_email_error.set("Could not save this. Try again.".to_string());
                }
            });
        }
    };

    let toggle_marketing_email = {
        let api_url = app_state.api_url.clone();
        move |_| {
            let Some(current) = marketing_email.get_untracked() else { return };
            let next = !current;
            // Shown at once; put back if the server does not take it.
            marketing_email.set(Some(next));
            marketing_email_error.set(String::new());
            let api_url = api_url.clone();
            spawn_local(async move {
                let client = ApiClient::new(&api_url);
                let saved = client
                    .put::<serde_json::Value, _>("/api/notifications/prefs", &serde_json::json!({ "marketing_email": next }))
                    .await;
                if saved.is_err() {
                    marketing_email.set(Some(current));
                    marketing_email_error.set("Could not save this. Try again.".to_string());
                }
            });
        }
    };

    // Get current user info
    let user_email = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.email.clone().unwrap_or_default(),
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
    let discord_status = move || match auth.get() {
        AuthState::Authenticated(user) => match (user.discord_id.is_some(), user.discord_username) {
            (true, Some(handle)) => format!("Connected as {handle}"),
            (true, None) => "Connected".to_string(),
            (false, _) => "Not connected".to_string(),
        },
        _ => "Not connected".to_string(),
    };
    
    let has_slack = move || match auth.get() {
        AuthState::Authenticated(user) => user.slack_linked,
        _ => false,
    };
    let slack_status = move || match auth.get() {
        AuthState::Authenticated(user) => crate::api::slack_status_text(&user),
        _ => "Not connected".to_string(),
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
    
    // Linking Discord (src/api/discord.rs): the Worker makes Discord's address and
    // the browser goes there; Discord sends it back to /auth/discord/callback.
    let discord_busy = RwSignal::new(false);
    let discord_error = RwSignal::new(String::new());
    // Callbacks are Copy, so the reactive view below can hand them to each render.
    let connect_discord = {
        let api_url = app_state.api_url.clone();
        Callback::new(move |_: ()| {
            if discord_busy.get_untracked() {
                return;
            }
            discord_busy.set(true);
            discord_error.set(String::new());
            let api_url = api_url.clone();
            spawn_local(async move {
                match crate::api::begin_discord_link(&api_url).await {
                    Ok(authorize_url) => {
                        // Stays busy: the page is leaving for Discord.
                        if let Some(window) = web_sys::window() {
                            if window.location().assign(&authorize_url).is_err() {
                                discord_busy.set(false);
                                discord_error.set("Your browser did not open Discord. Try again.".to_string());
                            }
                        }
                    }
                    Err(e) => {
                        discord_busy.set(false);
                        discord_error.set(crate::api::discord_failure_sentence(&e));
                    }
                }
            });
        })
    };
    let disconnect_discord = {
        let app_state = app_state.clone();
        Callback::new(move |_: ()| {
            if discord_busy.get_untracked() {
                return;
            }
            discord_busy.set(true);
            discord_error.set(String::new());
            let app_state = app_state.clone();
            spawn_local(async move {
                match crate::api::unlink_discord(&app_state.api_url).await {
                    Ok(user) => app_state.login(user),
                    Err(e) => discord_error.set(crate::api::discord_failure_sentence(&e)),
                }
                discord_busy.set(false);
            });
        })
    };

    // Linking Slack (src/api/slack.rs): the same flow as Discord. Slack sends the
    // browser back to /auth/slack/callback.
    let slack_busy = RwSignal::new(false);
    let slack_error = RwSignal::new(String::new());
    let connect_slack = {
        let api_url = app_state.api_url.clone();
        Callback::new(move |_: ()| {
            if slack_busy.get_untracked() {
                return;
            }
            slack_busy.set(true);
            slack_error.set(String::new());
            let api_url = api_url.clone();
            spawn_local(async move {
                match crate::api::begin_slack_link(&api_url).await {
                    Ok(authorize_url) => {
                        // Stays busy: the page is leaving for Slack.
                        if let Some(window) = web_sys::window() {
                            if window.location().assign(&authorize_url).is_err() {
                                slack_busy.set(false);
                                slack_error.set("Your browser did not open Slack. Try again.".to_string());
                            }
                        }
                    }
                    Err(e) => {
                        slack_busy.set(false);
                        slack_error.set(crate::api::slack_failure_sentence(&e));
                    }
                }
            });
        })
    };
    let disconnect_slack = {
        let app_state = app_state.clone();
        Callback::new(move |_: ()| {
            if slack_busy.get_untracked() {
                return;
            }
            slack_busy.set(true);
            slack_error.set(String::new());
            let app_state = app_state.clone();
            spawn_local(async move {
                match crate::api::unlink_slack(&app_state.api_url).await {
                    Ok(user) => app_state.login(user),
                    Err(e) => slack_error.set(crate::api::slack_failure_sentence(&e)),
                }
                slack_busy.set(false);
            });
        })
    };

    // Clone for use in closures
    let navigate_home = StoredValue::new_local(navigate.clone());
    let sign_out = crate::components::sign_out::use_sign_out(Callback::new(move |_| navigate_home.with_value(|go| go("/", Default::default()))));
    
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
                            {account_avatar(avatar_url)}
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
                        <button class="settings-nav-item logout" title=crate::components::sign_out::THIS_DEVICE_HELP on:click=move |_| sign_out.this_device.run(())>
                            <img src="/assets/icons/logout.svg" alt="" />
                            "Sign out of this device"
                        </button>
                        <button class="settings-nav-item logout" title=crate::components::sign_out::EVERYWHERE_HELP disabled=move || sign_out.busy.get() on:click=move |_| sign_out.everywhere.run(())>
                            <img src="/assets/icons/logout.svg" alt="" />
                            "Sign out everywhere"
                        </button>
                        {move || sign_out.error.get().map(|e| view! { <p class="sign-out-error" role="alert">{e}</p> })}
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
                                        <span class="social-status">{discord_status}</span>
                                    </div>
                                    {move || if has_discord() {
                                        view! {
                                            <span class="connected-badge">
                                                <img src="/assets/icons/check.svg" alt="Connected" />
                                            </span>
                                            <button
                                                class="btn-edit"
                                                disabled=move || discord_busy.get()
                                                on:click=move |_| disconnect_discord.run(())
                                            >
                                                {move || if discord_busy.get() { "Disconnecting..." } else { "Disconnect" }}
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button
                                                class="btn-connect"
                                                disabled=move || discord_busy.get()
                                                on:click=move |_| connect_discord.run(())
                                            >
                                                {move || if discord_busy.get() { "Opening Discord..." } else { "Connect" }}
                                            </button>
                                        }.into_any()
                                    }}
                                </div>
                                <p class="settings-hint">
                                    "Linking shows your Eustress profile to people who use /profile on you in Discord. "
                                    <a href="/discord">"What the Discord app does"</a>
                                </p>
                                <Show when=move || !discord_error.get().is_empty()>
                                    <p class="settings-error" role="alert">{move || discord_error.get()}</p>
                                </Show>

                                <div class="social-account-row">
                                    <div class="social-icon slack">
                                        <img src="/assets/icons/slack.svg" alt="Slack" />
                                    </div>
                                    <div class="social-info">
                                        <span class="social-name">"Slack"</span>
                                        <span class="social-status">{slack_status}</span>
                                    </div>
                                    {move || if has_slack() {
                                        view! {
                                            <span class="connected-badge">
                                                <img src="/assets/icons/check.svg" alt="Connected" />
                                            </span>
                                            <button
                                                class="btn-edit"
                                                disabled=move || slack_busy.get()
                                                on:click=move |_| disconnect_slack.run(())
                                            >
                                                {move || if slack_busy.get() { "Disconnecting..." } else { "Disconnect" }}
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <button
                                                class="btn-connect"
                                                disabled=move || slack_busy.get()
                                                on:click=move |_| connect_slack.run(())
                                            >
                                                {move || if slack_busy.get() { "Opening Slack..." } else { "Connect" }}
                                            </button>
                                        }.into_any()
                                    }}
                                </div>
                                <p class="settings-hint">
                                    "Linking shows your Eustress profile to people who use /eustress profile on you in Slack. It reads only your Slack name and workspace, keeps no Slack token, and cannot read your messages. "
                                    <a href="/slack">"What the Slack app does"</a>
                                </p>
                                <Show when=move || !slack_error.get().is_empty()>
                                    <p class="settings-error" role="alert">{move || slack_error.get()}</p>
                                </Show>
                            </div>
                        </div>
                    </Show>
                    
                    // Security Tab
                    <Show when=move || active_tab.get() == "security">
                        <div class="settings-panel">
                            <h1 class="panel-title">"Security"</h1>
                            <p class="panel-description">"Your identity file and how you sign in"</p>
                            
                            <ReplaceKeySection />

                            <PasskeysSection />

                            <EmailSection />

                            <AgentsSection />

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

                            <AdultContentSetting />
                            
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
                            
                            // Review results by email
                            <div class="settings-group">
                                <h2 class="group-title">"Review Results"</h2>

                                <div class="toggle-row">
                                    <div class="toggle-info">
                                        <span class="toggle-label">"Email me review results"</span>
                                        <span class="toggle-description">
                                            "When a simulation you published is listed, not listed, or needs changes"
                                        </span>
                                    </div>
                                    <button
                                        class="toggle-switch"
                                        class:active=move || review_email.get() == Some(true)
                                        disabled=move || review_email.get().is_none()
                                        aria-label="Email me review results"
                                        on:click=toggle_review_email.clone()
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                <Show when=move || !review_email_error.get().is_empty()>
                                    <p class="settings-error">{move || review_email_error.get()}</p>
                                </Show>
                            </div>

                            // Desktop Notifications Permission
                            <div class="settings-group">
                                <h2 class="group-title">"Desktop Notifications"</h2>
                                
                                <div class="notification-permission-card">
                                    <div class="permission-info">
                                        <img src="/assets/icons/bell.svg" alt="Notifications" class="permission-icon" />
                                        <div class="permission-text">
                                            <span class="permission-title">"Browser Notifications"</span>
                                            <span class="permission-description">
                                                "Get notified when your favorite simulations are updated"
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
                                        <span class="toggle-label">"Simulation Updates"</span>
                                        <span class="toggle-description">"When a favorited simulation gets a new version"</span>
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
                                        <span class="toggle-description">"Eustress news, release notes, your stats and holiday promotions. Unsubscribe anytime."</span>
                                    </div>
                                    <button
                                        type="button"
                                        class="toggle-switch"
                                        role="switch"
                                        aria-label="Marketing Emails"
                                        aria-checked=move || (marketing_email.get() == Some(true)).to_string()
                                        class:active=move || marketing_email.get() == Some(true)
                                        disabled=move || marketing_email.get().is_none()
                                        on:click=toggle_marketing_email.clone()
                                    >
                                        <span class="toggle-knob"></span>
                                    </button>
                                </div>
                                <Show when=move || !marketing_email_error.get().is_empty()>
                                    <p class="settings-error">{move || marketing_email_error.get()}</p>
                                </Show>
                            </div>
                        </div>
                    </Show>
                    
                    // Billing Tab
                    <Show when=move || active_tab.get() == "billing">
                        <BillingSection />
                    </Show>
                </main>
            </div>
            
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Billing
// -----------------------------------------------------------------------------

/// One day of the account's Bliss ledger (`GET /api/ledger/history/{id}`).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct LedgerDay {
    /// ISO date, or "opening" for the balance carried in before the ledger.
    date: String,
    change: f64,
    #[serde(default)]
    kinds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
struct LedgerHistory {
    #[serde(default)]
    balance: f64,
    #[serde(default)]
    series: Vec<LedgerDay>,
}

/// How many of the newest ledger days the tab shows; the rest are on /bliss/history.
const RECENT_DAYS: usize = 5;

/// The newest days first, at most `RECENT_DAYS`, days with no net change left out.
fn recent_days(series: &[LedgerDay]) -> Vec<LedgerDay> {
    series.iter().rev().filter(|d| (d.change * 100.0).round() != 0.0).take(RECENT_DAYS).cloned().collect()
}

/// What a ledger day was, in words.
fn day_label(day: &LedgerDay) -> &'static str {
    let has = |k: &str| day.kinds.iter().any(|x| x == k);
    if day.date == "opening" || has("migration_opening") {
        "Opening balance"
    } else if has("spend") && day.change < 0.0 {
        "Spent"
    } else if has("emission") {
        "Earned from contribution"
    } else if has("spend") {
        "Spent and earned"
    } else {
        "Ledger entry"
    }
}

/// "+1.50 BLS" or "-0.25 BLS".
fn signed_bliss(change: f64) -> String {
    let sign = if change < 0.0 { "-" } else { "+" };
    format!("{sign}{} BLS", crate::utils::format_bliss_exact(change.abs()))
}

/// The Billing tab: the account's Bliss and Ticket balances as the server holds
/// them now, the newest Bliss ledger activity, and where purchases are paid.
#[component]
fn BillingSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;

    // The account copy shows at once; the ledger and the wallet replace it.
    let stored = move || match auth.get() {
        AuthState::Authenticated(user) => Some((user.id.to_string(), user.bliss_balance, user.ticket_balance)),
        _ => None,
    };
    let bliss = RwSignal::new(None::<f64>);
    let tickets = RwSignal::new(None::<u64>);
    let recent = RwSignal::new(None::<Vec<LedgerDay>>);
    let ledger_error = RwSignal::new(false);
    let loaded_for = StoredValue::new(String::new());

    {
        let api_url = app_state.api_url.clone();
        Effect::new(move |_| {
            let Some((user_id, _, _)) = stored() else { return };
            if loaded_for.get_value() == user_id {
                return;
            }
            loaded_for.set_value(user_id.clone());
            let api_url_ledger = api_url.clone();
            spawn_local(async move {
                // A public read with its own CORS (no Authorization header allowed), so it is a plain GET with no token.
                let url = format!("{api_url_ledger}/api/ledger/history/{user_id}");
                let answer = match gloo_net::http::Request::get(&url).send().await {
                    Ok(resp) if resp.ok() => resp.json::<LedgerHistory>().await.ok(),
                    _ => None,
                };
                match answer {
                    Some(history) => {
                        bliss.set(Some(history.balance));
                        recent.set(Some(recent_days(&history.series)));
                        ledger_error.set(false);
                    }
                    None => ledger_error.set(true),
                }
            });
            let api_url_wallet = api_url.clone();
            spawn_local(async move {
                if let Ok(n) = crate::api::get_ticket_balance(&api_url_wallet).await {
                    tickets.set(Some(n));
                }
            });
        });
    }

    let bliss_shown = move || {
        let amount = bliss.get().or_else(|| stored().map(|(_, b, _)| b)).unwrap_or(0.0);
        crate::utils::format_bliss_exact(amount)
    };
    let tickets_shown = move || {
        let n = tickets.get().or_else(|| stored().map(|(_, _, t)| t)).unwrap_or(0);
        crate::utils::format_count(n)
    };

    view! {
        <div class="settings-panel">
            <h1 class="panel-title">"Billing"</h1>
            <p class="panel-description">"Your Bliss and Ticket balances, your Bliss activity, and your receipts"</p>

            <div class="settings-group">
                <h2 class="group-title">"Balances"</h2>

                <div class="bliss-balance-card">
                    <div class="balance-icon">
                        <img src="/assets/icons/bliss.svg" alt="" />
                    </div>
                    <div class="balance-info">
                        <span class="balance-amount">{bliss_shown}</span>
                        <span class="balance-label">"Bliss (BLS)"</span>
                    </div>
                    <a href="/bliss" class="btn-primary">"Get Bliss"</a>
                </div>

                <div class="bliss-balance-card billing-tickets-card">
                    <div class="balance-icon">
                        <img src="/assets/icons/ticket.svg" alt="" />
                    </div>
                    <div class="balance-info">
                        <span class="balance-amount">{tickets_shown}</span>
                        <span class="balance-label">"Tickets"</span>
                    </div>
                    <a href="/tickets" class="btn-primary">"Get Tickets"</a>
                </div>
            </div>

            <div class="settings-group">
                <h2 class="group-title">"Recent Bliss Activity"</h2>
                {move || {
                    if ledger_error.get() {
                        return view! {
                            <p class="settings-error">"Your Bliss activity could not be loaded. Your balance is safe; try again in a minute."</p>
                        }.into_any();
                    }
                    match recent.get() {
                        None => view! { <p class="billing-note">"Loading your activity..."</p> }.into_any(),
                        Some(days) if days.is_empty() => view! {
                            <div class="empty-state-small">
                                <img src="/assets/icons/archive.svg" alt="" />
                                <span>"No Bliss activity yet"</span>
                            </div>
                        }.into_any(),
                        Some(days) => view! {
                            <ul class="billing-activity">
                                {days.into_iter().map(|day| {
                                    let when = if day.date == "opening" { "Before the ledger".to_string() } else { day.date.clone() };
                                    let label = day_label(&day);
                                    let negative = day.change < 0.0;
                                    view! {
                                        <li class="billing-activity-row">
                                            <span class="billing-activity-what">
                                                <span class="billing-activity-label">{label}</span>
                                                <span class="billing-activity-date">{when}</span>
                                            </span>
                                            <span class="billing-activity-amount" class:negative=negative>{signed_bliss(day.change)}</span>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any(),
                    }
                }}
                <div class="billing-links">
                    <a href="/bliss/history" class="btn-edit">"Full Bliss ledger"</a>
                    <a href="/purchases" class="btn-edit">"Purchases and receipts"</a>
                </div>
            </div>

            <div class="settings-group">
                <h2 class="group-title">"Payment"</h2>
                <p class="billing-note">
                    "Bliss and Tickets are paid for through Stripe Checkout. Your card details go to Stripe; Eustress never sees or stores your card number."
                </p>
            </div>
        </div>
    }
}

#[cfg(test)]
mod billing_tests {
    use super::*;

    fn day(date: &str, change: f64, kinds: &[&str]) -> LedgerDay {
        LedgerDay { date: date.into(), change, kinds: kinds.iter().map(|k| k.to_string()).collect() }
    }

    #[test]
    fn recent_activity_is_newest_first_capped_and_skips_days_that_net_to_nothing() {
        let series: Vec<LedgerDay> = vec![
            day("opening", 10.0, &["migration_opening"]),
            day("2026-11-01", 1.25, &["emission"]),
            day("2026-11-02", 0.0, &["emission", "spend"]),
            day("2026-11-03", 0.5, &["emission"]),
            day("2026-11-04", -2.0, &["spend"]),
            day("2026-11-05", 0.75, &["emission"]),
            day("2026-11-06", 0.25, &["emission"]),
        ];
        let shown: Vec<String> = recent_days(&series).into_iter().map(|d| d.date).collect();
        assert_eq!(shown, vec!["2026-11-06", "2026-11-05", "2026-11-04", "2026-11-03", "2026-11-01"]);
        assert!(recent_days(&[]).is_empty());
    }

    #[test]
    fn a_day_says_what_it_was_and_by_how_much() {
        assert_eq!(day_label(&day("opening", 10.0, &["migration_opening"])), "Opening balance");
        assert_eq!(day_label(&day("2026-11-04", -2.0, &["spend"])), "Spent");
        assert_eq!(day_label(&day("2026-11-04", -1.0, &["emission", "spend"])), "Spent");
        assert_eq!(day_label(&day("2026-11-05", 0.75, &["emission"])), "Earned from contribution");
        assert_eq!(day_label(&day("2026-11-05", 0.75, &["emission", "spend"])), "Earned from contribution");
        assert_eq!(signed_bliss(1234.5), "+1,234.50 BLS");
        assert_eq!(signed_bliss(-0.25), "-0.25 BLS");
    }

    #[test]
    fn the_ledger_reply_reads() {
        let body = r#"{"user_id":"x","unit":{"decimals":2,"minor_per_bls":100},"points":1,"balance":12.5,"balance_minor":1250,
            "series":[{"date":"2026-11-01","change":12.5,"change_minor":1250,"cumulative":12.5,"cumulative_minor":1250,"kinds":["emission"]}]}"#;
        let h: LedgerHistory = serde_json::from_str(body).unwrap();
        assert_eq!(h.balance, 12.5);
        assert_eq!(h.series.len(), 1);
    }

    #[test]
    fn each_linked_app_row_points_at_its_own_page() {
        let source = include_str!("settings.rs");
        let page = &source[..source.find("#[cfg(test)]").expect("the tests module")];
        assert!(page.contains(r#"<a href="/discord">"What the Discord app does"</a>"#));
        assert!(page.contains(r#"<a href="/slack">"What the Slack app does"</a>"#));
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
