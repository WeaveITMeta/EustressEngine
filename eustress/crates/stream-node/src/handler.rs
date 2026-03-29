//! Per-connection async task: reads `ClientFrame`s, dispatches to the
//! embedded `EustressStream`, and writes `ServerFrame` responses.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use eustress_stream::{EustressStream, OwnedMessage, SubscriberId};

use crate::error::NodeError;
use crate::protocol::{read_client_frame, write_server_frame, ClientFrame, ServerFrame, TopicStats};

/// Handle one accepted TCP connection.
pub async fn handle_connection(
    stream: tokio::net::TcpStream,
    eustress: EustressStream,
    max_frame: usize,
    channel_capacity: usize,
) {
    let peer = stream.peer_addr().ok();
    debug!("connection from {:?}", peer);

    let (read_half, write_half) = stream.into_split();
    let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

    let mut handler = ConnectionHandler {
        reader: read_half,
        writer: Arc::clone(&write_half),
        stream: eustress,
        subscriptions: HashMap::new(),
        max_frame,
        channel_capacity,
    };

    if let Err(e) = handler.run().await {
        match e {
            NodeError::ConnectionClosed => debug!("connection closed by {:?}", peer),
            other => warn!("connection error from {:?}: {other}", peer),
        }
    }

    // Unsubscribe all active subscriptions on disconnect.
    for (topic, id) in handler.subscriptions.drain() {
        handler.stream.unsubscribe(&topic, id);
    }

    debug!("connection from {:?} cleaned up", peer);
}

struct ConnectionHandler {
    reader: OwnedReadHalf,
    writer: Arc<tokio::sync::Mutex<OwnedWriteHalf>>,
    stream: EustressStream,
    subscriptions: HashMap<String, SubscriberId>,
    max_frame: usize,
    channel_capacity: usize,
}

impl ConnectionHandler {
    async fn run(&mut self) -> Result<(), NodeError> {
        loop {
            let frame = read_client_frame(&mut self.reader, self.max_frame).await?;
            self.dispatch(frame).await?;
        }
    }

    async fn dispatch(&mut self, frame: ClientFrame) -> Result<(), NodeError> {
        match frame {
            ClientFrame::Publish { topic, payload } => {
                let offset = self.stream
                    .producer(&topic)
                    .send_bytes(Bytes::from(payload));
                self.send(ServerFrame::Ack { offset }).await?;
            }

            ClientFrame::Subscribe { topic, from_offset } => {
                if self.subscriptions.contains_key(&topic) {
                    // Already subscribed — re-use existing subscription.
                    return Ok(());
                }

                let (tx, mut rx) = mpsc::channel::<OwnedMessage>(self.channel_capacity);
                let topic_name = topic.clone();

                let sub_id = self.stream
                    .subscribe_owned(&topic, {
                        let tx = tx.clone();
                        let topic_name = topic_name.clone();
                        move |msg: OwnedMessage| {
                            if tx.try_send(msg).is_err() {
                                // Channel full — drop oldest (ring semantics)
                            }
                            let _ = topic_name.len(); // silence unused warning
                        }
                    })
                    .map_err(|e| NodeError::SubscribeLimit(e.to_string()))?;

                self.subscriptions.insert(topic.clone(), sub_id);

                // If from_offset is set, replay ring buffer first.
                if let Some(offset) = from_offset {
                    let topic_ref = topic.clone();
                    let writer_ref = Arc::clone(&self.writer);
                    self.stream.replay_ring(&topic_ref, offset, |view| {
                        let frame = ServerFrame::Message {
                            topic: view.topic.to_string(),
                            offset: view.offset,
                            timestamp: view.timestamp,
                            payload: view.data.to_vec(),
                        };
                        let writer = Arc::clone(&writer_ref);
                        tokio::spawn(async move {
                            let mut guard = writer.lock().await;
                            let _ = write_server_frame(&mut *guard, &frame).await;
                        });
                    });
                }

                // Spawn a forwarding task: channel → TCP write.
                let writer = Arc::clone(&self.writer);
                let topic_clone = topic.clone();
                tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        let frame = ServerFrame::Message {
                            topic: topic_clone.clone(),
                            offset: msg.offset,
                            timestamp: msg.timestamp,
                            payload: msg.data.to_vec(),
                        };
                        let mut guard = writer.lock().await;
                        if write_server_frame(&mut *guard, &frame).await.is_err() {
                            break;
                        }
                    }
                });
            }

            ClientFrame::Unsubscribe { topic } => {
                if let Some(id) = self.subscriptions.remove(&topic) {
                    self.stream.unsubscribe(&topic, id);
                }
            }

            ClientFrame::ListTopics => {
                let stats: Vec<TopicStats> = self.stream.topics()
                    .into_iter()
                    .map(|name| {
                        let head = self.stream.head(&name);
                        let subscriber_count = self.stream.subscriber_count(&name);
                        TopicStats { name, head, subscriber_count }
                    })
                    .collect();
                self.send(ServerFrame::TopicList(stats)).await?;
            }

            ClientFrame::Ping => {
                self.send(ServerFrame::Pong).await?;
            }
        }
        Ok(())
    }

    async fn send(&self, frame: ServerFrame) -> Result<(), NodeError> {
        let mut guard = self.writer.lock().await;
        write_server_frame(&mut *guard, &frame).await
    }
}
