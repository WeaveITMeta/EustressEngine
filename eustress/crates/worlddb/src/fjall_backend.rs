//! Fjall 2.x backend for [`crate::WorldDb`].
//!
//! Layout inside `world.fjalldb/`:
//!
//! - `entities` partition — flat KV space keyed by
//!   [`crate::keys::KeyEncoder::encode_component`].
//! - `meta` partition — `header.bin` mirror + per-class
//!   `schema_version` registry + tx-counter checkpoint.
//!
//! Two partitions today; Phase 2 adds a `spatial` partition once the
//! Morton encoder lands, and Phase 5 adds a `chunks` partition that
//! caches `.echk` bytes for already-baked chunks.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::backend::{Commit, CommitOp, EntityId, TreeEntry, WorldDb};
use crate::changestream::{ChangeStream, CommitDelta, EntityChange, Filter, Subscription, TxId};
use crate::error::Result;
use crate::header::{WorldHeader, WorldSchemaVersion};
use crate::keys::{ComponentTypeId, FlatKeyEncoder, KeyEncoder};

/// Normalise a caller-supplied relative path to the tree key form:
/// forward slashes, no leading/trailing slash, no `.`/`..` segments.
fn normalise_rel(rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// DataStore plain key: `{store}\x1f{scope}\x1f{key}`. `\x1f` (unit
/// separator) can't appear in a Roblox store/scope/key name, so this
/// is collision-free without escaping.
fn ds_key(store: &str, scope: &str, key: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(store.len() + scope.len() + key.len() + 2);
    k.extend_from_slice(store.as_bytes());
    k.push(0x1f);
    k.extend_from_slice(scope.as_bytes());
    k.push(0x1f);
    k.extend_from_slice(key.as_bytes());
    k
}

/// Prefix covering every key in `{store}\x1f{scope}`.
fn ds_scope_prefix(store: &str, scope: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(store.len() + scope.len() + 2);
    k.extend_from_slice(store.as_bytes());
    k.push(0x1f);
    k.extend_from_slice(scope.as_bytes());
    k.push(0x1f);
    k
}

/// Order-preserving i64 → 8 bytes: flip the sign bit so lexicographic
/// byte order equals numeric order (negatives sort below positives).
fn sort_to_be8(v: i64) -> [u8; 8] {
    ((v as u64) ^ 0x8000_0000_0000_0000).to_be_bytes()
}
fn be8_to_sort(b: &[u8]) -> i64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    (u64::from_be_bytes(a) ^ 0x8000_0000_0000_0000) as i64
}

/// Ordered key: `{store}\x1f{scope}\x1f{sort_be8}\x1f{key}`.
fn ds_ord_key(store: &str, scope: &str, sort: i64, key: &str) -> Vec<u8> {
    let mut k = ds_scope_prefix(store, scope);
    k.extend_from_slice(&sort_to_be8(sort));
    k.push(0x1f);
    k.extend_from_slice(key.as_bytes());
    k
}

/// Encode the UUID-keyed entity-core key for `entities_uuid` /
/// `uuid_to_path` (IDENTITY.md §5.2). Today the key IS the 16-byte uuid;
/// this helper exists so a schema-version prefix can be added later
/// without rewriting call sites (matches the `FlatKeyEncoder::TAG` /
/// `MortonKeyEncoder::TAG` discipline elsewhere in this crate).
fn encode_uuid_key(uuid: &[u8; 16]) -> [u8; 16] {
    *uuid
}

