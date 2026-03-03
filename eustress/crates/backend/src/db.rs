// =============================================================================
// Eustress Backend - Database Layer
// =============================================================================

use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Database connection pool wrapper.
#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

/// User model.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub steam_id: Option<String>,
    pub discord_id: Option<String>,
    pub avatar_url: Option<String>,
    pub bliss_balance: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Experience model (published experiences).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Experience {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: i32,
    pub is_public: bool,
    pub allow_copying: bool,
    pub author_id: String,
    pub version: i32,
    pub play_count: i64,
    pub favorite_count: i64,
    pub created_at: DateTime<Utc>,
    pub published_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Experience version history.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExperienceVersion {
    pub id: String,
    pub experience_id: String,
    pub version: i32,
    pub changelog: Option<String>,
    pub published_at: DateTime<Utc>,
}

/// User favorite (for notifications).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Favorite {
    pub id: String,
    pub user_id: String,
    pub experience_id: String,
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

/// Project model (user's local projects synced to backend).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub is_published: bool,
    pub experience_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// User response (without sensitive fields).
#[derive(Debug, Clone, Serialize)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub steam_id: Option<String>,
    pub discord_id: Option<String>,
    pub avatar_url: Option<String>,
    pub bliss_balance: i64,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            steam_id: user.steam_id,
            discord_id: user.discord_id,
            avatar_url: user.avatar_url,
            bliss_balance: user.bliss_balance,
            created_at: user.created_at,
        }
    }
}

impl Database {
    /// Create a new database connection pool.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        // Add create_if_missing option for SQLite
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
    
    /// Get a reference to the connection pool.
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
    
