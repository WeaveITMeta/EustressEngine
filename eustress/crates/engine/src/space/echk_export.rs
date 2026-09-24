//! # Exporting a world as `.echk`
//!
//! Publishing and hosting both need a Space as the bytes a Player can open:
//! every file of the Space, with every entity in its *current* state. This
//! module builds that record set, hands it to `eustress_worlddb::bake` to be
//! bucketed into spatial chunks, and assembles the [`WorldManifest`] that
//! names them.
//!
//! ## One record per entity, current state
//!
//! The WorldDb can hold up to three copies of one entity, and which copy is
//! current depends on the Space:
//!
//! 1. the `tree` partition's `_instance.toml` text;
//! 2. a `<path>#bin` twin, the bincode `InstanceDefinition` that property
//!    edits write (`active_db::put_instance`), which `get_instance` reads
//!    before the text;
//! 3. a Morton core, the rkyv `ArchInstanceCore` that the open-time bake
//!    (`bake_cores`) writes for every streamable leaf, keeping the tree entry.
//!
//! The rules, from the worlddb owner:
//!
//! - **`#bin` overlays the text.** Its typed tables replace the text's; any
//!   section the typed struct does not model (`[material]`,
//!   `[thermodynamic]`, …) is kept. Skipped exactly where `get_instance`
//!   skips it: file-natured classes, custom meshes, and parents. In practice
//!   an instance twin rarely exists: `InstanceDefinition` has a
//!   `#[serde(flatten)]` field and `toml::Value` fields, and bincode 1.x can
//!   neither serialize an unknown-length map nor `deserialize_any`, so
//!   `put_instance`'s binary write fails and the text path runs. A twin that
//!   does not decode is ignored here exactly as `get_instance` ignores it.
//! - **A baked core is a copy.** A core whose id is
//!   `stored_id_from_uuid(uuid)` of a tree entity mirrors that entity. In a
//!   small Space (at most `big_space_threshold` cores) the loader spawns the
//!   tree entry, so the tree is current and the core is dropped. In a large
//!   Space the loader streams the core for every streamable entry, so the
//!   core is current: it is written back at the tree path, keeping the tree's
//!   metadata, with its world transform turned back into a parent-relative one.
//!   Where both hold a table the core wins key by key; keys only the text has,
//!   and the three `[properties]` keys a core never stores (`physics`,
//!   `respect_gltf_materials`, `destructible`), keep the text's values.
//! - **A core with no tree entity** (Insert menu, importer) is published at
//!   the synthetic path `Workspace/__bin_{class}_{id:016x}/_instance.toml`
//!   the engine already uses for it, with its uuid from `path_to_uuid`.
//!
//! Every regenerated document is written through `toml::Value`, whose tables
//! are sorted maps, so the same Space always bakes to the same bytes and an
//! unchanged chunk is never uploaded twice.
//!
//! ## Files the database never ingested
//!
//! The seed import copies every file of a Space into the tree, but a binary
//! file added afterwards (a mesh an import wrote, a texture dropped into the
//! folder) may exist only on disk, where the renderer finds it. A Space read
//! from its database therefore also takes every file from its folder whose
//! path the tree lacks, except `.toml` documents: an entity the tree no longer
//! has was deleted, and must not come back from a file a delete left behind.
//!
//! ## Chunk size
//!
//! The bake puts every file without a position (meshes, scripts, textures) in
//! chunk (0,0), which in an imported world runs to hundreds of megabytes: more
//! than one upload request carries and more than a Player accepts. Any chunk
//! over [`MAX_EXPORT_CHUNK_BYTES`] is repacked into several at the same
//! coordinate. Readers do not care how a Space's records are split.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bevy::prelude::*;
use eustress_echk::{
    content_hash, decode_chunk, encode_chunk, is_safe_record_path, ChunkEntry, Record, SpaceManifest, WorldManifest,
};
use eustress_worlddb::{
    ChangeStream, Commit, ComponentTypeId, EntityId, Filter, Result as DbResult, Subscription, TreeEntry, TxId, WorldDb,
};

use super::instance_loader::{InstanceDefinition, TransformData};
use super::representation;

const BIN_SUFFIX: &str = "#bin";
/// Largest chunk the Universe `assets/` packer builds. Asset files have no
/// position, so they are packed by size rather than bucketed in space.
pub const ASSET_CHUNK_BYTES: usize = 32 * 1024 * 1024;
/// Largest chunk an export keeps whole. A single file larger than this still
/// gets a chunk of its own.
pub const MAX_EXPORT_CHUNK_BYTES: u64 = 64 * 1024 * 1024;
/// How long after a save the tree takes to hold what the save wrote: it lags
/// disk by 350 ms to 1.2 s. An export that follows a save waits this long.
pub const SAVE_SETTLE: Duration = Duration::from_millis(1500);

