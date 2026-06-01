//! Binary-ECS instance load + save wiring (K2 / representation router).
//!
//! This is the runtime half of the [`super::representation`] router's
//! **BinaryEcs** arm: entities whose authoritative state is a zero-copy
//! rkyv [`eustress_worlddb::ArchInstanceCore`] in the `entities`
//! partition (Morton-keyed, scalable to millions), with NO disk folder
//! and NO `tree` TOML. The **FileSystem** arm (folder + `_instance.toml`)
//! is still handled by the file loader; the two are mutually exclusive
//! per entity, so nothing double-spawns.
//!
//! ## What this module does
//!
//! - [`load_binary_ecs_instances`] — the boot-load: once per Space, after
//!   the file loader has spawned the services, it reads every core from
//!   the `entities` partition and spawns each as a real Bevy entity via
//!   the SAME [`spawn_instance`] the disk path uses, parented under
//!   Workspace. Because the Explorer, Properties panel and viewport are
//!   all ECS-driven (they query `Instance` + `Transform` + the hierarchy,
//!   not a file path), a binary-ECS entity shows up in all three with no
//!   extra wiring. It carries NO `LoadedFromFile`, so the Explorer
//!   classifier folds it under Workspace via the ClassName fallback.
//! - [`mirror_binary_ecs_changes`] — the save: when a binary-ECS entity's
//!   transform or part properties change, it rebuilds the core from the
//!   live components and writes it back. Because the Morton key is
//!   position-derived, a *move* deletes the record at the old position
//!   before putting at the new one.
//!
//! ## Why no stray disk writes from the edit tools
//!
//! A binary-ECS entity carries a *synthetic* `InstanceFile.toml_path`
//! (every `spawn_instance` inserts one) that points at a path which does
//! not exist on disk and is not in the `tree`. The move/scale/etc. tools
//! gate their write-back on `load_instance_definition` succeeding FIRST,
//! and that load fails for the synthetic path, so they self-skip
//! binary-ECS entities. This mirror is therefore the sole persister — no
//! tool guards or `InstanceFile` removal needed.
//!
//! ## Scalability
//!
//! [`BinaryEcsInstance`] is deliberately tiny (one id + the last-persisted
//! transform for the value-gate). The save mirror reconstructs the core
//! from live components rather than caching a per-entity copy, so a
//! million bare parts cost a million 48-byte markers, not a million cores.

#![cfg(feature = "world-db")]

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use eustress_common::classes::{BasePart, Instance};
use eustress_common::Tags;
use eustress_worlddb::{decode_instance_core, encode_instance_core, ArchInstanceCore};

use super::active_db;
use super::arch_instance;
use super::file_loader::LoadInProgress;
use super::instance_loader::{
    spawn_instance, AssetReference, InstanceDefinition, InstanceMetadata, InstanceProperties,
    PrimitiveMeshCache, TransformData,
};
use super::material_loader::MaterialRegistry;
use super::service_loader::ServiceComponent;
use super::SpaceRoot;

/// Default glTF scene name (matches `instance_loader::default_scene`,
/// inlined to avoid widening that module's visibility).
const DEFAULT_SCENE: &str = "Scene0";

/// Fallback primitive mesh when an entity has no `MeshSource` (defensive;
/// every spawned part gets one).
const FALLBACK_MESH: &str = "parts/block.glb";

/// Marks an entity whose authoritative state is a rkyv `ArchInstanceCore`
/// in the `entities` partition (the scalable binary-ECS representation),
/// NOT a disk/`tree` TOML.
#[derive(Component, Debug, Clone)]
pub struct BinaryEcsInstance {
    /// Stable persistence id. NOT the live `Entity::to_bits()` (those are
    /// not stable across sessions); minted once at create and preserved
    /// across load so the Morton key stays addressable.
    pub stored_id: u64,
    /// Last-persisted translation — also the position the current Morton
    /// key was computed from. A move deletes the record here before
    /// putting at the new position. Doubles as the transform value-gate
    /// baseline that kills the Avian same-value `Changed<Transform>` storm.
    pub morton_pos: [f32; 3],
    /// Last-persisted rotation (value-gate only).
    pub last_rot: [f32; 4],
    /// Last-persisted scale (value-gate only).
    pub last_scale: [f32; 3],
}

