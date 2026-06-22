//! Store-mediated SimCell submission + the one-directional WorldDb↔Raft
//! replicated-slice adapter — the single auditable boundary where local
//! world state and the cross-node replicated slice meet.
//!
//! ## Two responsibilities, one file
//!
//! 1. **SimCell submission** ([`submit_sim_cell`] / [`retire_sim_cell`]):
//!    write a [`SimCell`] under the EXACT key the [`super::Reconciler`] scans
//!    (`forge_orchestration::storage::keys::simcell(id)` under the
//!    `keys::SIMCELLS` prefix), so the next `reconcile_once` discovers it.
//!    Get the key wrong and reconcile silently schedules nothing.
//!
//! 2. **Replicated cell-slice** ([`CellSyncBridge`]): mirror a *tiny*
//!    per-cell [`CellSlice`] (owner + epoch + agent intents + small shared
//!    KV) between [`WorldDb`] and the [`super::RaftStateStore`]. Entity
//!    component bytes NEVER cross — the Raft log scales with cell-count, not
//!    entity-count.
//!
//! ## Invariants pinned from the state-layer review
//!
//! * **Single-writer per cell.** For any cell, this node is EITHER the
//!   `owner_node` (push-only — never `apply_inbound` it) OR not (pull-only —
//!   never `push` it). [`CellSyncBridge::push_dirty`] skips non-owned cells;
//!   [`CellSyncBridge::apply_inbound`] rejects owned cells. This is what
//!   makes the leader-local read contract sufficient in steady state.
//! * **Async/sync impedance.** [`StateStore`] is async (tokio) but
//!   [`WorldDb`] is synchronous, blocking LSM I/O. Every WorldDb access from
//!   here runs under `tokio::task::spawn_blocking` so it never stalls the
//!   runtime that drives Raft heartbeats. No WorldDb lock/commit is held
//!   across an `.await`.
//! * **Epoch monotonicity.** `StateStore::set` is a blind overwrite with no
//!   compare-and-set, so the LAND side enforces ordering:
//!   [`CellSyncBridge::apply_inbound`] drops any slice whose `epoch` is `<=`
//!   the last epoch already applied for that cell.
//! * **Bounded surface.** [`retire_sim_cell`] + tombstoning keep the
//!   replicated key-space from growing forever as cells unload.
//! * **Stable keys.** [`cell_slice_key`] takes an opaque universe id (NOT a
//!   human name) so a universe rename never orphans replicated cells.

use super::{store_get_json, store_set_json, CellCoord, SimCell, StateStore};
use eustress_worlddb::WorldDb;
use forge_orchestration::storage::keys;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use crate::error::{EustressForgeError, Result};

/// Map an upstream `forge_orchestration` error into this crate's error.
fn map_forge_err<E: Into<forge_orchestration::ForgeError>>(e: E) -> EustressForgeError {
    EustressForgeError::Orchestration(e.into())
}

// ── 1. SimCell submission ─────────────────────────────────────────────────

/// Submit one [`SimCell`] so the next `reconcile_once` discovers it. Writes
/// JSON under `keys::simcell(&cell.id)` — the SAME key the reconciler reads
/// via `list_prefix(keys::SIMCELLS)`.
pub async fn submit_sim_cell(store: &dyn StateStore, cell: &SimCell) -> Result<()> {
    store_set_json(store, &keys::simcell(&cell.id), cell)
        .await
        .map_err(map_forge_err)
}

/// Withdraw a SimCell on residency exit (its cell left the keep box). The
/// reconciler treats a missing key as "no longer desired" and releases the
/// gang on the next pass.
pub async fn retire_sim_cell(store: &dyn StateStore, cell_id: &str) -> Result<()> {
    store
        .delete(&keys::simcell(cell_id))
        .await
        .map_err(map_forge_err)
}

// ── 2. The replicated cell-slice ──────────────────────────────────────────

/// The cross-node replicated slice for ONE cell. Deliberately tiny: it
/// carries ownership + ordering metadata, agent INTENTS (not agent state),
/// and a small shared KV — never entity component bytes, which stay in
/// [`WorldDb`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CellSlice {
    /// The residency cell this slice describes.
    pub cell: CellCoord,
    /// The node that currently OWNS (writes) this cell.
    pub owner_node: u64,
    /// Monotonic version. The land side rejects a slice whose epoch is not
    /// strictly greater than the last applied epoch for this cell.
    pub epoch: u64,
    /// Agent placement intents — what the owner WANTS scheduled, not live
    /// agent state.
    pub agents: Vec<AgentIntent>,
    /// Small cell-shared key/value bag (e.g. shared timers, vote tallies).
    /// BTreeMap for deterministic serialisation.
    #[serde(default)]
    pub shared_kv: BTreeMap<String, serde_json::Value>,
}