/// What an export did, for the log and the publish summary.
#[derive(Debug, Clone, Default)]
pub struct ExportStats {
    /// Files published (entities included).
    pub files: usize,
    /// `_instance.toml` entities published.
    pub entities: usize,
    /// Entities whose `#bin` edits were folded into their text.
    pub bin_overlays: usize,
    /// Large Space: cores written back at their tree path.
    pub cores_written_back: usize,
    /// Cores with no tree entity, published at their synthetic path.
    pub standalone_cores: usize,
    /// Small Space: baked core copies dropped in favour of the tree.
    pub baked_copies_dropped: usize,
    /// The Space streams from its cores (more than `big_space_threshold`).
    pub large_space: bool,
    /// Paths left out, with the reason.
    pub skipped: Vec<String>,
}

/// Where one Space's content comes from.
pub enum SpaceSource {
    /// The Space's WorldDb: the engine's active DB, or one opened for the export.
    Db(Arc<dyn WorldDb>),
    /// A Space folder with no database: the files are the content.
    Disk(PathBuf),
}

/// One Space to export.
pub struct SpaceInput {
    pub name: String,
    pub source: SpaceSource,
    /// The Space's folder, for a [`SpaceSource::Db`] Space: files found there
    /// that the tree lacks are published too (see the module doc).
    pub folder: Option<PathBuf>,
}

/// A baked world on disk: the manifest plus where each chunk lives.
pub struct ExportedWorld {
    pub manifest: WorldManifest,
    /// Chunk file for every content hash the manifest names.
    pub files: HashMap<String, PathBuf>,
    pub stats: Vec<(String, ExportStats)>,
}

impl ExportedWorld {
    /// Read one chunk, checking it still hashes to its name.
    pub fn read_chunk(&self, hash: &str) -> Result<Vec<u8>, String> {
        let path = self.files.get(hash).ok_or_else(|| format!("no chunk {hash}"))?;
        let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if content_hash(&bytes) != hash {
            return Err(format!("{} changed on disk since it was baked", path.display()));
        }
        Ok(bytes)
    }

    /// Every chunk in memory, for a host that serves them to players.
    pub fn load_all(&self) -> Result<HashMap<String, Arc<Vec<u8>>>, String> {
        let mut out = HashMap::with_capacity(self.files.len());
        for hash in self.files.keys() {
            out.insert(hash.clone(), Arc::new(self.read_chunk(hash)?));
        }
        Ok(out)
    }
}

/// Export Spaces (and optionally the Universe's `assets/`) into `out_root`.
///
/// `out_root` persists between runs on purpose: the bake compares each chunk
/// with the one it wrote last time and leaves unchanged chunks alone.
pub fn export_world(
    universe: &str,
    spaces: Vec<SpaceInput>,
    start_space: &str,
    assets_root: Option<&Path>,
    out_root: &Path,
    big_space_threshold: usize,
) -> Result<ExportedWorld, String> {
    let mut manifest = WorldManifest::new(universe, env!("CARGO_PKG_VERSION"), eustress_worlddb::bake::DEFAULT_CHUNK_SIZE);
    manifest.start_space = start_space.to_string();
    let mut files = HashMap::new();
    let mut all_stats = Vec::new();

    for space in spaces {
        let (mut records, mut stats) = match &space.source {
            SpaceSource::Db(db) => collect_from_db(db.as_ref(), big_space_threshold)?,
            SpaceSource::Disk(root) => collect_from_disk(root)?,
        };
        let disk_only = match (&space.source, &space.folder) {
            (SpaceSource::Db(_), Some(folder)) => add_disk_only_files(&mut records, folder, &mut stats)?,
            _ => 0,
        };
        info!(
            "echk export: {}: {} files, {} entities ({} #bin overlays, {} cores written back, {} standalone cores, {} baked copies dropped, {} files only on disk){}",
            space.name,
            stats.files,
            stats.entities,
            stats.bin_overlays,
            stats.cores_written_back,
            stats.standalone_cores,
            stats.baked_copies_dropped,
            disk_only,
            if stats.large_space { ", streaming Space" } else { "" }
        );
        let dir = out_root.join("spaces").join(&space.name);
        let baked = bake_space(records, &dir)?;
        let chunks = split_oversized(baked, &dir, &mut files)?;
        manifest.spaces.push(SpaceManifest { name: space.name.clone(), chunks });
        all_stats.push((space.name, stats));
    }

    if let Some(root) = assets_root {
        let records = collect_assets(root)?;
        if !records.is_empty() {
            let dir = out_root.join("assets");
            let chunks = pack_assets(records, &dir)?;
            for c in &chunks {
                files.insert(c.blake3.clone(), dir.join("chunks").join(&c.file));
            }
            manifest.assets = chunks;
        }
    }

    manifest.canonicalize();
    manifest.validate().map_err(|e| format!("the exported world failed its own check: {e}"))?;
    Ok(ExportedWorld { manifest, files, stats: all_stats })
}

// ─────────────────────────────────────────────────────────────────────────────
// Collecting records
// ─────────────────────────────────────────────────────────────────────────────