impl BinaryEcsInstance {
    /// Build a marker from a core's transform fields (the load/create
    /// baseline) so the first mirror frame after spawn is a no-op.
    fn from_core(stored_id: u64, core: &ArchInstanceCore) -> Self {
        Self {
            stored_id,
            morton_pos: core.t,
            last_rot: core.r,
            last_scale: core.s,
        }
    }
}

/// Latch: the Space path the binary-ECS boot-load already ran for. A
/// genuine Space switch (path change) re-arms it, so the load runs
/// exactly once per Space — mirrors `WorldDbDecision`.
#[derive(Resource, Default)]
pub struct BinaryEcsLoadLatch(pub Option<PathBuf>);

/// Build an `InstanceDefinition` from an entity's live components, then
/// bake it to an `ArchInstanceCore` via the tested
/// [`arch_instance::instance_to_arch`]. Used by the save mirror so the
/// bytes match what the load path produced — and by the bridge
/// `entity.read` handler to project a RESIDENT entity's live state.
pub(crate) fn core_from_components(
    instance: &Instance,
    transform: &Transform,
    base: Option<&BasePart>,
    tags: Option<&Tags>,
    mesh: &str,
) -> ArchInstanceCore {
    let now = chrono::Utc::now().to_rfc3339();
    let props = base
        .map(|b| {
            let c = b.color.to_srgba();
            InstanceProperties {
                color: [c.red, c.green, c.blue, c.alpha],
                transparency: b.transparency,
                anchored: b.anchored,
                can_collide: b.can_collide,
                cast_shadow: b.cast_shadow,
                reflectance: b.reflectance,
                material: b.material_name.clone(),
                locked: b.locked,
            }
        })
        .unwrap_or_default();

    let def = InstanceDefinition {
        asset: if mesh.is_empty() {
            None
        } else {
            Some(AssetReference {
                mesh: mesh.to_string(),
                scene: DEFAULT_SCENE.to_string(),
            })
        },
        transform: TransformData {
            position: transform.translation.to_array(),
            rotation: [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ],
            scale: transform.scale.to_array(),
        },
        properties: props,
        metadata: InstanceMetadata {
            class_name: instance.class_name.as_str().to_string(),
            archivable: instance.archivable,
            name: Some(instance.name.clone()),
            created: now.clone(),
            last_modified: now,
            created_by: None,
            modifications: Vec::new(),
            unit: None,
            uuid: if instance.uuid.is_empty() {
                None
            } else {
                Some(instance.uuid.clone())
            },
        },
        material: None,
        thermodynamic: None,
        electrochemical: None,
        ui: None,
        attributes: None,
        tags: tags.map(|t| t.0.clone()).filter(|v| !v.is_empty()),
        parameters: None,
        extra: std::collections::HashMap::new(),
    };
    arch_instance::instance_to_arch(&def)
}

/// Synthetic in-Space path for a binary-ECS entity. The entity carries an
/// `InstanceFile.toml_path` (every spawn does) but NOTHING is ever written
/// here — see the module doc on why the edit tools self-skip these paths.
fn synthetic_path(space_root: &Path, class_name: &str, stored_id: u64) -> PathBuf {
    space_root
        .join("Workspace")
        .join(format!("__bin_{class_name}_{stored_id:016x}"))
        .join("_instance.toml")
}

