//! # Rune in Play / Run mode
//!
//! One system drives a whole frame of Rune script execution:
//! install bridges → `on_init` / `on_ready` / `on_update` → drain effects →
//! tear the bridges down.
//!
//! ## Why one system and not five
//!
//! Every bridge a Rune native function reaches for — the Space root, the
//! spatial-query handle, the `Instance::new` registry, the live-hierarchy
//! snapshot, the sim-value map, the EventBus — is a `thread_local!`. That is
//! forced: `#[rune::function]` fns take no Bevy `SystemParam`, so the only way
//! to hand them engine state is to stash it somewhere ambient before the VM
//! runs.
//!
//! The previous wiring installed those thread-locals in `prepare_script_bindings`
//! and read them from `run_script_init` / `run_script_update` — **different Bevy
//! systems**. `.after()` orders systems; it does not pin them to a thread. On
//! the multi-threaded executor the install and the read routinely landed on
//! different worker threads, so scripts saw empty bridges and every
//! side-effecting call became a silent no-op. Collapsing the sequence into one
//! system makes "installed" and "used" the same thread by construction — the
//! same discipline `drain_slint_actions` already follows for the command bar.
//!
//! ## What was simply never wired
//!
//! Beyond the threading bug, three bridges had **no caller at all**:
//! `set_space_root` (so `read_space_file`, `write_space_file`,
//! `query_workspace_entities` and the whole `part_set_*` family did nothing),
//! `set_spatial_bridge` (so `workspace_raycast` always returned nothing — and
//! `SpatialQueryBridgePlugin` was never even added to the app), and
//! `set_instance_registry` outside the command bar (so `Instance::new()` was
//! dead in play mode). Scripts compiled, ticked, and could log — and that was
//! the whole of it.
//!
//! ## Effects are drained, not applied inline
//!
//! Native Rune functions have no `&mut World`, so they QUEUE: created
//! instances land in the `InstanceRegistry`, `Instance:Destroy()` in
//! `PENDING_DESTROY`, `Instance:Set()` in `PENDING_PROPERTY_WRITES`,
//! `set_sim_value` in `SIM_VALUE_WRITES`, and `eustress::terrain` edits in
//! `rune_terrain`'s queue. This module drains them all after the callbacks
//! return, in the same frame.
//!
//! Instances created during Play are spawned ECS-ONLY — no `_instance.toml` is
//! written. Play-mode state is reverted on Stop, so persisting it would leave
//! debris behind after every play session. (The command bar, which runs in Edit
//! mode as an authoring action, does write TOML — that asymmetry is deliberate.)

use bevy::prelude::*;

use eustress_common::classes::{BasePart, Instance as ClassInstance, Material, Part, PartType};
use eustress_common::soul::rune_runtime::{
    call_script_exit, call_script_init, call_script_ready, call_script_update, RuneRuntimeState,
};

use crate::simulation::plugin::SimValuesResource;
use crate::spatial_query_bridge::ScriptSpatialQuery;
use crate::space::{SpaceFileRegistry, SpaceRoot};
#[cfg(feature = "realism-scripting")]
use crate::space::trash;

#[cfg(feature = "realism-scripting")]
use crate::soul::rune_ecs_module::{
    self, InstanceSnapshotEntry, RuneHierarchySnapshot, RuneTagSnapshot,
};
#[cfg(feature = "realism-scripting")]
use crate::soul::rune_terrain::{self, ScriptTerrain};

// ============================================================================
// Resources
// ============================================================================

/// Marker for an entity a Rune script spawned during this Play session.
/// Despawned on Stop so a play session never leaves geometry behind.
#[derive(Component, Debug, Clone, Copy)]
pub struct RuneSpawned;

