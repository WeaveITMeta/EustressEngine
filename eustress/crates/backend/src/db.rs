// =============================================================================
// Eustress Backend - Database Layer
// =============================================================================
// Content database for simulations, marketplace, projects, and favorites.
// User identity is managed by Cloudflare KV (api.eustress.dev) — NOT here.
// This DB stores content metadata that references user IDs from Cloudflare.
// =============================================================================

use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Database connection pool wrapper.
#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

// -----------------------------------------------------------------------------
// Models
// -----------------------------------------------------------------------------

/// Simulation model (published simulations).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Simulation {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub is_public: bool,
    pub allow_copying: bool,
    pub author_id: String,
    pub author_name: String,
    pub version: i32,
    pub play_count: i64,
    pub favorite_count: i64,
    pub created_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Simulation version history.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SimulationVersion {
    pub id: String,
    pub simulation_id: String,
    pub version: i32,
    pub changelog: Option<String>,
    pub published_at: DateTime<Utc>,
}

/// User favorite.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Favorite {
    pub id: String,
    pub user_id: String,
    pub simulation_id: String,
    pub created_at: DateTime<Utc>,
}

/// Marketplace item model.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct MarketplaceItem {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub price: i64,
    pub author_id: String,
    pub is_active: bool,
    pub purchase_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Purchase record model.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Purchase {
    pub id: String,
    pub user_id: String,
    pub item_id: String,
    pub price_paid: i64,
    pub purchased_at: DateTime<Utc>,
}

/// Project model.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub is_published: bool,
    pub simulation_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -----------------------------------------------------------------------------
// Database
// -----------------------------------------------------------------------------

impl Database {
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let url_with_options = if url.starts_with("sqlite:") && !url.contains("?") {
            format!("{}?mode=rwc", url)
        } else if url.starts_with("sqlite:") && !url.contains("mode=") {
            format!("{}&mode=rwc", url)
        } else {
            url.to_string()
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url_with_options)
            .await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS simulations (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                genre TEXT NOT NULL DEFAULT 'all_genres',
                max_players INTEGER NOT NULL DEFAULT 10,
                is_public INTEGER NOT NULL DEFAULT 1,
                allow_copying INTEGER NOT NULL DEFAULT 0,
                author_id TEXT NOT NULL,
                author_name TEXT NOT NULL DEFAULT '',
                version INTEGER NOT NULL DEFAULT 1,
                play_count INTEGER NOT NULL DEFAULT 0,
                favorite_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                published_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS simulation_versions (
                id TEXT PRIMARY KEY,
                simulation_id TEXT NOT NULL REFERENCES simulations(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                changelog TEXT,
                published_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(simulation_id, version)
            )"#,
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS favorites (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                simulation_id TEXT NOT NULL REFERENCES simulations(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, simulation_id)
            )"#,
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS marketplace_items (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                category TEXT NOT NULL DEFAULT 'assets',
                price INTEGER NOT NULL DEFAULT 0,
                author_id TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                purchase_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS purchases (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                item_id TEXT NOT NULL REFERENCES marketplace_items(id) ON DELETE CASCADE,
                price_paid INTEGER NOT NULL DEFAULT 0,
                purchased_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, item_id)
            )"#,
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                owner_id TEXT NOT NULL,
                is_published INTEGER NOT NULL DEFAULT 0,
                simulation_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )"#,
        ).execute(&self.pool).await?;

        // Indexes
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_simulations_author ON simulations(author_id)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_simulations_updated ON simulations(updated_at)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_favorites_user ON favorites(user_id)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_favorites_simulation ON favorites(simulation_id)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_marketplace_category ON marketplace_items(category)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_purchases_user ON purchases(user_id)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_projects_owner ON projects(owner_id)").execute(&self.pool).await;

        tracing::info!("Database migrations complete");
        Ok(())
    }

    // ── Simulations ──

    pub async fn get_simulations(&self, limit: i32) -> Result<Vec<Simulation>, sqlx::Error> {
        sqlx::query_as::<_, Simulation>(
            "SELECT * FROM simulations WHERE is_public = 1 ORDER BY updated_at DESC LIMIT ?"
        ).bind(limit).fetch_all(&self.pool).await
    }

    pub async fn get_simulation_by_id(&self, id: &str) -> Result<Option<Simulation>, sqlx::Error> {
        sqlx::query_as::<_, Simulation>("SELECT * FROM simulations WHERE id = ?")
            .bind(id).fetch_optional(&self.pool).await
    }

    pub async fn get_user_simulations(&self, user_id: &str) -> Result<Vec<Simulation>, sqlx::Error> {
        sqlx::query_as::<_, Simulation>(
            "SELECT * FROM simulations WHERE author_id = ? ORDER BY updated_at DESC"
        ).bind(user_id).fetch_all(&self.pool).await
    }

    // ── Favorites ──

    pub async fn get_user_favorites(&self, user_id: &str) -> Result<Vec<Favorite>, sqlx::Error> {
        sqlx::query_as::<_, Favorite>(
            "SELECT * FROM favorites WHERE user_id = ? ORDER BY created_at DESC"
        ).bind(user_id).fetch_all(&self.pool).await
    }

    pub async fn add_favorite(&self, id: &str, user_id: &str, simulation_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO favorites (id, user_id, simulation_id) VALUES (?, ?, ?)")
            .bind(id).bind(user_id).bind(simulation_id)
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn remove_favorite(&self, user_id: &str, simulation_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM favorites WHERE user_id = ? AND simulation_id = ?")
            .bind(user_id).bind(simulation_id)
            .execute(&self.pool).await?;
        Ok(())
    }

    // ── Projects ──

    pub async fn get_user_projects(&self, user_id: &str) -> Result<Vec<Project>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE owner_id = ? ORDER BY updated_at DESC"
        ).bind(user_id).fetch_all(&self.pool).await
    }

    pub async fn get_project_by_id(&self, id: &str) -> Result<Option<Project>, sqlx::Error> {
        sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?")
            .bind(id).fetch_optional(&self.pool).await
    }

    // ── Marketplace ──

    pub async fn get_marketplace_items(&self, limit: i32) -> Result<Vec<MarketplaceItem>, sqlx::Error> {
        sqlx::query_as::<_, MarketplaceItem>(
            "SELECT * FROM marketplace_items WHERE is_active = 1 ORDER BY purchase_count DESC LIMIT ?"
        ).bind(limit).fetch_all(&self.pool).await
    }

    pub async fn get_marketplace_item_by_id(&self, id: &str) -> Result<Option<MarketplaceItem>, sqlx::Error> {
        sqlx::query_as::<_, MarketplaceItem>("SELECT * FROM marketplace_items WHERE id = ?")
            .bind(id).fetch_optional(&self.pool).await
    }
}