/// The current state of a DB-backed Space, one record per file and entity.
pub fn collect_from_db(db: &dyn WorldDb, big_space_threshold: usize) -> Result<(Vec<Record>, ExportStats), String> {
    let mut stats = ExportStats::default();

    // 1. Snapshot the tree, setting `#bin` twins aside.
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut bins: HashMap<String, Vec<u8>> = HashMap::new();
    for row in db.iter_tree().map_err(|e| format!("tree scan: {e}"))? {
        let (key, bytes) = row.map_err(|e| format!("tree scan: {e}"))?;
        if let Some(base) = key.strip_suffix(BIN_SUFFIX) {
            bins.insert(base.to_string(), bytes);
            continue;
        }
        if !publishable(&key) {
            stats.skipped.push(format!("{key}: not part of the Space's content"));
            continue;
        }
        files.insert(key, bytes);
    }

    // 2. Small or large, the way the loader decides it.
    let cap = big_space_threshold.saturating_add(1);
    stats.large_space = db.count_instance_cores_capped(cap).unwrap_or(0) > big_space_threshold;

    // 3. Cores by stored id.
    let mut cores: BTreeMap<u64, Vec<u8>> = db
        .iter_instance_cores()
        .map_err(|e| format!("core scan: {e}"))?
        .into_iter()
        .map(|(id, bytes)| (id.0, bytes))
        .collect();

    // 4. Hierarchy: direct parents (the streaming predicate's view) and every
    //    ancestor (the `#bin` rule's view, which is recursive).
    let entity_keys: Vec<&str> = files.keys().map(String::as_str).filter(|k| is_entity(k)).collect();
    let mut direct_parents: HashSet<String> = HashSet::new();
    let mut ancestors: HashSet<String> = HashSet::new();
    for key in &entity_keys {
        if let Some(p) = parent_key(key) {
            direct_parents.insert(p);
        }
        for a in ancestor_keys(key) {
            ancestors.insert(a);
        }
    }

    // 5. Tree entries.
    let mut out: Vec<Record> = Vec::with_capacity(files.len() + cores.len());
    let mut world_cache: HashMap<String, Transform> = HashMap::new();
    for (key, bytes) in &files {
        if !is_entity(key) {
            out.push((key.clone(), bytes.clone()));
            continue;
        }
        stats.entities += 1;
        let baked = db
            .path_to_uuid(key)
            .ok()
            .flatten()
            .map(|uuid| super::bake_cores::stored_id_from_uuid(&uuid))
            .and_then(|sid| cores.remove(&sid));

        if stats.large_space {
            if let Some(core_bytes) = &baked {
                if streams_from_db(key, bytes, &direct_parents) {
                    match core_at_tree_path(key, bytes, core_bytes, &files, &mut world_cache) {
                        Ok(text) => {
                            out.push((key.clone(), text.into_bytes()));
                            stats.cores_written_back += 1;
                            continue;
                        }
                        Err(e) => stats.skipped.push(format!("{key}: core not written back ({e}); tree text used")),
                    }
                }
            }
        } else if baked.is_some() {
            stats.baked_copies_dropped += 1;
        }

        match bins.get(key.as_str()).and_then(|bin| overlay_bin(key, bytes, bin, &ancestors)) {
            Some(text) => {
                out.push((key.clone(), text.into_bytes()));
                stats.bin_overlays += 1;
            }
            None => out.push((key.clone(), bytes.clone())),
        }
    }

    // 6. Cores with no tree entity.
    for (sid, core_bytes) in &cores {
        match standalone_core(db, *sid, core_bytes) {
            Ok(record) => {
                out.push(record);
                stats.standalone_cores += 1;
                stats.entities += 1;
            }
            Err(e) => stats.skipped.push(format!("core {sid:016x}: {e}")),
        }
    }

    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    stats.files = out.len();
    Ok((out, stats))
}

/// A Space with no database: its files are its content.
pub fn collect_from_disk(space_root: &Path) -> Result<(Vec<Record>, ExportStats), String> {
    let mut stats = ExportStats::default();
    let mut out = Vec::new();
    walk_files(space_root, space_root, "", &mut out, &mut stats)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    stats.entities = out.iter().filter(|(p, _)| is_entity(p)).count();
    stats.files = out.len();
    Ok((out, stats))
}

