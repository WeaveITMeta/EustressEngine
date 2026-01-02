//! # Eustress Networking
//!
//! High-performance multiplayer networking for Eustress Engine.
//! Built on Lightyear (prediction/rollback replication) + QUIC transport.
//!
//! ## Features
//!
//! - **120Hz tick rate** with adaptive fallback
//! - **Client-owned physics** via Avian integration
//! - **Ownership transfers** with server arbitration
//! - **AOI filtering** for 250+ player scaling
//! - **Delta compression** and anti-exploit validation
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Eustress Networking                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Protocol Layer (messages, channels, components)            │
//! │  ├── Replicated components (Transform, Velocity, etc.)      │
//! │  ├── Messages (OwnershipTransfer, Input, RPC)               │
//! │  └── Channels (Reliable, Unreliable, Ordered)               │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Server Plugin                                               │
//! │  ├── QUIC/TLS transport                                      │
//! │  ├── Ownership arbitration                                   │
//! │  ├── Physics validation (anti-exploit)                       │
//! │  └── AOI replication filtering                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Client Plugin                                               │
//! │  ├── Prediction for owned entities                           │
//! │  ├── Interpolation for remote entities                       │
//! │  └── Input buffering and reconciliation                      │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## World Scale
//!
//! Eustress uses a consistent unit system based on **studs** (1 stud = 0.28 meters).
//! See [`scale`] module for constants and conversion utilities.

pub mod scale;
pub mod protocol;
pub mod ownership;
pub mod replication;
pub mod transport;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "physics")]
pub mod physics;

#[cfg(feature = "p2p")]
pub mod p2p;

mod config;
mod error;

// Re-exports
pub use config::{NetworkConfig, TickConfig, TransportConfig};
pub use error::NetworkError;
pub use ownership::{NetworkOwner, OwnershipRequest, OwnershipTransfer};
pub use protocol::{EustressChannel, EustressMessage, EustressProtocol};
pub use replication::{Replicated, ReplicationFilter, ReplicationGroup};

use bevy::prelude::*;

// ============================================================================
// Main Plugin
// ============================================================================

/// Main networking plugin for Eustress Engine.
///
/// Add this to your Bevy app to enable multiplayer networking.
/// Configure via [`NetworkConfig`] resource before adding.
///
/// # Example
///
/// ```rust,ignore
/// use bevy::prelude::*;
/// use eustress_networking::{EustressNetworkingPlugin, NetworkConfig};
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .insert_resource(NetworkConfig::default())
///         .add_plugins(EustressNetworkingPlugin)
///         .run();
/// }
/// ```
pub struct EustressNetworkingPlugin;

impl Plugin for EustressNetworkingPlugin {
    fn build(&self, app: &mut App) {
        // Register core types
        app.register_type::<NetworkOwner>()
            .register_type::<Replicated>()
            .register_type::<ReplicationGroup>();

        // Initialize config if not present, and extract nested configs as separate resources
        let config = if app.world().contains_resource::<NetworkConfig>() {
            app.world().resource::<NetworkConfig>().clone()
        } else {
            let cfg = NetworkConfig::default();
            app.insert_resource(cfg.clone());
            cfg
        };
        
        // Insert nested configs as separate resources for systems that need them directly
        app.insert_resource(config.replication);
        app.insert_resource(config.ownership);
        app.insert_resource(config.anti_exploit);

        // Add protocol plugin (shared between server/client)
        app.add_plugins(protocol::ProtocolPlugin);
        
        // Add transport plugin ONCE (shared between server/client)
        app.add_plugins(transport::TransportPlugin);

        // Add server or client plugins based on features
        #[cfg(feature = "server")]
        app.add_plugins(server::ServerNetworkPlugin);

        #[cfg(feature = "client")]
        app.add_plugins(client::ClientNetworkPlugin);

        // Add physics integration if enabled
        #[cfg(feature = "physics")]
        app.add_plugins(physics::NetworkPhysicsPlugin);

        // Add P2P distributed world support if enabled
        #[cfg(feature = "p2p")]
        app.add_plugins(p2p::DistributedWorldPlugin);

        info!("Eustress Networking initialized (120Hz, QUIC/TLS)");
    }
}

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common networking types.
pub mod prelude {
    pub use super::{
        EustressNetworkingPlugin,
        NetworkConfig, TickConfig, TransportConfig,
        NetworkError,
        NetworkOwner, OwnershipRequest, OwnershipTransfer,
        EustressChannel, EustressMessage, EustressProtocol,
        Replicated, ReplicationFilter, ReplicationGroup,
        scale::{Stud, STUD_TO_METERS, METERS_TO_STUDS},
    };

    pub use super::protocol::{
        NetworkTransform, NetworkVelocity, NetworkEntity, NetworkHealth,
        PlayerInput, EntityState, EntityDelta,
    };

    #[cfg(feature = "server")]
    pub use super::server::{
        ServerNetworkPlugin, ServerState, ClientManager, StartServer, StopServer,
    };

    #[cfg(feature = "client")]
    pub use super::client::{
        ClientNetworkPlugin, ClientState, LocalClient, Predicted, Interpolated,
        Connect, Disconnect,
    };

    #[cfg(feature = "physics")]
    pub use super::physics::{NetworkPhysicsPlugin, PhysicsOwnership};

    #[cfg(feature = "p2p")]
    pub use super::p2p::{
        DistributedWorldPlugin, ChunkId, ChunkRegistry, DistributedChunk,
        HostManager, PeerId, PeerInfo, P2PSyncConfig, SyncMessage,
    };
}

