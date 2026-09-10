//! File watcher for hot-reload of Space files
//!
//! Watches for changes to .soul, .glb, and other files in the Space directory
//! and automatically reloads them when modified externally.

use bevy::prelude::*;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use std::path::{Path, PathBuf};
use crossbeam_channel::{unbounded, Receiver};
use std::time::Duration;

use super::file_loader::{FileType, SpaceFileRegistry};

/// How long a burst must go without a new watcher event before we act on
/// it. Anything shorter than the notify debounce (300ms) would let a
/// single logical write land in two batches again.
const BURST_QUIET_PERIOD: Duration = Duration::from_millis(350);

/// Ceiling on how long a burst may keep deferring itself. A writer that
/// paces its files just under the quiet period would otherwise hold the
/// batch open indefinitely; this flushes what we have and starts a fresh
/// window. It also bounds how stale a `RecentlyWrittenFiles` mark can be
/// when its batch finally runs — worst case is this plus the notify
/// debounce, which must stay under that resource's 2-second suppression
/// or the engine's own writes would stop being recognised as its own.
const BURST_MAX_WAIT: Duration = Duration::from_millis(1200);

/// Watcher events accumulated since the last settled flush.
///
/// The whole point of buffering here rather than acting per poll is that
/// `notify_debouncer_full` splits ONE logical write across several poll
/// batches whenever a path's queue straddles its 300ms expiry boundary —
/// so a `Remove`/`Modify` for a path can be delivered a frame or two
/// BEFORE that same path's `Create`. Handling each batch on its own made
/// the engine act on half a story; buffering to quiescence means every
/// event for a path is in hand before any of them is interpreted.
#[derive(Default)]
struct PendingBurst {
    /// Events seen since the last flush, in arrival order.
    events: Vec<FileChangeEvent>,
    /// Arrival of the first buffered event — drives `BURST_MAX_WAIT`.
    first_seen: Option<std::time::Instant>,
    /// Arrival of the most recent event — drives `BURST_QUIET_PERIOD`.
    last_seen: Option<std::time::Instant>,
}

/// Space path the live [`SpaceFileWatcher`] was built for — the latch for
/// [`setup_file_watcher`].
///
/// A Resource rather than a `Local` because that system is registered in BOTH
/// `Startup` and `Update` (the Update copy is what follows a runtime Space
/// switch). Bevy gives every registration its own `Local`, so a `Local` latch
/// would leave the Update copy unarmed on frame 1 and it would rebuild the
/// launch Space's watcher immediately — restarting `created_at` and with it the
/// 5-second grace period that suppresses notify's spurious Modify storm for
/// pre-existing files. `WorldDbDecision` latches its Startup/Update pair the
/// same way.
#[derive(Resource, Default)]
pub struct WatchedSpace(pub Option<PathBuf>);

/// File watcher resource
#[derive(Resource)]
pub struct SpaceFileWatcher {
    /// Debounced watcher
    _watcher: Debouncer<RecommendedWatcher, FileIdMap>,
    /// Channel receiver for file events
    receiver: Receiver<DebounceEventResult>,
    /// Space root path being watched
    space_path: PathBuf,
    /// Timestamp when the watcher was created — used to ignore spurious
    /// Modify events that `notify` fires for pre-existing files on startup.
    created_at: std::time::Instant,
    /// Burst-coalescing buffer. Behind a `Mutex` (not a separate
    /// `Resource`) so the state lives and dies with the watcher itself —
    /// switching Spaces drops the resource and the half-collected burst
    /// with it, which is exactly the desired reset.
    pending: std::sync::Mutex<PendingBurst>,
}

impl SpaceFileWatcher {
    /// Create a new file watcher for the given Space path
    pub fn new(space_path: PathBuf) -> Result<Self, String> {
        let (tx, rx) = unbounded();
        
        // Create debounced watcher (300ms debounce to avoid rapid fire events)
        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            None,
            move |result: DebounceEventResult| {
                if let Err(e) = tx.send(result) {
                    error!("Failed to send file event: {}", e);
                }
            },
        ).map_err(|e| format!("Failed to create file watcher: {}", e))?;
        
        // Watch the Space directory recursively
        debouncer
            .watcher()
            .watch(&space_path, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch directory: {}", e))?;
        
        info!("👁 File watcher started for: {:?}", space_path);
        
        Ok(Self {
            _watcher: debouncer,
            receiver: rx,
            space_path,
            created_at: std::time::Instant::now(),
            pending: std::sync::Mutex::new(PendingBurst::default()),
        })
    }

    /// Drain the watcher channel into the burst buffer and hand back a
    /// batch only once the burst has SETTLED — either `BURST_QUIET_PERIOD`
    /// has passed with no new event, or the batch has been open for
    /// `BURST_MAX_WAIT`. `None` means "still filling, come back next
    /// frame".
    ///
    /// Acting on every poll instead let one logical write be interpreted
    /// from a half-delivered event set: the debouncer emits a path's queue
    /// only up to the first entry younger than its 300ms timeout, so a
    /// `Remove`/`Modify` could reach the engine a frame or two ahead of the
    /// `Create` for the SAME path. Settling first is what makes the
    /// downstream ordering pass (Creates shallow-first, then everything
    /// else) meaningful across a whole burst rather than per 300ms slice.
    pub fn poll_settled_events(&self) -> Option<Vec<FileChangeEvent>> {
        let fresh = self.poll_events();

        // A panic elsewhere must not wedge hot-reload for the rest of the
        // session, so recover a poisoned guard instead of propagating.
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        if !fresh.is_empty() {
            if pending.events.is_empty() {
                pending.first_seen = Some(std::time::Instant::now());
            }
            pending.last_seen = Some(std::time::Instant::now());
            pending.events.extend(fresh);
        }

        if pending.events.is_empty() {
            return None;
        }

        let quiet = pending.last_seen.is_none_or(|t| t.elapsed() >= BURST_QUIET_PERIOD);
        let capped = pending.first_seen.is_none_or(|t| t.elapsed() >= BURST_MAX_WAIT);
        if !quiet && !capped {
            return None;
        }

        pending.first_seen = None;
        pending.last_seen = None;
        Some(std::mem::take(&mut pending.events))
    }

    /// Poll for file events (non-blocking)
    pub fn poll_events(&self) -> Vec<FileChangeEvent> {
        let _start = std::time::Instant::now();
        let mut events = Vec::new();
        let mut raw_event_count = 0;
        
        // Drain all pending events
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                Ok(debounced_events) => {
                    raw_event_count += debounced_events.len();
                    for event in debounced_events {
                        if let Some(change_event) = self.process_event(event.event) {
                            events.push(change_event);
                        }
                    }
                }
                Err(errors) => {
                    for err in errors {
                        error!("File watcher error: {}", err);
                    }
                }
            }
        }
        
        let elapsed = _start.elapsed();
        if raw_event_count > 0 {
            warn!("🔍 File watcher received {} raw events, buffered {} change events in {:.1}ms",
                raw_event_count, events.len(), elapsed.as_secs_f64() * 1000.0);
        }
        
        events
    }
    
    /// Process a raw notify event into a FileChangeEvent
    fn process_event(&self, event: Event) -> Option<FileChangeEvent> {
        // Only care about modify and create events
        let change_type = match event.kind {
            EventKind::Modify(_) => FileChangeType::Modified,
            EventKind::Create(_) => FileChangeType::Created,
            EventKind::Remove(_) => FileChangeType::Removed,
            _ => return None,
        };
        
        // Get the first path (notify can have multiple paths per event)
        let path = event.paths.first()?.clone();

        // Ignore churn OUTSIDE the editable Space content, BEFORE any
        // syscall.
        if is_engine_internal_path(&path) {
            return None;
        }

        // Skip if not a file
        if !path.is_file() && change_type != FileChangeType::Removed {
            return None;
        }
        
        // For Remove events, file type is irrelevant — we just need the path
        // to look up the entity in the registry and despawn it.
        // For other events, determine file type from path/extension.
        let file_type = if change_type == FileChangeType::Removed {
            FileType::Toml // placeholder — not used for removal, just needs a value
        } else {
            FileType::from_path(&path)
                .or_else(|| path.extension().and_then(|e| e.to_str()).and_then(FileType::from_extension))?
        };
        
        // Determine service from path
        let service = self.extract_service_from_path(&path)?;
        
        Some(FileChangeEvent {
            path,
            file_type,
            service,
            change_type,
        })
    }
    
    /// Extract service name from file path
    fn extract_service_from_path(&self, path: &Path) -> Option<String> {
        service_from_space_root(&self.space_path, path)
    }
}