/// Per-session state for the Rune play driver.
#[derive(Resource, Default)]
pub struct RunePlayBridges {
    /// The `Instance::new()` backing store for this Play session. Recreated on
    /// every Play so ids restart from 1 and nothing leaks between sessions.
    #[cfg(feature = "realism-scripting")]
    pub registry: Option<std::sync::Arc<std::sync::RwLock<eustress_common::scripting::InstanceRegistry>>>,
    /// Cached live-hierarchy snapshot, rebuilt only when the scene changes.
    #[cfg(feature = "realism-scripting")]
    pub hierarchy: RuneHierarchySnapshot,
    /// Cached `{ tag -> [entity_id, …] }` snapshot, rebuilt alongside it.
    #[cfg(feature = "realism-scripting")]
    pub tags: RuneTagSnapshot,
    /// False until the first snapshot build, so entering Play with a static
    /// scene still seeds one.
    pub snapshot_built: bool,
    /// Classes we've already warned about not materializing during Play, so a
    /// script calling `Instance::new` in `on_update` logs once, not 60×/s.
    pub warned_classes: std::collections::HashSet<String>,
}

// ============================================================================
// Session lifecycle
// ============================================================================

/// Carries out the deletes scripts asked for (`instance_delete`), in the frame
/// after the script ran. Outside Play, the instance moves to the Space trash
/// through the shared helper, as the Delete key does, and its entities go. In
/// Play only the live entities are despawned; no authored file moves.
pub fn drain_script_deletes(
    mut commands: Commands,
    mut registry: Option<ResMut<SpaceFileRegistry>>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    #[cfg(feature = "realism-scripting")]
    {
        for request in rune_ecs_module::drain_script_deletes() {
            match request {
                rune_ecs_module::ScriptDelete::DespawnLive { record } => {
                    // A folder's descendants go with it; a flat file is its own entry.
                    let is_folder_record =
                        record.file_name().and_then(|n| n.to_str()) == Some("_instance.toml");
                    let ancestor = if is_folder_record {
                        record.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| record.clone())
                    } else {
                        record.clone()
                    };
                    let victims: Vec<Entity> = registry
                        .as_deref()
                        .map(|r| r.descendants_of(&ancestor).into_iter().map(|(_, e)| e).collect::<Vec<Entity>>())
                        .unwrap_or_default();
                    for entity in victims {
                        commands.entity(entity).try_despawn();
                    }
                }
                rune_ecs_module::ScriptDelete::Trash { source, record } => {
                    let entity = registry.as_deref().and_then(|r| r.get_entity(&record));
                    let destination = trash::trash_destination(&source);
                    let request = trash::TrashRequest {
                        entity: entity.unwrap_or(Entity::PLACEHOLDER),
                        source: &source,
                        record: &record,
                        destination: &destination,
                    };
                    match trash::trash_instance(&request, &mut commands, registry.as_deref_mut()) {
                        Ok(_) => {
                            if let Some(entity) = entity {
                                commands.entity(entity).try_despawn();
                            }
                        }
                        Err(e) => warn!(
                            "Script delete: could not move {} to the trash, so it was left in place ({:?})",
                            source.display(),
                            e,
                        ),
                    }
                }
            }
        }
    }
    #[cfg(not(feature = "realism-scripting"))]
    {
        let _ = (&mut commands, &mut registry);
    }
}

/// OnEnter(Playing): create this session's `Instance::new()` registry and drop
/// any raycast state left over from a previous session.
pub fn start_rune_session(
    mut bridges: ResMut<RunePlayBridges>,
    spatial: Option<Res<ScriptSpatialQuery>>,
    // Clears the script state that lives on the main thread (see
    // `drive_rune_frame`).
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    #[cfg(feature = "realism-scripting")]
    {
        bridges.registry = Some(std::sync::Arc::new(std::sync::RwLock::new(
            eustress_common::scripting::InstanceRegistry::default(),
        )));
        // A session never starts with a previous one's tags or attributes.
        rune_ecs_module::clear_entity_tags();
        rune_ecs_module::clear_instance_attributes();
    }
    bridges.snapshot_built = false;
    bridges.warned_classes.clear();

    if let Some(spatial) = spatial {
        spatial.clear_raycast_state();
    }
}

