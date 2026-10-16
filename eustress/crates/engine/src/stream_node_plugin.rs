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
//!
//! ## Authentication
//!
//! Every connection logs in over TLS 1.3, loopback included, and each party
//! is held to its grants in `parties.toml`. The plugin starts no node unless
//! [`StreamNodeConfig`] names a certificate, a private key and a parties file
//! that pass the node's startup checks. The default names none, so a default
//! engine opens no stream port. Where the engine finds those files is an open
//! item (see `docs/architecture/STREAM_NODE_AUTH.md`).

use bevy::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use eustress_common::change_queue::ChangeQueue;
use eustress_stream_node::{NodeConfig, NodeServer};
use eustress_stream_node::{EustressStream, NodeAuth, RestServer, RestState};

// ─────────────────────────────────────────────────────────────────────────────
// StreamNodeConfig resource
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the StreamNodePlugin.
///
/// Insert before the plugin runs to override defaults.
#[derive(Resource, Debug, Clone)]
pub struct StreamNodeConfig {
    /// Interface every node binds. Default: `127.0.0.1`.
    ///
    /// Every connection logs in over TLS 1.3, loopback included, and each
    /// party may publish and subscribe only to the topics its `parties.toml`
    /// entry grants (`scene_deltas` and `agent_commands` also need
    /// `admin = true` to publish). A non-loopback bind additionally requires
    /// at least one enabled party; with none, no node starts.
    pub bind_addr: IpAddr,
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
    /// PEM certificate every node presents. Required: without it, `tls_key`
    /// and `parties_path`, no node starts. Default: `None`.
    pub tls_cert: Option<PathBuf>,
    /// PEM private key for `tls_cert`. Required, and it must pass the
    /// owner-only check. Default: `None`.
    pub tls_key: Option<PathBuf>,
    /// `parties.toml`: the parties enrolled to log in, with their grants.
    /// Required; it and its folder must pass the owner-only check.
    /// Default: `None`.
    pub parties_path: Option<PathBuf>,
}

impl Default for StreamNodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            base_port: 33000,
            cluster_nodes: 1,
            rest: true,
            max_connections: 4096,
            tls_cert: None,
            tls_key: None,
            parties_path: None,
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
    /// The runtime the nodes' accept loops and REST servers run on. It lives
    /// as long as this resource; dropping the resource stops it, which closes
    /// every listener.
    _runtime: NodeRuntime,
}

