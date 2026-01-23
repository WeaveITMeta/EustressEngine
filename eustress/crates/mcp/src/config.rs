//! MCP Server configuration.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{McpError, McpResult};

/// MCP Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Server bind address
    pub host: String,
    /// Server port
    pub port: u16,
    /// API keys for authentication (model_id -> api_key)
    pub api_keys: std::collections::HashMap<String, ApiKeyConfig>,
    /// CORS configuration
    pub cors: CorsConfig,
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
    /// WebSocket configuration
    pub websocket: WebSocketConfig,
    /// Forge server connection
    pub forge: ForgeConnection,
    /// Protocol version
    pub protocol_version: String,
}

impl McpConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> McpResult<Self> {
        Ok(Self {
            host: std::env::var("MCP_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("MCP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8090),
            api_keys: std::collections::HashMap::new(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            websocket: WebSocketConfig::default(),
            forge: ForgeConnection::from_env(),
            protocol_version: "eep_v1".to_string(),
        })
    }

    /// Load configuration from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> McpResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| McpError::Config(format!("Failed to read config: {}", e)))?;
        toml::from_str(&content)
            .map_err(|e| McpError::Config(format!("Failed to parse config: {}", e)))
    }

    /// Get the server address
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8090,
            api_keys: std::collections::HashMap::new(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            websocket: WebSocketConfig::default(),
            forge: ForgeConnection::default(),
            protocol_version: "eep_v1".to_string(),
        }
    }
}

/// API key configuration for a model/provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// API key value
    pub key: String,
    /// Model/provider name
    pub name: String,
    /// Allowed capabilities
    pub capabilities: Vec<String>,
    /// Rate limit override (requests per minute)
    pub rate_limit: Option<u32>,
    /// Whether this key is enabled
    pub enabled: bool,
}

/// CORS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Allowed origins
    pub allowed_origins: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Max age for preflight cache
    pub max_age_secs: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allow_credentials: false,
            max_age_secs: 3600,
        }
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Requests per minute (default)
    pub requests_per_minute: u32,
    /// Burst size
    pub burst_size: u32,
    /// Enable rate limiting
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            burst_size: 10,
            enabled: true,
        }
    }
}

/// WebSocket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Max message size in bytes
    pub max_message_size: usize,
    /// Ping interval in seconds
    pub ping_interval_secs: u64,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            ping_interval_secs: 30,
            timeout_secs: 60,
        }
    }
}

/// Forge server connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConnection {
    /// Forge server URL
    pub url: String,
    /// Authentication token
    pub token: Option<String>,
}

impl ForgeConnection {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("FORGE_URL")
                .unwrap_or_else(|_| "http://localhost:8080".into()),
            token: std::env::var("FORGE_TOKEN").ok(),
        }
    }
}

impl Default for ForgeConnection {
    fn default() -> Self {
        Self {
            url: "http://localhost:8080".to_string(),
            token: None,
        }
    }
}