/// OnEnter(Editing): call `on_exit()` with the bridges still installed, then
/// tear everything down and despawn script-spawned geometry.
///
/// Replaces the bare `rune_api::run_script_exit`, which called `on_exit` with
/// no bridges installed — so a script's cleanup (`log_error`, `set_sim_value`,
/// `Instance:Destroy()`) ran into empty thread-locals and did nothing.
pub fn stop_rune_session(
    mut commands: Commands,
    runtime: Res<RuneRuntimeState>,
    mut bridges: ResMut<RunePlayBridges>,
    space_root: Option<Res<SpaceRoot>>,
    spatial: Option<Res<ScriptSpatialQuery>>,
    spawned: Query<Entity, With<RuneSpawned>>,
    // `on_exit` runs the VM, so this shares the main thread with the frame
    // driver (see `drive_rune_frame`).
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    #[cfg(feature = "realism-scripting")]
    {
        // `on_exit` is the session's last Rune work, timed for the script
        // profile as driver time, like a frame of `drive_rune_frame`.
        let profile_start = eustress_common::utils::script_profile::armed().then(std::time::Instant::now);
        let _profile_scope = eustress_common::utils::script_profile::scope();
        install_bridges(&bridges, space_root.as_deref(), spatial.as_deref(), None, None);
        call_script_exit(&runtime);
        // A script's `on_exit` may still queue destroys; honour them before
        // the world restore runs.
        for id in rune_ecs_module::drain_pending_destroys() {
            let entity = Entity::from_bits(id as u64);
            if let Ok(mut e) = commands.get_entity(entity) {
                e.despawn();
            }
        }
        let _ = rune_ecs_module::drain_pending_property_writes();
        let _ = rune_ecs_module::drain_script_sim_writes();
        // Left on this thread they would reach the next session's first frame.
        let _ = rune_ecs_module::drain_physics_commands();
        rune_ecs_module::clear_entity_tags();
        rune_ecs_module::clear_instance_attributes();
        // Stop restores the authored world. A delete that on_exit queued must not
        // reach the Edit world, and the next session starts with no Play deletes.
        rune_ecs_module::discard_play_script_deletes();
        clear_bridges();
        bridges.registry = None;
        bridges.snapshot_built = false;
        if let Some(start) = profile_start {
            eustress_common::utils::script_profile::record_driver("rune", start.elapsed());
        }
    }
    #[cfg(not(feature = "realism-scripting"))]
    {
        call_script_exit(&runtime);
        let _ = (&space_root, &mut bridges);
    }

    if let Some(spatial) = spatial {
        spatial.clear_raycast_state();
    }

    let mut removed = 0usize;
    for entity in spawned.iter() {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.despawn();
            removed += 1;
        }
    }
    if removed > 0 {
        info!("🧹 Despawned {removed} Rune-spawned entit(ies) on Stop");
    }
}

// ============================================================================
// Scene snapshot cache
// ============================================================================

/// Rebuild the live-hierarchy + tag snapshots the `Instance` handle API and
/// `CollectionService::GetTagged` resolve against.
///
/// Only rebuilds when the scene actually changed. The command bar can afford a
/// full rebuild per invocation; a 60 Hz play loop cannot — a full walk of a
/// 100K-entity Space allocates two Strings per entity, which would dominate the
/// frame. Physics moving a part touches `Transform`, not `Instance`, so a
/// running simulation normally pays one empty change-query check per frame.
#[cfg(feature = "realism-scripting")]
pub fn refresh_rune_scene_snapshot(
    mut bridges: ResMut<RunePlayBridges>,
    runtime: Res<RuneRuntimeState>,
    changed: Query<
        Entity,
        Or<(
            Changed<ClassInstance>,
            Changed<ChildOf>,
            Changed<eustress_common::attributes::Tags>,
        )>,
    >,
    mut removed: RemovedComponents<ClassInstance>,
    all: Query<(
        Entity,
        &ClassInstance,
        Option<&ChildOf>,
        Option<&eustress_common::attributes::Tags>,
    )>,
) {
    // Nothing compiled → nothing will read the snapshot.
    if runtime.compiled.is_empty() {
        return;
    }
    let dirty = !changed.is_empty() || removed.read().next().is_some();
    if bridges.snapshot_built && !dirty {
        return;
    }

    let mut entries: Vec<InstanceSnapshotEntry> = Vec::new();
    let mut tag_map: std::collections::HashMap<String, Vec<i64>> = std::collections::HashMap::new();
    for (entity, inst, parent, tags) in all.iter() {
        entries.push(InstanceSnapshotEntry {
            entity,
            name: inst.name.clone(),
            class_name: inst.class_name.as_str().to_string(),
            parent: parent.map(|c| c.0),
        });
        if let Some(tags) = tags {
            let id = entity.to_bits() as i64;
            for tag in tags.0.iter() {
                tag_map.entry(tag.clone()).or_default().push(id);
            }
        }
    }

    bridges.hierarchy = rune_ecs_module::build_instance_snapshot(entries);
    bridges.tags = RuneTagSnapshot::new(tag_map);
    bridges.snapshot_built = true;
}

