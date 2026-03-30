//! # StreamNodePlugin
//!
//! Bridges the engine's in-process `EustressStream` to the network so that
//! remote tools, AI agents, and other game servers can subscribe to scene
//! deltas, simulation results, and log output via TCP or QUIC.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  Bevy ECS                                                   │
//! │  ChangeQueue.stream (in-process EustressStream)             │
//! │        │                                                    │
//! │        │  Arc clone — zero-copy, same ring buffer           │
//! │        ▼                                                    │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │  StreamNodePlugin                                    │  │
//! │  │  NodeServer × cluster_nodes                          │  │
//! │  │  TCP  :33000 – 33000+N   (EustressStream protocol)  │  │
//! │  │  REST :43000 – 43000+N   (HTTP + SSE)               │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//!         │ TCP/QUIC
//!         ├─► Remote AI agent
//!         ├─► CLI tools (eustress stream subscribe ...)
//!         ├─► Browser dashboard (REST/SSE)
//!         └─► Other game servers
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Single node (default) — port 33000
//! app.add_plugins(StreamNodePlugin::default());
//!
//! // Three-node fan-out cluster — ports 33000, 33001, 33002
//! app.add_plugins(StreamNodePlugin::cluster(33000, 3));
//! ```
//!
//! ## Port layout
//!
//! | Node | TCP    | REST   |
//! |------|--------|--------|
//! | 0    | 33000  | 43000  |
//! | 1    | 33001  | 43001  |
//! | …    | …      | …      |
//! | N-1  | 33000+N| 43000+N|

use bevy::prelude::*;
use std::net::SocketAddr;
use std::sync::Arc;

use eustress_common::change_queue::ChangeQueue;
use eustress_stream_node::{NodeConfig, NodeServer};

// ─────────────────────────────────────────────────────────────────────────────
// StreamNodeConfig resource
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the StreamNodePlugin.
///
/// Insert before the plugin runs to override defaults.
#[derive(Resource, Debug, Clone)]
pub struct StreamNodeConfig {
    /// Base TCP port. Nodes occupy `base_port..base_port + cluster_nodes`.
    /// Default: 33000.
    pub base_port: u16,
    /// Number of TCP nodes to start. Each shares the same in-process stream,
    /// giving remote clients N endpoints to connect to for load distribution.
    /// Default: 1.
    pub cluster_nodes: u8,
    /// Enable REST + SSE on `base_port + 10000`. Default: true.
    pub rest: bool,
    /// Maximum simultaneous TCP connections per node. Default: 4096.
    pub max_connections: usize,
}

impl Default for StreamNodeConfig {
    fn default() -> Self {
        Self {
            base_port: 33000,
            cluster_nodes: 1,
            rest: true,
            max_connections: 4096,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamNodeHandle resource  (stored after startup)
// ─────────────────────────────────────────────────────────────────────────────

/// Live node handles — kept alive for the duration of the session.
///
/// Drop this resource to shut down all nodes.
#[derive(Resource)]
pub struct StreamNodeHandle {
    /// One `NodeServer` per `cluster_nodes`. All share the engine's stream.
    pub nodes: Vec<Arc<NodeServer>>,
    /// Resolved listen addresses (TCP).
    pub addrs: Vec<SocketAddr>,
}

impl StreamNodeHandle {
    /// Primary TCP address (first node).
    pub fn primary_addr(&self) -> Option<SocketAddr> {
        self.addrs.first().copied()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamNodePlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy plugin that exposes the engine's in-process EustressStream over TCP.
///
/// Requires `StreamingPlugin` (or a manually inserted `ChangeQueue`) to be
/// present — the plugin reads `ChangeQueue.stream` and bridges it to N
/// TCP nodes.
pub struct StreamNodePlugin {
    config: StreamNodeConfig,
}

impl StreamNodePlugin {
    /// Single node on the default port (33000).
    pub fn default() -> Self {
        Self { config: StreamNodeConfig::default() }
    }

    /// Fan-out cluster of `nodes` TCP endpoints starting at `base_port`.
    pub fn cluster(base_port: u16, nodes: u8) -> Self {
        Self {
            config: StreamNodeConfig {
                base_port,
                cluster_nodes: nodes.max(1),
                ..Default::default()
            },
        }
    }

    /// Full builder — consume a pre-built config.
    pub fn with_config(config: StreamNodeConfig) -> Self {
        Self { config }
    }
}

impl Plugin for StreamNodePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
           .add_systems(Startup, start_stream_nodes.run_if(resource_exists::<ChangeQueue>));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// startup system
// ─────────────────────────────────────────────────────────────────────────────

fn start_stream_nodes(
    config: Res<StreamNodeConfig>,
    queue: Res<ChangeQueue>,
    mut commands: Commands,
) {
    let stream = queue.stream.clone();
    let cfg = config.clone();

    // Spin up nodes on a temporary tokio runtime so we don't need one on the
    // main thread.  The NodeServer background tasks run on the engine's own
    // tokio runtime (spawned inside NodeServer::start).
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        rt.block_on(async move {
            let mut nodes = Vec::with_capacity(cfg.cluster_nodes as usize);
            let mut addrs = Vec::with_capacity(cfg.cluster_nodes as usize);

            for i in 0..cfg.cluster_nodes {
                let port = cfg.base_port.saturating_add(i as u16);
                let rest_port = if cfg.rest {
                    Some(port + 10_000)
                } else {
                    None
                };

                let node_config = NodeConfig {
                    port,
                    rest_port,
                    max_connections: cfg.max_connections,
                    auto_increment: false, // hard-bind to the configured port
                    node_id: format!("engine-node-{i}"),
                    ..Default::default()
                };

                match NodeServer::start(stream.clone(), node_config).await {
                    Ok(node) => {
                        let addr = node.listen_addr;
                        info!("StreamNodePlugin: node {i} listening on {addr}");
                        addrs.push(addr);
                        nodes.push(node);
                    }
                    Err(e) => {
                        warn!("StreamNodePlugin: failed to start node {i}: {e}");
                    }
                }
            }

            (nodes, addrs)
        })
    })
    .join()
    .unwrap_or_else(|_| (vec![], vec![]));

    let (nodes, addrs) = result;

    if nodes.is_empty() {
        warn!("StreamNodePlugin: no nodes started — remote pub/sub unavailable.");
        return;
    }

    info!(
        "StreamNodePlugin: {} node(s) online. Primary TCP {}{}",
        nodes.len(),
        addrs.first().map(|a| a.to_string()).unwrap_or_default(),
        if config.rest {
            format!(", REST http://{}", {
                let mut a = addrs[0];
                a.set_port(addrs[0].port() + 10_000);
                a
            })
        } else {
            String::new()
        }
    );

    commands.insert_resource(StreamNodeHandle { nodes, addrs });
}