/// Create a brand-new binary-ECS entity at runtime — the create-twin of
/// [`load_binary_ecs_instances`]'s per-record body, and the heart of the
/// "Insert defaults to the scalable representation" flip
/// (SCALING_ARCHITECTURE.md §0.5 C1).
///
/// Given a freshly-built [`InstanceDefinition`] (the same parse model the
/// disk path uses), this:
/// 1. Refuses anything the router keeps on the filesystem (custom mesh,
///    file-natured class) — returns `None` so the caller takes its TOML
///    path. This is the V-Cell custom-mesh guard, enforced at create.
/// 2. Mints the persistent identity: a 32-hex `uuid` (or honours one
///    already on the def) and derives the stable `stored_id` (Morton key)
///    from its first 8 bytes.
/// 3. Bakes to `ArchInstanceCore` and back, so the spawned entity is
///    BYTE-IDENTICAL to what the boot-load reconstructs from the persisted
///    bytes (create == load — no visual drift across a reload).
/// 4. Spawns via the SHARED [`spawn_instance`] + inserts the
///    [`BinaryEcsInstance`] marker and `ChildOf(workspace)` (exactly the
///    boot-load shape: no `InstanceFile`/`LoadedFromFile`, no disk TOML).
/// 5. Persists all five stores via [`active_db::create_binary_instance`]
///    (Morton core + uuid primary + path/uuid/class indices) so the new
///    part is immediately findable by uuid / path / class.
///
/// Returns the spawned `Entity` (and its `stored_id` + `uuid` for undo
/// recording) or `None` when routed to the filesystem / no DB active /
/// encode failed.
#[allow(clippy::too_many_arguments)]
pub fn spawn_binary_instance(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    space_root: &Path,
    workspace: Entity,
    mut def: InstanceDefinition,
) -> Option<SpawnedBinary> {
    use eustress_common::instance_create::{
        fresh_uuid_for_create, is_valid_uuid, uuid_hex_to_bytes,
    };

    // 1. Router guard — never binarise a custom-mesh / file-natured class.
    let mesh = def.asset.as_ref().map(|a| a.mesh.as_str());
    if super::representation::representation_for_part(&def.metadata.class_name, mesh, None)
        != super::representation::Representation::BinaryEcs
    {
        return None;
    }

    // 2. Mint identity (honour a pre-set valid uuid; else fresh).
    let uuid_hex = match def.metadata.uuid.as_deref() {
        Some(u) if is_valid_uuid(u) => u.to_string(),
        _ => fresh_uuid_for_create(),
    };
    let uuid_bytes = uuid_hex_to_bytes(&uuid_hex)?;
    def.metadata.uuid = Some(uuid_hex.clone());
    // Stable Morton-key id derived from the identity uuid (no separate
    // counter; collision-resistant because the uuid is blake3-random).
    let stored_id = u64::from_be_bytes(
        uuid_bytes[0..8]
            .try_into()
            .expect("uuid_bytes is 16 long; [0..8] is 8"),
    );

    // 3. Bake → encode → bake-back for create==load parity.
    let arch = arch_instance::instance_to_arch(&def);
    let encoded = match encode_instance_core(&arch) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db",
                error = %e, stored_id,
                "spawn_binary_instance: encode failed; not creating"
            );
            return None;
        }
    };
    let synthetic = synthetic_path(space_root, &arch.class_name, stored_id);
    let rel = synthetic
        .strip_prefix(space_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| {
            format!(
                "Workspace/__bin_{}_{stored_id:016x}/_instance.toml",
                arch.class_name
            )
        });
    let def_for_spawn = arch_instance::arch_to_instance(&arch);

    // 4. Spawn (shared path) + marker + parent under Workspace.
    let entity = spawn_instance(
        commands,
        asset_server,
        materials,
        material_registry,
        mesh_cache,
        synthetic,
        def_for_spawn,
    );
    let marker = BinaryEcsInstance::from_core(stored_id, &arch);
    commands.entity(entity).insert((marker, ChildOf(workspace)));

    // 5. Persist all five stores (best-effort; warns on partial write).
    active_db::create_binary_instance(stored_id, &uuid_bytes, &arch.class_name, arch.t, &encoded, &rel);

    Some(SpawnedBinary {
        entity,
        stored_id,
        uuid: uuid_hex,
        pos: arch.t,
        class_name: arch.class_name,
        synthetic_rel: rel,
        def,
    })
}

/// Outcome of [`spawn_binary_instance`] — the live entity plus the
/// identity needed to undo / delete the create (all five stores keyed by
/// these). `def` carries the uuid, so it can be serialized into the undo
/// action and re-spawned (same identity) on redo.
pub struct SpawnedBinary {
    pub entity: Entity,
    pub stored_id: u64,
    pub uuid: String,
    pub pos: [f32; 3],
    pub class_name: String,
    pub synthetic_rel: String,
    pub def: InstanceDefinition,
}

/// Redo queue for binary creates: serialized `InstanceDefinition`s (each
/// carrying its uuid) awaiting re-spawn. The undo system's redo arm pushes
/// here; [`drain_pending_binary_recreate`] re-spawns next frame with proper
/// system params. Keeping the spawn in a real system avoids the
/// `&mut World` vs `Commands`+`ResMut` borrow conflict.
#[derive(Resource, Default)]
pub struct PendingBinaryRecreate(pub Vec<String>);