/// No-op snapshot refresh for builds without the scripting feature.
#[cfg(not(feature = "realism-scripting"))]
pub fn refresh_rune_scene_snapshot() {}

// ============================================================================
// Bridge install / teardown
// ============================================================================

#[cfg(feature = "realism-scripting")]
fn install_bridges(
    bridges: &RunePlayBridges,
    space_root: Option<&SpaceRoot>,
    spatial: Option<&ScriptSpatialQuery>,
    ecs_bindings: Option<&crate::ui::rune_ecs_bindings::ECSBindings>,
    sim_seed: Option<&std::collections::HashMap<String, f64>>,
) {
    if let Some(sr) = space_root {
        rune_ecs_module::set_space_root(sr.0.clone());
    }
    if let Some(spatial) = spatial {
        rune_ecs_module::set_spatial_bridge(spatial.clone());
    }
    if let Some(registry) = bridges.registry.as_ref() {
        rune_ecs_module::set_instance_registry(registry.clone());
    }
    rune_ecs_module::seed_instance_snapshot_shared(&bridges.hierarchy);
    rune_ecs_module::seed_existing_tags_shared(&bridges.tags);
    // Purchase prompts count only from a Play frame, never from the editor's
    // analyzer running `on_update` on its own.
    rune_ecs_module::set_rune_play_frame(true);

    if let Some(bindings) = ecs_bindings {
        rune_ecs_module::set_ecs_bindings(bindings.clone());
        if let Ok(sim) = bindings.simulation.read() {
            rune_ecs_module::SIM_VALUES.with(|sv| {
                let mut sv = sv.borrow_mut();
                for (k, v) in sim.iter() {
                    sv.insert(k.clone(), *v);
                }
            });
        }
    }

    // Seed from `SimValuesResource` LAST so it wins: it is the cross-thread
    // map every publisher (electrochemistry, MCP sim-commands, earlier script
    // frames) writes into, and is therefore the freshest view. Without this
    // `get_sim_value` would only ever see the battery aggregates that
    // `ECSBindings` happens to compute.
    if let Some(seed) = sim_seed {
        rune_ecs_module::SIM_VALUES.with(|sv| {
            let mut sv = sv.borrow_mut();
            for (k, v) in seed.iter() {
                sv.insert(k.clone(), *v);
            }
        });
    }
}

/// Install the script bridges, run `f`, then tear them down — for callers
/// outside [`drive_rune_frame`] that build their own VM (the ScreenGui
/// button-click dispatcher). Install and use MUST happen inside one Bevy
/// system; this scopes that correctly.
#[cfg(feature = "realism-scripting")]
pub fn with_bridges_installed<R>(
    bridges: &RunePlayBridges,
    space_root: Option<&SpaceRoot>,
    spatial: Option<&ScriptSpatialQuery>,
    ecs_bindings: Option<&crate::ui::rune_ecs_bindings::ECSBindings>,
    sim_seed: Option<&std::collections::HashMap<String, f64>>,
    f: impl FnOnce() -> R,
) -> R {
    install_bridges(bridges, space_root, spatial, ecs_bindings, sim_seed);
    crate::spatial_query_bridge::reset_raycast_slots();
    let out = f();
    clear_bridges();
    out
}

