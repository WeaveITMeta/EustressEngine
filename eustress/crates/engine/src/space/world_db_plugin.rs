//! `WorldDb` Bevy plugin — Phase 1 wiring of the Fjall-backed
//! authoritative ECS store. Gated by the `world-db` cargo feature.
//!
//! ## What this plugin does today (Phase 1)
//!
//! - On Space open, opens `<SpaceRoot>/world.fjalldb/` via
//!   [`eustress_worlddb::backend::open`] and stows the handle as a
//!   `Resource<WorldDbHandle>`.
//! - Mirrors `Changed<Transform>` and `Changed<BasePart>` writes into
//!   the WorldDb on a per-frame budget (so a big drag doesn't stall
//!   the frame). The legacy TOML write path still runs — Fjall is
//!   additive until Phase 3's importer + Phase 6's read-from-Fjall
//!   path land.
//! - Bridges the WorldDb change-stream's `CommitDelta` into a
//!   `Events<WorldDbCommit>` resource so subsystems (Loro, Telemetry
//!   tee, Watchman) can subscribe without depending on the worlddb
//!   crate directly.
//!
//! ## Which store is authoritative
//!
//! Fjall. `world_db_binary::load_binary_ecs_instances` cold-loads entity
//! cores from the `entities` partition at startup, and edits persist
//! there. The disk TOML hierarchy is the import seed plus an ingest
//! surface for files a human drops in between sessions (see the
//! reconcile below) — it is NOT kept in sync with edits, because
//! `write_instance_definition` skips the disk write for every instance
//! `active_db::put_instance` accepts. The `toml` feature restores
//! dual-write; it is deliberately not in the `core` tier.
//!
//! ## What this plugin does NOT do yet
//!
//! - Bridge to the `eustress-common::streaming` topic broker. The
//!   bridge here is a Bevy `Events<>` queue; the topic-name mapping
//!   (`world.entity.changed.<class>.<component>`) lives in the
//!   Telemetry plugin which subscribes to those events.

#![cfg(feature = "world-db")]

use std::sync::Arc;

use bevy::prelude::*;
use eustress_worlddb::{
    Commit, ComponentTypeId, EntityId as WdbEntityId, Filter, Subscription, TxId, WorldDb,
    WorldHeader,
};

use super::SpaceRoot;

/// Bevy resource holding the open WorldDb for the current Space.
/// `None` between Space switches; populated by [`open_world_db_on_space_change`].
#[derive(Resource, Default)]
pub struct WorldDbHandle(pub Option<Arc<dyn WorldDb>>);

/// A Space open whose disk work (TOML reconcile, core bake, voxel import) is
/// running on a worker thread. `open_world_db_on_space_change` installs the
/// DB the frame the worker reports back.
///
/// PERF (load time): those three steps ran synchronously inside the open, on
/// the main thread — 22.2 s of `db-reconcile` on Super Station with the
/// window frozen, the bridge self-test timing out behind it, and no way to
/// show progress. Ordering is preserved exactly (nothing reads the DB until
/// it is installed; the loaders are gated on [`world_db_open_settled`]), the
/// work is the same, only the thread changed.
#[derive(Resource, Default)]
pub struct PendingWorldDbOpen(pub Option<PendingOpen>);

/// What the open worker ("eustress-space-open") sends back, in order.
pub enum OpenWorkerMsg {
    /// The DB can be installed: the one-time bake and voxel import are done
    /// (or were already done). `cores` is the capped instance-core count
    /// for the streaming decision, and `prescan` the loader's entry tree,
    /// both taken off the main thread.
    Ready {
        cores: usize,
        prescan: Option<super::file_loader::PreScan>,
    },
    /// The disk → tree reconcile finished. `report` lists what it changed
    /// so the scene already loading can apply it as hot updates.
    Reconciled(ReconcileReport),
}

/// What a disk → tree reconcile changed, by Space-relative path.
#[derive(Default)]
pub struct ReconcileReport {
    pub reconciled: usize,
    /// Files whose disk bytes were newer than the tree's: put into the tree.
    pub put: Vec<String>,
    /// Tree entries whose file is gone from disk: pruned.
    pub removed: Vec<String>,
}

pub struct PendingOpen {
    /// The Space this open belongs to; a switch mid-open abandons it.
    pub root: std::path::PathBuf,
    pub world_db_dir: std::path::PathBuf,
    /// Taken at install; `None` once the DB is the active source.
    pub db: Option<Arc<dyn WorldDb>>,
    /// Worker → main. `Receiver` is `Send` but not `Sync`; a Bevy resource
    /// must be both, so it lives behind a `Mutex` (uncontended: only the
    /// main thread ever polls it).
    pub rx: std::sync::Mutex<std::sync::mpsc::Receiver<OpenWorkerMsg>>,
    /// Keeps the `space-open` watchdog phase open until installation.
    pub _phase: Option<super::load_phase::PhaseGuard>,
}

impl PendingOpen {
    /// The DB is installed; only the reconcile report is still to come.
    pub fn installed(&self) -> bool {
        self.db.is_none()
    }
}

/// Cap the streaming decision counts against (mirrors the file loader's
/// `BIG_SPACE_THRESHOLD + 1` and `ResidencyConfig::big_space_threshold`).
const STREAMING_COUNT_CAP: usize = 100_001;

/// Threads for the reconcile's disk walk: a quarter of the machine, at
/// least two, at most four. It runs behind the loader, so it trades its own
/// speed for the drain's.
fn reconcile_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 4)
        .unwrap_or(2)
        .clamp(2, 4)
}

/// Run condition: true when NO Space open is in flight, so a loader may
/// read the Space through its final source. While an open is pending the
/// `ActiveSpaceSource` still points at the previous (or disk) source and a
/// loader that ran would load the wrong thing.
/// The DB for the current Space is installed as the active source (or there
/// is no DB open in flight). The reconcile may still be running on its
/// worker; its result arrives as hot updates, so nothing waits for it.
pub fn world_db_open_settled(pending: Res<PendingWorldDbOpen>) -> bool {
    pending.0.as_ref().map(|p| p.installed()).unwrap_or(true)
}

/// Bake `tree` entities into Morton `entities` cores, then seed the `voxels`
/// partition from disk chunks. Runs on the open worker, in this order, right
/// after the reconcile (so the tree is current) and before anything reads
/// `count_instance_cores_capped` for the streaming decision.
///
/// `bake_once` is one-time per Space (marker under `.eustress/`) and additive.
/// The voxel import is idempotent: a non-empty partition is an O(1) skip; an
/// empty one with chunk files on disk is seeded; no `voxel_chunks/` dir is a
/// silent no-op (most Spaces). Both had to complete BEFORE the DB handle is
/// installed and they still do: `finish_pending_open` runs only after this.
fn bake_and_import_voxels(space_root: &std::path::Path, db: &dyn WorldDb) {
    super::bake_cores::bake_once(space_root, db);
    import_voxels_if_absent(space_root, db);
}

