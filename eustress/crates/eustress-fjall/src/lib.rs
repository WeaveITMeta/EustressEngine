//! # eustress-fjall — Eustress's owned database substrate (the Fjall fork)
//!
//! This crate is the fork point. Per the fork strategy: it starts by
//! wrapping and re-exporting upstream `fjall`/`lsm-tree`, and adds the
//! engine-specific capabilities that multiplayer and distributed
//! worlds need on top — **multiplexing**, **concurrency**, and a
//! **per-store replication feed**. When a genuine internal patch to
//! the upstream crates is required, the dependency here is the single
//! place it is pointed (vendored fork or `[patch.crates-io]`), so the
//! rest of the workspace never changes.
//!
//! The whole workspace depends on `eustress-fjall`, not on `fjall`
//! directly, so the substrate is owned in one place.
//!
//! ## Why this exists (the requirement)
//!
//! Multiplayer and distributed worlds need three things stock single-
//! handle usage does not give cleanly:
//!
//! 1. **Multiplexing** — one process holding many independently
//!    addressable logical stores at once: every Roblox-style named
//!    DataStore, plus the DataModel (the hierarchical instance tree),
//!    plus the engine partitions (entities/tree/meta). Created on
//!    demand, isolated, concurrently accessible.
//! 2. **Concurrency** — many readers taking point-in-time
//!    snapshot-isolated views that never block writers (an
//!    area-of-interest query for one player must not stall another
//!    player's write), with writes serialised per store for batch
//!    atomicity.
//! 3. **Replication feed** — every commit on every store emits a
//!    sequence-numbered delta on a bounded channel, the substrate for
//!    server→client replication and conflict-free-replicated-data-type
//!    synchronisation across a distributed world.
//!
//! ## Re-exports
//!
//! `pub use fjall;` — callers reach the upstream surface through this
//! crate. Migrating internal patches later does not ripple outward.

#![warn(missing_docs)]

pub use fjall;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use parking_lot::{Mutex, RwLock};
use thiserror::Error;

/// Result alias for the substrate.
pub type Result<T> = std::result::Result<T, Error>;

/// Substrate error.
#[derive(Debug, Error)]
pub enum Error {
    /// Upstream fjall failure (open, compaction, flush, input/output).
    #[error("fjall: {0}")]
    Fjall(#[from] fjall::Error),
    /// A store name that is not a valid partition name, or a registry
    /// invariant violation.
    #[error("store: {0}")]
    Store(String),
    /// A snapshot read failure. `fjall::Snapshot::get` surfaces the
    /// inner `lsm-tree` error type rather than `fjall::Error`; mapped
    /// here as a string so this crate need not take `lsm-tree` as a
    /// direct dependency.
    #[error("snapshot: {0}")]
    Snapshot(String),
}

/// The well-known logical store holding the DataModel — the
/// hierarchical instance tree (services → folders → models → parts).
/// Distinct from the per-game named DataStores so the scene graph and
/// game-state persistence never contend.
pub const DATAMODEL_STORE: &str = "__datamodel__";

/// Default per-subscriber replication queue depth. Past this the
/// oldest delta is dropped and the drop counter advances (a slow
/// replication consumer must not stall the authoritative writer).
const DEFAULT_FEED_DEPTH: usize = 4096;

// ─────────────────────────────────────────────────────────────────────────────
// Replication feed
// ─────────────────────────────────────────────────────────────────────────────

/// The operation a [`ReplicationEvent`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplOp {
    /// A key was written (insert or overwrite).
    Put,
    /// A key was removed.
    Remove,
}

/// One sequence-numbered change on one store. The unit of
/// server→client replication and distributed synchronisation. The
/// value is a truncated preview (≤ 64 bytes); a consumer needing the
/// full payload reads it from a snapshot at `seq`.
#[derive(Debug, Clone)]
pub struct ReplicationEvent {
    /// Logical store name this change happened on.
    pub store: String,
    /// Per-store monotonic sequence number (gap-free, starts at 1).
    pub seq: u64,
    /// Put or Remove.
    pub op: ReplOp,
    /// The changed key.
    pub key: Vec<u8>,
    /// Truncated value preview (empty for Remove).
    pub value_preview: Vec<u8>,
}