#[cfg(feature = "realism-scripting")]
fn clear_bridges() {
    rune_ecs_module::clear_space_root();
    rune_ecs_module::clear_spatial_bridge();
    rune_ecs_module::clear_instance_registry();
    rune_ecs_module::clear_instance_snapshot();
    rune_ecs_module::clear_existing_tags();
    rune_ecs_module::clear_ecs_bindings();
    rune_ecs_module::clear_physics_state();
    eustress_common::events::clear_event_bus_for_rune();
    rune_terrain::clear_terrain_bridge();
    rune_ecs_module::set_rune_play_frame(false);
}

// ============================================================================
// The per-frame driver
// ============================================================================

/// What the Rune frame driver's script calls cost over a window of frames,
/// kept only while profiling is armed (`EUSTRESS_PROFILE`; the window is
/// `EUSTRESS_PROFILE_FRAMES`). The driver runs on the main thread, so one
/// thread-local is exact. The per-system profile needs a `profiling` build;
/// this reports what the VM costs on any build.
#[cfg(feature = "realism-scripting")]
#[derive(Default)]
struct RuneFrameClock {
    total: std::time::Duration,
    worst: std::time::Duration,
    frames: u64,
}

#[cfg(feature = "realism-scripting")]
thread_local! {
    static RUNE_FRAME_CLOCK: std::cell::RefCell<RuneFrameClock> =
        std::cell::RefCell::new(RuneFrameClock::default());
}

/// Add one frame's script time to the window, and log it when the window is full.
#[cfg(feature = "realism-scripting")]
fn record_rune_frame(elapsed: std::time::Duration, scripts: usize) {
    RUNE_FRAME_CLOCK.with(|cell| {
        let mut clock = cell.borrow_mut();
        clock.total += elapsed;
        clock.worst = clock.worst.max(elapsed);
        clock.frames += 1;
        if clock.frames < crate::profiler::phase_window() {
            return;
        }
        info!(
            "Rune frame driver: {:.3} ms per frame (worst {:.3} ms) over {} frame(s), {} script(s)",
            clock.total.as_secs_f64() * 1000.0 / clock.frames as f64,
            clock.worst.as_secs_f64() * 1000.0,
            clock.frames,
            scripts,
        );
        *clock = RuneFrameClock::default();
    });
}

