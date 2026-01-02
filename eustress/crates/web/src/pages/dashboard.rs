// =============================================================================
// Eustress Web - Dashboard Page (Industrial Design)
// =============================================================================
// User dashboard with quick actions, stats, and recent activity
// Recent projects populated from Cloudflare R2 Experience Bucket
// =============================================================================

use leptos::prelude::*;
use crate::state::{AppState, AuthState};
use crate::components::{CentralNav, Footer};

/// Recent project from R2 bucket
#[derive(Clone, Debug, PartialEq)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub last_modified: String,
    pub status: String, // "published", "draft", "edited"
}

/// User dashboard page.
#[component]
pub fn DashboardPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;
    
    // Recent projects from R2 (would be fetched from API)
    let recent_projects = RwSignal::new(Vec::<RecentProject>::new());
    let is_loading = RwSignal::new(true);
    
    // Fetch recent projects from R2 on mount
    Effect::new(move |_| {
        // TODO: Fetch from Cloudflare R2 API
        // GET /api/projects/recent - returns projects from R2 bucket
        // For now, simulate empty state or mock data
        is_loading.set(false);
        
        // Mock data for development - remove when API is ready
        // recent_projects.set(vec![
        //     RecentProject {
        //         id: "proj_123".to_string(),
        //         name: "My First Game".to_string(),
        //         thumbnail_url: None,
        //         last_modified: "2 hours ago".to_string(),
        //         status: "edited".to_string(),
        //     },
        // ]);
    });
    
    let username = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.username,
            _ => "User".to_string(),
        }
    };
    
    let bliss_balance = move || {
        match auth.get() {
            AuthState::Authenticated(user) => user.bliss_balance,
            _ => 0,
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
                                <p>"Discover amazing experiences"</p>
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
                
                // Recent Projects (from Cloudflare R2)
                <section class="dashboard-section">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/clock.svg" alt="Recent" class="section-icon" />
                        <h2>"Recent Projects"</h2>
                    </div>
                    
                    <div class="recent-projects-card">
                        <Show
                            when=move || is_loading.get()
                            fallback=move || {
                                let projects = recent_projects.get();
                                if projects.is_empty() {
                                    view! {
                                        <div class="empty-state">
                                            <img src="/assets/icons/folder.svg" alt="No projects" class="empty-icon" />
                                            <h3>"No projects yet"</h3>
                                            <p>"Create your first project to get started!"</p>
                                            <button 
                                                class="btn-primary-steel"
                                                on:click=move |_| {
                                                    // Launch Eustress Engine with new project
                                                    // Uses eustress:// protocol handler
                                                    let _ = window().location().set_href("eustress://new-project");
                                                }
                                            >
                                                "Create Project"
                                            </button>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div class="recent-projects-grid">
                                            <For
                                                each=move || recent_projects.get()
                                                key=|project| project.id.clone()
                                                children=move |project| {
                                                    let project_id = project.id.clone();
                                                    let project_name = project.name.clone();
                                                    let last_modified = project.last_modified.clone();
                                                    let status = project.status.clone();
                                                    let status_class = format!("project-status status-{}", status);
                                                    let thumbnail = project.thumbnail_url.clone();
                                                    
                                                    view! {
                                                        <div class="recent-project-card">
                                                            <div class="project-thumbnail">
                                                                {match thumbnail {
                                                                    Some(url) => view! { <img src=url alt="Thumbnail" /> }.into_any(),
                                                                    None => view! { <div class="placeholder-thumb"><img src="/assets/icons/cube.svg" alt="Project" /></div> }.into_any(),
                                                                }}
                                                            </div>
                                                            <div class="project-info">
                                                                <h4>{project_name}</h4>
                                                                <span class="project-modified">{last_modified}</span>
                                                                <span class=status_class>{status}</span>
                                                            </div>
                                                            <button 
                                                                class="btn-open-project"
                                                                on:click=move |_| {
                                                                    // Open project in Eustress Engine
                                                                    let url = format!("eustress://open-project/{}", project_id);
                                                                    let _ = window().location().set_href(&url);
                                                                }
                                                            >
                                                                "Open"
                                                            </button>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    }.into_any()
                                }
                            }
                        >
                            <div class="loading-state">
                                <div class="spinner"></div>
                                <p>"Loading recent projects..."</p>
                            </div>
                        </Show>
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