/// The runtime a plugin's nodes run on, on a thread of its own that stays in
/// `block_on` until this is dropped.
struct NodeRuntime {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for NodeRuntime {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Whether the plugin started its nodes. The startup system inserts it.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub enum StreamNodeStatus {
    /// The configuration does not name a certificate, a key and a parties
    /// file that pass the startup checks, so no node was started. The text
    /// says why.
    NotConfigured(String),
    /// The configuration passed, and at least one node is listening.
    Running,
    /// The configuration passed, but no node started. The log says why.
    Failed(String),
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
           .add_systems(Startup, start_stream_nodes.run_if(resource_exists::<ChangeQueue>))
           .add_systems(
               Startup,
               write_stream_port_file
                   .run_if(resource_exists::<StreamNodeHandle>)
                   .after(start_stream_nodes),
           );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Port-file advertisement
// ─────────────────────────────────────────────────────────────────────────────

/// Writes the primary TCP port of the StreamNode to
/// `<universe>/.eustress/engine.stream.port` so sibling processes can
/// discover it the same way they discover the LSP (`.eustress/lsp.port`)
/// and the Engine Bridge (`.eustress/engine.port`).
///
/// Kept as a separate system so callers who want a different port-file
/// location (multi-instance, CI) can skip this system and write their
/// own. No-op if no node started.
pub fn write_stream_port_file(handle: Res<StreamNodeHandle>) {
    let Some(addr) = handle.primary_addr() else {
        return;
    };
    let universe = default_universe_root();
    let dir = universe.join(".eustress");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("StreamNodePlugin: failed to create .eustress dir for port file: {}", e);
        return;
    }
    let path = dir.join("engine.stream.port");
    if let Err(e) = std::fs::write(&path, addr.port().to_string()) {
        warn!("StreamNodePlugin: failed to write {}: {}", path.display(), e);
        return;
    }
    info!(
        "🔗 StreamNode TCP port {} advertised at {}",
        addr.port(),
        path.display()
    );
}

/// Best-effort lookup of the active Universe root for advertising the
/// stream-node port. Delegates to
/// [`crate::space::best_default_universe_root`] (recorded
/// `.default_universe` → first real Universe → `Universe1`), which routes
/// through `workspace_root` and so bypasses OneDrive's redirected Documents
/// folder — calling `dirs::document_dir()` directly (the old code) put the
/// port file in `OneDrive\Documents`, where no sibling looks. A bare
/// `first_universe_root()` would also misfire here: the workspace holds
/// several universe dirs, and the loaded one (`Universe1`) is not always
/// the alphabetically-first.
fn default_universe_root() -> std::path::PathBuf {
    if let Ok(env_path) = std::env::var("EUSTRESS_UNIVERSE_ROOT") {
        return std::path::PathBuf::from(env_path);
    }
    crate::space::best_default_universe_root()
}

// ─────────────────────────────────────────────────────────────────────────────
// startup system
// ─────────────────────────────────────────────────────────────────────────────

/// The configured-only guard. A node starts only when the certificate, the
/// key and the parties file are all named and pass the node's own startup
/// checks: the files load, the key, the parties file and its folder are
/// owner-only, and a non-loopback bind has an enabled party. The node repeats
/// the checks when it starts, so either layer alone refuses.
fn configured(cfg: &StreamNodeConfig) -> Result<(), String> {
    let (Some(cert), Some(key), Some(parties)) = (&cfg.tls_cert, &cfg.tls_key, &cfg.parties_path) else {
        return Err("tls_cert, tls_key and parties_path are not all set".to_string());
    };
    let probe = NodeConfig { bind_addr: cfg.bind_addr, ..Default::default() }
        .with_tls(cert.clone(), key.clone())
        .with_parties(parties.clone());
    NodeAuth::from_config(&probe).map(|_| ()).map_err(|e| e.to_string())
}

/// Start the configured nodes, and REST when `rest` is set, and keep the
/// runtime they run on in the returned handle.
fn start_nodes(stream: EustressStream, cfg: StreamNodeConfig) -> Option<StreamNodeHandle> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    // `NodeServer::start` spawns its accept loops on the runtime it runs on,
    // so that runtime must outlive startup: one dropped when startup returns
    // cancels the loops and closes every listener. This runtime runs on its
    // own thread, held in `block_on` until the handle is dropped.
    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        rt.block_on(async move {
            let mut nodes = Vec::with_capacity(cfg.cluster_nodes as usize);
            let mut addrs = Vec::with_capacity(cfg.cluster_nodes as usize);
            let mut rest_servers: Vec<RestServer> = Vec::new();

            for i in 0..cfg.cluster_nodes {
                let port = cfg.base_port.saturating_add(i as u16);
                let rest_port = if cfg.rest {
                    Some(port + 10_000)
                } else {
                    None
                };

                let node_config = NodeConfig {
                    bind_addr: cfg.bind_addr,
                    port,
                    rest_port,
                    max_connections: cfg.max_connections,
                    auto_increment: false, // hard-bind to the configured port
                    node_id: format!("engine-node-{i}"),
                    tls_cert: cfg.tls_cert.clone(),
                    tls_key: cfg.tls_key.clone(),
                    parties_path: cfg.parties_path.clone(),
                    ..Default::default()
                };
                let rest_config = node_config.clone();

                match NodeServer::start(stream.clone(), node_config).await {
                    Ok(node) => {
                        let addr = node.listen_addr;
                        info!("StreamNodePlugin: node {i} listening on {addr}");
                        // REST shares the node's TLS configuration and its
                        // parties, sessions and tokens.
                        if cfg.rest {
                            let state = RestState::new(
                                stream.clone(),
                                Arc::clone(node.auth()),
                                addr.port(),
                                rest_config.effective_rest_port(),
                            );
                            match RestServer::bind(state, &rest_config).await {
                                Ok(rest) => rest_servers.push(rest),
                                Err(e) => warn!("StreamNodePlugin: REST for node {i} did not start: {e}"),
                            }
                        }
                        addrs.push(addr);
                        nodes.push(node);
                    }
                    Err(e) => {
                        warn!("StreamNodePlugin: failed to start node {i}: {e}");
                    }
                }
            }

            let _ = ready_tx.send((nodes, addrs));
            // The accept loops and the REST servers run on this runtime.
            // Holding it here until the handle is dropped keeps them alive.
            let _ = stop_rx.await;
            drop(rest_servers);
        });
    });

    let (nodes, addrs) = ready_rx.recv().unwrap_or_else(|_| (vec![], vec![]));
    let runtime = NodeRuntime { stop: Some(stop_tx), thread: Some(thread) };
    if nodes.is_empty() {
        return None;
    }
    Some(StreamNodeHandle { nodes, addrs, _runtime: runtime })
}