/// Encode the `class_index/<class>\x1f<uuid>` key. The `\x1f` (unit
/// separator) can't appear in a class name (TOML keys / Rust idents),
/// so this is collision-free without escaping.
fn encode_class_index_key(class_name: &str, uuid: &[u8; 16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(class_name.len() + 1 + 16);
    out.extend_from_slice(class_name.as_bytes());
    out.push(0x1f);
    out.extend_from_slice(uuid);
    out
}

/// Production [`WorldDb`] backend on Fjall 2.x. See module docs for
/// the on-disk layout.
pub struct FjallWorldDb {
    /// The owned multiplexer substrate (the `eustress-fjall` fork).
    /// Held as the entry point and to keep the multiplexed keyspace
    /// alive; the proven partition code below operates on raw handles
    /// obtained THROUGH it. The next incremental step routes commits
    /// onto its per-store replication feed for multiplayer/distributed
    /// synchronisation.
    #[allow(dead_code)]
    efj: eustress_fjall::EustressFjall,
    /// Multiplexer store handles, kept so every write path can emit a
    /// sequenced replication delta onto the per-store feed AFTER its
    /// (proven, cross-store-atomic) raw commit succeeds. This is what
    /// turns entity / DataModel-tree / DataStore mutations into the
    /// multiplayer/distributed replication stream.
    s_entities: eustress_fjall::StoreHandle,
    s_tree: eustress_fjall::StoreHandle,
    s_datastore: eustress_fjall::StoreHandle,
    s_datastore_ord: eustress_fjall::StoreHandle,
    /// Multiplexer store for the new UUID-keyed primary store + indexes.
    /// IDENTITY.md Wave 2.1.
    s_entities_uuid: eustress_fjall::StoreHandle,
    s_path_to_uuid: eustress_fjall::StoreHandle,
    s_uuid_to_path: eustress_fjall::StoreHandle,
    s_class_index: eustress_fjall::StoreHandle,
    /// Multiplexer store for the Wave 9.A voxel-chunk terrain partition.
    s_voxels: eustress_fjall::StoreHandle,
    /// Multiplexer stores for the Data Platform partitions, so each write
    /// emits a sequenced replication delta after its durable commit.
    s_datasets: eustress_fjall::StoreHandle,
    s_timeseries: eustress_fjall::StoreHandle,
    keyspace: fjall::Keyspace,
    entities: fjall::PartitionHandle,
    meta: fjall::PartitionHandle,
    /// Path-keyed file mirror of the whole Space tree. Keys are
    /// normalised forward-slash relative paths; values are raw file
    /// bytes. This is what `SpaceSource::Fjall` reads through.
    tree: fjall::PartitionHandle,
    /// Phase 8 — Roblox-parity DataStore. Plain entries:
    /// `{store}\x1f{scope}\x1f{key}` → value (ordered stores prefix the
    /// value with an 8-byte order-preserving sort tag).
    datastore: fjall::PartitionHandle,
    /// Ordered index for `OrderedDataStore` range scans:
    /// `{store}\x1f{scope}\x1f{sort_be8}\x1f{key}` → value.
    datastore_ord: fjall::PartitionHandle,

    // ── IDENTITY.md Wave 2.1 partitions ───────────────────────────────
    /// Primary UUID-keyed entity-core store. Keys are 16-byte raw UUIDs;
    /// values are tagged rkyv `ArchInstanceCore` archives (same encoding
    /// as the Morton-keyed `INSTANCE_CORE` rows in `entities`). The
    /// existing `entities` partition stays untouched in Wave 2.1.
    entities_uuid: fjall::PartitionHandle,
    /// Secondary index: forward-slash relative path → 16-byte UUID.
    /// `Fjall::get(rel_path.as_bytes())` answers "what entity lives at
    /// this path right now?" in one point-get.
    path_to_uuid: fjall::PartitionHandle,
    /// Secondary index: 16-byte UUID → forward-slash relative path
    /// (utf-8). Used by Explorer renderers and error messages.
    uuid_to_path: fjall::PartitionHandle,
    /// Secondary index: `<class_name>\x1f<uuid_16>` → empty marker.
    /// Prefix-scan over `<class_name>\x1f` returns every uuid registered
    /// under that class — IDENTITY.md §5.3.
    class_index: fjall::PartitionHandle,

    // ── Wave 9.A — terrain voxel partition ────────────────────────────
    /// Voxel-chunk store. Keys are `V | ver | morton63(biased cx,cy,cz)`
    /// (see [`crate::keys::encode_voxel_chunk_key`]); values are the opaque
    /// LZ4 material+occupancy chunk bytes the Roblox terrain importer
    /// produces. Morton chunk-coord keys make a region scan a spatial
    /// chunk-window request, 1:1 with the entity streaming model.
    voxels: fjall::PartitionHandle,

    // ── Data Platform — dataset-blob + timeseries partitions ──────────
    /// Materialized columnar Parquet blobs, read-whole-value, keyed by a
    /// 16-byte dataset id ([`crate::keys::encode_dataset_key`]). Opened
    /// KV-separation-tuned so multi-MB blobs land in the value log.
    datasets: fjall::PartitionHandle,
    /// Per-series time-ordered append rows, keyed by
    /// [`crate::keys::encode_timeseries_key`]; a series range scan == a
    /// time-window query.
    timeseries: fjall::PartitionHandle,

    /// Key layout. Boxed `dyn` so the engine plugin can swap encoders
    /// at open time (flat today, Morton in Phase 2).
    encoder: Box<dyn KeyEncoder>,

    /// Monotonic commit counter. Loaded from `meta:tx_counter` at
    /// open; advanced atomically per commit; persisted on flush /
    /// graceful shutdown.
    tx_counter: AtomicU64,

    /// Mutex held briefly during commit to serialise the
    /// "compute tx_id → write keys → publish delta" sequence so
    /// subscribers see deltas in tx-id order matching the SSTable
    /// visibility.
    commit_lock: Mutex<()>,

    change_stream: ChangeStream,
}

impl FjallWorldDb {
    /// Tag for the meta key that holds the current tx counter.
    const META_TX_COUNTER: &'static [u8] = b"tx_counter";

    /// Tag for the meta key that mirrors the header.bin schema
    /// version — lets `WorldDb` reject a DB whose on-disk layout
    /// disagrees with its header.bin without re-reading the file.
    const META_SCHEMA: &'static [u8] = b"schema_version";

    /// Open or create a Fjall world database at `path`. `path` is the
    /// `world.fjalldb/` directory INSIDE the `.eustress` container —
    /// the engine has already verified the container shape and
    /// loaded `header.bin` before calling here.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_encoder(path, Box::new(FlatKeyEncoder::default()))
    }

    /// Open with a specific [`KeyEncoder`]. Phase 2 callers pass
    /// `MortonKeyEncoder`; today's path always uses the flat encoder.
    pub fn open_with_encoder(path: &Path, encoder: Box<dyn KeyEncoder>) -> Result<Self> {
        let _span = tracing::info_span!("worlddb.open", path = %path.display()).entered();

        // Enter through the owned `eustress-fjall` multiplexer fork:
        // each partition is now a multiplexed logical store (concurrent,
        // snapshot-isolated, replication-aware). The proven partition
        // code below keeps operating on raw handles obtained through
        // the multiplexer — own the entry point first, port internals
        // incrementally (the migration strategy).
        let efj = eustress_fjall::EustressFjall::open(path)
            .map_err(|e| crate::error::Error::Other(format!("eustress-fjall open: {e}")))?;
        let store = |name: &str| -> Result<eustress_fjall::StoreHandle> {
            efj.store(name)
                .map_err(|e| crate::error::Error::Other(format!("eustress-fjall store {name}: {e}")))
        };
        let s_entities = store("entities")?;
        let s_meta = store("meta")?;
        let s_tree = store("tree")?;
        let s_datastore = store("datastore")?;
        let s_datastore_ord = store("datastore_ord")?;
        // IDENTITY.md Wave 2.1 partitions. Opening these is additive — a
        // pre-Wave-2 world with no entries in any of them stays loadable;
        // they only start filling after migration runs.
        let s_entities_uuid = store("entities_uuid")?;
        let s_path_to_uuid = store("path_to_uuid")?;
        let s_uuid_to_path = store("uuid_to_path")?;
        let s_class_index = store("class_index")?;
        // Wave 9.A voxel-chunk terrain partition. Additive — a pre-Wave-9
        // world with no voxel rows stays loadable; it only fills when the
        // terrain importer redirects chunk writes here.
        let s_voxels = store("voxels")?;
        // Data Platform partitions. Additive — a world predating the Data
        // Platform has no rows in either; they fill only when the platform
        // writes. `datasets` is KV-separation-tuned (multi-MB Parquet blobs
        // → value log, far less LSM compaction churn) with a large scan
        // block size; `timeseries` uses a scan-tuned block size for fast
        // range reads. These create-time options are persisted on FIRST
        // open and ignored on later opens (fjall semantics). Call
        // `store_with_opts` directly — the `store` closure above hardcodes
        // the default options, which would silently drop this tuning.
        let s_datasets = efj
            .store_with_opts(
                "datasets",
                fjall::PartitionCreateOptions::default()
                    .block_size(64 * 1024)
                    .compression(fjall::CompressionType::Lz4)
                    .with_kv_separation(
                        fjall::KvSeparationOptions::default().separation_threshold(4 * 1024),
                    ),
            )
            .map_err(|e| crate::error::Error::Other(format!("eustress-fjall store datasets: {e}")))?;
        let s_timeseries = efj
            .store_with_opts(
                "timeseries",
                fjall::PartitionCreateOptions::default()
                    .block_size(32 * 1024)
                    .compression(fjall::CompressionType::Lz4),
            )
            .map_err(|e| {
                crate::error::Error::Other(format!("eustress-fjall store timeseries: {e}"))
            })?;

        let keyspace = s_entities.raw_keyspace();
        let entities = s_entities.raw_partition();
        let meta = s_meta.raw_partition();
        let tree = s_tree.raw_partition();
        let datastore = s_datastore.raw_partition();
        let datastore_ord = s_datastore_ord.raw_partition();
        let entities_uuid = s_entities_uuid.raw_partition();
        let path_to_uuid = s_path_to_uuid.raw_partition();
        let uuid_to_path = s_uuid_to_path.raw_partition();
        let class_index = s_class_index.raw_partition();
        let voxels = s_voxels.raw_partition();
        let datasets = s_datasets.raw_partition();
        let timeseries = s_timeseries.raw_partition();

        // Load the persisted tx counter; fall back to GENESIS for a
        // fresh world.
        let tx_counter = match meta.get(Self::META_TX_COUNTER)? {
            Some(bytes) if bytes.len() == 8 => {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes);
                u64::from_le_bytes(arr)
            }
            _ => TxId::GENESIS.0,
        };

        // Cross-check schema version against header.bin's claim.
        // If absent (fresh world), stamp ours.
        let on_disk_schema = match meta.get(Self::META_SCHEMA)? {
            Some(bytes) if bytes.len() == 2 => {
                let mut arr = [0u8; 2];
                arr.copy_from_slice(&bytes);
                Some(WorldSchemaVersion(u16::from_le_bytes(arr)))
            }
            _ => None,
        };
        match on_disk_schema {
            Some(v) if v.is_future(WorldSchemaVersion::CURRENT) => {
                return Err(crate::error::Error::SchemaUnsupported(format!(
                    "world.fjalldb claims schema v{}, this build only handles v{}",
                    v.0,
                    WorldSchemaVersion::CURRENT.0,
                )));
            }
            None => {
                // Fresh world — stamp the current version.
                meta.insert(
                    Self::META_SCHEMA,
                    &WorldSchemaVersion::CURRENT.0.to_le_bytes(),
                )?;
            }
            Some(_) => { /* equal or older with valid migration plan — caller handled migration before opening */
            }
        }

        Ok(Self {
            efj,
            s_entities,
            s_tree,
            s_datastore,
            s_datastore_ord,
            s_entities_uuid,
            s_path_to_uuid,
            s_uuid_to_path,
            s_class_index,
            s_voxels,
            s_datasets,
            s_timeseries,
            keyspace,
            entities,
            meta,
            tree,
            datastore,
            datastore_ord,
            entities_uuid,
            path_to_uuid,
            uuid_to_path,
            class_index,
            voxels,
            datasets,
            timeseries,
            encoder,
            tx_counter: AtomicU64::new(tx_counter),
            commit_lock: Mutex::new(()),
            change_stream: ChangeStream::new(),
        })
    }

    /// Read the on-disk schema version, mostly for engine diagnostics
    /// and the Studio "About this world" panel.
    pub fn on_disk_schema(&self) -> Result<WorldSchemaVersion> {
        let bytes = self
            .meta
            .get(Self::META_SCHEMA)?
            .ok_or_else(|| crate::error::Error::Other("meta:schema_version missing".to_string()))?;
        if bytes.len() != 2 {
            return Err(crate::error::Error::Other(format!(
                "meta:schema_version has wrong length: {}",
                bytes.len()
            )));
        }
        let mut arr = [0u8; 2];
        arr.copy_from_slice(&bytes);
        Ok(WorldSchemaVersion(u16::from_le_bytes(arr)))
    }

    /// Convenience: write+flush the header.bin alongside the Fjall
    /// directory. The engine plugin uses this on world create.
    pub fn write_header(world_root: &Path, header: &WorldHeader) -> Result<()> {
        header.write(world_root)
    }

    /// Persist the tx counter so a crash doesn't replay the same id.
    fn checkpoint_tx_counter(&self) -> Result<()> {
        let val = self.tx_counter.load(Ordering::Acquire);
        self.meta.insert(Self::META_TX_COUNTER, &val.to_le_bytes())?;
        Ok(())
    }
}

