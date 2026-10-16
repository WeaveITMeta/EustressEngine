//! # eustress-stream-node
//!
//! Network transport layer for EustressStream.
//!
//! ## Transport hierarchy (fastest → most flexible)
//!
//! | Transport | API | Latency | Throughput | Best for |
//! |-----------|-----|---------|------------|---------|
//! | **In-process** | `EustressStream` directly | **< 1 µs** | **~85M msg/s** | Default — Bevy ECS, AI agents, same binary |
//! | **SHM** | `ShmNodeClient` + `ShmBridge` | **~50 ns** | **~20M msg/s** | Same host, separate processes |
//! | **TCP batch-256** | `StreamNodeClient::publish_batch` | ~412 µs | ~622K msg/s | High-volume network ingest |
//! | **TCP sequential** | `StreamNodeClient::publish` | ~104 µs | ~10K msg/s | Simple network publish |
//! | **QUIC batch-256** | `QuicNodeClient::publish_batch` | ~762 µs | ~336K msg/s | WAN, lossy/multiplexed links |
//!
//! The figures predate TLS on the network transports. The SHM transport and
//! the Unix socket compile only under the `bench-transports` feature, for the
//! benches; no shipped build has them (see Local transports below).
//!
//! **In-process is the default.** `EustressStream` is an embedded crate with no
//! network dependency — call `stream.producer("topic").send_bytes(payload)` directly.
//! All other transports are additive opt-in layers on top.
//!
//! ## Authentication
//!
//! Every network listener (TCP, QUIC, REST) speaks TLS 1.3 only, with an
//! operator-supplied certificate, and every connection logs in, loopback
//! included. Parties are enrolled by Ed25519 public key in `parties.toml` and
//! may publish and subscribe only to the topics their entry grants. See
//! [`auth`] and `docs/architecture/STREAM_NODE_AUTH.md`.
//!
//! ## Local transports
//!
//! The Unix socket transport (`uds`), the shared-memory transport (`shm`) and
//! `StreamNode::start_shm_bridge` have no login, so they compile only under
//! the non-default `bench-transports` feature, which exists for the benches.
//! The crate refuses to compile with it unless `bench-harness` is on too.
//!
//! ## Quick start (in-process, default)
//!
//! ```rust,no_run
//! use eustress_stream::{EustressStream, StreamConfig};
//! use eustress_stream::message::OwnedMessage;
//!
//! let stream = EustressStream::new(StreamConfig::default().in_memory());
//! // Subscribe
//! let _ = stream.subscribe_owned("world_model", |msg: OwnedMessage| println!("{} bytes", msg.data.len()));
//! // Publish — sub-microsecond, ~85M msg/s
//! stream.producer("world_model").send_bytes(bytes::Bytes::from_static(b"delta"));
//! ```
//!
//! ## TCP node (network)
//!
//! ```rust,no_run
//! use eustress_stream_node::{StreamNode, NodeConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = NodeConfig::default()
//!         .with_tls("node.crt", "private/node.key")
//!         .with_parties("private/parties.toml");
//!     let node = StreamNode::start(config).await.unwrap();
//!     println!("Listening on {} (pin {})", node.listen_addr(), node.spki_pin());
//! }
//! ```
//!
//! ## SHM bridge (same host, cross-process)
//!
//! ```rust,ignore
//! // Server process
//! use eustress_stream_node::{StreamNode, NodeConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let node = StreamNode::start(NodeConfig::default()).await.unwrap();
//!     let _bridge = node.start_shm_bridge(64 * 1024 * 1024).unwrap();
//!     // Bridge polls SHM ring and routes into the in-process EustressStream.
//! }
//! ```
//!
//! ```rust,ignore
//! // Publisher in another process (no TCP, ~50 ns/msg)
//! use eustress_stream_node::shm::ShmNodeClient;
//!
//! let mut client = ShmNodeClient::open(33000).unwrap();
//! client.publish("world_model", b"delta").unwrap();
//! ```
//!
//! ## Forge cluster
//!
//! ```rust,no_run
//! use eustress_stream_node::{ForgeCluster, NodeConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Start 10 nodes on ports 33000–33009
//!     let config = NodeConfig::default()
//!         .with_tls("node.crt", "private/node.key")
//!         .with_parties("private/parties.toml");
//!     let cluster = ForgeCluster::start(33000, 10, config).await.unwrap();
//!     println!("Cluster: {} nodes", cluster.node_count());
//!
//!     // Publish — automatically routed to the correct node
//!     cluster.publish("scene_deltas", bytes::Bytes::from_static(b"hello"));
//! }
//! ```

#[cfg(all(feature = "bench-transports", not(feature = "bench-harness")))]
compile_error!("bench-transports is for the benches only; see docs/architecture/STREAM_NODE_AUTH.md");

pub mod auth;
pub mod cluster;
pub mod client;
pub mod config;
pub mod error;
pub mod handler;
pub mod mcp;
pub mod protocol;
pub mod quic;
pub mod rest;
pub mod server;
pub mod transport;
#[cfg(feature = "bench-transports")]
pub mod uds;
#[cfg(feature = "bench-transports")]
pub mod shm;

