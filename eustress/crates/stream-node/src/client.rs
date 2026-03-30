//! `StreamNodeClient` — connect to a remote EustressStream node over TCP.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::warn;

use crate::error::NodeError;
use crate::protocol::{
    read_server_frame, write_client_frame, ClientFrame, ServerFrame, TopicStats,
};

const MAX_FRAME: usize = 16 * 1024 * 1024;

/// A client connected to a single EustressStream node.
///
/// Cheaply cloneable — all clones share the same TCP connection.
#[derive(Clone)]
pub struct StreamNodeClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    writer: Mutex<tokio::net::tcp::OwnedWriteHalf>,
    // Pending ack waiters — one-shot senders in FIFO order (TCP preserves order).
    ack_queue: Mutex<VecDeque<oneshot::Sender<u64>>>,
    // Pending batch ack waiters — parallel to ack_queue, carries Vec<u64>.
    batch_ack_queue: Mutex<VecDeque<oneshot::Sender<Vec<u64>>>>,
    // Pending compact batch ack waiters (PublishBatchTopic) — carries (first_offset, count).
    compact_ack_queue: Mutex<VecDeque<oneshot::Sender<(u64, u32)>>>,
    // Pending ListTopics waiter.
    topic_list_waiter: Mutex<Option<oneshot::Sender<Vec<TopicStats>>>>,
    // Per-topic subscriber channels.
    sub_channels: DashMap<String, mpsc::Sender<(u64, u64, Vec<u8>)>>,
}

impl StreamNodeClient {
    /// Connect to an EustressStream node.
    pub async fn connect(addr: SocketAddr) -> Result<Self, NodeError> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, write_half) = stream.into_split();

        let inner = Arc::new(ClientInner {
            writer: Mutex::new(write_half),
            ack_queue: Mutex::new(VecDeque::new()),
            batch_ack_queue: Mutex::new(VecDeque::new()),
            compact_ack_queue: Mutex::new(VecDeque::new()),
            topic_list_waiter: Mutex::new(None),
            sub_channels: DashMap::new(),
        });

        // Spawn reader task.
        let inner_ref = Arc::clone(&inner);
        tokio::spawn(async move {
            reader_task(read_half, inner_ref).await;
        });

        Ok(StreamNodeClient { inner })
    }

    /// Publish bytes to a topic. Returns the assigned offset.
    pub async fn publish(&self, topic: &str, payload: Bytes) -> Result<u64, NodeError> {
        let (tx, rx) = oneshot::channel::<u64>();
        {
            let mut queue = self.inner.ack_queue.lock().await;
            queue.push_back(tx);
        }
        {
            let mut writer = self.inner.writer.lock().await;
            write_client_frame(
                &mut *writer,
                &ClientFrame::Publish {
                    topic: topic.to_string(),
                    payload: payload.to_vec(),
                },
            )
            .await?;
        }
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    /// Publish a batch of `(topic, payload)` pairs in one round trip.
    /// Returns the assigned offsets in order.
    pub async fn publish_batch(&self, messages: Vec<(String, Bytes)>) -> Result<Vec<u64>, NodeError> {
        let (tx, rx) = oneshot::channel::<Vec<u64>>();
        {
            let mut queue = self.inner.batch_ack_queue.lock().await;
            queue.push_back(tx);
        }
        {
            let mut writer = self.inner.writer.lock().await;
            write_client_frame(
                &mut *writer,
                &ClientFrame::PublishBatch {
                    messages: messages.into_iter().map(|(t, p)| (t, p.to_vec())).collect(),
                },
            )
            .await?;
        }
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    /// **Zero-copy single-topic batch** — all payloads go to the same topic.
    ///
    /// Returns `(first_offset, count)`. Individual offsets are `first_offset + i`.
    /// Use this instead of `publish_batch` when all messages share one topic —
    /// the ack shrinks from `count × 8` bytes to 12 bytes fixed.
    pub async fn publish_batch_topic(
        &self,
        topic: &str,
        payloads: Vec<Bytes>,
    ) -> Result<(u64, u32), NodeError> {
        let (tx, rx) = oneshot::channel::<(u64, u32)>();
        {
            let mut queue = self.inner.compact_ack_queue.lock().await;
            queue.push_back(tx);
        }
        {
            let mut writer = self.inner.writer.lock().await;
            write_client_frame(
                &mut *writer,
                &ClientFrame::PublishBatchTopic {
                    topic: topic.to_string(),
                    payloads: payloads.into_iter().map(|p| p.to_vec()).collect(),
                },
            )
            .await?;
        }
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
            &ClientFrame::Subscribe {
                topic: topic.to_string(),
                from_offset,
            },
        )
        .await?;

        Ok(rx)
    }

    /// Unsubscribe from a topic.
    pub async fn unsubscribe(&self, topic: &str) -> Result<(), NodeError> {
        self.inner.sub_channels.remove(topic);
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(
            &mut *writer,
            &ClientFrame::Unsubscribe { topic: topic.to_string() },
        )
        .await
    }

    /// List topics on the connected node.
    pub async fn list_topics(&self) -> Result<Vec<TopicStats>, NodeError> {
        let (tx, rx) = oneshot::channel::<Vec<TopicStats>>();
        {
            let mut waiter = self.inner.topic_list_waiter.lock().await;
            *waiter = Some(tx);
        }
        {
            let mut writer = self.inner.writer.lock().await;
            write_client_frame(&mut *writer, &ClientFrame::ListTopics).await?;
        }
        rx.await.map_err(|_| NodeError::ConnectionClosed)
    }

    /// Ping the node.
    pub async fn ping(&self) -> Result<(), NodeError> {
        let mut writer = self.inner.writer.lock().await;
        write_client_frame(&mut *writer, &ClientFrame::Ping).await
    }
}

/// Background task: reads server frames and dispatches them.
async fn reader_task(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    inner: Arc<ClientInner>,
) {
    loop {
        match read_server_frame(&mut reader, MAX_FRAME).await {
            Ok(ServerFrame::Ack { offset }) => {
                let mut queue = inner.ack_queue.lock().await;
                if let Some(tx) = queue.pop_front() {
                    let _ = tx.send(offset);
                }
            }
            Ok(ServerFrame::Message { topic, offset, timestamp, payload }) => {
                if let Some(entry) = inner.sub_channels.get(&topic) {
                    if entry.try_send((offset, timestamp, payload)).is_err() {
                        // Channel full — drop (ring semantics).
                    }
                }
            }
            Ok(ServerFrame::BatchAck { offsets }) => {
                let mut queue = inner.batch_ack_queue.lock().await;
                if let Some(tx) = queue.pop_front() {
                    let _ = tx.send(offsets);
                }
            }
            Ok(ServerFrame::BatchAckCompact { first_offset, count }) => {
                let mut queue = inner.compact_ack_queue.lock().await;
                if let Some(tx) = queue.pop_front() {
                    let _ = tx.send((first_offset, count));
                }
            }
            Ok(ServerFrame::TopicList(list)) => {
                let mut waiter = inner.topic_list_waiter.lock().await;
                if let Some(tx) = waiter.take() {
                    let _ = tx.send(list);
                }
            }
            Ok(ServerFrame::Error { code, message }) => {
                warn!("server error {code}: {message}");
            }
            Ok(ServerFrame::Pong) => {}
            Err(NodeError::ConnectionClosed) => break,
            Err(e) => {
                warn!("client reader error: {e}");
                break;
            }
        }
    }
}
