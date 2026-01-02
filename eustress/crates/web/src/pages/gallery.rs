// =============================================================================
// Eustress Web - Gallery Page (Industrial Redesign)
// =============================================================================
// Browse and search published experiences
// =============================================================================

use leptos::prelude::*;
use crate::api::{self, ApiClient, GalleryExperience, GalleryQuery};
use crate::components::{CentralNav, Footer};
use crate::state::AppState;

/// Experience type alias for gallery display.
pub type Experience = GalleryExperience;

/// Gallery page - browse experiences.
#[component]
pub fn GalleryPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_client = ApiClient::new(&app_state.api_url);
    
    // Search and filter state
    let search_query = RwSignal::new(String::new());
    let selected_category = RwSignal::new("all".to_string());
    let sort_by = RwSignal::new("popular".to_string());
    
    // API data state
    let experiences = RwSignal::new(Vec::<Experience>::new());
    let featured_experiences = RwSignal::new(Vec::<Experience>::new());
    let is_loading = RwSignal::new(true);
    let error_message = RwSignal::new(Option::<String>::None);
    
    // Fetch gallery data on mount
    let api_url = app_state.api_url.clone();
    Effect::new(move |_| {
        let api_url = api_url.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            // Fetch featured
            match api::gallery::get_featured(&client).await {
                Ok(response) => {
                    featured_experiences.set(response.featured);
                }
                Err(e) => {
                    log::warn!("Failed to fetch featured: {:?}", e);
                }
            }
            
            // Fetch all experiences
            let query = GalleryQuery {
                sort: Some("popular".to_string()),
                limit: Some(50),
                ..Default::default()
            };
            
            match api::gallery::get_gallery(&client, &query).await {
                Ok(response) => {
                    experiences.set(response.experiences);
                    is_loading.set(false);
                }
                Err(e) => {
                    error_message.set(Some(format!("Failed to load gallery: {:?}", e)));
                    is_loading.set(false);
                }
            }
        });
    });
    
    // Re-fetch when filters change
    let api_url_search = app_state.api_url.clone();
    let do_search = move || {
        let api_url = api_url_search.clone();
        let query_text = search_query.get();
        let category = selected_category.get();
        let sort = sort_by.get();
        
        is_loading.set(true);
        
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            
            let query = GalleryQuery {
                q: if query_text.is_empty() { None } else { Some(query_text) },
                category: if category == "all" { None } else { Some(category) },
                sort: Some(sort),
                limit: Some(50),
                ..Default::default()
            };
            
            match api::gallery::get_gallery(&client, &query).await {
                Ok(response) => {
                    experiences.set(response.experiences);
                }
                Err(e) => {
                    log::warn!("Search failed: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    };
    
    // Filter experiences based on local search (for instant feedback)
    let filtered_experiences = move || {
        let query = search_query.get().to_lowercase();
        let category = selected_category.get();
        
        experiences.get()
            .into_iter()
            .filter(|exp| {
                let matches_search = query.is_empty() 
                    || exp.name.to_lowercase().contains(&query)
                    || exp.description.to_lowercase().contains(&query)
                    || exp.creator_name.to_lowercase().contains(&query)
                    || exp.tags.iter().any(|t| t.to_lowercase().contains(&query));
                
                let matches_category = category == "all" 
                    || exp.genre == category
                    || exp.tags.iter().any(|t| t == &category);
                
                matches_search && matches_category
            })
            .collect::<Vec<_>>()
    };
    
    // Featured experiences
    let featured = move || {
        featured_experiences.get()
    };
    
    view! {
        <div class="page page-gallery-industrial">
            <CentralNav active="gallery".to_string() />
            
            // Background
            <div class="gallery-bg">
                <div class="gallery-grid-overlay"></div>
                <div class="gallery-glow glow-1"></div>
                <div class="gallery-glow glow-2"></div>
            </div>
            
            // Hero section with search
            <section class="gallery-hero-industrial">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"GALLERY"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="gallery-title">"Experience Gallery"</h1>
                <p class="gallery-subtitle">"Explore experiences created by our community"</p>
                
                <div class="search-box">
                    <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <circle cx="11" cy="11" r="8"></circle>
                        <path d="m21 21-4.3-4.3"></path>
                    </svg>
                    <input 
                        type="text"
                        class="search-input-industrial"
                        placeholder="Search experiences, creators, or tags..."
                        prop:value=move || search_query.get()
                        on:input=move |e| search_query.set(event_target_value(&e))
                    />
                </div>
                
                <div class="filter-bar-industrial">
                    <div class="category-chips">
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "all"
                            on:click=move |_| selected_category.set("all".to_string())
                        >"All"</button>
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "action"
                            on:click=move |_| selected_category.set("action".to_string())
                        >"Action"</button>
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "adventure"
                            on:click=move |_| selected_category.set("adventure".to_string())
                        >"Adventure"</button>
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "simulation"
                            on:click=move |_| selected_category.set("simulation".to_string())
                        >"Simulation"</button>
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "social"
                            on:click=move |_| selected_category.set("social".to_string())
                        >"Social"</button>
                        <button 
                            class="chip"
                            class:active=move || selected_category.get() == "rpg"
                            on:click=move |_| selected_category.set("rpg".to_string())
                        >"RPG"</button>
                    </div>
                    
                    <div class="sort-dropdown">
                        <select 
                            class="sort-select-industrial"
                            prop:value=move || sort_by.get()
                            on:change=move |e| sort_by.set(event_target_value(&e))
                        >
                            <option value="popular">"Most Popular"</option>
                            <option value="recent">"Recently Added"</option>
                            <option value="trending">"Trending"</option>
                            <option value="top-rated">"Top Rated"</option>
                        </select>
                    </div>
                </div>
            </section>
            
            // Featured section
            <Show when=move || search_query.get().is_empty() && selected_category.get() == "all">
                <section class="featured-section-industrial">
                    <div class="section-header-industrial">
                        <img src="/assets/icons/star.svg" alt="Featured" class="section-icon" />
                        <h2>"Featured"</h2>
                    </div>
                    <div class="featured-grid-industrial">
                        <For
                            each=featured
                            key=|exp| exp.id.clone()
                            children=move |exp| view! { <FeaturedCard experience=exp /> }
                        />
                    </div>
                </section>
            </Show>
            
            // All experiences grid
            <section class="experiences-section-industrial">
                <div class="section-header-industrial">
                    <img src="/assets/icons/grid.svg" alt="All" class="section-icon" />
                    <h2>
                        {move || if search_query.get().is_empty() && selected_category.get() == "all" {
                            "All Experiences".to_string()
                        } else {
                            format!("Results ({})", filtered_experiences().len())
                        }}
                    </h2>
                </div>
                
                <Show
                    when=move || !filtered_experiences().is_empty()
                    fallback=|| view! {
                        <div class="no-results-industrial">
                            <svg class="no-results-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                                <circle cx="11" cy="11" r="8"></circle>
                                <path d="m21 21-4.35-4.35"></path>
                                <path d="M8 8l6 6"></path>
                                <path d="M14 8l-6 6"></path>
                            </svg>
                            <h3>"No experiences found"</h3>
                            <p>"Try adjusting your search or filters"</p>
                        </div>
                    }
                >
                    <div class="experiences-grid-industrial">
                        <For
                            each=filtered_experiences
                            key=|exp| exp.id.clone()
                            children=move |exp| view! { <ExperienceCard experience=exp /> }
                        />
                    </div>
                </Show>
            </section>
            
            <Footer />
        </div>
    }
}

/// Featured experience card (larger) - Industrial style.
#[component]
fn FeaturedCard(experience: Experience) -> impl IntoView {
    let exp_url = format!("/experience/{}", experience.id);
    let creator_url = format!("/profile/{}", experience.creator_id);
    let creator_name = experience.creator_name.clone();
    let player_count = experience.player_count;
    let rating = experience.rating;
    let tags = experience.tags.clone();
    
    view! {
        <a href=exp_url class="featured-card-industrial">
            <div class="card-thumbnail">
                <div class="thumbnail-visual">
                    <img src="/assets/icons/gamepad.svg" alt="Game" class="thumbnail-icon" />
                </div>
                <div class="featured-badge-industrial">
                    <img src="/assets/icons/star.svg" alt="Featured" class="badge-icon" />
                    "Featured"
                </div>
            </div>
            <div class="card-content">
                <h3 class="card-title">{experience.name}</h3>
                <p class="card-desc">{experience.description}</p>
                <div class="card-meta">
                    <a href=creator_url class="creator-info">
                        <img src="/assets/icons/user.svg" alt="Creator" class="creator-icon" />
                        <span>{creator_name}</span>
                    </a>
                    <div class="card-stats">
                        <span class="stat-item">
                            <img src="/assets/icons/users.svg" alt="Playing" class="stat-icon" />
                            {format_number(player_count as u64)}
                        </span>
                        <span class="stat-item">
                            <img src="/assets/icons/star.svg" alt="Rating" class="stat-icon" />
                            {format!("{:.1}", rating)}
                        </span>
                    </div>
                </div>
                <div class="card-tags">
                    <For
                        each=move || tags.clone()
                        key=|tag| tag.clone()
                        children=move |tag| view! {
                            <span class="tag-chip">{tag}</span>
                        }
                    />
                </div>
            </div>
        </a>
    }
}

/// Regular experience card - Industrial style.
#[component]
fn ExperienceCard(experience: Experience) -> impl IntoView {
    let exp_url = format!("/experience/{}", experience.id);
    let exp_url2 = exp_url.clone();
    let creator_url = format!("/profile/{}", experience.creator_id);
    let creator_name = experience.creator_name.clone();
    let player_count = experience.player_count;
    let rating = experience.rating;
    
    view! {
        <div class="exp-card-industrial">
            <a href=exp_url class="card-link">
                <div class="card-thumbnail-sm">
                    <img src="/assets/icons/gamepad.svg" alt="Game" class="thumbnail-icon" />
                    <div class="play-hover">
                        <img src="/assets/icons/play.svg" alt="Play" class="play-icon" />
                    </div>
                </div>
            </a>
            <div class="card-body">
                <a href=exp_url2 class="card-name">{experience.name}</a>
                <p class="card-description">{experience.description}</p>
                <div class="card-footer">
                    <a href=creator_url class="creator-link">
                        <img src="/assets/icons/user.svg" alt="Creator" class="creator-icon-sm" />
                        <span>{creator_name}</span>
                    </a>
                    <div class="card-stats-sm">
                        <span>
                            <img src="/assets/icons/users.svg" alt="Playing" class="stat-icon-sm" />
                            {format_number(player_count as u64)}
                        </span>
                        <span>
                            <img src="/assets/icons/star.svg" alt="Rating" class="stat-icon-sm" />
                            {format!("{:.1}", rating)}
                        </span>
                    </div>
                </div>
            </div>
        </div>
    }
}

/// Format large numbers (e.g., 125000 -> "125K").
fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
