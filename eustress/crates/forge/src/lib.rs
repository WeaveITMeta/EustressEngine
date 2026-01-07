//! # Eustress Forge
//!
//! Rust-native multiplayer orchestration platform built on HashiCorp Nomad.
//! Replaces the deprecated Kubernetes MoE architecture with a more efficient,
//! lower-overhead solution.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Eustress Forge                                   │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Control Plane (Rust)                                                    │
//! │  ├── ForgeController: Orchestrates game server lifecycle                 │
//! │  ├── SessionManager: Tracks active sessions and player routing           │
//! │  ├── ScalingEngine: Auto-scales based on demand metrics                  │
//! │  └── HealthMonitor: Monitors server health and triggers failover         │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Data Plane (Nomad)                                                      │
//! │  ├── GameServer jobs: Actual game instances                              │
//! │  ├── PhysicsServer jobs: Dedicated physics simulation                    │
//! │  └── AIServer jobs: NPC behavior and pathfinding                         │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Service Mesh (Consul)                                                   │
//! │  ├── Service discovery for game servers                                  │
//! │  ├── Health checking and load balancing                                  │
//! │  └── Configuration management                                            │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Benefits over K8s
//!
//! - **Lower overhead**: <0.5% cluster waste vs 3-7% for K8s
//! - **Faster scaling**: Milliseconds vs seconds-minutes
//! - **Cost savings**: 80-90% reduction at scale
//! - **Pure Rust SDK**: No CRD/controller abstractions
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use eustress_forge::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), ForgeError> {
//!     let forge = ForgeController::new(ForgeConfig::from_env()?).await?;
//!     
//!     // Spawn a game server
//!     let server = forge.spawn_server(ServerSpec {
//!         experience_id: "my-experience".into(),
//!         region: Region::UsEast,
//!         max_players: 100,
//!     }).await?;
//!     
//!     // Route player to server
//!     forge.route_player(player_id, server.id()).await?;
//!     
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod controller;
pub mod error;
pub mod health;
pub mod jobs;
pub mod metrics;
pub mod routing;
pub mod scaling;
pub mod session;

pub use config::ForgeConfig;
pub use controller::ForgeController;
pub use error::ForgeError;
pub use session::SessionManager;

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common Forge types.
pub mod prelude {
    pub use super::config::{ForgeConfig, Region, ServerSpec};
    pub use super::controller::ForgeController;
    pub use super::error::ForgeError;
    pub use super::health::HealthStatus;
    pub use super::jobs::{JobSpec, JobStatus};
    pub use super::routing::PlayerRoute;
    pub use super::scaling::{ScalingPolicy, ScalingMetrics};
    pub use super::session::{Session, SessionManager};
}