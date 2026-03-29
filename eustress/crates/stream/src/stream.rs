//! EustressStream — the main entry point.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use eustress_stream::{EustressStream, StreamConfig};
//!
//! let stream = EustressStream::new(StreamConfig::default().in_memory());
//!
//! // Subscribe (zero-copy callback)
//! stream.subscribe("scene-updates", |view| {
//!     if let Some(entity_id) = view.cast::<u64>() {
//!         println!("entity {entity_id} updated at offset {}", view.offset);
//!     }
//! });
//!
//! // Produce
//! let producer = stream.producer("scene-updates");
//! producer.send_bytes(bytes::Bytes::from_static(b"hello"));
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;

use crate::config::StreamConfig;
use crate::error::StreamError;
use crate::message::{MessageView, OwnedMessage};
use crate::storage;
use crate::topic::{SubscriberId, Topic};

// ─────────────────────────────────────────────────────────────────────────────
// EustressStream
// ─────────────────────────────────────────────────────────────────────────────

/// The embedded streaming core.  Cheaply cloneable — all clones share the
/// same underlying topics via `Arc`.
#[derive(Clone)]
pub struct EustressStream {
    inner: Arc<StreamInner>,
}

struct StreamInner {
    config: StreamConfig,
    topics: RwLock<HashMap<String, Arc<Topic>>>,
}

impl EustressStream {
    /// Create a new stream with the given config.
    pub fn new(config: StreamConfig) -> Self {
        Self {
            inner: Arc::new(StreamInner {
                config,
                topics: RwLock::new(HashMap::new()),
            }),
        }
    }

    // ── Topic lifecycle ──────────────────────────────────────────────────────

    /// Get or create a topic by name.
    pub fn topic(&self, name: &str) -> Arc<Topic> {
        // Fast path: topic already exists.
        {
            let guard = self.inner.topics.read();
            if let Some(t) = guard.get(name) {
                return Arc::clone(t);
            }
        }

        // Slow path: create it.
        let mut guard = self.inner.topics.write();
        // Double-check after acquiring the write lock.
        if let Some(t) = guard.get(name) {
            return Arc::clone(t);
        }

        let backend = storage::create_backend(&self.inner.config, name);
        let topic   = Topic::new(
            name,
            self.inner.config.ring_capacity,
            backend,
            self.inner.config.max_subscribers,
        );
        guard.insert(name.to_owned(), Arc::clone(&topic));
        topic
    }

    /// List all topic names.
    pub fn topics(&self) -> Vec<String> {
        self.inner.topics.read().keys().cloned().collect()
    }

    // ── Producer ─────────────────────────────────────────────────────────────

    /// Get a `Producer` for the named topic.
    pub fn producer(&self, topic: &str) -> Producer {
        Producer { topic: self.topic(topic) }
    }

    // ── Subscriber ───────────────────────────────────────────────────────────

