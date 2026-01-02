// =============================================================================
// Eustress Web - Layout Components
// =============================================================================
// Table of Contents:
// 1. Layout (Main App Shell)
// 2. Header
// 3. Sidebar
// 4. Footer
// =============================================================================

use leptos::prelude::*;
use crate::state::{AppState, AuthState};

// -----------------------------------------------------------------------------
// 1. Layout (Main App Shell)
// -----------------------------------------------------------------------------

/// Main application layout with header, sidebar, and content area.
#[component]
pub fn Layout(children: Children) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let dark_mode = app_state.dark_mode;
    
    view! {
        <div class="layout" class:dark=move || dark_mode.get()>
            <Header />
            <div class="layout-body">
                <Sidebar />
                <main class="layout-content">
                    {children()}
                </main>
            </div>
        </div>
    }
}

// -----------------------------------------------------------------------------
// 2. Header
// -----------------------------------------------------------------------------

/// Top navigation header.
#[component]
pub fn Header() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    let dark_mode = app_state.dark_mode;
    
    let user_display = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.username.clone(),
            _ => "Guest".to_string(),
        }
    };
    
    let app_state_clone = app_state.clone();
    let toggle_dark = move |_| {
        app_state_clone.toggle_dark_mode();
    };
    
    view! {
        <header class="header">
            <div class="header-brand">
                <a href="/" class="header-logo">
                    <span class="logo-icon">"⚡"</span>
                    <span class="logo-text">"Eustress"</span>
                </a>
            </div>
            
            <nav class="header-nav">
                <a href="/" class="nav-link">"Home"</a>
                <a href="/gallery" class="nav-link">"Gallery"</a>
                <a href="/create" class="nav-link">"Create"</a>
                <a href="/dashboard" class="nav-link">"Dashboard"</a>
                <a href="/projects" class="nav-link">"Projects"</a>
                <a href="/marketplace" class="nav-link">"Marketplace"</a>
                <a href="/learn" class="nav-link">"Learn"</a>
                <a href="/community" class="nav-link">"Community"</a>
            </nav>
            
            <div class="header-actions">
                <button class="btn-icon" on:click=toggle_dark title="Toggle theme">
                    {move || if dark_mode.get() { "🌙" } else { "☀️" }}
                </button>
                
                <div class="user-menu">
                    <span class="user-name">{user_display}</span>
                </div>
            </div>
        </header>
    }
}

// -----------------------------------------------------------------------------
// 3. Sidebar
// -----------------------------------------------------------------------------

/// Left sidebar navigation.
#[component]
pub fn Sidebar() -> impl IntoView {
    view! {
        <aside class="sidebar">
            <nav class="sidebar-nav">
                <a href="/dashboard" class="sidebar-link">
                    <span class="sidebar-icon">"📊"</span>
                    <span class="sidebar-text">"Dashboard"</span>
                </a>
                <a href="/projects" class="sidebar-link">
                    <span class="sidebar-icon">"📁"</span>
                    <span class="sidebar-text">"Projects"</span>
                </a>
                <a href="/templates" class="sidebar-link">
                    <span class="sidebar-icon">"📋"</span>
                    <span class="sidebar-text">"Templates"</span>
                </a>
                <a href="/settings" class="sidebar-link">
                    <span class="sidebar-icon">"⚙️"</span>
                    <span class="sidebar-text">"Settings"</span>
                </a>
            </nav>
        </aside>
    }
}

// -----------------------------------------------------------------------------
// 4. Footer
// -----------------------------------------------------------------------------

/// Page footer.
#[component]
pub fn Footer() -> impl IntoView {
    view! {
        <footer class="footer">
            <p>"© 2025 Eustress Engine. Built with Leptos + Rust."</p>
        </footer>
    }
}
