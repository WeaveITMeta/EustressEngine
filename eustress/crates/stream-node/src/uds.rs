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
use dashmap::DashMap;
use std::collections::VecDeque;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use eustress_stream::EustressStream;

use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::protocol::{
    read_client_frame, read_server_frame, write_client_frame, write_server_frame,
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

    loop {
        let frame = match read_client_frame(&mut reader, max_frame).await {
            Ok(f) => f,
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { debug!("UDS recv error: {e}"); break; }
        };

        let reply = dispatch_uds_frame(frame, &stream, cap, Arc::clone(&writer)).await;
        if let Some(reply) = reply {
            let mut guard = writer.lock().await;
            if write_server_frame(&mut *guard, &reply).await.is_err() {
                break;
            }
        }
    }
}

async fn dispatch_uds_frame(
    frame: ClientFrame,
    stream: &EustressStream,
    cap: usize,
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
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
            let _ = stream.subscribe_owned(&topic, move |msg| {
                let _ = tx.try_send(msg);
            });

            if let Some(off) = from_offset {
                let w = Arc::clone(&writer);
                stream.replay_ring(&topic, off, |view| {
                    let f = ServerFrame::Message {
                        topic: view.topic.to_string(),
                        offset: view.offset,
                        timestamp: view.timestamp,
                        payload: view.data.to_vec(),
                    };
                    let w2 = Arc::clone(&w);
                    tokio::spawn(async move {
                        let mut g = w2.lock().await;
                        let _ = write_server_frame(&mut *g, &f).await;
                    });
                });
            }

            tokio::spawn(async move {
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
            let offsets: Vec<u64> = payloads.into_iter()
                .map(|p| stream.producer(&topic).send_bytes(Bytes::from(p)))
                .collect();
            Some(ServerFrame::BatchAck { offsets })
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
    writer: Mutex<tokio::net::unix::OwnedWriteHalf>,
    ack_queue: Mutex<VecDeque<oneshot::Sender<u64>>>,
    batch_ack_queue: Mutex<VecDeque<oneshot::Sender<Vec<u64>>>>,
    topic_list_waiter: Mutex<Option<oneshot::Sender<Vec<TopicStats>>>>,
    sub_channels: DashMap<String, mpsc::Sender<(u64, u64, Vec<u8>)>>,
}

impl UnixNodeClient {
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, NodeError> {
        let socket = UnixStream::connect(socket_path).await.map_err(NodeError::Io)?;
        let (read_half, write_half) = socket.into_split();

        let inner = Arc::new(UnixClientInner {
            writer: Mutex::new(write_half),
            ack_queue: Mutex::new(VecDeque::new()),
            batch_ack_queue: Mutex::new(VecDeque::new()),
            topic_list_waiter: Mutex::new(None),
            sub_channels: DashMap::new(),
        });

        let inner_ref = Arc::clone(&inner);
        tokio::spawn(async move {
            uds_client_reader(read_half, inner_ref).await;
        });

        Ok(UnixNodeClient { inner })
    }

    /// Publish bytes to a topic. Returns the assigned offset.
    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<u64>();
        self.inner.ack_queue.lock().await.push_back(tx);
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(
            &mut *writer,
            &ClientFrame::Publish { topic: topic.to_string(), payload: payload.to_vec() },
        ).await?;
        drop(writer);
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    /// Publish a batch of `(topic, payload)` pairs in one round trip.
    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Vec<u64>>();
        self.inner.batch_ack_queue.lock().await.push_back(tx);
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(
            &mut *writer,
            &ClientFrame::PublishBatch {
                messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
            },
        ).await?;
        drop(writer);
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    /// Subscribe to a topic. Returns a receiver for `(offset, timestamp, payload)` triples.
    pub async fn subscribe(
        &self,
        topic: &str,
        from_offset: Option<u64>,
    ) -> Result<mpsc::Receiver<(u64, u64, Vec<u8>)>, NodeError> {
        let (tx, rx) = mpsc::channel(4096);
        self.inner.sub_channels.insert(topic.to_string(), tx);
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(
            &mut *writer,
            &ClientFrame::Subscribe { topic: topic.to_string(), from_offset },
        ).await?;
        Ok(rx)
    }

    /// Ping the node.
    pub async fn ping(&self) -> Result<(), NodeError> {
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(&mut *writer, &ClientFrame::Ping).await
    }
}

async fn uds_client_reader(
    mut reader: tokio::net::unix::OwnedReadHalf,
    inner: Arc<UnixClientInner>,
) {
    loop {
        match read_server_frame(&mut reader, MAX_FRAME).await {
            Ok(ServerFrame::Ack { offset }) => {
                if let Some(tx) = inner.ack_queue.lock().await.pop_front() {
                    let _ = tx.send(offset);
                }
            }
            Ok(ServerFrame::BatchAck { offsets }) => {
                if let Some(tx) = inner.batch_ack_queue.lock().await.pop_front() {
                    let _ = tx.send(offsets);
                }
            }
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(ch) = inner.sub_channels.get(&topic) {
                    let _ = ch.try_send((offset, timestamp, payload));
                }
            }
            Ok(ServerFrame::TopicList(list)) => {
                if let Some(tx) = inner.topic_list_waiter.lock().await.take() {
                    let _ = tx.send(list);
                }
            }
            Ok(ServerFrame::BatchAckCompact { first_offset, count }) => {
                // Compact batch ack — expand to offsets if needed
                let offsets: Vec<u64> = (0..count).map(|i| first_offset + i as u64).collect();
                if let Some(tx) = inner.batch_ack_queue.lock().await.pop_front() {
                    let _ = tx.send(offsets);
                }
            }
            Ok(ServerFrame::Pong) => {}
            Ok(ServerFrame::Error { code, message }) => {
                warn!("UDS server error {code}: {message}");
            }
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => { warn!("UDS client reader error: {e}"); break; }
        }
    }
}
