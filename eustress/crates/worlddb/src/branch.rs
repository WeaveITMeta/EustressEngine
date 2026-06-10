//! Copy-on-write branches over any [`WorldDb`] — the "fork the world,
//! perturb it, simulate forward, keep or discard" primitive.
//!
//! A [`BranchHandle`] wraps a parent `Arc<dyn WorldDb>` with an
//! in-memory overlay. Reads fall through to the parent when the overlay
//! has no opinion; writes land ONLY in the overlay. The parent is never
//! touched until [`BranchHandle::commit`] — and a discarded branch
//! ([`BranchHandle::discard`] or plain drop) costs the parent nothing.
//! Crucially there is **no duplication**: a branch over a 10M-entity
//! world allocates memory proportional to its *perturbations*, not to
//! the world.
//!
//! ```text
//!            ┌──────────────┐  read-through   ┌─────────────────┐
//!   reads ──▶│   overlay    │ ──── miss ────▶ │ parent WorldDb  │
//!            │ (BTreeMaps)  │                 │ (Fjall, or      │
//!   writes ─▶│  + tombstones│                 │  another branch)│
//!            └──────────────┘                 └─────────────────┘
//! ```
//!
//! `BranchHandle` itself implements [`WorldDb`], so branches **nest**:
//! `Arc::new(branch).branch()` forks a fork. That is the tree-search
//! shape the AI decision loop wants — one base world, N candidate
//! futures, each refining into sub-futures, none of them paying for a
//! copy.
//!
//! ## Semantics
//!
//! - **Tombstones** — deleting a key the parent holds records a
//!   tombstone; reads then report the key absent even though the parent
//!   still stores it.
//! - **Despawn masking** — a despawned entity's parent components stay
//!   hidden for the rest of the branch's life; a later `put_component`
//!   on the same entity re-adds just that component (the overlay value
//!   wins over the despawn mask, matching `apply_commit` replay order).
//! - **commit()** — replays the overlay into the parent. Component ops
//!   (puts/deletes/despawns) go through ONE `apply_commit`, so the
//!   component lane is atomic. The other partitions (tree, datastore,
//!   cores, voxels, uuid stores, meta) replay via their individual
//!   trait methods — cross-partition atomicity is NOT provided (the
//!   underlying trait has none either); callers needing it should
//!   serialise commits themselves.
//! - **Instance cores** — Morton keys are position-derived, so core
//!   ops are replayed on commit **in the order they were issued**.
//!   Follow the same contract as the raw trait: when moving an entity
//!   that the parent already stores, `delete_instance_core` at the OLD
//!   position before `put_instance_core` at the new one.
//! - **Change stream** — a branch owns a private [`ChangeStream`];
//!   its `apply_commit` publishes deltas with synthetic [`TxId`]s.
//!   Parent subscribers see nothing until `commit()`.
//! - **Counts on branches** — [`WorldDb::count_instance_cores_capped`]
//!   and [`WorldDb::iter_all_classes`] return *estimates* when the
//!   overlay overlaps parent rows (exactness would need full parent
//!   scans). They are sizing heuristics, not invariants — exact
//!   enumeration paths (`iter_*`) are always exact.
//!
//! ## Why an in-memory overlay (and not Fjall snapshots)?
//!
//! Fjall snapshot semantics give cheap read-isolation, but a branch
//! also needs cheap *write*-isolation with discard — and dozens of
//! concurrent simulation branches must not contend the parent's commit
//! lock or WAL. A `BTreeMap` overlay is allocation-proportional to the
//! perturbation set, trivially `Send + Sync` behind one `RwLock`, and
//! drops in O(overlay). If a future workload wants overlays bigger
//! than RAM, the same `WorldDb`-over-`WorldDb` shape can swap the maps
//! for a private Fjall keyspace without touching callers.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::backend::{Commit, EntityId, TreeEntry, WorldDb};
use crate::changestream::{
    ChangeStream, CommitDelta, EntityChange, Filter, Subscription, TxId,
};
use crate::error::Result;
use crate::keys::{world_to_cell, ComponentTypeId, VOXEL_CHUNK_EDGE_STUDS};

/// Chunk size (studs) of the Morton cell grid the `entities` partition
/// keys instance cores with. Must match the backend's default encoder
/// (`MortonKeyEncoder`, chunk_size 256) so a branch's region filter
/// agrees with the parent's region scan.
const CORE_CELL_SIZE: f32 = 256.0;

/// One replayed instance-core operation. Kept as an ordered log (not
/// just a map) because core keys are position-derived: a move is
/// `delete(old_pos)` then `put(new_pos)`, and commit() must replay
/// that order against the parent or the old record leaks.
#[derive(Debug, Clone)]
enum CoreOp {
    Put {
        entity: EntityId,
        pos: (f32, f32, f32),
        bytes: Vec<u8>,
    },
    Delete {
        entity: EntityId,
        pos: (f32, f32, f32),
    },
}

/// Effective (read-side) state of one entity's instance core in the
/// overlay — the latest CoreOp wins.
#[derive(Debug, Clone)]
enum CoreState {
    Put { pos: (f32, f32, f32), bytes: Vec<u8> },
    Deleted,
}