impl WorldDb for FjallWorldDb {
    fn apply_commit(&self, commit: Commit) -> Result<TxId> {
        if commit.is_empty() {
            return Ok(TxId(self.tx_counter.load(Ordering::Acquire)));
        }

        let _guard = self.commit_lock.lock();
        let _span = tracing::info_span!(
            "worlddb.commit",
            ops = commit.len(),
        )
        .entered();

        // Stake out the next tx id BEFORE doing any IO so the delta
        // we publish carries the same id we hand back to the caller.
        let next = self
            .tx_counter
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let tx_id = TxId(next);

        let mut batch = self.keyspace.batch();
        let mut changes: Vec<EntityChange> = Vec::with_capacity(commit.len());
        let mut byte_size: usize = 0;

        for op in &commit.ops {
            match op {
                CommitOp::Put {
                    entity,
                    component,
                    value,
                } => {
                    let key = self.encoder.encode_component(*entity, *component);
                    batch.insert(&self.entities, key, value.as_slice());
                    let preview = value[..value.len().min(64)].to_vec();
                    byte_size += value.len();
                    changes.push(EntityChange::Put {
                        entity: *entity,
                        component: *component,
                        value_preview: preview,
                    });
                }
                CommitOp::Delete { entity, component } => {
                    let key = self.encoder.encode_component(*entity, *component);
                    batch.remove(&self.entities, key);
                    changes.push(EntityChange::Removed {
                        entity: *entity,
                        component: *component,
                    });
                }
                CommitOp::DespawnEntity { entity } => {
                    // Flat encoder doesn't natively support
                    // entity-prefix range deletes. Fall back to
                    // probing each well-known component slot. Phase 2
                    // (Morton encoder with entity-first layout) makes
                    // this a single range_remove.
                    for c in [
                        ComponentTypeId::TRANSFORM,
                        ComponentTypeId::BASE_PART,
                        ComponentTypeId::TAGS,
                        ComponentTypeId::ATTRIBUTES,
                        ComponentTypeId::INSTANCE_META,
                        ComponentTypeId::ASSET_REF,
                        ComponentTypeId::MEASURE_UNIT,
                    ] {
                        let key = self.encoder.encode_component(*entity, c);
                        batch.remove(&self.entities, key);
                    }
                    changes.push(EntityChange::Despawned { entity: *entity });
                }
            }
        }

        // Persist the new tx counter inside the same batch so a crash
        // can't desync the counter from the data.
        batch.insert(&self.meta, Self::META_TX_COUNTER, next.to_le_bytes());
        batch.commit()?;

        // Atomicity contract: emit ONLY after the durable commit. Two
        // sinks now: (1) the typed engine-facing CommitDelta on the
        // worlddb change-stream, and (2) the per-store byte-level
        // ReplicationEvent feed on the owned eustress-fjall fork — the
        // multiplayer/distributed replication substrate. Every entity
        // component change becomes a sequenced replication delta.
        for ch in &changes {
            match ch {
                EntityChange::Put {
                    entity,
                    component,
                    value_preview,
                } => {
                    let k = self.encoder.encode_component(*entity, *component);
                    self.s_entities
                        .publish_external(eustress_fjall::ReplOp::Put, &k, value_preview);
                }
                EntityChange::Removed { entity, component } => {
                    let k = self.encoder.encode_component(*entity, *component);
                    self.s_entities
                        .publish_external(eustress_fjall::ReplOp::Remove, &k, &[]);
                }
                EntityChange::Despawned { entity } => {
                    // One Remove per well-known component slot (mirrors
                    // the despawn batch above) so a replica drops the
                    // whole entity.
                    for c in [
                        ComponentTypeId::TRANSFORM,
                        ComponentTypeId::BASE_PART,
                        ComponentTypeId::TAGS,
                        ComponentTypeId::ATTRIBUTES,
                        ComponentTypeId::INSTANCE_META,
                        ComponentTypeId::ASSET_REF,
                        ComponentTypeId::MEASURE_UNIT,
                    ] {
                        let k = self.encoder.encode_component(*entity, c);
                        self.s_entities
                            .publish_external(eustress_fjall::ReplOp::Remove, &k, &[]);
                    }
                }
            }
        }

        let delta = CommitDelta {
            tx_id,
            changes,
            byte_size,
        };
        self.change_stream.publish(delta);

        Ok(tx_id)
    }

