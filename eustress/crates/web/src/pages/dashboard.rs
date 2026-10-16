// =============================================================================
// Eustress Web - Dashboard Page (Industrial Design)
// =============================================================================
// User dashboard with quick actions, stats, and recent activity
// Recent Spaces are the account's own listings (GET /api/projects), newest edit first
// =============================================================================

use leptos::prelude::*;
use crate::api::ApiClient;
use crate::components::{CentralNav, Footer};
use crate::pages::projects::{ProjectsResponse, SpaceStatus};
use crate::state::{AppState, AuthState};

/// How many Spaces the dashboard shows.
const RECENT: usize = 6;

/// One of the account's Spaces, as the dashboard shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub last_modified: String,
    /// The review state in words ("Published", "In review", ...).
    pub status: String,
    pub status_class: String,
}

/// The newest `RECENT` Spaces first. Edit times are ISO text, which sorts as time does.
pub fn most_recent(mut rows: Vec<RecentProject>) -> Vec<RecentProject> {
    rows.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    rows.truncate(RECENT);
    rows
}

/// "2026-10-01T22:01:09Z" reads as "2026-10-01"; text that is not a date is shown as it is.
pub fn day_of(iso: &str) -> String {
    iso.split('T').next().unwrap_or(iso).to_string()
}