/// The branch's private write set. Every map is `key → Some(value)`
/// (branch wrote) or `key → None` (branch tombstoned a parent key).
/// Absent key = no opinion → read-through.
#[derive(Default)]
struct Overlay {
    /// Component writes: `(entity, component) → value | tombstone`.
    components: BTreeMap<(u64, u16), Option<Vec<u8>>>,
    /// Entities despawned on this branch — masks ALL parent components.
    despawned: BTreeSet<u64>,
    /// Tree partition (path-keyed file mirror).
    files: BTreeMap<String, Option<Vec<u8>>>,
    /// Plain datastore values: `(store, scope, key)`.
    ds: BTreeMap<(String, String, String), Option<Vec<u8>>>,
    /// Ordered datastore values: `(store, scope, key) → (value, sort)`.
    ds_ord: BTreeMap<(String, String, String), Option<(Vec<u8>, i64)>>,
    /// Read-side core state per entity (latest op wins).
    cores: BTreeMap<u64, CoreState>,
    /// Ordered core-op log for commit replay (see [`CoreOp`]).
    core_ops: Vec<CoreOp>,
    /// Voxel chunks keyed by signed chunk coords.
    voxels: BTreeMap<(i32, i32, i32), Option<Vec<u8>>>,
    /// UUID-keyed primary store.
    uuid_cores: BTreeMap<[u8; 16], Option<Vec<u8>>>,
    /// `path → uuid` identity index.
    path_to_uuid: BTreeMap<String, Option<[u8; 16]>>,
    /// `uuid → path` reverse index.
    uuid_to_path: BTreeMap<[u8; 16], Option<String>>,
    /// `(class, uuid) → present?` markers.
    class_index: BTreeMap<(String, [u8; 16]), bool>,
    /// Meta partition.
    meta: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl Overlay {
    /// Total op count across every namespace — the "how perturbed is
    /// this branch" diagnostic and the digest's cheap companion.
    fn len(&self) -> usize {
        self.components.len()
            + self.despawned.len()
            + self.files.len()
            + self.ds.len()
            + self.ds_ord.len()
            + self.cores.len()
            + self.voxels.len()
            + self.uuid_cores.len()
            + self.path_to_uuid.len()
            + self.uuid_to_path.len()
            + self.class_index.len()
            + self.meta.len()
    }
}

/// A copy-on-write branch over a parent [`WorldDb`]. See module docs.
///
/// Create via [`WorldDbBranchExt::branch`] (on any `Arc<dyn WorldDb>`)
/// or [`BranchHandle::new`]. The handle is `Send + Sync` and itself
/// implements [`WorldDb`], so it slots anywhere a world does —
/// including as the parent of a deeper branch.
pub struct BranchHandle {
    parent: Arc<dyn WorldDb>,
    overlay: RwLock<Overlay>,
    changes: ChangeStream,
    /// Serial for the synthetic [`TxId`]s this branch's own
    /// `apply_commit` mints (parent tx ids are minted only on commit).
    tx_serial: AtomicU64,
}

impl BranchHandle {
    /// Fork a branch over `parent`. O(1) — nothing is copied.
    pub fn new(parent: Arc<dyn WorldDb>) -> Self {
        Self {
            parent,
            overlay: RwLock::new(Overlay::default()),
            changes: ChangeStream::new(),
            tx_serial: AtomicU64::new(1),
        }
    }

    /// Borrow the parent this branch reads through to.
    pub fn parent(&self) -> &Arc<dyn WorldDb> {
        &self.parent
    }

    /// True when the branch has recorded at least one write.
    pub fn is_dirty(&self) -> bool {
        self.overlay.read().len() > 0
    }

    /// Number of overlay entries across all namespaces — proportional
    /// to the branch's perturbation set, NOT to the parent world.
    pub fn overlay_len(&self) -> usize {
        self.overlay.read().len()
    }

    /// Deterministic blake3 digest of the branch's *effective overlay
    /// state* (tombstones included, replay log excluded). Two branches
    /// of the same parent with identical perturbations hash equal —
    /// which is exactly what the simulate-N-futures loop needs to
    /// dedupe / compare candidate outcomes without scanning worlds.
    pub fn digest(&self) -> [u8; 32] {
        let o = self.overlay.read();
        let mut h = blake3::Hasher::new();
        // Namespace tags keep `files{"a"→x}` from colliding with
        // `meta{"a"→x}`. Lengths are hashed before contents so
        // boundary-shifted concatenations can't collide.
        fn put(h: &mut blake3::Hasher, tag: u8, key: &[u8], val: Option<&[u8]>) {
            h.update(&[tag]);
            h.update(&(key.len() as u64).to_le_bytes());
            h.update(key);
            match val {
                Some(v) => {
                    h.update(&[1]);
                    h.update(&(v.len() as u64).to_le_bytes());
                    h.update(v);
                }
                None => {
                    h.update(&[0]);
                }
            }
        }
        for ((e, c), v) in &o.components {
            let mut k = [0u8; 10];
            k[..8].copy_from_slice(&e.to_le_bytes());
            k[8..].copy_from_slice(&c.to_le_bytes());
            put(&mut h, 1, &k, v.as_deref());
        }
        for e in &o.despawned {
            put(&mut h, 2, &e.to_le_bytes(), None);
        }
        for (p, v) in &o.files {
            put(&mut h, 3, p.as_bytes(), v.as_deref());
        }
        for ((s, sc, k), v) in &o.ds {
            let key = format!("{s}\x1f{sc}\x1f{k}");
            put(&mut h, 4, key.as_bytes(), v.as_deref());
        }
        for ((s, sc, k), v) in &o.ds_ord {
            let key = format!("{s}\x1f{sc}\x1f{k}");
            match v {
                Some((bytes, sort)) => {
                    let mut payload = sort.to_le_bytes().to_vec();
                    payload.extend_from_slice(bytes);
                    put(&mut h, 5, key.as_bytes(), Some(&payload));
                }
                None => put(&mut h, 5, key.as_bytes(), None),
            }
        }
        for (e, state) in &o.cores {
            match state {
                CoreState::Put { pos, bytes } => {
                    let mut payload = Vec::with_capacity(12 + bytes.len());
                    payload.extend_from_slice(&pos.0.to_le_bytes());
                    payload.extend_from_slice(&pos.1.to_le_bytes());
                    payload.extend_from_slice(&pos.2.to_le_bytes());
                    payload.extend_from_slice(bytes);
                    put(&mut h, 6, &e.to_le_bytes(), Some(&payload));
                }
                CoreState::Deleted => put(&mut h, 6, &e.to_le_bytes(), None),
            }
        }
        for ((cx, cy, cz), v) in &o.voxels {
            let mut k = [0u8; 12];
            k[..4].copy_from_slice(&cx.to_le_bytes());
            k[4..8].copy_from_slice(&cy.to_le_bytes());
            k[8..].copy_from_slice(&cz.to_le_bytes());
            put(&mut h, 7, &k, v.as_deref());
        }
        for (u, v) in &o.uuid_cores {
            put(&mut h, 8, u, v.as_deref());
        }
        for (p, v) in &o.path_to_uuid {
            put(&mut h, 9, p.as_bytes(), v.as_ref().map(|u| &u[..]));
        }
        for (u, v) in &o.uuid_to_path {
            put(&mut h, 10, u, v.as_ref().map(|s| s.as_bytes()));
        }
        for ((c, u), live) in &o.class_index {
            let mut k = c.as_bytes().to_vec();
            k.push(0x1f);
            k.extend_from_slice(u);
            put(&mut h, 11, &k, if *live { Some(&[1u8][..]) } else { None });
        }
        for (k, v) in &o.meta {
            put(&mut h, 12, k, v.as_deref());
        }
        *h.finalize().as_bytes()
    }

