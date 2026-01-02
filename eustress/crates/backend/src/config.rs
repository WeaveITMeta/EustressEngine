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
    
    /// JWT secret for signing tokens
    pub jwt_secret: String,
    
    /// JWT token expiry in hours
    pub jwt_expiry_hours: i64,
    
    /// Frontend URL for redirects
    pub frontend_url: String,
    
    /// Steam API Key (from https://steamcommunity.com/dev/apikey)
    pub steam_api_key: String,
    
    /// Steam OpenID realm (your domain)
    pub steam_realm: String,
    
    /// Steam OpenID return URL
    pub steam_return_url: String,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7000".into()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:eustress.db".into()),
            jwt_secret: env::var("JWT_SECRET").map_err(|_| ConfigError::Missing("JWT_SECRET"))?,
            jwt_expiry_hours: env::var("JWT_EXPIRY_HOURS")
                .unwrap_or_else(|_| "24".into())
                .parse()
                .unwrap_or(24),
            frontend_url: env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
            steam_api_key: env::var("STEAM_API_KEY").unwrap_or_default(),
            steam_realm: env::var("STEAM_REALM").unwrap_or_else(|_| "http://localhost:7000".into()),
            steam_return_url: env::var("STEAM_RETURN_URL")
                .unwrap_or_else(|_| "http://localhost:7000/api/auth/steam/callback".into()),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    Missing(&'static str),
}
