//! Configuration for Eustress Forge game server orchestration.
//!
//! Extends `forge_orchestration` with game-server-specific configuration.

use crate::error::Result;
use forge_orchestration::{AutoscalerConfig, ForgeBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export base types
pub use forge_orchestration::types::Region as BaseRegion;

/// Geographic region for game server placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Region {
    UsEast,
    UsWest,
    EuWest,
    EuCentral,
    AsiaPacific,
    SouthAmerica,
}

impl Region {
    /// Get the Nomad datacenter name for this region.
    pub fn datacenter(&self) -> &'static str {
        match self {
            Region::UsEast => "us-east-1",
            Region::UsWest => "us-west-2",
            Region::EuWest => "eu-west-1",
            Region::EuCentral => "eu-central-1",
            Region::AsiaPacific => "ap-southeast-1",
            Region::SouthAmerica => "sa-east-1",
        }
    }
    
    /// Get display name for this region.
    pub fn display_name(&self) -> &'static str {
        match self {
            Region::UsEast => "US East",
            Region::UsWest => "US West",
            Region::EuWest => "EU West",
            Region::EuCentral => "EU Central",
            Region::AsiaPacific => "Asia Pacific",
            Region::SouthAmerica => "South America",
        }
    }
}

impl From<Region> for BaseRegion {
    fn from(r: Region) -> Self {
        BaseRegion::new(r.datacenter())
    }
}

/// Specification for spawning a new game server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServerSpec {
    /// Experience ID to run
    pub experience_id: String,
    /// Target region
    pub region: Region,
    /// Maximum players allowed
    pub max_players: u32,
    /// Server version (optional, defaults to latest)
    pub version: Option<String>,
    /// Custom environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl GameServerSpec {
    /// Create a new game server spec.
    pub fn new(experience_id: impl Into<String>, region: Region, max_players: u32) -> Self {
        Self {
            experience_id: experience_id.into(),
            region,
            max_players,
            version: None,
            env: HashMap::new(),
        }
    }
    
    /// Set the server version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
    
    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// Game server scaling configuration (extends forge-orchestration's AutoscalerConfig).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameScalingConfig {
    /// Minimum number of game servers per region
    pub min_servers_per_region: u32,
    /// Maximum number of game servers per region
    pub max_servers_per_region: u32,
    /// Target player count per server
    pub target_players_per_server: u32,
    /// Scale-up CPU threshold (0.0 - 1.0)
    pub upscale_threshold: f64,
    /// Scale-down CPU threshold (0.0 - 1.0)
    pub downscale_threshold: f64,
    /// Hysteresis period in seconds
    pub hysteresis_secs: u64,
}

impl Default for GameScalingConfig {
    fn default() -> Self {
        Self {
            min_servers_per_region: 1,
            max_servers_per_region: 100,
            target_players_per_server: 50,
            upscale_threshold: 0.8,
            downscale_threshold: 0.3,
            hysteresis_secs: 300,
        }
    }
}

impl From<GameScalingConfig> for AutoscalerConfig {
    fn from(config: GameScalingConfig) -> Self {
        AutoscalerConfig::default()
            .upscale_threshold(config.upscale_threshold)
            .downscale_threshold(config.downscale_threshold)
            .hysteresis_secs(config.hysteresis_secs)
            .bounds(config.min_servers_per_region, config.max_servers_per_region)
    }
}

/// Eustress Forge configuration for game server orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EustressForgeConfig {
    /// Nomad API address
    pub nomad_api: String,
    /// Nomad ACL token (optional)
    pub nomad_token: Option<String>,
    /// Datacenter
    pub datacenter: String,
    /// Node name
    pub node_name: String,
    /// Game server scaling configuration
    pub scaling: GameScalingConfig,
    /// Enable metrics
    pub metrics_enabled: bool,
    /// HTTP API port
    pub http_port: u16,
    /// Artifact base URL for game server binaries
    pub artifact_base_url: String,
}

impl Default for EustressForgeConfig {
    fn default() -> Self {
        Self {
            nomad_api: "http://127.0.0.1:4646".into(),
            nomad_token: None,
            datacenter: "dc1".into(),
            node_name: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "eustress-forge".into()),
            scaling: GameScalingConfig::default(),
            metrics_enabled: true,
            http_port: 8080,
            artifact_base_url: "s3::https://s3.amazonaws.com/eustress-forge-artifacts".into(),
        }
    }
}

impl EustressForgeConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            nomad_api: std::env::var("NOMAD_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:4646".into()),
            nomad_token: std::env::var("NOMAD_TOKEN").ok(),
            datacenter: std::env::var("NOMAD_DC")
                .unwrap_or_else(|_| "dc1".into()),
            node_name: std::env::var("FORGE_NODE_NAME")
                .or_else(|_| hostname::get().map(|h| h.to_string_lossy().to_string()))
                .unwrap_or_else(|_| "eustress-forge".into()),
            scaling: GameScalingConfig::default(),
            metrics_enabled: std::env::var("FORGE_METRICS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            http_port: std::env::var("FORGE_HTTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            artifact_base_url: std::env::var("FORGE_ARTIFACT_URL")
                .unwrap_or_else(|_| "s3::https://s3.amazonaws.com/eustress-forge-artifacts".into()),
        })
    }
    
    /// Build a ForgeBuilder from this configuration.
    pub fn into_builder(self) -> ForgeBuilder {
        let mut builder = ForgeBuilder::new()
            .with_nomad_api(&self.nomad_api)
            .with_datacenter(&self.datacenter)
            .with_node_name(&self.node_name)
            .with_autoscaler(self.scaling.into())
            .with_metrics(self.metrics_enabled);
        
        if let Some(token) = self.nomad_token {
            builder = builder.with_nomad_token(token);
        }
        
        builder
    }
}