    /// [`Self::digest`] as lowercase hex — the wire/log-friendly form
    /// the batch-rollout API returns.
    pub fn digest_hex(&self) -> String {
        self.digest().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Throw the branch away. Equivalent to dropping it — provided so
    /// call sites read as the decision they are (`branch.discard()`
    /// after a rejected rollout, vs an accidental drop).
    pub fn discard(self) {}

    /// Replay the overlay into the parent and consume the branch.
    ///
    /// Component puts/deletes/despawns travel in ONE parent
    /// `apply_commit` (atomic, single change-stream delta). The other
    /// namespaces replay through their individual trait methods in a
    /// fixed order: despawn-safe component lane first, then cores (in
    /// op order), voxels, tree files, datastores, identity stores,
    /// meta. Returns the [`TxId`] of the component-lane commit, or a
    /// synthetic id when the branch touched no components.
    ///
    /// On error the parent may hold a PARTIAL replay (the trait offers
    /// no cross-partition transaction) — callers treat a failed commit
    /// as "world needs reconcile", same as any interrupted writer.
    pub fn commit(self) -> Result<TxId> {
        let overlay = self.overlay.into_inner();

        // ── component lane (atomic) ──────────────────────────────────
        let mut commit = Commit::new();
        for e in &overlay.despawned {
            commit.despawn(EntityId(*e));
        }
        for ((e, c), v) in &overlay.components {
            match v {
                Some(bytes) => {
                    commit.put_component(EntityId(*e), ComponentTypeId(*c), bytes.clone())
                }
                None => commit.delete_component(EntityId(*e), ComponentTypeId(*c)),
            }
        }
        let tx = if commit.is_empty() {
            TxId::synthetic(self.tx_serial.fetch_add(1, Ordering::Relaxed))
        } else {
            self.parent.apply_commit(commit)?
        };

        // ── instance cores: replay the LOG in order (Morton keys are
        //    position-derived; see module docs) ─────────────────────
        for op in &overlay.core_ops {
            match op {
                CoreOp::Put { entity, pos, bytes } => {
                    self.parent.put_instance_core(*entity, *pos, bytes)?
                }
                CoreOp::Delete { entity, pos } => {
                    self.parent.delete_instance_core(*entity, *pos)?
                }
            }
        }

        // ── voxels ───────────────────────────────────────────────────
        for ((cx, cy, cz), v) in &overlay.voxels {
            match v {
                Some(bytes) => self.parent.put_voxel_chunk(*cx, *cy, *cz, bytes)?,
                // The trait has no voxel delete — a tombstoned chunk is
                // overwritten with empty bytes (loaders treat an empty
                // chunk as absent).
                None => self.parent.put_voxel_chunk(*cx, *cy, *cz, &[])?,
            }
        }

        // ── tree files ───────────────────────────────────────────────
        for (path, v) in &overlay.files {
            match v {
                Some(bytes) => self.parent.put_file(path, bytes)?,
                None => self.parent.delete_file(path)?,
            }
        }

        // ── datastores ───────────────────────────────────────────────
        for ((s, sc, k), v) in &overlay.ds {
            match v {
                Some(bytes) => self.parent.ds_set(s, sc, k, bytes)?,
                None => {
                    self.parent.ds_remove(s, sc, k)?;
                }
            }
        }
        for ((s, sc, k), v) in &overlay.ds_ord {
            match v {
                Some((bytes, sort)) => self.parent.ds_set_sorted(s, sc, k, bytes, *sort)?,
                None => {
                    self.parent.ds_remove(s, sc, k)?;
                }
            }
        }

        // ── identity stores ──────────────────────────────────────────
        for (u, v) in &overlay.uuid_cores {
            match v {
                Some(bytes) => self.parent.put_entity_core_by_uuid(u, bytes)?,
                None => self.parent.delete_entity_by_uuid(u)?,
            }
        }
        for (p, v) in &overlay.path_to_uuid {
            match v {
                Some(u) => self.parent.put_path_to_uuid(p, u)?,
                None => self.parent.delete_path_to_uuid(p)?,
            }
        }
        for (u, v) in &overlay.uuid_to_path {
            match v {
                Some(p) => self.parent.put_uuid_to_path(u, p)?,
                None => self.parent.delete_uuid_to_path(u)?,
            }
        }
        for ((c, u), live) in &overlay.class_index {
            if *live {
                self.parent.put_class_index(c, u)?;
            } else {
                self.parent.delete_class_index(c, u)?;
            }
        }

        // ── meta ─────────────────────────────────────────────────────
        for (k, v) in &overlay.meta {
            match v {
                Some(bytes) => self.parent.put_meta(k, bytes)?,
                None => {
                    // No meta delete on the trait — tombstone as empty.
                    self.parent.put_meta(k, &[])?;
                }
            }
        }

        Ok(tx)
    }
}

impl WorldDb for BranchHandle {
    fn apply_commit(&self, commit: Commit) -> Result<TxId> {
        if commit.is_empty() {
            return Ok(TxId::synthetic(
                self.tx_serial.fetch_add(1, Ordering::Relaxed),
            ));
        }
        let tx = TxId::synthetic(self.tx_serial.fetch_add(1, Ordering::Relaxed));
        let mut changes = Vec::with_capacity(commit.ops.len());
        let mut byte_size = 0usize;
        {
            let mut o = self.overlay.write();
            for op in &commit.ops {
                match op {
                    crate::backend::CommitOp::Put {
                        entity,
                        component,
                        value,
                    } => {
                        byte_size += value.len();
                        o.components
                            .insert((entity.0, component.0), Some(value.clone()));
                        changes.push(EntityChange::Put {
                            entity: *entity,
                            component: *component,
                            value_preview: value.iter().take(64).copied().collect(),
                        });
                    }
                    crate::backend::CommitOp::Delete { entity, component } => {
                        o.components.insert((entity.0, component.0), None);
                        changes.push(EntityChange::Removed {
                            entity: *entity,
                            component: *component,
                        });
                    }
                    crate::backend::CommitOp::DespawnEntity { entity } => {
                        // Purge earlier branch writes for the entity, then
                        // mask the parent. Later puts re-add (see module
                        // docs §despawn masking).
                        let e = entity.0;
                        o.components.retain(|(oe, _), _| *oe != e);
                        o.despawned.insert(e);
                        changes.push(EntityChange::Despawned { entity: *entity });
                    }
                }
            }
        }
        self.changes.publish(CommitDelta {
            tx_id: tx,
            changes,
            byte_size,
        });
        Ok(tx)
    }