/// Add the files in `folder` whose paths `records` lacks, `.toml` documents
/// excepted (see the module doc). Returns how many were added.
pub fn add_disk_only_files(records: &mut Vec<Record>, folder: &Path, stats: &mut ExportStats) -> Result<usize, String> {
    if !folder.is_dir() {
        return Ok(0);
    }
    let known: HashSet<String> = records.iter().map(|(p, _)| p.clone()).collect();
    let mut found = Vec::new();
    walk_paths(folder, folder, &mut found)?;
    let mut added = 0;
    for (rel, path) in found {
        if rel.ends_with(".toml") || known.contains(&rel) {
            continue;
        }
        if !is_safe_record_path(&rel) {
            stats.skipped.push(format!("{rel}: path cannot be published"));
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        records.push((rel, bytes));
        added += 1;
    }
    if added > 0 {
        records.sort_by(|a, b| a.0.cmp(&b.0));
        stats.files = records.len();
    }
    Ok(added)
}

/// Every publishable file under `dir`, as (path relative to `base`, path).
fn walk_paths(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_name(&name) {
            continue;
        }
        if path.is_dir() {
            walk_paths(base, &path, out)?;
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        out.push((rel, path));
    }
    Ok(())
}

/// The Universe's shared `assets/` folder, as records under `assets/`.
pub fn collect_assets(universe_root: &Path) -> Result<Vec<Record>, String> {
    let root = universe_root.join("assets");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut stats = ExportStats::default();
    walk_files(&root, &root, "assets/", &mut out, &mut stats)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk_files(base: &Path, dir: &Path, prefix: &str, out: &mut Vec<Record>, stats: &mut ExportStats) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_name(&name) {
            continue;
        }
        if path.is_dir() {
            walk_files(base, &path, prefix, out, stats)?;
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let rel = format!("{prefix}{rel}");
        if !is_safe_record_path(&rel) {
            stats.skipped.push(format!("{rel}: path cannot be published"));
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        out.push((rel, bytes));
    }
    Ok(())
}

/// Names never published: hidden entries (`.eustress/`, `.git/`), the
/// database and its backups, and temporaries.
fn skip_name(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with("world.fjalldb")
        || name.starts_with("header.bin")
        || name.ends_with(".tmp")
        || name.ends_with(".lock")
        || matches!(name, "Thumbs.db" | "desktop.ini")
}

fn publishable(key: &str) -> bool {
    is_safe_record_path(key) && !key.split('/').any(skip_name)
}

fn is_entity(key: &str) -> bool {
    key.ends_with("/_instance.toml")
}

/// The tree key of an entity's parent entity; `None` for a service's direct
/// child. Same rule as `bake_cores`.
fn parent_key(key: &str) -> Option<String> {
    let folder = key.strip_suffix("/_instance.toml")?;
    let up = folder.rsplit_once('/')?.0;
    if !up.contains('/') {
        return None;
    }
    Some(format!("{up}/_instance.toml"))
}

/// Every ancestor folder that could hold an entity, as its `_instance.toml`
/// key, whether or not one exists. `active_db`'s parent check is recursive,
/// so a folder with an entity anywhere below it counts.
fn ancestor_keys(key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(mut folder) = key.strip_suffix("/_instance.toml") else { return out };
    while let Some((up, _)) = folder.rsplit_once('/') {
        if !up.contains('/') {
            break; // the service folder
        }
        out.push(format!("{up}/_instance.toml"));
        folder = up;
    }
    out
}

/// Whether the loader streams this entry from its core in a large Space:
/// the same predicate `bake_cores` and `file_loader` share.
fn streams_from_db(key: &str, text: &[u8], direct_parents: &HashSet<String>) -> bool {
    let Ok(text) = std::str::from_utf8(text) else { return false };
    let Ok(def) = toml::from_str::<InstanceDefinition>(text) else { return false };
    representation::streams_from_db(
        &def.metadata.class_name,
        direct_parents.contains(key),
        representation::toml_mentions_custom_mesh(text),
    )
}

/// Fold a `#bin` twin into its text, where `get_instance` would honour it.
fn overlay_bin(key: &str, text: &[u8], bin: &[u8], ancestors: &HashSet<String>) -> Option<String> {
    let def: InstanceDefinition = bincode::deserialize(bin).ok()?;
    let mesh = def.asset.as_ref().map(|a| a.mesh.as_str());
    let stays_filesystem = representation::class_is_file_natured(&def.metadata.class_name)
        || mesh.map(representation::mesh_requires_filesystem).unwrap_or(false);
    if stays_filesystem || ancestors.contains(key) {
        return None;
    }
    let typed = toml::Value::try_from(&def).ok()?;
    let doc = match std::str::from_utf8(text).ok().and_then(|t| t.parse::<toml::Value>().ok()) {
        Some(mut doc) => {
            merge_tables(&mut doc, typed, &[]);
            doc
        }
        None => typed,
    };
    toml::to_string(&doc).ok()
}

/// A large Space's core, written back at its tree path: the tree's own
/// document with the core's typed tables over it, the tree's metadata kept,
/// and the core's world transform made parent-relative again.
fn core_at_tree_path(
    key: &str,
    text: &[u8],
    core_bytes: &[u8],
    files: &BTreeMap<String, Vec<u8>>,
    world_cache: &mut HashMap<String, Transform>,
) -> Result<String, String> {
    let core = eustress_worlddb::decode_instance_core(core_bytes).map_err(|e| e.to_string())?;
    let mut def = super::arch_instance::arch_to_instance(&core);
    let world = Transform::from(def.transform.clone());
    let parent = parent_world(key, files, world_cache);
    let local = Transform::from_matrix(parent.to_matrix().inverse() * world.to_matrix());
    def.transform = TransformData::from(local);

    let mut typed = toml::Value::try_from(&def).map_err(|e| e.to_string())?;
    drop_core_defaults(&mut typed);
    let mut doc: toml::Value = std::str::from_utf8(text)
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|e: toml::de::Error| e.to_string())?;
    merge_tables(&mut doc, typed, &["metadata"]);
    toml::to_string(&doc).map_err(|e| e.to_string())
}

/// `[properties]` keys a core does not store: `arch_to_instance` fills them
/// with defaults, so the tree's text holds the only real value.
const CORE_DEFAULTED_PROPERTIES: &[&str] = &["physics", "respect_gltf_materials", "destructible"];

fn drop_core_defaults(typed: &mut toml::Value) {
    if let Some(props) = typed.get_mut("properties").and_then(|p| p.as_table_mut()) {
        for key in CORE_DEFAULTED_PROPERTIES {
            props.remove(*key);
        }
    }
}

/// The world transform of `key`'s parent chain, composed root-down from the
/// tree's text. Parents are never baked, so their text is current.
fn parent_world(key: &str, files: &BTreeMap<String, Vec<u8>>, cache: &mut HashMap<String, Transform>) -> Transform {
    let mut chain: Vec<String> = Vec::new();
    let mut base = Transform::IDENTITY;
    let mut cursor = parent_key(key);
    while let Some(k) = cursor {
        if let Some(t) = cache.get(&k) {
            base = *t;
            break;
        }
        cursor = parent_key(&k);
        chain.push(k);
        if chain.len() > 4096 {
            break; // a malformed key set; treat the rest as a root
        }
    }
    let mut world = base;
    for k in chain.iter().rev() {
        let local = files
            .get(k)
            .and_then(|b| std::str::from_utf8(b).ok())
            .and_then(|t| toml::from_str::<InstanceDefinition>(t).ok())
            .map(|d| Transform::from(d.transform))
            .unwrap_or(Transform::IDENTITY);
        world = world.mul_transform(local);
        cache.insert(k.clone(), world);
    }
    world
}

/// A core with no tree entity, at the engine's synthetic path.
fn standalone_core(db: &dyn WorldDb, sid: u64, core_bytes: &[u8]) -> Result<Record, String> {
    let core = eustress_worlddb::decode_instance_core(core_bytes).map_err(|e| e.to_string())?;
    let mut def = super::arch_instance::arch_to_instance(&core);
    let class: String = def
        .metadata
        .class_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let rel = format!("Workspace/__bin_{class}_{sid:016x}/_instance.toml");
    if def.metadata.uuid.is_none() {
        if let Ok(Some(uuid)) = db.path_to_uuid(&rel) {
            def.metadata.uuid = Some(eustress_common::instance_create::uuid_bytes_to_hex(&uuid));
        }
    }
    let value = toml::Value::try_from(&def).map_err(|e| e.to_string())?;
    let text = toml::to_string(&value).map_err(|e| e.to_string())?;
    Ok((rel, text.into_bytes()))
}

/// Lay `typed`'s entries over `doc`'s, except the top-level keys named in
/// `keep`. Where both hold a table, `typed` wins key by key and keys only
/// `doc` has survive, so a section the typed struct models only in part
/// keeps the rest of what the text says.
fn merge_tables(doc: &mut toml::Value, typed: toml::Value, keep: &[&str]) {
    let (Some(doc_table), toml::Value::Table(typed_table)) = (doc.as_table_mut(), typed) else { return };
    for (k, v) in typed_table {
        if keep.contains(&k.as_str()) && doc_table.contains_key(&k) {
            continue;
        }
        match v {
            toml::Value::Table(incoming) if doc_table.get(&k).is_some_and(|e| e.is_table()) => {
                if let Some(toml::Value::Table(existing)) = doc_table.get_mut(&k) {
                    for (ik, iv) in incoming {
                        existing.insert(ik, iv);
                    }
                }
            }
            v => {
                doc_table.insert(k, v);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Baking
// ─────────────────────────────────────────────────────────────────────────────

/// Bake one Space's records into `dir/chunks/*.echk` + `dir/manifest.toml`
/// with worlddb's bake, and return its chunks after checking every file
/// hashes to its manifest entry.
pub fn bake_space(records: Vec<Record>, dir: &Path) -> Result<Vec<ChunkEntry>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let view = RecordView::new(records);
    let bake = || -> Result<Vec<ChunkEntry>, String> {
        eustress_worlddb::bake::bake_to_echk_with(&view, dir, eustress_worlddb::bake::DEFAULT_CHUNK_SIZE)
            .map_err(|e| format!("bake {}: {e}", dir.display()))?;
        let text = std::fs::read_to_string(dir.join("manifest.toml"))
            .map_err(|e| format!("read {}/manifest.toml: {e}", dir.display()))?;
        let manifest = eustress_echk::parse_bake_manifest(&text).map_err(|e| e.to_string())?;
        if !(eustress_echk::MIN_VERSION..=eustress_echk::VERSION).contains(&manifest.encoder_version) {
            return Err(format!(
                "the bake wrote .echk version {}, this engine reads {} to {}",
                manifest.encoder_version,
                eustress_echk::MIN_VERSION,
                eustress_echk::VERSION
            ));
        }
        for c in &manifest.chunks {
            let path = dir.join("chunks").join(&c.file);
            let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            if content_hash(&bytes) != c.blake3 {
                return Err(format!("{} does not match its manifest entry", path.display()));
            }
        }
        Ok(manifest.chunks)
    };
    match bake() {
        Ok(chunks) => Ok(chunks),
        Err(first) => {
            // The bake skips chunks whose hash matches its last manifest,
            // trusting the file is still there. If one was deleted or edited
            // since, forget the old manifest and bake everything once more.
            warn!("echk export: {first}; rebaking {} from scratch", dir.display());
            let _ = std::fs::remove_file(dir.join("manifest.toml"));
            bake()
        }
    }
}

/// Repack every chunk over [`MAX_EXPORT_CHUNK_BYTES`] into several at the
/// same coordinate, written under `dir/split/`, and record where each chunk
/// the result names lives.
pub fn split_oversized(
    chunks: Vec<ChunkEntry>,
    dir: &Path,
    files: &mut HashMap<String, PathBuf>,
) -> Result<Vec<ChunkEntry>, String> {
    split_at(chunks, dir, files, MAX_EXPORT_CHUNK_BYTES)
}

fn split_at(
    chunks: Vec<ChunkEntry>,
    dir: &Path,
    files: &mut HashMap<String, PathBuf>,
    limit: u64,
) -> Result<Vec<ChunkEntry>, String> {
    let chunks_dir = dir.join("chunks");
    let split_dir = dir.join("split");
    // Rewritten whole each time, so a split that shrinks leaves no strays.
    if split_dir.exists() {
        std::fs::remove_dir_all(&split_dir).map_err(|e| format!("clear {}: {e}", split_dir.display()))?;
    }
    let mut out = Vec::with_capacity(chunks.len());
    for c in chunks {
        let path = chunks_dir.join(&c.file);
        if c.size <= limit {
            files.insert(c.blake3.clone(), path);
            out.push(c);
            continue;
        }
        let records = {
            let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            decode_chunk(&bytes).map_err(|e| format!("{}: {e}", path.display()))?
        };
        std::fs::create_dir_all(&split_dir).map_err(|e| format!("create {}: {e}", split_dir.display()))?;
        let mut part: Vec<Record> = Vec::new();
        let mut part_bytes = 12u64;
        for record in records {
            let n = 8 + record.0.len() as u64 + record.1.len() as u64;
            if !part.is_empty() && part_bytes + n > limit {
                let file = format!("{}_{}.{}.echk", c.cx, c.cz, out.len());
                let entry = write_chunk_file(&split_dir, &file, &part, c.cx, c.cz)?;
                files.insert(entry.blake3.clone(), split_dir.join(&entry.file));
                out.push(entry);
                part.clear();
                part_bytes = 12;
            }
            part_bytes += n;
            part.push(record);
        }
        if !part.is_empty() {
            let file = format!("{}_{}.{}.echk", c.cx, c.cz, out.len());
            let entry = write_chunk_file(&split_dir, &file, &part, c.cx, c.cz)?;
            files.insert(entry.blake3.clone(), split_dir.join(&entry.file));
            out.push(entry);
        }
        info!("echk export: split {} ({} MB) at ({}, {})", c.file, c.size / (1024 * 1024), c.cx, c.cz);
    }
    Ok(out)
}

/// Pack asset records into size-bounded chunks under `dir/chunks/`.
pub fn pack_assets(records: Vec<Record>, dir: &Path) -> Result<Vec<ChunkEntry>, String> {
    let chunks_dir = dir.join("chunks");
    std::fs::create_dir_all(&chunks_dir).map_err(|e| format!("create {}: {e}", chunks_dir.display()))?;
    let mut entries = Vec::new();
    let mut batch: Vec<Record> = Vec::new();
    let mut batch_bytes = 0usize;
    for record in records {
        let n = record.1.len();
        if !batch.is_empty() && batch_bytes + n > ASSET_CHUNK_BYTES {
            entries.push(write_asset_chunk(&chunks_dir, entries.len(), &batch)?);
            batch.clear();
            batch_bytes = 0;
        }
        batch_bytes += n;
        batch.push(record);
    }
    if !batch.is_empty() {
        entries.push(write_asset_chunk(&chunks_dir, entries.len(), &batch)?);
    }
    Ok(entries)
}

fn write_asset_chunk(dir: &Path, index: usize, batch: &[Record]) -> Result<ChunkEntry, String> {
    write_chunk_file(dir, &format!("assets_{index}.echk"), batch, index as i32, 0)
}

/// Encode `batch` into `dir/file`, leaving an identical file untouched.
fn write_chunk_file(dir: &Path, file: &str, batch: &[Record], cx: i32, cz: i32) -> Result<ChunkEntry, String> {
    let bytes = encode_chunk(batch);
    let hash = content_hash(&bytes);
    let file = file.to_string();
    let final_path = dir.join(&file);
    let unchanged = std::fs::read(&final_path).map(|b| content_hash(&b) == hash).unwrap_or(false);
    if !unchanged {
        let tmp = dir.join(format!("{file}.tmp"));
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
            f.write_all(&bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
            f.sync_all().map_err(|e| format!("sync {}: {e}", tmp.display()))?;
        }
        std::fs::rename(&tmp, &final_path).map_err(|e| format!("rename {}: {e}", final_path.display()))?;
    }
    Ok(ChunkEntry {
        cx,
        cz,
        file,
        size: bytes.len() as u64,
        count: batch.len() as u32,
        blake3: hash,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// A read-only WorldDb over a fixed record set
// ─────────────────────────────────────────────────────────────────────────────

/// The bake takes a `&dyn WorldDb` and reads only its `tree`. This view
/// presents the adapted records as that tree, so the bake chunks exactly
/// what was decided above and nothing else. Every write is refused.
struct RecordView {
    records: Vec<Record>,
    stream: ChangeStream,
}

impl RecordView {
    fn new(mut records: Vec<Record>) -> Self {
        records.sort_by(|a, b| a.0.cmp(&b.0));
        Self { records, stream: ChangeStream::new() }
    }
}

fn read_only() -> eustress_worlddb::Error {
    eustress_worlddb::Error::Other("the export view is read-only".into())
}

impl WorldDb for RecordView {
    fn apply_commit(&self, _commit: Commit) -> DbResult<TxId> {
        Err(read_only())
    }
    fn get_component(&self, _entity: EntityId, _component: ComponentTypeId) -> DbResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn iter_component(
        &self,
        _component: ComponentTypeId,
    ) -> DbResult<Box<dyn Iterator<Item = DbResult<(EntityId, Vec<u8>)>> + '_>> {
        Ok(Box::new(std::iter::empty()))
    }
    fn flush(&self) -> DbResult<()> {
        Ok(())
    }
    fn subscribe(&self, filter: Filter) -> Subscription {
        self.stream.subscribe(filter)
    }
    fn change_stream(&self) -> &ChangeStream {
        &self.stream
    }
    fn put_file(&self, _rel_path: &str, _bytes: &[u8]) -> DbResult<()> {
        Err(read_only())
    }
    fn get_file(&self, rel_path: &str) -> DbResult<Option<Vec<u8>>> {
        Ok(self
            .records
            .binary_search_by(|(p, _)| p.as_str().cmp(rel_path))
            .ok()
            .map(|i| self.records[i].1.clone()))
    }
    fn delete_file(&self, _rel_path: &str) -> DbResult<()> {
        Err(read_only())
    }
    fn list_dir(&self, _rel_dir: &str) -> DbResult<Vec<TreeEntry>> {
        Ok(Vec::new())
    }
    fn tree_is_empty(&self) -> DbResult<bool> {
        Ok(self.records.is_empty())
    }
    fn iter_tree(&self) -> DbResult<Box<dyn Iterator<Item = DbResult<(String, Vec<u8>)>> + '_>> {
        Ok(Box::new(self.records.iter().map(|(p, d)| Ok((p.clone(), d.clone())))))
    }
    fn ds_get(&self, _store: &str, _scope: &str, _key: &str) -> DbResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn ds_set(&self, _store: &str, _scope: &str, _key: &str, _value: &[u8]) -> DbResult<()> {
        Err(read_only())
    }
    fn ds_remove(&self, _store: &str, _scope: &str, _key: &str) -> DbResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn ds_update(
        &self,
        _store: &str,
        _scope: &str,
        _key: &str,
        _max_retries: u32,
        _transform: &mut dyn FnMut(Option<Vec<u8>>) -> Option<Vec<u8>>,
    ) -> DbResult<Option<Vec<u8>>> {
        Err(read_only())
    }
    fn ds_range(
        &self,
        _store: &str,
        _scope: &str,
        _ascending: bool,
        _limit: usize,
        _min: Option<i64>,
        _max: Option<i64>,
        _cursor: &str,
    ) -> DbResult<Vec<(String, Vec<u8>, i64)>> {
        Ok(Vec::new())
    }
    fn ds_set_sorted(&self, _store: &str, _scope: &str, _key: &str, _value: &[u8], _sort: i64) -> DbResult<()> {
        Err(read_only())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_keys_match_the_bake_rules() {
        assert_eq!(parent_key("Workspace/A/_instance.toml"), None);
        assert_eq!(parent_key("Workspace/A/B/_instance.toml").as_deref(), Some("Workspace/A/_instance.toml"));
        assert_eq!(
            ancestor_keys("Workspace/A/B/C/_instance.toml"),
            vec!["Workspace/A/B/_instance.toml".to_string(), "Workspace/A/_instance.toml".to_string()]
        );
    }

    #[test]
    fn hidden_and_database_paths_are_never_published() {
        assert!(!publishable(".eustress/output.log"));
        assert!(!publishable("Workspace/.eustress/trash/x/_instance.toml"));
        assert!(!publishable("world.fjalldb.bak-20260521/x"));
        assert!(publishable("Workspace/Floor/_instance.toml"));
    }

    #[test]
    fn merge_keeps_unmodelled_sections_and_is_deterministic() {
        let tree: toml::Value = "[metadata]\nclass_name = \"Part\"\nname = \"Tree\"\n\n\
            [material]\nname = \"Steel\"\n\n[transform]\nposition = [0.0, 0.0, 0.0]\n"
            .parse()
            .unwrap();
        let typed: toml::Value = "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [1.0, 2.0, 3.0]\n"
            .parse()
            .unwrap();
        let mut a = tree.clone();
        let mut b = tree;
        merge_tables(&mut a, typed.clone(), &["metadata"]);
        merge_tables(&mut b, typed, &["metadata"]);
        assert_eq!(toml::to_string(&a).unwrap(), toml::to_string(&b).unwrap(), "same inputs, same bytes");
        assert_eq!(a["material"]["name"].as_str(), Some("Steel"), "unmodelled section lost");
        assert_eq!(a["transform"]["position"][0].as_float(), Some(1.0), "typed value lost");
        assert_eq!(a["metadata"]["name"].as_str(), Some("Tree"), "kept table overwritten");
    }

    #[test]
    fn a_core_never_overwrites_what_it_cannot_store() {
        // The tree text knows the part is destructible and has a physics
        // table; the core defaults both. The core's color is newer.
        let mut doc: toml::Value = "[metadata]\nclass_name = \"Part\"\n\n\
            [properties]\ncolor = [1.0, 0.0, 0.0, 1.0]\ndestructible = true\n\n\
            [properties.physics]\nmass = 12.0\n"
            .parse()
            .unwrap();
        let mut typed: toml::Value = "[properties]\ncolor = [0.0, 0.0, 1.0, 1.0]\ndestructible = false\n\
            respect_gltf_materials = false\n"
            .parse()
            .unwrap();
        drop_core_defaults(&mut typed);
        merge_tables(&mut doc, typed, &["metadata"]);
        assert_eq!(doc["properties"]["color"][2].as_float(), Some(1.0), "the core's newer color was lost");
        assert_eq!(doc["properties"]["destructible"].as_bool(), Some(true), "a core default overwrote the text");
        assert_eq!(doc["properties"]["physics"]["mass"].as_float(), Some(12.0), "a nested table the core lacks was lost");
        assert!(doc["properties"].get("respect_gltf_materials").is_none());
    }

    #[test]
    fn an_undecodable_bin_leaves_the_text_alone() {
        // bincode cannot round-trip InstanceDefinition (see the module doc),
        // so this is the case every instance twin hits in practice.
        let text = b"[metadata]\nclass_name = \"Part\"\n";
        assert!(overlay_bin("Workspace/P/_instance.toml", text, b"not bincode", &HashSet::new()).is_none());
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_echk_export_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_oversized_chunk_is_split_without_losing_a_record() {
        let dir = scratch("split");
        std::fs::create_dir_all(dir.join("chunks")).unwrap();
        // Version 1, as the bake writes: stored size is the raw size.
        let records: Vec<Record> = (0..5).map(|i| (format!("Workspace/P{i}/mesh.glb"), vec![i as u8; 40])).collect();
        let big = eustress_echk::encode_chunk_v1(&records);
        std::fs::write(dir.join("chunks/0_0.echk"), &big).unwrap();
        let small = eustress_echk::encode_chunk_v1(&[("Workspace/Q/_instance.toml".to_string(), b"x".to_vec())]);
        std::fs::write(dir.join("chunks/1_0.echk"), &small).unwrap();
        let entry = |cx, bytes: &[u8], count| ChunkEntry {
            cx, cz: 0, file: format!("{cx}_0.echk"), size: bytes.len() as u64, count, blake3: content_hash(bytes),
        };

        let mut files = HashMap::new();
        let limit = 150;
        let out = split_at(vec![entry(0, &big, 5), entry(1, &small, 1)], &dir, &mut files, limit).unwrap();

        assert!(out.len() > 2, "the big chunk was not split");
        let mut seen = Vec::new();
        for c in &out {
            let bytes = std::fs::read(&files[&c.blake3]).unwrap();
            assert_eq!(content_hash(&bytes), c.blake3, "a chunk's file does not match its name");
            assert_eq!(bytes.len() as u64, c.size);
            if c.count > 1 {
                assert!(c.size <= limit, "a multi-record part is over the limit");
            }
            seen.extend(decode_chunk(&bytes).unwrap());
        }
        seen.sort();
        let mut expected = records.clone();
        expected.push(("Workspace/Q/_instance.toml".to_string(), b"x".to_vec()));
        expected.sort();
        assert_eq!(seen, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_only_on_disk_join_but_toml_never_does() {
        let dir = scratch("disk_only");
        let put = |rel: &str, bytes: &[u8]| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        put("assets/meshes/wheel.glb", b"disk mesh");
        put("Workspace/Car/mesh.glb", b"disk copy");
        put("Workspace/Deleted/_instance.toml", b"[metadata]\nclass_name = \"Part\"\n");
        put(".eustress/cache.bin", b"hidden");
        put("world.fjalldb/journal", b"db");

        let mut records: Vec<Record> = vec![("Workspace/Car/mesh.glb".into(), b"tree copy".to_vec())];
        let mut stats = ExportStats::default();
        let added = add_disk_only_files(&mut records, &dir, &mut stats).unwrap();

        assert_eq!(added, 1);
        let paths: Vec<&str> = records.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, vec!["Workspace/Car/mesh.glb", "assets/meshes/wheel.glb"]);
        assert_eq!(records[0].1, b"tree copy", "the tree's copy wins");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_view_serves_exactly_its_records() {
        let view = RecordView::new(vec![("b".into(), vec![2]), ("a".into(), vec![1])]);
        let keys: Vec<String> = view.iter_tree().unwrap().map(|r| r.unwrap().0).collect();
        assert_eq!(keys, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(view.get_file("b").unwrap(), Some(vec![2]));
        assert!(view.put_file("c", &[]).is_err());
    }
}