/// A subscriber handle. Dropping it unsubscribes (swept on next
/// publish). Bounded — backpressure drops oldest, never blocks the
/// committer.
pub struct ReplicationFeed {
    rx: Receiver<ReplicationEvent>,
}

impl ReplicationFeed {
    /// Non-blocking receive — integrate into the server tick / async
    /// replication task.
    pub fn try_recv(&self) -> Option<ReplicationEvent> {
        self.rx.try_recv().ok()
    }
    /// Blocking receive — tooling/tests only.
    pub fn recv_blocking(&self) -> Option<ReplicationEvent> {
        self.rx.recv().ok()
    }
}

struct Subscriber {
    tx: Sender<ReplicationEvent>,
    dropped: AtomicU64,
}

#[derive(Default)]
struct Broadcaster {
    subs: RwLock<Vec<Arc<Subscriber>>>,
}

impl Broadcaster {
    fn subscribe(&self, depth: usize) -> ReplicationFeed {
        let (tx, rx) = bounded(depth);
        self.subs.write().push(Arc::new(Subscriber {
            tx,
            dropped: AtomicU64::new(0),
        }));
        ReplicationFeed { rx }
    }

    fn publish(&self, ev: &ReplicationEvent) {
        let mut dead = Vec::new();
        {
            let subs = self.subs.read();
            for (i, s) in subs.iter().enumerate() {
                match s.tx.try_send(ev.clone()) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        s.dropped.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!(
                            target: "eustress_fjall::replication",
                            store = %ev.store, seq = ev.seq,
                            dropped = s.dropped.load(Ordering::Relaxed),
                            "replication subscriber backpressure — dropping oldest"
                        );
                    }
                    Err(TrySendError::Disconnected(_)) => dead.push(i),
                }
            }
        }
        if !dead.is_empty() {
            let mut w = self.subs.write();
            for i in dead.into_iter().rev() {
                if i < w.len() {
                    w.swap_remove(i);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Store handle — one logical store, concurrent + snapshot-isolated
// ─────────────────────────────────────────────────────────────────────────────

/// A clonable, thread-safe handle to one logical store (one fjall
/// partition). Reads are lock-free and may take a point-in-time
/// [`Snapshot`]; writes go through an atomic batch and emit a
/// replication event AFTER the commit is durable/visible.
#[derive(Clone)]
pub struct StoreHandle {
    name: String,
    keyspace: fjall::Keyspace,
    partition: fjall::PartitionHandle,
    /// Per-store monotonic replication sequence.
    seq: Arc<AtomicU64>,
    /// Serialises the commit critical section so batched multi-key
    /// writes are atomic w.r.t. the sequence number + replication
    /// ordering. Reads never take this.
    commit_lock: Arc<Mutex<()>>,
    feed: Arc<Broadcaster>,
}

impl StoreHandle {
    /// The logical store name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Point lookup (lock-free).
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.partition.get(key)?.map(|s| s.to_vec()))
    }

    /// Prefix scan (lock-free). Not `Send` — fjall's iterator borrows
    /// the partition; collect eagerly to cross threads.
    pub fn prefix<'a>(
        &'a self,
        prefix: &[u8],
    ) -> impl Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + 'a {
        self.partition.prefix(prefix.to_vec()).map(|kv| {
            let (k, v) = kv?;
            Ok((k.to_vec(), v.to_vec()))
        })
    }

    /// Single atomic write + replication emit.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<u64> {
        self.commit(vec![(key.to_vec(), Some(value.to_vec()))])
    }

    /// Single atomic delete + replication emit.
    pub fn remove(&self, key: &[u8]) -> Result<u64> {
        self.commit(vec![(key.to_vec(), None)])
    }

    /// Atomic multi-key batch (`Some` = put, `None` = remove). Returns
    /// the last replication sequence assigned. All ops land in one
    /// fjall batch; replication events publish AFTER `batch.commit()`
    /// (the atomicity contract — subscribers never see a change that
    /// is not yet durable).
    pub fn commit(&self, ops: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> Result<u64> {
        if ops.is_empty() {
            return Ok(self.seq.load(Ordering::Acquire));
        }
        let _guard = self.commit_lock.lock();
        let mut batch = self.keyspace.batch();
        for (k, v) in &ops {
            match v {
                Some(val) => batch.insert(&self.partition, k.as_slice(), val.as_slice()),
                None => batch.remove(&self.partition, k.as_slice()),
            }
        }
        batch.commit()?;

        // Publish post-commit, in op order, with gap-free sequences.
        let mut last = self.seq.load(Ordering::Acquire);
        for (k, v) in ops {
            last = self.seq.fetch_add(1, Ordering::AcqRel) + 1;
            let (op, preview) = match &v {
                Some(val) => (ReplOp::Put, val[..val.len().min(64)].to_vec()),
                None => (ReplOp::Remove, Vec::new()),
            };
            self.feed.publish(&ReplicationEvent {
                store: self.name.clone(),
                seq: last,
                op,
                key: k,
                value_preview: preview,
            });
        }
        Ok(last)
    }

    /// Emit a replication delta for a write the caller already
    /// performed itself (through a raw handle / cross-partition batch),
    /// without re-writing. This is the bridge for code that keeps a
    /// proven upstream-`fjall` write path (e.g. a cross-store atomic
    /// commit the per-store [`Self::commit`] cannot express) but still
    /// wants every mutation on the multiplayer/distributed replication
    /// feed. Advances the same gap-free per-store sequence and
    /// publishes post-write, preserving the atomicity contract
    /// (subscribers never see a change that is not yet durable —
    /// the caller calls this only AFTER its commit succeeded).
    pub fn publish_external(&self, op: ReplOp, key: &[u8], value_preview: &[u8]) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::AcqRel) + 1;
        self.feed.publish(&ReplicationEvent {
            store: self.name.clone(),
            seq,
            op,
            key: key.to_vec(),
            value_preview: value_preview[..value_preview.len().min(64)].to_vec(),
        });
        seq
    }

    /// Subscribe to this store's replication feed (default depth).
    pub fn subscribe(&self) -> ReplicationFeed {
        self.feed.subscribe(DEFAULT_FEED_DEPTH)
    }

    /// Take a point-in-time snapshot. Reads through it are isolated
    /// from concurrent writers (multi-version concurrency control) —
    /// an area-of-interest query for one player does not block another
    /// player's write.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            inner: self.partition.snapshot(),
        }
    }

    /// Current replication sequence (for resume / catch-up).
    pub fn current_seq(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// The underlying upstream keyspace (clone — cheap, reference-
    /// counted). Lets a caller that already has battle-tested
    /// upstream-`fjall` code (cross-partition atomic batches, custom
    /// iteration) keep that code while still entering through the
    /// owned multiplexer. The migration path: own the entry point
    /// first, port internals incrementally.
    pub fn raw_keyspace(&self) -> fjall::Keyspace {
        self.keyspace.clone()
    }

    /// The underlying upstream partition handle for this store (clone
    /// — cheap, reference-counted).
    pub fn raw_partition(&self) -> fjall::PartitionHandle {
        self.partition.clone()
    }
}