    fn get_component(
        &self,
        entity: EntityId,
        component: ComponentTypeId,
    ) -> Result<Option<Vec<u8>>> {
        let key = self.encoder.encode_component(entity, component);
        let _span = tracing::trace_span!("worlddb.get", entity = entity.0, component = component.0).entered();
        Ok(self.entities.get(key)?.map(|s| s.to_vec()))
    }

    fn iter_component(
        &self,
        component: ComponentTypeId,
    ) -> Result<Box<dyn Iterator<Item = Result<(EntityId, Vec<u8>)>> + '_>> {
        let prefix = self.encoder.component_prefix(component);
        let encoder_handle: &dyn KeyEncoder = self.encoder.as_ref();
        let iter = self.entities.prefix(prefix).map(move |res| -> Result<_> {
            let (key, value) = res?;
            let (entity, _c) = encoder_handle.decode_component(&key)?;
            Ok((entity, value.to_vec()))
        });
        Ok(Box::new(iter))
    }

    fn put_instance_core(&self, entity: EntityId, pos: (f32, f32, f32), core: &[u8]) -> Result<()> {
        // Morton-keyed so a region scan returns a spatial neighbourhood.
        // Inline default encoder (chunk_size 256) — must stay consistent
        // with the read side below and with the streaming chunk size.
        let key = crate::keys::MortonKeyEncoder::default().encode_spatial(
            entity,
            ComponentTypeId::INSTANCE_CORE,
            pos,
        );
        let mut batch = self.keyspace.batch();
        batch.insert(&self.entities, key.clone(), core);
        batch.commit()?;
        // Replication feed (mirrors apply_commit): emit AFTER the durable
        // commit so replicas only ever see persisted state.
        self.s_entities.publish_external(
            eustress_fjall::ReplOp::Put,
            &key,
            &core[..core.len().min(64)],
        );
        Ok(())
    }

    fn delete_instance_core(&self, entity: EntityId, pos: (f32, f32, f32)) -> Result<()> {
        let key = crate::keys::MortonKeyEncoder::default().encode_spatial(
            entity,
            ComponentTypeId::INSTANCE_CORE,
            pos,
        );
        let mut batch = self.keyspace.batch();
        batch.remove(&self.entities, key.clone());
        batch.commit()?;
        self.s_entities
            .publish_external(eustress_fjall::ReplOp::Remove, &key, &[]);
        Ok(())
    }

    fn iter_instance_cores(&self) -> Result<Vec<(EntityId, Vec<u8>)>> {
        // Morton keys are `M | ver | morton63 | component | entity`; the
        // component prefix for the spatial encoder is just `M | ver`, so
        // this scan walks every Morton-keyed record and we keep only the
        // INSTANCE_CORE ones (the only Morton-keyed component today).
        let morton = crate::keys::MortonKeyEncoder::default();
        let prefix = morton.component_prefix(ComponentTypeId::INSTANCE_CORE);
        let mut out = Vec::new();
        for res in self.entities.prefix(prefix) {
            let (key, value) = res?;
            if let Ok((entity, component)) = morton.decode_component(&key) {
                if component == ComponentTypeId::INSTANCE_CORE {
                    out.push((entity, value.to_vec()));
                }
            }
        }
        Ok(out)
    }

    fn iter_instance_cores_in_region(
        &self,
        cx: (u32, u32),
        cy: (u32, u32),
        cz: (u32, u32),
    ) -> Result<Vec<(EntityId, Vec<u8>)>> {
        // Guard: a bad camera value must never trigger a multi-million-cell
        // sweep. 4096 cells ≈ a 16×16×16 box at 256-unit cells (≈4 km span).
        let nx = u64::from(cx.1.saturating_sub(cx.0)) + 1;
        let ny = u64::from(cy.1.saturating_sub(cy.0)) + 1;
        let nz = u64::from(cz.1.saturating_sub(cz.0)) + 1;
        let cell_count = nx.saturating_mul(ny).saturating_mul(nz);
        if cell_count > 4096 {
            return Err(crate::error::Error::Other(format!(
                "iter_instance_cores_in_region: cell box too large ({cell_count} > 4096)"
            )));
        }
        // Per-cell prefix scans, unioned. (A single Morton range over a 3D
        // box would pull a huge out-of-box superset — Z-order stitches
        // between rows — so we scan each cell's exact prefix instead.)
        let morton = crate::keys::MortonKeyEncoder::default();
        let mut out = Vec::new();
        for x in cx.0..=cx.1 {
            for y in cy.0..=cy.1 {
                for z in cz.0..=cz.1 {
                    let prefix = morton.cell_prefix(x, y, z);
                    for res in self.entities.prefix(prefix) {
                        let (key, value) = res?;
                        if let Ok((entity, component)) = morton.decode_component(&key) {
                            if component == ComponentTypeId::INSTANCE_CORE {
                                out.push((entity, value.to_vec()));
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn count_instance_cores_capped(&self, cap: usize) -> Result<usize> {
        let morton = crate::keys::MortonKeyEncoder::default();
        let prefix = morton.component_prefix(ComponentTypeId::INSTANCE_CORE);
        let mut n = 0usize;
        for res in self.entities.prefix(prefix) {
            let (key, _value) = res?;
            if let Ok((_e, component)) = morton.decode_component(&key) {
                if component == ComponentTypeId::INSTANCE_CORE {
                    n += 1;
                    if n >= cap {
                        break;
                    }
                }
            }
        }
        Ok(n)
    }

    // ── Wave 9.A — voxel-chunk terrain partition ──────────────────────

    fn put_voxel_chunk(&self, cx: i32, cy: i32, cz: i32, bytes: &[u8]) -> Result<()> {
        let key = crate::keys::encode_voxel_chunk_key(cx, cy, cz);
        self.voxels.insert(key, bytes)?;
        // DataModel/terrain mutation → replication feed (mirrors put_file /
        // put_instance_core): emit AFTER the durable insert so replicas only
        // ever see persisted state. Preview-cap the value like other paths.
        self.s_voxels.publish_external(
            eustress_fjall::ReplOp::Put,
            &key,
            &bytes[..bytes.len().min(64)],
        );
        Ok(())
    }

    fn get_voxel_chunk(&self, cx: i32, cy: i32, cz: i32) -> Result<Option<Vec<u8>>> {
        let key = crate::keys::encode_voxel_chunk_key(cx, cy, cz);
        Ok(self.voxels.get(key)?.map(|s| s.to_vec()))
    }

    fn iter_voxel_chunks_in_region(
        &self,
        min: (f32, f32, f32),
        max: (f32, f32, f32),
    ) -> Result<Vec<((i32, i32, i32), Vec<u8>)>> {
        let edge = crate::keys::VOXEL_CHUNK_EDGE_STUDS;
        // World box → inclusive chunk-coord box. Floor both corners (the
        // chunk that CONTAINS each corner); take per-axis min/max so an
        // inverted box (max < min) still yields a well-formed range rather
        // than an empty/negative one.
        let ax = crate::keys::world_to_chunk_coord(min.0, edge);
        let bx = crate::keys::world_to_chunk_coord(max.0, edge);
        let ay = crate::keys::world_to_chunk_coord(min.1, edge);
        let by = crate::keys::world_to_chunk_coord(max.1, edge);
        let az = crate::keys::world_to_chunk_coord(min.2, edge);
        let bz = crate::keys::world_to_chunk_coord(max.2, edge);
        let (min_cx, max_cx) = (ax.min(bx), ax.max(bx));
        let (min_cy, max_cy) = (ay.min(by), ay.max(by));
        let (min_cz, max_cz) = (az.min(bz), az.max(bz));

        // Morton bit-interleave is per-axis monotone, so every in-box key
        // satisfies encode(min_corner) <= key <= encode(max_corner). The
        // inclusive range [lo..=hi] is therefore a SUPERSET of the box
        // (Z-order stitches through coords outside it); the per-key filter
        // below is MANDATORY (litmax/bigmin property).
        let lo = crate::keys::encode_voxel_chunk_key(min_cx, min_cy, min_cz);
        let hi = crate::keys::encode_voxel_chunk_key(max_cx, max_cy, max_cz);
        let mut out = Vec::new();
        for res in self.voxels.range(lo..=hi) {
            let (key, value) = res?;
            let (cx, cy, cz) = crate::keys::decode_voxel_chunk_key(&key)?;
            if (min_cx..=max_cx).contains(&cx)
                && (min_cy..=max_cy).contains(&cy)
                && (min_cz..=max_cz).contains(&cz)
            {
                out.push(((cx, cy, cz), value.to_vec()));
            }
        }
        Ok(out)
    }

    fn iter_all_voxel_chunks(&self) -> Result<Vec<((i32, i32, i32), Vec<u8>)>> {
        // Every voxel key starts `V | ver`; that 2-byte prefix bounds the
        // scan to this partition's voxel rows.
        let prefix = crate::keys::voxel_key_prefix();
        let mut out = Vec::new();
        for res in self.voxels.prefix(prefix) {
            let (key, value) = res?;
            let (cx, cy, cz) = crate::keys::decode_voxel_chunk_key(&key)?;
            out.push(((cx, cy, cz), value.to_vec()));
        }
        Ok(out)
    }

    fn has_voxel_chunks(&self) -> bool {
        // One bounded prefix-scan step — O(1) emptiness probe. An Err row
        // still proves a row EXISTS under the voxel prefix, so any
        // `Some(_)` counts as non-empty (never re-seed over real data
        // because one row failed to read).
        self.voxels
            .prefix(crate::keys::voxel_key_prefix())
            .next()
            .is_some()
    }

    // ── Data Platform — dataset-blob partition ────────────────────────

    fn put_dataset_chunk(&self, id: &[u8; 16], bytes: &[u8]) -> Result<()> {
        let key = crate::keys::encode_dataset_key(id);
        self.datasets.insert(key, bytes)?;
        // Mirror put_voxel_chunk: emit AFTER the durable insert so replicas
        // only ever see persisted state. Preview-cap the value.
        self.s_datasets.publish_external(
            eustress_fjall::ReplOp::Put,
            &key,
            &bytes[..bytes.len().min(64)],
        );
        Ok(())
    }

    fn get_dataset_chunk(&self, id: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        let key = crate::keys::encode_dataset_key(id);
        Ok(self.datasets.get(key)?.map(|s| s.to_vec()))
    }

    fn iter_dataset_chunks(&self) -> Result<Vec<([u8; 16], Vec<u8>)>> {
        // Every dataset key starts `D | ver`; that 2-byte prefix bounds the
        // scan to this partition's dataset rows.
        let prefix = crate::keys::dataset_key_prefix();
        let mut out = Vec::new();
        for res in self.datasets.prefix(prefix) {
            let (key, value) = res?;
            let id = crate::keys::decode_dataset_key(&key)?;
            out.push((id, value.to_vec()));
        }
        Ok(out)
    }

    // ── Data Platform — timeseries partition ──────────────────────────

    fn ts_append(&self, series: &str, ts: u64, seq: u32, row: &[u8]) -> Result<()> {
        let key = crate::keys::encode_timeseries_key(series, ts, seq);
        self.timeseries.insert(&key, row)?;
        self.s_timeseries.publish_external(
            eustress_fjall::ReplOp::Put,
            &key,
            &row[..row.len().min(64)],
        );
        Ok(())
    }

    fn ts_range(
        &self,
        series: &str,
        min_ts: u64,
        max_ts: u64,
    ) -> Result<Vec<(u64, u32, Vec<u8>)>> {
        // Inclusive bounds: low at seq 0, high at seq u32::MAX so the whole
        // same-timestamp run is captured at each end. The series segment is
        // a fixed prefix and ts is big-endian (order-preserving), so the
        // range is EXACTLY the in-window rows — no post-filter needed
        // (unlike the Morton voxel range, which returns a superset).
        let lo = crate::keys::timeseries_range_bound(series, min_ts, 0);
        let hi = crate::keys::timeseries_range_bound(series, max_ts, u32::MAX);
        let mut out = Vec::new();
        for res in self.timeseries.range(lo..=hi) {
            let (key, value) = res?;
            let (_series, ts, seq) = crate::keys::decode_timeseries_key(&key)?;
            out.push((ts, seq, value.to_vec()));
        }
        Ok(out)
    }

    // ── IDENTITY.md Wave 2.1 ─────────────────────────────────────────

    fn put_entity_core_by_uuid(&self, uuid: &[u8; 16], core_bytes: &[u8]) -> Result<()> {
        let key = encode_uuid_key(uuid);
        self.entities_uuid.insert(&key, core_bytes)?;
        // Replication feed — mirrors the existing apply_commit/put_instance_core
        // pattern: emit on the entities-uuid store handle so a replica sees
        // the new UUID-primary row.
        self.s_entities_uuid.publish_external(
            eustress_fjall::ReplOp::Put,
            &key,
            &core_bytes[..core_bytes.len().min(64)],
        );
        Ok(())
    }

    fn get_entity_core_by_uuid(&self, uuid: &[u8; 16]) -> Result<Option<Vec<u8>>> {
        let key = encode_uuid_key(uuid);
        Ok(self.entities_uuid.get(&key)?.map(|s| s.to_vec()))
    }

    fn delete_entity_by_uuid(&self, uuid: &[u8; 16]) -> Result<()> {
        let key = encode_uuid_key(uuid);
        self.entities_uuid.remove(&key)?;
        self.s_entities_uuid
            .publish_external(eustress_fjall::ReplOp::Remove, &key, &[]);
        Ok(())
    }

    fn path_to_uuid(&self, rel_path: &str) -> Result<Option<[u8; 16]>> {
        let key = normalise_rel(rel_path);
        match self.path_to_uuid.get(key.as_bytes())? {
            Some(bytes) if bytes.len() == 16 => {
                let mut out = [0u8; 16];
                out.copy_from_slice(&bytes);
                Ok(Some(out))
            }
            Some(other) => Err(crate::error::Error::Other(format!(
                "path_to_uuid: malformed value (len {} != 16) for {key:?}",
                other.len()
            ))),
            None => Ok(None),
        }
    }

    fn put_path_to_uuid(&self, rel_path: &str, uuid: &[u8; 16]) -> Result<()> {
        let key = normalise_rel(rel_path);
        self.path_to_uuid.insert(key.as_bytes(), uuid.as_slice())?;
        self.s_path_to_uuid.publish_external(
            eustress_fjall::ReplOp::Put,
            key.as_bytes(),
            uuid.as_slice(),
        );
        Ok(())
    }

    fn delete_path_to_uuid(&self, rel_path: &str) -> Result<()> {
        let key = normalise_rel(rel_path);
        self.path_to_uuid.remove(key.as_bytes())?;
        self.s_path_to_uuid
            .publish_external(eustress_fjall::ReplOp::Remove, key.as_bytes(), &[]);
        Ok(())
    }

    fn uuid_to_path(&self, uuid: &[u8; 16]) -> Result<Option<String>> {
        let key = encode_uuid_key(uuid);
        match self.uuid_to_path.get(&key)? {
            Some(bytes) => Ok(Some(
                std::str::from_utf8(&bytes)
                    .map_err(|e| crate::error::Error::KeyDecode(
                        format!("uuid_to_path: value not utf-8: {e}"),
                    ))?
                    .to_string(),
            )),
            None => Ok(None),
        }
    }

    fn put_uuid_to_path(&self, uuid: &[u8; 16], rel_path: &str) -> Result<()> {
        let key = encode_uuid_key(uuid);
        let val = normalise_rel(rel_path);
        self.uuid_to_path.insert(&key, val.as_bytes())?;
        self.s_uuid_to_path
            .publish_external(eustress_fjall::ReplOp::Put, &key, val.as_bytes());
        Ok(())
    }

    fn delete_uuid_to_path(&self, uuid: &[u8; 16]) -> Result<()> {
        let key = encode_uuid_key(uuid);
        self.uuid_to_path.remove(&key)?;
        self.s_uuid_to_path
            .publish_external(eustress_fjall::ReplOp::Remove, &key, &[]);
        Ok(())
    }

    fn put_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        let key = encode_class_index_key(class_name, uuid);
        // Empty marker — the prefix-scan in iter_class only needs the key.
        self.class_index.insert(&key, &[])?;
        self.s_class_index
            .publish_external(eustress_fjall::ReplOp::Put, &key, &[]);
        Ok(())
    }

    fn delete_class_index(&self, class_name: &str, uuid: &[u8; 16]) -> Result<()> {
        let key = encode_class_index_key(class_name, uuid);
        self.class_index.remove(&key)?;
        self.s_class_index
            .publish_external(eustress_fjall::ReplOp::Remove, &key, &[]);
        Ok(())
    }

    fn iter_class(&self, class_name: &str) -> Result<Vec<[u8; 16]>> {
        let mut prefix = Vec::with_capacity(class_name.len() + 1);
        prefix.extend_from_slice(class_name.as_bytes());
        prefix.push(0x1f);
        let mut out = Vec::new();
        for kv in self.class_index.prefix(prefix.as_slice()) {
            let (k, _v) = kv?;
            // Key layout: `<class>\x1f<uuid_16>` — take the trailing 16 bytes.
            if k.len() < prefix.len() + 16 {
                continue;
            }
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&k[k.len() - 16..]);
            out.push(uuid);
        }
        Ok(out)
    }

    fn iter_class_capped(&self, class_name: &str, cap: usize) -> Result<Vec<[u8; 16]>> {
        if cap == 0 {
            return Ok(Vec::new());
        }
        let mut prefix = Vec::with_capacity(class_name.len() + 1);
        prefix.extend_from_slice(class_name.as_bytes());
        prefix.push(0x1f);
        let mut out = Vec::with_capacity(cap.min(512));
        for kv in self.class_index.prefix(prefix.as_slice()) {
            let (k, _v) = kv?;
            if k.len() < prefix.len() + 16 {
                continue;
            }
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&k[k.len() - 16..]);
            out.push(uuid);
            if out.len() >= cap {
                break; // early exit — never materializes the whole class
            }
        }
        Ok(out)
    }

    fn iter_all_classes(&self) -> Result<Vec<(String, usize)>> {
        // One full scan of `class_index`. Each key is `<class>\x1f<uuid_16>`
        // with an empty value, so we only read keys. Accumulate per-class
        // counts in a BTreeMap (sorted output for stable UI ordering).
        use std::collections::BTreeMap;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for kv in self.class_index.iter() {
            let (k, _v) = kv?;
            // Split on the first 0x1f unit separator; the prefix is the class.
            let Some(sep) = k.iter().position(|&b| b == 0x1f) else {
                continue;
            };
            let Ok(class) = std::str::from_utf8(&k[..sep]) else {
                continue;
            };
            *counts.entry(class.to_string()).or_insert(0) += 1;
        }
        Ok(counts.into_iter().collect())
    }

    fn get_meta(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.meta.get(key)?.map(|s| s.to_vec()))
    }

    fn put_meta(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.meta.insert(key, value)?;
        Ok(())
    }

    fn flush(&self) -> Result<()> {
        let _span = tracing::info_span!("worlddb.flush").entered();
        // Persist tx counter into the same flush so it doesn't lag
        // the SSTable visibility.
        self.checkpoint_tx_counter()?;
        self.keyspace.persist(fjall::PersistMode::SyncAll)?;
        Ok(())
    }

    fn subscribe(&self, filter: Filter) -> Subscription {
        self.change_stream.subscribe(filter)
    }

    fn change_stream(&self) -> &ChangeStream {
        &self.change_stream
    }

    fn put_file(&self, rel_path: &str, bytes: &[u8]) -> Result<()> {
        let key = normalise_rel(rel_path);
        self.tree.insert(key.as_bytes(), bytes)?;
        // DataModel/scene-tree mutation → replication feed.
        self.s_tree
            .publish_external(eustress_fjall::ReplOp::Put, key.as_bytes(), bytes);
        Ok(())
    }

    fn get_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        let key = normalise_rel(rel_path);
        Ok(self.tree.get(key.as_bytes())?.map(|s| s.to_vec()))
    }

    fn delete_file(&self, rel_path: &str) -> Result<()> {
        let key = normalise_rel(rel_path);
        self.tree.remove(key.as_bytes())?;
        self.s_tree
            .publish_external(eustress_fjall::ReplOp::Remove, key.as_bytes(), &[]);
        Ok(())
    }

    fn clear_tree_prefix(&self, rel_prefix: &str) -> Result<usize> {
        let p = normalise_rel(rel_prefix);
        if p.is_empty() {
            return Ok(0); // refuse to nuke the whole tree via "" — use a real reset path
        }
        let with_slash = format!("{p}/");
        // Prefix-scan the narrower `p/` namespace; also catch an exact
        // `p` key (a file stored at the prefix itself). This is a path
        // boundary match so `Foo` never deletes `Foobar`.
        let mut victims: Vec<Vec<u8>> = Vec::new();
        for kv in self.tree.prefix(with_slash.as_bytes()) {
            let (k, _v) = kv?;
            victims.push(k.to_vec());
        }
        if let Some(exact) = self.tree.get(p.as_bytes())? {
            let _ = exact;
            victims.push(p.clone().into_bytes());
        }
        let removed = victims.len();
        for k in victims {
            self.tree.remove(&k)?;
            self.s_tree
                .publish_external(eustress_fjall::ReplOp::Remove, &k, &[]);
        }
        Ok(removed)
    }

    fn list_dir(&self, rel_dir: &str) -> Result<Vec<TreeEntry>> {
        let dir = normalise_rel(rel_dir);
        // Prefix scan: everything under `dir/` (or all keys when dir
        // is root). Derive immediate children — a stored key
        // `a/b/c.toml` under prefix `a` yields the dir `b`; under
        // prefix `a/b` yields the file `c.toml`. Subdirectories are
        // inferred (no explicit dir markers stored), deduped here.
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut seen: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
        for kv in self.tree.prefix(prefix.as_bytes()) {
            let (k, _v) = kv?;
            let key = std::str::from_utf8(&k)
                .map_err(|e| crate::error::Error::KeyDecode(format!("tree key not utf-8: {e}")))?;
            let rest = &key[prefix.len()..];
            if rest.is_empty() {
                continue;
            }
            match rest.find('/') {
                // `child` is a directory (more path follows).
                Some(slash) => {
                    let name = rest[..slash].to_string();
                    seen.entry(name).or_insert(true);
                }
                // `child` is a file leaf.
                None => {
                    seen.insert(rest.to_string(), false);
                }
            }
        }
        Ok(seen
            .into_iter()
            .map(|(name, is_dir)| {
                let rel_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}{name}")
                };
                TreeEntry {
                    name,
                    rel_path,
                    is_dir,
                }
            })
            .collect())
    }

    fn tree_is_empty(&self) -> Result<bool> {
        // `iter().next()` on the partition is O(1)-ish (seeks first
        // block); cheaper than a full count for the "should I seed?"
        // decision.
        Ok(self.tree.iter().next().is_none())
    }

    fn iter_tree(&self) -> Result<Box<dyn Iterator<Item = Result<(String, Vec<u8>)>> + '_>> {
        let iter = self.tree.iter().map(|res| -> Result<_> {
            let (k, v) = res?;
            let path = std::str::from_utf8(&k)
                .map_err(|e| crate::error::Error::KeyDecode(format!("tree key not utf-8: {e}")))?
                .to_string();
            Ok((path, v.to_vec()))
        });
        Ok(Box::new(iter))
    }

    fn ds_get(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .datastore
            .get(ds_key(store, scope, key))?
            .map(|s| s.to_vec()))
    }

    fn ds_set(&self, store: &str, scope: &str, key: &str, value: &[u8]) -> Result<()> {
        let k = ds_key(store, scope, key);
        self.datastore.insert(&k, value)?;
        // DataStore game-state mutation → replication feed (for
        // server-authoritative / distributed DataStore consistency).
        self.s_datastore
            .publish_external(eustress_fjall::ReplOp::Put, &k, value);
        Ok(())
    }

    fn ds_remove(&self, store: &str, scope: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let k = ds_key(store, scope, key);
        let prior = self.datastore.get(&k)?.map(|s| s.to_vec());
        self.datastore.remove(&k)?;
        self.s_datastore
            .publish_external(eustress_fjall::ReplOp::Remove, &k, &[]);
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
        // Serialise CAS under the commit lock — DataStore writes are
        // low-frequency game-state, not the scene hot path, so a
        // process-wide mutex is correct and simplest. `max_retries`
        // is honoured for API parity (Roblox `UpdateAsync` retries on
        // contention); under the lock the first attempt always wins,
        // so retries only matter if the transform itself signals abort.
        let _guard = self.commit_lock.lock();
        let k = ds_key(store, scope, key);
        let mut attempt = 0;
        loop {
            let current = self.datastore.get(&k)?.map(|s| s.to_vec());
            match transform(current) {
                Some(new_val) => {
                    self.datastore.insert(&k, new_val.as_slice())?;
                    self.s_datastore.publish_external(
                        eustress_fjall::ReplOp::Put,
                        &k,
                        new_val.as_slice(),
                    );
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
        let prefix = ds_scope_prefix(store, scope);
        let plen = prefix.len();
        // Collect matching ordered entries, then take `limit` from the
        // requested end. Range scans here are leaderboard-page sized
        // (≤ a few hundred), so the collect is bounded.
        let mut rows: Vec<(String, Vec<u8>, i64)> = Vec::new();
        for kv in self.datastore_ord.prefix(prefix.as_slice()) {
            let (k, v) = kv?;
            if k.len() < plen + 8 + 1 {
                continue;
            }
            let sort = be8_to_sort(&k[plen..plen + 8]);
            if let Some(lo) = min {
                if sort < lo {
                    continue;
                }
            }
            if let Some(hi) = max {
                if sort > hi {
                    continue;
                }
            }
            let name = std::str::from_utf8(&k[plen + 9..])
                .map_err(|e| crate::error::Error::KeyDecode(format!("ds key not utf-8: {e}")))?
                .to_string();
            if !cursor.is_empty() && name == cursor {
                continue;
            }
            rows.push((name, v.to_vec(), sort));
        }
        rows.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
        if !ascending {
            rows.reverse();
        }
        rows.truncate(limit);
        Ok(rows)
    }

    fn ds_set_sorted(
        &self,
        store: &str,
        scope: &str,
        key: &str,
        value: &[u8],
        sort: i64,
    ) -> Result<()> {
        let _guard = self.commit_lock.lock();
        // Plain entry stores `[sort_be8][value]` so a re-rank can find
        // and delete the stale ordered key without a side index.
        let plain_k = ds_key(store, scope, key);
        if let Some(prev) = self.datastore.get(&plain_k)? {
            if prev.len() >= 8 {
                let old_sort = be8_to_sort(&prev[..8]);
                self.datastore_ord
                    .remove(ds_ord_key(store, scope, old_sort, key))?;
            }
        }
        let mut tagged = Vec::with_capacity(8 + value.len());
        tagged.extend_from_slice(&sort_to_be8(sort));
        tagged.extend_from_slice(value);
        self.datastore.insert(&plain_k, tagged.as_slice())?;
        let ord_k = ds_ord_key(store, scope, sort, key);
        self.datastore_ord.insert(&ord_k, value)?;
        // Ordered DataStore write → replication on both the plain and
        // the ranking store so a replica reconstructs leaderboards.
        self.s_datastore
            .publish_external(eustress_fjall::ReplOp::Put, &plain_k, value);
        self.s_datastore_ord
            .publish_external(eustress_fjall::ReplOp::Put, &ord_k, value);
        Ok(())
    }
}

impl Drop for FjallWorldDb {
    fn drop(&mut self) {
        // Best-effort checkpoint on shutdown. Errors are logged, not
        // surfaced — drop can't return Result.
        if let Err(e) = self.checkpoint_tx_counter() {
            tracing::warn!(
                target: "eustress_worlddb",
                error = %e,
                "checkpoint_tx_counter on drop failed",
            );
        }
        if let Err(e) = self.keyspace.persist(fjall::PersistMode::SyncAll) {
            tracing::warn!(
                target: "eustress_worlddb",
                error = %e,
                "final persist on drop failed",
            );
        }
    }
}

#[cfg(test)]
mod voxel_tests {
    use super::*;
    use crate::keys::{encode_voxel_chunk_key, VOXEL_CHUNK_EDGE_STUDS};

    /// Fresh on-disk Fjall world in a unique temp dir (mirrors the temp-dir
    /// pattern the migration tests use). Returned `PathBuf` is cleaned up by
    /// the caller.
    fn fresh_db() -> (FjallWorldDb, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "eustress_voxel_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = FjallWorldDb::open(&tmp).unwrap();
        (db, tmp)
    }

    /// Centre world position (studs) of chunk `(cx, cy, cz)` — used to build
    /// a region box that lands squarely on the intended chunks.
    fn chunk_centre(cx: i32, cy: i32, cz: i32) -> (f32, f32, f32) {
        let e = VOXEL_CHUNK_EDGE_STUDS;
        (
            (cx as f32 + 0.5) * e,
            (cy as f32 + 0.5) * e,
            (cz as f32 + 0.5) * e,
        )
    }

    #[test]
    fn put_get_roundtrip_negatives_overwrite_and_large_value() {
        let (db, tmp) = fresh_db();

        // Basic positive round-trip.
        db.put_voxel_chunk(1, 2, 3, b"hello-voxels").unwrap();
        assert_eq!(
            db.get_voxel_chunk(1, 2, 3).unwrap().as_deref(),
            Some(&b"hello-voxels"[..])
        );

        // Negative coords round-trip (the biased-Morton path).
        db.put_voxel_chunk(-5, 9, -2, b"neg").unwrap();
        assert_eq!(
            db.get_voxel_chunk(-5, 9, -2).unwrap().as_deref(),
            Some(&b"neg"[..])
        );

        // Missing chunk → None.
        assert_eq!(db.get_voxel_chunk(100, 100, 100).unwrap(), None);

        // Overwrite replaces, doesn't append.
        db.put_voxel_chunk(1, 2, 3, b"replaced").unwrap();
        assert_eq!(
            db.get_voxel_chunk(1, 2, 3).unwrap().as_deref(),
            Some(&b"replaced"[..])
        );

        // ~10 KB value (realistic LZ4 chunk payload size).
        let big = vec![0xABu8; 10 * 1024];
        db.put_voxel_chunk(7, -7, 7, &big).unwrap();
        assert_eq!(db.get_voxel_chunk(7, -7, 7).unwrap(), Some(big));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn iter_all_returns_exactly_what_was_put() {
        let (db, tmp) = fresh_db();
        let put: Vec<(i32, i32, i32)> =
            vec![(0, 0, 0), (1, 0, 0), (-3, 4, -5), (12, 12, 12), (0, 0, -1)];
        for &(cx, cy, cz) in &put {
            db.put_voxel_chunk(cx, cy, cz, format!("{cx},{cy},{cz}").as_bytes())
                .unwrap();
        }
        let mut got: Vec<(i32, i32, i32)> = db
            .iter_all_voxel_chunks()
            .unwrap()
            .into_iter()
            .map(|(coord, bytes)| {
                // value integrity: bytes match the coord they were stored under
                assert_eq!(bytes, format!("{},{},{}", coord.0, coord.1, coord.2).as_bytes());
                coord
            })
            .collect();
        got.sort();
        let mut want = put.clone();
        want.sort();
        assert_eq!(got, want);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn iter_in_region_returns_only_in_box_chunks() {
        let (db, tmp) = fresh_db();
        // In-box chunks: the 2×2×2 block [0..=1]³.
        let inside: Vec<(i32, i32, i32)> = vec![
            (0, 0, 0),
            (1, 0, 0),
            (0, 1, 0),
            (0, 0, 1),
            (1, 1, 1),
        ];
        // Out-of-box chunks scattered around (incl. negatives + far away).
        let outside: Vec<(i32, i32, i32)> = vec![
            (-1, 0, 0),
            (2, 0, 0),
            (0, -1, 0),
            (0, 0, 2),
            (50, 50, 50),
            (-9, -9, -9),
        ];
        for &(cx, cy, cz) in inside.iter().chain(outside.iter()) {
            db.put_voxel_chunk(cx, cy, cz, b"x").unwrap();
        }

        // Region box from the centre of chunk (0,0,0) to the centre of
        // chunk (1,1,1) → chunk-coord box [0..=1]³.
        let lo = chunk_centre(0, 0, 0);
        let hi = chunk_centre(1, 1, 1);
        let mut got: Vec<(i32, i32, i32)> = db
            .iter_voxel_chunks_in_region(lo, hi)
            .unwrap()
            .into_iter()
            .map(|(c, _)| c)
            .collect();
        got.sort();
        let mut want = inside.clone();
        want.sort();
        assert_eq!(got, want, "region must return exactly the in-box chunks");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// litmax/bigmin guard: a thin box whose Morton key range provably
    /// CONTAINS an out-of-box chunk. We first prove such a chunk exists
    /// (the range is a strict superset of the box), then assert the region
    /// query's filter excludes it.
    #[test]
    fn region_filter_excludes_morton_range_intruder() {
        // A deliberately thin/asymmetric chunk-coord box. With Z-order,
        // the linear key range between the box corners sweeps through
        // coords that are spatially outside the box.
        let (min_cx, min_cy, min_cz) = (0i32, 0i32, 0i32);
        let (max_cx, max_cy, max_cz) = (1i32, 0i32, 3i32);
        let lo = encode_voxel_chunk_key(min_cx, min_cy, min_cz);
        let hi = encode_voxel_chunk_key(max_cx, max_cy, max_cz);
        let lo_u = u64::from_be_bytes(lo[2..10].try_into().unwrap());
        let hi_u = u64::from_be_bytes(hi[2..10].try_into().unwrap());
        assert!(lo_u <= hi_u);

        // Search a small neighbourhood for an out-of-box chunk whose Morton
        // key lands inside [lo..=hi] — proving the range is a SUPERSET.
        let in_box = |cx: i32, cy: i32, cz: i32| {
            (min_cx..=max_cx).contains(&cx)
                && (min_cy..=max_cy).contains(&cy)
                && (min_cz..=max_cz).contains(&cz)
        };
        let mut intruder: Option<(i32, i32, i32)> = None;
        'search: for cx in -2..=3 {
            for cy in -2..=3 {
                for cz in -2..=5 {
                    if in_box(cx, cy, cz) {
                        continue;
                    }
                    let k = encode_voxel_chunk_key(cx, cy, cz);
                    let ku = u64::from_be_bytes(k[2..10].try_into().unwrap());
                    if lo_u <= ku && ku <= hi_u {
                        intruder = Some((cx, cy, cz));
                        break 'search;
                    }
                }
            }
        }
        let intruder = intruder.expect(
            "Z-order range over a thin box must contain an out-of-box chunk \
             (litmax/bigmin) — otherwise the filter test proves nothing",
        );

        // Now store the intruder (and one genuinely in-box chunk) and run a
        // region query whose chunk box is exactly [min..=max]. The intruder
        // is inside the Morton SCAN range but outside the box → the filter
        // must drop it.
        let (db, tmp) = fresh_db();
        db.put_voxel_chunk(intruder.0, intruder.1, intruder.2, b"intruder")
            .unwrap();
        db.put_voxel_chunk(min_cx, min_cy, min_cz, b"in-box").unwrap();

        let region_min = chunk_centre(min_cx, min_cy, min_cz);
        let region_max = chunk_centre(max_cx, max_cy, max_cz);
        let got: Vec<(i32, i32, i32)> = db
            .iter_voxel_chunks_in_region(region_min, region_max)
            .unwrap()
            .into_iter()
            .map(|(c, _)| c)
            .collect();

        assert!(
            !got.contains(&intruder),
            "filter must exclude the in-range, out-of-box chunk {intruder:?}; got {got:?}"
        );
        assert!(
            got.contains(&(min_cx, min_cy, min_cz)),
            "in-box chunk must be present; got {got:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