    fn get_component(
        &self,
        entity: EntityId,
        component: ComponentTypeId,
    ) -> Result<Option<Vec<u8>>> {
        let o = self.overlay.read();
        if let Some(v) = o.components.get(&(entity.0, component.0)) {
            return Ok(v.clone());
        }
        if o.despawned.contains(&entity.0) {
            return Ok(None);
        }
        drop(o);
        self.parent.get_component(entity, component)
    }

    fn iter_component(
        &self,
        component: ComponentTypeId,
    ) -> Result<Box<dyn Iterator<Item = Result<(EntityId, Vec<u8>)>> + '_>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for item in self.parent.iter_component(component)? {
            let (e, v) = item?;
            if o.despawned.contains(&e.0) || o.components.contains_key(&(e.0, component.0)) {
                continue; // masked or overridden below
            }
            merged.insert(e.0, v);
        }
        for ((e, c), v) in &o.components {
            if *c != component.0 {
                continue;
            }
            match v {
                Some(bytes) => {
                    merged.insert(*e, bytes.clone());
                }
                None => {
                    merged.remove(e);
                }
            }
        }
        drop(o);
        Ok(Box::new(
            merged
                .into_iter()
                .map(|(e, v)| Ok((EntityId(e), v)))
                .collect::<Vec<_>>()
                .into_iter(),
        ))
    }

    fn flush(&self) -> Result<()> {
        // The overlay is in-memory by design; there is nothing durable
        // to sync until commit(). Deliberately NOT forwarded to the
        // parent — a branch must never cost the parent IO.
        Ok(())
    }

    fn subscribe(&self, filter: Filter) -> Subscription {
        self.changes.subscribe(filter)
    }

    fn change_stream(&self) -> &ChangeStream {
        &self.changes
    }

    // ── tree partition ────────────────────────────────────────────────

    fn put_file(&self, rel_path: &str, bytes: &[u8]) -> Result<()> {
        self.overlay
            .write()
            .files
            .insert(normalise_rel(rel_path), Some(bytes.to_vec()));
        Ok(())
    }