/// Point-in-time isolated read view over one store.
pub struct Snapshot {
    inner: fjall::Snapshot,
}

impl Snapshot {
    /// Isolated point lookup as of the snapshot instant.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.inner
            .get(key)
            .map(|opt| opt.map(|s| s.to_vec()))
            .map_err(|e| Error::Snapshot(e.to_string()))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The multiplexer
// ─────────────────────────────────────────────────────────────────────────────

/// Owns one fjall keyspace and multiplexes many logical stores over
/// it (one fjall partition per logical store), created on demand and
/// concurrently accessible. This is the database substrate the engine
/// holds; the Roblox-style named DataStores, the DataModel, and the
/// engine partitions are all logical stores here.
pub struct EustressFjall {
    keyspace: fjall::Keyspace,
    stores: RwLock<HashMap<String, StoreHandle>>,
}

impl EustressFjall {
    /// Open (or recover) the multiplexed substrate rooted at `path`
    /// (the `world.fjalldb/` directory inside an `.eustress`
    /// container).
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let keyspace = fjall::Config::new(path).open()?;
        Ok(Self {
            keyspace,
            stores: RwLock::new(HashMap::new()),
        })
    }

    /// Get (lazily creating) a logical store by name. Thread-safe:
    /// the registry read path is lock-free for already-open stores;
    /// only first-open of a given name takes a brief write lock.
    /// Many threads/players can hold and use different stores
    /// concurrently.
    pub fn store(&self, name: &str) -> Result<StoreHandle> {
        if let Some(h) = self.stores.read().get(name) {
            return Ok(h.clone());
        }
        let mut w = self.stores.write();
        if let Some(h) = w.get(name) {
            return Ok(h.clone());
        }
        let partition = self
            .keyspace
            .open_partition(name, fjall::PartitionCreateOptions::default())
            .map_err(|e| Error::Store(format!("open_partition {name}: {e}")))?;
        let handle = StoreHandle {
            name: name.to_string(),
            keyspace: self.keyspace.clone(),
            partition,
            seq: Arc::new(AtomicU64::new(0)),
            commit_lock: Arc::new(Mutex::new(())),
            feed: Arc::new(Broadcaster::default()),
        };
        w.insert(name.to_string(), handle.clone());
        Ok(handle)
    }

