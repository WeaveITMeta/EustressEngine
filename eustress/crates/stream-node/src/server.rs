//! TCP listener: accepts connections and spawns a `ConnectionHandler` per socket.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{info, warn};

use eustress_stream::EustressStream;

use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::handler::handle_connection;

/// State shared across the node.
pub struct NodeServer {
    pub stream: EustressStream,
    pub config: Arc<NodeConfig>,
    pub listen_addr: SocketAddr,
    pub shutdown_tx: broadcast::Sender<()>,
}

impl NodeServer {
    /// Bind to the configured port (auto-incrementing if `auto_increment = true`)
    /// and start accepting connections in a background task.
    ///
    /// Returns the resolved listen address.
    pub async fn start(stream: EustressStream, config: NodeConfig) -> Result<Arc<Self>, NodeError> {
        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let config = Arc::new(config);

        let listener = bind_with_auto_increment(&config).await?;
        let listen_addr = listener.local_addr()?;

        info!("EustressStream node listening on {listen_addr}");

        let server = Arc::new(NodeServer {
            stream: stream.clone(),
            config: Arc::clone(&config),
            listen_addr,
            shutdown_tx: shutdown_tx.clone(),
        });

        let srv = Arc::clone(&server);
        tokio::spawn(async move {
            srv.accept_loop(listener).await;
        });

        Ok(server)
    }

    async fn accept_loop(&self, listener: TcpListener) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((socket, _)) => {
                            let stream = self.stream.clone();
                            let max_frame = self.config.frame_max_bytes;
                            let cap = self.config.connection_channel_capacity;
                            tokio::spawn(async move {
                                handle_connection(socket, stream, max_frame, cap).await;
                            });
                        }
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

    /// Signal all tasks to shut down.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn bind_with_auto_increment(config: &NodeConfig) -> Result<TcpListener, NodeError> {
    let mut port = config.port;

    loop {
        let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
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