/// Drain [`PendingBinaryRecreate`] (redo of a binary create): deserialize
/// each stored def and re-spawn it via [`spawn_binary_instance`]. Because
/// the def keeps its uuid, the re-created entity gets the SAME `stored_id`
/// and overwrites the same five stores — identity is preserved across
/// undo→redo. If services aren't ready yet, the items are left queued.
#[allow(clippy::too_many_arguments)]
fn drain_pending_binary_recreate(
    mut pending: ResMut<PendingBinaryRecreate>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<MaterialRegistry>,
    mut mesh_cache: ResMut<PrimitiveMeshCache>,
    space_root: Res<SpaceRoot>,
    services: Query<(Entity, &ServiceComponent)>,
) {
    if pending.0.is_empty() {
        return;
    }
    let Some(workspace) = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e)
    else {
        return; // services not spawned yet — keep the queue, retry next frame
    };
    let jobs: Vec<String> = pending.0.drain(..).collect();
    for def_json in jobs {
        match serde_json::from_str::<InstanceDefinition>(&def_json) {
            Ok(def) => {
                spawn_binary_instance(
                    &mut commands,
                    &asset_server,
                    &mut materials,
                    &mut material_registry,
                    &mut mesh_cache,
                    &space_root.0,
                    workspace,
                    def,
                );
            }
            Err(e) => {
                warn!(
                    target: "eustress_engine::world_db",
                    error = %e,
                    "redo recreate: failed to deserialize stored def; dropping"
                );
            }
        }
    }
}

/// Spawn ONE binary-ECS core into the live world — the shared per-record
/// body used by BOTH the boot-load (small Spaces) and the streaming
/// residency manager (large Spaces). One place guarantees a streamed
/// entity is byte-identical to a boot-loaded / created one, and the mirror
/// persists either's edits unchanged. Returns the spawned `Entity`, or
/// `None` if the core failed to decode.
pub(crate) fn spawn_binary_core(
    commands: &mut Commands,
    asset_server: &AssetServer,
    materials: &mut Assets<StandardMaterial>,
    material_registry: &mut MaterialRegistry,
    mesh_cache: &mut PrimitiveMeshCache,
    space_root: &Path,
    workspace: Entity,
    stored_id: u64,
    bytes: &[u8],
) -> Option<Entity> {
    let arch = match decode_instance_core(bytes) {
        Ok(a) => a,
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db",
                error = %e, stored_id,
                "binary-ECS spawn: decode failed; skipping record"
            );
            return None;
        }
    };
    let marker = BinaryEcsInstance::from_core(stored_id, &arch);
    let synthetic = synthetic_path(space_root, &arch.class_name, stored_id);
    let def = arch_instance::arch_to_instance(&arch);
    let entity = spawn_instance(
        commands,
        asset_server,
        materials,
        material_registry,
        mesh_cache,
        synthetic,
        def,
    );
    commands.entity(entity).insert((marker, ChildOf(workspace)));
    Some(entity)
}

