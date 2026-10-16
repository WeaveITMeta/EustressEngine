//! Unix Domain Socket transport for EustressStream nodes.
//!
//! Uses `tokio::net::UnixListener` / `UnixStream` — no TCP/IP stack, no port
//! allocation, no Nagle. The kernel copies data directly between processes via
//! a named socket file, giving 2–5× lower latency than loopback TCP on Linux.
//!
//! ## Platform
//!
//! Unix only (`cfg(unix)`). On macOS and Linux this achieves sub-50 µs RTT
//! even with TLS disabled. Windows users should use TCP loopback instead.
//!
//! ## Benches only
//!
//! This transport has no TLS, no login and no grants: any process that can
//! connect to the socket file may publish to and subscribe to every topic.
//! It compiles only under the non-default `bench-transports` feature, which
//! no shipped build enables (see `docs/architecture/STREAM_NODE_AUTH.md`). A
//! `Hello` frame is answered with an error.
//!
//! ## Wire format
//!
//! Identical to the TCP protocol: `[ 8-byte LE length ][ bincode(Frame) ]`.
//! Clients and servers are drop-in replacements for `StreamNode`/`StreamNodeClient`.
//!
//! ## Quick start
//!
//! ```rust,no_run,ignore
//! # #[cfg(unix)]
//! # {
//! use eustress_stream_node::uds::{UnixNode, UnixNodeClient};
//! use eustress_stream_node::NodeConfig;
//!
//! #[tokio::main]
//! async fn main() {
//!     let node = UnixNode::start("/tmp/eustress.sock", NodeConfig::default()).await.unwrap();
//!     let client = UnixNodeClient::connect("/tmp/eustress.sock").await.unwrap();
//!     let offset = client.publish("events", bytes::Bytes::from_static(b"hello")).await.unwrap();
//! }
//! # }
//! ```

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use eustress_stream::{EustressStream, SubscriberId};

use crate::config::NodeConfig;
use crate::client::{close_conn, send_frame, spawn_writer, Conn, Outgoing, Pending};
use crate::error::NodeError;
use crate::protocol::{
    read_client_frame, read_server_frame, write_server_frame,
    ClientFrame, ServerFrame, TopicStats,
};

const MAX_FRAME: usize = 16 * 1024 * 1024;

// ─────────────────────────────────────────────────────────────────────────────
// UnixNode — server side
// ─────────────────────────────────────────────────────────────────────────────

/// An EustressStream node bound to a Unix domain socket.
pub struct UnixNode {
    pub stream: EustressStream,
    pub socket_path: PathBuf,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

impl UnixNode {
    /// Start a node at `socket_path`.
    ///
    /// Any existing socket file at that path is removed first.
    pub async fn start(
        socket_path: impl AsRef<Path>,
        config: NodeConfig,
    ) -> Result<Arc<Self>, NodeError> {
        let path = socket_path.as_ref().to_path_buf();

        // Remove stale socket file.
        let _ = std::fs::remove_file(&path);

        let listener = UnixListener::bind(&path)
            .map_err(NodeError::Io)?;

        info!("EustressStream UDS node at {}", path.display());

        let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let stream = EustressStream::new(config.stream_config.clone());

        let node = Arc::new(UnixNode {
            stream: stream.clone(),
            socket_path: path,
            shutdown_tx: shutdown_tx.clone(),
        });

        let cap = config.connection_channel_capacity;
        let max_frame = config.frame_max_bytes;
        let node_ref = Arc::clone(&node);
        tokio::spawn(async move {
            node_ref.accept_loop(listener, cap, max_frame).await;
        });

        Ok(node)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        let _ = std::fs::remove_file(&self.socket_path);
    }

    async fn accept_loop(&self, listener: UnixListener, cap: usize, max_frame: usize) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((socket, _)) => {
                            let stream = self.stream.clone();
                            tokio::spawn(async move {
                                handle_uds_connection(socket, stream, cap, max_frame).await;
                            });
                        }
                        Err(e) => warn!("UDS accept error: {e}"),
                    }
                }
                _ = shutdown_rx.recv() => break,
            }
        }
    }
}

