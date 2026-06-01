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

    /// Collect every `INSTANCE_CORE` whose Morton cell lies in the
    /// inclusive 3D cell box `[cx.0..=cx.1] × [cy.0..=cy.1] × [cz.0..=cz.1]`.
    /// Cell coords are the 21-bit values produced by `world_to_cell` at
    /// chunk_size 256.0 — the SAME encoding `put_instance_core` uses — so a
    /// caller enumerates the camera's cell box and gets exactly the cores in
    /// that spatial neighbourhood. The read side of camera-locality
    /// streaming (boot-load reads the whole world; this reads a region).
    fn iter_instance_cores_in_region(
        &self,
        cx: (u32, u32),
        cy: (u32, u32),
        cz: (u32, u32),
    ) -> Result<Vec<(EntityId, Vec<u8>)>> {
        let _ = (cx, cy, cz);
        Ok(Vec::new())
    }

    /// Count `INSTANCE_CORE` records, stopping early once `cap` is reached;
    /// returns `min(actual, cap)`. Lets the engine choose "boot-load all"
    /// (small Space) vs "stream by camera" (large Space) without
    /// materializing millions of cores just to size the Space.
    fn count_instance_cores_capped(&self, cap: usize) -> Result<usize> {
        let _ = cap;
        Ok(0)
    }

    // ── UUID-keyed primary store — IDENTITY.md Wave 2.1 ──────────────
    //
    // The `entities_uuid` partition keys each entity's `ArchInstanceCore`
    // by the persistent 16-byte UUID (not the session-local `EntityId`).
    // This is the long-term home for the rkyv archive-model record; the
    // Morton-keyed `entities` partition above keys by position for
    // spatial scans and is allowed to coexist during Wave 2 (the migration
    // writes to NEW partitions; nothing in the existing `entities`
    // partition is touched).
    //
    // Secondary indexes (`path_to_uuid`, `uuid_to_path`, `class_index`)
    // are all derivable from the primary store via `rebuild_indexes()` —
    // crash-recovery primitive.
    //
    // The default impls let non-Fjall test backends compile; the
    // production `FjallWorldDb` overrides every one.

    /// Write an entity's rkyv `ArchInstanceCore` bytes, keyed by its
    /// 16-byte UUID (IDENTITY.md §5.2). Overwrites any existing row at
    /// this key — surfaces 1+4 (TOML import, Roblox re-import) rely on
    /// UPDATE-in-place semantics.
    fn put_entity_core_by_uuid(&self, uuid: &[u8; 16], core_bytes: &[u8]) -> Result<()> {
        let _ = (uuid, core_bytes);
        Err(crate::error::Error::Other(
            "put_entity_core_by_uuid not supported by this backend".into(),
        ))
    }

    /// Read one entity's rkyv `ArchInstanceCore` bytes by its UUID.
    /// `Ok(None)` when no record exists for this uuid.
    fn get_entity_core_by_uuid(&self, uuid: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        let _ = uuid;
        Ok(None)
    }

    /// Remove an entity from the UUID-keyed primary store. The secondary
    /// indexes (`path_to_uuid`, `uuid_to_path`, `class_index`) must be
    /// updated by the caller in the same atomic commit (see the
    /// engine-side `delete_instance` helper).
    fn delete_entity_by_uuid(&self, uuid: &[u8; 16]) -> Result<()> {
        let _ = uuid;
        Ok(())
    }

    /// Look up a path → uuid mapping (IDENTITY.md §5.3 `path_to_uuid`).
    /// `Ok(None)` when no entity lives at this path right now.
    fn path_to_uuid(&self, rel_path: &str) -> Result<Option<[u8; 16]>> {
        let _ = rel_path;
        Ok(None)
    }

    /// Write the `path -> uuid` mapping for one entity. Idempotent —
    /// re-writing the same key with the same value is a no-op.
    fn put_path_to_uuid(&self, rel_path: &str, uuid: &[u8; 16]) -> Result<()> {
        let _ = (rel_path, uuid);
        Err(crate::error::Error::Other(
            "put_path_to_uuid not supported by this backend".into(),
        ))
    }

    /// Drop the `path -> uuid` mapping. Used when a TOML is deleted off
    /// disk (file_watcher) or when an entity is moved/renamed.
    fn delete_path_to_uuid(&self, rel_path: &str) -> Result<()> {
        let _ = rel_path;
        Ok(())
    }

    /// Reverse lookup: uuid → last-known relative path (IDENTITY.md
    /// §5.3 `uuid_to_path`). Used by the Explorer to render rows, by
    /// error messages, and by the file-watcher to know what disk path to
    /// write to when a live ECS edit fires.
    fn uuid_to_path(&self, uuid: &[u8; 16]) -> Result<Option<String>> {
        let _ = uuid;
        Ok(None)
    }

    /// Write the `uuid -> path` reverse mapping.
    fn put_uuid_to_path(&self, uuid: &[u8; 16], rel_path: &str) -> Result<()> {
        let _ = (uuid, rel_path);
        Err(crate::error::Error::Other(
            "put_uuid_to_path not supported by this backend".into(),
        ))
    }

    /// Drop the `uuid -> path` reverse mapping.
    fn delete_uuid_to_path(&self, uuid: &[u8; 16]) -> Result<()> {
        let _ = uuid;
        Ok(())
    }

    /// Write a `class_index/<class>/<uuid>` empty marker so a future
    /// `iter_class(class)` returns this entity. Idempotent.
    fn put_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        let _ = (class_name, uuid);
        Err(crate::error::Error::Other(
            "put_class_index not supported by this backend".into(),
        ))
    }

    /// Drop the `class_index/<class>/<uuid>` marker. Caller is
    /// responsible for knowing the entity's class — see `delete_instance`
    /// for the canonical pattern (read the core first, then delete).
    fn delete_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        let _ = (class_name, uuid);
        Ok(())
    }

    /// Iterate every uuid registered under `class_name` (IDENTITY.md §5.3
    /// `class_index`). One-shot collect into a `Vec` so the iterator
    /// doesn't borrow the backend across the engine system boundary.
    fn iter_class(&self, class_name: &str) -> Result<Vec<[u8; 16]>> {
        let _ = class_name;
        Ok(Vec::new())
    }

    /// Like [`iter_class`] but stops after collecting `cap` uuids. The
    /// virtual DB Explorer (Phase 4) lists only a bounded page of a class,
    /// so a 10M-entity `Part` bucket must NOT materialize 10M uuids (160 MB)
    /// just to show the first 500 rows. Prefix-scans `class_index/<class>\x1f`
    /// and early-exits at `cap`.
    fn iter_class_capped(&self, class_name: &str, cap: usize) -> Result<Vec<[u8; 16]>> {
        // Default: fall back to the full scan, then truncate. Backends that
        // can early-exit (FjallWorldDb) override this for real boundedness.
        let mut v = self.iter_class(class_name)?;
        v.truncate(cap);
        Ok(v)
    }

    /// Enumerate every distinct class present in `class_index` together
    /// with how many entities each holds, as `(class_name, count)`. Powers
    /// the virtual DB-backed Explorer (Phase 4): it lists class buckets +
    /// counts without materializing any cores, so a 10M-entity Space shows
    /// `Part (10000000)` instantly. One full prefix-scan of the (small,
    /// empty-valued) `class_index` partition — cost scales with entity
    /// count, but each entry is a bare key, so it is far cheaper than
    /// reading cores. Returns sorted by class name for stable UI ordering.
    fn iter_all_classes(&self) -> Result<Vec<(String, usize)>> {
        Ok(Vec::new())
    }

    /// Read or write a meta-partition key — used by the migration to
    /// stamp `migration_checkpoint` every 1000 entities for resume-from-
    /// checkpoint (IDENTITY.md §6.3).
    fn get_meta(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let _ = key;
        Ok(None)
    }

    /// Write one byte-keyed meta entry. Used by the migration to record
    /// `migration_checkpoint = "done"` at the end of a clean pass.
    fn put_meta(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let _ = (key, value);
        Err(crate::error::Error::Other(
            "put_meta not supported by this backend".into(),
        ))
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