/// Phase 4 — stream a DB-only entity into the live ECS when the user selects
/// its row in the virtual "Database (streamed)" Explorer section.
///
/// The Explorer's `SelectNode` handler set
/// `UnifiedExplorerState.selected = DbStreamed(uuid)` (it cannot spawn — it
/// has no Bevy command buffer). This system is the only place that can:
/// it resolves the uuid to its core, spawns it via the SAME path as
/// boot-load / create (`spawn_binary_core`), registers it in the selection
/// manager (which inserts `Selected`, so residency eviction skips it —
/// residency.rs), and flips selection to the now-live `Entity`. The mirror
/// then persists any edit; the entity evicts normally once deselected (no
/// leak — the pin is released by the selection manager, not held forever).
///
/// Idempotent: if the entity is already resident (residency streamed it in
/// between the click and now), it just selects the live one.
#[allow(clippy::too_many_arguments)]
pub fn sys_stream_in_on_select(
    mut commands: Commands,
    explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<MaterialRegistry>,
    mut mesh_cache: ResMut<PrimitiveMeshCache>,
    space_root: Res<SpaceRoot>,
    handle: Res<super::world_db_plugin::WorldDbHandle>,
    selection_manager: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    services: Query<(Entity, &ServiceComponent)>,
    existing: Query<(Entity, &Instance), With<BinaryEcsInstance>>,
) {
    use crate::ui::slint_ui::SelectedItem;
    let Some(mut es) = explorer_state else {
        return;
    };
    // Cheap early-out: act only on the transient DbStreamed state.
    let SelectedItem::DbStreamed(uuid_hex) = es.selected.clone() else {
        return;
    };

    // Helper to register selection in the manager (authoritative — it drives
    // the `Selected` component via sync_selection_components, and removes it
    // on deselect so the entity un-pins and can evict).
    let select_in_manager = |entity: Entity| {
        if let Some(ref mgr) = selection_manager {
            let id_str = format!("{}v{}", entity.index(), entity.generation());
            mgr.0.write().select(id_str);
        }
    };

    // Already resident? (residency may have streamed it in.) Just select it.
    if let Some((entity, _)) = existing.iter().find(|(_, i)| i.uuid == uuid_hex) {
        commands.entity(entity).insert(crate::selection_box::Selected);
        select_in_manager(entity);
        es.selected = SelectedItem::Entity(entity);
        es.needs_immediate_sync = true;
        return;
    }

    // Resolve DB + raw core bytes + Workspace.
    let Some(db) = handle.0.as_ref() else {
        es.selected = SelectedItem::None;
        return;
    };
    let Some(uuid_bytes) =
        eustress_common::instance_create::uuid_hex_to_bytes(&uuid_hex)
    else {
        es.selected = SelectedItem::None;
        return;
    };
    let bytes = match db.get_entity_core_by_uuid(&uuid_bytes) {
        Ok(Some(b)) => b,
        Ok(None) => {
            warn!(
                target: "eustress_engine::world_db", uuid = %uuid_hex,
                "stream-in: no core for uuid (stale Explorer row?)"
            );
            es.selected = SelectedItem::None;
            return;
        }
        Err(e) => {
            warn!(
                target: "eustress_engine::world_db", error = %e, uuid = %uuid_hex,
                "stream-in: core read failed"
            );
            es.selected = SelectedItem::None;
            return;
        }
    };
    let Some(workspace) = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e)
    else {
        // Services not ready yet — keep DbStreamed, retry next frame.
        return;
    };
    let stored_id = u64::from_be_bytes(
        uuid_bytes[0..8]
            .try_into()
            .expect("uuid_bytes is 16 long; [0..8] is 8"),
    );

    match spawn_binary_core(
        &mut commands,
        &asset_server,
        &mut materials,
        &mut material_registry,
        &mut mesh_cache,
        &space_root.0,
        workspace,
        stored_id,
        &bytes,
    ) {
        Some(entity) => {
            // Immediate pin (covers a same-frame evict) + authoritative
            // manager selection (keeps it pinned until deselect).
            commands
                .entity(entity)
                .insert(crate::selection_box::Selected);
            select_in_manager(entity);
            es.selected = SelectedItem::Entity(entity);
            es.needs_immediate_sync = true;
            info!(
                target: "eustress_engine::world_db", uuid = %uuid_hex, ?entity,
                "stream-in: DB-only entity streamed into the live ECS"
            );
        }
        None => {
            es.selected = SelectedItem::None;
        }
    }
}

