//! # Forge Orchestration
//!
//! A Rust-native orchestration platform for distributed workloads with
//! Mixture of Experts (MoE) routing, autoscaling, and Nomad integration.
//!
//! ## Features
//!
//! - **Job Management**: Define and submit jobs with task groups
//! - **MoE Routing**: Intelligent request routing to expert workers
//! - **Autoscaling**: Threshold and predictive scaling policies
//! - **Nomad Integration**: Schedule via HashiCorp Nomad
//! - **Metrics**: Prometheus-compatible metrics export
//! - **SDK**: Embedded SDK for workloads (lifecycle, ports, heartbeats)
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use forge_orchestration::{ForgeBuilder, AutoscalerConfig, Job, Task, Driver};
//!
//! #[tokio::main]
//! async fn main() -> forge_orchestration::Result<()> {
//!     let forge = ForgeBuilder::new()
//!         .with_autoscaler(AutoscalerConfig::default())
//!         .build()?;
//!
//!     let job = Job::new("my-service")
//!         .with_group("api", Task::new("server")
//!             .driver(Driver::Exec)
//!             .command("/usr/bin/server"));
//!
//!     forge.submit_job(job).await?;
//!     forge.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## SDK Usage (for workloads)
//!
//! ```rust,no_run
//! use forge_orchestration::sdk::{ready, allocate_port, graceful_shutdown};
//!
//! #[tokio::main]
//! async fn main() -> forge_orchestration::Result<()> {
//!     ready()?;
//!     let port = allocate_port(8000..9000)?;
//!     graceful_shutdown();
//!     // ... serve on port ...
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod autoscaler;
pub mod builder;
pub mod controlplane;
pub mod error;
pub mod federation;
pub mod inference;
pub mod job;
pub mod metrics;
pub mod moe;
pub mod networking;
pub mod nomad;
pub mod resilience;
pub mod runtime;
pub mod scheduler;
pub mod sdk;
pub mod storage;
pub mod types;

// Re-exports for ergonomic API
pub use autoscaler::{Autoscaler, AutoscalerConfig, ScalingDecision};
pub use builder::ForgeBuilder;
pub use error::{ForgeError, Result};
pub use job::{Driver, Job, Task, TaskGroup};
pub use metrics::{ForgeMetrics, MetricsExporter, MetricsHook};
pub use moe::{DefaultMoERouter, LoadAwareMoERouter, MoERouter, RoundRobinMoERouter, RouteResult, GpuAwareMoERouter, VersionAwareMoERouter};
pub use networking::{HttpServer, HttpServerConfig};
#[cfg(feature = "quic")]
pub use networking::QuicTransport;
pub use nomad::NomadClient;
pub use runtime::Forge;
pub use storage::{FileStore, MemoryStore, StateStore};
pub use types::{Expert, GpuResources, NodeId, Region, Shard, ShardId};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::autoscaler::{Autoscaler, AutoscalerConfig};
    pub use crate::builder::ForgeBuilder;
    pub use crate::error::Result;
    pub use crate::job::{Driver, Job, Task};
    pub use crate::moe::MoERouter;
    pub use crate::runtime::Forge;
    pub use crate::sdk::{allocate_port, graceful_shutdown, ready, ForgeClient};
    pub use crate::types::{Expert, Shard};
}