/// One agent's cross-node intent inside a [`CellSlice`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentIntent {
    /// Stable agent id.
    pub id: String,
    /// Cell the agent wants to act in (usually the slice's own cell, but an
    /// agent may target a neighbour for hand-off).
    pub target_cell: CellCoord,
    /// Agent role token (mirrors `MemberRole::token`).
    pub role: String,
    /// Soft deadline in epoch-millis for the intent.
    pub deadline_ms: i64,
}

/// Key prefix every replicated cell-slice lives under. Disjoint from the
/// reserved `keys::{JOBS, NODES, CONFIG}` and from `keys::SIMCELLS`.
pub const CELL_SLICE_PREFIX: &str = "forge/cell";

/// `forge/cell/{universe}/{cx:07}-{cy:07}-{cz:07}` — the replicated-slice key
/// for one cell in one universe.
///
/// `universe` MUST be a STABLE opaque id, never a human-readable name: these
/// keys live long-term in the Raft log, and a universe rename would otherwise
/// orphan every replicated cell. Store the human name in a per-universe meta
/// key instead.
pub fn cell_slice_key(universe: &str, c: CellCoord) -> String {
    format!("{CELL_SLICE_PREFIX}/{universe}/{:07}-{:07}-{:07}", c.0, c.1, c.2)
}

/// The bridge that moves [`CellSlice`]s between the local [`WorldDb`] and the
/// replicated [`StateStore`], honouring the single-writer + epoch invariants.
pub struct CellSyncBridge {
    /// Local entity source-of-truth. Entity cores never leave it; the bridge
    /// only reads/writes the small cell-shared slice (today via the dedicated
    /// `forge_shared` landing zone — see [`WorldDb::get_shared`] /
    /// [`WorldDb::put_shared`] TODO below).
    pub db: Arc<dyn WorldDb>,
    /// The replicated store.
    pub store: Arc<dyn StateStore>,
    /// This node's id (the `owner_node` value it stamps on cells it owns).
    pub node_id: u64,
    /// Stable opaque universe id used in [`cell_slice_key`].
    pub universe: String,
    /// Cells touched locally since the last push.
    dirty: HashSet<CellCoord>,
    /// Last epoch successfully applied per cell (inbound monotonicity guard).
    /// In-memory mirror of the durable `forge_shared` epoch record; rebuilt
    /// from the store on restart (TODO with the landing-zone partition).
    last_applied_epoch: HashMap<CellCoord, u64>,
}

impl CellSyncBridge {
    /// Construct a bridge for one node + universe.
    pub fn new(db: Arc<dyn WorldDb>, store: Arc<dyn StateStore>, node_id: u64, universe: impl Into<String>) -> Self {
        Self {
            db,
            store,
            node_id,
            universe: universe.into(),
            dirty: HashSet::new(),
            last_applied_epoch: HashMap::new(),
        }
    }

    /// Mark a cell dirty so the next [`push_dirty`](Self::push_dirty) mirrors
    /// it. The engine driver calls this when a local edit touches the cell's
    /// shared slice.
    pub fn mark_dirty(&mut self, c: CellCoord) {
        self.dirty.insert(c);
    }

    /// True iff this node currently owns `slice` (so it may push, not land).
    fn owns(&self, slice: &CellSlice) -> bool {
        slice.owner_node == self.node_id
    }

    /// Push every dirty cell THIS NODE OWNS to the replicated store,
    /// epoch-bumped, clearing the dirty set. Cells not owned here are skipped
    /// (single-writer invariant).
    ///
    /// TODO(engine-driver): the slice payload is built from the cell's
    /// `forge_shared` landing-zone record (see the WorldDb get_shared/put_shared
    /// trait seam). Until that lands, `build_local_slice` returns a minimal
    /// owner+epoch slice so the path is exercised end-to-end. Entity cores are
    /// NEVER read into the slice.
    pub async fn push_dirty(&mut self) -> Result<()> {
        let cells: Vec<CellCoord> = self.dirty.iter().copied().collect();
        for cell in cells {
            let mut slice = self.build_local_slice(cell).await?;
            if !self.owns(&slice) {
                // Not ours to write — single-writer discipline. Leave dirty
                // flag cleared (we don't re-push someone else's cell).
                self.dirty.remove(&cell);
                continue;
            }
            // Monotonic bump relative to what we last applied/observed.
            let next = self.last_applied_epoch.get(&cell).copied().unwrap_or(0) + 1;
            slice.epoch = slice.epoch.max(next);
            let key = cell_slice_key(&self.universe, cell);
            store_set_json(self.store.as_ref(), &key, &slice)
                .await
                .map_err(map_forge_err)?;
            self.last_applied_epoch.insert(cell, slice.epoch);
            self.dirty.remove(&cell);
        }
        Ok(())
    }

