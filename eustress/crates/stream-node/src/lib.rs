//! # eustress-stream-node
//!
//! Network transport layer for EustressStream.
//!
//! Wraps the embedded `eustress-stream` crate with:
//! - **TCP node** — binary framing (8-byte LE length + bincode), port range 33000–49151
//! - **REST + SSE** — axum-based HTTP API (`/topics/{name}/publish`, `/topics/{name}/stream`)
//! - **Forge cluster** — consistent-hash routing across N nodes
//! - **MCP tools** — `stream_publish`, `stream_subscribe`, `stream_topics`
//!
//! ## Quick start
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
pub mod rest;
pub mod server;

pub use cluster::{ClusterStats, ForgeCluster};
pub use client::StreamNodeClient;
pub use config::NodeConfig;
pub use error::NodeError;
pub use protocol::{ClientFrame, ServerFrame, TopicStats};
pub use rest::{build_router, serve_rest, RestState};
pub use server::NodeServer;

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

    /// Gracefully shut down the node.
    pub fn shutdown(&self) {
        self.server.shutdown();
    }
}