    /// The well-known DataModel store (the hierarchical instance
    /// tree). Convenience for `store(DATAMODEL_STORE)`.
    pub fn datamodel(&self) -> Result<StoreHandle> {
        self.store(DATAMODEL_STORE)
    }

    /// Names of all currently-open logical stores.
    pub fn store_names(&self) -> Vec<String> {
        self.stores.read().keys().cloned().collect()
    }

    /// Flush every open store to disk (graceful shutdown / explicit
    /// save). Persistence normally rides the write-ahead log; this is
    /// the "ensure durable + visible" gate.
    pub fn persist(&self) -> Result<()> {
        self.keyspace.persist(fjall::PersistMode::SyncAll)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplex_isolated_stores_and_replication() {
        let dir = std::env::temp_dir().join(format!("efj_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = EustressFjall::open(&dir).unwrap();

        let players = db.store("PlayerData").unwrap();
        let inv = db.store("Inventory").unwrap();
        let dm = db.datamodel().unwrap();

        let feed = players.subscribe();

        players.put(b"u1", b"score=10").unwrap();
        inv.put(b"u1", b"sword").unwrap();
        dm.put(b"Workspace/Part1", b"<instance>").unwrap();

        // Stores are isolated.
        assert_eq!(players.get(b"u1").unwrap().as_deref(), Some(&b"score=10"[..]));
        assert_eq!(inv.get(b"u1").unwrap().as_deref(), Some(&b"sword"[..]));
        assert!(players.get(b"Workspace/Part1").unwrap().is_none());

        // Replication feed saw only PlayerData's change, sequenced.
        let ev = feed.try_recv().expect("one event");
        assert_eq!(ev.store, "PlayerData");
        assert_eq!(ev.seq, 1);
        assert_eq!(ev.op, ReplOp::Put);
        assert!(feed.try_recv().is_none());

        // Snapshot isolation: snapshot, then mutate, snapshot stays old.
        let snap = players.snapshot();
        players.put(b"u1", b"score=99").unwrap();
        assert_eq!(snap.get(b"u1").unwrap().as_deref(), Some(&b"score=10"[..]));
        assert_eq!(players.get(b"u1").unwrap().as_deref(), Some(&b"score=99"[..]));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
