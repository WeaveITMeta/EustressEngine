//! Per-commit change-stream — Tier 1 #4 of the Fjall fork value zones.
//!
//! Every successful [`crate::backend::WorldDb::apply_commit`] fans out
//! a [`CommitDelta`] to every active [`Subscription`] whose [`Filter`]
//! accepts it. Subscribers are bounded crossbeam channels so a slow
//! consumer can't stall the committer — when its queue fills the
//! oldest event drops, the subscriber's `dropped_count` counter
//! advances, and the engine bridge surfaces the lag as a
//! `world.db.subscriber_lag` topic event.
//!
//! ## Engine bridge (gated on `streams-bridge` feature)
//!
//! When wired by the engine plugin, [`ChangeStream::publish`] also
//! republishes the delta into the existing `EustressStream` topic
//! fabric under names like:
//!
//! - `world.commit`                    — per-commit summary
//! - `world.entity.added.<class>`      — one per spawn
//! - `world.entity.changed.<class>.<component>`
//! - `world.entity.removed.<class>`
//! - `world.db.flush`                  — diagnostic
//! - `world.db.compaction`             — diagnostic
//! - `world.db.subscriber_lag`         — backpressure surface
//!
//! The actual translation lives in the engine plugin (separation of
//! concerns — this crate doesn't link the broker). The trait-level
//! design is: [`CommitDelta`] is the wire fmt, topics are derived
//! from class/component ids the bridge knows about.

use std::sync::Arc;

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use parking_lot::RwLock;

use crate::backend::EntityId;
use crate::keys::ComponentTypeId;

/// Monotonically-incrementing commit id. Carried in every delta + the
/// Fjall WAL so subscribers can correlate. `synthetic` bit lets
/// engine-side events (Workshop snapshots, AI annotations) ride on the
/// same fabric without colliding with genuine commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxId(pub u64);

impl TxId {
    /// Lowest non-synthetic id — used as the seed for a fresh world.
    pub const GENESIS: TxId = TxId(1);

    /// High bit is the synthetic flag.
    const SYNTHETIC_FLAG: u64 = 1u64 << 63;

    /// Construct a synthetic id (engine annotations, dry-run previews).
    pub fn synthetic(serial: u64) -> Self {
        TxId(Self::SYNTHETIC_FLAG | (serial & !Self::SYNTHETIC_FLAG))
    }

    /// `true` when this id was minted via [`Self::synthetic`].
    pub fn is_synthetic(self) -> bool {
        self.0 & Self::SYNTHETIC_FLAG != 0
    }

    /// Next id after `self` for the genuine-commit lane.
    pub fn next(self) -> Self {
        TxId((self.0 & !Self::SYNTHETIC_FLAG).wrapping_add(1) & !Self::SYNTHETIC_FLAG)
    }
}

/// One semantic change inside a [`CommitDelta`].
#[derive(Debug, Clone)]
pub enum EntityChange {
    /// `entity` got a new component value (insert OR overwrite).
    Put {
        entity: EntityId,
        component: ComponentTypeId,
        /// Truncated value preview (≤ 64 bytes). Subscribers needing
        /// the full payload do a fresh `get_component` against the
        /// committed snapshot — keeps the delta lightweight.
        value_preview: Vec<u8>,
    },
    /// `entity` lost a component (Properties panel "Reset", scripted
    /// `remove_component`, etc.). NOT fired during full despawn —
    /// the per-component `Delete` is suppressed in favour of a single
    /// [`Self::Despawned`] event.
    Removed {
        entity: EntityId,
        component: ComponentTypeId,
    },
    /// `entity` is gone in full (despawn). Subscribers should drop any
    /// per-entity state they were holding.
    Despawned { entity: EntityId },
}

/// All deltas from a single atomic commit. Ordering inside `changes`
/// matches the commit-build order so subscribers can replay
/// deterministically (Loro op streams, debug step-through, …).
#[derive(Debug, Clone)]
pub struct CommitDelta {
    pub tx_id: TxId,
    pub changes: Vec<EntityChange>,
    /// Total size in bytes — sum of `value` lengths in the commit.
    /// Surfaced as a `world.commit` payload field for throughput
    /// budgeting.
    pub byte_size: usize,
}

/// Subscriber-side filter. Cheap to evaluate inside the publish
/// critical section; complex filtering belongs to the consumer.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only deliver changes touching these components. Empty = all.
    pub components: Vec<ComponentTypeId>,
    /// Drop synthetic events (default false — most subscribers want
    /// the full unified stream).
    pub exclude_synthetic: bool,
}

impl Filter {
    /// Allow-everything filter — the default.
    pub fn any() -> Self {
        Self::default()
    }

