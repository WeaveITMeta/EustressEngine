//! Configuration for Eustress Forge.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{ForgeError, ForgeResult};

/// Main configuration for the Forge controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConfig {
    /// Nomad cluster configuration
    pub nomad: NomadConfig,
    
    /// Consul configuration for service discovery
    pub consul: ConsulConfig,
    
    /// Default scaling policies
    pub scaling: ScalingConfig,
    
    /// Health check configuration
    pub health: HealthConfig,
    
    /// Metrics configuration
    pub metrics: MetricsConfig,
}

impl ForgeConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> ForgeResult<Self> {
        Ok(Self {
            nomad: NomadConfig {
                address: std::env::var("NOMAD_ADDR")
                    .unwrap_or_else(|_| "http://127.0.0.1:4646".into()),
                token: std::env::var("NOMAD_TOKEN").ok(),
                namespace: std::env::var("NOMAD_NAMESPACE")
                    .unwrap_or_else(|_| "eustress".into()),
                datacenter: std::env::var("NOMAD_DC")
                    .unwrap_or_else(|_| "dc1".into()),
            },
            consul: ConsulConfig {
                address: std::env::var("CONSUL_ADDR")
                    .unwrap_or_else(|_| "http://127.0.0.1:8500".into()),
                token: std::env::var("CONSUL_TOKEN").ok(),
            },
            scaling: ScalingConfig::default(),
            health: HealthConfig::default(),
            metrics: MetricsConfig::default(),
        })
    }
    
    /// Load configuration from a TOML file.
    pub fn from_file(path: impl AsRef<Path>) -> ForgeResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ForgeError::Config(format!("Failed to read config: {}", e)))?;
        toml::from_str(&content)
            .map_err(|e| ForgeError::Config(format!("Failed to parse config: {}", e)))
    }
}

/// Nomad cluster configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NomadConfig {
    /// Nomad API address
    pub address: String,
    /// ACL token (optional)
    pub token: Option<String>,
    /// Namespace for jobs
    pub namespace: String,
    /// Default datacenter
    pub datacenter: String,
}

/// Consul configuration for service discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsulConfig {
    /// Consul API address
    pub address: String,
    /// ACL token (optional)
    pub token: Option<String>,
}

/// Scaling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingConfig {
    /// Minimum number of game servers per region
    pub min_servers_per_region: u32,
    /// Maximum number of game servers per region
    pub max_servers_per_region: u32,
    /// Target CPU utilization (0.0 - 1.0)
    pub target_cpu_utilization: f32,
    /// Target player count per server
    pub target_players_per_server: u32,
    /// Scale-up cooldown in seconds
    pub scale_up_cooldown_secs: u64,
    /// Scale-down cooldown in seconds
    pub scale_down_cooldown_secs: u64,
}

impl Default for ScalingConfig {
    fn default() -> Self {
        Self {
            min_servers_per_region: 1,
            max_servers_per_region: 100,
            target_cpu_utilization: 0.7,
            target_players_per_server: 50,
            scale_up_cooldown_secs: 30,
            scale_down_cooldown_secs: 300,
        }
    }
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// Health check interval in seconds
    pub check_interval_secs: u64,
    /// Timeout for health checks in seconds
    pub timeout_secs: u64,
    /// Number of consecutive failures before marking unhealthy
    pub unhealthy_threshold: u32,
    /// Number of consecutive successes before marking healthy
    pub healthy_threshold: u32,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 10,
            timeout_secs: 5,
            unhealthy_threshold: 3,
            healthy_threshold: 2,
        }
    }
}

/// Metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics
    pub enabled: bool,
    /// Metrics endpoint port
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9090,
        }
    }
}

/// Geographic region for server placement.
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

/// Specification for spawning a new game server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSpec {
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
    pub env: std::collections::HashMap<String, String>,
}