async fn handle_uds_connection(
    socket: UnixStream,
    stream: EustressStream,
    cap: usize,
    max_frame: usize,
) {
    let (mut reader, write_half) = socket.into_split();
    let writer = Arc::new(Mutex::new(write_half));
    // This connection's subscriptions, removed when it ends.
    let mut subscriptions: Vec<(String, SubscriberId)> = Vec::new();

    loop {
        let frame = match read_client_frame(&mut reader, max_frame).await {
            Ok(f) => f,
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { debug!("UDS recv error: {e}"); break; }
        };

        let reply = dispatch_uds_frame(frame, &stream, cap, Arc::clone(&writer), &mut subscriptions).await;
        if let Some(reply) = reply {
            let mut guard = writer.lock().await;
            if write_server_frame(&mut *guard, &reply).await.is_err() {
                break;
            }
        }
    }

    // The peer is gone: its subscriptions end with it, so they hold no
    // subscriber slot, and their forwarding tasks stop once their channels
    // close.
    for (topic, id) in subscriptions {
        stream.unsubscribe(&topic, id);
    }
}

async fn dispatch_uds_frame(
    frame: ClientFrame,
    stream: &EustressStream,
    cap: usize,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    subscriptions: &mut Vec<(String, SubscriberId)>,
) -> Option<ServerFrame> {
    match frame {
        ClientFrame::Publish { topic, payload } => {
            let offset = stream.producer(&topic).send_bytes(Bytes::from(payload));
            Some(ServerFrame::Ack { offset })
        }
        ClientFrame::PublishBatch { messages } => {
            let offsets: Vec<u64> = messages.into_iter()
                .map(|(t, p)| stream.producer(&t).send_bytes(Bytes::from(p)))
                .collect();
            Some(ServerFrame::BatchAck { offsets })
        }
        ClientFrame::Subscribe { topic, from_offset } => {
            let (tx, mut rx) = mpsc::channel(cap);
            // One reply per request: `Error` when the topic is full.
            let id = match stream.subscribe_owned(&topic, move |msg| {
                let _ = tx.try_send(msg);
            }) {
                Ok(id) => id,
                Err(e) => {
                    return Some(ServerFrame::Error {
                        code: crate::auth::ERR_TOPIC_FULL,
                        message: e.to_string(),
                    });
                }
            };
            subscriptions.push((topic.clone(), id));

            // `Subscribed` goes out here, before any Message for this
            // subscription: the replay and the forwarding task come after it.
            {
                let head = stream.head(&topic);
                let mut g = writer.lock().await;
                let _ = write_server_frame(&mut *g, &ServerFrame::Subscribed { head }).await;
            }

            let mut backlog = Vec::new();
            if let Some(off) = from_offset {
                stream.replay_ring(&topic, off, |view| {
                    let f = ServerFrame::Message {
                        topic: view.topic.to_string(),
                        offset: view.offset,
                        timestamp: view.timestamp,
                        payload: view.data.to_vec(),
                    };
                    backlog.push(f);
                });
            }

            tokio::spawn(async move {
                for f in backlog {
                    let mut g = writer.lock().await;
                    if write_server_frame(&mut *g, &f).await.is_err() {
                        return;
                    }
                }
                while let Some(msg) = rx.recv().await {
                    let f = ServerFrame::Message {
                        topic: msg.topic.clone(),
                        offset: msg.offset,
                        timestamp: msg.timestamp,
                        payload: msg.data.to_vec(),
                    };
                    let mut g = writer.lock().await;
                    if write_server_frame(&mut *g, &f).await.is_err() {
                        break;
                    }
                }
            });
            None
        }
        ClientFrame::ListTopics => {
            let stats: Vec<TopicStats> = stream.topics().into_iter().map(|name| {
                let head = stream.head(&name);
                let subscriber_count = stream.subscriber_count(&name);
                TopicStats { name, head, subscriber_count }
            }).collect();
            Some(ServerFrame::TopicList(stats))
        }
        ClientFrame::PublishBatchTopic { topic, payloads } => {
            // The reply of the kind this request expects, as on TCP and QUIC.
            let count = payloads.len() as u32;
            let producer = stream.producer(&topic);
            let mut first_offset = 0u64;
            for (i, p) in payloads.into_iter().enumerate() {
                let offset = producer.send_bytes(Bytes::from(p));
                if i == 0 {
                    first_offset = offset;
                }
            }
            Some(ServerFrame::BatchAckCompact { first_offset, count })
        }
        ClientFrame::PublishNoAck { topic, payload } => {
            stream.producer(&topic).send_bytes(Bytes::from(payload));
            None
        }
        ClientFrame::PublishBatchNoAck { topic, payloads } => {
            for p in payloads {
                stream.producer(&topic).send_bytes(Bytes::from(p));
            }
            None
        }
        ClientFrame::Ping => Some(ServerFrame::Pong),
        ClientFrame::Unsubscribe { .. } => None,
        ClientFrame::Hello { .. } => Some(ServerFrame::Error {
            code: crate::auth::ERR_BAD_REQUEST,
            message: "the Unix socket transport has no login".to_string(),
        }),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UnixNodeClient — client side
// ─────────────────────────────────────────────────────────────────────────────

/// Client connected to a `UnixNode` over a Unix domain socket.
///
/// Clone-able — all clones share the same socket connection.
#[derive(Clone)]
pub struct UnixNodeClient {
    inner: Arc<UnixClientInner>,
}

struct UnixClientInner {
    // The write half, the reply queue and the subscriber channels.
    conn: Arc<Conn<tokio::net::unix::OwnedWriteHalf>>,
    // The writer task's channel: every frame goes out through it.
    outgoing: mpsc::Sender<Outgoing>,
}

impl UnixNodeClient {
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, NodeError> {
        let socket = UnixStream::connect(socket_path).await.map_err(NodeError::Io)?;
        let (read_half, write_half) = socket.into_split();

        let conn = Conn::new(write_half);
        let outgoing = spawn_writer(Arc::clone(&conn));
        let reader_conn = Arc::clone(&conn);
        tokio::spawn(async move {
            uds_client_reader(read_half, reader_conn).await;
        });

        Ok(UnixNodeClient { inner: Arc::new(UnixClientInner { conn, outgoing }) })
    }

    /// Publish bytes to a topic. Returns the assigned offset.
    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<u64, NodeError>>();
        self.request(
            ClientFrame::Publish { topic: topic.to_string(), payload: payload.to_vec() },
            Pending::Ack(tx),
        ).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Publish a batch of `(topic, payload)` pairs in one round trip.
    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Result<Vec<u64>, NodeError>>();
        self.request(
            ClientFrame::PublishBatch {
                messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
            },
            Pending::BatchAck(tx),
        ).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Subscribe to a topic. Returns a receiver for `(offset, timestamp, payload)` triples.
    pub async fn subscribe(
        &self,
        topic: &str,
        from_offset: Option<u64>,
    ) -> Result<mpsc::Receiver<(u64, u64, Vec<u8>)>, NodeError> {
        let (tx, rx) = mpsc::channel(4096);
        // The reader registers the channel when `Subscribed` arrives; a
        // Message before it is dropped, and a refusal registers nothing.
        let (done, subscribed) = oneshot::channel::<Result<u64, NodeError>>();
        self.request(
            ClientFrame::Subscribe { topic: topic.to_string(), from_offset },
            Pending::Subscribed { topic: topic.to_string(), channel: tx, done },
        )
        .await?;
        subscribed.await.map_err(|_| NodeError::ConnectionClosed)??;
        Ok(rx)
    }

    /// Ping the node.
    /// Returns when the node's `Pong` arrives.
    pub async fn ping(&self) -> Result<(), NodeError> {
        let (tx, rx) = oneshot::channel::<Result<(), NodeError>>();
        self.request(ClientFrame::Ping, Pending::Pong(tx)).await?;
        rx.await.map_err(|_| NodeError::ConnectionClosed)?
    }

    /// Hand a request to the writer task, which queues `pending` and then
    /// writes the frame, with nothing in between.
    async fn request(&self, frame: ClientFrame, pending: Pending) -> Result<(), NodeError> {
        send_frame(&self.inner.outgoing, frame, Some(pending)).await
    }
}

async fn uds_client_reader(
    mut reader: tokio::net::unix::OwnedReadHalf,
    conn: Arc<Conn<tokio::net::unix::OwnedWriteHalf>>,
) {
    loop {
        match read_server_frame(&mut reader, MAX_FRAME).await {
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(ch) = conn.sub_channels.get(&topic) {
                    let _ = ch.try_send((offset, timestamp, payload));
                }
            }
            // Every other frame is the reply to the front request.
            Ok(reply) => {
                if let Err(e) = conn.pending.lock().await.answer(reply, &conn.sub_channels) {
                    warn!("UDS: {e}; closing");
                    break;
                }
            }
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { warn!("UDS client reader error: {e}"); break; }
        }
    }
    close_conn(&conn).await;
}
