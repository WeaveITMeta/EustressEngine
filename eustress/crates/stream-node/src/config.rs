use eustress_stream::StreamConfig;

/// Configuration for a single EustressStream network node.
#[derive(Debug, Clone)]
pub struct NodeConfig {
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
    pub frame_max_bytes: usize,
    /// Node identifier (used in cluster consistent hash ring).
    pub node_id: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl NodeConfig {
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

    /// Resolved REST port: explicit if set, otherwise `port + 10000`.
    pub fn effective_rest_port(&self) -> u16 {
        self.rest_port.unwrap_or_else(|| self.port + 10000)
    }
}