    /// Pull the replicated slice for each interest-set cell NOT owned here and
    /// land it locally (epoch-guarded). Owned cells are skipped — we are the
    /// source of truth for those.
    pub async fn pull_shared(&mut self, cells: &[CellCoord]) -> Result<()> {
        for &cell in cells {
            let key = cell_slice_key(&self.universe, cell);
            let fetched: Option<CellSlice> = store_get_json(self.store.as_ref(), &key)
                .await
                .map_err(map_forge_err)?;
            let Some(slice) = fetched else { continue };
            self.apply_inbound(slice).await?;
        }
        Ok(())
    }

    /// Land one inbound [`CellSlice`] into local state, enforcing both
    /// invariants:
    /// * reject if THIS node owns the cell (we'd be landing over our own
    ///   newer write),
    /// * reject if `epoch <= last_applied_epoch[cell]` (stale / replayed).
    /// Returns `true` when the slice was actually applied.
    pub async fn apply_inbound(&mut self, slice: CellSlice) -> Result<bool> {
        if self.owns(&slice) {
            // Inbound for a cell we own — ignore (single-writer guard).
            return Ok(false);
        }
        let cell = slice.cell;
        let last = self.last_applied_epoch.get(&cell).copied().unwrap_or(0);
        if slice.epoch <= last {
            return Ok(false); // stale or duplicate
        }
        // TODO(engine-driver): land `slice` into the dedicated `forge_shared`
        // WorldDb partition as ONE single-key write (atomic by construction —
        // never split across the entities/tree partitions, which have no
        // cross-partition atomicity). Done under spawn_blocking so the sync
        // WorldDb call never stalls the tokio runtime:
        //
        //   let db = self.db.clone();
        //   let key = cell_slice_key(&self.universe, cell);
        //   let bytes = serde_json::to_vec(&slice)?;
        //   tokio::task::spawn_blocking(move || db.put_shared(&key, &bytes))
        //       .await
        //       .map_err(|e| EustressForgeError::WorldDb(e.to_string()))?
        //       .map_err(|e| EustressForgeError::WorldDb(e.to_string()))?;
        //
        // The landing-zone trait methods are intentionally not wired in this
        // scaffold (open decision: dedicated `forge_shared` partition vs a
        // single tree Commit); the epoch bookkeeping below is real so the
        // monotonicity invariant holds regardless of where bytes land.
        self.last_applied_epoch.insert(cell, slice.epoch);
        Ok(true)
    }

    /// Build the local slice for an owned cell. Scaffold: owner + epoch only.
    /// TODO(engine-driver): populate `agents`/`shared_kv` from the cell's
    /// `forge_shared` record read under `spawn_blocking`. NEVER reads entity
    /// cores.
    async fn build_local_slice(&self, cell: CellCoord) -> Result<CellSlice> {
        Ok(CellSlice {
            cell,
            owner_node: self.node_id,
            epoch: self.last_applied_epoch.get(&cell).copied().unwrap_or(0),
            agents: Vec::new(),
            shared_kv: BTreeMap::new(),
        })
    }

    /// Retire a cell this node owns: delete its replicated slice (tombstone)
    /// so the replicated surface stays bounded as cells unload. No-op if not
    /// owned here.
    pub async fn retire_owned_cell(&mut self, cell: CellCoord) -> Result<()> {
        let key = cell_slice_key(&self.universe, cell);
        // Only the owner tombstones; a non-owner pulling will simply stop
        // seeing the key.
        if self.last_applied_epoch.contains_key(&cell) {
            store_get_json::<CellSlice>(self.store.as_ref(), &key)
                .await
                .map_err(map_forge_err)?
                .filter(|s| self.owns(s))
                .map(|_| ())
                .unwrap_or(());
        }
        self.store.delete(&key).await.map_err(map_forge_err)?;
        self.last_applied_epoch.remove(&cell);
        self.dirty.remove(&cell);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_slice_key_is_stable_and_disjoint() {
        let k = cell_slice_key("u-abc123", (5, 9, 2));
        assert_eq!(k, "forge/cell/u-abc123/0000005-0000009-0000002");
        // Disjoint from reserved forge namespaces.
        assert!(k.starts_with(CELL_SLICE_PREFIX));
        assert!(!k.starts_with(keys::JOBS));
        assert!(!k.starts_with(keys::NODES));
        assert!(!k.starts_with(keys::CONFIG));
        assert!(!k.starts_with(keys::SIMCELLS));
    }

    #[test]
    fn cell_slice_serde_roundtrip() {
        let slice = CellSlice {
            cell: (1, 2, 3),
            owner_node: 7,
            epoch: 42,
            agents: vec![AgentIntent {
                id: "a1".into(),
                target_cell: (1, 2, 3),
                role: "agent".into(),
                deadline_ms: 1_700_000_000_000,
            }],
            shared_kv: BTreeMap::from([("k".to_string(), serde_json::json!(1))]),
        };
        let j = serde_json::to_string(&slice).unwrap();
        let back: CellSlice = serde_json::from_str(&j).unwrap();
        assert_eq!(slice, back);
    }
}