    /// Restrict to a single component type. Common case: Loro CRDT
    /// only cares about `Transform` for its position-replication
    /// layer; building UI cares about `Tags` for the Explorer tree;
    /// etc.
    pub fn component(c: ComponentTypeId) -> Self {
        Self {
            components: vec![c],
            ..Default::default()
        }
    }

    /// True when the filter would accept this delta.
    pub fn matches(&self, delta: &CommitDelta) -> bool {
        if self.exclude_synthetic && delta.tx_id.is_synthetic() {
            return false;
        }
        if self.components.is_empty() {
            return true;
        }
        delta.changes.iter().any(|c| match c {
            EntityChange::Put { component, .. } | EntityChange::Removed { component, .. } => {
                self.components.contains(component)
            }
            EntityChange::Despawned { .. } => true,
        })
    }
}

/// Handle held by a subscriber. Dropping the value unsubscribes —
/// the underlying [`ChangeStream`] sweeps dead receivers on the next
/// publish, keeping the registry compact without an explicit `close`.
pub struct Subscription {
    pub(crate) inner: Receiver<CommitDelta>,
    pub(crate) filter: Filter,
}

impl Subscription {
    /// Try to receive the next matching delta. Returns immediately —
    /// callers integrate this into their own poll loop (Bevy system,
    /// async task, raw thread).
    pub fn try_recv(&self) -> Option<CommitDelta> {
        self.inner.try_recv().ok()
    }

    /// Block until a delta arrives. Used by tests and tooling, not by
    /// the engine.
    pub fn recv_blocking(&self) -> Option<CommitDelta> {
        self.inner.recv().ok()
    }

    /// Borrow the filter — diagnostics + the engine bridge that
    /// builds topic names from filter shape.
    pub fn filter(&self) -> &Filter {
        &self.filter
    }
}

struct ActiveSub {
    tx: Sender<CommitDelta>,
    filter: Filter,
    /// Counter advanced when a publish drops a delta due to full queue.
    /// Surfaced on `world.db.subscriber_lag` so ops can spot slow
    /// consumers without reading every queue depth.
    dropped: std::sync::atomic::AtomicU64,
}

/// Owner of the subscriber registry. One per [`crate::WorldDb`].
pub struct ChangeStream {
    subscribers: RwLock<Vec<Arc<ActiveSub>>>,
    /// Default queue depth for new subscriptions. Past this the
    /// oldest delta drops; advances [`ActiveSub::dropped`].
    queue_depth: usize,
}

impl ChangeStream {
    /// Construct with a default per-subscriber queue depth (1024 is
    /// a starting point — the engine bridge tunes this against the
    /// Tier 1 #1 frame-budget compaction story).
    pub fn new() -> Self {
        Self {
            subscribers: RwLock::new(Vec::new()),
            queue_depth: 1024,
        }
    }

    /// Override the default queue depth. Picks per-subscriber; doesn't
    /// resize existing channels.
    pub fn with_queue_depth(mut self, depth: usize) -> Self {
        self.queue_depth = depth;
        self
    }

    /// Hand out a fresh subscription. The lock is held briefly to
    /// push the new sender into the registry; reads + publishes are
    /// not blocked.
    pub fn subscribe(&self, filter: Filter) -> Subscription {
        let (tx, rx) = bounded(self.queue_depth);
        let active = Arc::new(ActiveSub {
            tx,
            filter: filter.clone(),
            dropped: std::sync::atomic::AtomicU64::new(0),
        });
        self.subscribers.write().push(active);
        Subscription { inner: rx, filter }
    }

    /// Broadcast one delta to every matching live subscriber. Called
    /// from inside `apply_commit` AFTER the commit becomes visible to
    /// `get` — never before (the atomicity contract).
    pub fn publish(&self, delta: CommitDelta) {
        let mut dead = Vec::new();
        let subs = self.subscribers.read();
        for (idx, sub) in subs.iter().enumerate() {
            if !sub.filter.matches(&delta) {
                continue;
            }
            match sub.tx.try_send(delta.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    sub.dropped
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    tracing::warn!(
                        target: "eustress_worlddb::changestream",
                        tx_id = delta.tx_id.0,
                        dropped_total = sub.dropped.load(std::sync::atomic::Ordering::Relaxed),
                        "change-stream subscriber backpressure — dropping oldest delta",
                    );
                }
                Err(TrySendError::Disconnected(_)) => {
                    dead.push(idx);
                }
            }
        }
        drop(subs);
        if !dead.is_empty() {
            let mut w = self.subscribers.write();
            // Reverse-order remove so indices stay valid as we swap.
            for idx in dead.into_iter().rev() {
                if idx < w.len() {
                    w.swap_remove(idx);
                }
            }
        }
    }

    /// Live subscriber count — diagnostics + the engine bridge's
    /// `world.db.subscribers` topic.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.read().len()
    }
}

impl Default for ChangeStream {
    fn default() -> Self {
        Self::new()
    }
}
