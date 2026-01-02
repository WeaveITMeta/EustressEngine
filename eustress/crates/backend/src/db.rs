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