/// Turn a reconcile report into watcher events for the scene that is
/// already loading: puts as Modified (re-classified to Created downstream
/// when no entity is registered for the path), prunes as Removed.
fn inject_reconcile_changes(
    space_root: &std::path::Path,
    report: &ReconcileReport,
    out: &mut Vec<super::file_watcher::FileChangeEvent>,
) {
    use super::file_watcher::{FileChangeEvent, FileChangeType};
    let mut pushed = 0usize;
    for (rels, kind) in [
        (&report.put, FileChangeType::Modified),
        (&report.removed, FileChangeType::Removed),
    ] {
        for rel in rels {
            let mut path = space_root.to_path_buf();
            for seg in rel.split('/').filter(|s| !s.is_empty()) {
                path.push(seg);
            }
            let Some(file_type) = super::file_loader::FileType::from_path(&path) else {
                continue;
            };
            let service = rel.split('/').next().unwrap_or("").to_string();
            out.push(FileChangeEvent { path, file_type, service, change_type: kind });
            pushed += 1;
        }
    }
    if pushed > 0 {
        info!(
            target: "eustress_engine::world_db",
            put = report.put.len(),
            removed = report.removed.len(),
            "reconcile finished behind the loader — {} change(s) handed to the file-change pipeline as hot updates",
            pushed
        );
    }
}

fn import_voxels_if_absent(space_root: &std::path::Path, db: &dyn WorldDb) {
    if !db.has_voxel_chunks() {
        match eustress_worlddb::import::import_voxel_chunks(db, space_root) {
            Ok(s) if s.chunks_imported > 0 || s.skipped > 0 => {
                info!(
                    target: "eustress_engine::world_db",
                    chunks = s.chunks_imported,
                    bytes = s.bytes_imported,
                    skipped = s.skipped,
                    space = %space_root.display(),
                    "voxel reconcile: seeded Fjall `voxels` partition from \
                     Workspace/Terrain/voxel_chunks on open"
                );
            }
            Ok(_) => {
                // No voxel_chunks dir / empty dir — the common case.
            }
            Err(e) => {
                warn!(
                    target: "eustress_engine::world_db",
                    error = %e,
                    space = %space_root.display(),
                    "voxel reconcile: disk → `voxels` partition import failed; \
                     imported terrain will not render this Space"
                );
            }
        }
    }
}

/// The install tail of a Space open: subscribe, switch the content source to
/// Fjall, run the load-vs-render diagnostic count, expose the DataStore and
/// install the DB into the global funnel. Main thread only.
fn finish_pending_open(
    space_root: &std::path::Path,
    world_db_dir: &std::path::Path,
    db: Arc<dyn WorldDb>,
    handle: &mut WorldDbHandle,
    sub: &mut WorldDbSubscription,
    active_source: &mut super::space_source::ActiveSpaceSource,
    datastore: &mut WorldDataStore,
) {
    let subscription = db.subscribe(Filter::any());
    info!(
        target: "eustress_engine::world_db",
        dir = %world_db_dir.display(),
        "WorldDb opened — Space content source = FJALL"
    );
    *active_source = super::space_source::ActiveSpaceSource(std::sync::Arc::new(
        super::space_source::FjallSource::new(db.clone()),
    ));

    // ── DIAGNOSTIC: prove the load-vs-render pipeline split ──
    // The scene loader now sources from this Fjall tree. The STREAMING render
    // pipeline (StreamingPlugin) does a separate `std::fs` scan of the disk
    // Workspace and never reads this tree. If the tree holds instances that
    // aren't also on disk (e.g. generator wrote direct-to-Fjall), the scene
    // loader "loads" them but the streaming grid never gets them, so the
    // radius gate spawns/renders zero. Count the tree's instance files here so
    // this line and the streaming scan's "loaded N" line sit side-by-side in
    // the log and the divergence is unambiguous.
    match db.iter_tree() {
        Ok(it) => {
            let mut total_files = 0usize;
            let mut instance_files = 0usize;
            for entry in it {
                match entry {
                    Ok((path, _)) => {
                        total_files += 1;
                        if path.ends_with("_instance.toml")
                            || path.ends_with(".part.toml")
                            || path.ends_with(".instance.toml")
                            || path.ends_with(".glb.toml")
                        {
                            instance_files += 1;
                        }
                    }
                    Err(e) => {
                        warn!(
                            target: "eustress_engine::world_db",
                            error = %e,
                            "iter_tree entry error during diagnostic count"
                        );
                    }
                }
            }
            warn!(
                target: "eustress_engine::world_db",
                tree_total_files = total_files,
                tree_instance_files = instance_files,
                space = %space_root.display(),
                "FJALL SOURCE ACTIVE: scene loader reads these from the DB. \
                 The StreamingPlugin render grid does a SEPARATE std::fs \
                 scan of the disk Workspace and will NOT see Fjall-only \
                 instances — compare this count against the streaming \
                 'initial scan loaded N instances' line. A large gap == \
                 the load-but-no-render bug (rendering pipeline is still \
                 disk-fed)."
            );
        }
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "iter_tree failed during diagnostic instance count"
            );
        }
    }
    // Phase 8 (WS-1): expose the Roblox-parity DataStore for this Space to
    // the script bindings. Same Arc as the handle/source so all three view
    // one consistent DB.
    datastore.0 = Some(eustress_worlddb::DataStoreService::new(db.clone()));
    info!(
        target: "eustress_engine::world_db",
        "DataStoreService ready — scripts can now GetDataStore/GetOrderedDataStore"
    );
    // Install the DB into the global funnel handle: from here every
    // `load_instance_definition` / `load_gui_definition` /
    // `write_instance_definition` call site (the ~25 edit/tool/hot-reload
    // sites that only carry an absolute path) reads/writes the binary ECS
    // record in this DB instead of disk TOML.
    super::active_db::set(db.clone(), space_root.to_path_buf());
    handle.0 = Some(db);
    sub.0 = Some(subscription);
    // LOAD-PHASE milestone 2: Fjall keyspace recovery + auto-convert +
    // TOML↔DB reconcile are all complete and the DB is installed as the live
    // funnel/source.
    super::load_phase::mark("db-recovery-complete");
}