pub use auth::{NodeAuth, PartiesFile, PartyEntry, TokenClass};
pub use cluster::{ClusterStats, ForgeCluster};
pub use client::{ClientAuth, OsRandom, RandomSource, RestClient, RetryPolicy, StreamNodeClient};
pub use config::NodeConfig;
pub use error::NodeError;
pub use protocol::{ClientFrame, ServerFrame, TopicStats};
pub use rest::{serve_rest, RestServer, RestState};
pub use server::NodeServer;
#[cfg(feature = "bench-transports")]
pub use shm::{ShmBridge, ShmNodeClient, ShmError, shm_ring_path};
pub use transport::{NodeTls, SpkiPin};
// For callers that build a client or a node without naming these crates.
pub use eustress_identity::keypair::UserKeypair;
pub use eustress_stream::StreamConfig;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

pub use eustress_stream::EustressStream;

// ─────────────────────────────────────────────────────────────────────────────
// StreamNode — high-level facade (TCP + REST)
// ─────────────────────────────────────────────────────────────────────────────

/// A running EustressStream node: TCP server + optional REST API.
///
/// The TCP and REST listeners share one set of parties, sessions, quotas and
/// tokens.
pub struct StreamNode {
    server: Arc<NodeServer>,
    stream: EustressStream,
    config: NodeConfig,
}

impl StreamNode {
    /// Start the node. TCP listener begins accepting connections immediately.
    /// REST server starts in a background task if `config.rest_port` is configured
    /// (or when `start_rest()` is called explicitly).
    ///
    /// The startup checks run first. The node refuses, with no listener
    /// started, when the TLS certificate or key is missing, a file fails the
    /// owner-only check, `parties.toml` does not parse, or a non-loopback bind
    /// has no enabled party.
    pub async fn start(config: NodeConfig) -> Result<Self, NodeError> {
        let stream = EustressStream::new(config.stream_config.clone());
        let server = NodeServer::start(stream.clone(), config.clone()).await?;

        Ok(StreamNode { server, stream, config })
    }

    /// Also start the REST API server (non-blocking — runs in background task).
    pub fn start_rest(&self) {
        let state = RestState {
            stream: self.stream.clone(),
            start_time: Arc::new(Instant::now()),
            node_id: self.config.node_id.clone(),
            tcp_port: self.listen_addr().port(),
            rest_port: self.config.effective_rest_port(),
            auth: Arc::clone(self.server.auth()),
        };
        let config = self.config.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_rest(state, &config).await {
                tracing::error!("REST server error: {e}");
            }
        });
    }

    /// Start the REST API server and return it once it is bound, sharing the
    /// TCP listener's authentication state.
    pub async fn start_rest_server(&self) -> Result<RestServer, NodeError> {
        let state = RestState::new(
            self.stream.clone(),
            Arc::clone(self.server.auth()),
            self.listen_addr().port(),
            self.config.effective_rest_port(),
        );
        RestServer::bind(state, &self.config).await
    }

    /// The resolved TCP listen address (may differ from config.port if auto-incremented).
    pub fn listen_addr(&self) -> SocketAddr {
        self.server.listen_addr
    }

    /// Access the underlying EustressStream for direct in-process pub/sub.
    pub fn stream(&self) -> &EustressStream {
        &self.stream
    }

    /// The pin clients hold: SHA-256 of the node certificate's SPKI.
    pub fn spki_pin(&self) -> SpkiPin {
        self.server.auth().tls().spki_pin()
    }

    /// The node's authentication and authorization state.
    pub fn auth(&self) -> &Arc<NodeAuth> {
        self.server.auth()
    }

    /// Start the SHM bridge for same-host cross-process publishing.
    ///
    /// Creates a `ShmRing` at `{tmp}/eustress_{port}.ring` and polls it on a
    /// dedicated blocking thread. Same-host publishers can then use
    /// [`ShmNodeClient::open`] instead of `StreamNodeClient` for ~2000× lower
    /// publish latency (~50 ns vs ~100 µs TCP).
    ///
    /// `ring_bytes` is the SHM ring capacity. 64 MiB (67_108_864) is a good default.
    ///
    /// ## Note on transport choice
    /// - **Subscriptions** still go through TCP — SHM is publish-only (SPSC ring).
    /// - The bridge routes SHM publishes into the same in-process `EustressStream`,
    ///   so TCP subscribers receive messages published via SHM and vice versa.
    /// - The SHM ring has no login and no grants: anything that can open the
    ///   ring file can publish to any topic. It compiles only under the
    ///   bench-only `bench-transports` feature.
    #[cfg(feature = "bench-transports")]
    pub fn start_shm_bridge(&self, ring_bytes: usize) -> Result<ShmBridge, NodeError> {
        let port = self.listen_addr().port();
        shm::ShmBridge::start(port, ring_bytes, self.stream.clone())
            .map_err(|e| NodeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
    }

    /// Gracefully shut down the node.
    pub fn shutdown(&self) {
        self.server.shutdown();
    }
}
