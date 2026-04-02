// =============================================================================
// Eustress Backend - Configuration
// =============================================================================

use std::env;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Server bind address (e.g., "127.0.0.1:7000")
    pub bind_address: String,

    /// Database URL (SQLite path)
    pub database_url: String,

    /// JWT secret — must match the Cloudflare Worker's JWT_SECRET
    /// so this server can validate tokens issued by api.eustress.dev
    pub jwt_secret: String,

    /// JWT token expiry in hours (for validation window)
    pub jwt_expiry_hours: i64,

    /// Frontend URL for CORS
    pub frontend_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7000".into()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:eustress.db".into()),
            jwt_secret: env::var("JWT_SECRET").map_err(|_| ConfigError::Missing("JWT_SECRET"))?,
            jwt_expiry_hours: env::var("JWT_EXPIRY_HOURS")
                .unwrap_or_else(|_| "72".into())
                .parse()
                .unwrap_or(72),
            frontend_url: env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    Missing(&'static str),
}
