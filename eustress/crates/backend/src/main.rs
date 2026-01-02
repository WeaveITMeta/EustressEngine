// =============================================================================
// Eustress Backend - API Server Entry Point
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. Application State
// 3. Main Entry Point
// 4. Router Setup
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
mod steam;

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
// 2. Application State
// -----------------------------------------------------------------------------

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Database,
}

// -----------------------------------------------------------------------------
// 3. Main Entry Point
// -----------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load environment variables from multiple possible locations
    // Try current directory first, then crates/backend/
    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_filename("crates/backend/.env");
    }

    // Load configuration
    let config = Config::from_env()?;
    let bind_addr = config.bind_address.clone();

    // Ensure database directory exists for SQLite
    if config.database_url.starts_with("sqlite:") {
        let db_path = config.database_url.trim_start_matches("sqlite:");
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
    }

    // Initialize database
    let db = Database::new(&config.database_url).await?;
    db.run_migrations().await?;

    // Create app state
    let state = AppState {
        config: Arc::new(config),
        db,
    };

    // Build router
    let app = create_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("🚀 Eustress API Server running on http://{}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

// -----------------------------------------------------------------------------
// 4. Router Setup
// -----------------------------------------------------------------------------

fn create_router(state: AppState) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Health check
        .route("/health", get(|| async { "OK" }))
        // Auth routes
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::get_current_user))
        .route("/api/auth/refresh", post(auth::refresh_token))
        .route("/api/auth/add-email", post(auth::add_email_password))
        // Steam OAuth
        .route("/api/auth/steam", get(steam::steam_login_redirect))
        .route("/api/auth/steam/callback", get(steam::steam_callback))
        .route("/api/auth/steam/link", get(steam::steam_link_redirect))
        // Studio SSO
        .route("/api/auth/studio", get(auth::studio_login_page))
        // Experiences API
        .route("/api/experiences/sync", post(experiences::sync_experience))
        .route("/api/experiences/mine", get(experiences::get_my_experiences))
        .route("/api/experiences/:id", get(experiences::get_experience))
        // Favorites API
        .route("/api/favorites", get(experiences::get_favorites))
        .route("/api/favorites/:experience_id", post(experiences::add_favorite))
        .route("/api/favorites/:experience_id", delete(experiences::remove_favorite))
        .route("/api/favorites/updates", get(experiences::get_favorite_updates))
        // Gallery API (public)
        .route("/api/gallery", get(gallery::get_gallery))
        .route("/api/gallery/featured", get(gallery::get_featured))
        .route("/api/gallery/:id", get(gallery::get_experience_details))
        // Community API (public)
        .route("/api/community/search", get(community::search_users))
        .route("/api/community/leaderboard", get(community::get_leaderboard))
        .route("/api/community/stats", get(community::get_community_stats))
        .route("/api/community/users/:username", get(community::get_user_profile))
        // Marketplace API
        .route("/api/marketplace", get(marketplace::get_marketplace))
        .route("/api/marketplace/featured", get(marketplace::get_featured_items))
        .route("/api/marketplace/purchased", get(marketplace::get_purchased_items))
        .route("/api/marketplace/purchase", post(marketplace::purchase_item))
        .route("/api/marketplace/:id", get(marketplace::get_marketplace_item))
        // Projects API (authenticated)
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