/// Update (Playing): run one frame of Rune script execution end to end.
///
/// Pinned to the main thread (`_main_thread`): Rune's per-script state lives in
/// thread-locals, and the executor would otherwise run this on a different
/// worker from frame to frame, losing what the last frame stored.
#[cfg(feature = "realism-scripting")]
#[allow(clippy::too_many_arguments)]
pub fn drive_rune_frame(
    mut commands: Commands,
    time: Res<Time>,
    mut runtime: ResMut<RuneRuntimeState>,
    mut sim_values: ResMut<SimValuesResource>,
    mut script_writes: ResMut<crate::simulation::plugin::ScriptSimWrites>,
    mut bridges: ResMut<RunePlayBridges>,
    space_root: Option<Res<SpaceRoot>>,
    spatial: Option<Res<ScriptSpatialQuery>>,
    ecs_bindings: Option<Res<crate::ui::rune_ecs_bindings::ECSBindings>>,
    event_bus: Option<Res<eustress_common::events::EventBusResource>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut physics: ResMut<crate::soul::physics_bridge::RunePhysicsBridge>,
    terrain: ScriptTerrain,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    if runtime.compiled.is_empty() {
        // No scripts — make sure last frame's assertions don't linger and keep
        // pinning `battery.current` after the scripts were unloaded.
        if !script_writes.0.is_empty() {
            script_writes.0.clear();
        }
        return;
    }

    // The script profile (`eustress_common::utils::script_profile`), only
    // while it is armed: the whole driver is Rune's time for the frame, so
    // the part its callbacks did not take shows as Rune's own time, and the
    // frame's DataModel traffic counts as script work.
    let profile_start = eustress_common::utils::script_profile::armed().then(std::time::Instant::now);
    let _profile_scope = eustress_common::utils::script_profile::scope();

    // ── 1. Install every bridge on THIS thread ───────────────────────────
    install_bridges(
        &bridges,
        space_root.as_deref(),
        spatial.as_deref(),
        ecs_bindings.as_deref(),
        Some(&sim_values.0),
    );
    if let Some(bus) = event_bus.as_deref() {
        eustress_common::events::set_event_bus_for_rune(bus.0.clone());
    }
    // Physics reads (`part_get_mass` / `part_get_velocity`, the live gravity)
    // come from `RunePhysicsBridge`, installed here like every other bridge.
    rune_ecs_module::set_physics_state(physics.state.clone());
    let gravity_seed = physics.gravity;
    rune_ecs_module::WORKSPACE_GRAVITY.with(|g| *g.borrow_mut() = gravity_seed);
    // Raycast answers are keyed by call slot within a frame — see
    // `ScriptSpatialQuery`'s docs.
    crate::spatial_query_bridge::reset_raycast_slots();

    // ── 2. Lifecycle callbacks ───────────────────────────────────────────
    // The terrain view borrows the root's components, so rather than being
    // installed with the bridges above it is scoped to the callbacks.
    // `eustress::terrain` reads it and takes edits only while it is in place.
    let clock_start = crate::profiler::phase_armed().then(std::time::Instant::now);
    let terrain_view = terrain.view();
    rune_terrain::with_terrain_view(terrain_view.as_ref(), || {
        call_script_init(&mut runtime);
        call_script_ready(&mut runtime);
        call_script_update(&mut runtime, time.delta_secs() as f64);
    });
    // Purchases to grant go to the script defining `process_receipt`, while
    // the bridges are still installed.
    rune_ecs_module::process_rune_receipts(&mut runtime);
    if let Some(start) = clock_start {
        record_rune_frame(start.elapsed(), runtime.compiled.len());
    }

    // ── 3. Drain queued effects ──────────────────────────────────────────
    // Physics: impulses and velocity sets go to the systems that apply them,
    // and a gravity a script set goes to `Workspace.gravity`.
    physics.commands.extend(rune_ecs_module::drain_physics_commands());
    let gravity = rune_ecs_module::WORKSPACE_GRAVITY.with(|g| *g.borrow());
    if gravity != gravity_seed {
        physics.gravity_write = Some(gravity);
    }

    // Sim values: merge only what scripts wrote, so watchpoints, recordings,
    // `runtime-snapshot.json` and the MCP sim tools finally see script output.
    // The same set is published as `ScriptSimWrites` so consumers can tell an
    // explicit script assertion from an engine-published value.
    let written = rune_ecs_module::drain_script_sim_writes();
    for (key, value) in &written {
        sim_values.0.insert(key.clone(), *value);
    }
    // Replace, don't merge — a key only counts for the frame it was written in.
    if !written.is_empty() || !script_writes.0.is_empty() {
        script_writes.0 = written;
    }

    // Instances created via `Instance::new()`.
    if let Some(registry) = bridges.registry.clone() {
        let created = rune_ecs_module::drain_created_instances(&registry);
        if !created.is_empty() {
            spawn_created_instances(
                &mut commands,
                &asset_server,
                &mut materials,
                &mut bridges,
                created,
            );
        }
    }

    // `Instance:Destroy()` — despawn only. Unlike the command bar (an Edit-mode
    // authoring action, which trashes the backing file) a Play-mode destroy is
    // transient and must not touch disk; Stop restores the world anyway.
    for id in rune_ecs_module::drain_pending_destroys() {
        let entity = Entity::from_bits(id as u64);
        if let Ok(mut e) = commands.get_entity(entity) {
            e.despawn();
        }
    }

    // `Instance:Set()` against live entities — needs `&mut World`, so it goes
    // through the command queue, exactly as the command-bar path does.
    //
    // Unlike that path this does NOT push onto `crate::undo::UndoStack`:
    // gameplay mutations are transient (Stop restores the world), and every
    // frame of an `on_update` write would otherwise flood the undo history.
    let property_writes = rune_ecs_module::drain_pending_property_writes();
    if !property_writes.is_empty() {
        commands.queue(move |world: &mut World| {
            for write in property_writes {
                let entity = Entity::from_bits(write.entity_id as u64);
                if world.get_entity(entity).is_err() {
                    continue;
                }
                let old_value = crate::commands::PropertyCommand::read_property(
                    world,
                    entity,
                    &write.property_name,
                )
                .unwrap_or_else(|| write.new_value.clone());

                let cmd = crate::commands::PropertyCommand::new(
                    entity,
                    write.property_name.clone(),
                    old_value,
                    write.new_value.clone(),
                );
                if let Err(e) = cmd.execute(world) {
                    warn!(
                        "[Rune Script] Instance:Set(\"{}\", …) failed: {}",
                        write.property_name, e
                    );
                    continue;
                }

                // `BasePart::cframe` is the property-access target, but nothing
                // syncs it back onto `Transform` — every live mutation site in
                // this codebase (move/rotate/scale tools, undo.rs) mirrors by
                // hand. Without this the entity would never actually move.
                if write.property_name == "Position" {
                    if let eustress_common::classes::PropertyValue::Vector3(v) = &write.new_value {
                        let v = *v;
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            t.translation = v;
                        }
                    }
                } else if write.property_name == "Orientation" {
                    if let Some(bp) = world.get::<BasePart>(entity) {
                        let rot = bp.cframe.rotation;
                        if let Some(mut t) = world.get_mut::<Transform>(entity) {
                            t.rotation = rot;
                        }
                    }
                }
            }
        });
    }

    // `eustress::terrain` edits, in call order, as one batch of scripted
    // edits: no undo entry, and Stop restores the terrain Play snapshotted
    // before the first.
    let terrain_edits = rune_terrain::drain_pending_terrain();
    if !terrain_edits.is_empty() {
        commands.queue(move |world: &mut World| {
            rune_terrain::apply_script_edits(world, terrain_edits);
        });
    }

    // ── 4. Tear down ─────────────────────────────────────────────────────
    clear_bridges();
    if let Some(start) = profile_start {
        eustress_common::utils::script_profile::record_driver("rune", start.elapsed());
    }
}