/// True for paths the watcher must never treat as editable Space content.
///
/// The watcher is recursive over the whole Space, which also covers: the
/// binary Fjall DB (`world.fjalldb/` — journals + segments rewritten
/// constantly + compaction) and the autosave git repo (`.git/` —
/// rewritten by `git add -A` every autosave interval). Without this,
/// every autosave tick produced a burst of raw events that the main
/// thread drained with a `path.is_file()` stat EACH — the ~5-second
/// editor stutter. `.eustress/` is sidecar/trash, also not editable
/// content. Cheap path-component scan; no filesystem access.
fn is_engine_internal_path(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("world.fjalldb") | Some(".git") | Some(".eustress")
        )
    })
}

/// Service name for a path — the first path component below the Space
/// root (`Workspace`, `StarterGui`, `Lighting`, …).
fn service_from_space_root(space_path: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(space_path).ok()?;
    let service = relative.components().next()?.as_os_str().to_str()?;
    Some(service.to_string())
}

/// File change event
#[derive(Debug, Clone)]
pub struct FileChangeEvent {
    pub path: PathBuf,
    pub file_type: FileType,
    pub service: String,
    pub change_type: FileChangeType,
}

/// Type of file change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeType {
    Created,
    Modified,
    Removed,
}

/// Resource to track files recently written by the engine (to avoid hot-reload loops)
#[derive(Resource, Default)]
pub struct RecentlyWrittenFiles {
    /// Map of file path to the time it was written
    pub files: std::collections::HashMap<PathBuf, std::time::Instant>,
}

impl RecentlyWrittenFiles {
    /// Mark a file as recently written
    pub fn mark_written(&mut self, path: PathBuf) {
        self.files.insert(path, std::time::Instant::now());
    }
    
    /// Check if a file was recently written (within the last 2 seconds)
    /// Extended window to prevent hot-reload loops when Transform changes trigger writes
    pub fn was_recently_written(&self, path: &Path) -> bool {
        if let Some(time) = self.files.get(path) {
            time.elapsed() < std::time::Duration::from_millis(2000)
        } else {
            false
        }
    }
    
    /// Clean up old entries (older than 2 seconds)
    pub fn cleanup(&mut self) {
        let cutoff = std::time::Duration::from_secs(2);
        self.files.retain(|_, time| time.elapsed() < cutoff);
    }
}

