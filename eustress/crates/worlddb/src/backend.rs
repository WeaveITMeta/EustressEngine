//! [`WorldDb`] — the storage abstraction every consumer codes against.
//!
//! Designed so the implementation can swap (Fjall version bump, an
//! in-memory backend for tests, even a different LSM) without touching
//! call sites. Operations are bytes-in / bytes-out; serialisation
//! belongs to the caller (the engine wraps Bevy components in rkyv
//! before calling [`WorldDb::put_component`]).
//!
//! ## Threading model
//!
//! `WorldDb` is `Send + Sync` and intended to be wrapped in `Arc`. The
//! Bevy plugin holds the `Arc<dyn WorldDb>` as a `Resource`. Reads and
//! writes are concurrent — the backend serialises internally where
//! needed.

use std::path::Path;

use crate::changestream::{ChangeStream, Filter, Subscription, TxId};
use crate::error::Result;
use crate::keys::ComponentTypeId;

/// The 64-bit identity of an entity inside a [`WorldDb`]. Mirrors
/// `bevy::ecs::entity::Entity::to_bits()` so engine code can convert
/// either direction with a single `u64` round-trip.
///
/// The key encoder owns the byte layout (see [`crate::keys`]) — this
/// type only carries the integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u64);

impl From<u64> for EntityId {
    fn from(v: u64) -> Self {
        EntityId(v)
    }
}
impl From<EntityId> for u64 {
    fn from(e: EntityId) -> Self {
        e.0
    }
}

/// Batch of pending writes that commit atomically. Mirrors a Fjall
/// `WriteBatch` but lives behind the trait so callers don't depend on
/// the backend type.
pub struct Commit {
    pub(crate) ops: Vec<CommitOp>,
}

pub(crate) enum CommitOp {
    Put {
        entity: EntityId,
        component: ComponentTypeId,
        value: Vec<u8>,
    },
    Delete {
        entity: EntityId,
        component: ComponentTypeId,
    },
    DespawnEntity {
        entity: EntityId,
    },
}

impl Commit {
    /// Create an empty commit. Build it up with the `put*` / `delete*`
    /// helpers, then hand to [`WorldDb::apply_commit`].
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// Stage a component write. `value` is opaque bytes — rkyv archives
    /// in the engine, raw bytes in tests / CLI tools.
    pub fn put_component(
        &mut self,
        entity: EntityId,
        component: ComponentTypeId,
        value: impl Into<Vec<u8>>,
    ) {
        self.ops.push(CommitOp::Put {
            entity,
            component,
            value: value.into(),
        });
    }

    /// Stage a component removal.
    pub fn delete_component(&mut self, entity: EntityId, component: ComponentTypeId) {
        self.ops.push(CommitOp::Delete { entity, component });
    }

    /// Stage an entire-entity despawn. Implementations walk every
    /// component key prefix for the entity and tombstone in one shot.
    pub fn despawn(&mut self, entity: EntityId) {
        self.ops.push(CommitOp::DespawnEntity { entity });
    }

    /// True when nothing has been staged. `apply_commit` short-circuits
    /// on empty commits so the change-stream doesn't fire spurious
    /// no-op deltas.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Number of staged operations — for diagnostics + size budgeting.
    pub fn len(&self) -> usize {
        self.ops.len()
    }
}

impl Default for Commit {
    fn default() -> Self {
        Self::new()
    }
}