/// No-op driver for builds without the scripting feature, so `play_mode.rs`
/// registers the same system either way.
#[cfg(not(feature = "realism-scripting"))]
pub fn drive_rune_frame() {}

// ============================================================================
// Materializing script-created instances
// ============================================================================

/// Spawn ECS entities for everything `Instance::new()` produced this frame.
///
/// Play-mode spawns are transient: `RuneSpawned` marks them, `stop_rune_session`
/// removes them, and nothing is written to disk.
#[cfg(feature = "realism-scripting")]
fn spawn_created_instances(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    bridges: &mut RunePlayBridges,
    created: Vec<eustress_common::luau::runtime::LuauCreatedInstance>,
) {
    use eustress_common::attributes::Tags;

    // Rune id → spawned entity, so `Parent = …` inside the same batch resolves.
    let mut rune_to_bevy: std::collections::HashMap<i64, Entity> = std::collections::HashMap::new();
    let mut spawned = 0usize;

    for inst in &created {
        let Some(shape) = part_shape_for_class(&inst.class_name, &inst.shape) else {
            // GUI classes and the rest reach the world through their own
            // bridges (`gui_set_*` → GuiCommand) or aren't representable as a
            // transient part. Say so once per class rather than every frame.
            if bridges.warned_classes.insert(inst.class_name.clone()) {
                warn!(
                    "[Rune Script] Instance::new(\"{}\") — only Part-family classes \
                     materialize during Play; use the command bar in Edit mode to \
                     author other classes",
                    inst.class_name
                );
            }
            continue;
        };

        let transform = Transform {
            translation: Vec3::from(inst.position),
            rotation: Quat::from_xyzw(
                inst.rotation[0],
                inst.rotation[1],
                inst.rotation[2],
                inst.rotation[3],
            )
            .normalize(),
            scale: Vec3::ONE,
        };

        let base_part = BasePart {
            cframe: transform,
            size: Vec3::from(inst.size),
            color: Color::srgba(inst.color[0], inst.color[1], inst.color[2], inst.color[3]),
            material: material_from_name(&inst.material),
            transparency: inst.transparency,
            anchored: inst.anchored,
            can_collide: inst.can_collide,
            ..Default::default()
        };

        let class_instance = ClassInstance {
            name: inst.name.clone(),
            class_name: eustress_common::classes::ClassName::Part,
            archivable: true,
            ..Default::default()
        };

        let entity = crate::spawn::spawn_part_glb(
            commands,
            asset_server,
            materials,
            class_instance,
            base_part,
            Part { shape },
        );
        commands.entity(entity).insert(RuneSpawned);
        if !inst.tags.is_empty() {
            commands.entity(entity).insert(Tags(inst.tags.clone()));
        }
        rune_to_bevy.insert(inst.luau_entity_id, entity);
        spawned += 1;
    }

    // Second pass: parent within the batch.
    for inst in &created {
        let (Some(&child), Some(parent_id)) =
            (rune_to_bevy.get(&inst.luau_entity_id), inst.parent_entity_id)
        else {
            continue;
        };
        if let Some(&parent) = rune_to_bevy.get(&parent_id) {
            commands.entity(parent).add_child(child);
        }
    }

    if spawned > 0 {
        info!("✨ Rune script spawned {spawned} part(s) this frame");
    }
}

