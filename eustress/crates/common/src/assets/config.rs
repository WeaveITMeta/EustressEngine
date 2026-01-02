//! # Asset Configuration
//!
//! TOML-based configuration for asset sources and caching.

use super::AssetSource;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Asset system configuration
#[derive(Debug, Clone, Serialize, Deserialize, bevy::prelude::Resource)]
pub struct AssetConfig {
    /// Cache size in megabytes
    #[serde(default = "default_cache_size")]
    pub cache_size_mb: usize,
    
    /// Primary asset sources (tried in order)
    #[serde(default)]
    pub sources: Vec<AssetSource>,
    
    /// Fallback IPFS gateways
    #[serde(default = "default_ipfs_gateways")]
    pub ipfs_gateways: Vec<String>,
    
    /// Enable P2P asset sharing
    #[serde(default)]
    pub enable_p2p: bool,
    
    /// Maximum concurrent downloads
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_downloads: usize,
    
    /// Download timeout in seconds
    #[serde(default = "default_timeout")]
    pub download_timeout_secs: u64,
    
    /// Enable progressive loading
    #[serde(default = "default_true")]
    pub progressive_loading: bool,
    
    /// Auto-cleanup cache interval (seconds)
    #[serde(default = "default_cleanup_interval")]
    pub cache_cleanup_interval_secs: u64,
}

fn default_cache_size() -> usize { 256 } // 256 MB
fn default_max_concurrent() -> usize { 4 }
fn default_timeout() -> u64 { 30 }
fn default_true() -> bool { true }
fn default_cleanup_interval() -> u64 { 60 }

fn default_ipfs_gateways() -> Vec<String> {
    vec![
        "https://ipfs.io".to_string(),
        "https://gateway.pinata.cloud".to_string(),
        "https://cloudflare-ipfs.com".to_string(),
        "https://dweb.link".to_string(),
    ]
}

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            cache_size_mb: default_cache_size(),
            sources: Vec::new(),
            ipfs_gateways: default_ipfs_gateways(),
            enable_p2p: false,
            max_concurrent_downloads: default_max_concurrent(),
            download_timeout_secs: default_timeout(),
            progressive_loading: true,
            cache_cleanup_interval_secs: default_cleanup_interval(),
        }
    }
}

impl AssetConfig {
    /// Load from TOML file or return default
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(config) => return config,
                        Err(e) => {
                            tracing::warn!("Failed to parse asset config: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read asset config: {}", e);
                }
            }
        }
        Self::default()
    }
    
    /// Save to TOML file
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
    
    /// Create a development config (local only)
    pub fn development() -> Self {
        Self {
            cache_size_mb: 512,
            sources: vec![],
            ipfs_gateways: Vec::new(),
            enable_p2p: false,
            max_concurrent_downloads: 8,
            download_timeout_secs: 10,
            progressive_loading: false, // Faster iteration
            cache_cleanup_interval_secs: 300,
        }
    }
    
    /// Create a production config (all sources)
    pub fn production() -> Self {
        Self {
            cache_size_mb: 256,
            sources: vec![],
            ipfs_gateways: default_ipfs_gateways(),
            enable_p2p: true,
            max_concurrent_downloads: 4,
            download_timeout_secs: 30,
            progressive_loading: true,
            cache_cleanup_interval_secs: 60,
        }
    }
    
    /// Add a URL source
    pub fn with_url(mut self, url: &str) -> Self {
        self.sources.push(AssetSource::Url(url.to_string()));
        self
    }
    
    /// Add an S3 source
    pub fn with_s3(mut self, bucket: &str, region: &str) -> Self {
        self.sources.push(AssetSource::S3 {
            bucket: bucket.to_string(),
            key: String::new(), // Will be filled per-asset
            region: region.to_string(),
            endpoint: None,
        });
        self
    }
    
    /// Add Cloudflare R2 source
    pub fn with_r2(mut self, account_id: &str, bucket: &str) -> Self {
        self.sources.push(AssetSource::CloudflareR2 {
            account_id: account_id.to_string(),
            bucket: bucket.to_string(),
            key: String::new(),
        });
        self
    }
}

/// Example TOML configuration file
pub const EXAMPLE_CONFIG: &str = r#"
# Eustress Asset Configuration
# Save as assets.toml in your project root

# Cache size in megabytes
cache_size_mb = 256

# Maximum concurrent downloads
max_concurrent_downloads = 4

# Download timeout in seconds
download_timeout_secs = 30

# Enable progressive LOD loading
progressive_loading = true

# Enable P2P asset sharing (reduces server costs)
enable_p2p = false

# Cache cleanup interval (seconds)
cache_cleanup_interval_secs = 60

# IPFS gateways (tried in order)
ipfs_gateways = [
    "https://ipfs.io",
    "https://gateway.pinata.cloud",
    "https://cloudflare-ipfs.com",
    "https://dweb.link",
]

# Asset sources (tried in order)
# Uncomment and configure as needed:

# [[sources]]
# type = "Url"
# url = "https://assets.mygame.com"

# [[sources]]
# type = "S3"
# bucket = "my-game-assets"
# region = "us-east-1"

# [[sources]]
# type = "CloudflareR2"
# account_id = "your-account-id"
# bucket = "game-assets"
"#;

/// Game-specific asset configuration (embedded in game.ron)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAssetConfig {
    /// Primary CDN URL
    pub primary_cdn: Option<String>,
    
    /// Fallback CDN URLs
    pub fallback_cdns: Vec<String>,
    
    /// IPFS CID for the game's asset bundle
    pub ipfs_bundle_cid: Option<String>,
    
    /// Required assets (must be loaded before game starts)
    pub required_assets: Vec<String>,
    
    /// Preload assets (load in background)
    pub preload_assets: Vec<String>,
}

impl Default for GameAssetConfig {
    fn default() -> Self {
        Self {
            primary_cdn: None,
            fallback_cdns: Vec::new(),
            ipfs_bundle_cid: None,
            required_assets: Vec::new(),
            preload_assets: Vec::new(),
        }
    }
}
