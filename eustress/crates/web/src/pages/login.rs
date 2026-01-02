// =============================================================================
// Eustress Web - Login Page
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;
use crate::api::{self, ApiClient};
use crate::components::CentralNav;
use crate::state::AppState;

/// Login/Register page.
#[component]
pub fn LoginPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let navigate = use_navigate();
    
    // Form state
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let username = RwSignal::new(String::new());
    let is_register = RwSignal::new(false);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    
    // Store api_url for closure
    let api_url = app_state.api_url.clone();
    
    // Handle form submission - use inline button handler instead of closure
    // to avoid FnOnce issues with spawn_local
    
    view! {
        <div class="page page-login">
            <CentralNav active="".to_string() />
            
            // Industrial background
            <div class="login-background">
                <div class="bg-grid"></div>
                <div class="bg-glow bg-glow-1"></div>
                <div class="bg-glow bg-glow-2"></div>
            </div>
            
            // Main content wrapper - two column layout
            <div class="login-wrapper">
                // Left side - Blueprint icons showcase
                <div class="login-showcase">
                    <div class="blueprint-grid">
                        <div class="blueprint-icon tilt-1">
                            <img src="/assets/icons/cube.svg" alt="3D Objects" />
                        </div>
                        <div class="blueprint-icon tilt-2">
                            <img src="/assets/icons/dog.svg" alt="Dog" />
                        </div>
                        <div class="blueprint-icon tilt-3">
                            <img src="/assets/icons/sparkles.svg" alt="Effects" />
                        </div>
                        <div class="blueprint-icon tilt-4">
                            <img src="/assets/icons/gamepad.svg" alt="Games" />
                        </div>
                        <div class="blueprint-icon tilt-5">
                            <img src="/assets/icons/rocket.svg" alt="Launch" />
                        </div>
                        <div class="blueprint-icon tilt-6">
                            <img src="/assets/icons/users.svg" alt="Multiplayer" />
                        </div>
                        <div class="blueprint-icon tilt-7">
                            <img src="/assets/icons/network.svg" alt="Connect" />
                        </div>
                        <div class="blueprint-icon tilt-8">
                            <img src="/assets/icons/settings.svg" alt="Tools" />
                        </div>
                        <div class="blueprint-icon tilt-9">
                            <img src="/assets/icons/trending.svg" alt="Growth" />
                        </div>
                    </div>
                    <div class="showcase-tagline">
                        <h2>"Create Without Limits"</h2>
                        <p>"Build games, simulations, and experiences with the power of Rust"</p>
                    </div>
                </div>
                
                // Right side - Centered login modal
                <div class="login-container">
                    <div class="login-card">
                        <div class="login-header">
                            <img src="/assets/logo.svg" alt="Eustress Engine" class="login-logo" />
                        </div>
                    
                    // Tab switcher
                    <div class="login-tabs">
                        <button 
                            type="button"
                            class=move || if !is_register.get() { "login-tab active" } else { "login-tab" }
                            on:click=move |_| is_register.set(false)
                        >
                            "Sign In"
                        </button>
                        <button 
                            type="button"
                            class=move || if is_register.get() { "login-tab active" } else { "login-tab" }
                            on:click=move |_| is_register.set(true)
                        >
                            "Create Account"
                        </button>
                    </div>
                    
                    <form class="login-form" on:submit=|e| e.prevent_default()>
                    {move || error.get().map(|e| view! {
                        <div class="form-error-banner">{e}</div>
                    })}
                    
                    <div class="form-field" class:hidden=move || !is_register.get()>
                        <label class="form-label">
                            "Username"
                            <span class="required">"*"</span>
                        </label>
                        <input
                            type="text"
                            class="form-input"
                            placeholder="Choose a username"
                            prop:value=move || username.get()
                            on:input=move |e| username.set(event_target_value(&e))
                            on:change=move |e| username.set(event_target_value(&e))
                        />
                    </div>
                    
                    <div class="form-field">
                        <label class="form-label">
                            "Email"
                            <span class="required">"*"</span>
                        </label>
                        <input
                            type="email"
                            class="form-input"
                            placeholder="you@example.com"
                            prop:value=move || email.get()
                            on:input=move |e| email.set(event_target_value(&e))
                            on:change=move |e| email.set(event_target_value(&e))
                        />
                    </div>
                    
                    <div class="form-field">
                        <label class="form-label">
                            "Password"
                            <span class="required">"*"</span>
                        </label>
                        <input
                            type="password"
                            class="form-input"
                            placeholder="••••••••"
                            prop:value=move || password.get()
                            on:input=move |e| password.set(event_target_value(&e))
                            on:change=move |e| password.set(event_target_value(&e))
                        />
                    </div>
                    
                    <button 
                        type="button"
                        class="btn btn-primary"
                        disabled=loading.get()
                        on:click=move |_| {
                            let email_val = email.get();
                            let password_val = password.get();
                            let username_val = username.get();
                            let registering = is_register.get();
                            let api_url = api_url.clone();
                            let nav = navigate.clone();
                            
                            if email_val.is_empty() || password_val.is_empty() {
                                error.set(Some("Please fill in all fields".to_string()));
                                return;
                            }
                            
                            if registering && username_val.is_empty() {
                                error.set(Some("Username is required".to_string()));
                                return;
                            }
                            
                            loading.set(true);
                            error.set(None);
                            
                            let app_state_clone = app_state.clone();
                            spawn_local(async move {
                                let client = ApiClient::new(&api_url);
                                
                                let result = if registering {
                                    api::register(&client, &username_val, &email_val, &password_val).await
                                } else {
                                    api::login(&client, &email_val, &password_val).await
                                };
                                
                                loading.set(false);
                                
                                match result {
                                    Ok(response) => {
                                        app_state_clone.login_with_token(response.token, response.user);
                                        nav("/dashboard", Default::default());
                                    }
                                    Err(e) => {
                                        error.set(Some(e.to_string()));
                                    }
                                }
                            });
                        }
                    >
                        {move || {
                            if loading.get() {
                                if is_register.get() { "Creating Account..." } else { "Signing In..." }
                            } else {
                                if is_register.get() { "Create Account" } else { "Sign In" }
                            }
                        }}
                    </button>
                    
                    <div class="login-divider">
                        <span>"or continue with"</span>
                    </div>
                    
                    <div class="social-logins">
                        <a href="https://api.eustress.dev/api/auth/discord" class="social-login-btn discord">
                            <img src="/assets/icons/discord.svg" alt="Discord" class="social-login-icon" />
                            "Discord"
                        </a>
                    </div>
                    </form>
                    </div>
                </div>
            </div>
        </div>
    }
}
