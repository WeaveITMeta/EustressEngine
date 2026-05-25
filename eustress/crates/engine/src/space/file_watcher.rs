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
        })
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
            warn!("🔍 File watcher received {} raw events, processed {} change events in {:.1}ms", 
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
        // syscall. The watcher is recursive over the whole Space, which
        // also covers: the binary Fjall DB (`world.fjalldb/` — journals +
        // segments rewritten constantly + compaction) and the autosave
        // git repo (`.git/` — rewritten by `git add -A` every autosave
        // interval). Without this, every autosave tick produced a burst
        // of raw events that the main thread drained with a
        // `path.is_file()` stat EACH — the ~5-second editor stutter.
        // `.eustress/` is sidecar/trash, also not editable content. Cheap
        // path-component scan; no filesystem access.
        if path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some("world.fjalldb") | Some(".git") | Some(".eustress")
            )
        }) {
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
        // Get relative path from space root
        let relative = path.strip_prefix(&self.space_path).ok()?;
        
        // First component should be the service name
        let service = relative.components().next()?.as_os_str().to_str()?;
        
        Some(service.to_string())
    }
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
    
    let events = watcher.poll_events();

    if !events.is_empty() {
        // Mark asset manager and explorer caches stale so they rescan on next sync
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
    }

    // Coalesce external renames. On most filesystems an `mv Foo Bar`
    // arrives as a Remove + Create pair in the same poll window —
    // processing them naively would despawn the entity and re-spawn
    // a fresh one, losing any transient ECS state (physics velocity,
    // animation progress, in-flight tool results, etc.). We detect
    // the pair by matching final-filename equality (same basename,
    // different full path) and promote it to an in-place path update.
    let (mut events, renames) = coalesce_renames(events);
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

    // Grace period: ignore Modified events for the first 5 seconds after watcher
    // creation. `notify` fires spurious Modify events for pre-existing files when
    // the watcher starts — those files were already loaded by load_space_files_system.
    let in_grace_period = watcher.created_at.elapsed() < Duration::from_secs(5);

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
                recently_written.mark_written(event.path.clone());

                handle_file_modified(
                    &event,
                    &mut registry,
                    &mut commands,
                    &asset_server,
                    &file_entities,
                    &mut soul_scripts,
                );
            }
            FileChangeType::Created => {
                handle_file_created(&event, &mut registry, &mut material_registry, &mut mesh_cache, &mut commands, &asset_server, &mut materials, &space_root.0, class_defaults.as_deref());
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
                    recently_written.mark_written(event.path.clone());
                    handle_file_modified(
                        &event,
                        &mut registry,
                        &mut commands,
                        &asset_server,
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
            if let Some(entity) = registry.get_entity(&event.path) {
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
                        commands.entity(entity).insert(SceneRoot(scene_handle));
                        
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
                                                let bb_class = super::gui_loader::billboard_class_from_props(&gui_def.gui);
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
                SceneRoot(scene_handle),
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

                // Strict UDim2 schema — no legacy fallback for `size` /
                // `position` shapes. The class default fills in for
                // missing fields when the TOML doesn't yet declare
                // them.
                let mut bb_class = eustress_common::classes::BillboardGui::default();
                let bb_max_distance: f32;
                let bb_always_on_top: bool;
                let bb_offset: bevy::math::Vec3;
                let mut bb_tags: Vec<String> = Vec::new();
                if let Ok(gui_def) = super::gui_loader::load_gui_definition(&event.path) {
                    bb_tags = gui_def.tags.clone();
                    let g = &gui_def.gui;
                    bb_class.size = g.size;
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
                    let entity = super::instance_loader::spawn_instance(
                        commands,
                        asset_server,
                        materials,
                        material_registry,
                        mesh_cache,
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
                        Node { display: Display::None, ..default() },
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
/// appear in the SAME poll batch (~one frame), so a human-scale
/// delete-then-create a few hundred milliseconds apart won't be
/// mistaken for a rename. A matching filename across different paths in
/// the same frame is overwhelmingly a filesystem rename.
fn coalesce_renames(
    events: Vec<FileChangeEvent>,
) -> (Vec<FileChangeEvent>, Vec<(FileChangeEvent, FileChangeEvent)>) {
    let mut consumed = vec![false; events.len()];
    let mut renames: Vec<(FileChangeEvent, FileChangeEvent)> = Vec::new();

    // Paths that are CREATED somewhere in this batch. A `Remove` whose path is
    // ALSO `Create`d in the same batch is an atomic-write in-place rewrite
    // (temp file → rename over the SAME path), NOT a `mv` — so it must never be
    // a rename candidate. This is the guard that fixes group-move corruption:
    // every folder entity's marker is named `_instance.toml`, so moving N
    // entities at once emits N Remove + N Create events all sharing that
    // basename. Without this guard the basename match below greedily cross-
    // paired e.g. `Remove(Center/_instance.toml)` with
    // `Create(Edge_09/_instance.toml)` → a FALSE rename that rekeyed Center's
    // entity onto Edge_09's path (the name↔source scramble) AND freed Center's
    // path so its own atomic re-Create spawned a DUPLICATE Center. Only a path
    // that is gone and NOT re-created in the batch is a genuine rename.
    let created_paths: std::collections::HashSet<std::path::PathBuf> = events.iter()
        .filter(|e| e.change_type == FileChangeType::Created)
        .map(|e| e.path.clone())
        .collect();

    for i in 0..events.len() {
        if consumed[i] || events[i].change_type != FileChangeType::Removed { continue; }
        // Same-path re-Create in this batch ⇒ atomic-write rewrite, not a `mv`.
        if created_paths.contains(&events[i].path) { continue; }
        let rm_basename = match events[i].path.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        for j in (i + 1)..events.len() {
            if consumed[j] || events[j].change_type != FileChangeType::Created { continue; }
            if events[j].path == events[i].path { continue; }
            if events[j].path.file_name() == Some(rm_basename.as_os_str()) {
                consumed[i] = true;
                consumed[j] = true;
                renames.push((events[i].clone(), events[j].clone()));
                break;
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

/// Initialize file watcher on startup
pub fn setup_file_watcher(
    mut commands: Commands,
    space_root: Res<super::SpaceRoot>,
) {
    let space_path = space_root.0.clone();
    
    if !space_path.exists() {
        warn!("Space path does not exist, file watcher disabled: {:?}", space_path);
        return;
    }
    
    match SpaceFileWatcher::new(space_path) {
        Ok(watcher) => {
            commands.insert_resource(watcher);
            info!("✅ File watcher initialized");
        }
        Err(e) => {
            error!("❌ Failed to initialize file watcher: {}", e);
        }
    }
}