/// Map a script's `class_name` / `Shape` pair onto a [`PartType`], or `None`
/// when the class isn't part-shaped.
#[cfg(feature = "realism-scripting")]
fn part_shape_for_class(class_name: &str, shape: &str) -> Option<PartType> {
    match class_name {
        "Part" | "MeshPart" => Some(match shape {
            "Ball" | "Sphere" => PartType::Ball,
            "Cylinder" => PartType::Cylinder,
            "Wedge" => PartType::Wedge,
            "CornerWedge" => PartType::CornerWedge,
            "Cone" => PartType::Cone,
            _ => PartType::Block,
        }),
        "SpherePart" => Some(PartType::Ball),
        "CylinderPart" => Some(PartType::Cylinder),
        "WedgePart" => Some(PartType::Wedge),
        "CornerWedgePart" => Some(PartType::CornerWedge),
        _ => None,
    }
}

/// Resolve a `Material` preset name. Unknown names fall back to `Plastic`,
/// matching every other string→Material site in the engine.
#[cfg(feature = "realism-scripting")]
fn material_from_name(name: &str) -> Material {
    match name {
        "SmoothPlastic" => Material::SmoothPlastic,
        "Wood" => Material::Wood,
        "WoodPlanks" => Material::WoodPlanks,
        "Metal" => Material::Metal,
        "CorrodedMetal" => Material::CorrodedMetal,
        "DiamondPlate" => Material::DiamondPlate,
        "Foil" => Material::Foil,
        "Grass" => Material::Grass,
        "Concrete" => Material::Concrete,
        "Brick" => Material::Brick,
        "Granite" => Material::Granite,
        "Marble" => Material::Marble,
        "Slate" => Material::Slate,
        "Sand" => Material::Sand,
        "Fabric" => Material::Fabric,
        "Glass" => Material::Glass,
        "Neon" => Material::Neon,
        "Ice" => Material::Ice,
        _ => Material::Plastic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "realism-scripting")]
    fn part_classes_map_to_shapes() {
        assert_eq!(part_shape_for_class("Part", "Ball"), Some(PartType::Ball));
        assert_eq!(part_shape_for_class("Part", ""), Some(PartType::Block));
        assert_eq!(part_shape_for_class("SpherePart", "Block"), Some(PartType::Ball));
        // Non-part classes must NOT silently become blocks — the caller warns.
        assert_eq!(part_shape_for_class("BillboardGui", "Block"), None);
        assert_eq!(part_shape_for_class("Folder", "Block"), None);
    }

    #[test]
    #[cfg(feature = "realism-scripting")]
    fn unknown_material_falls_back_to_plastic() {
        assert!(matches!(material_from_name("Neon"), Material::Neon));
        assert!(matches!(material_from_name("NotAMaterial"), Material::Plastic));
    }
}
