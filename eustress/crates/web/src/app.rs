// =============================================================================
// Eustress Web - Main App Component
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. App Component
// 3. Router Configuration
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;
use web_sys::window;

use crate::pages::{
    about::AboutPage,
    acts::ActsPage,
    ai::AiPage,
    home::HomePage,
    login::LoginPage,
    bliss::BlissPage,
    careers::CareersPage,
    community::CommunityPage,
    contact::ContactPage,
    cookies::CookiesPage,
    dashboard::DashboardPage,
    dmca::DmcaPage,
    docs_networking::DocsNetworkingPage,
    docs_physics::DocsPhysicsPage,
    docs_scripting::DocsScriptingPage,
    download::DownloadPage,
    download_player::DownloadPlayerPage,
    experience::{ExperiencesPage, ExperienceDetailPage},
    friends::FriendsPage,
    premium::PremiumPage,
    press::PressPage,
    privacy::PrivacyPage,
    projects::ProjectsPage,
    gallery::GalleryPage,
    leaderboard::LeaderboardPage,
    learn::LearnPage,
    marketplace::MarketplacePage,
    marketplace_item::MarketplaceItemPage,
    profile::ProfilePage,
    settings::SettingsPage,
    terms::TermsPage,
};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// 2. App Component
// -----------------------------------------------------------------------------

/// Root application component with routing.
#[component]
pub fn App() -> impl IntoView {
    // Provide global app state
    let app_state = AppState::new();
    
    // Check for OAuth callback token in URL (from Discord login)
    if let Some(win) = window() {
        if let Ok(search) = win.location().search() {
            if search.contains("token=") {
                // Parse token and user_id from URL
                let params: std::collections::HashMap<String, String> = search
                    .trim_start_matches('?')
                    .split('&')
                    .filter_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        Some((parts.next()?.to_string(), parts.next()?.to_string()))
                    })
                    .collect();
                
                if let (Some(token), Some(user_id)) = (params.get("token"), params.get("user_id")) {
                    let token = token.clone();
                    let user_id = user_id.clone();
                    let app_state_clone = app_state.clone();
                    
                    // Fetch user data and complete login
                    spawn_local(async move {
                        let client = crate::api::ApiClient::new(&app_state_clone.api_url);
                        if let Ok(user) = crate::api::get_me(&client, &token).await {
                            app_state_clone.login_with_token(token, user);
                            // Clear URL params
                            if let Some(win) = window() {
                                let _ = win.history().and_then(|h| {
                                    h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/dashboard"))
                                });
                            }
                        }
                    });
                }
            }
        }
    }
    
    // Restore session from localStorage on startup
    app_state.restore_session();
    
    provide_context(app_state);
    
    view! {
        <Router>
            <Routes fallback=|| "Not found.">
                // Public routes
                <Route path=path!("/") view=HomePage />
                <Route path=path!("/login") view=LoginPage />
                <Route path=path!("/gallery") view=GalleryPage />
                <Route path=path!("/experiences") view=ExperiencesPage />
                <Route path=path!("/experience/:id") view=ExperienceDetailPage />
                <Route path=path!("/community") view=CommunityPage />
                <Route path=path!("/leaderboard") view=LeaderboardPage />
                <Route path=path!("/marketplace") view=MarketplacePage />
                <Route path=path!("/marketplace/:id") view=MarketplaceItemPage />
                <Route path=path!("/learn") view=LearnPage />
                <Route path=path!("/docs/scripting") view=DocsScriptingPage />
                <Route path=path!("/docs/physics") view=DocsPhysicsPage />
                <Route path=path!("/docs/networking") view=DocsNetworkingPage />
                <Route path=path!("/bliss") view=BlissPage />
                <Route path=path!("/download") view=DownloadPage />
                <Route path=path!("/downloads/player") view=DownloadPlayerPage />
                <Route path=path!("/about") view=AboutPage />
                <Route path=path!("/careers") view=CareersPage />
                <Route path=path!("/contact") view=ContactPage />
                <Route path=path!("/press") view=PressPage />
                <Route path=path!("/premium") view=PremiumPage />
                <Route path=path!("/profile/:username") view=ProfilePage />
                <Route path=path!("/profile") view=ProfilePage />
                
                // Legal pages
                <Route path=path!("/terms") view=TermsPage />
                <Route path=path!("/privacy") view=PrivacyPage />
                <Route path=path!("/cookies") view=CookiesPage />
                <Route path=path!("/dmca") view=DmcaPage />
                <Route path=path!("/acts") view=ActsPage />
                
                // Protected routes
                <Route path=path!("/dashboard") view=DashboardPage />
                <Route path=path!("/projects") view=ProjectsPage />
                <Route path=path!("/ai") view=AiPage />
                <Route path=path!("/settings") view=SettingsPage />
                <Route path=path!("/friends") view=FriendsPage />
            </Routes>
        </Router>
    }
}