/// Live subscription to the WorldDb change-stream. Drained each frame
/// by [`drain_change_stream`] into the public `Events<WorldDbCommit>`.
#[derive(Resource, Default)]
pub struct WorldDbSubscription(pub Option<Subscription>);

/// Phase 8 (WS-1) — the Roblox-parity `DataStoreService` for the
/// current Space, constructed from the same `Arc<dyn WorldDb>` the
/// handle holds. `None` until a WorldDb is open. The Rune/Luau script
/// bindings (next WS-1 step) read this resource so a game script's
/// `DataStoreService:GetDataStore("X")` resolves to the live Fjall
/// `datastore` partition. Cheap to clone (Arc inside).
#[derive(Resource, Default)]
pub struct WorldDataStore(pub Option<eustress_worlddb::DataStoreService>);

/// Latch: the absolute Space path the open/seed decision has already
/// run for. Without this, a failed DB-open or failed seed import
/// leaves `WorldDbHandle == None`, and `open_world_db_on_space_change`
/// (an `Update` system) re-runs the FULL open + 50k-file faithful
/// import EVERY FRAME — a per-frame disk cycle that pegs the engine
/// at single-digit FPS. The decision must run exactly once per Space
/// regardless of outcome; a real Space switch updates this latch.
#[derive(Resource, Default)]
pub struct WorldDbDecision(pub Option<std::path::PathBuf>);

/// Engine-side message carrying a single WorldDb commit. Mirrors
/// [`eustress_worlddb::CommitDelta`] but lives in engine types so
/// downstream Bevy plugins (Telemetry, Loro, Watchman) don't have to
/// link the worlddb crate. (Bevy 0.18 renamed `Event` → `Message`.)
#[derive(Message, Debug, Clone)]
pub struct WorldDbCommit {
    pub tx_id: u64,
    pub byte_size: usize,
    pub changes: Vec<WorldDbEntityChange>,
}

/// Engine-side projection of a single entity change inside a commit.
#[derive(Debug, Clone)]
pub enum WorldDbEntityChange {
    Put {
        entity_bits: u64,
        component_id: u16,
    },
    Removed {
        entity_bits: u64,
        component_id: u16,
    },
    Despawned {
        entity_bits: u64,
    },
}

/// Per-frame mirror budget. Above this, additional Changed<Transform>
/// writes spill to the next frame to keep `apply_commit` cost bounded.
const MIRROR_PER_FRAME_BUDGET: usize = 2_048;