fn start_stream_nodes(
    config: Res<StreamNodeConfig>,
    queue: Res<ChangeQueue>,
    mut commands: Commands,
) {
    let stream = queue.stream.clone();
    let cfg = config.clone();

    // Configured only: no listener starts unless the certificate, the key
    // and the parties file are all named and pass the node's startup checks.
    if let Err(why) = configured(&cfg) {
        info!("StreamNodePlugin: no stream node started: {why}");
        commands.insert_resource(StreamNodeStatus::NotConfigured(why));
        return;
    }

    let Some(handle) = start_nodes(stream, cfg) else {
        warn!("StreamNodePlugin: no nodes started — remote pub/sub unavailable.");
        commands.insert_resource(StreamNodeStatus::Failed("no node started; see the log".to_string()));
        return;
    };
    let (nodes, addrs) = (&handle.nodes, &handle.addrs);

    if !config.bind_addr.is_loopback() {
        info!(
            "StreamNodePlugin: bound to {}. Every connection logs in over TLS 1.3 \
and is held to its party's grants in parties.toml.",
            config.bind_addr
        );
    }

    info!(
        "StreamNodePlugin: {} node(s) online. Primary TCP {}{}",
        nodes.len(),
        addrs.first().map(|a| a.to_string()).unwrap_or_default(),
        if config.rest {
            format!(", REST https://{}", {
                let mut a = addrs[0];
                a.set_port(addrs[0].port() + 10_000);
                a
            })
        } else {
            String::new()
        }
    );

    commands.insert_resource(StreamNodeStatus::Running);
    commands.insert_resource(handle);
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_stream_node::{ClientAuth, SpkiPin, StreamConfig, StreamNodeClient, UserKeypair};

    /// A certificate, an owner-only key and parties file, and one enrolled
    /// party, in a fresh folder under the system temp folder.
    struct TestFiles {
        dir: PathBuf,
        cert: PathBuf,
        key: PathBuf,
        parties: PathBuf,
        pin: SpkiPin,
        party: Arc<UserKeypair>,
    }

    impl TestFiles {
        fn new(tag: &str) -> TestFiles {
            let dir = std::env::temp_dir().join(format!("eustress-stream-plugin-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("test folder");
            let private = dir.join("private");
            crate::private_file::private_dir(&private).expect("owner-only folder");
            // Test code generates a certificate. Production code never does.
            let certified = rcgen::generate_simple_self_signed(vec!["eustress-node".to_string()])
                .expect("test certificate");
            let cert = dir.join("node.crt");
            std::fs::write(&cert, certified.cert.pem()).expect("write the certificate");
            let key = private.join("node.key");
            crate::private_file::write_private(&key, certified.key_pair.serialize_pem().as_bytes())
                .expect("write the key owner-only");
            let pin = SpkiPin::of_certificate(certified.cert.der()).expect("pin of the test certificate");
            let party = Arc::new(UserKeypair::generate());
            let parties_toml = format!(
                "[[party]]\nid = \"engine-test\"\npublic_key = \"{}\"\nenabled = true\n",
                party.public_key_b64()
            );
            let parties = private.join("parties.toml");
            crate::private_file::write_private(&parties, parties_toml.as_bytes())
                .expect("write parties.toml owner-only");
            TestFiles { dir, cert, key, parties, pin, party }
        }
    }

    impl Drop for TestFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A port nothing listens on right now.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port");
        listener.local_addr().expect("local addr").port()
    }

    /// After the startup path returns, a pinned client can log in: the
    /// runtime the accept loop runs on outlives startup. Fails when the
    /// runtime is dropped at the end of startup, because the listener then
    /// closes and the connect is refused.
    #[test]
    fn a_pinned_client_logs_in_after_startup_returns() {
        let files = TestFiles::new("runtime");
        let port = free_port();
        let cfg = StreamNodeConfig {
            base_port: port,
            rest: false,
            tls_cert: Some(files.cert.clone()),
            tls_key: Some(files.key.clone()),
            parties_path: Some(files.parties.clone()),
            ..StreamNodeConfig::default()
        };
        configured(&cfg).expect("the test files pass the configured-only guard");
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        let handle = start_nodes(stream, cfg).expect("the configured node starts");
        assert_eq!(handle.primary_addr().map(|a| a.port()), Some(port));

        // Startup has returned. A client on a runtime of its own connects now.
        let client_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("client runtime");
        let auth = ClientAuth::new("engine-test", Arc::clone(&files.party), files.pin);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let login = client_rt.block_on(StreamNodeClient::connect(addr, &auth));
        assert!(login.is_ok(), "a pinned client must log in after startup returned: {:?}", login.err());
        drop(handle);
    }

    /// A default-configured plugin opens no stream port: the
    /// `StreamNodeHandle` resource is absent and a TCP connect to the
    /// configured port is refused. The status says the plugin was not
    /// configured, which is what fails when the configured-only guard is
    /// removed: the node's own startup check then refuses instead, and the
    /// status reads `Failed`.
    #[test]
    fn a_default_configured_plugin_opens_no_stream_port() {
        let port = free_port();
        let queue = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(eustress_common::change_queue::init_change_queue(
                &eustress_common::change_queue::ChangeQueueConfig::default(),
            ))
            .expect("change queue");
        let mut app = App::new();
        app.insert_resource(queue);
        app.add_plugins(StreamNodePlugin::with_config(StreamNodeConfig {
            base_port: port,
            ..StreamNodeConfig::default()
        }));
        app.update();

        assert!(app.world().get_resource::<StreamNodeHandle>().is_none(), "no StreamNodeHandle");
        match app.world().get_resource::<StreamNodeStatus>() {
            Some(StreamNodeStatus::NotConfigured(_)) => {}
            other => panic!("expected the plugin to report NotConfigured, got {other:?}"),
        }
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
            "nothing may listen on the configured port"
        );
    }
}