/// The storage trait every consumer codes against. See module docs.
pub trait WorldDb: Send + Sync + 'static {
    /// Apply a batch of operations atomically. On success returns the
    /// [`TxId`] assigned by the backend — the same id is broadcast on
    /// the change stream so subscribers can correlate.
    fn apply_commit(&self, commit: Commit) -> Result<TxId>;

    /// Read one component value for an entity. Returns `Ok(None)` when
    /// the entity exists but lacks that component (the trait makes no
    /// distinction between "no entity" and "no component" — both are
    /// missing keys).
    fn get_component(
        &self,
        entity: EntityId,
        component: ComponentTypeId,
    ) -> Result<Option<Vec<u8>>>;

    /// Range-scan every (entity, component_value) pair for one
    /// component type. Used by the importer and by Studio's
    /// "load all Transforms" boot path.
    ///
    /// The iterator is NOT `Send` — fjall's prefix iterator borrows
    /// the partition handle non-thread-safely. Engine callers run
    /// this from a single Bevy system; CLI tooling collects eagerly
    /// when crossing threads.
    fn iter_component(
        &self,
        component: ComponentTypeId,
    ) -> Result<Box<dyn Iterator<Item = Result<(EntityId, Vec<u8>)>> + '_>>;

    /// Force a flush of pending writes to disk. Called by Studio on
    /// explicit Save and from the engine plugin on graceful shutdown.
    /// Normal operation persists via the WAL — this is the
    /// "ensure SSTable visibility" gate.
    fn flush(&self) -> Result<()>;

    /// Subscribe to per-commit deltas. The filter narrows which
    /// `EntityChange` events the subscriber sees. Returns a handle
    /// the caller drops to unsubscribe.
    fn subscribe(&self, filter: Filter) -> Subscription;

    /// Borrow the change-stream so engine systems can publish synthetic
    /// events (Workshop tool snapshots, AI annotations) onto the same
    /// fabric. Synthetic deltas carry `TxId::synthetic(...)` so
    /// archival pipelines can tell them apart from genuine commits.
    fn change_stream(&self) -> &ChangeStream;

    // ── Tree partition — path-keyed file mirror ──────────────────────
    //
    // The `SpaceSource` abstraction in the engine reads Space content
    // (every `_instance.toml`, `_service.toml`, script, GUI TOML, .md,
    // mesh) from here instead of `std::fs` once a world is migrated.
    // Keys are forward-slash relative paths from the Space root; this
    // preserves the FULL hierarchy (services + parent/child folders)
    // so a Fjall-sourced load reconstructs the identical scene tree.

    /// Store one file's bytes at a Space-relative path
    /// (e.g. `Workspace/MegaTower_Core/_instance.toml`). Forward-slash
    /// separators; the backend normalises. Overwrites if present.
    fn put_file(&self, rel_path: &str, bytes: &[u8]) -> Result<()>;

    /// Read one file's bytes. `Ok(None)` when the path isn't in the
    /// tree (caller falls back to disk or treats as absent).
    fn get_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>>;

    /// Remove one file from the tree (entity despawn, script delete).
    fn delete_file(&self, rel_path: &str) -> Result<()>;

    /// Remove every tree entry under `rel_prefix` (the prefix itself or
    /// any key beginning `rel_prefix/`). Returns the count removed.
    ///
    /// This is the reconciliation primitive: it makes a subtree
    /// idempotently regenerable so a producer (the benchmark grid
    /// generator, a procedural Space, a re-import) can replace its
    /// output instead of appending onto stale keys. Without it the
    /// `tree` partition is append-only and accumulates deleted parts
    /// that "are no longer there" — keys for entities removed or
    /// shrunk out on a later regenerate.
    ///
    /// Default implementation drains via [`WorldDb::iter_tree`] +
    /// [`WorldDb::delete_file`]; backends with a native range-delete
    /// override for a prefix scan instead of a full-tree scan.
    fn clear_tree_prefix(&self, rel_prefix: &str) -> Result<usize> {
        let p = rel_prefix
            .trim_matches(|c| c == '/' || c == '\\')
            .replace('\\', "/");
        let with_slash = format!("{p}/");
        let mut victims: Vec<String> = Vec::new();
        for item in self.iter_tree()? {
            let (path, _bytes) = item?;
            if path == p || path.starts_with(&with_slash) {
                victims.push(path);
            }
        }
        let removed = victims.len();
        for path in victims {
            self.delete_file(&path)?;
        }
        Ok(removed)
    }

    /// List the immediate children of a tree directory. `rel_dir`
    /// `""` lists the Space root. Returns files AND inferred
    /// subdirectories (a key `a/b/c` under prefix `a` yields dir `b`).
    fn list_dir(&self, rel_dir: &str) -> Result<Vec<TreeEntry>>;

    /// True when the tree partition has no entries — the signal the
    /// engine uses to decide "first open, seed from disk".
    fn tree_is_empty(&self) -> Result<bool>;

    /// Iterate every (rel_path, bytes) pair in the tree. Used by the
    /// bake path and by tooling. Not `Send` (fjall iterator borrow).
    fn iter_tree(&self) -> Result<Box<dyn Iterator<Item = Result<(String, Vec<u8>)>> + '_>>;

    // ── DataStore partition — Roblox-parity script persistence ───────
    //
    // Phase 8. Backs `DataStoreService` / `OrderedDataStore` /
    // `DataStorePages` (see [`crate::datastore`]). Separate partition
    // from `entities`/`tree` so game-state writes never contend with
    // scene streaming. Bytes in/out — the script bridge serialises
    // Luau/Rune values to JSON before calling.

    /// Read a datastore key. `scope` is Roblox's per-store namespace
    /// (default `"global"`). `Ok(None)` = absent.
    fn ds_get(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Unconditional write (`SetAsync`).
    fn ds_set(&self, store: &str, scope: &str, key: &str, value: &[u8]) -> Result<()>;

    /// Delete a key (`RemoveAsync`). Returns the prior value if any.
    fn ds_remove(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Compare-and-swap (`UpdateAsync`). The transform receives the
    /// current value (or `None`) and returns the new value, or `None`
    /// to abort. Retries up to `max_retries` on a concurrent write so
    /// two scripts updating the same key serialise correctly.
    fn ds_update(
        &self,
        store: &str,
        scope: &str,
        key: &str,
        max_retries: u32,
        transform: &mut dyn FnMut(Option<Vec<u8>>) -> Option<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>>;

    /// Ordered range scan for `OrderedDataStore` / `DataStorePages`.
    /// Keys in an ordered store carry an i64 sort value; this returns
    /// `(key, raw_value_bytes, sort_i64)` within `[min, max]`,
    /// ascending or descending, capped at `limit`. `cursor` is the
    /// last key seen (exclusive) for pagination, or empty to start.
    fn ds_range(
        &self,
        store: &str,
        scope: &str,
        ascending: bool,
        limit: usize,
        min: Option<i64>,
        max: Option<i64>,
        cursor: &str,
    ) -> Result<Vec<(String, Vec<u8>, i64)>>;

    /// Write an ordered entry — value bytes plus the i64 sort key the
    /// leaderboard ranks on.
    fn ds_set_sorted(
        &self,
        store: &str,
        scope: &str,
        key: &str,
        value: &[u8],
        sort: i64,
    ) -> Result<()>;

    // ── Binary-ECS instance core — Morton-keyed scalable entity store ──
    //
    // The "binary ECS" home for file-less, scalable entities (the
    // representation router's BinaryEcs side): a whole entity's rkyv
    // `ArchInstanceCore` stored as ONE component
    // ([`ComponentTypeId::INSTANCE_CORE`]), keyed by world position via
    // the Morton spatial encoder so a region scan returns a spatial
    // neighbourhood. File-bearing entities never live here — they stay in
    // the `tree` partition. The default impls let non-Fjall test backends
    // compile; `FjallWorldDb` overrides all three.

    /// Store an entity's `ArchInstanceCore` bytes, Morton-keyed by `pos`
    /// (the entity's translation — the same value encoded inside the
    /// core). Overwrites the record at that exact position+entity.
    fn put_instance_core(&self, entity: EntityId, pos: (f32, f32, f32), core: &[u8]) -> Result<()> {
        let _ = (entity, pos, core);
        Err(crate::error::Error::Other(
            "put_instance_core not supported by this backend".into(),
        ))
    }

    /// Remove a stored `ArchInstanceCore`. `pos` MUST be the position the
    /// record was last written at — its Morton key is position-derived,
    /// so a *move* deletes at the OLD position before putting at the new
    /// one (the engine tracks last-written position to supply it).
    fn delete_instance_core(&self, entity: EntityId, pos: (f32, f32, f32)) -> Result<()> {
        let _ = (entity, pos);
        Ok(())
    }

    /// Eagerly collect every stored `ArchInstanceCore` as
    /// `(EntityId, core_bytes)` — the binary-ECS boot-load path. Eager
    /// (not a borrowed iterator) so the engine scene-spawner can consume
    /// it across a Bevy system boundary.
    fn iter_instance_cores(&self) -> Result<Vec<(EntityId, Vec<u8>)>> {
        Ok(Vec::new())
    }
}

/// One entry returned by [`WorldDb::list_dir`] — a file or an
/// inferred subdirectory at one tree level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// Leaf name (no slashes) — folder name or file name.
    pub name: String,
    /// Full Space-relative path (forward slashes).
    pub rel_path: String,
    /// True for an inferred directory, false for a stored file.
    pub is_dir: bool,
}

/// Open or create a world at `path`. The directory MUST already exist —
/// `WorldDb::open` does NOT create the `.eustress` container itself
/// (that's the engine's responsibility, since it also has to populate
/// `header.bin`, `schema/`, and `assets/`).
///
/// Returns `Arc<dyn WorldDb>` so the result drops into the Bevy
/// `Resource` slot directly.
pub fn open(
    path: &Path,
) -> Result<std::sync::Arc<dyn WorldDb>> {
    let db = crate::fjall_backend::FjallWorldDb::open(path)?;
    Ok(std::sync::Arc::new(db))
}
