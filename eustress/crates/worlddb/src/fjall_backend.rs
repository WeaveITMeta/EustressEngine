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

        let keyspace = s_entities.raw_keyspace();
        let entities = s_entities.raw_partition();
        let meta = s_meta.raw_partition();
        let tree = s_tree.raw_partition();
        let datastore = s_datastore.raw_partition();
        let datastore_ord = s_datastore_ord.raw_partition();

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
            keyspace,
            entities,
            meta,
            tree,
            datastore,
            datastore_ord,
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