/// Bring the Fjall `tree` partition back in step with the on-disk TOML
/// hierarchy on Space open (non-migrated Spaces only).
///
/// For a non-migrated Space the on-disk `_instance.toml` / `_service.toml`
/// hierarchy is the human source of truth, yet the loader serves the
/// `tree` partition (FjallSource). The tree is seeded from disk on first
/// open and thereafter the file-watcher only reconciles disk→tree for
/// edits made WHILE the engine runs — so closed-engine edits drift. This
/// walks the disk tree and, for every `.toml` whose bytes differ from the
/// tree (or are missing from it), overwrites the tree key and drops the
/// matching `#bin` bincode cache (which `active_db::get_instance` reads
/// before the base key). Only small text files are considered — `.toml`
/// entity defs plus `.rune`/`.luau`/`.soul`/`.md` script sources (a DB-primary
/// `FjallSource` load reads script bodies from the tree). The large GLB/asset
/// bytes the tree also holds are skipped, so this stays cheap. Unchanged
/// files are left alone, so the change-stream and `#bin` caches aren't
/// churned. Mirrors the out-of-band `reseed-space-subtree` bin, run
/// automatically. Returns the number of files reconciled.
fn reconcile_disk_toml_into_tree(space_root: &std::path::Path, db: &dyn WorldDb) -> ReconcileReport {
    // PERF (load time): re-reading every `_instance.toml` on every open is the
    // dominant cost on a large imported Space — Vehicle Simulator's ~161K files
    // are ~57s of pure `std::fs::read` every open, even when nothing changed.
    // mtime-GATE it: persist the last-reconcile wall-clock in
    // `.eustress/last_reconcile`; a file whose mtime is at/older than that was
    // already reconciled, so we SKIP its read. Correctness is preserved — any
    // edit, INCLUDING the closed-engine disk edit this reconcile exists to
    // catch, bumps the file's mtime past the marker and is read+synced. The
    // marker is stamped with the time captured BEFORE the walk, so a file
    // touched during the walk is caught on the next open, never missed. First
    // open (no marker) does the full pass once, then stamps.
    if super::skip_disk_scans() {
        info!(
            target: "eustress_engine::world_db",
            "disk→tree reconcile SKIPPED (EUSTRESS_SKIP_DISK_SCANS) — closed-engine \
             disk edits will not be ingested for this open"
        );
        return ReconcileReport::default();
    }

    let marker = space_root.join(".eustress").join("last_reconcile");
    let last_reconcile: u64 = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // PERF (load time): the serial recursion above was ~23s of pure tree walk
    // after the mtime-gate landed — `std::fs::read_dir` + `entry.metadata()` +
    // (for the few changed files) `std::fs::read` + `db.get_file` byte-compare,
    // all on one thread over ~161K files. Parallelize the EXPENSIVE part — the
    // per-file stat + mtime-gate + read + tree byte-compare — across the
    // `rayon` global pool (already an engine dependency, Cargo.toml:162), then
    // funnel the actual writes back to THIS thread.
    //
    // Two phases keep correctness identical to the serial version:
    //   1. Single-thread directory walk to enumerate candidate `.toml` paths.
    //      Cheap relative to per-file work; keeping it serial sidesteps any
    //      `read_dir` recursion fan-out bookkeeping and preserves the exact
    //      `.`/`world.fjalldb` dir-skip + `.toml`-only filter.
    //   2. `par_iter` the candidates: each worker does the mtime-gate, reads
    //      the file, and byte-compares against the tree via `db.get_file`. The
    //      `WorldDb` trait is `Send + Sync + 'static` (worlddb/src/backend.rs
    //      `pub trait WorldDb: Send + Sync + 'static` + module doc "Reads and
    //      writes are concurrent — the backend serialises internally"), so
    //      `db.get_file(&self, …)` is safe to call concurrently from the pool.
    //      Each worker returns `Some((rel, bytes))` only for a file that
    //      actually differs (or is absent) from the tree.
    //   3. Back on this thread, serially `put_file` + drop the `#bin` cache for
    //      each changed file. In the common case (mtime-gated) this set is tiny
    //      — funneling the writes to one thread keeps the change-stream commit
    //      order deterministic and the `reconciled` count exact, without
    //      relying on concurrent-write semantics.
    //
    // Correctness is preserved exactly: same dir-skip, same mtime-gate, same
    // byte-compare-before-write, same `#bin` delete, same final marker stamp.
    use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

    // Phase 1 — enumerate candidate paths, one directory LEVEL at a time with
    // the level read in parallel.
    //
    // This was a serial stack walk. That is fine at ~161K files and is minutes
    // of blocked main thread at ~1.3M (the Digital Twin shape), because the
    // walk is per-entry syscalls on one core. Breadth-first by level lets rayon
    // read every directory of a level concurrently; correctness is identical
    // since the traversal order of a set-producing walk does not matter.
    //
    // `entry.file_type()` replaces `path.is_dir()`: `read_dir` already carries
    // the type, so this drops one stat syscall PER ENTRY — the single biggest
    // constant-factor win at this scale. It falls back to `is_dir()` only if
    // the type is unavailable (symlink races).
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let mut frontier: Vec<std::path::PathBuf> = vec![space_root.to_path_buf()];
    while !frontier.is_empty() {
        let (subdirs, files): (Vec<Vec<std::path::PathBuf>>, Vec<Vec<std::path::PathBuf>>) =
            frontier
                .par_iter()
                .map(|dir| {
                    let mut subdirs = Vec::new();
                    let mut files = Vec::new();
                    let Ok(read_dir) = std::fs::read_dir(dir) else {
                        return (subdirs, files);
                    };
                    for entry in read_dir.flatten() {
                        let path = entry.path();
                        let is_dir = entry
                            .file_type()
                            .map(|t| t.is_dir())
                            .unwrap_or_else(|_| path.is_dir());
                        if is_dir {
                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            // Skip the database directory + its backups and any
                            // hidden / `.eustress` container dirs — only the
                            // human TOML tree.
                            if name.starts_with('.') || name.starts_with("world.fjalldb") {
                                continue;
                            }
                            subdirs.push(path);
                            continue;
                        }
                        // `.toml` = entity/instance definitions;
                        // `.rune`/`.luau`/`.soul`/`.md` = script sources a
                        // DB-primary (FjallSource) load reads from the tree. All
                        // small text files — the large GLB/image asset bytes the
                        // tree also holds are still skipped.
                        match path.extension().and_then(|e| e.to_str()) {
                            Some("toml" | "rune" | "luau" | "soul" | "md") => files.push(path),
                            _ => {}
                        }
                    }
                    (subdirs, files)
                })
                .unzip();
        candidates.extend(files.into_iter().flatten());
        frontier = subdirs.into_iter().flatten().collect();
    }

    // Phase 1b — pull the tree's key set ONCE.
    //
    // Phase 2 below used to ask `db.has_file(&rel)` per candidate. Each of
    // those is a backend round-trip that serialises internally, so at ~1.3M
    // candidates they were the bottleneck: the process burns ~1.3 cores with
    // flat memory (workers blocked on the same lock) and the window goes
    // "Not Responding". One sequential key scan answers every probe.
    //
    // Keys come back in stored form, so candidates are put through the SAME
    // `normalise_rel` the backend uses when writing. Hand-rolling the
    // separator fix here would risk a mismatch, and a mismatch means every
    // file reads as absent and the whole tree gets re-ingested.
    let tree_keys: std::collections::HashSet<String> = match db.iter_tree_keys() {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "tree key scan failed — falling back to per-file existence probes"
            );
            std::collections::HashSet::new()
        }
    };
    // Distinguishes "scan failed / genuinely empty tree" from "scan worked":
    // on an empty set we must fall back to probing, or a first-open Space
    // (empty tree, every file missing) would look identical to a failed scan.
    let have_key_set = !tree_keys.is_empty();

    // Phase 2 — parallel stat + mtime-gate + read + tree byte-compare. Returns
    // only files whose disk bytes differ from (or are missing in) the tree.
    let changed: Vec<(String, Vec<u8>)> = candidates
        .par_iter()
        .filter_map(|path| {
            let stripped = path.strip_prefix(space_root).ok()?;
            // Normalise through the backend's own function so this key is
            // byte-identical to what `put_file` stored — see the key-set
            // comment in Phase 1b.
            let rel = eustress_worlddb::normalise_rel(&stripped.to_string_lossy());
            // A file MISSING from the tree was never ingested (dropped into a
            // migrated Space, or the reconcile was gated off for it) — always
            // ingest it, no matter how old its mtime. The mtime-gate skips ONLY
            // files ALREADY in the tree: an unchanged in-tree file was
            // reconciled before, so skipping its read keeps a ~161K-file tree
            // open cheap. `has_file` is a key-only existence probe (no value
            // read), so the presence check stays cheap at scale. Without it, an
            // un-ingested file whose mtime predates the marker is skipped
            // forever — the "I dropped SoulScripts in and nothing registers" bug.
            // In-memory set hit when the bulk scan succeeded; the per-file
            // probe only when it did not (or the tree really is empty), which
            // preserves the original behaviour exactly on that path.
            let in_tree = if have_key_set {
                tree_keys.contains(&rel)
            } else {
                db.has_file(&rel).unwrap_or(false)
            };
            if in_tree && last_reconcile != 0 {
                let unchanged = std::fs::metadata(path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() <= last_reconcile)
                    .unwrap_or(false);
                if unchanged {
                    return None;
                }
            }
            let disk_bytes = std::fs::read(path).ok()?;
            // Only write when disk actually differs from the tree (in-tree only;
            // a missing file always falls through to the write below).
            if in_tree {
                if let Ok(Some(tree_bytes)) = db.get_file(&rel) {
                    if tree_bytes == disk_bytes {
                        return None;
                    }
                }
            }
            Some((rel, disk_bytes))
        })
        .collect();

    // Phase 3 — funnel the (few) writes back to a single thread so the change-
    // stream order is deterministic and `reconciled` stays exact.
    let mut reconciled = 0usize;
    let mut put_paths: Vec<String> = Vec::new();
    for (rel, disk_bytes) in &changed {
        if db.put_file(rel, disk_bytes).is_ok() {
            let _ = db.delete_file(&format!("{rel}#bin"));
            reconciled += 1;
            put_paths.push(rel.clone());
        }
    }
    let mut removed_paths: Vec<String> = Vec::new();
    // Phase 4 — PRUNE. Everything above is additive: a .toml that CHANGED or is
    // NEW gets written into the tree, and a .toml that was DELETED on disk was
    // simply never visited, so its tree entry survived forever. That is how a
    // regenerated DataService left 52 stale Column nodes and two copies of a
    // series that no longer existed: the Explorer spawns from the tree, the tree
    // still held them, and deleting the directory could not reach them.
    //
    // Three guards, because a wrong prune is destructive in a way a missed
    // reconcile is not:
    //
    //   1. Only .toml keys are considered. Binary caches, voxel chunks and
    //      instance cores are not disk-backed this way and are never touched.
    //   2. A key is pruned only when its PARENT DIRECTORY EXISTS and the file
    //      within it does not. If the whole directory is gone the subtree is
    //      left alone, so an unmounted drive, a half-finished sync or a Space
    //      opened from the wrong root cannot wipe the tree. That case is
    //      counted and reported instead of acted on.
    //   3. The #bin sibling goes with its .toml, the same pairing the write
    //      path above maintains.
    let mut pruned = 0usize;
    let mut skipped_missing_dir = 0usize;
    if let Ok(keys) = db.iter_tree_keys() {
        let candidates: Vec<String> = keys
            .filter_map(|k| k.ok())
            .filter(|k| k.ends_with(".toml"))
            .collect();
        for rel in candidates {
            let disk = space_root.join(&rel);
            if disk.exists() {
                continue;
            }
            match disk.parent() {
                Some(dir) if !dir.exists() => {
                    skipped_missing_dir += 1;
                }
                _ => {
                    if db.delete_file(&rel).is_ok() {
                        let _ = db.delete_file(&format!("{rel}#bin"));
                        pruned += 1;
                        removed_paths.push(rel);
                    }
                }
            }
        }
    }
    if pruned > 0 || skipped_missing_dir > 0 {
        info!(
            target: "eustress_engine::world_db",
            "disk to tree reconcile pruned {} entries whose file was deleted on disk; left {} alone because the parent directory is missing entirely",
            pruned, skipped_missing_dir
        );
    }

    // Stamp the marker (best-effort) so the next open can mtime-skip unchanged
    // files. A write failure just means the next open does a full pass.
    let _ = std::fs::create_dir_all(space_root.join(".eustress"));
    let _ = std::fs::write(&marker, now_secs.to_string());
    ReconcileReport {
        reconciled: reconciled + pruned,
        put: put_paths,
        removed: removed_paths,
    }
}