/// System to process file change events and hot-reload
pub fn process_file_changes(
    watcher: Option<Res<SpaceFileWatcher>>,
    mut registry: ResMut<SpaceFileRegistry>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<super::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<super::instance_loader::PrimitiveMeshCache>,
    mut recently_written: ResMut<RecentlyWrittenFiles>,
    space_root: Res<super::SpaceRoot>,
    // Query for entities loaded from files
    file_entities: Query<(Entity, &super::file_loader::LoadedFromFile)>,
    // Parent links + an all-entities probe so the stale-cleanup sweep can
    // tell whether a candidate is a live CHILD of a live parent (e.g. a
    // BillboardGui/TextLabel under a Part). Such children are owned by the
    // ECS hierarchy, not the disk sweep — they must NOT be despawned just
    // because their folder path momentarily fails a filesystem stat.
    // Bundled into one tuple param to stay within Bevy's 16-system-param
    // ceiling (this system is already param-dense).
    ownership_queries: (
        Query<&bevy::prelude::ChildOf>,
        Query<Entity>,
    ),
    // Query for Soul scripts
    mut soul_scripts: Query<&mut crate::soul::SoulScriptData>,
    class_defaults: Option<Res<super::class_defaults::ClassDefaultsRegistry>>,
    mut asset_manager_state: Option<ResMut<crate::ui::slint_ui::AssetManagerState>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    // Outbound broadcast to non-ECS subsystems (streaming spatial grid,
    // plugins, etc.). See `common::file_events` — this is the single
    // notify-driven channel everyone subscribes to; no other notify
    // watcher should exist in the workspace.
    mut file_change_out: MessageWriter<eustress_common::file_events::FileChanged>,
) {
    let _start = std::time::Instant::now();
    let Some(watcher) = watcher else {
        return;
    };

    // Unbundle the ownership-probe queries (tupled to respect the
    // 16-system-param ceiling). Used only by the stale-cleanup sweep below.
    let (child_of_query, alive_entities) = (&ownership_queries.0, &ownership_queries.1);

    // Clean up old entries from recently written files
    recently_written.cleanup();

    // Stale-entity safety net (AMORTIZED). Catches deletions the watcher
    // might have missed (bulk delete, deletion before the watcher was up,
    // directory removal). The OLD version stat'd EVERY file-loaded entity
    // (`path.exists()`) in a SINGLE frame every ~300 frames — at 215+
    // parts that ~5-second main-thread filesystem-stat burst WAS the
    // editor stutter. Now: every ~300 frames we START a sweep, then
    // spread the stats at STALE_SCAN_BATCH per frame over the following
    // frames until the set is covered, then idle. Same total work +
    // cadence, but no frame spikes and the vast majority of frames do
    // zero stale-scan work (the "nothing every frame" rule).
    const STALE_SCAN_BATCH: usize = 32;
    static FRAME_TICK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    // usize::MAX == idle (not sweeping); otherwise the next entity offset.
    static SWEEP_POS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(usize::MAX);
    if FRAME_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 300 == 0 {
        SWEEP_POS.store(0, std::sync::atomic::Ordering::Relaxed); // kick off a sweep
    }
    let pos = SWEEP_POS.load(std::sync::atomic::Ordering::Relaxed);
    if pos != usize::MAX {
        let mut stale: Vec<(Entity, std::path::PathBuf)> = Vec::new();
        let mut scanned = 0usize;
        for (entity, loaded) in file_entities.iter().skip(pos).take(STALE_SCAN_BATCH) {
            scanned += 1;
            if !loaded.path.exists() && !registry.rename_in_progress.contains(&loaded.path) {
                // OWNERSHIP GUARD (2026-05-24): never sweep a live CHILD of a
                // live parent. A MindSpace label is `Part → BillboardGui →
                // TextLabel` in the ECS; moving the Part could leave the
                // BillboardGui folder momentarily un-stat-able (rename race,
                // dual-model Fjall-vs-disk skew, atomic-write window), and the
                // old sweep then despawned the billboard + its label — the
                // user-reported "moving a block deletes the billboard's text
                // label". A genuine on-disk delete still fires a watcher
                // `Remove` event handled by `handle_file_removed` (which
                // despawns regardless of parent); this amortized sweep is only
                // a backstop for MISSED deletes, so skipping owned children is
                // safe. Real parent deletion despawns children with it (Bevy
                // recursive despawn), so they never reach this sweep orphaned.
                if let Ok(child_of) = child_of_query.get(entity) {
                    if alive_entities.contains(child_of.parent()) {
                        continue;
                    }
                }
                stale.push((entity, loaded.path.clone()));
            }
        }
        for (entity, path) in stale {
            info!("🧹 Stale entity cleanup: despawning {:?} (file deleted: {:?})", entity, path);
            commands.entity(entity).despawn();
            registry.unregister_file(&path);
        }
        // Advance through the set; finish (idle) when a short batch shows
        // we reached the end.
        SWEEP_POS.store(
            if scanned < STALE_SCAN_BATCH { usize::MAX } else { pos + STALE_SCAN_BATCH },
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    
    // Act on a whole SETTLED burst, never on a single poll. `notify`'s
    // debouncer expires a path's queue entry-by-entry, so one logical write
    // can be split across consecutive polls — and a `Remove`/`Modify` for a
    // path can reach us a frame or two BEFORE that path's own `Create`.
    // Buffering to quiescence puts every event for a path in the same batch,
    // which is what the ordering pass below then relies on.
    let Some(events) = watcher.poll_settled_events() else {
        return;
    };

    // Drop anything outside the ACTIVE Space root.
    //
    // `setup_file_watcher` re-points the watcher on a Space switch, but a burst
    // collected just before the switch — or an event still queued inside
    // `notify` when the old watcher is dropped — can surface here afterwards.
    // That matters because nothing downstream re-checks provenance:
    // `handle_file_created` resolves the service from the CURRENT `space_root`,
    // so a stale event does not fail, it silently materialises the outgoing
    // Space's instance as a real entity in the Space now open. Cheap guard, and
    // the only place the two paths are both in scope.
    let space_canonical = space_root.0.canonicalize().ok();
    let before = events.len();
    let events: Vec<FileChangeEvent> = events
        .into_iter()
        .filter(|e| {
            if e.path.starts_with(&space_root.0) {
                return true;
            }
            // Fall back to canonical comparison: `notify` may hand back a
            // verbatim (`\\?\`) or symlink-resolved path that does not share a
            // textual prefix with the configured root. (A genuine delete cannot
            // canonicalize — it is already gone — but such a path passes the
            // prefix test above, so only foreign paths reach here.)
            match (&space_canonical, e.path.canonicalize().ok()) {
                (Some(root), Some(p)) => p.starts_with(root),
                _ => false,
            }
        })
        .collect();
    if events.len() != before {
        // Loud enough to diagnose, quiet enough not to spam: this fires once
        // per burst, and only right after a Space switch. If it ever fires
        // steadily, the watcher and `SpaceRoot` have diverged and hot-reload is
        // being suppressed — that is the symptom to search for.
        info!(
            "👁 dropped {}/{} watcher event(s) from outside the active Space {:?}",
            before - events.len(),
            before,
            space_root.0
        );
    }
    if events.is_empty() {
        return;
    }

    // Grace period: ignore Modified events for the first 5 seconds after watcher
    // creation. `notify` fires spurious Modify events for pre-existing files when
    // the watcher starts — those files were already loaded by load_space_files_system.
    let in_grace_period = watcher.created_at.elapsed() < Duration::from_secs(5);

    // Mark asset manager and explorer caches stale so they rescan on next
    // sync. Once per settled burst rather than once per poll — a bulk write
    // used to force the Explorer through a filesystem rescan for every
    // 300ms slice of the same operation.
    if let Some(ref mut ams) = asset_manager_state {
        ams.cache_stale = true;
        ams.dirty = true;
    }
    if let Some(ref mut es) = explorer_state {
        es.explorer_fs_stale = true;
        es.needs_immediate_sync = true;
    }

    let elapsed = _start.elapsed();
    if elapsed.as_millis() > 50 {
        warn!("🐌 process_file_changes took {:.1}ms ({} events)", elapsed.as_secs_f64() * 1000.0, events.len());
    }

    // Coalesce external renames. On most filesystems an `mv Foo Bar`
    // arrives as a Remove + Create pair in the same settled batch —
    // processing them naively would despawn the entity and re-spawn
    // a fresh one, losing any transient ECS state (physics velocity,
    // animation progress, in-flight tool results, etc.). We detect
    // the pair by matching final-filename equality (same basename,
    // different full path) and promote it to an in-place path update.
    let (mut events, renames) = coalesce_renames(events, &registry);
    for (old_ev, new_ev) in renames {
        handle_file_renamed(&old_ev.path, &new_ev.path, &mut registry, &mut commands, &file_entities);
    }

    // PARENT-BEFORE-CHILD ordering for CREATE events (2026-05-24). Copy-paste /
    // duplicate writes a whole folder TREE to disk; the watcher can deliver a
    // child's `_instance.toml` (e.g. `<Part>/Label/_instance.toml`) BEFORE the
    // parent's, so the child's parent-lookup (`registry.get_entity(parent
    // marker)`) returns None and it spawns UNPARENTED — the user-reported
    // "copy-paste didn't bring the children" (the pasted Part's BillboardGui/
    // TextLabel detach). Cold-load never hits this because it loads depth-first
    // parent-first. Depth-ordering the Creates (shallower paths first) makes the
    // parent entity register before its descendants look it up. Non-Create
    // events keep their relative order (stable sort, key 0).
    // CREATE events FIRST (shallow-path-first so parents register before
    // children), THEN Modified/Removed LAST. Ordering Removed/Modified first
    // (the previous `_ => 0`) let a pasted root's atomic-write-replace Remove —
    // which the reroute turns into a Modify that marks the path
    // `recently_written` — run BEFORE the root's Create, so the Create got
    // skipped and NOTHING spawned. Processing Creates first spawns+registers the
    // whole pasted subtree (parents before children); the trailing
    // Modified/Removed then update in place harmlessly.
    events.sort_by_key(|e| match e.change_type {
        FileChangeType::Created => e.path.components().count(),
        _ => usize::MAX,
    });

    for event in events {
        // Skip files that were recently written by the engine (prevents hot-reload loops)
        if recently_written.was_recently_written(&event.path) {
            debug!("Skipping hot-reload for recently written file: {:?}", event.path);
            continue;
        }

        // Skip dot-prefixed engine-internal paths. `.eustress/` holds the
        // trash bin, undo cache, per-folder metadata — none of it is
        // scene state. Without this guard, a delete-then-restore cycle
        // (which trashes files to `.eustress/trash/<name>/`) would
        // hot-load each trashed `_instance.toml` as a fresh workspace
        // entity. Symptom: copy-paste of a parent that had ever held
        // trashed children spawns ghost entries with the trashed names.
        if event.path
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        {
            continue;
        }

        match event.change_type {
            FileChangeType::Modified => {
                // During startup grace period, skip spurious modify events
                if in_grace_period {
                    continue;
                }
                // Mark as recently written BEFORE hot-reload to prevent write-back loop
                // When we hot-reload and insert Transform, it triggers Changed<Transform>,
                // which would trigger write_instance_changes_system. By marking it here,
                // that system will skip writing this file.
                //
                // Only for a path that already backs a live entity, because
                // that loop needs an entity to run through — `handle_file_modified`
                // is a no-op for anything else. Marking an unowned path instead
                // BLOCKS it: the 2-second suppression window is longer than a
                // burst, so a `Create` arriving for the same path afterwards is
                // dropped by the guard at the top of this loop and the file never
                // loads at all.
                if registry.get_entity(&event.path).is_some() {
                    recently_written.mark_written(event.path.clone());
                }

                handle_file_modified(
                    &event,
                    &mut registry,
                    &mut commands,
                    &asset_server,
                    &mut mesh_cache,
                    &file_entities,
                    &mut soul_scripts,
                );
            }
            FileChangeType::Created => {
                // `process_file_changes` is already at the 16-param ceiling, so
                // we don't add a ForwardDecalMaterial ResMut here. A hot-created
                // Decal lands its ForwardDecal in this transient store and
                // renders on the next reload (the watcher-create path is for
                // live external edits — rare for decals). Real decal rendering
                // is driven by the cold-load file-loader path, which holds the
                // live ResMut.
                let mut decal_materials =
                    bevy::asset::Assets::<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>::default();
                handle_file_created(&event, &mut registry, &mut material_registry, &mut mesh_cache, &mut decal_materials, &mut commands, &asset_server, &mut materials, &space_root.0, class_defaults.as_deref());
            }
            FileChangeType::Removed => {
                // Atomic-write replace (every TOML save: temp file →
                // MoveFileEx REPLACE_EXISTING over the destination) surfaces to
                // `notify` as a SAME-path Remove+Create pair, which
                // `coalesce_renames` does NOT fuse (it only fuses
                // DIFFERENT-path rename pairs). The old code despawned on the
                // spurious Remove — recursively killing a Part's
                // BillboardGui/TextLabel children — then the paired Create
                // respawned a CHILDLESS part (entity generation climbed once
                // per move/save). If the path STILL EXISTS it was replaced, not
                // deleted: re-route to the Modify handler so the entity updates
                // IN PLACE (children preserved) AND external-editor edits still
                // hot-reload their new content. A genuine delete leaves the
                // path gone and falls through to the real despawn. Grace-period
                // spurious Removes are left to `handle_file_removed`'s own
                // still-exists guard (it skips despawn there too).
                if !in_grace_period
                    && event.path.exists()
                    && !registry.rename_in_progress.contains(&event.path)
                {
                    // Same rule as the Modify arm: the write-back suppression
                    // is only meaningful for a path we already own. An
                    // atomic-write Remove that lands before its partner Create
                    // (the debouncer can split one path's queue across polls)
                    // would otherwise mark a path we have never loaded and
                    // silently veto its own Create.
                    if registry.get_entity(&event.path).is_some() {
                        recently_written.mark_written(event.path.clone());
                    }
                    handle_file_modified(
                        &event,
                        &mut registry,
                        &mut commands,
                        &asset_server,
                        &mut mesh_cache,
                        &file_entities,
                        &mut soul_scripts,
                    );
                } else {
                    handle_file_removed(&event, &mut registry, &mut commands);
                }
            }
        }

        // Broadcast to any non-ECS subsystem that subscribed to disk
        // changes (streaming spatial grid, plugin hosts, …). Emitted
        // AFTER the engine's own processing so subscribers see a
        // world where the ECS already reflects the change.
        let kind = match event.change_type {
            FileChangeType::Created  => eustress_common::file_events::FileChangeKind::Created,
            FileChangeType::Modified => eustress_common::file_events::FileChangeKind::Modified,
            FileChangeType::Removed  => eustress_common::file_events::FileChangeKind::Removed,
        };
        file_change_out.write(eustress_common::file_events::FileChanged {
            path: event.path.clone(),
            kind,
        });
    }
}

/// Handle file modification (hot-reload)
/// True when every component of the `Transform` is finite (no NaN, no Inf).
/// Used by the TOML hot-reload path to detect mid-write partial parses
/// that would inject a non-finite Position into Avian.
fn raw_transform_is_finite(t: &Transform) -> bool {
    t.translation.is_finite()
        && t.rotation.x.is_finite()
        && t.rotation.y.is_finite()
        && t.rotation.z.is_finite()
        && t.rotation.w.is_finite()
        && t.scale.is_finite()
}

fn handle_file_modified(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
    asset_server: &AssetServer,
    mesh_cache: &mut super::instance_loader::PrimitiveMeshCache,
    file_entities: &Query<(Entity, &super::file_loader::LoadedFromFile)>,
    soul_scripts: &mut Query<&mut crate::soul::SoulScriptData>,
) {
    match event.file_type {
        FileType::Soul | FileType::Rune | FileType::Lua => {
            // Hot-reload script source for every dynamic language. The
            // actual in-memory recompile / re-execute happens in
            // `hot_recompile_dirty_rune_scripts` (Rune) and
            // `hot_reload_dirty_luau_scripts` (Luau); both run in
            // Update and pick up `dirty = true` flags we set here.
            // Doing the work there (not inline) keeps this system free
            // of RuneRuntimeState / LuauRuntimeState / module-registry
            // params and avoids Bevy query-borrow conflicts.
            // Two ways a script entity gets into the registry, and only one of
            // them registers the source file.
            //
            // A bare `foo.rune` sitting in a service is registered under its own
            // path, so the lookup below finds it. A FOLDER-based script -
            // `vcell_cycle_driver/_instance.toml` plus
            // `vcell_cycle_driver/vcell_cycle_driver.rune` - registers only the
            // `_instance.toml`, because that is the entity's definition. The
            // `.rune` beside it was never registered, so this lookup missed and
            // every edit to a folder-based script was silently discarded: the
            // file changed on disk, the ECS kept the source it booted with, and
            // the next play recompiled the stale text. The byte count even
            // matches in the log, so nothing looks wrong.
            //
            // That cost a V-Cell run: a driver edited from a cold-soak servo to
            // a cycling duty kept running the cold servo, reported the cold
            // start complete against a 25 C cell, and stopped the run before it
            // started. Fall back to the sibling `_instance.toml` so a folder
            // script reloads like a bare one.
            let entity = registry.get_entity(&event.path).or_else(|| {
                event.path.parent()
                    .map(|p| p.join("_instance.toml"))
                    .filter(|p| p.exists())
                    .and_then(|p| registry.get_entity(&p))
            });
            if let Some(entity) = entity {
                if let Ok(mut script_data) = soul_scripts.get_mut(entity) {
                    match std::fs::read_to_string(&event.path) {
                        Ok(new_source) => {
                            script_data.source = new_source;
                            script_data.dirty = true;
                            script_data.build_status = crate::soul::SoulBuildStatus::Stale;

                            info!("🔄 Hot-reloaded script source: {:?}", event.path);

                            // SoulScript (AI-assisted) still goes through
                            // the build pipeline. Rune and Luau both skip
                            // it and re-run / recompile directly — those
                            // per-language systems decide based on
                            // `run_context`.
                            match script_data.run_context {
                                crate::soul::SoulRunContext::Rune
                                | crate::soul::SoulRunContext::Luau => { /* direct path */ }
                                _ => {
                                    commands.trigger(crate::soul::TriggerBuildEvent { entity });
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to reload script {:?}: {}", event.path, e);
                        }
                    }
                }
            }
        }
        
        FileType::Gltf => {
            // Hot-reload glTF/GLB model
            if let Some(entity) = registry.get_entity(&event.path) {
                // Find the entity with this file
                for (ent, loaded) in file_entities.iter() {
                    if ent == entity && loaded.path == event.path {
                        // Reload the scene
                        let scene_handle = asset_server.load(format!("{}#Scene0", event.path.display()));
                        commands.entity(entity).insert(WorldAssetRoot(scene_handle));
                        
                        info!("🔄 Hot-reloaded glTF model: {:?}", event.path);
                        break;
                    }
                }
            }
        }
        
        FileType::Toml => {
            // Hot-reload TOML instance file. `_instance.toml` is included
            // so external-editor updates (VS Code, Workshop `update_entity`,
            // any CRUD pathway that writes the folder marker directly)
            // propagate without a restart. Engine-side Properties-panel
            // edits also land here, but they're harmless because the
            // re-deserialize produces the same ECS state we just wrote.
            let path_str = event.path.to_string_lossy();
            if path_str.ends_with(".glb.toml")
                || path_str.ends_with(".part.toml")
                || path_str.ends_with(".model.toml")
                || path_str.ends_with(".instance.toml")
                || path_str.ends_with("_instance.toml")
            {
                if let Some(entity) = registry.get_entity(&event.path) {
                    // Reload the TOML and update ECS components. Editors
                    // commonly write files in two syscalls (truncate,
                    // then content), so the first Modify event can fire
                    // on a half-written file — zero bytes or trailing
                    // garbage. Downgrading the parse/read failures to
                    // debug! avoids spamming the Output panel with
                    // transient errors; the next Modify will land on a
                    // complete file and succeed.
                    match std::fs::read_to_string(&event.path) {
                        Ok(toml_content) => {
                            if toml_content.trim().is_empty() {
                                debug!("Skipping mid-write reload of empty {:?}", event.path);
                            } else {
                                match toml::from_str::<crate::space::instance_loader::InstanceDefinition>(&toml_content) {
                                    Ok(instance_def) => {
                                        // Sanitise BEFORE inserting so a
                                        // mid-write partial parse (which
                                        // can leave numeric fields at
                                        // their `Default::default()`
                                        // produced 0.0 / 0.0 / 0.0 / 0.0
                                        // quaternion — degenerate, would
                                        // panic Avian's
                                        // `assert_components_finite`
                                        // on the next physics step) is
                                        // turned into a benign clamp
                                        // instead of crashing the
                                        // engine. The same sanity
                                        // clamps that `spawn_instance`
                                        // applies at load time apply
                                        // here at reload time too —
                                        // single source of truth lives
                                        // in `instance_loader`.
                                        let raw: Transform = instance_def.transform.into();
                                        let transform = crate::space::instance_loader::sanitize_transform(raw);
                                        if !raw_transform_is_finite(&raw) {
                                            warn!(
                                                "🛡️ {:?}: hot-reload Transform had non-finite fields (pos={:?} rot={:?} scale={:?}) — clamped before insert",
                                                event.path,
                                                raw.translation,
                                                raw.rotation,
                                                raw.scale,
                                            );
                                        }
                                        commands.entity(entity).insert(transform);

                                        // Re-derive BasePart.size from the mesh
                                        // AABB × the new Transform.scale, exactly
                                        // as the spawn path does (it tags every
                                        // mesh part with NeedsMeshSize). Without
                                        // this, a TOML scale edit updated only
                                        // Transform.scale while BasePart.size went
                                        // stale; the legacy TOML write-back
                                        // serialises scale FROM size, so the next
                                        // save reverted the resize on reload
                                        // (user-reported 2026-05-23: the entrance
                                        // arch's columns reverted to their original
                                        // height after a restart). The marker is a
                                        // no-op on non-mesh instances (the
                                        // recompute query requires Mesh3d + BasePart).
                                        commands
                                            .entity(entity)
                                            .insert(crate::space::instance_loader::NeedsMeshSize);

                                        // Gap 4 — mesh hot-swap. Re-resolve the
                                        // Mesh3d handle on a TOML reload so an
                                        // in-place `[asset] mesh = ...` edit swaps
                                        // the live geometry instead of requiring
                                        // delete+recreate. Mirrors spawn_instance's
                                        // custom-mesh path: normalise the joined
                                        // path, build the space:// URL, pin it in
                                        // the resident cache. Primitives and
                                        // missing/Draco meshes are skipped (the part
                                        // keeps its current mesh); the NeedsMeshSize
                                        // insert above re-derives BasePart.size.
                                        if let Some(asset_ref) = instance_def.asset.as_ref() {
                                            let mesh_lc = asset_ref.mesh.to_lowercase();
                                            let fname =
                                                mesh_lc.rsplit('/').next().unwrap_or(&mesh_lc);
                                            let is_primitive = [
                                                "block", "ball", "cylinder", "wedge",
                                                "corner_wedge", "cone",
                                            ]
                                            .iter()
                                            .any(|hint| fname.contains(hint));
                                            if !is_primitive {
                                                let toml_dir = event
                                                    .path
                                                    .parent()
                                                    .unwrap_or_else(|| std::path::Path::new("."));
                                                // Inline `..`/`.` normalisation —
                                                // canonicalize would add the Windows
                                                // \\?\ verbatim prefix and break the
                                                // strip_prefix below.
                                                let joined = toml_dir.join(&asset_ref.mesh);
                                                let mut absolute_mesh_path =
                                                    std::path::PathBuf::new();
                                                for comp in joined.components() {
                                                    match comp {
                                                        std::path::Component::ParentDir => {
                                                            absolute_mesh_path.pop();
                                                        }
                                                        std::path::Component::CurDir => {}
                                                        _ => absolute_mesh_path
                                                            .push(comp.as_os_str()),
                                                    }
                                                }
                                                if absolute_mesh_path.exists()
                                                    && !super::draco_decoder::is_draco_compressed(
                                                        &absolute_mesh_path,
                                                    )
                                                {
                                                    let space_root =
                                                        super::space_asset_source::space_asset_root();
                                                    let relative_mesh_path = absolute_mesh_path
                                                        .strip_prefix(&space_root)
                                                        .map(|p| {
                                                            p.to_string_lossy().replace('\\', "/")
                                                        })
                                                        .unwrap_or_else(|_| {
                                                            absolute_mesh_path
                                                                .to_string_lossy()
                                                                .replace('\\', "/")
                                                        });
                                                    let mesh_url = format!(
                                                        "space://{}#Mesh0/Primitive0",
                                                        relative_mesh_path
                                                    );
                                                    let mesh_handle = mesh_cache
                                                        .get_or_load_custom(asset_server, &mesh_url);
                                                    commands
                                                        .entity(entity)
                                                        .insert(Mesh3d(mesh_handle));
                                                    debug!(
                                                        "[mesh hot-swap] {:?} -> {}",
                                                        event.path, mesh_url
                                                    );
                                                }
                                            }
                                        }

                                        if let Some(ref mat) = instance_def.material {
                                            commands.entity(entity).insert(mat.to_component());
                                        }

                                        if let Some(ref thermo) = instance_def.thermodynamic {
                                            commands.entity(entity).insert(thermo.to_component());
                                        }

                                        if let Some(ref echem) = instance_def.electrochemical {
                                            commands.entity(entity).insert(echem.to_component());
                                        }

                                        // BillboardGui class hot-reload. The
                                        // InstanceDefinition path above only
                                        // re-inserts Transform + material; a
                                        // BillboardGui carries its size /
                                        // z_index / offsets in the `[gui]`
                                        // section, which lives on the
                                        // `BillboardGui` CLASS component, not on
                                        // anything the InstanceDefinition reload
                                        // touches. Without re-inserting the
                                        // class, an in-place `[gui] size` edit
                                        // updated the file but never the live
                                        // quad (built once at spawn from the
                                        // class). Re-load the gui definition and
                                        // re-insert the class so
                                        // `Changed<BillboardGui>` fires
                                        // sync_billboard_class_to_marker →
                                        // sync_billboard_properties, which
                                        // rebuilds the quad scale/canvas/z-bias.
                                        // The class_name guard keeps Part /
                                        // Model / Script instances (which also
                                        // end in `_instance.toml`) out of this
                                        // path. `units_offset` reaches the
                                        // Transform through the same sync chain,
                                        // so it wins over the generic Transform
                                        // insert above (BillboardGui placement
                                        // is `units_offset`, matching cold-load).
                                        if let Ok(gui_def) = super::gui_loader::load_gui_definition(&event.path) {
                                            if gui_def.metadata.class_name == "BillboardGui" {
                                                let mut gui_props = gui_def.gui.clone();
                                                gui_props.size = gui_props.resolved_size();
                                                let bb_class = super::gui_loader::billboard_class_from_props(&gui_props);
                                                if let Ok(mut ec) = commands.get_entity(entity) {
                                                    ec.insert(bb_class);
                                                }
                                                debug!("🔄 Hot-reloaded BillboardGui class: {:?}", event.path);
                                            }
                                        }

                                        debug!("🔄 Hot-reloaded TOML instance: {:?}", event.path);
                                    }
                                    Err(e) => {
                                        debug!("Partial-write parse of {:?} deferred: {}", event.path, e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Partial-write read of {:?} deferred: {}", event.path, e);
                        }
                    }
                }
            }
        }
        
        FileType::Png | FileType::Jpg | FileType::Tga => {
            // Hot-reload texture
            // Bevy's asset server handles this automatically via hot-reload
            info!("🔄 Texture changed (Bevy will auto-reload): {:?}", event.path);
        }
        
        FileType::GuiElement => {
            // Hot-reload GUI element TOML (Frame, TextLabel, TextButton, etc.)
            if let Some(entity) = registry.get_entity(&event.path) {
                match super::gui_loader::load_gui_definition(&event.path) {
                    Ok(gui_def) => {
                        let gui_type = super::gui_loader::gui_class_from_extension(&event.path);
                        let display = super::gui_loader::gui_display_from_props(
                            &gui_def.gui,
                            gui_def.text.as_ref(),
                            gui_type,
                        );
                        // The registry entry can outlive the entity in two
                        // ways: (a) the entity got despawned + replaced
                        // mid-frame (the spawn-replace pattern in the GUI
                        // insert path) and the registry still points at
                        // the stale id, or (b) a parent was despawned and
                        // this child was reaped with it. Either way, a
                        // best-effort insert is correct — guard with
                        // `commands.get_entity` so a stale registry entry
                        // doesn't surface Bevy's generic "Entity despawned"
                        // warning every time the file watcher fires after
                        // a respawn.
                        if let Ok(mut ec) = commands.get_entity(entity) {
                            ec.insert(display);
                        }
                        info!("🔄 Hot-reloaded GUI element: {:?}", event.path);
                    }
                    Err(e) => {
                        error!("Failed to reload GUI element {:?}: {}", event.path, e);
                    }
                }
            }
        }

        _ => {
            debug!("File modified but no hot-reload handler: {:?}", event.path);
        }
    }
}

/// Handle new file creation
fn handle_file_created(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    material_registry: &mut super::material_loader::MaterialRegistry,
    mesh_cache: &mut super::instance_loader::PrimitiveMeshCache,
    decal_materials: &mut Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>,
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    space_root: &std::path::Path,
    class_defaults: Option<&super::class_defaults::ClassDefaultsRegistry>,
) {
    // Check if file type should spawn an entity
    if !event.file_type.spawns_entity_in_service(&event.service) {
        return;
    }
    
    // Check if already loaded
    if registry.is_loaded(&event.path) {
        return;
    }
    
    // Skip files inside leaf entity folders (Script, Part) — e.g. a
    // Script's `.rune` source or a Part's mesh file, which are internal
    // assets of the parent entity, not their own scene entities.
    //
    // EXCEPTION: `_instance.toml` IS the leaf entity's definition (not
    // an internal asset), so a Create event for it must flow through
    // to the Toml branch below. Without this guard-skip, every
    // folder-based entity written at runtime (Workshop create_entity,
    // scripts, manual drag-in) would be silently dropped because its
    // parent's `_instance.toml` matches the "leaf" heuristic (which
    // self-matches since parent.join("_instance.toml") == event.path).
    let is_instance_marker = event.path.file_name()
        .map(|n| n == "_instance.toml")
        .unwrap_or(false);
    if !is_instance_marker {
        if let Some(parent) = event.path.parent() {
            let instance_toml = parent.join("_instance.toml");
            if instance_toml.exists() {
                if let Ok(content) = std::fs::read_to_string(&instance_toml) {
                    if content.contains("\"Script\"") || content.contains("\"SoulScript\"")
                        || content.contains("\"Part\"")
                    {
                        debug!("Skipping {:?} (internal file of leaf entity folder)", event.path);
                        return;
                    }
                }
            }
        }
    }

    info!("➕ New file detected: {:?}", event.path);

    // Load the new file (same logic as initial scan)
    match event.file_type {
        FileType::Gltf => {
            let scene_handle = asset_server.load(format!("{}#Scene0", event.path.display()));
            let name = event.path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            
            let entity = commands.spawn((
                WorldAssetRoot(scene_handle),
                Transform::default(),
                eustress_common::classes::Instance {
                    name: name.clone(),
                    class_name: eustress_common::classes::ClassName::Part,
                    archivable: true,
                    id: 0,
                    ai: false,
                uuid: String::new(),
                },
                eustress_common::default_scene::PartEntityMarker {
                    part_id: name.clone(),
                },
                super::file_loader::LoadedFromFile {
                    path: event.path.clone(),
                    file_type: event.file_type,
                    service: event.service.clone(),
                },
                Name::new(name.clone()),
            )).id();
            
            registry.register(
                event.path.clone(),
                entity,
                super::file_loader::FileMetadata {
                    path: event.path.clone(),
                    file_type: event.file_type,
                    service: event.service.clone(),
                    name,
                    size: 0,
                    modified: std::time::SystemTime::now(),
                    children: Vec::new(),
                },
            );
        }
        
        FileType::Soul | FileType::Rune => {
            match std::fs::read_to_string(&event.path) {
                Ok(source) => {
                    let name = event.path.file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();

                    let entity = commands.spawn((
                        eustress_common::classes::Instance {
                            name: name.clone(),
                            class_name: eustress_common::classes::ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source,
                            dirty: false,
                            ast: None,
                            generated_code: None,
                            build_status: crate::soul::SoulBuildStatus::NotBuilt,
                            errors: Vec::new(),
                            run_context: Default::default(),
                        },
                        super::file_loader::LoadedFromFile {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                        },
                        Name::new(name.clone()),
                    )).id();

                    // Parent to service entity so the Explorer primary path finds it
                    let service_toml = space_root.join(&event.service).join("_service.toml");
                    if let Some(service_entity) = registry.get_entity(&service_toml) {
                        commands.entity(entity).insert(ChildOf(service_entity));
                    }

                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name,
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );
                    info!("➕ Loaded new {} script: {:?}",
                        if event.file_type == FileType::Rune { "Rune" } else { "Soul" },
                        event.path);
                }
                Err(e) => {
                    error!("Failed to read new script {:?}: {}", event.path, e);
                }
            }
        }

        FileType::Toml => {
            // If the entity is already registered (e.g. written by auto-save while loaded),
            // this is a modify not a create — skip spawning a duplicate.
            if registry.is_loaded(&event.path) {
                return;
            }

            // GUI classes (TextLabel, Frame, BillboardGui, …) inside an
            // `_instance.toml` would otherwise route through
            // `instance_loader::spawn_instance` and land in the "non-visual"
            // branch (no `Aabb`, no `Text`, no `GuiElementDisplay`) — the
            // entity exists in the Explorer but renders nothing and the
            // billboard renderer's `collect_subtree` skips it. Peek at
            // the file's class_name and route GUI classes through
            // `gui_loader::spawn_gui_element` instead, which attaches the
            // proper visual scaffolding.
            //
            // BillboardGui itself stays on the instance_loader path
            // because `file_loader.rs` builds it as a 3D quad host with
            // its own custom render pipeline; only its DESCENDANTS
            // (TextLabel/Frame/etc.) need the gui_loader route.
            let gui_class_name = std::fs::read_to_string(&event.path)
                .ok()
                .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                .and_then(|v| {
                    let meta = v.get("metadata").or_else(|| v.get("Metadata"))?;
                    let cn = meta.get("class_name").or_else(|| meta.get("ClassName"))?;
                    cn.as_str().map(|s| s.to_string())
                });
            // BillboardGui hot-create (e.g. trash restore via undo): inline
            // a minimal spawn that mirrors `file_loader.rs`'s BillboardGui
            // branch. We don't bring back child UI elements here — the
            // file watcher will fire separately for each restored
            // descendant `_instance.toml` and the gui-descendant route
            // below picks them up.
            if matches!(gui_class_name.as_deref(), Some("BillboardGui")) {
                let bb_dir = event.path.parent().unwrap_or(event.path.as_path()).to_path_buf();
                let dir_name = bb_dir.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Label")
                    .to_string();

                // The class default fills in for missing fields when the
                // TOML doesn't yet declare them.
                let mut bb_class = eustress_common::classes::BillboardGui::default();
                let bb_max_distance: f32;
                let bb_always_on_top: bool;
                let bb_offset: bevy::math::Vec3;
                let mut bb_tags: Vec<String> = Vec::new();
                if let Ok(gui_def) = super::gui_loader::load_gui_definition(&event.path) {
                    bb_tags = gui_def.tags.clone();
                    let g = &gui_def.gui;
                    bb_class.size = g.resolved_size();
                    bb_class.max_distance = g.max_distance.unwrap_or(bb_class.max_distance);
                    bb_class.always_on_top = g.always_on_top.unwrap_or(bb_class.always_on_top);
                    if let Some(v) = g.units_offset { bb_class.units_offset = v; }
                    bb_max_distance = bb_class.max_distance;
                    bb_always_on_top = bb_class.always_on_top;
                    bb_offset = bevy::math::Vec3::new(
                        bb_class.units_offset[0], bb_class.units_offset[1], bb_class.units_offset[2],
                    );
                } else {
                    bb_max_distance = bb_class.max_distance;
                    bb_always_on_top = bb_class.always_on_top;
                    bb_offset = bevy::math::Vec3::new(
                        bb_class.units_offset[0], bb_class.units_offset[1], bb_class.units_offset[2],
                    );
                }

                // Resolve UDim2 → pixel size for the renderer marker.
                // PIXELS_PER_METER == 50 (defined in billboard_gui.rs);
                // duplicated here so this branch doesn't reach into
                // engine internals.
                let [w_px, h_px] = bb_class.size.to_pixels(50.0, 50.0);

                let marker = eustress_common::gui::billboard_renderer::BillboardGuiMarker {
                    size: [w_px.max(1.0), h_px.max(1.0)],
                    max_distance: bb_max_distance,
                    always_on_top: bb_always_on_top,
                    face_camera: true,
                    visible: true,
                    ..Default::default()
                };

                let entity = commands.spawn((
                    eustress_common::classes::Instance {
                        name: dir_name.clone(),
                        class_name: eustress_common::classes::ClassName::BillboardGui,
                        archivable: true,
                        id: 0,
                        ai: false,
                        uuid: String::new(),
                    },
                    bb_class,
                    marker,
                    super::file_loader::LoadedFromFile {
                        path: bb_dir.clone(),
                        file_type: super::file_loader::FileType::Directory,
                        service: event.service.clone(),
                    },
                    super::instance_loader::InstanceFile {
                        toml_path: event.path.clone(),
                        mesh_path: std::path::PathBuf::new(),
                        name: dir_name.clone(),
                    },
                    Name::new(dir_name.clone()),
                    Transform::from_translation(bb_offset),
                    Visibility::default(),
                )).id();
                // Tag hydration on hot-reload mirrors the cold-load path
                // (file_loader BillboardGui branch) — keeps MCP/ECS in
                // sync after the user edits tags in the TOML.
                if !bb_tags.is_empty() {
                    commands.entity(entity).insert(eustress_common::attributes::Tags(bb_tags));
                }

                // Parent to the containing folder if we can resolve it.
                if let Some(gp) = bb_dir.parent() {
                    let marker_path = gp.join("_instance.toml");
                    if let Some(parent_entity) = registry.get_entity(&marker_path)
                        .or_else(|| registry.get_entity(gp))
                    {
                        commands.entity(entity).insert(ChildOf(parent_entity));
                    }
                }

                registry.register(
                    event.path.clone(),
                    entity,
                    super::file_loader::FileMetadata {
                        path: event.path.clone(),
                        file_type: super::file_loader::FileType::Toml,
                        service: event.service.clone(),
                        name: dir_name,
                        size: 0,
                        modified: std::time::SystemTime::now(),
                        children: Vec::new(),
                    },
                );
                info!("✅ Hot-loaded restored BillboardGui: {:?}", bb_dir);
                return;
            }

            let is_gui_descendant = matches!(
                gui_class_name.as_deref(),
                Some("TextLabel") | Some("TextButton") | Some("TextBox")
                | Some("Frame") | Some("ScrollingFrame")
                | Some("ImageLabel") | Some("ImageButton")
                | Some("ScreenGui") | Some("ViewportFrame")
            );
            if is_gui_descendant {
                match super::gui_loader::load_gui_definition(&event.path) {
                    Ok(gui_def) => {
                        let entity = super::gui_loader::spawn_gui_element(
                            commands,
                            &event.path,
                            &gui_def,
                        );
                        // Parent to enclosing folder if it's a registered
                        // entity (the BillboardGui / Frame / ScreenGui).
                        if let Some(parent_dir) = event.path.parent().and_then(|p| p.parent()) {
                            let parent_marker = parent_dir.join("_instance.toml");
                            if let Some(parent_entity) = registry.get_entity(&parent_marker)
                                .or_else(|| registry.get_entity(parent_dir))
                            {
                                commands.entity(entity).insert(ChildOf(parent_entity));
                            }
                        }
                        let name = event.path.parent()
                            .and_then(|p| p.file_name())
                            .and_then(|s| s.to_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        registry.register(
                            event.path.clone(),
                            entity,
                            super::file_loader::FileMetadata {
                                path: event.path.clone(),
                                file_type: super::file_loader::FileType::GuiElement,
                                service: event.service.clone(),
                                name,
                                size: 0,
                                modified: std::time::SystemTime::now(),
                                children: Vec::new(),
                            },
                        );
                        info!("✅ Hot-loaded new GUI element from _instance.toml: {:?}", event.path);
                        return;
                    }
                    Err(e) => {
                        error!("Failed to hot-load GUI element {:?}: {}", event.path, e);
                        return;
                    }
                }
            }

            // Load .part.toml, .model.toml, .instance.toml files
            match super::instance_loader::load_instance_definition_with_defaults(&event.path, class_defaults) {
                Ok(instance) => {
                    // Capture identity BEFORE `instance` is moved into spawn_instance
                    // (used by the op-log create record below, after registration).
                    let create_uuid = instance
                        .metadata
                        .uuid
                        .as_deref()
                        .and_then(eustress_common::instance_create::uuid_hex_to_bytes);
                    let create_class = instance.metadata.class_name.clone();
                    let entity = super::instance_loader::spawn_instance(
                        commands,
                        asset_server,
                        materials,
                        material_registry,
                        mesh_cache,
                        decal_materials,
                        event.path.clone(),
                        instance,
                    );

                    // Attach `LoadedFromFile` so any system that
                    // identifies the entity's backing file (Explorer
                    // classification, drag-drop reparent, copy/cut)
                    // can find it. Cold-load (file_loader::spawn_file_entry)
                    // already does this externally; the hot-load path
                    // used to skip it — so a Part created at runtime
                    // via the Insert menu / paste / MCP looked
                    // identical in the Explorer but couldn't be
                    // dragged into another folder ("source X has no
                    // LoadedFromFile — cannot move on disk; skipping"
                    // was the user-reported regression).
                    commands.entity(entity).insert(super::file_loader::LoadedFromFile {
                        path: event.path.clone(),
                        file_type: event.file_type,
                        service: event.service.clone(),
                    });

                    let name = event.path.file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    
                    // Parent to containing folder entity or service root.
                    // event.path = .../V1/VCell_Foo/_instance.toml
                    // parent_dir  = .../V1/VCell_Foo/          (the part folder itself)
                    // grandparent = .../V1/                     (the folder that should own it)
                    if let Some(part_folder) = event.path.parent() {
                        if let Some(grandparent_dir) = part_folder.parent() {
                            // Try grandparent as a named folder (registered by path)
                            let grandparent_instance = grandparent_dir.join("_instance.toml");
                            if let Some(parent_entity) = registry.get_entity(&grandparent_instance)
                                .or_else(|| registry.get_entity(grandparent_dir))
                            {
                                commands.entity(entity).insert(ChildOf(parent_entity));
                            } else {
                                // grandparent is the service root itself
                                let service_toml = space_root.join(&event.service).join("_service.toml");
                                if let Some(service_entity) = registry.get_entity(&service_toml) {
                                    commands.entity(entity).insert(ChildOf(service_entity));
                                }
                            }
                        }
                    }

                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name,
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );

                    info!("✅ Loaded new instance file: {:?}", event.path);

                    // Causal op-log (Phase 1, Way 8): a new on-disk `_instance.toml`
                    // is the one-shot create funnel for a disk / MCP `create_entity`
                    // create. Record it exactly once (actor = FileWatcher). Moves,
                    // edits, residency stream-ins and boot/cold-load never reach this
                    // arm (the `is_loaded` guard + atomic-write re-route ensure it).
                    // Skip if the TOML carries no valid uuid — don't mint a phantom
                    // identity. Best-effort; never affects the spawn.
                    if let Some(uuid_bytes) = create_uuid {
                        let rel = event
                            .path
                            .strip_prefix(space_root)
                            .ok()
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_default();
                        super::active_db::record_disk_create(&uuid_bytes, &create_class, &rel, None);
                    } else {
                        warn!(
                            "op-log: new _instance.toml {:?} has no valid uuid — create not recorded",
                            event.path
                        );
                    }
                }
                Err(e) => {
                    error!("Failed to load new instance file {:?}: {}", event.path, e);
                }
            }
        }
        
        FileType::GuiElement => {
            // Hot-load new GUI element TOML (TextLabel, TextButton, Frame, etc.)
            match super::gui_loader::load_gui_definition(&event.path) {
                Ok(gui_def) => {
                    let gui_type = super::gui_loader::gui_class_from_extension(&event.path);
                    let display = super::gui_loader::gui_display_from_props(
                        &gui_def.gui,
                        gui_def.text.as_ref(),
                        gui_type,
                    );
                    let name = if !gui_def.instance.name.is_empty() {
                        gui_def.instance.name.clone()
                    } else {
                        event.path.file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string()
                    };

                    let class_name = super::gui_loader::gui_class_name_from_type(gui_type);

                    let entity = commands.spawn((
                        eustress_common::classes::Instance {
                            name: name.clone(),
                            class_name,
                            archivable: true,
                            id: 0,
                            ai: false,
                uuid: String::new(),
                        },
                        display,
                        // No bevy_ui Node — rendered via GuiElementDisplay
                        // (PERF: see gui_loader::spawn_frame_element note).
                        super::file_loader::LoadedFromFile {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                        },
                        Name::new(name.clone()),
                    )).id();

                    // Parent to containing directory entity if it exists
                    if let Some(parent_dir) = event.path.parent() {
                        let parent_instance = parent_dir.join("_instance.toml");
                        if let Some(parent_entity) = registry.get_entity(&parent_instance) {
                            commands.entity(entity).insert(ChildOf(parent_entity));
                        } else {
                            // Try parent service
                            let service_toml = space_root.join(&event.service).join("_service.toml");
                            if let Some(service_entity) = registry.get_entity(&service_toml) {
                                commands.entity(entity).insert(ChildOf(service_entity));
                            }
                        }
                    }

                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name,
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );
                    info!("➕ Loaded new GUI element: {:?}", event.path);
                }
                Err(e) => {
                    error!("Failed to load new GUI element {:?}: {}", event.path, e);
                }
            }
        }

        FileType::Material => {
            // Hot-load new .mat.toml files into MaterialRegistry
            match super::material_loader::load_material_definition(&event.path) {
                Ok(definition) => {
                    let mat_name = if definition.material.name.is_empty() {
                        super::material_loader::material_name_from_path(&event.path)
                    } else {
                        definition.material.name.clone()
                    };
                    let mat_toml_dir = event.path.parent().unwrap_or(std::path::Path::new("."));
                    let standard_mat = super::material_loader::build_standard_material(
                        &definition,
                        asset_server,
                        mat_toml_dir,
                        space_root,
                    );
                    let handle = materials.add(standard_mat);
                    material_registry.insert(
                        mat_name.clone(),
                        handle,
                        definition.clone(),
                        event.path.clone(),
                    );
                    // Maps attach on first use — a hot-reloaded material drops its
                    // hydrated state so the NEW maps are picked up.
                    material_registry.defer_textures(
                        &mat_name,
                        &definition.textures,
                        mat_toml_dir,
                        space_root,
                    );
                    let entity = super::material_loader::spawn_material_entity(
                        commands,
                        event.path.clone(),
                        &definition,
                    );
                    registry.register(
                        event.path.clone(),
                        entity,
                        super::file_loader::FileMetadata {
                            path: event.path.clone(),
                            file_type: event.file_type,
                            service: event.service.clone(),
                            name: mat_name.clone(),
                            size: 0,
                            modified: std::time::SystemTime::now(),
                            children: Vec::new(),
                        },
                    );
                    info!("🎨 Hot-loaded new material '{}' from {:?}", mat_name, event.path);
                }
                Err(e) => {
                    error!("Failed to load new material {:?}: {}", event.path, e);
                }
            }
        }
        
        _ => {}
    }
}

/// Handle file deletion
/// Scan a poll batch for Remove+Create pairs that look like a rename:
/// same final filename (basename) but different absolute paths. Returns
/// the surviving non-rename events plus the matched pairs.
///
/// The heuristic is intentionally conservative: the two events must
/// appear in the SAME settled batch, so a human-scale delete-then-create
/// seconds apart won't be mistaken for a rename. A matching filename
/// across different paths inside one burst is overwhelmingly a filesystem
/// rename — but "overwhelmingly" is not "always", and a wrong guess here
/// is silent content loss, because fusing CONSUMES the create. Hence the
/// four preconditions below; anything that fails them falls back to plain
/// Remove + Create, which costs a respawn but never loses the file.
fn coalesce_renames(
    events: Vec<FileChangeEvent>,
    registry: &SpaceFileRegistry,
) -> (Vec<FileChangeEvent>, Vec<(FileChangeEvent, FileChangeEvent)>) {
    let mut consumed = vec![false; events.len()];
    let mut renames: Vec<(FileChangeEvent, FileChangeEvent)> = Vec::new();

    // Paths that are CREATED somewhere in this batch. A `Remove` whose path is
    // ALSO `Create`d in the same batch is an atomic-write in-place rewrite
    // (temp file → rename over the SAME path), NOT a `mv` — so it must never be
    // a rename candidate. Left unguarded, the basename match below cross-paired
    // e.g. `Remove(Center/_instance.toml)` with `Create(Edge_09/_instance.toml)`
    // → a FALSE rename that rekeyed Center's entity onto Edge_09's path (the
    // name↔source scramble) AND freed Center's path so its own atomic re-Create
    // spawned a DUPLICATE Center. This covers the same-path case; the
    // unambiguous-partner rule further down covers the other shape of the same
    // hazard, where N entities move at once and every marker is `_instance.toml`.
    let created_paths: std::collections::HashSet<std::path::PathBuf> = events.iter()
        .filter(|e| e.change_type == FileChangeType::Created)
        .map(|e| e.path.clone())
        .collect();

    for i in 0..events.len() {
        if consumed[i] || events[i].change_type != FileChangeType::Removed { continue; }
        // Same-path re-Create in this batch ⇒ atomic-write rewrite, not a `mv`.
        if created_paths.contains(&events[i].path) { continue; }
        // A `mv` leaves the source GONE. A path still on disk was replaced,
        // not moved, so there is nothing to rekey.
        if events[i].path.exists() { continue; }
        // No registered entity at the source ⇒ nothing to preserve, and
        // `handle_file_renamed` would bail — but the create it consumed is
        // gone by then, so the new file never spawns. Leave both events
        // alone and let the normal handlers do their jobs.
        if registry.get_entity(&events[i].path).is_none() { continue; }
        let rm_basename = match events[i].path.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        // Require an UNAMBIGUOUS partner. Every folder entity's marker is
        // named `_instance.toml`, so a batch that carries a bulk write and a
        // move at once offers many equally-good basename matches; picking the
        // first would rekey one entity onto another's path (a name↔source
        // scramble) and swallow that path's create. Two or more candidates
        // means we cannot tell, so we decline to guess.
        let mut partners: Vec<usize> = Vec::new();
        for j in (i + 1)..events.len() {
            if consumed[j] || events[j].change_type != FileChangeType::Created { continue; }
            if events[j].path == events[i].path { continue; }
            if events[j].path.file_name() != Some(rm_basename.as_os_str()) { continue; }
            partners.push(j);
            if partners.len() > 1 { break; }
        }
        match partners.as_slice() {
            [j] => {
                consumed[i] = true;
                consumed[*j] = true;
                renames.push((events[i].clone(), events[*j].clone()));
            }
            [] => {}
            _ => {
                debug!(
                    "🔀 Ambiguous rename candidates for {:?} in this batch — treating as delete + create",
                    events[i].path
                );
            }
        }
    }

    let survivors: Vec<FileChangeEvent> = events.into_iter()
        .enumerate()
        .filter_map(|(i, ev)| if consumed[i] { None } else { Some(ev) })
        .collect();
    (survivors, renames)
}

/// In-place rename: rekey the entity in the registry under its new path
/// and patch any path-bearing components on the entity itself
/// (`LoadedFromFile`, `InstanceFile`) so subsequent writes land on the
/// renamed file. Leaves `Instance.name` alone — if the user wants the
/// entity display name to follow the folder name, editing the
/// `_instance.toml` fires a Modify event that reloads the metadata
/// through the normal path.
fn handle_file_renamed(
    old_path: &Path,
    new_path: &Path,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
    file_entities: &Query<(Entity, &super::file_loader::LoadedFromFile)>,
) {
    let Some(entity) = registry.get_entity(old_path) else {
        // Nothing in the registry at the old path — treat as a plain
        // Create and let the downstream handler spawn a fresh entity.
        debug!("🔀 Rename {:?} → {:?} but old path not registered; skipping rekey", old_path, new_path);
        return;
    };

    // Move the registry entry. `unregister_file` returns `()`, so we
    // snapshot the metadata first, then rekey under the new path.
    let mut meta = registry.file_metadata.get(old_path).cloned()
        .unwrap_or_else(|| super::file_loader::FileMetadata {
            path: new_path.to_path_buf(),
            file_type: super::file_loader::FileType::Toml,
            service: String::new(),
            name: String::new(),
            size: 0,
            modified: std::time::SystemTime::now(),
            children: Vec::new(),
        });
    registry.unregister_file(old_path);
    meta.path = new_path.to_path_buf();
    registry.register(new_path.to_path_buf(), entity, meta);

    // Update `LoadedFromFile.path` on the entity so stale-entity
    // cleanup and any downstream reload logic point to the new file.
    if let Ok((_, loaded)) = file_entities.get(entity) {
        if loaded.path == old_path {
            commands.entity(entity).insert(super::file_loader::LoadedFromFile {
                path: new_path.to_path_buf(),
                file_type: loaded.file_type,
                service: loaded.service.clone(),
            });
        }
    }

    // `InstanceFile` tracks the TOML path used for write-back by the
    // transform persistence system; keep it in sync so edits after a
    // rename land on the new location instead of recreating the old.
    commands.entity(entity).insert(super::instance_loader::InstanceFile {
        toml_path: new_path.to_path_buf(),
        mesh_path: std::path::PathBuf::new(),
        name: new_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string(),
    });

    info!("🔀 Rename {:?} → {:?} (entity preserved)", old_path, new_path);
}

fn handle_file_removed(
    event: &FileChangeEvent,
    registry: &mut SpaceFileRegistry,
    commands: &mut Commands,
) {
    // Skip if this path is being renamed — the delete is expected
    if registry.rename_in_progress.remove(&event.path) {
        info!("➖ File deleted (rename in progress, skipping despawn): {:?}", event.path);
        return;
    }
    // ATOMIC-WRITE ARTIFACT GUARD (2026-05-24). `write_atomic` saves by
    // writing a temp file then `std::fs::rename(tmp, dest)` — on Windows that
    // is `MoveFileEx(REPLACE_EXISTING)`, and replacing an EXISTING destination
    // can surface to `notify` as a Remove(dest)+Create(dest) pair for the SAME
    // path. `coalesce_renames` only fuses DIFFERENT-path rename pairs, so this
    // same-path pair slips through: the old handler despawned the entity on the
    // spurious Remove — recursively killing its children (a Part's
    // BillboardGui/TextLabel) — then the paired Create respawned a CHILDLESS
    // part. That is the user-reported "moving the block loses its label", with
    // the part's entity generation climbing once per move. A path that STILL
    // EXISTS on disk was not actually deleted, so skip the despawn; the paired
    // same-path Create is already a no-op via `handle_file_created`'s
    // `is_loaded` check (we never unregistered). A genuine deletion leaves the
    // path gone and still despawns normally; external editor saves arrive as
    // Modify (handled elsewhere), not Remove, so they're unaffected.
    if event.path.exists() {
        debug!("➖ ignoring Remove — path still present (atomic-write replace artifact): {:?}", event.path);
        return;
    }
    if let Some(entity) = registry.get_entity(&event.path) {
        info!("➖ File deleted, despawning entity: {:?}", event.path);
        commands.entity(entity).despawn();
        registry.unregister_file(&event.path);
    }
}

/// Build the file watcher for the active Space, and REBUILD it whenever the
/// Space changes.
///
/// Registered in both `Startup` and `Update` (latched by [`WatchedSpace`], so
/// non-switch frames cost one path comparison). The Update copy is the one that
/// follows a runtime Space switch: `open_space` re-points `SpaceRoot`, the
/// WorldDb, the `space://` asset root and both registries, but for a long time
/// nothing re-pointed the watcher — so it kept watching the Space the engine
/// launched into, and that Space's writes hot-spawned into whichever Space was
/// open at the time.
pub fn setup_file_watcher(
    mut commands: Commands,
    space_root: Res<super::SpaceRoot>,
    mut watching: ResMut<WatchedSpace>,
) {
    let space_path = space_root.0.clone();

    // Run once per genuine Space path. A Space switch changes the path and
    // re-arms this.
    if watching.0.as_deref() == Some(space_path.as_path()) {
        return;
    }

    if !space_path.exists() {
        warn!("Space path does not exist, file watcher disabled: {:?}", space_path);
        return;
    }

    // Stamp BEFORE the build so a failed registration does not retry every
    // frame (the same reason the WorldDb decision latches up front).
    watching.0 = Some(space_path.clone());

    match SpaceFileWatcher::new(space_path.clone()) {
        Ok(watcher) => {
            // Replacing the resource drops the previous watcher — its notify
            // registration, its channel, and its half-collected burst — which
            // is the reset `PendingBurst` documents.
            commands.insert_resource(watcher);
            info!("✅ File watcher now watching: {:?}", space_path);
        }
        Err(e) => {
            error!("❌ Failed to initialize file watcher: {}", e);
        }
    }
}