/// Boot-load every binary-ECS core in the active Space into the ECS, once
/// per Space, after the file loader has spawned the services.
#[allow(clippy::too_many_arguments)]
fn load_binary_ecs_instances(
    mut commands: Commands,
    space_root: Res<SpaceRoot>,
    mut latch: ResMut<BinaryEcsLoadLatch>,
    load_in_progress: Res<LoadInProgress>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut material_registry: ResMut<MaterialRegistry>,
    mut mesh_cache: ResMut<PrimitiveMeshCache>,
    services: Query<(Entity, &ServiceComponent)>,
    // Already-live binary entities (e.g. created by `spawn_binary_instance`
    // before this one-shot boot-load ran for the Space). We skip their
    // stored_ids so an early runtime create is never double-spawned.
    existing: Query<&BinaryEcsInstance>,
    // Phase 2: decide boot-load-all (small Space) vs camera streaming (large).
    mut residency: ResMut<super::residency::ResidencyState>,
    residency_cfg: Res<super::residency::ResidencyConfig>,
) {
    // Already loaded for this Space.
    if latch.0.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    // Wait until the file loader's pass finishes (services + disk entities
    // spawned) so we can parent under Workspace without racing the gate.
    if load_in_progress.active {
        return;
    }
    // Need the Workspace service entity. If services aren't spawned yet,
    // retry next frame WITHOUT latching.
    let Some(workspace) = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e)
    else {
        return;
    };

    // Commit the decision for this Space so we never re-spawn.
    latch.0 = Some(space_root.0.clone());

    // No DB → legacy disk-only Space; the entities partition is empty by
    // definition. Latched above, so this is a one-shot no-op.
    if !active_db::is_active() {
        return;
    }

    // Phase 2 threshold: a LARGE Space is NOT boot-loaded in full — the
    // camera-locality residency manager streams cells in/out so the live
    // ECS set stays bounded. A small Space keeps today's spawn-everything
    // behavior (all content present, zero streaming overhead). The probe
    // stops counting at threshold+1, so a 10M Space pays only a bounded
    // scan here, not a full materialization.
    let cap = residency_cfg.big_space_threshold.saturating_add(1);
    if active_db::count_instance_cores_capped(cap) > residency_cfg.big_space_threshold {
        residency.enabled = true;
        residency.last_camera_cell = None; // first residency tick loads the camera box
        // Mirror into the non-gated flag the Explorer reads to show the
        // virtual "Database (streamed)" section (Phase 4).
        active_db::set_streaming_active(true);
        info!(
            target: "eustress_engine::world_db",
            threshold = residency_cfg.big_space_threshold,
            space = %space_root.0.display(),
            "binary-ECS: large Space — STREAMING by camera (residency manager), \
             not boot-loading all cores"
        );
        return;
    }
    residency.enabled = false; // small Space: boot-load all; residency idle
    active_db::set_streaming_active(false); // no DB section for small Spaces

    let cores = active_db::iter_instance_cores();
    if cores.is_empty() {
        return;
    }

    // stored_ids already live this session (runtime-created before boot-load).
    let existing_ids: std::collections::HashSet<u64> =
        existing.iter().map(|b| b.stored_id).collect();

    let mut spawned = 0usize;
    for (stored_id, bytes) in cores {
        if existing_ids.contains(&stored_id) {
            continue;
        }
        if spawn_binary_core(
            &mut commands,
            &asset_server,
            &mut materials,
            &mut material_registry,
            &mut mesh_cache,
            &space_root.0,
            workspace,
            stored_id,
            &bytes,
        )
        .is_some()
        {
            spawned += 1;
        }
    }
    info!(
        target: "eustress_engine::world_db",
        spawned,
        space = %space_root.0.display(),
        "binary-ECS boot-load: spawned entities from the entities partition \
         into the ECS (visible in viewport + Explorer + Properties)"
    );
}

