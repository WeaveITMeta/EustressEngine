//! # Eustress Forge
//!
//! Game server orchestration built on [`forge_orchestration`] v0.4.0.
//!
//! This crate extends the base `forge-orchestration` platform with game-server-specific
//! functionality including experience routing, player matchmaking, and QUIC networking.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Eustress Forge                                   │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  eustress-forge (this crate)                                             │
//! │  ├── GameServerJob: Game server job definitions                          │
//! │  ├── ExperienceRouter: Route players to experiences                      │
//! │  └── EustressForgeConfig: Game-specific configuration                    │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  forge-orchestration v0.4.0 (base crate)                                 │
//! │  ├── Forge: Core runtime with HTTP API                                   │
//! │  ├── MoERouter: Hash, load-aware, GPU-aware, version-aware routing       │
//! │  ├── Autoscaler: Threshold-based scaling with hysteresis                 │
//! │  ├── SDK: Sessions, spot handling, UDP/TCP ports, lifecycle              │
//! │  ├── Inference: Request batching, SSE streaming for AI/ML                │
//! │  ├── Resilience: Circuit breakers, retry with backoff                    │
//! │  └── Federation: Multi-region routing and replication                    │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Data Plane (Nomad)                                                      │
//! │  ├── GameServer jobs: Actual game instances (QUIC/UDP)                   │
//! │  ├── PhysicsServer jobs: Dedicated physics simulation                    │
//! │  └── AIServer jobs: NPC behavior and pathfinding                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use eustress_forge::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> eustress_forge::Result<()> {
//!     // Build from environment
//!     let config = EustressForgeConfig::from_env()?;
//!     let forge = config.into_builder().build()?;
//!     
//!     // Spawn a game server
//!     let job = GameServerJob::new("my-experience", Region::UsEast, 100);
//!     forge.submit_job(job.into_job()).await?;
//!     
//!     // Run the control plane
//!     forge.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## SDK Usage (for game servers)
//!
//! ```rust,no_run
//! use eustress_forge::sdk::*;
//!
//! #[tokio::main]
//! async fn main() -> forge_orchestration::Result<()> {
//!     // Signal readiness to orchestrator
//!     ready()?;
//!     
//!     // Allocate QUIC port (UDP + TCP on same port)
//!     let port = allocate_game_port(4433..4500)?;
//!     println!("Game server listening on port {}", port.port);
//!     
//!     // Set up session tracking
//!     let sessions = SessionTracker::default_config();
//!     
//!     // Monitor for spot instance interruption
//!     let spot = start_spot_monitor().await?;
//!     
//!     // Install graceful shutdown handlers
//!     graceful_shutdown();
//!     
//!     // Wait for shutdown or spot interruption
//!     tokio::select! {
//!         _ = shutdown_signal() => println!("Shutdown requested"),
//!         interruption = wait_for_spot_interruption(&spot) => {
//!             println!("Spot interruption: {:?}", interruption);
//!             // Migrate sessions before termination
//!             let migrating = sessions.prepare_migration();
//!         }
//!     }
//!     
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod error;
pub mod jobs;
pub mod routing;

// Re-export core forge-orchestration types
pub use forge_orchestration::{
    // Core runtime
    Forge, ForgeBuilder,
    // Job management
    Job, Task, TaskGroup, Driver,
    // MoE routing (including new 0.4.0 routers)
    MoERouter, DefaultMoERouter, LoadAwareMoERouter, RoundRobinMoERouter, 
    GpuAwareMoERouter, VersionAwareMoERouter, RouteResult,
    // Autoscaling
    Autoscaler, AutoscalerConfig, ScalingDecision,
    // Nomad
    NomadClient,
    // Storage
    StateStore, MemoryStore, FileStore,
    // Metrics
    ForgeMetrics,
    // Types (including new GpuResources)
    types::{Expert, GpuResources, NodeId, Region as BaseRegion, Shard, ShardId},
};

// Re-export SDK for game servers (now includes sessions, spot, UDP ports)
pub use forge_orchestration::sdk;

// Re-export inference module for AI workloads
pub use forge_orchestration::inference;

// Re-export resilience module
pub use forge_orchestration::resilience;

// Re-export federation module
pub use forge_orchestration::federation;

// Re-export control plane module
pub use forge_orchestration::controlplane;

// Re-export scheduler module
pub use forge_orchestration::scheduler;

// Eustress-specific exports
pub use config::{EustressForgeConfig, GameServerSpec, GameScalingConfig, Region};
pub use error::{EustressForgeError, Result};
pub use jobs::{GameServerJob, PhysicsServerJob, AIServerJob};
pub use routing::ExperienceRouter;

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common Eustress Forge types.
pub mod prelude {
    // Eustress-specific
    pub use crate::config::{EustressForgeConfig, GameServerSpec, GameScalingConfig, Region};
    pub use crate::error::Result;
    pub use crate::jobs::{GameServerJob, PhysicsServerJob, AIServerJob};
    pub use crate::routing::ExperienceRouter;
    
    // Core forge-orchestration
    pub use forge_orchestration::{
        Forge, ForgeBuilder,
        Job, Task, Driver,
        AutoscalerConfig,
        MoERouter, GpuAwareMoERouter, VersionAwareMoERouter,
    };
    
    // SDK - lifecycle
    pub use forge_orchestration::sdk::{
        ready, graceful_shutdown, shutdown_signal,
    };
    
    // SDK - ports (including new UDP/game port allocation)
    pub use forge_orchestration::sdk::{
        allocate_port, allocate_udp_port, allocate_game_port,
        AllocatedPort, Protocol,
    };
    
    // SDK - sessions (new in 0.4.0)
    pub use forge_orchestration::sdk::{
        Session, SessionId, SessionState, SessionTracker, SessionConfig,
    };
    
    // SDK - spot instance handling (new in 0.4.0)
    pub use forge_orchestration::sdk::{
        SpotHandler, SpotInterruption, SpotAction, CloudProvider,
        start_spot_monitor, wait_for_spot_interruption, is_spot_instance,
    };
    
    // Inference (new in 0.4.0)
    pub use forge_orchestration::inference::{
        BatchConfig, BatchProcessor,
        StreamingResponse, StreamEvent,
        streaming::StreamSender,
    };
    
    // Resilience (new in 0.4.0)
    pub use forge_orchestration::resilience;
}