    /// Register a zero-copy callback on the named topic.
    /// Returns `Err` if the subscriber limit is reached.
    pub fn subscribe<F>(&self, topic: &str, callback: F) -> Result<SubscriberId, StreamError>
    where
        F: Fn(MessageView<'_>) + Send + Sync + 'static,
    {
        self.topic(topic)
            .subscribe(callback)
            .ok_or_else(|| StreamError::SubscriberLimit(topic.to_owned()))
    }

    /// Register an owned-message callback (allocates `OwnedMessage` per dispatch).
    pub fn subscribe_owned<F>(&self, topic: &str, callback: F) -> Result<SubscriberId, StreamError>
    where
        F: Fn(OwnedMessage) + Send + Sync + 'static,
    {
        self.topic(topic)
            .subscribe_owned(callback)
            .ok_or_else(|| StreamError::SubscriberLimit(topic.to_owned()))
    }

    /// Subscribe via a flume channel.
    /// ```rust,no_run,ignore
    /// let (tx, rx) = flume::unbounded::<eustress_stream::OwnedMessage>();
    /// stream.subscribe_channel("events", tx)?;
    /// // rx.recv() in your system
    /// ```
    pub fn subscribe_channel(
        &self,
        topic: &str,
        tx: flume::Sender<OwnedMessage>,
    ) -> Result<SubscriberId, StreamError> {
        self.topic(topic)
            .subscribe_channel(tx)
            .ok_or_else(|| StreamError::SubscriberLimit(topic.to_owned()))
    }

    /// Unsubscribe a previously registered callback.
    pub fn unsubscribe(&self, topic: &str, id: SubscriberId) {
        if let Some(t) = self.inner.topics.read().get(topic) {
            t.unsubscribe(id);
        }
    }

    // ── Replay ───────────────────────────────────────────────────────────────

    /// Replay all messages in the ring buffer (most recent `ring_capacity`).
    /// For full disk replay, iterate segment files via `StorageBackend::read_range`.
    pub fn replay_ring<F>(&self, topic: &str, from_offset: u64, callback: F)
    where
        F: FnMut(MessageView<'_>),
    {
        if let Some(t) = self.inner.topics.read().get(topic) {
            t.replay_from_ring(from_offset, callback);
        }
    }

    // ── Stats ────────────────────────────────────────────────────────────────

    pub fn head(&self, topic: &str) -> u64 {
        self.inner.topics.read().get(topic).map(|t| t.head()).unwrap_or(0)
    }

    pub fn subscriber_count(&self, topic: &str) -> usize {
        self.inner.topics.read().get(topic).map(|t| t.subscriber_count()).unwrap_or(0)
    }

    pub fn config(&self) -> &StreamConfig {
        &self.inner.config
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Producer
// ─────────────────────────────────────────────────────────────────────────────

/// High-throughput message producer.  Cheaply cloneable.
#[derive(Clone)]
pub struct Producer {
    topic: Arc<Topic>,
}

impl Producer {
    /// Send raw bytes.  Zero-copy: `Bytes` is a refcounted slice.
    #[inline]
    pub fn send_bytes(&self, data: Bytes) -> u64 {
        self.topic.publish(data)
    }

    /// Send pre-serialized rkyv bytes.
    ///
    /// Callers serialize their types with the project-standard:
    /// ```rust,ignore
    /// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&value)
    ///     .map_err(|e| StreamError::Serialize(e.to_string()))?;
    /// producer.send_rkyv_bytes(bytes.to_vec());
    /// ```
    pub fn send_rkyv_bytes(&self, bytes: Vec<u8>) -> u64 {
        self.send_bytes(Bytes::from(bytes))
    }

    /// Serialize `T` via serde + bincode and send.
    pub fn send<T: serde::Serialize>(&self, value: &T) -> Result<u64, StreamError> {
        let bytes = bincode_encode(value)?;
        Ok(self.send_bytes(bytes))
    }

    /// Send a `bytemuck::Pod` value with zero allocation (stack → ring).
    #[inline]
    pub fn send_pod<T: bytemuck::Pod>(&self, value: &T) -> u64 {
        let bytes = Bytes::copy_from_slice(bytemuck::bytes_of(value));
        self.send_bytes(bytes)
    }

    pub fn topic_name(&self) -> &str { &self.topic.name }
    pub fn head(&self) -> u64 { self.topic.head() }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bincode helper
// ─────────────────────────────────────────────────────────────────────────────

fn bincode_encode<T: serde::Serialize>(value: &T) -> Result<Bytes, StreamError> {
    let encoded = serde_json::to_vec(value)
        .map_err(|e| StreamError::Serialize(e.to_string()))?;
    Ok(Bytes::from(encoded))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn pub_sub_zero_copy() {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        let count  = Arc::new(AtomicU64::new(0));
        let cnt    = Arc::clone(&count);

        stream.subscribe("events", move |view| {
            assert_eq!(view.topic, "events");
            cnt.fetch_add(1, Ordering::Relaxed);
        }).unwrap();

        let producer = stream.producer("events");
        for _ in 0..1000 {
            producer.send_bytes(Bytes::from_static(b"ping"));
        }

        assert_eq!(count.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn pod_zero_copy_cast() {
        #[derive(Clone, Copy)]
        #[repr(C)]
        struct Point { x: f32, y: f32 }
        unsafe impl bytemuck::Pod      for Point {}
        unsafe impl bytemuck::Zeroable for Point {}

        let stream   = EustressStream::new(StreamConfig::default().in_memory());
        let received = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let rx       = Arc::clone(&received);

        stream.subscribe("points", move |view| {
            if let Some(pt) = view.cast::<Point>() {
                rx.lock().push((pt.x, pt.y));
            }
        }).unwrap();

        let p = stream.producer("points");
        p.send_pod(&Point { x: 1.0, y: 2.0 });
        p.send_pod(&Point { x: 3.0, y: 4.0 });

        let got = received.lock();
        assert_eq!(got[0], (1.0, 2.0));
        assert_eq!(got[1], (3.0, 4.0));
    }

    #[test]
    fn channel_subscriber() {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        let (tx, rx) = flume::unbounded::<OwnedMessage>();
        stream.subscribe_channel("ch", tx).unwrap();

        stream.producer("ch").send_bytes(Bytes::from_static(b"msg1"));
        stream.producer("ch").send_bytes(Bytes::from_static(b"msg2"));

        assert_eq!(rx.recv().unwrap().data.as_ref(), b"msg1");
        assert_eq!(rx.recv().unwrap().data.as_ref(), b"msg2");
    }

    #[test]
    fn multi_topic_independent() {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        let a_count = Arc::new(AtomicU64::new(0));
        let b_count = Arc::new(AtomicU64::new(0));
        let ac = Arc::clone(&a_count);
        let bc = Arc::clone(&b_count);

        stream.subscribe("a", move |_| { ac.fetch_add(1, Ordering::Relaxed); }).unwrap();
        stream.subscribe("b", move |_| { bc.fetch_add(1, Ordering::Relaxed); }).unwrap();

        stream.producer("a").send_bytes(Bytes::from_static(b"x"));
        stream.producer("a").send_bytes(Bytes::from_static(b"x"));
        stream.producer("b").send_bytes(Bytes::from_static(b"y"));

        assert_eq!(a_count.load(Ordering::Relaxed), 2);
        assert_eq!(b_count.load(Ordering::Relaxed), 1);
    }
}
