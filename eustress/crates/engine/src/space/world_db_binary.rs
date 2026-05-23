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
/// bytes match what the load path produced.
fn core_from_components(
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

    let cores = active_db::iter_instance_cores();
    if cores.is_empty() {
        return;
    }

    let mut spawned = 0usize;
    for (stored_id, bytes) in cores {
        let arch = match decode_instance_core(&bytes) {
            Ok(a) => a,
            Err(e) => {
                warn!(
                    target: "eustress_engine::world_db",
                    error = %e, stored_id,
                    "binary-ECS load: decode failed; skipping record"
                );
                continue;
            }
        };
        let marker = BinaryEcsInstance::from_core(stored_id, &arch);
        let synthetic = synthetic_path(&space_root.0, &arch.class_name, stored_id);
        let def = arch_instance::arch_to_instance(&arch);
        let entity = spawn_instance(
            &mut commands,
            &asset_server,
            &mut materials,
            &mut material_registry,
            &mut mesh_cache,
            synthetic,
            def,
        );
        commands.entity(entity).insert((marker, ChildOf(workspace)));
        spawned += 1;
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
    mut q: Query<
        (
            &Instance,
            Ref<Transform>,
            Option<Ref<BasePart>>,
            Option<Ref<Tags>>,
            Option<&crate::spawn::MeshSource>,
            &mut BinaryEcsInstance,
        ),
        Or<(Changed<Transform>, Changed<BasePart>, Changed<Tags>)>,
    >,
) {
    if load_in_progress.active {
        return;
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
        let props_changed = base
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
            instance,
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

        // Morton key is position-derived: a move must delete the old key
        // before writing the new one (a same-cell move re-writes the same
        // key, which is harmless).
        if new_pos != bin.morton_pos {
            active_db::delete_instance_core(bin.stored_id, bin.morton_pos);
        }
        if active_db::put_instance_core(bin.stored_id, new_pos, &encoded) {
            bin.morton_pos = new_pos;
            bin.last_rot = new_rot;
            bin.last_scale = new_scale;
        }
    }
}

/// Register the binary-ECS load + save systems. Called from
/// [`super::world_db_plugin::WorldDbPlugin`].
pub fn register(app: &mut App) {
    app.init_resource::<BinaryEcsLoadLatch>().add_systems(
        Update,
        (load_binary_ecs_instances, mirror_binary_ecs_changes).chain(),
    );
}