/// Persist live edits to binary-ECS entities back into the `entities`
/// partition. Value-gated so the Avian same-value `Changed<Transform>`
/// storm does zero work on idle anchored parts.
#[allow(clippy::type_complexity)]
fn mirror_binary_ecs_changes(
    load_in_progress: Res<LoadInProgress>,
    // Play mode is ephemeral: physics moves bodies and scripts spawn parts
    // that must NOT persist. Bail entirely while playing so a `Stop` always
    // restores the pre-play Fjall state (no leaked / mutated cores).
    play_mode_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    mut q: Query<
        (
            Ref<Instance>,
            Ref<Transform>,
            Option<Ref<BasePart>>,
            Option<Ref<Tags>>,
            Option<&crate::spawn::MeshSource>,
            &mut BinaryEcsInstance,
        ),
        (
            // `Changed<Instance>` catches renames (the display name lives on
            // `Instance.name`, which `core_from_components` persists).
            Or<(
                Changed<Transform>,
                Changed<BasePart>,
                Changed<Tags>,
                Changed<Instance>,
            )>,
            // Parts spawned during play are ephemeral — never persisted, so
            // there is nothing to clean up on Stop.
            Without<crate::play_mode::SpawnedDuringPlayMode>,
        ),
    >,
) {
    if load_in_progress.active {
        return;
    }
    if let Some(state) = play_mode_state {
        if *state.get() != crate::play_mode::PlayModeState::Editing {
            return;
        }
    }
    if !active_db::is_active() {
        return;
    }

    for (instance, transform, base, tags, mesh_src, mut bin) in q.iter_mut() {
        let new_pos = transform.translation.to_array();
        let new_rot = [
            transform.rotation.x,
            transform.rotation.y,
            transform.rotation.z,
            transform.rotation.w,
        ];
        let new_scale = transform.scale.to_array();

        // Cheap value-gate FIRST (no alloc). The Avian transform-sync
        // re-writes anchored bodies to the SAME value every frame, tripping
        // `Changed<Transform>`; skip when nothing actually moved AND no part
        // property changed. `Ref::is_changed` on BasePart/Tags is false
        // during that storm (Avian touches Transform, not BasePart).
        let tf_changed =
            new_pos != bin.morton_pos || new_rot != bin.last_rot || new_scale != bin.last_scale;
        // `!is_added()` skips the spawn frame: a just-loaded entity's
        // BasePart/Tags read as "changed" the frame after spawn, but the
        // data already lives in the DB (we loaded it FROM there), so
        // persisting again would be a redundant write spike across the
        // whole grid. A genuine later edit has is_changed && !is_added.
        // A rename (or any other Instance edit) is a genuine change; the
        // `!is_added()` guard skips the post-spawn frame like BasePart/Tags.
        let inst_changed = instance.is_changed() && !instance.is_added();
        let props_changed = inst_changed
            || base
                .as_ref()
                .map(|b| b.is_changed() && !b.is_added())
                .unwrap_or(false)
            || tags
                .as_ref()
                .map(|t| t.is_changed() && !t.is_added())
                .unwrap_or(false);
        if !tf_changed && !props_changed {
            continue;
        }

        let mesh = mesh_src.map(|m| m.path.as_str()).unwrap_or(FALLBACK_MESH);
        let core = core_from_components(
            &*instance,
            &*transform,
            base.as_deref(),
            tags.as_deref(),
            mesh,
        );
        let encoded = match encode_instance_core(&core) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    target: "eustress_engine::world_db",
                    error = %e, stored_id = bin.stored_id,
                    "binary-ECS mirror: encode failed; skipping this change"
                );
                continue;
            }
        };

        // Persist to BOTH cores (Morton spatial + uuid-primary) so an edit
        // is visible to boot-load/streaming AND to find-by-uuid / the bridge
        // `entity.read` of a non-resident entity. Morton key is position-
        // derived, so a move deletes the old key before writing the new one
        // (handled inside mirror_binary_core); the uuid key is position-
        // independent (plain overwrite).
        let uuid_bytes = eustress_common::instance_create::uuid_hex_to_bytes(&instance.uuid);
        if active_db::mirror_binary_core(
            bin.stored_id,
            uuid_bytes.as_ref(),
            bin.morton_pos,
            new_pos,
            &encoded,
        ) {
            bin.morton_pos = new_pos;
            bin.last_rot = new_rot;
            bin.last_scale = new_scale;
        }
    }
}

/// Register the binary-ECS load + save systems. Called from
/// [`super::world_db_plugin::WorldDbPlugin`].
pub fn register(app: &mut App) {
    app.init_resource::<BinaryEcsLoadLatch>()
        .init_resource::<PendingBinaryRecreate>()
        .init_resource::<super::residency::ResidencyState>()
        .init_resource::<super::residency::ResidencyConfig>()
        // Order is load-bearing (Phase 2 risk R1): boot-load decides the
        // streaming mode → residency loads camera-local cells → the mirror
        // PERSISTS edits → residency evicts far cells. Eviction must come
        // AFTER the mirror so an edited entity's core is written before it
        // is despawned.
        .add_systems(
            Update,
            (
                load_binary_ecs_instances,
                super::residency::sys_residency_load,
                mirror_binary_ecs_changes,
                super::residency::sys_residency_evict,
            )
                .chain(),
        )
        .add_systems(Update, drain_pending_binary_recreate)
        // Phase 4 — select-to-stream-in for the virtual DB Explorer. Runs
        // before eviction so a just-streamed entity is registered/pinned in
        // the same frame's selection pass (the spawn itself is deferred, so
        // it can't be evicted this frame regardless).
        .add_systems(
            Update,
            sys_stream_in_on_select.before(super::residency::sys_residency_evict),
        );
}

