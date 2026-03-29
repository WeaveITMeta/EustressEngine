//! # EustressStream
//!
//! Embedded, cross-platform, append-only streaming core for EustressEngine.
//!
//! ## Design goals
//! - **Zero-copy delivery**: subscribers receive `MessageView<'_>` — a borrowed
//!   slice directly into the ring buffer slot.  No heap allocation on the hot path.
//! - **In-process first**: producers and consumers live in the same address space.
//!   No external server process, no network round-trip.
//! - **Persistent**: optional segment-log storage (cross-platform `memmap2`; Linux
//!   `io_uring` with the `io-uring` feature for maximum throughput).
//! - **Cross-platform**: Linux, macOS, Windows — the same API everywhere.
//! - **Bevy-native**: optional `bevy` feature adds `EustressStreamPlugin`,
//!   `Res<EustressStream>`, and `StreamMetrics`.
//!
//! ## Quick start
//!
//! ```rust
//! use eustress_stream::{EustressStream, StreamConfig};
//! use bytes::Bytes;
//!
//! let stream = EustressStream::new(StreamConfig::default().in_memory());
//!
//! // Subscribe — zero-copy
//! stream.subscribe("events", |view| {
//!     println!("offset={} data={:?}", view.offset, view.data);
//! }).unwrap();
//!
//! // Produce
//! let producer = stream.producer("events");
//! producer.send_bytes(Bytes::from_static(b"hello world"));
//! ```
//!
//! ## Bevy integration
//!
//! ```rust,no_run,ignore
//! // Requires the `bevy` feature flag.
//! use bevy::prelude::*;
//! use eustress_stream::{EustressStreamPlugin, StreamConfig, EustressStream};
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(EustressStreamPlugin::new(StreamConfig::default()))
//!     .run();
//! ```
//!
//! ## Performance targets
//! - In-memory pub/sub: **> 10 M messages/sec** (single-threaded, no sub, 100-byte payloads)
//! - Pod send + cast:   **> 8 M messages/sec** (one subscriber, 40-byte struct)
//! - Persistent (mmap): **> 2 M messages/sec** (one subscriber, 100-byte payloads, no compression)
//!
//! Run `cargo bench -p eustress-stream` to verify on your hardware.

#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod message;
pub mod ring;
pub mod storage;
pub mod stream;
pub mod topic;

#[cfg(feature = "bevy")]
pub mod bevy_plugin;

// ── Re-exports (flat public API) ──────────────────────────────────────────────

pub use config::StreamConfig;
pub use error::StreamError;
pub use message::{MessageHeader, MessageView, OwnedMessage};
pub use stream::{EustressStream, Producer};
pub use topic::{SubscriberId, Topic};

#[cfg(feature = "bevy")]
pub use bevy_plugin::{
    EustressStreamPlugin, StreamMetrics, StreamRef, SubscriptionHandle,
};

// ── Dependency re-exports for downstream convenience ─────────────────────────

pub use bytes;
pub use flume;
