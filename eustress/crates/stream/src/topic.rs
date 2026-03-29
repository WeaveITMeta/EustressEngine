//! Topic — named append-only log with an in-memory ring + optional
//! persistent segment backend.
//!
//! A `Topic` owns:
//! - a `RingBuffer` for hot in-process delivery
//! - a `Vec<SubscriberEntry>` for registered callbacks
//! - an optional `Arc<dyn StorageBackend>` for durability

use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;

use crate::message::{MessageHeader, MessageView, OwnedMessage};
use crate::ring::RingBuffer;
use crate::storage::StorageBackend;

// ─────────────────────────────────────────────────────────────────────────────
// Subscriber
// ─────────────────────────────────────────────────────────────────────────────

/// Opaque handle returned by `EustressStream::subscribe`.
/// Drop it to unsubscribe (or call `EustressStream::unsubscribe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SubscriberId(pub u64);

type SubscriberFn = Box<dyn Fn(MessageView<'_>) + Send + Sync + 'static>;

struct SubscriberEntry {
    id: SubscriberId,
    callback: SubscriberFn,
}

// ─────────────────────────────────────────────────────────────────────────────
// Topic
// ─────────────────────────────────────────────────────────────────────────────

pub struct Topic {
    pub name: String,
    ring: Arc<RingBuffer>,
    subs: RwLock<Vec<SubscriberEntry>>,
    storage: Option<Arc<dyn StorageBackend>>,
    next_sub_id: std::sync::atomic::AtomicU64,
    max_subscribers: usize,
}

impl Topic {
    pub fn new(
        name: impl Into<String>,
        ring_capacity: usize,
        storage: Option<Arc<dyn StorageBackend>>,
        max_subscribers: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            ring: RingBuffer::new(ring_capacity),
            subs: RwLock::new(Vec::new()),
            storage,
            next_sub_id: std::sync::atomic::AtomicU64::new(1),
            max_subscribers,
        })
    }

    // ── Publish ──────────────────────────────────────────────────────────────

    /// Append a raw payload.  Dispatches to subscribers synchronously, then
    /// writes to storage asynchronously (fire-and-forget).
    pub fn publish(&self, payload: Bytes) -> u64 {
        // 1. Push to ring — get the offset.
        let offset = self.ring.push(payload.clone());

        // 2. Dispatch subscribers with a zero-copy view.
        let view = MessageView {
            topic:     &self.name,
            offset,
            timestamp: now_micros(),
            data:      &payload,
        };
        let subs = self.subs.read();
        for sub in subs.iter() {
            (sub.callback)(view);
        }
        drop(subs);

        // 3. Persist to storage backend (non-blocking — best effort).
        if let Some(ref store) = self.storage {
            let header = MessageHeader::new(offset, &payload);
            let store  = Arc::clone(store);
            let data   = payload;
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(MessageHeader::SIZE + data.len());
                buf.extend_from_slice(bytemuck::bytes_of(&header));
                buf.extend_from_slice(&data);
                if let Err(e) = store.append(Bytes::from(buf)).await {
                    tracing::warn!("EustressStream: storage write error: {e}");
                }
            });
        }

        offset
    }

    // ── Subscribe ────────────────────────────────────────────────────────────

    /// Register a callback.  Returns `None` if the subscriber limit is reached.
    pub fn subscribe<F>(&self, callback: F) -> Option<SubscriberId>
    where
        F: Fn(MessageView<'_>) + Send + Sync + 'static,
    {
        let mut subs = self.subs.write();
        if subs.len() >= self.max_subscribers {
            return None;
        }
        let id = SubscriberId(
            self.next_sub_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        subs.push(SubscriberEntry { id, callback: Box::new(callback) });
        Some(id)
    }

    /// Register a callback that receives owned (heap) messages instead of
    /// zero-copy views.  Slightly higher allocation cost but `'static`.
    pub fn subscribe_owned<F>(&self, callback: F) -> Option<SubscriberId>
    where
        F: Fn(OwnedMessage) + Send + Sync + 'static,
    {
        let name = self.name.clone();
        self.subscribe(move |view| {
            callback(OwnedMessage {
                topic:     name.clone(),
                offset:    view.offset,
                timestamp: view.timestamp,
                data:      Bytes::copy_from_slice(view.data),
            });
        })
    }

    /// Register a flume sender — messages are forwarded to a channel.
    pub fn subscribe_channel(&self, tx: flume::Sender<OwnedMessage>) -> Option<SubscriberId> {
        let name = self.name.clone();
        self.subscribe(move |view| {
            let msg = OwnedMessage {
                topic:     name.clone(),
                offset:    view.offset,
                timestamp: view.timestamp,
                data:      Bytes::copy_from_slice(view.data),
            };
            let _ = tx.try_send(msg);
        })
    }

    pub fn unsubscribe(&self, id: SubscriberId) {
        self.subs.write().retain(|s| s.id != id);
    }

    // ── Replay ───────────────────────────────────────────────────────────────

    /// Deliver all in-ring messages from `from_offset` to a callback.
    /// For full replay from disk, use `StorageBackend::read_range`.
    pub fn replay_from_ring<F>(&self, from_offset: u64, mut callback: F)
    where
        F: FnMut(MessageView<'_>),
    {
        let head = self.ring.head();
        for offset in from_offset..head {
            if let Some(data) = self.ring.get(offset) {
                let view = MessageView {
                    topic:     &self.name,
                    offset,
                    timestamp: 0, // not stored in ring
                    data:      &data,
                };
                callback(view);
            }
        }
    }

    // ── Accessors ────────────────────────────────────────────────────────────

    pub fn head(&self) -> u64 { self.ring.head() }
    pub fn subscriber_count(&self) -> usize { self.subs.read().len() }
}

fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
