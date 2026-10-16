use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use eustress_stream::StreamConfig;

/// Configuration for a single EustressStream network node.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Interface to bind every listener to (TCP, QUIC, REST).
    ///
    /// Defaults to `127.0.0.1`, **loopback only**. Every connection logs in
    /// over TLS 1.3, loopback included, and each party may publish and
    /// subscribe only to the topics its `parties.toml` entry grants. A
    /// non-loopback bind additionally requires at least one enabled party;
    /// with none, the node refuses to start.
    ///
    /// Override with [`NodeConfig::with_bind_addr`], or with
    /// [`NodeConfig::bind_all`] once a trusted network boundary (VPN, firewall,
    /// authenticating reverse proxy) is actually in front of it.
    pub bind_addr: IpAddr,
    /// TCP port to listen on. Default: 33000.
    pub port: u16,
    /// Upper bound of the auto-increment port range. Default: 49151.
    pub port_range_max: u16,
    /// When true, try the next port if the configured port is in use.
    pub auto_increment: bool,
    /// REST/SSE HTTP port. Default: `port + 10000` (e.g. 43000 for node 33000).
    pub rest_port: Option<u16>,
    /// Number of nodes in a ForgeCluster. Default: 10.
    pub cluster_size: usize,
    /// Underlying EustressStream configuration.
    pub stream_config: StreamConfig,
    /// Maximum simultaneous TCP connections. Default: 4096.
    pub max_connections: usize,
    /// Per-connection outbound channel capacity (messages). Default: 8192.
    pub connection_channel_capacity: usize,
    /// Maximum inbound frame size in bytes. Default: 16 MiB.
    /// Frames before login are capped at 64 KB whatever this says.
    pub frame_max_bytes: usize,
    /// Node identifier (used in cluster consistent hash ring).
    /// Every login transcript carries it too, so it is at most 64 bytes.
    pub node_id: String,
    /// PEM certificate every listener presents (TCP, QUIC, REST). Required:
    /// without it the node does not start its listeners. The node never
    /// generates a certificate.
    pub tls_cert: Option<PathBuf>,
    /// PEM private key for `tls_cert`. Required. It is opened with the
    /// owner-only check (`eustress_identity::private_file::verify_open`) and
    /// read through the handle that check returns.
    pub tls_key: Option<PathBuf>,
    /// `parties.toml`: the parties enrolled to log in, with their grants.
    /// Required. The file and its folder must pass the owner-only check.
    pub parties_path: Option<PathBuf>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 33000,
            port_range_max: 49151,
            auto_increment: true,
            rest_port: None,
            cluster_size: 10,
            stream_config: StreamConfig::default().in_memory(),
            max_connections: 4096,
            connection_channel_capacity: 8192,
            frame_max_bytes: 16 * 1024 * 1024,
            node_id: format!("node-{}", std::process::id()),
            tls_cert: None,
            tls_key: None,
            parties_path: None,
        }
    }
}

impl NodeConfig {
    /// Bind every listener to `addr` instead of loopback.
    pub fn with_bind_addr(mut self, addr: IpAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Bind to all interfaces (`0.0.0.0`).
    ///
    /// Every connection still logs in and is held to its grants, and the node
    /// refuses to start unless at least one party is enabled.
    pub fn bind_all(mut self) -> Self {
        self.bind_addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        self
    }

    /// True when [`Self::bind_addr`] is reachable from off-host.
    pub fn is_externally_bound(&self) -> bool {
        !self.bind_addr.is_loopback()
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_rest_port(mut self, port: u16) -> Self {
        self.rest_port = Some(port);
        self
    }

    pub fn with_stream_config(mut self, cfg: StreamConfig) -> Self {
        self.stream_config = cfg;
        self
    }

    pub fn with_cluster_size(mut self, n: usize) -> Self {
        self.cluster_size = n;
        self
    }

    pub fn with_node_id(mut self, id: impl Into<String>) -> Self {
        self.node_id = id.into();
        self
    }

    /// The PEM certificate and private key every listener uses.
    pub fn with_tls(mut self, cert: impl Into<PathBuf>, key: impl Into<PathBuf>) -> Self {
        self.tls_cert = Some(cert.into());
        self.tls_key = Some(key.into());
        self
    }

    /// The `parties.toml` the node enrolls parties from.
    pub fn with_parties(mut self, path: impl Into<PathBuf>) -> Self {
        self.parties_path = Some(path.into());
        self
    }

    /// Resolved REST port: explicit if set, otherwise `port + 10000`.
    pub fn effective_rest_port(&self) -> u16 {
        self.rest_port.unwrap_or_else(|| self.port + 10000)
    }
}
