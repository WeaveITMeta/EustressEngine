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

use crate::state::JurisdictionState;

use crate::pages::{
    about::AboutPage,
    acts::ActsPage,
    ai::AiPage,
    home::HomePage,
    login::LoginPage,
    bliss::BlissPage,
    bliss_leaderboard::BlissLeaderboardPage,
    careers::CareersPage,
    community::CommunityPage,
    contact::ContactPage,
    cookies::CookiesPage,
    dashboard::DashboardPage,
    dmca::DmcaPage,
    docs_networking::DocsNetworkingPage,
    docs_philosophy::DocsPhilosophyPage,
    docs_physics::DocsPhysicsPage,
    docs_realism::DocsRealismPage,
    docs_scripting::DocsScriptingPage,
    docs_simulation::DocsSimulationPage,
    docs_ui::DocsUiPage,
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
    trust_registry::TrustRegistryPage,
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
    
    // Detect jurisdiction from Cloudflare trace
    let jurisdiction_signal = app_state.jurisdiction;
    spawn_local(async move {
        match detect_jurisdiction().await {
            Some((iso2, supported)) => {
                if supported {
                    jurisdiction_signal.set(JurisdictionState::Supported {
                        iso2: iso2.clone(),
                        name: jurisdiction_name(&iso2),
                    });
                } else {
                    jurisdiction_signal.set(JurisdictionState::Unsupported { iso2 });
                }
            }
            None => {
                // Can't detect (localhost, no CF) — allow through
                jurisdiction_signal.set(JurisdictionState::Supported {
                    iso2: "XX".to_string(),
                    name: "Local Development".to_string(),
                });
            }
        }
    });

    provide_context(app_state);

    view! {
        {move || {
            match jurisdiction_signal.get() {
                JurisdictionState::Detecting => {
                    view! {
                        <div class="jurisdiction-loading">
                            <div class="loading-spinner"></div>
                            <p>"Verifying jurisdiction..."</p>
                        </div>
                    }.into_any()
                }
                JurisdictionState::Unsupported { iso2 } => {
                    view! { <JurisdictionBlockedPage iso2=iso2 /> }.into_any()
                }
                JurisdictionState::Supported { .. } => {
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
                <Route path=path!("/docs/ui") view=DocsUiPage />
                <Route path=path!("/docs/simulation") view=DocsSimulationPage />
                <Route path=path!("/docs/realism") view=DocsRealismPage />
                <Route path=path!("/docs/philosophy") view=DocsPhilosophyPage />
                <Route path=path!("/bliss") view=BlissPage />
                <Route path=path!("/bliss-leaderboard") view=BlissLeaderboardPage />
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
                <Route path=path!("/trust-registry") view=TrustRegistryPage />

                // Protected routes
                <Route path=path!("/dashboard") view=DashboardPage />
                <Route path=path!("/projects") view=ProjectsPage />
                <Route path=path!("/ai") view=AiPage />
                <Route path=path!("/settings") view=SettingsPage />
                <Route path=path!("/friends") view=FriendsPage />
            </Routes>
                        </Router>
                    }.into_any()
                }
            }
        }}
    }
}

// ── Jurisdiction Detection ──────────────────────────────────────────────────

/// Supported KYC jurisdictions (IRS QI approved).
const SUPPORTED_JURISDICTIONS: &[&str] = &[
    "AD","AE","AG","AR","AU","AW","BE","BH","BM","BN","BQ","BR","BS",
    "CA","CH","CK","CN","CO","CY","CZ","DE","DK","EE","ES","FI","FR",
    "GB","GG","GI","GR","HK","HR","HU","IE","IM","IN","IS","IT","JE",
    "JP","KR","KY","KZ","LC","LI","LT","LU","LV","MC","MT","MU","MX",
    "NL","NO","NZ","PA","PL","PT","RO","RU","SA","SC","SE","SG","SI",
    "SK","SM","TC","TW","US","VC","VG","ZA",
];

/// Detect jurisdiction from Cloudflare cdn-cgi/trace.
/// Returns (iso2, is_supported) or None if detection fails.
async fn detect_jurisdiction() -> Option<(String, bool)> {
    // On localhost, skip detection
    if let Some(win) = window() {
        let hostname = win.location().hostname().unwrap_or_default();
        if hostname == "localhost" || hostname == "127.0.0.1" {
            return None; // Allow through in dev
        }
    }

    let resp = gloo_net::http::Request::get("https://1.1.1.1/cdn-cgi/trace")
        .send()
        .await
        .ok()?;
    let text = resp.text().await.ok()?;

    for line in text.lines() {
        if let Some(loc) = line.strip_prefix("loc=") {
            let iso2 = loc.trim().to_uppercase();
            let supported = SUPPORTED_JURISDICTIONS.contains(&iso2.as_str());
            return Some((iso2, supported));
        }
    }

    None
}

/// Get a human-readable name for common jurisdictions.
fn jurisdiction_name(iso2: &str) -> String {
    match iso2 {
        "US" => "United States", "CA" => "Canada", "GB" => "United Kingdom",
        "AU" => "Australia", "NZ" => "New Zealand", "DE" => "Germany",
        "FR" => "France", "JP" => "Japan", "KR" => "Korea", "IN" => "India",
        "SG" => "Singapore", "HK" => "Hong Kong", "CH" => "Switzerland",
        "NL" => "Netherlands", "SE" => "Sweden", "NO" => "Norway",
        "FI" => "Finland", "DK" => "Denmark", "IE" => "Ireland",
        "IT" => "Italy", "ES" => "Spain", "PT" => "Portugal",
        "BE" => "Belgium", "BR" => "Brazil", "MX" => "Mexico",
        "CN" => "China", "TW" => "Taiwan", "SA" => "Saudi Arabia",
        "AE" => "UAE", "XX" => "Local Development",
        other => return other.to_string(),
    }.to_string()
}

// ── Blocked Page ────────────────────────────────────────────────────────────

/// Shown when the user is in an unsupported KYC jurisdiction.
#[component]
fn JurisdictionBlockedPage(iso2: String) -> impl IntoView {
    view! {
        <div class="jurisdiction-blocked">
            <div class="blocked-card">
                <img src="/assets/logo.svg" alt="Eustress Engine" class="blocked-logo" />
                <h1>"Jurisdiction Not Supported"</h1>
                <p class="blocked-message">
                    "Your country ("<strong>{iso2.clone()}</strong>") does not currently have "
                    "Know Your Customer (KYC) legislation approved by the IRS Qualified Intermediary program."
                </p>
                <p class="blocked-detail">
                    "Eustress requires KYC compliance for identity verification and Bliss (BLS) participation. "
                    "We can only operate in jurisdictions with IRS QI-approved KYC frameworks."
                </p>
                <div class="blocked-action">
                    <p>"Inform your country's financial leadership to apply for KYC approval:"</p>
                    <a
                        href="https://www.irs.gov/businesses/forms-and-instructions-required-to-apply-for-kyc-approval"
                        target="_blank"
                        rel="noopener noreferrer"
                        class="btn btn-primary"
                    >
                        "IRS KYC Approval Application"
                    </a>
                </div>
                <p class="blocked-footer">
                    "Once your jurisdiction is approved, Eustress will automatically detect it and grant access."
                </p>
            </div>
        </div>
    }
}
