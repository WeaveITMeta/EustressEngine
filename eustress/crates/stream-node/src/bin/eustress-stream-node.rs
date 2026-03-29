//! `eustress-stream-node` binary — standalone TCP/REST streaming node.
//!
//! # Usage
//!
//! ```
//! # Single node on default port 33000 with REST on 43000
//! eustress-stream-node
//!
//! # Custom ports
//! eustress-stream-node --port 34000 --rest-port 44000
//!
//! # Forge cluster of 10 nodes starting at port 33000
//! eustress-stream-node --cluster --cluster-size 10 --port 33000
//!
//! # Override via env vars
//! EUSTRESS_PORT=33000 EUSTRESS_CLUSTER_SIZE=5 eustress-stream-node --cluster
//! ```

use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use eustress_stream::StreamConfig;
use eustress_stream_node::{ForgeCluster, NodeConfig, StreamNode};

// ─────────────────────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "eustress-stream-node",
    version,
    about = "EustressStream standalone node — TCP pub/sub + REST/SSE API",
    long_about = None,
)]
struct Args {
    /// TCP port to listen on (auto-increments if occupied).
    #[arg(long, env = "EUSTRESS_PORT", default_value_t = 33000)]
    port: u16,

    /// REST/SSE HTTP port. Defaults to `port + 10000`.
    #[arg(long, env = "EUSTRESS_REST_PORT")]
    rest_port: Option<u16>,

    /// Log filter (RUST_LOG syntax, e.g. "info,eustress_stream_node=debug").
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log_level: String,

    /// Ring buffer capacity per topic (rounded up to next power of two).
    #[arg(long, env = "EUSTRESS_RING_CAP", default_value_t = 65536)]
    ring_cap: usize,

    /// Launch a ForgeCluster of N nodes instead of a single node.
    #[arg(long)]
    cluster: bool,

    /// Number of nodes in the ForgeCluster (requires --cluster).
    #[arg(long, env = "EUSTRESS_CLUSTER_SIZE", default_value_t = 10)]
    cluster_size: usize,

    /// Maximum frame size in bytes (default: 16 MiB).
    #[arg(long, default_value_t = 16 * 1024 * 1024)]
    frame_max: usize,

    /// Per-connection outbound channel capacity (messages).
    #[arg(long, default_value_t = 8192)]
    conn_cap: usize,

    /// Disable the REST/SSE server.
    #[arg(long)]
    no_rest: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&args.log_level))
        .with_target(false)
        .compact()
        .init();

    if args.cluster {
        run_cluster(&args).await;
    } else {
        run_single(&args).await;
    }
}

async fn run_single(args: &Args) {
    let stream_config = StreamConfig::default()
        .in_memory()
        .with_ring_capacity(args.ring_cap);

    let config = NodeConfig {
        port: args.port,
        rest_port: args.rest_port,
        frame_max_bytes: args.frame_max,
        connection_channel_capacity: args.conn_cap,
        stream_config,
        ..NodeConfig::default()
    };

    let node = match StreamNode::start(config).await {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to start node: {e}");
            std::process::exit(1);
        }
    };

    info!("EustressStream node online — TCP {}", node.listen_addr());

    if !args.no_rest {
        node.start_rest();
        let rest_port = node.listen_addr().port() + 10000;
        let effective_rest = args.rest_port.unwrap_or(rest_port);
        info!("REST/SSE API on http://0.0.0.0:{effective_rest}");
        info!("  POST /topics/{{name}}/publish");
        info!("  GET  /topics/{{name}}/stream   (SSE live feed)");
        info!("  GET  /topics/{{name}}/replay   (ring buffer replay)");
        info!("  GET  /topics");
        info!("  GET  /health");
    }

    wait_for_shutdown().await;
    node.shutdown();
    info!("Node shut down cleanly.");
}

async fn run_cluster(args: &Args) {
    let stream_config = StreamConfig::default()
        .in_memory()
        .with_ring_capacity(args.ring_cap);

    let base_config = NodeConfig {
        rest_port: args.rest_port,
        frame_max_bytes: args.frame_max,
        connection_channel_capacity: args.conn_cap,
        stream_config,
        ..NodeConfig::default()
    };

    let cluster = match ForgeCluster::start(args.port, args.cluster_size, base_config).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to start cluster: {e}");
            std::process::exit(1);
        }
    };

    let addrs = cluster.node_addrs();
    info!(
        "ForgeCluster online — {} nodes on ports {}–{}",
        cluster.node_count(),
        addrs.first().map(|a| a.port()).unwrap_or(0),
        addrs.last().map(|a| a.port()).unwrap_or(0),
    );
    for addr in &addrs {
        info!("  node TCP {addr}");
    }

    let stats = cluster.aggregate_stats();
    info!(
        "Cluster ready: {} nodes, {} topics, {} subscribers",
        stats.node_count, stats.total_topics, stats.total_subscribers
    );

    wait_for_shutdown().await;
    cluster.shutdown();
    info!("Cluster shut down cleanly.");
}

// ─────────────────────────────────────────────────────────────────────────────
// Graceful shutdown
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("Received SIGINT"),
        _ = sigterm.recv() => info!("Received SIGTERM"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    info!("Received Ctrl+C");
}