// ─────────────────────────────────────────────────────────────────────
// IDENTITY.md §10.4 — find_entity_by_* helpers
// ─────────────────────────────────────────────────────────────────────
//
// Wrap the trait calls and handle the `[u8; 16]` ↔ hex `String` conversion
// at the boundary so the rest of the engine stays string-typed. The MCP
// `find_entity --uuid` / `--path` / `--class` tools route through these.

use eustress_common::instance_create::{uuid_hex_to_bytes, is_valid_uuid};
use eustress_worlddb::WorldDb;

/// Look up an entity by its 32-char-hex UUID. Returns the rkyv
/// `ArchInstanceCore` for that entity (or `None` when no row exists for
/// this uuid). Used by MCP `find_entity --uuid`, the audit-log replayer,
/// and the multiplayer "follow player" routing.
pub fn find_entity_by_uuid(
    db: &dyn WorldDb,
    uuid_hex: &str,
) -> Result<Option<ArchInstanceCore>, String> {
    if !is_valid_uuid(uuid_hex) {
        return Err(format!(
            "find_entity_by_uuid: malformed uuid {uuid_hex:?} — expected 32 lowercase hex chars"
        ));
    }
    let bytes = match uuid_hex_to_bytes(uuid_hex) {
        Some(b) => b,
        None => return Ok(None),
    };
    match db.get_entity_core_by_uuid(&bytes) {
        Ok(Some(buf)) => match decode_instance_core(&buf) {
            Ok(core) => Ok(Some(core)),
            Err(e) => Err(format!("decode core for uuid {uuid_hex}: {e}")),
        },
        Ok(None) => Ok(None),
        Err(e) => Err(format!("get_entity_core_by_uuid {uuid_hex}: {e}")),
    }
}

/// Look up an entity by its Space-relative TOML path (e.g.
/// `Workspace/Tower/_instance.toml`). Hops `path_to_uuid` once, then
/// `entities_uuid` — both point reads. Returns `Ok(None)` when no entity
/// lives at this path right now. Used by MCP `find_entity --path` for
/// backward compatibility after the Wave 2.1 uuid pivot.
pub fn find_entity_by_path(
    db: &dyn WorldDb,
    rel_path: &str,
) -> Result<Option<ArchInstanceCore>, String> {
    let uuid_bytes = match db.path_to_uuid(rel_path) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("path_to_uuid {rel_path}: {e}")),
    };
    match db.get_entity_core_by_uuid(&uuid_bytes) {
        Ok(Some(buf)) => match decode_instance_core(&buf) {
            Ok(core) => Ok(Some(core)),
            Err(e) => Err(format!(
                "decode core for path {rel_path}: {e}"
            )),
        },
        Ok(None) => Ok(None),
        Err(e) => Err(format!(
            "get_entity_core_by_uuid via path {rel_path}: {e}"
        )),
    }
}

/// Eagerly collect every entity registered under `class_name` in the
/// class_index. Returns the full set of `ArchInstanceCore` records. Used
/// by Studio's class-filter views + by AI tools that want "all parts in
/// this Space" without hitting `iter_instance_cores` (which scans the
/// whole Morton-keyed prefix). Cost scales with the count returned, NOT
/// total entity count.
pub fn find_entities_by_class(
    db: &dyn WorldDb,
    class_name: &str,
) -> Result<Vec<ArchInstanceCore>, String> {
    let uuids = match db.iter_class(class_name) {
        Ok(v) => v,
        Err(e) => return Err(format!("iter_class {class_name}: {e}")),
    };
    let mut out = Vec::with_capacity(uuids.len());
    for u in uuids {
        match db.get_entity_core_by_uuid(&u) {
            Ok(Some(buf)) => match decode_instance_core(&buf) {
                Ok(core) => out.push(core),
                Err(e) => {
                    warn!(
                        target: "eustress_engine::world_db",
                        error = %e,
                        class = class_name,
                        "find_entities_by_class: decode failed for one uuid; skipping"
                    );
                }
            },
            Ok(None) => {
                // Stale class_index entry — the entity was deleted but the
                // index wasn't dropped. Skip; rebuild_indexes() repairs.
            }
            Err(e) => {
                return Err(format!(
                    "get_entity_core_by_uuid in find_entities_by_class {class_name}: {e}"
                ))
            }
        }
    }
    Ok(out)
}