/// Open / re-open the WorldDb whenever `SpaceRoot` changes (on
/// startup + on Space switch), then decide the [`ActiveSpaceSource`]:
///
/// 1. Open `<SpaceRoot>/world.fjalldb/`.
/// 2. If the tree partition is **empty** and the disk Space has
///    content → run the faithful importer once (disk → Fjall tree).
/// 3. Install [`FjallSource`] as the active source so the loader
///    sources every subsequent read from Fjall — zero disk reads,
///    ECS+DB primary. On any failure, fall back to [`DiskSource`]
///    (the engine stays bootable; never a hard stop).
pub fn open_world_db_on_space_change(
    space_root: Res<SpaceRoot>,
    mut handle: ResMut<WorldDbHandle>,
    mut sub: ResMut<WorldDbSubscription>,
    mut active_source: ResMut<super::space_source::ActiveSpaceSource>,
    mut decision: ResMut<WorldDbDecision>,
    mut datastore: ResMut<WorldDataStore>,
    mut pending: ResMut<PendingWorldDbOpen>,
    mut injected: ResMut<super::file_watcher::InjectedFileChanges>,
    mut prescan_out: ResMut<super::file_loader::PendingPreScan>,
) {
    // ── An open is in flight: poll it ───────────────────────────────────
    if let Some(p) = pending.0.as_ref() {
        if p.root != space_root.0 {
            // Space switched mid-open. Abandon the pending record (the worker
            // finishes against its own `Arc` and simply drops it) and fall
            // through to start the new Space's open below.
            info!(
                target: "eustress_engine::world_db",
                abandoned = %p.root.display(),
                "Space switched while its open was pending — abandoning"
            );
            pending.0 = None;
        } else {
            // Poll into a local first so the lock guard (and the shared borrow
            // of `pending`) is released before `pending.0` is touched below.
            // `Err(())` = the worker died (a panic inside the reconcile).
            let outcome: Option<Result<OpenWorkerMsg, ()>> = match p.rx.lock() {
                Ok(r) => match r.try_recv() {
                    Ok(msg) => Some(Ok(msg)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(())),
                },
                Err(_) => Some(Err(())),
            };
            let Some(outcome) = outcome else {
                return; // still running
            };
            match outcome {
                Ok(OpenWorkerMsg::Ready { cores, prescan }) => {
                    let p = pending.0.as_mut().expect("checked above");
                    if let Some(db) = p.db.take() {
                        finish_pending_open(
                            &p.root,
                            &p.world_db_dir,
                            db,
                            &mut handle,
                            &mut sub,
                            &mut active_source,
                            &mut datastore,
                        );
                        super::active_db::preseed_count(STREAMING_COUNT_CAP, cores);
                        prescan_out.0 = prescan;
                        // `space-open` completes here; the reconcile keeps
                        // its own `db-reconcile` phase on the worker.
                        p._phase.take();
                    }
                }
                Ok(OpenWorkerMsg::Reconciled(report)) => {
                    let p = pending.0.take().expect("checked above");
                    if let Some(db) = p.db {
                        // Never expected (the worker sends Ready first), but
                        // a report without an install must still install.
                        finish_pending_open(
                            &p.root,
                            &p.world_db_dir,
                            db,
                            &mut handle,
                            &mut sub,
                            &mut active_source,
                            &mut datastore,
                        );
                    }
                    inject_reconcile_changes(&p.root, &report, &mut injected.0);
                }
                Err(()) => {
                    let p = pending.0.take().expect("checked above");
                    if let Some(db) = p.db {
                        // Install the DB anyway: the tree is whatever it was,
                        // which is the state a failed synchronous reconcile
                        // would also have left.
                        warn!(
                            target: "eustress_engine::world_db",
                            space = %p.root.display(),
                            "space-open worker ended without reporting — installing the DB as-is"
                        );
                        finish_pending_open(
                            &p.root,
                            &p.world_db_dir,
                            db,
                            &mut handle,
                            &mut sub,
                            &mut active_source,
                            &mut datastore,
                        );
                    } else {
                        warn!(
                            target: "eustress_engine::world_db",
                            space = %p.root.display(),
                            "space-open worker ended before its reconcile report — closed-engine disk edits, if any, apply on the next open"
                        );
                    }
                }
            }
            return;
        }
    }
    // Run the open/seed decision exactly once per Space path. The
    // latch (not `handle.0.is_some()`) is the guard: a failed open or
    // failed seed leaves the handle None, and keying off the handle
    // would re-run the full 50k-file import every frame — the
    // "disk cycle" 4 FPS footgun. A genuine Space switch changes the
    // path and re-arms the decision.
    if decision.0.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    decision.0 = Some(space_root.0.clone());
    // LOAD-PHASE milestone 1: space-open begins. Stamp the process-global
    // start clock here (once per genuine Space switch, gated by the latch
    // above) so every later milestone — in file_loader / residency — reads
    // elapsed-since-open from the same anchor. Silent unless EUSTRESS_PROFILE.
    super::load_phase::stamp_open_start();
    // Watchdog: guard the synchronous space-open. Unlike the milestone above
    // this is ALWAYS armed — it is what reports the load as unfinished if the
    // main thread never comes back out (the ~1.3M-file reconcile case). RAII,
    // so all four early returns and the normal exit close it; only an actual
    // failure to return leaves it open, which is exactly the condition worth
    // a popup.
    let _open_phase =
        super::load_phase::scope("space-open", space_root.0.display().to_string());
    // M0 (diagnostics): reset the per-load SPAWN-COST accumulators so this
    // Space's decode/arch/spawn breakdown measures from zero. Paired with
    // the `eager-spawn-complete` settle-point emit in file_loader. No-op
    // cost when EUSTRESS_PROFILE is unset (atomics stay zero, never read).
    super::world_db_binary::spawn_cost::reset();
    // Reset the DataStore for this decision; only a fully-successful
    // open re-populates it below. Every error path therefore leaves
    // it None (scripts see "DataStore unavailable", logged loudly).
    datastore.0 = None;
    // Drop any prior Space's DB from the global funnel handle. Only a
    // fully-successful Fjall open re-installs it below; every disk
    // fallback path therefore leaves it cleared, so the loader/tool/
    // writer funnels (`active_db::*`) correctly use disk for a legacy
    // un-converted world and the DB for a converted one.
    super::active_db::clear();
    info!(
        target: "eustress_engine::world_db",
        space = %space_root.0.display(),
        "WorldDb open/seed decision running (DataStore reset pending open)"
    );

    let world_db_dir = space_root.0.join("world.fjalldb");
    if let Err(e) = std::fs::create_dir_all(&world_db_dir) {
        warn!(
            target: "eustress_engine::world_db",
            error = %e,
            dir = %world_db_dir.display(),
            "cannot create world.fjalldb — falling back to disk source"
        );
        handle.0 = None;
        sub.0 = None;
        *active_source = super::space_source::ActiveSpaceSource::disk(space_root.0.clone());
        return;
    }

    // Ensure header.bin exists at the Space root (sibling to
    // world.fjalldb/, services, Workspace). Missing → fresh world.
    if WorldHeader::read(&space_root.0).ok().flatten().is_none() {
        let fresh = WorldHeader::default();
        if let Err(e) = fresh.write(&space_root.0) {
            warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "failed to stamp fresh header.bin"
            );
        }
    }

    match eustress_worlddb::backend::open(&world_db_dir) {
        Ok(db) => {
            // Conversion is AUTOMATIC: opening a Space *is* converting
            // it. If this world is not yet a migrated `.eustress`, do
            // the full verified, reversible, in-process conversion now
            // — additive disk→Fjall import, per-tree byte-verify,
            // reversible relocation of loose service trees into
            // `.eustress/trash/`, then stamp `header.migrated_at`.
            // Idempotent once stamped (O(1) header read thereafter).
            // Returns false only in the catastrophic "fresh empty tree
            // AND import hard-failed" case → fall back to the disk
            // source so the engine still boots.
            if !super::auto_convert::convert_space_if_needed(&space_root.0, db.as_ref()) {
                warn!(
                    target: "eustress_engine::world_db",
                    "auto-convert: DB unusable (empty tree + import failed) — disk source this Space"
                );
                handle.0 = Some(db);
                sub.0 = None;
                *active_source =
                    super::space_source::ActiveSpaceSource::disk(space_root.0.clone());
                return;
            }

            // ── TOML ↔ DB coherence on open ──────────────────────────
            // The loader sources from the Fjall `tree` partition below,
            // but the runtime file-watcher only mirrors disk→tree for
            // edits made WHILE the engine runs. A CLOSED-engine disk edit
            // (external editor, `git checkout`, an offline tool, or simply
            // editing a `_instance.toml` between sessions) was therefore
            // stranded: the stale tree shadowed the new disk bytes, so
            // changes like `anchored`/color/scale silently failed to load
            // (the "anchored loads from the DB, ignores my disk edit" and
            // V-Cell-staleness class of bug). Reconcile the changed `.toml`
            // back into the tree here, BEFORE FjallSource goes live, so the
            // human-editable disk hierarchy and the database always agree
            // on open. Runs for a migrated Space TOO — NOT because disk is
            // authoritative (it is not; the DB is, and it is .gitignore'd but
            // never derived), but because disk is the INGEST surface: Parts
            // and SoulScripts dropped into the Space while the engine was
            // closed have no other way in, and a DB-primary (FjallSource)
            // load would never see them ("I put files in the Space and
            // nothing registers"). Ingest is one-way, disk → DB, on open.
            // Mtime-gated, so an empty/unchanged disk tree on a migrated Space
            // pays ~nothing.
            let migrated = WorldHeader::read(&space_root.0)
                .ok()
                .flatten()
                .map(|h| h.is_migrated())
                .unwrap_or(false);
            let _ = migrated; // reconcile runs for migrated Spaces too — disk is the ingest surface
            // ── Off-thread: reconcile → bake → voxel import ─────────────
            // These three take `&dyn WorldDb` + a path and touch no ECS
            // state, so they run on a worker in the SAME order they ran
            // here. The main thread keeps rendering; `open_world_db_on_
            // space_change` polls `PendingWorldDbOpen` each frame and runs
            // the install tail (below, in `finish_pending_open`) the frame
            // the worker reports. Nothing observes the DB before then: the
            // loaders are gated on `world_db_open_settled`.
            {
                let (tx, rx) = std::sync::mpsc::channel::<OpenWorkerMsg>();
                let root = space_root.0.clone();
                let db_for_worker = db.clone();
                let spawned = std::thread::Builder::new()
                    .name("eustress-space-open".into())
                    .spawn(move || {
                        let db = db_for_worker.as_ref();
                        let reconcile = || {
                            // Guarded separately from `space-open` so the
                            // log names the reconcile, not just "the load".
                            let _phase = super::load_phase::scope(
                                "db-reconcile",
                                format!("scanning disk tree under {}", root.display()),
                            );
                            // On its own small pool: the walk is off the
                            // critical path now, and on the global pool its
                            // 114K stats took every core from the drain it
                            // overlaps (measured 20 s of contention).
                            let pool = rayon::ThreadPoolBuilder::new()
                                .num_threads(reconcile_threads())
                                .thread_name(|i| format!("eustress-reconcile-{i}"))
                                .build();
                            let report = match pool {
                                Ok(pool) => pool.install(|| reconcile_disk_toml_into_tree(&root, db)),
                                Err(_) => reconcile_disk_toml_into_tree(&root, db),
                            };
                            if report.reconciled > 0 {
                                let _ = db.flush();
                                info!(
                                    target: "eustress_engine::world_db",
                                    reconciled = report.reconciled,
                                    space = %root.display(),
                                    "TOML↔DB reconcile: synced changed disk .toml → Fjall tree on open"
                                );
                            }
                            report
                        };
                        let cores = |db: &dyn WorldDb| {
                            db.count_instance_cores_capped(STREAMING_COUNT_CAP).unwrap_or(0)
                        };
                        if super::bake_cores::is_baked(&root) {
                            // Every open after the first: the loader spawns
                            // from the tree, so it starts now; the reconcile
                            // (a stat walk of every file on disk, 7 to 15 s on
                            // Super Station) runs behind it and reports what
                            // changed, which the scene applies as hot updates.
                            import_voxels_if_absent(&root, db);
                            let n = cores(db);
                            let prescan = super::file_loader::prescan_tree(db, &root, n);
                            let _ = tx.send(OpenWorkerMsg::Ready { cores: n, prescan });
                            let _ = tx.send(OpenWorkerMsg::Reconciled(reconcile()));
                        } else {
                            // The one open that bakes: the bake must read the
                            // tree AFTER the disk edits are in it, so the order
                            // stays reconcile → bake → install, and nothing is
                            // loading yet that the report could update.
                            let _report = reconcile();
                            bake_and_import_voxels(&root, db);
                            let n = cores(db);
                            let prescan = super::file_loader::prescan_tree(db, &root, n);
                            let _ = tx.send(OpenWorkerMsg::Ready { cores: n, prescan });
                            let _ = tx.send(OpenWorkerMsg::Reconciled(ReconcileReport::default()));
                        }
                    });
                match spawned {
                    Ok(_) => {
                        // Move the `space-open` guard into the pending record so
                        // the phase stays open until installation.
                        pending.0 = Some(PendingOpen {
                            root: space_root.0.clone(),
                            world_db_dir: world_db_dir.clone(),
                            db: Some(db),
                            rx: std::sync::Mutex::new(rx),
                            _phase: Some(_open_phase),
                        });
                        return;
                    }
                    Err(e) => {
                        // Could not get a worker: do the work here, exactly as
                        // before, rather than leave the Space unopened.
                        warn!(
                            target: "eustress_engine::world_db",
                            error = %e,
                            "space-open worker failed to spawn — reconciling on the main thread"
                        );
                        let _phase = super::load_phase::scope(
                            "db-reconcile",
                            format!("scanning disk tree under {}", space_root.0.display()),
                        );
                        let report = reconcile_disk_toml_into_tree(&space_root.0, db.as_ref());
                        if report.reconciled > 0 {
                            let _ = db.flush();
                        }
                        bake_and_import_voxels(&space_root.0, db.as_ref());
                        finish_pending_open(
                            &space_root.0,
                            &world_db_dir,
                            db,
                            &mut handle,
                            &mut sub,
                            &mut active_source,
                            &mut datastore,
                        );
                        return;
                    }
                }
            }
            // (bake + voxel import run on the open worker, right after the
            // reconcile; see `bake_and_import_voxels`.)
        }
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "WorldDb open failed — falling back to disk source this Space"
            );
            handle.0 = None;
            sub.0 = None;
            *active_source = super::space_source::ActiveSpaceSource::disk(space_root.0.clone());
        }
    }
}

