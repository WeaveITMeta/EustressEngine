//! # Forge SDK
//!
//! Lightweight SDK for workloads to interact with the Forge orchestrator.
//!
//! This module provides utilities for workloads running under Forge orchestration:
//! - **Lifecycle**: Signal readiness, handle graceful shutdown
//! - **Port Allocation**: Request and release ports dynamically
//! - **Client**: HTTP client for Forge API communication
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use forge_orchestration::sdk::{ready, allocate_port, graceful_shutdown};
//!
//! #[tokio::main]
//! async fn main() -> forge_orchestration::Result<()> {
//!     // Signal readiness
//!     ready()?;
//!
//!     // Get a port
//!     let port = allocate_port(8000..9000)?;
//!     println!("Listening on port {}", port);
//!
//!     // Handle shutdown
//!     graceful_shutdown();
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod lifecycle;
pub mod port;
pub mod session;
pub mod spot;

pub use client::{ForgeClient, MetricsReport, start_heartbeat};
pub use lifecycle::{graceful_shutdown, is_ready, notify_shutdown, ready, shutdown_requested, shutdown_signal};
pub use port::{
    allocate_port, allocate_specific_port, allocated_ports, is_port_available, release_port,
    allocate_udp_port, allocate_specific_udp_port, allocate_game_port, allocate_specific_game_port,
    allocate_port_with_protocol, allocate_specific_port_with_protocol,
    allocated_ports_detailed, is_udp_port_available, is_port_available_for_protocol,
    AllocatedPort, PortAllocator, Protocol,
};
pub use spot::{
    SpotHandler, SpotInterruption, SpotAction, CloudProvider,
    start_spot_monitor, wait_for_spot_interruption, is_spot_instance,
};
pub use session::{
    Session, SessionId, SessionState, SessionTracker, SessionConfig,
    ConnectionInfo,
};

use crate::error::ForgeError;

/// Environment variable for Forge API endpoint
pub const FORGE_API_ENV: &str = "FORGE_API";

/// Environment variable for allocation ID
pub const FORGE_ALLOC_ID_ENV: &str = "FORGE_ALLOC_ID";

/// Environment variable for task name
pub const FORGE_TASK_NAME_ENV: &str = "FORGE_TASK_NAME";

/// Get the Forge API endpoint from environment
pub fn forge_api_url() -> Option<String> {
    std::env::var(FORGE_API_ENV).ok()
}

/// Get the allocation ID from environment
pub fn alloc_id() -> Option<String> {
    std::env::var(FORGE_ALLOC_ID_ENV).ok()
}

/// Get the task name from environment
pub fn task_name() -> Option<String> {
    std::env::var(FORGE_TASK_NAME_ENV).ok()
}

/// SDK-specific error type
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    /// Forge API not configured
    #[error("Forge API not configured: set {0} environment variable")]
    NotConfigured(&'static str),

    /// API communication error
    #[error("API error: {0}")]
    Api(String),

    /// Port allocation failed
    #[error("Port allocation failed: {0}")]
    PortAllocation(String),

    /// Lifecycle error
    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// HTTP error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

impl SdkError {
    /// Create an API error
    pub fn api(msg: impl Into<String>) -> Self {
        Self::Api(msg.into())
    }

    /// Create a port allocation error
    pub fn port(msg: impl Into<String>) -> Self {
        Self::PortAllocation(msg.into())
    }

    /// Create a lifecycle error
    pub fn lifecycle(msg: impl Into<String>) -> Self {
        Self::Lifecycle(msg.into())
    }
}

impl From<SdkError> for ForgeError {
    fn from(err: SdkError) -> Self {
        ForgeError::Internal(err.to_string())
    }
}

/// SDK Result type alias
pub type SdkResult<T> = std::result::Result<T, SdkError>;