    fn get_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        let p = normalise_rel(rel_path);
        if let Some(v) = self.overlay.read().files.get(&p) {
            return Ok(v.clone());
        }
        self.parent.get_file(&p)
    }

    fn delete_file(&self, rel_path: &str) -> Result<()> {
        self.overlay
            .write()
            .files
            .insert(normalise_rel(rel_path), None);
        Ok(())
    }

    fn list_dir(&self, rel_dir: &str) -> Result<Vec<TreeEntry>> {
        let dir = normalise_rel(rel_dir);
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        // name → is_dir, parent first, then overlay corrections.
        let mut seen: BTreeMap<String, bool> = BTreeMap::new();
        for entry in self.parent.list_dir(&dir)? {
            seen.insert(entry.name, entry.is_dir);
        }
        let o = self.overlay.read();
        for (path, v) in &o.files {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            match rest.find('/') {
                Some(slash) => {
                    // A live file deeper down implies the child dir
                    // exists on this branch. (A tombstone deeper down
                    // does NOT remove the dir — the parent may hold
                    // other files under it; dirs are inferred.)
                    if v.is_some() {
                        seen.entry(rest[..slash].to_string()).or_insert(true);
                    }
                }
                None => match v {
                    Some(_) => {
                        seen.insert(rest.to_string(), false);
                    }
                    None => {
                        // Tombstoned leaf — hide it unless the parent
                        // had a dir of the same name.
                        if seen.get(rest) == Some(&false) {
                            seen.remove(rest);
                        }
                    }
                },
            }
        }
        drop(o);
        Ok(seen
            .into_iter()
            .map(|(name, is_dir)| TreeEntry {
                rel_path: if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}{name}")
                },
                name,
                is_dir,
            })
            .collect())
    }

    fn tree_is_empty(&self) -> Result<bool> {
        if self.overlay.read().files.values().any(|v| v.is_some()) {
            return Ok(false);
        }
        // Tombstones-only overlay: approximate with the parent's answer
        // (a branch that tombstoned EVERY parent file still reports
        // non-empty; acceptable for a simulation scratch space).
        self.parent.tree_is_empty()
    }

    fn iter_tree(&self) -> Result<Box<dyn Iterator<Item = Result<(String, Vec<u8>)>> + '_>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for item in self.parent.iter_tree()? {
            let (path, bytes) = item?;
            if o.files.contains_key(&path) {
                continue;
            }
            merged.insert(path, bytes);
        }
        for (path, v) in &o.files {
            if let Some(bytes) = v {
                merged.insert(path.clone(), bytes.clone());
            }
        }
        drop(o);
        Ok(Box::new(
            merged
                .into_iter()
                .map(Ok)
                .collect::<Vec<_>>()
                .into_iter(),
        ))
    }

    // ── datastore partition ──────────────────────────────────────────

    fn ds_get(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let k = (store.to_string(), scope.to_string(), key.to_string());
        if let Some(v) = self.overlay.read().ds.get(&k) {
            return Ok(v.clone());
        }
        self.parent.ds_get(store, scope, key)
    }

    fn ds_set(&self, store: &str, scope: &str, key: &str, value: &[u8]) -> Result<()> {
        self.overlay.write().ds.insert(
            (store.to_string(), scope.to_string(), key.to_string()),
            Some(value.to_vec()),
        );
        Ok(())
    }

    fn ds_remove(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let prior = self.ds_get(store, scope, key)?;
        self.overlay.write().ds.insert(
            (store.to_string(), scope.to_string(), key.to_string()),
            None,
        );
        Ok(prior)
    }

    fn ds_update(
        &self,
        store: &str,
        scope: &str,
        key: &str,
        max_retries: u32,
        transform: &mut dyn FnMut(Option<Vec<u8>>) -> Option<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>> {
        // Single-writer per branch: the overlay write lock IS the CAS
        // serialisation, so the first attempt always observes the
        // latest value — retries only matter when the transform aborts
        // (mirrors the Fjall backend under its commit lock).
        let k = (store.to_string(), scope.to_string(), key.to_string());
        let mut attempt = 0;
        loop {
            let current = self.ds_get(store, scope, key)?;
            match transform(current) {
                Some(new_val) => {
                    self.overlay
                        .write()
                        .ds
                        .insert(k, Some(new_val.clone()));
                    return Ok(Some(new_val));
                }
                None => {
                    attempt += 1;
                    if attempt > max_retries {
                        return Ok(None);
                    }
                }
            }
        }
    }

    fn ds_range(
        &self,
        store: &str,
        scope: &str,
        ascending: bool,
        limit: usize,
        min: Option<i64>,
        max: Option<i64>,
        cursor: &str,
    ) -> Result<Vec<(String, Vec<u8>, i64)>> {
        let o = self.overlay.read();
        let touched: Vec<(&(String, String, String), &Option<(Vec<u8>, i64)>)> = o
            .ds_ord
            .iter()
            .filter(|((s, sc, _), _)| s == store && sc == scope)
            .collect();
        if touched.is_empty() {
            drop(o);
            return self
                .parent
                .ds_range(store, scope, ascending, limit, min, max, cursor);
        }
        // Overlay overlaps this store: fetch a widened parent page,
        // apply overrides/tombstones/additions, re-sort, re-paginate.
        // NOTE under deep pagination (non-empty cursor) the widened
        // parent page may not contain every overlay neighbour — page
        // boundaries on a branch are approximate when the branch wrote
        // into the same ordered store it is paging. Simulation rollouts
        // don't page mid-branch; documented trade-off.
        let parent_rows = self.parent.ds_range(
            store,
            scope,
            ascending,
            limit.saturating_add(touched.len()),
            min,
            max,
            cursor,
        )?;
        let mut rows: BTreeMap<String, (Vec<u8>, i64)> = BTreeMap::new();
        for (k, v, sort) in parent_rows {
            rows.insert(k, (v, sort));
        }
        for ((_, _, k), v) in touched {
            match v {
                Some((bytes, sort)) => {
                    let in_range = min.map_or(true, |m| *sort >= m)
                        && max.map_or(true, |m| *sort <= m);
                    if in_range {
                        rows.insert(k.clone(), (bytes.clone(), *sort));
                    } else {
                        rows.remove(k);
                    }
                }
                None => {
                    rows.remove(k);
                }
            }
        }
        drop(o);
        let mut out: Vec<(String, Vec<u8>, i64)> = rows
            .into_iter()
            .map(|(k, (v, s))| (k, v, s))
            .collect();
        out.sort_by(|a, b| {
            let ord = a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0));
            if ascending {
                ord
            } else {
                ord.reverse()
            }
        });
        if !cursor.is_empty() {
            if let Some(pos) = out.iter().position(|(k, _, _)| k == cursor) {
                out.drain(..=pos);
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    fn ds_set_sorted(
        &self,
        store: &str,
        scope: &str,
        key: &str,
        value: &[u8],
        sort: i64,
    ) -> Result<()> {
        self.overlay.write().ds_ord.insert(
            (store.to_string(), scope.to_string(), key.to_string()),
            Some((value.to_vec(), sort)),
        );
        Ok(())
    }

    // ── instance cores ───────────────────────────────────────────────

    fn put_instance_core(&self, entity: EntityId, pos: (f32, f32, f32), core: &[u8]) -> Result<()> {
        let mut o = self.overlay.write();
        o.cores.insert(
            entity.0,
            CoreState::Put {
                pos,
                bytes: core.to_vec(),
            },
        );
        o.core_ops.push(CoreOp::Put {
            entity,
            pos,
            bytes: core.to_vec(),
        });
        Ok(())
    }

    fn delete_instance_core(&self, entity: EntityId, pos: (f32, f32, f32)) -> Result<()> {
        let mut o = self.overlay.write();
        o.cores.insert(entity.0, CoreState::Deleted);
        o.core_ops.push(CoreOp::Delete { entity, pos });
        Ok(())
    }

    fn iter_instance_cores(&self) -> Result<Vec<(EntityId, Vec<u8>)>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for (e, v) in self.parent.iter_instance_cores()? {
            if o.cores.contains_key(&e.0) {
                continue;
            }
            merged.insert(e.0, v);
        }
        for (e, state) in &o.cores {
            if let CoreState::Put { bytes, .. } = state {
                merged.insert(*e, bytes.clone());
            }
        }
        Ok(merged
            .into_iter()
            .map(|(e, v)| (EntityId(e), v))
            .collect())
    }

    fn iter_instance_cores_in_region(
        &self,
        cx: (u32, u32),
        cy: (u32, u32),
        cz: (u32, u32),
    ) -> Result<Vec<(EntityId, Vec<u8>)>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for (e, v) in self.parent.iter_instance_cores_in_region(cx, cy, cz)? {
            if o.cores.contains_key(&e.0) {
                continue;
            }
            merged.insert(e.0, v);
        }
        for (e, state) in &o.cores {
            if let CoreState::Put { pos, bytes } = state {
                let c = (
                    world_to_cell(pos.0, CORE_CELL_SIZE),
                    world_to_cell(pos.1, CORE_CELL_SIZE),
                    world_to_cell(pos.2, CORE_CELL_SIZE),
                );
                let inside = c.0 >= cx.0
                    && c.0 <= cx.1
                    && c.1 >= cy.0
                    && c.1 <= cy.1
                    && c.2 >= cz.0
                    && c.2 <= cz.1;
                if inside {
                    merged.insert(*e, bytes.clone());
                }
            }
        }
        Ok(merged
            .into_iter()
            .map(|(e, v)| (EntityId(e), v))
            .collect())
    }

    fn count_instance_cores_capped(&self, cap: usize) -> Result<usize> {
        // Estimate (see module docs §counts): parent's capped count
        // adjusted by the overlay's net put/delete balance. Exact would
        // require probing whether each overlay op shadows a parent row.
        let o = self.overlay.read();
        let mut puts = 0isize;
        let mut dels = 0isize;
        for state in o.cores.values() {
            match state {
                CoreState::Put { .. } => puts += 1,
                CoreState::Deleted => dels += 1,
            }
        }
        drop(o);
        let base = self.parent.count_instance_cores_capped(cap)? as isize;
        Ok((base + puts - dels).clamp(0, cap as isize) as usize)
    }

    // ── voxels ───────────────────────────────────────────────────────

    fn put_voxel_chunk(&self, cx: i32, cy: i32, cz: i32, bytes: &[u8]) -> Result<()> {
        self.overlay
            .write()
            .voxels
            .insert((cx, cy, cz), Some(bytes.to_vec()));
        Ok(())
    }

    fn get_voxel_chunk(&self, cx: i32, cy: i32, cz: i32) -> Result<Option<Vec<u8>>> {
        if let Some(v) = self.overlay.read().voxels.get(&(cx, cy, cz)) {
            return Ok(v.clone());
        }
        self.parent.get_voxel_chunk(cx, cy, cz)
    }

    fn iter_voxel_chunks_in_region(
        &self,
        min: (f32, f32, f32),
        max: (f32, f32, f32),
    ) -> Result<Vec<((i32, i32, i32), Vec<u8>)>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<(i32, i32, i32), Vec<u8>> = BTreeMap::new();
        for (coord, bytes) in self.parent.iter_voxel_chunks_in_region(min, max)? {
            if o.voxels.contains_key(&coord) {
                continue;
            }
            merged.insert(coord, bytes);
        }
        // Same world→chunk mapping the backend uses (floor-divide by
        // the chunk edge), so the branch filter agrees with the parent
        // scan's box.
        let e = VOXEL_CHUNK_EDGE_STUDS;
        let lo = (
            (min.0 / e).floor() as i32,
            (min.1 / e).floor() as i32,
            (min.2 / e).floor() as i32,
        );
        let hi = (
            (max.0 / e).floor() as i32,
            (max.1 / e).floor() as i32,
            (max.2 / e).floor() as i32,
        );
        for ((cx, cy, cz), v) in &o.voxels {
            if let Some(bytes) = v {
                let inside = *cx >= lo.0
                    && *cx <= hi.0
                    && *cy >= lo.1
                    && *cy <= hi.1
                    && *cz >= lo.2
                    && *cz <= hi.2;
                if inside {
                    merged.insert((*cx, *cy, *cz), bytes.clone());
                }
            }
        }
        Ok(merged.into_iter().collect())
    }

    fn iter_all_voxel_chunks(&self) -> Result<Vec<((i32, i32, i32), Vec<u8>)>> {
        let o = self.overlay.read();
        let mut merged: BTreeMap<(i32, i32, i32), Vec<u8>> = BTreeMap::new();
        for (coord, bytes) in self.parent.iter_all_voxel_chunks()? {
            if o.voxels.contains_key(&coord) {
                continue;
            }
            merged.insert(coord, bytes);
        }
        for (coord, v) in &o.voxels {
            if let Some(bytes) = v {
                merged.insert(*coord, bytes.clone());
            }
        }
        Ok(merged.into_iter().collect())
    }

    fn has_voxel_chunks(&self) -> bool {
        if self.overlay.read().voxels.values().any(|v| v.is_some()) {
            return true;
        }
        self.parent.has_voxel_chunks()
    }

    // ── uuid identity stores ─────────────────────────────────────────

    fn put_entity_core_by_uuid(&self, uuid: &[u8; 16], core_bytes: &[u8]) -> Result<()> {
        self.overlay
            .write()
            .uuid_cores
            .insert(*uuid, Some(core_bytes.to_vec()));
        Ok(())
    }

    fn get_entity_core_by_uuid(&self, uuid: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        if let Some(v) = self.overlay.read().uuid_cores.get(uuid) {
            return Ok(v.clone());
        }
        self.parent.get_entity_core_by_uuid(uuid)
    }

    fn delete_entity_by_uuid(&self, uuid: &[u8; 16]) -> Result<()> {
        self.overlay.write().uuid_cores.insert(*uuid, None);
        Ok(())
    }

    fn path_to_uuid(&self, rel_path: &str) -> Result<Option<[u8; 16]>> {
        let p = normalise_rel(rel_path);
        if let Some(v) = self.overlay.read().path_to_uuid.get(&p) {
            return Ok(*v);
        }
        self.parent.path_to_uuid(&p)
    }

    fn put_path_to_uuid(&self, rel_path: &str, uuid: &[u8; 16]) -> Result<()> {
        self.overlay
            .write()
            .path_to_uuid
            .insert(normalise_rel(rel_path), Some(*uuid));
        Ok(())
    }

    fn delete_path_to_uuid(&self, rel_path: &str) -> Result<()> {
        self.overlay
            .write()
            .path_to_uuid
            .insert(normalise_rel(rel_path), None);
        Ok(())
    }

    fn uuid_to_path(&self, uuid: &[u8; 16]) -> Result<Option<String>> {
        if let Some(v) = self.overlay.read().uuid_to_path.get(uuid) {
            return Ok(v.clone());
        }
        self.parent.uuid_to_path(uuid)
    }

    fn put_uuid_to_path(&self, uuid: &[u8; 16], rel_path: &str) -> Result<()> {
        self.overlay
            .write()
            .uuid_to_path
            .insert(*uuid, Some(normalise_rel(rel_path)));
        Ok(())
    }

    fn delete_uuid_to_path(&self, uuid: &[u8; 16]) -> Result<()> {
        self.overlay.write().uuid_to_path.insert(*uuid, None);
        Ok(())
    }

    fn put_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        self.overlay
            .write()
            .class_index
            .insert((class_name.to_string(), *uuid), true);
        Ok(())
    }

    fn delete_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        self.overlay
            .write()
            .class_index
            .insert((class_name.to_string(), *uuid), false);
        Ok(())
    }

    fn iter_class(&self, class_name: &str) -> Result<Vec<[u8; 16]>> {
        let o = self.overlay.read();
        let mut set: BTreeSet<[u8; 16]> =
            self.parent.iter_class(class_name)?.into_iter().collect();
        for ((c, u), live) in &o.class_index {
            if c != class_name {
                continue;
            }
            if *live {
                set.insert(*u);
            } else {
                set.remove(u);
            }
        }
        Ok(set.into_iter().collect())
    }

    fn iter_class_capped(&self, class_name: &str, cap: usize) -> Result<Vec<[u8; 16]>> {
        let touched = {
            let o = self.overlay.read();
            o.class_index.keys().any(|(c, _)| c == class_name)
        };
        if !touched {
            return self.parent.iter_class_capped(class_name, cap);
        }
        // Overlay touched this class — do the exact merge, then cap.
        let mut v = self.iter_class(class_name)?;
        v.truncate(cap);
        Ok(v)
    }

    fn iter_all_classes(&self) -> Result<Vec<(String, usize)>> {
        let o = self.overlay.read();
        let mut counts: BTreeMap<String, isize> = self
            .parent
            .iter_all_classes()?
            .into_iter()
            .map(|(c, n)| (c, n as isize))
            .collect();
        // Estimate (see module docs §counts): an overlay add counts as
        // new unless the parent already stores the uuid's core; a
        // tombstone subtracts on the inverse condition.
        for ((c, u), live) in &o.class_index {
            let in_parent = self
                .parent
                .get_entity_core_by_uuid(u)
                .ok()
                .flatten()
                .is_some();
            let n = counts.entry(c.clone()).or_insert(0);
            if *live && !in_parent {
                *n += 1;
            } else if !*live && in_parent {
                *n -= 1;
            }
        }
        Ok(counts
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(c, n)| (c, n as usize))
            .collect())
    }

    // ── meta ─────────────────────────────────────────────────────────

    fn get_meta(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(v) = self.overlay.read().meta.get(key) {
            return Ok(v.clone());
        }
        self.parent.get_meta(key)
    }

    fn put_meta(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.overlay
            .write()
            .meta
            .insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }
}

