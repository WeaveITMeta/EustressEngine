//! # Eustress Networking
//!
//! Multiplayer for Eustress. A Studio (the host) serves the Space it is
//! playing; Players join it, download that Space as `.echk` chunks, and see
//! each other's avatars move in real time.
//!
//! ## Layers
//!
//! | Module | Holds | IO |
//! |---|---|---|
//! | [`wire`] | Protocol version 1: messages, framing, size limits | none |
//! | [`join_link`] | `eustress://join/…` links | none |
//! | [`session`] | Bevy systems: hosting, joining, world download, avatar replication | none |
//! | [`native`] | Desktop transport: a WebTransport host and player | sockets and threads; not built for wasm32 |
//!
//! The first three are IO-free, so a browser build keeps them and swaps in
//! its own transport behind the same [`session::NetLink`] channels.
//!
//! ## Using it
//!
//! Both shells add [`session::NetPlugin`]. Studio starts a host with
//! [`native::start_host`] and [`session::begin_host`]; the Player joins with
//! [`native::start_join`] and [`session::begin_join`]. Shells learn what
//! happened through [`session::NetNotice`] and, on a Player, open the world
//! when [`session::WorldDownloaded`] arrives.
//!
//! ## Also here
//!
//! [`protocol`], [`replication`], [`ownership`] and [`scale`] hold the
//! network component vocabulary `eustress-runtime` syncs `BasePart` through,
//! and ownership arbitration (locks, distance checks, transfer cooldown,
//! ping contention) kept for server-authoritative physics, the next
//! replication tier after avatars.

pub mod join_link;
pub mod session;
pub mod wire;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub mod native;

pub mod ownership;
pub mod protocol;
pub mod replication;
pub mod scale;

mod config;
mod error;

pub use config::{NetworkConfig, TickConfig, TransportConfig};
pub use error::NetworkError;
pub use join_link::JoinLink;
pub use ownership::{NetworkOwner, OwnershipRequest, OwnershipTransfer};
pub use protocol::{EustressChannel, EustressMessage, EustressProtocol};
pub use replication::{Replicated, ReplicationFilter, ReplicationGroup};
pub use session::{
    begin_host, begin_join, EndSession, HostConfig, HostSession, LocalWorldReady, NetLink, NetNotice, NetPlugin,
    NetReplica, PlayerSession, SendChat, WorldDownloaded,
};

/// Common re-exports.
pub mod prelude {
    pub use super::{
        begin_host, begin_join, EndSession, HostConfig, HostSession, JoinLink, LocalWorldReady, NetLink, NetNotice,
        NetPlugin, NetReplica, PlayerSession, SendChat, WorldDownloaded,
    };

    pub use super::{
        scale::{Stud, METERS_TO_STUDS, STUD_TO_METERS},
        EustressChannel, EustressMessage, EustressProtocol, NetworkConfig, NetworkError, NetworkOwner,
        OwnershipRequest, OwnershipTransfer, Replicated, ReplicationFilter, ReplicationGroup, TickConfig,
        TransportConfig,
    };

    pub use super::protocol::{
        EntityDelta, EntityState, NetworkEntity, NetworkHealth, NetworkTransform, NetworkVelocity, PlayerInput,
    };
}
