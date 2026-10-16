//! TCP listener: accepts connections and spawns a `ConnectionHandler` per socket.
//!
//! Startup runs the node's checks first ([`NodeAuth::from_config`]). Each
//! accepted socket takes a pre-login slot (128 in all, 16 per source IP) or
//! is refused at accept, then logs in over TLS 1.3 within 5 seconds.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::Instant;
use tracing::{info, warn};

use eustress_stream::EustressStream;

use crate::auth::{Channel, NodeAuth};
use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::handler::handle_connection;
use crate::handler::ConnectionContext;

/// State shared across the node.
pub struct NodeServer {
    pub stream: EustressStream,
    pub config: Arc<NodeConfig>,
    pub listen_addr: SocketAddr,
    pub shutdown_tx: broadcast::Sender<()>,
    auth: Arc<NodeAuth>,
    ctx: Arc<ConnectionContext>,
}

impl NodeServer {
    /// Bind to the configured port (auto-incrementing if `auto_increment = true`)
    /// and start accepting connections in a background task.
    ///
    /// Returns the resolved listen address.
    ///
    /// The startup checks run first. The node refuses, with no listener
    /// started, when the TLS certificate or key is missing, a file fails the
    /// owner-only check, `parties.toml` does not parse, or a non-loopback bind
    /// has no enabled party (see [`NodeAuth::from_config`]).
    pub async fn start(stream: EustressStream, config: NodeConfig) -> Result<Arc<Self>, NodeError> {
        let auth = NodeAuth::from_config(&config)?;
        Self::start_with_auth(stream, config, auth).await
    }

    /// Start with authentication state that already passed the startup
    /// checks, so a REST listener on the same node can share it.
    pub async fn start_with_auth(
        stream: EustressStream,
        config: NodeConfig,
        auth: Arc<NodeAuth>,
    ) -> Result<Arc<Self>, NodeError> {
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let config = Arc::new(config);

        let listener = bind_with_auto_increment(&config).await?;
        let listen_addr = listener.local_addr()?;

        info!("EustressStream node listening on {listen_addr}");
        info!("Every connection logs in over TLS 1.3. Clients pin SPKI SHA-256 {}", auth.tls().spki_pin());

        let ctx = ConnectionContext::new(
            stream.clone(),
            Arc::clone(&auth),
            config.frame_max_bytes,
            config.connection_channel_capacity,
        );

        let server = Arc::new(NodeServer {
            stream: stream.clone(),
            config: Arc::clone(&config),
            listen_addr,
            shutdown_tx: shutdown_tx.clone(),
            auth: Arc::clone(&auth),
            ctx,
        });

        auth.spawn_revocation_task(Some(shutdown_tx.subscribe()));

        let srv = Arc::clone(&server);
        tokio::spawn(async move {
            srv.accept_loop(listener).await;
        });

        Ok(server)
    }

    /// The node's authentication and authorization state.
    pub fn auth(&self) -> &Arc<NodeAuth> {
        &self.auth
    }

    /// What every connection on this node shares. A caller holding a byte
    /// stream other than a TCP socket can pass it to
    /// [`crate::handler::handle_connection`] with a permit from
    /// `auth().pre_login`.
    pub fn connection_context(&self) -> Arc<ConnectionContext> {
        Arc::clone(&self.ctx)
    }

    async fn accept_loop(&self, listener: TcpListener) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((socket, peer)) => self.admit(socket, peer),
                        Err(e) => {
                            warn!("accept error: {e}");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("EustressStream node shutting down.");
                    break;
                }
            }
        }
    }

    /// Take a pre-login slot for the connection, or refuse it at accept.
    fn admit(&self, socket: TcpStream, peer: SocketAddr) {
        let accepted_at = Instant::now();
        match self.auth.pre_login.try_acquire(peer.ip()) {
            Some(permit) => {
                let ctx = Arc::clone(&self.ctx);
                tokio::spawn(async move {
                    handle_connection(socket, peer, permit, ctx, accepted_at).await;
                });
            }
            None => {
                self.auth.log_refused_at_accept(peer.ip(), Channel::Tcp);
                drop(socket);
            }
        }
    }

    /// Signal all tasks to shut down.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn bind_with_auto_increment(config: &NodeConfig) -> Result<TcpListener, NodeError> {
    let mut port = config.port;

    loop {
        let addr = SocketAddr::new(config.bind_addr, port);
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if is_addr_in_use(&e) && config.auto_increment && port < config.port_range_max => {
                port += 1;
            }
            Err(e) if is_addr_in_use(&e) => {
                return Err(NodeError::PortInUse(port));
            }
            Err(e) => return Err(NodeError::Io(e)),
        }
    }
}

fn is_addr_in_use(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::AddrInUse
}