/// User dashboard page.
#[component]
pub fn DashboardPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    
    // The account's own Spaces: the same list the Projects page shows.
    let recent_projects = RwSignal::new(Vec::<RecentProject>::new());
    let is_loading = RwSignal::new(true);
    let failed = RwSignal::new(false);
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        if !matches!(auth.get(), AuthState::Authenticated(_)) {
            return;
        }
        let api_url = api_url.clone();
        leptos::task::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match client.get::<ProjectsResponse>("/api/projects").await {
                Ok(response) => {
                    let rows = response
                        .projects
                        .into_iter()
                        .map(|p| {
                            let status = SpaceStatus::from_api(&p.status);
                            RecentProject {
                                id: p.id,
                                name: p.name,
                                thumbnail_url: p.thumbnail_url.filter(|u| u.starts_with("https://") || u.starts_with('/')),
                                last_modified: p.last_edited,
                                status: status.display_name().to_string(),
                                status_class: status.as_str().to_string(),
                            }
                        })
                        .collect();
                    recent_projects.set(most_recent(rows));
                    failed.set(false);
                }
                Err(_) => failed.set(true),
            }
            is_loading.set(false);
        });
    });

    let username = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.username,
            _ => "User".to_string(),
        }
    };
    
    let bliss_balance = move || {
        match auth.get() {
            // Whole BLS with 2 decimals (fractional after daily distribution).
            AuthState::Authenticated(user) => format!("{:.2}", user.bliss_balance),
            _ => "0.00".to_string(),
        }
    };
    
    view! {
        <div class="page page-dashboard-industrial">
            <CentralNav active="home".to_string() />
            
            // Background
            <div class="dashboard-bg">
                <div class="dashboard-grid-overlay"></div>
                <div class="dashboard-glow glow-1"></div>
                <div class="dashboard-glow glow-2"></div>
            </div>
            
            // Hero Section
            <section class="dashboard-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"DASHBOARD"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="dashboard-title">"Welcome back, " {username} "!"</h1>
                <p class="dashboard-subtitle">"Here's what's happening with your projects."</p>
            </section>
            
            <div class="dashboard-container">
                // Stats Banner
                <section class="dashboard-stats">
                    <div class="stat-card">
                        <div class="stat-icon">
                            <img src="/assets/icons/folder.svg" alt="Projects" />
                        </div>
                        <div class="stat-content">
                            <span class="stat-value">"0"</span>
                            <span class="stat-label">"Projects"</span>
                        </div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-icon">
                            <img src="/assets/icons/rocket.svg" alt="Published" />
                        </div>
                        <div class="stat-content">
                            <span class="stat-value">"0"</span>
                            <span class="stat-label">"Published"</span>
                        </div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-icon">
                            <img src="/assets/icons/users.svg" alt="Views" />
                        </div>
                        <div class="stat-content">
                            <span class="stat-value">"0"</span>
                            <span class="stat-label">"Total Views"</span>
                        </div>
                    </div>
                    <div class="stat-card bliss-gold">
                        <div class="stat-icon">
                            <img src="/assets/icons/bliss.svg" alt="Bliss" />
                        </div>
                        <div class="stat-content">
                            <span class="stat-value bliss-gold-text">{bliss_balance}</span>
                            <span class="stat-label">"Bliss Balance"</span>
                        </div>
                    </div>
                </section>
                
                // Quick Actions
                <section class="dashboard-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/sparkles.svg" alt="Actions" class="section-icon" />
                        <h2>"Quick Actions"</h2>
                    </div>
                    
                    <div class="quick-actions-grid">
                        <a href="/projects" class="action-card">
                            <div class="action-icon">
                                <img src="/assets/icons/folder.svg" alt="Projects" />
                            </div>
                            <div class="action-content">
                                <h3>"My Projects"</h3>
                                <p>"View and manage your projects"</p>
                            </div>
                            <span class="action-arrow">"→"</span>
                        </a>
                        <a href="/download" class="action-card">
                            <div class="action-icon">
                                <img src="/assets/icons/download.svg" alt="Download" />
                            </div>
                            <div class="action-content">
                                <h3>"Download Studio"</h3>
                                <p>"Get the latest Eustress Engine"</p>
                            </div>
                            <span class="action-arrow">"→"</span>
                        </a>
                        <a href="/gallery" class="action-card">
                            <div class="action-icon">
                                <img src="/assets/icons/grid.svg" alt="Gallery" />
                            </div>
                            <div class="action-content">
                                <h3>"Explore Gallery"</h3>
                                <p>"Discover amazing simulations"</p>
                            </div>
                            <span class="action-arrow">"→"</span>
                        </a>
                        <a href="/learn" class="action-card">
                            <div class="action-icon">
                                <img src="/assets/icons/book.svg" alt="Learn" />
                            </div>
                            <div class="action-content">
                                <h3>"Learn"</h3>
                                <p>"Tutorials and documentation"</p>
                            </div>
                            <span class="action-arrow">"→"</span>
                        </a>
                    </div>
                </section>
                
                // Recent Spaces: the account's own listings
                <section class="dashboard-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/clock.svg" alt="" class="section-icon" />
                        <h2>"Recent Spaces"</h2>
                        <a href="/projects" class="rp-all">"View all"</a>
                    </div>

                    <div class="recent-projects-card">
                        {move || {
                            if is_loading.get() {
                                return view! {
                                    <div class="loading-state" role="status">
                                        <div class="spinner"></div>
                                        <p>"Loading your Spaces..."</p>
                                    </div>
                                }.into_any();
                            }
                            if failed.get() {
                                return view! {
                                    <div class="empty-state" role="alert">
                                        <h3>"Your Spaces could not load"</h3>
                                        <p>"Refresh the page to try again."</p>
                                    </div>
                                }.into_any();
                            }
                            let projects = recent_projects.get();
                            if projects.is_empty() {
                                return view! {
                                    <div class="empty-state">
                                        <img src="/assets/icons/folder.svg" alt="" class="empty-icon" />
                                        <h3>"No Spaces yet"</h3>
                                        <p>"Build one in Eustress Studio and publish it. It shows up here, and in the Gallery once it is approved."</p>
                                        <a href="/download" class="btn-primary-steel">"Get Eustress Studio"</a>
                                    </div>
                                }.into_any();
                            }
                            view! {
                                <div class="rp-grid">
                                    {projects.into_iter().map(|project| {
                                        let studio = format!("eustress://edit/{}", project.id);
                                        let page = format!("/simulation/{}", project.id);
                                        let status_class = format!("rp-status {}", project.status_class);
                                        view! {
                                            <article class="rp-card">
                                                <a class="rp-thumb" href=page.clone() aria-label=format!("Open the page of {}", project.name)>
                                                    {match project.thumbnail_url.clone() {
                                                        Some(url) => view! { <img src=url alt="" loading="lazy" decoding="async" /> }.into_any(),
                                                        None => view! { <span class="rp-thumb-empty"><img src="/assets/icons/cube.svg" alt="" /></span> }.into_any(),
                                                    }}
                                                </a>
                                                <div class="rp-info">
                                                    <h3><a href=page>{project.name.clone()}</a></h3>
                                                    <span class="rp-when">{format!("Edited {}", day_of(&project.last_modified))}</span>
                                                    <span class=status_class>{project.status.clone()}</span>
                                                </div>
                                                <a class="btn-open-project" href=studio>"Edit in Studio"</a>
                                            </article>
                                        }
                                    }).collect_view()}
                                </div>
                            }.into_any()
                        }}
                    </div>
                </section>

                // Getting Started
                <section class="dashboard-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/rocket.svg" alt="Start" class="section-icon" />
                        <h2>"Getting Started"</h2>
                    </div>
                    
                    <div class="getting-started-grid">
                        <div class="step-card">
                            <span class="step-number">"1"</span>
                            <h4>"Create a Project"</h4>
                            <p>"Start from scratch or use a template"</p>
                        </div>
                        <div class="step-card">
                            <span class="step-number">"2"</span>
                            <h4>"Design Your Scene"</h4>
                            <p>"Use the visual editor to build your world"</p>
                        </div>
                        <div class="step-card">
                            <span class="step-number">"3"</span>
                            <h4>"Add Behaviors"</h4>
                            <p>"Write scripts to bring it to life"</p>
                        </div>
                        <div class="step-card">
                            <span class="step-number">"4"</span>
                            <h4>"Publish & Share"</h4>
                            <p>"Share your creation with the world"</p>
                        </div>
                    </div>
                </section>
            </div>
            
            <Footer />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, when: &str) -> RecentProject {
        RecentProject { id: id.to_string(), name: id.to_string(), thumbnail_url: None, last_modified: when.to_string(), status: String::new(), status_class: String::new() }
    }

    #[test]
    fn the_newest_edits_come_first_and_only_six_are_shown() {
        let rows: Vec<RecentProject> = (1..=9).map(|d| row(&format!("s{d}"), &format!("2026-10-0{d}T10:00:00Z"))).collect();
        let shown = most_recent(rows);
        assert_eq!(shown.len(), RECENT);
        assert_eq!(shown[0].id, "s9");
        assert_eq!(shown[5].id, "s4");
    }

    #[test]
    fn an_edit_time_reads_as_its_day_and_other_text_is_left_alone() {
        assert_eq!(day_of("2026-10-01T22:01:09.945Z"), "2026-10-01");
        assert_eq!(day_of("2 hours ago"), "2 hours ago");
        assert_eq!(day_of(""), "");
    }
}