    /// Run database migrations.
    pub async fn run_migrations(&self) -> Result<(), sqlx::Error> {
        // Users table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                email TEXT UNIQUE,
                password_hash TEXT,
                steam_id TEXT UNIQUE,
                discord_id TEXT UNIQUE,
                avatar_url TEXT,
                bliss_balance INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Add discord_id column if it doesn't exist (migration for existing DBs)
        let _ = sqlx::query("ALTER TABLE users ADD COLUMN discord_id TEXT UNIQUE")
            .execute(&self.pool)
            .await;
        
        // Experiences table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS experiences (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                genre TEXT NOT NULL DEFAULT 'all_genres',
                max_players INTEGER NOT NULL DEFAULT 10,
                is_public INTEGER NOT NULL DEFAULT 1,
                allow_copying INTEGER NOT NULL DEFAULT 0,
                author_id TEXT NOT NULL REFERENCES users(id),
                version INTEGER NOT NULL DEFAULT 1,
                play_count INTEGER NOT NULL DEFAULT 0,
                favorite_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                published_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Experience versions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS experience_versions (
                id TEXT PRIMARY KEY,
                experience_id TEXT NOT NULL REFERENCES experiences(id) ON DELETE CASCADE,
                version INTEGER NOT NULL,
                changelog TEXT,
                published_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(experience_id, version)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Favorites table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS favorites (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                experience_id TEXT NOT NULL REFERENCES experiences(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, experience_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Create indexes for performance
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_experiences_author ON experiences(author_id)")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_experiences_updated ON experiences(updated_at)")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_favorites_user ON favorites(user_id)")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_favorites_experience ON favorites(experience_id)")
            .execute(&self.pool)
            .await;
        
        // Marketplace items table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketplace_items (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                category TEXT NOT NULL DEFAULT 'assets',
                price INTEGER NOT NULL DEFAULT 0,
                author_id TEXT NOT NULL REFERENCES users(id),
                is_active INTEGER NOT NULL DEFAULT 1,
                purchase_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Purchases table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS purchases (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                item_id TEXT NOT NULL REFERENCES marketplace_items(id) ON DELETE CASCADE,
                price_paid INTEGER NOT NULL DEFAULT 0,
                purchased_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(user_id, item_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Projects table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                is_published INTEGER NOT NULL DEFAULT 0,
                experience_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // Additional indexes
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_marketplace_category ON marketplace_items(category)")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_purchases_user ON purchases(user_id)")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_projects_owner ON projects(owner_id)")
            .execute(&self.pool)
            .await;
        
        tracing::info!("Database migrations complete");
        Ok(())
    }
    
    /// Find user by ID.
    pub async fn find_user_by_id(&self, id: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Find user by email.
    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ?")
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Find user by Steam ID.
    pub async fn find_user_by_steam_id(&self, steam_id: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE steam_id = ?")
            .bind(steam_id)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Create a new user.
    pub async fn create_user(
        &self,
        id: &str,
        username: &str,
        email: Option<&str>,
        password_hash: Option<&str>,
        steam_id: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<User, sqlx::Error> {
        let now = Utc::now();
        
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, steam_id, avatar_url, bliss_balance, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)
            "#,
        )
        .bind(id)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .bind(steam_id)
        .bind(avatar_url)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        self.find_user_by_id(id)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)
    }
    
    /// Update user's Steam info.
    pub async fn update_user_steam(
        &self,
        id: &str,
        steam_id: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE users 
            SET steam_id = ?, avatar_url = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(steam_id)
        .bind(avatar_url)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Update user's email and password.
    pub async fn update_user_email_password(
        &self,
        id: &str,
        email: &str,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE users 
            SET email = ?, password_hash = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(email)
        .bind(password_hash)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Update user's Discord info.
    pub async fn update_user_discord(
        &self,
        id: &str,
        discord_id: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE users 
            SET discord_id = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(discord_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Find user by Discord ID.
    pub async fn find_user_by_discord_id(&self, discord_id: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE discord_id = ?")
            .bind(discord_id)
            .fetch_optional(&self.pool)
            .await
    }
    
    // =========================================================================
    // Experience Methods
    // =========================================================================
    
    /// Create or update an experience.
    pub async fn upsert_experience(&self, experience: &Experience) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO experiences (id, name, description, genre, max_players, is_public, allow_copying, author_id, version, play_count, favorite_count, created_at, published_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                genre = excluded.genre,
                max_players = excluded.max_players,
                is_public = excluded.is_public,
                allow_copying = excluded.allow_copying,
                version = excluded.version,
                published_at = excluded.published_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&experience.id)
        .bind(&experience.name)
        .bind(&experience.description)
        .bind(&experience.genre)
        .bind(experience.max_players)
        .bind(experience.is_public)
        .bind(experience.allow_copying)
        .bind(&experience.author_id)
        .bind(experience.version)
        .bind(experience.play_count)
        .bind(experience.favorite_count)
        .bind(experience.created_at.to_rfc3339())
        .bind(experience.published_at.to_rfc3339())
        .bind(experience.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Find experience by ID.
    pub async fn find_experience_by_id(&self, id: &str) -> Result<Option<Experience>, sqlx::Error> {
        sqlx::query_as::<_, Experience>("SELECT * FROM experiences WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Get experiences by author.
    pub async fn get_experiences_by_author(&self, author_id: &str) -> Result<Vec<Experience>, sqlx::Error> {
        sqlx::query_as::<_, Experience>(
            "SELECT * FROM experiences WHERE author_id = ? ORDER BY updated_at DESC"
        )
        .bind(author_id)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Record experience version.
    pub async fn record_experience_version(
        &self,
        experience_id: &str,
        version: i32,
        changelog: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO experience_versions (id, experience_id, version, changelog, published_at)
            VALUES (?, ?, ?, ?, datetime('now'))
            "#,
        )
        .bind(&id)
        .bind(experience_id)
        .bind(version)
        .bind(changelog)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    // =========================================================================
    // Favorites Methods
    // =========================================================================
    
    /// Add experience to favorites.
    pub async fn add_favorite(&self, user_id: &str, experience_id: &str) -> Result<(), sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO favorites (id, user_id, experience_id, created_at)
            VALUES (?, ?, ?, datetime('now'))
            "#,
        )
        .bind(&id)
        .bind(user_id)
        .bind(experience_id)
        .execute(&self.pool)
        .await?;
        
        // Update favorite count
        sqlx::query(
            "UPDATE experiences SET favorite_count = favorite_count + 1 WHERE id = ?"
        )
        .bind(experience_id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    /// Remove experience from favorites.
    pub async fn remove_favorite(&self, user_id: &str, experience_id: &str) -> Result<(), sqlx::Error> {
        let result = sqlx::query(
            "DELETE FROM favorites WHERE user_id = ? AND experience_id = ?"
        )
        .bind(user_id)
        .bind(experience_id)
        .execute(&self.pool)
        .await?;
        
        if result.rows_affected() > 0 {
            sqlx::query(
                "UPDATE experiences SET favorite_count = MAX(0, favorite_count - 1) WHERE id = ?"
            )
            .bind(experience_id)
            .execute(&self.pool)
            .await?;
        }
        
        Ok(())
    }
    
    /// Get user's favorites.
    pub async fn get_user_favorites(&self, user_id: &str) -> Result<Vec<Favorite>, sqlx::Error> {
        sqlx::query_as::<_, Favorite>(
            "SELECT * FROM favorites WHERE user_id = ? ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }
    
    // =========================================================================
    // Community Methods
    // =========================================================================
    
    /// Search users by username (case-insensitive partial match).
    pub async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>, sqlx::Error> {
        let pattern = format!("%{}%", query);
        sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE username LIKE ? ORDER BY username ASC LIMIT ? OFFSET ?"
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Find user by username (exact match).
    pub async fn find_user_by_username(&self, username: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Get users ordered by bliss balance (leaderboard).
    pub async fn get_leaderboard(&self, limit: i64, offset: i64) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as::<_, User>(
            "SELECT * FROM users ORDER BY bliss_balance DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Get total user count.
    pub async fn get_user_count(&self) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
    
    /// Get total experience count.
    pub async fn get_experience_count(&self) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM experiences")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
    
    /// Get total play count across all experiences.
    pub async fn get_total_play_count(&self) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COALESCE(SUM(play_count), 0) FROM experiences")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
    
    /// Get experience count for a specific user.
    pub async fn get_user_experience_count(&self, user_id: &str) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM experiences WHERE author_id = ?")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
    
    /// Get total play count for a specific user's experiences.
    pub async fn get_user_total_plays(&self, user_id: &str) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(play_count), 0) FROM experiences WHERE author_id = ?"
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }
    
    // =========================================================================
    // Gallery Methods
    // =========================================================================
    
    /// Get public experiences with optional genre filter, sorted by updated_at descending.
    pub async fn get_public_experiences(
        &self,
        genre: Option<&str>,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Experience>, sqlx::Error> {
        match (genre, search) {
            (Some(g), Some(s)) => {
                let pattern = format!("%{}%", s);
                sqlx::query_as::<_, Experience>(
                    r#"SELECT * FROM experiences 
                       WHERE is_public = 1 AND genre = ? AND name LIKE ?
                       ORDER BY updated_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(g)
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (Some(g), None) => {
                sqlx::query_as::<_, Experience>(
                    r#"SELECT * FROM experiences 
                       WHERE is_public = 1 AND genre = ?
                       ORDER BY updated_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(g)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (None, Some(s)) => {
                let pattern = format!("%{}%", s);
                sqlx::query_as::<_, Experience>(
                    r#"SELECT * FROM experiences 
                       WHERE is_public = 1 AND name LIKE ?
                       ORDER BY updated_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (None, None) => {
                sqlx::query_as::<_, Experience>(
                    r#"SELECT * FROM experiences 
                       WHERE is_public = 1
                       ORDER BY updated_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
        }
    }
    
    /// Get featured experiences (highest play_count + favorite_count).
    pub async fn get_featured_experiences(&self, limit: i64) -> Result<Vec<Experience>, sqlx::Error> {
        sqlx::query_as::<_, Experience>(
            r#"SELECT * FROM experiences 
               WHERE is_public = 1
               ORDER BY (play_count + favorite_count) DESC
               LIMIT ?"#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Get the author username for an experience.
    pub async fn get_experience_author_name(&self, author_id: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT username FROM users WHERE id = ?"
        )
        .bind(author_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }
    
    // =========================================================================
    // Marketplace Methods
    // =========================================================================
    
    /// Get marketplace items with optional category filter.
    pub async fn get_marketplace_items(
        &self,
        category: Option<&str>,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<MarketplaceItem>, sqlx::Error> {
        match (category, search) {
            (Some(c), Some(s)) => {
                let pattern = format!("%{}%", s);
                sqlx::query_as::<_, MarketplaceItem>(
                    r#"SELECT * FROM marketplace_items 
                       WHERE is_active = 1 AND category = ? AND name LIKE ?
                       ORDER BY created_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(c)
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (Some(c), None) => {
                sqlx::query_as::<_, MarketplaceItem>(
                    r#"SELECT * FROM marketplace_items 
                       WHERE is_active = 1 AND category = ?
                       ORDER BY created_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(c)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (None, Some(s)) => {
                let pattern = format!("%{}%", s);
                sqlx::query_as::<_, MarketplaceItem>(
                    r#"SELECT * FROM marketplace_items 
                       WHERE is_active = 1 AND name LIKE ?
                       ORDER BY created_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(&pattern)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
            (None, None) => {
                sqlx::query_as::<_, MarketplaceItem>(
                    r#"SELECT * FROM marketplace_items 
                       WHERE is_active = 1
                       ORDER BY created_at DESC LIMIT ? OFFSET ?"#
                )
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
            }
        }
    }
    
    /// Get featured marketplace items (highest purchase_count).
    pub async fn get_featured_marketplace_items(&self, limit: i64) -> Result<Vec<MarketplaceItem>, sqlx::Error> {
        sqlx::query_as::<_, MarketplaceItem>(
            r#"SELECT * FROM marketplace_items 
               WHERE is_active = 1
               ORDER BY purchase_count DESC
               LIMIT ?"#
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Find marketplace item by ID.
    pub async fn find_marketplace_item_by_id(&self, id: &str) -> Result<Option<MarketplaceItem>, sqlx::Error> {
        sqlx::query_as::<_, MarketplaceItem>("SELECT * FROM marketplace_items WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Get items purchased by a user.
    pub async fn get_user_purchases(&self, user_id: &str) -> Result<Vec<Purchase>, sqlx::Error> {
        sqlx::query_as::<_, Purchase>(
            "SELECT * FROM purchases WHERE user_id = ? ORDER BY purchased_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Check if user has already purchased an item.
    pub async fn has_purchased(&self, user_id: &str, item_id: &str) -> Result<bool, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM purchases WHERE user_id = ? AND item_id = ?"
        )
        .bind(user_id)
        .bind(item_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0 > 0)
    }
    
    /// Record a purchase and deduct bliss balance (atomic transaction).
    pub async fn purchase_item(&self, user_id: &str, item_id: &str, price: i64) -> Result<(), sqlx::Error> {
        let purchase_id = uuid::Uuid::new_v4().to_string();
        let mut transaction = self.pool.begin().await?;
        
        // Deduct bliss balance
        sqlx::query("UPDATE users SET bliss_balance = bliss_balance - ?, updated_at = datetime('now') WHERE id = ? AND bliss_balance >= ?")
            .bind(price)
            .bind(user_id)
            .bind(price)
            .execute(&mut *transaction)
            .await?;
        
        // Record purchase
        sqlx::query(
            r#"INSERT INTO purchases (id, user_id, item_id, price_paid, purchased_at)
               VALUES (?, ?, ?, ?, datetime('now'))"#
        )
        .bind(&purchase_id)
        .bind(user_id)
        .bind(item_id)
        .bind(price)
        .execute(&mut *transaction)
        .await?;
        
        // Increment purchase count on item
        sqlx::query("UPDATE marketplace_items SET purchase_count = purchase_count + 1 WHERE id = ?")
            .bind(item_id)
            .execute(&mut *transaction)
            .await?;
        
        transaction.commit().await?;
        Ok(())
    }
    
    /// Get user's current bliss balance.
    pub async fn get_user_balance(&self, user_id: &str) -> Result<i64, sqlx::Error> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT bliss_balance FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.0).unwrap_or(0))
    }
    
    // =========================================================================
    // Projects Methods
    // =========================================================================
    
    /// Get all projects owned by a user.
    pub async fn get_user_projects(&self, owner_id: &str) -> Result<Vec<Project>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE owner_id = ? ORDER BY updated_at DESC"
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Get recent projects for a user (last 10 by updated_at).
    pub async fn get_recent_projects(&self, owner_id: &str, limit: i64) -> Result<Vec<Project>, sqlx::Error> {
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE owner_id = ? ORDER BY updated_at DESC LIMIT ?"
        )
        .bind(owner_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }
    
    /// Find project by ID.
    pub async fn find_project_by_id(&self, id: &str) -> Result<Option<Project>, sqlx::Error> {
        sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
    
    /// Create a new project.
    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        owner_id: &str,
    ) -> Result<Project, sqlx::Error> {
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO projects (id, name, description, owner_id, is_published, created_at, updated_at)
               VALUES (?, ?, ?, ?, 0, ?, ?)"#
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(owner_id)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        self.find_project_by_id(id)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)
    }
    
    /// Update a project's name and description.
    pub async fn update_project(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE projects SET name = ?, description = ?, updated_at = datetime('now') WHERE id = ?"#
        )
        .bind(name)
        .bind(description)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    
    /// Delete a project by ID.
    pub async fn delete_project(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    
    /// Mark a project as published and link to an experience.
    pub async fn publish_project(&self, id: &str, experience_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE projects SET is_published = 1, experience_id = ?, updated_at = datetime('now') WHERE id = ?"#
        )
        .bind(experience_id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    
    /// Get updated favorites since timestamp (for notifications).
    pub async fn get_favorite_updates(
        &self,
        user_id: &str,
        since: &DateTime<Utc>,
    ) -> Result<Vec<Experience>, sqlx::Error> {
        sqlx::query_as::<_, Experience>(
            r#"
            SELECT e.* FROM experiences e
            INNER JOIN favorites f ON e.id = f.experience_id
            WHERE f.user_id = ? AND e.updated_at > ?
            ORDER BY e.updated_at DESC
            "#,
        )
        .bind(user_id)
        .bind(since.to_rfc3339())
        .fetch_all(&self.pool)
        .await
    }
}
