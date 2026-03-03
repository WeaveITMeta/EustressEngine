//! # Deployment Management
//!
//! ## Table of Contents
//!
//! 1. **DeploymentSpec** - Specification for deploying an experience
//! 2. **DeploymentStatus** - Current status of a deployment
//! 3. **DeploymentInfo** - Full deployment information

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::types::Region;

/// Specification for deploying an experience to Forge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentSpec {
    /// Experience identifier
    pub experience_id: String,
    /// Version string (semver)
    pub version: String,
    /// Target regions for deployment
    pub regions: Vec<Region>,
    /// Minimum number of servers per region
    pub min_servers: u32,
    /// Maximum number of servers per region (auto-scale ceiling)
    pub max_servers: u32,
    /// Maximum players per server
    pub max_players_per_server: Option<u32>,
    /// Environment variables for the server process
    pub env: Option<std::collections::HashMap<String, String>>,
}

/// Current status of a deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentStatus {
    /// Deployment is being created
    Pending,
    /// Servers are being provisioned
    Provisioning,
    /// Deployment is live and serving players
    Active,
    /// Deployment is being scaled down
    Draining,
    /// Deployment has been stopped
    Stopped,
    /// Deployment failed
    Failed,
}

impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Pending => write!(f, "Pending"),
            DeploymentStatus::Provisioning => write!(f, "Provisioning"),
            DeploymentStatus::Active => write!(f, "Active"),
            DeploymentStatus::Draining => write!(f, "Draining"),
            DeploymentStatus::Stopped => write!(f, "Stopped"),
            DeploymentStatus::Failed => write!(f, "Failed"),
        }
    }
}

/// Full deployment information returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentInfo {
    /// Unique deployment ID
    pub id: Uuid,
    /// Experience ID
    pub experience_id: String,
    /// Deployed version
    pub version: String,
    /// Current status
    pub status: DeploymentStatus,
    /// Target regions
    pub regions: Vec<Region>,
    /// Number of running servers
    pub server_count: u32,
    /// Total connected players
    pub player_count: u32,
    /// When the deployment was created
    pub created_at: DateTime<Utc>,
    /// When the deployment was last updated
    pub updated_at: DateTime<Utc>,
}