/// `Arc<dyn WorldDb>::branch()` — the ergonomic fork entry point the
/// engine plugin and the AI bridge call. Implemented as an extension
/// trait (not a default method on [`WorldDb`]) because forking needs an
/// owned `Arc` of the parent, which a `&self` trait method cannot mint.
pub trait WorldDbBranchExt {
    /// Fork a copy-on-write branch over this world. O(1).
    fn branch(&self) -> BranchHandle;
}

impl WorldDbBranchExt for Arc<dyn WorldDb> {
    fn branch(&self) -> BranchHandle {
        BranchHandle::new(self.clone())
    }
}

/// Mirror of the backend's path normalisation: forward slashes, no
/// leading/trailing separators — so overlay keys collide correctly
/// with parent tree keys.
fn normalise_rel(rel: &str) -> String {
    rel.trim_matches(|c| c == '/' || c == '\\').replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fjall_backend::FjallWorldDb;

    fn fresh_parent() -> (Arc<dyn WorldDb>, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "eustress_branch_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db: Arc<dyn WorldDb> = Arc::new(FjallWorldDb::open(&tmp).unwrap());
        (db, tmp)
    }

    fn put_one(db: &dyn WorldDb, e: u64, c: ComponentTypeId, v: &[u8]) {
        let mut commit = Commit::new();
        commit.put_component(EntityId(e), c, v.to_vec());
        db.apply_commit(commit).unwrap();
    }

    #[test]
    fn read_through_overlay_write_and_parent_isolation() {
        let (parent, tmp) = fresh_parent();
        put_one(parent.as_ref(), 1, ComponentTypeId::TRANSFORM, b"parent-t");

        let branch = parent.branch();
        // Read-through: branch sees the parent's value.
        assert_eq!(
            branch
                .get_component(EntityId(1), ComponentTypeId::TRANSFORM)
                .unwrap()
                .as_deref(),
            Some(&b"parent-t"[..])
        );

        // Branch write: branch sees the new value, parent is untouched.
        put_one(&branch, 1, ComponentTypeId::TRANSFORM, b"branch-t");
        assert_eq!(
            branch
                .get_component(EntityId(1), ComponentTypeId::TRANSFORM)
                .unwrap()
                .as_deref(),
            Some(&b"branch-t"[..])
        );
        assert_eq!(
            parent
                .get_component(EntityId(1), ComponentTypeId::TRANSFORM)
                .unwrap()
                .as_deref(),
            Some(&b"parent-t"[..])
        );

        // Discard: parent still untouched.
        branch.discard();
        assert_eq!(
            parent
                .get_component(EntityId(1), ComponentTypeId::TRANSFORM)
                .unwrap()
                .as_deref(),
            Some(&b"parent-t"[..])
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn despawn_masks_parent_and_commit_replays() {
        let (parent, tmp) = fresh_parent();
        put_one(parent.as_ref(), 7, ComponentTypeId::TRANSFORM, b"alive");
        put_one(parent.as_ref(), 8, ComponentTypeId::TRANSFORM, b"also");

        let branch = parent.branch();
        let mut c = Commit::new();
        c.despawn(EntityId(7));
        branch.apply_commit(c).unwrap();

        // Masked on the branch, alive in the parent.
        assert_eq!(
            branch
                .get_component(EntityId(7), ComponentTypeId::TRANSFORM)
                .unwrap(),
            None
        );
        assert!(parent
            .get_component(EntityId(7), ComponentTypeId::TRANSFORM)
            .unwrap()
            .is_some());

        // iter_component on the branch hides 7, keeps 8.
        let seen: Vec<u64> = branch
            .iter_component(ComponentTypeId::TRANSFORM)
            .unwrap()
            .map(|r| r.unwrap().0 .0)
            .collect();
        assert_eq!(seen, vec![8]);

        // Commit → the despawn lands in the parent.
        put_one(&branch, 9, ComponentTypeId::TRANSFORM, b"new-on-branch");
        branch.commit().unwrap();
        assert_eq!(
            parent
                .get_component(EntityId(7), ComponentTypeId::TRANSFORM)
                .unwrap(),
            None
        );
        assert_eq!(
            parent
                .get_component(EntityId(9), ComponentTypeId::TRANSFORM)
                .unwrap()
                .as_deref(),
            Some(&b"new-on-branch"[..])
        );
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn tree_files_overlay_and_nested_branches() {
        let (parent, tmp) = fresh_parent();
        parent
            .put_file("Workspace/A/_instance.toml", b"a-parent")
            .unwrap();

        let b1 = parent.branch();
        b1.put_file("Workspace/B/_instance.toml", b"b-branch").unwrap();
        b1.delete_file("Workspace/A/_instance.toml").unwrap();

        assert_eq!(b1.get_file("Workspace/A/_instance.toml").unwrap(), None);
        assert_eq!(
            b1.get_file("Workspace/B/_instance.toml").unwrap().as_deref(),
            Some(&b"b-branch"[..])
        );
        // Parent untouched.
        assert!(parent
            .get_file("Workspace/A/_instance.toml")
            .unwrap()
            .is_some());

        // Nested: a branch of a branch reads through both layers.
        let b1: Arc<dyn WorldDb> = Arc::new(b1);
        let b2 = b1.branch();
        assert_eq!(b2.get_file("Workspace/A/_instance.toml").unwrap(), None);
        assert_eq!(
            b2.get_file("Workspace/B/_instance.toml").unwrap().as_deref(),
            Some(&b"b-branch"[..])
        );
        b2.put_file("Workspace/C/_instance.toml", b"c-deep").unwrap();
        assert!(b1.get_file("Workspace/C/_instance.toml").unwrap().is_none());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn digest_distinguishes_and_matches_perturbations() {
        let (parent, tmp) = fresh_parent();
        let a = parent.branch();
        let b = parent.branch();
        assert_eq!(a.digest(), b.digest(), "clean branches hash equal");

        put_one(&a, 1, ComponentTypeId::TRANSFORM, b"x");
        assert_ne!(a.digest(), b.digest(), "perturbed branch diverges");

        put_one(&b, 1, ComponentTypeId::TRANSFORM, b"x");
        assert_eq!(a.digest(), b.digest(), "identical perturbations converge");
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn instance_core_overlay_and_commit_replay_order() {
        let (parent, tmp) = fresh_parent();
        let pos_a = (10.0, 0.0, 10.0);
        parent
            .put_instance_core(EntityId(42), pos_a, b"core-a")
            .unwrap();

        let branch = parent.branch();
        // Move: delete at old pos, put at new pos (trait contract).
        let pos_b = (500.0, 0.0, 500.0);
        branch.delete_instance_core(EntityId(42), pos_a).unwrap();
        branch.put_instance_core(EntityId(42), pos_b, b"core-b").unwrap();

        // Branch sees exactly one core with the new bytes.
        let cores = branch.iter_instance_cores().unwrap();
        assert_eq!(cores.len(), 1);
        assert_eq!(cores[0].1.as_slice(), b"core-b");
        // Parent still has the old record.
        assert_eq!(
            parent.iter_instance_cores().unwrap()[0].1.as_slice(),
            b"core-a"
        );

        // Commit replays delete-then-put: parent ends with ONE record.
        branch.commit().unwrap();
        let cores = parent.iter_instance_cores().unwrap();
        assert_eq!(cores.len(), 1, "old Morton record must not leak");
        assert_eq!(cores[0].1.as_slice(), b"core-b");
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn datastore_overlay_with_ordered_range_merge() {
        let (parent, tmp) = fresh_parent();
        parent.ds_set_sorted("lb", "global", "p1", b"100", 100).unwrap();
        parent.ds_set_sorted("lb", "global", "p2", b"200", 200).unwrap();

        let branch = parent.branch();
        branch.ds_set_sorted("lb", "global", "p3", b"150", 150).unwrap();
        branch.ds_set_sorted("lb", "global", "p1", b"300", 300).unwrap();

        let rows = branch
            .ds_range("lb", "global", true, 10, None, None, "")
            .unwrap();
        let keys: Vec<&str> = rows.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["p3", "p2", "p1"], "merged + re-sorted by sort value");

        // Parent leaderboard unchanged.
        let rows = parent
            .ds_range("lb", "global", true, 10, None, None, "")
            .unwrap();
        assert_eq!(rows.len(), 2);
        let _ = std::fs::remove_dir_all(tmp);
    }
}
