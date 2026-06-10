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
//! ## What this plugin does NOT do yet
//!
//! - Read from Fjall at startup. The cold-load path still scans TOML.
//! - Authoritative TOML retirement. Fjall and TOML are dual-writers
//!   for now — once Phase 6 wires the Fjall read path the TOML write
//!   path retires and the Studio "Save" UX becomes "Flush".
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
/// before the base key). Only `.toml` is considered — the large GLB/asset
/// bytes the tree also holds are skipped, so this stays cheap. Unchanged
/// files are left alone, so the change-stream and `#bin` caches aren't
/// churned. Mirrors the out-of-band `reseed-space-subtree` bin, run
/// automatically. Returns the number of files reconciled.
fn reconcile_disk_toml_into_tree(space_root: &std::path::Path, db: &dyn WorldDb) -> usize {
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

    // Phase 1 — enumerate candidate `.toml` paths (serial walk).
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let mut stack = vec![space_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Skip the database directory + its backups and any hidden
                // / `.eustress` container dirs — only the human TOML tree.
                if name.starts_with('.') || name.starts_with("world.fjalldb") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            candidates.push(path);
        }
    }

    // Phase 2 — parallel stat + mtime-gate + read + tree byte-compare. Returns
    // only files whose disk bytes differ from (or are missing in) the tree.
    let changed: Vec<(String, Vec<u8>)> = candidates
        .par_iter()
        .filter_map(|path| {
            // mtime-gate: skip a file unchanged since the last reconcile so we
            // never re-read the ~161K-file tree. On the first open
            // (`last_reconcile == 0`) nothing is skipped.
            if last_reconcile != 0 {
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
            let stripped = path.strip_prefix(space_root).ok()?;
            let rel = stripped.to_string_lossy().replace('\\', "/");
            let disk_bytes = std::fs::read(path).ok()?;
            // Only write when disk actually differs from the tree.
            if let Ok(Some(tree_bytes)) = db.get_file(&rel) {
                if tree_bytes == disk_bytes {
                    return None;
                }
            }
            Some((rel, disk_bytes))
        })
        .collect();

    // Phase 3 — funnel the (few) writes back to a single thread so the change-
    // stream order is deterministic and `reconciled` stays exact.
    let mut reconciled = 0usize;
    for (rel, disk_bytes) in &changed {
        if db.put_file(rel, disk_bytes).is_ok() {
            let _ = db.delete_file(&format!("{rel}#bin"));
            reconciled += 1;
        }
    }
    // Stamp the marker (best-effort) so the next open can mtime-skip unchanged
    // files. A write failure just means the next open does a full pass.
    let _ = std::fs::create_dir_all(space_root.join(".eustress"));
    let _ = std::fs::write(&marker, now_secs.to_string());
    reconciled
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
fn open_world_db_on_space_change(
    space_root: Res<SpaceRoot>,
    mut handle: ResMut<WorldDbHandle>,
    mut sub: ResMut<WorldDbSubscription>,
    mut active_source: ResMut<super::space_source::ActiveSpaceSource>,
    mut decision: ResMut<WorldDbDecision>,
    mut datastore: ResMut<WorldDataStore>,
) {
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
            // on open. Skipped for a migrated Space (no loose disk tree).
            let migrated = WorldHeader::read(&space_root.0)
                .ok()
                .flatten()
                .map(|h| h.is_migrated())
                .unwrap_or(false);
            if !migrated {
                let n = reconcile_disk_toml_into_tree(&space_root.0, db.as_ref());
                if n > 0 {
                    let _ = db.flush();
                    info!(
                        target: "eustress_engine::world_db",
                        reconciled = n,
                        space = %space_root.0.display(),
                        "TOML↔DB reconcile: synced changed disk .toml → Fjall tree on open"
                    );
                }
            }

            // ── Wave 9.C — voxel-chunk reconcile on open ─────────────
            // The Roblox importer writes decoded terrain to
            // `Workspace/Terrain/voxel_chunks/chunk_<cx>_<cy>_<cz>.bin`
            // on DISK, but the runtime terrain loader
            // (`terrain_voxel_load::load_voxel_terrain_on_space_open`)
            // reads ONLY the Fjall `voxels` partition. Nothing else
            // copies disk → partition, and a hook inside the one-shot
            // initial migration would never run for an ALREADY-migrated
            // Space (Vehicle Simulator: `header.migrated_at` set long
            // before terrain import existed). So reconcile here, on
            // EVERY open, for migrated and non-migrated Spaces alike:
            //   - partition non-empty → O(1) probe, skip instantly
            //     (idempotent; never re-seeds over live data);
            //   - partition empty + chunk files on disk → seed it now;
            //   - no `voxel_chunks/` dir → Ok(default), silent no-op
            //     (most Spaces have no imported terrain).
            // This runs synchronously BEFORE `handle.0` is installed
            // below, and the voxel terrain loader can't act until it
            // sees that handle — so on the first open after import the
            // loader is guaranteed to find the partition populated.
            if !db.has_voxel_chunks() {
                match eustress_worlddb::import::import_voxel_chunks(db.as_ref(), &space_root.0) {
                    Ok(s) if s.chunks_imported > 0 || s.skipped > 0 => {
                        info!(
                            target: "eustress_engine::world_db",
                            chunks = s.chunks_imported,
                            bytes = s.bytes_imported,
                            skipped = s.skipped,
                            space = %space_root.0.display(),
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
                            space = %space_root.0.display(),
                            "voxel reconcile: disk → `voxels` partition import failed; \
                             imported terrain will not render this Space"
                        );
                    }
                }
            }

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
            // The scene loader now sources from this Fjall tree. The
            // STREAMING render pipeline (StreamingPlugin) does a
            // separate `std::fs` scan of the disk Workspace and never
            // reads this tree. If the tree holds instances that aren't
            // also on disk (e.g. generator wrote direct-to-Fjall), the
            // scene loader "loads" them but the streaming grid never
            // gets them, so the radius gate spawns/renders zero. Count
            // the tree's instance files here so this line and the
            // streaming scan's "loaded N" line sit side-by-side in the
            // log and the divergence is unambiguous.
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
                        space = %space_root.0.display(),
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
            // Phase 8 (WS-1): expose the Roblox-parity DataStore for
            // this Space to the script bindings. Same Arc as the
            // handle/source so all three view one consistent DB.
            datastore.0 = Some(eustress_worlddb::DataStoreService::new(db.clone()));
            info!(
                target: "eustress_engine::world_db",
                "DataStoreService ready — scripts can now GetDataStore/GetOrderedDataStore"
            );
            // Install the DB into the global funnel handle: from here
            // every `load_instance_definition` / `load_gui_definition`
            // / `write_instance_definition` call site (the ~25 edit/
            // tool/hot-reload sites that only carry an absolute path)
            // reads/writes the binary ECS record in this DB instead of
            // disk TOML — the full conversion, with no per-call-site
            // signature churn.
            super::active_db::set(db.clone(), space_root.0.clone());
            handle.0 = Some(db);
            sub.0 = Some(subscription);
            // LOAD-PHASE milestone 2: Fjall keyspace recovery + auto-convert
            // + TOML↔DB reconcile are all complete and the DB is installed
            // as the live funnel/source. Everything above (backend::open,
            // convert_space_if_needed, reconcile_disk_toml_into_tree) is the
            // ~2.3s recovery+reconcile block the analysis called out.
            super::load_phase::mark("db-recovery-complete");
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
            .add_systems(
                Startup,
                open_world_db_on_space_change
                    .before(crate::space::file_loader::load_space_files_system),
            )
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
    }
}
