// =============================================================================
// Eustress Backend - API Server Entry Point
// =============================================================================
// Content API for simulations, marketplace, projects, gallery, community.
// Auth is handled by Cloudflare Worker at api.eustress.dev — this server
// validates JWTs from that Worker using a shared secret.
// =============================================================================

mod auth;
mod community;
mod config;
mod db;
mod error;
mod experiences;
mod gallery;
mod marketplace;
mod projects;

use axum::{
    routing::{get, post, put, delete},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Database;

// -----------------------------------------------------------------------------
// Application State
// -----------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_filename("crates/backend/.env");
    }

    let config = Config::from_env()?;
    let bind_addr = config.bind_address.clone();

    if config.database_url.starts_with("sqlite:") {
        let db_path = config.database_url.trim_start_matches("sqlite:");
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
    }

    let db = Database::new(&config.database_url).await?;
    db.run_migrations().await?;

    let state = AppState {
        config: Arc::new(config),
        db,
    };

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("Eustress Content API running on http://{}", bind_addr);
    tracing::info!("Auth: validated via JWT from api.eustress.dev (Cloudflare)");

    axum::serve(listener, app).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Router
// -----------------------------------------------------------------------------

fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health
        .route("/health", get(|| async { "OK" }))
        // Simulations API (renamed from experiences)
        .route("/api/simulations/sync", post(experiences::sync_experience))
        .route("/api/simulations/mine", get(experiences::get_my_experiences))
        .route("/api/simulations/:id", get(experiences::get_experience))
        // Favorites
        .route("/api/favorites", get(experiences::get_favorites))
        .route("/api/favorites/:simulation_id", post(experiences::add_favorite))
        .route("/api/favorites/:simulation_id", delete(experiences::remove_favorite))
        .route("/api/favorites/updates", get(experiences::get_favorite_updates))
        // Gallery (public)
        .route("/api/gallery", get(gallery::get_gallery))
        .route("/api/gallery/featured", get(gallery::get_featured))
        .route("/api/gallery/:id", get(gallery::get_experience_details))
        // Community (public)
        .route("/api/community/search", get(community::search_users))
        .route("/api/community/leaderboard", get(community::get_leaderboard))
        .route("/api/community/stats", get(community::get_community_stats))
        .route("/api/community/users/:username", get(community::get_user_profile))
        // Marketplace
        .route("/api/marketplace", get(marketplace::get_marketplace))
        .route("/api/marketplace/featured", get(marketplace::get_featured_items))
        .route("/api/marketplace/purchased", get(marketplace::get_purchased_items))
        .route("/api/marketplace/purchase", post(marketplace::purchase_item))
        .route("/api/marketplace/:id", get(marketplace::get_marketplace_item))
        // Projects (authenticated)
        .route("/api/projects", get(projects::get_projects))
        .route("/api/projects", post(projects::create_project))
        .route("/api/projects/recent", get(projects::get_recent_projects))
        .route("/api/projects/:id", get(projects::get_project))
        .route("/api/projects/:id", put(projects::update_project))
        .route("/api/projects/:id", delete(projects::delete_project))
        .route("/api/projects/:id/publish", post(projects::publish_project))
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}
