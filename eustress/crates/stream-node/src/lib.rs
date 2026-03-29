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
//! **In-process is the default.** `EustressStream` is an embedded crate with no
//! network dependency — call `stream.producer("topic").send_bytes(payload)` directly.
//! All other transports are additive opt-in layers on top.
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
//!     let node = StreamNode::start(NodeConfig::default()).await.unwrap();
//!     println!("Listening on {}", node.listen_addr());
//! }
//! ```
//!
//! ## SHM bridge (same host, cross-process)
//!
//! ```rust,no_run
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
//! ```rust,no_run
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
//!     let cluster = ForgeCluster::start(33000, 10, NodeConfig::default()).await.unwrap();
//!     println!("Cluster: {} nodes", cluster.node_count());
//!
//!     // Publish — automatically routed to the correct node
//!     cluster.publish("scene_deltas", bytes::Bytes::from_static(b"hello"));
//! }
//! ```

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
pub mod uds;
pub mod shm;

pub use cluster::{ClusterStats, ForgeCluster};
pub use client::StreamNodeClient;
pub use config::NodeConfig;
pub use error::NodeError;
pub use protocol::{ClientFrame, ServerFrame, TopicStats};
pub use rest::{build_router, serve_rest, RestState};
pub use server::NodeServer;
pub use shm::{ShmBridge, ShmNodeClient, ShmError, shm_ring_path};

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use eustress_stream::EustressStream;

// ─────────────────────────────────────────────────────────────────────────────
// StreamNode — high-level facade (TCP + REST)
// ─────────────────────────────────────────────────────────────────────────────

/// A running EustressStream node: TCP server + optional REST API.
pub struct StreamNode {
    server: Arc<NodeServer>,
    stream: EustressStream,
    config: NodeConfig,
}

impl StreamNode {
    /// Start the node. TCP listener begins accepting connections immediately.
    /// REST server starts in a background task if `config.rest_port` is configured
    /// (or when `start_rest()` is called explicitly).
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
        };
        let config = self.config.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_rest(state, &config).await {
                tracing::error!("REST server error: {e}");
            }
        });
    }

    /// The resolved TCP listen address (may differ from config.port if auto-incremented).
    pub fn listen_addr(&self) -> SocketAddr {
        self.server.listen_addr
    }

    /// Access the underlying EustressStream for direct in-process pub/sub.
    pub fn stream(&self) -> &EustressStream {
        &self.stream
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