/// Mirror Changed<Transform> writes into WorldDb. Bypassed entirely
/// when the load gate is active — same condition that gates the
/// legacy TOML writer (see file_loader::LoadInProgress).
fn mirror_transform_changes(
    handle: Res<WorldDbHandle>,
    load_in_progress: Res<super::file_loader::LoadInProgress>,
    // Binary-ECS entities are excluded: their canonical store is the
    // Morton-keyed INSTANCE_CORE record (written by
    // `world_db_binary::mirror_binary_ecs_changes`), not the flat-keyed
    // TRANSFORM component — so this mirror would only write a redundant
    // second record for them.
    q: Query<
        (Entity, &Transform),
        (
            Changed<Transform>,
            Without<super::world_db_binary::BinaryEcsInstance>,
        ),
    >,
    // Value-gate (2026-05-21 fix). `Changed<Transform>` flips on ANY
    // deref-mut, including same-value re-writes — e.g. Avian's per-frame
    // transform sync re-writing anchored/static bodies even with physics
    // paused. Measured: ~1200 NO-OP transform commits PER FRAME while
    // idle (a Fjall journal + FPS storm). Persist only when the VALUE
    // actually changed vs. the last commit; the compare is alloc-free
    // (no encode) so the common idle case does ~zero work. This is the
    // "nothing every frame unless data actually changed" rule.
    mut last_written: Local<std::collections::HashMap<Entity, ([f32; 3], [f32; 4], [f32; 3])>>,
) {
    let Some(db) = handle.0.as_ref() else {
        return;
    };
    if load_in_progress.active {
        return;
    }

    let mut commit = Commit::new();
    let mut budget = MIRROR_PER_FRAME_BUDGET;
    for (entity, transform) in q.iter() {
        if budget == 0 {
            break;
        }
        let cur = (
            [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
            [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            [transform.scale.x, transform.scale.y, transform.scale.z],
        );
        // Skip no-op re-writes (value already persisted). This is what
        // kills the change-detection-false-positive storm: an entity
        // whose Transform was deref-mut'd to the SAME value contributes
        // zero work past this point.
        if last_written.get(&entity) == Some(&cur) {
            continue;
        }
        let bytes = encode_transform(transform);
        commit.put_component(
            WdbEntityId(entity.to_bits()),
            ComponentTypeId::TRANSFORM,
            bytes,
        );
        last_written.insert(entity, cur);
        budget -= 1;
    }

    if commit.is_empty() {
        return;
    }
    if let Err(e) = db.apply_commit(commit) {
        warn!(
            target: "eustress_engine::world_db",
            error = %e,
            "Transform mirror commit failed"
        );
    }
}

/// Drain the change-stream into Bevy events. Runs once per frame in
/// `First` so downstream plugins see the events the same frame they
/// were committed.
fn drain_change_stream(
    sub: Res<WorldDbSubscription>,
    mut writer: MessageWriter<WorldDbCommit>,
) {
    let Some(subscription) = sub.0.as_ref() else {
        return;
    };
    while let Some(delta) = subscription.try_recv() {
        let changes = delta
            .changes
            .into_iter()
            .map(|c| match c {
                eustress_worlddb::EntityChange::Put {
                    entity, component, ..
                } => WorldDbEntityChange::Put {
                    entity_bits: entity.0,
                    component_id: component.0,
                },
                eustress_worlddb::EntityChange::Removed { entity, component } => {
                    WorldDbEntityChange::Removed {
                        entity_bits: entity.0,
                        component_id: component.0,
                    }
                }
                eustress_worlddb::EntityChange::Despawned { entity } => {
                    WorldDbEntityChange::Despawned {
                        entity_bits: entity.0,
                    }
                }
            })
            .collect();
        writer.write(WorldDbCommit {
            tx_id: delta.tx_id.0,
            byte_size: delta.byte_size,
            changes,
        });
        let _ = TxId(delta.tx_id.0); // grep anchor — pulls TxId into scope explicitly
    }
}

/// Encode a Bevy `Transform` to a Fjall value via the worlddb rkyv
/// mirror (Phase 4 — replaced the hand-rolled 40-byte layout). The
/// stored bytes are a tagged rkyv archive; the read path is
/// `eustress_worlddb::decode_transform` (validate + deserialize past the
/// tag byte; Fjall buffers are unaligned so a true zero-copy borrow
/// isn't possible, but it still beats the TOML parse it replaced).
pub(crate) fn encode_transform(t: &Transform) -> Vec<u8> {
    let arch = eustress_worlddb::ArchTransform::new(
        [t.translation.x, t.translation.y, t.translation.z],
        [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
        [t.scale.x, t.scale.y, t.scale.z],
    );
    // Encode failure is effectively impossible for a fixed-size POD
    // struct; fall back to an empty vec (skipped by the mirror) rather
    // than panic the frame.
    eustress_worlddb::encode_transform(&arch).unwrap_or_default()
}

/// Inverse of [`encode_transform`]. `None` on malformed input so
/// callers fall back to the TOML/tree read path instead of crashing.
pub(crate) fn decode_transform(bytes: &[u8]) -> Option<Transform> {
    let a = eustress_worlddb::decode_transform(bytes).ok()?;
    Some(Transform {
        translation: Vec3::new(a.t[0], a.t[1], a.t[2]),
        rotation: Quat::from_xyzw(a.r[0], a.r[1], a.r[2], a.r[3]),
        scale: Vec3::new(a.s[0], a.s[1], a.s[2]),
    })
}

/// Bevy plugin entry. Add to your `App` in `engine::main` when the
/// `world-db` feature is enabled.
pub struct WorldDbPlugin;

/// Dual model (2026-05-17): keep the binary Fjall store in lockstep
/// with runtime TOML edits. The single file-watcher broadcasts
/// `FileChanged`; here we import Created/Modified Space files into the
/// Fjall `tree` (so `FjallSource` + the binary store reflect the edit)
/// and drop Removed ones. Writing to Fjall never touches disk, so it
/// can't re-trigger the disk watcher — no hot-reload loop. This is the
/// "if a TOML exists/changes, read it and update the engine" wire.
fn sync_toml_edits_to_fjall(
    mut reader: MessageReader<eustress_common::file_events::FileChanged>,
    handle: Res<WorldDbHandle>,
    space_root: Res<SpaceRoot>,
) {
    use eustress_common::file_events::FileChangeKind;
    let Some(db) = handle.0.as_ref() else {
        return;
    };
    for change in reader.read() {
        let Some(rel) =
            crate::space::space_source::rel_from_root(&space_root.0, &change.path)
        else {
            continue;
        };
        match change.kind {
            FileChangeKind::Created | FileChangeKind::Modified => {
                // read error = transient / mid-write; the watcher's own
                // reload retries, so skip silently here.
                if let Ok(bytes) = std::fs::read(&change.path) {
                    if let Err(e) = db.put_file(&rel, &bytes) {
                        warn!(
                            target: "eustress_engine::world_db",
                            error = %e,
                            rel = %rel,
                            "TOML→Fjall sync: put_file failed"
                        );
                    } else {
                        debug!(
                            target: "eustress_engine::world_db",
                            rel = %rel,
                            "TOML edit synced into Fjall (dual model)"
                        );
                    }
                }
            }
            FileChangeKind::Removed => {
                let _ = db.delete_file(&rel);
            }
        }
    }
}

impl Plugin for WorldDbPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldDbHandle>()
            .init_resource::<WorldDbSubscription>()
            .init_resource::<WorldDbDecision>()
            .init_resource::<WorldDataStore>()
            .init_resource::<PendingWorldDbOpen>()
            .init_resource::<super::file_watcher::InjectedFileChanges>()
            .add_message::<WorldDbCommit>()
            .add_systems(First, drain_change_stream)
            // Open + seed the WorldDb at Startup, BEFORE the loader, so
            // priority services (Workspace, Lighting) are sourced from
            // Fjall too — not just the deferred services. Without this,
            // `load_space_files_system` (Startup) runs while
            // `ActiveSpaceSource` is still the default DiskSource, so
            // the 50k Workspace parts disk-load even on an
            // already-migrated world. The `WorldDbDecision` latch makes
            // the Update copy below a no-op for the same Space path.
            // Startup: kick the open off on frame 0. The initial Space load
            // now runs in Update, gated on `world_db_open_settled`, so no
            // Startup ordering against it is needed (or possible).
            .add_systems(Startup, open_world_db_on_space_change)
            // Update copy handles runtime Space switches (latched per
            // path); `mirror_transform_changes` persists live edits.
            .add_systems(
                Update,
                (open_world_db_on_space_change, mirror_transform_changes).chain(),
            )
            // Dual model: mirror runtime TOML edits into the Fjall tree
            // so the binary store stays in lockstep with hand/IDE edits.
            .add_systems(Update, sync_toml_edits_to_fjall);

        // Binary-ECS arm of the representation router: boot-load the
        // `entities` partition into the ECS + persist live edits back.
        // Self-contained (its own latch + value-gate); a no-op when the
        // partition is empty, so it can't regress a legacy disk Space.
        super::world_db_binary::register(app);

        // Data Platform Recorder (P2): durable batch-flush of sensor samples
        // into the `timeseries` partition. Only when the `data` feature is on
        // (world-db is implied — we're inside WorldDbPlugin).
        #[cfg(feature = "data")]
        app.add_plugins(super::data_recorder::DataRecorderPlugin);
    }
}
