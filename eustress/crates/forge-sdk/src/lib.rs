//! # Eustress Forge SDK
//!
//! Developer tools and client libraries for building multiplayer experiences
//! with Eustress Forge orchestration platform.
//!
//! ## Features
//!
//! - **Forge Client**: High-level client for server management
//! - **Session Management**: Player session tracking and routing
//! - **Metrics & Monitoring**: Real-time performance metrics
//! - **CLI Tools**: Command-line tools for developers
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use eustress_forge_sdk::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), SdkError> {
//!     let client = ForgeClient::new("http://localhost:4646").await?;
//!     
//!     // Deploy a new experience
//!     let deployment = client.deploy_experience(DeploymentSpec {
//!         experience_id: "my-game".into(),
//!         version: "1.0.0".into(),
//!         regions: vec![Region::UsEast],
//!         min_servers: 2,
//!         max_servers: 10,
//!     }).await?;
//!     
//!     println!("Deployed: {}", deployment.id);
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod deployment;
pub mod error;
pub mod metrics;
pub mod session;
pub mod types;

#[cfg(feature = "cli")]
pub mod cli;

pub use client::ForgeClient;
pub use deployment::DeploymentSpec;
pub use error::SdkError;
pub use types::*;

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common SDK types.
pub mod prelude {
    pub use super::client::ForgeClient;
    pub use super::deployment::{DeploymentSpec, DeploymentStatus};
    pub use super::error::SdkError;
    pub use super::metrics::{MetricsCollector, ServerMetrics};
    pub use super::session::{PlayerSession, SessionManager};
    pub use super::types::{Region, ServerInfo, PlayerInfo};
}