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
use eustress_common::class_registry::{ClassRegistry, ClassSpawner};
use eustress_common::classes::{BasePart, ClassName, Instance};
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
    pub(crate) fn from_core(stored_id: u64, core: &ArchInstanceCore) -> Self {
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

/// Marks a binary-ECS entity whose typed config component(s) were edited
/// in place (NOT via `Transform`/`BasePart`/`Tags`/`Instance`, which the
/// mirror already change-detects). Inserting this is the seam that makes
/// the save mirror re-bake + persist an edit to e.g. `UICorner` or
/// `AudioReverb` — `Added<BinaryDirty>` is part of the mirror's filter.
///
/// Insert it via [`mark_binary_dirty`] from any surface that mutates a
/// class-specific component (the future Properties-panel `apply_edit`
/// dispatch, MCP `ecs.update` of a typed field, a script write-back). The
/// mirror removes it after persisting so the next edit re-triggers.
///
/// ## Why a marker (not 106 `Changed<T>` filter terms)
///
/// There are ~106 typed config component types (`UICorner`, `AudioReverb`,
/// `VectorForce`, `Tool`, …); enumerating them in the mirror's `Or<>`
/// filter is neither expressible (tuple arity) nor maintainable. One
/// marker the mirror watches generalises to every present and future
/// class for the cost of one `commands.insert` at the edit site.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct BinaryDirty;

/// Flag a binary-ECS entity's typed config component(s) as edited so the
/// save mirror re-bakes its core next frame. No-op-safe to call on any
/// entity (the mirror simply ignores it on non-binary entities). Call
/// this AFTER mutating a class-specific component in place.
pub fn mark_binary_dirty(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).insert(BinaryDirty);
}

/// Build an `InstanceDefinition` from an entity's live cross-cutting
/// components (the fields EVERY class shares: identity, transform,
/// part-render props, tags). This is the class-AGNOSTIC half; the typed
/// config fields (`UICorner.corner_radius`, …) are folded in separately
/// by [`fold_spawner_properties`] on the `&World` path.
fn def_from_components(
    instance: &Instance,
    transform: &Transform,
    base: Option<&BasePart>,
    tags: Option<&Tags>,
    mesh: &str,
) -> InstanceDefinition {
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
                physics: None,
            }
        })
        .unwrap_or_default();

    InstanceDefinition {
        nuclear: None,
        plasma: None,
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
    }
}

/// Build an `InstanceDefinition` from an entity's live components, then
/// bake it to an `ArchInstanceCore` via the tested
/// [`arch_instance::instance_to_arch`]. Used by the save mirror so the
/// bytes match what the load path produced — and by the bridge
/// `entity.read` handler to project a RESIDENT entity's live state.
///
/// NOTE: this is the class-AGNOSTIC baker — it sees only the cross-cutting
/// components, so an entity's typed config fields are NOT included. Callers
/// that have `&World` should prefer [`core_from_entity`], which additionally
/// folds in the per-class properties (so the round-trip is lossless for the
/// Wave 6/7 config classes). This thin wrapper stays for the callers that
/// hold plain component refs (the bridge `entity.read`, `promote`) where the
/// entities are bare primitives carrying no typed config component.
pub(crate) fn core_from_components(
    instance: &Instance,
    transform: &Transform,
    base: Option<&BasePart>,
    tags: Option<&Tags>,
    mesh: &str,
) -> ArchInstanceCore {
    let def = def_from_components(instance, transform, base, tags, mesh);
    arch_instance::instance_to_arch(&def)
}

/// Fold an entity's class-specific config fields into `def.extra` so
/// [`arch_instance::instance_to_arch`] bakes them into the core's cold
/// tail (under the reserved `__extra` key) and
/// [`arch_instance::arch_to_instance`] restores them — the SAME working
/// round-trip the TOML save path relies on, reused for binary save.
///
/// For the entity's [`ClassName`], if a [`ClassSpawner`] is registered, we
/// call its `export_to_toml(world, entity)` (the inverse of
/// `import_from_toml`, which emits a `[properties]` sub-table of the
/// class's fields) and copy that `[properties]` table verbatim into
/// `def.extra["properties"]`. We deliberately take ONLY `[properties]`:
/// the spawner's `[metadata]` is already captured by the typed
/// `Instance`-derived fields, so re-folding it would duplicate (and risk
/// drifting) the identity/name. Anything outside `[properties]` a spawner
/// might emit (rare) is ignored here — `[properties]` is the universal
/// home for class payload across every Wave 3–7 spawner.
///
/// No-op (leaves `def.extra` untouched) when no spawner is registered for
/// the class, the class name is unparsable, or the export carries no
/// `[properties]` table — i.e. bare primitives stay byte-identical to the
/// pre-fold baker.
fn fold_spawner_properties(
    def: &mut InstanceDefinition,
    world: &World,
    entity: Entity,
    registry: &ClassRegistry,
) {
    let Ok(class) = ClassName::from_str(&def.metadata.class_name) else {
        return;
    };
    let Some(spawner) = registry.get(class) else {
        return;
    };
    let exported = spawner.export_to_toml(world, entity);
    if let Some(toml::Value::Table(props)) = exported.get("properties") {
        if !props.is_empty() {
            def.extra
                .insert("properties".to_string(), toml::Value::Table(props.clone()));
        }
    }
}

/// `&World` baker — like [`core_from_components`] but ALSO folds the
/// entity's typed config-component fields into the core via the registered
/// spawner's `export_to_toml` (see [`fold_spawner_properties`]). This is
/// the lossless save path for the Wave 6/7 config classes: their
/// class-specific fields now survive a binary save instead of being
/// dropped. The save mirror uses this; on decode,
/// [`arch_instance::arch_to_instance`] restores `def.extra["properties"]`.
pub(crate) fn core_from_entity(
    world: &World,
    entity: Entity,
    registry: &ClassRegistry,
    instance: &Instance,
    transform: &Transform,
    base: Option<&BasePart>,
    tags: Option<&Tags>,
    mesh: &str,
) -> ArchInstanceCore {
    let mut def = def_from_components(instance, transform, base, tags, mesh);
    fold_spawner_properties(&mut def, world, entity, registry);
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
///
/// ## M0 spawn-cost breakdown (env-gated)
///
/// When `EUSTRESS_PROFILE` is armed, the three sub-steps —
/// `decode_instance_core` (rkyv deserialize), `arch_to_instance` (the
/// InstanceDefinition reconstruction), and `spawn_instance` (the
/// asset/material/mesh + Bevy-command work) — are wall-timed and folded
/// into process-global atomics by [`spawn_cost`]. A one-line summary is
/// emitted at the load-settle point (see [`spawn_cost::log_summary`]),
/// telling M5 whether load is decode-bound (→ parallel rkyv) or
/// spawn_instance-bound (→ defer/budget, not parallelize). Off by default:
/// one relaxed `OnceLock` read per call when unarmed.
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
    // M0: only pay the clock cost when the profiler env knob is armed.
    let timing = spawn_cost::armed();

    let t0 = timing.then(std::time::Instant::now);
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
    let t1 = timing.then(std::time::Instant::now);

    let marker = BinaryEcsInstance::from_core(stored_id, &arch);
    let synthetic = synthetic_path(space_root, &arch.class_name, stored_id);
    let def = arch_instance::arch_to_instance(&arch);
    let t2 = timing.then(std::time::Instant::now);

    let entity = spawn_instance(
        commands,
        asset_server,
        materials,
        material_registry,
        mesh_cache,
        synthetic,
        def,
    );
    let t3 = timing.then(std::time::Instant::now);

    // P2 two-tier: every binary part spawned through this shared path
    // (boot-load + residency streaming + select-to-stream-in) is COLD by
    // default — it must render/be-selectable, but it is not user-edited, so it
    // is excluded from the hot edit-reactive Update systems via
    // `Without<ColdStreamed>`. `Instance` (from `spawn_instance`) and this
    // marker land in the SAME command flush, so the entity already carries
    // `ColdStreamed` the frame its `Added<Instance>` first fires — change-queue
    // and lighting hydration skip it from frame one. Promotion removes the
    // marker so an edited part rejoins those systems. NOTE: the authoritative
    // promotion seam (`sync_selection_components`) only removes `ColdStreamed`
    // inside its `Without<Selected>` query, so a caller that DIRECT-inserts
    // `Selected` (like `sys_stream_in_on_select`) bypasses it and MUST remove
    // `ColdStreamed` itself — that caller does so at both of its insert sites.
    commands
        .entity(entity)
        .insert((marker, ChildOf(workspace), eustress_common::classes::ColdStreamed));

    // Fold the three sub-step durations into the process-global accumulators.
    if let (Some(t0), Some(t1), Some(t2), Some(t3)) = (t0, t1, t2, t3) {
        spawn_cost::record(
            t1.duration_since(t0), // decode_instance_core
            t2.duration_since(t1), // arch_to_instance (+ marker/path build)
            t3.duration_since(t2), // spawn_instance
        );
    }
    Some(entity)
}

/// M0 (diagnostics) — process-global spawn-cost accumulators for
/// [`spawn_binary_core`]'s three sub-steps. Mirrors the arming pattern in
/// `crate::profiler` / `space::load_phase`: dormant until `EUSTRESS_PROFILE`
/// is set, one relaxed `OnceLock` read per call when off, never reads the
/// clock or formats a string unless armed.
///
/// The summary is emitted by [`log_summary`], hooked at the load-settle
/// point alongside the existing LOAD-PHASE `eager-spawn-complete` mark, so
/// it reports exactly the boot-load / first-fill spawn population. It is
/// idempotent per load: the totals are reset whenever a fresh load is
/// stamped via [`reset`].
pub(crate) mod spawn_cost {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Duration;

    /// Read `EUSTRESS_PROFILE` once — same knob the phase profiler + the
    /// load-phase milestones read, so a single env var arms all three.
    pub(crate) fn armed() -> bool {
        static ARMED: OnceLock<bool> = OnceLock::new();
        *ARMED.get_or_init(|| {
            std::env::var_os("EUSTRESS_PROFILE")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        })
    }

    // Nanosecond accumulators (saturating into u64 ns ≈ 584 years of headroom)
    // plus a spawn count. Relaxed ordering: these are pure counters, never a
    // happens-before edge for other state.
    static DECODE_NS: AtomicU64 = AtomicU64::new(0);
    static ARCH_NS: AtomicU64 = AtomicU64::new(0);
    static SPAWN_NS: AtomicU64 = AtomicU64::new(0);
    static COUNT: AtomicU64 = AtomicU64::new(0);

    /// Fold one spawn's three sub-step durations into the accumulators.
    /// Caller already gated on [`armed`]; this just adds.
    pub(crate) fn record(decode: Duration, arch: Duration, spawn: Duration) {
        DECODE_NS.fetch_add(decode.as_nanos() as u64, Ordering::Relaxed);
        ARCH_NS.fetch_add(arch.as_nanos() as u64, Ordering::Relaxed);
        SPAWN_NS.fetch_add(spawn.as_nanos() as u64, Ordering::Relaxed);
        COUNT.fetch_add(1, Ordering::Relaxed);
    }

    /// Reset all accumulators (a fresh Space load is starting). Cheap; safe
    /// to call unconditionally — no-op effect when nothing has accumulated.
    pub(crate) fn reset() {
        DECODE_NS.store(0, Ordering::Relaxed);
        ARCH_NS.store(0, Ordering::Relaxed);
        SPAWN_NS.store(0, Ordering::Relaxed);
        COUNT.store(0, Ordering::Relaxed);
    }

    /// Emit the one-line `SPAWN-COST: decode=… arch=… spawn=… n=…` summary
    /// (total ms per sub-step + spawn count) when armed and at least one
    /// core was spawned. Silent (one `OnceLock` read) when unarmed.
    ///
    /// Hooked at the load-settle point so it reflects the boot-load /
    /// first-fill spawn population; M5 reads the dominant sub-step to decide
    /// parallel-decode vs defer/budget.
    pub(crate) fn log_summary() {
        if !armed() {
            return;
        }
        let n = COUNT.load(Ordering::Relaxed);
        if n == 0 {
            return; // no spawns this load — don't emit a bogus all-zero line
        }
        let to_ms = |ns: u64| ns as f64 / 1.0e6;
        let decode = to_ms(DECODE_NS.load(Ordering::Relaxed));
        let arch = to_ms(ARCH_NS.load(Ordering::Relaxed));
        let spawn = to_ms(SPAWN_NS.load(Ordering::Relaxed));
        bevy::log::info!(
            target: "eustress_engine::world_db",
            "SPAWN-COST: decode={decode:.1}ms arch={arch:.1}ms spawn={spawn:.1}ms n={n}"
        );
    }
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
        // Direct-insert bypasses the selection driver's promotion seam, so
        // remove `ColdStreamed` here — otherwise this selected-to-edit part
        // stays cold and its edits emit no deltas / never refresh the panels.
        commands
            .entity(entity)
            .insert(crate::selection_box::Selected)
            .remove::<eustress_common::classes::ColdStreamed>();
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
            // manager selection (keeps it pinned until deselect). Remove
            // `ColdStreamed` here too: spawn_binary_core inserted it, and this
            // direct `Selected` insert bypasses the selection driver's promotion
            // seam (which only fires inside its `Without<Selected>` query), so
            // without this the just-selected part would stay cold and its edits
            // would emit no deltas / never refresh the panels.
            commands
                .entity(entity)
                .insert(crate::selection_box::Selected)
                .remove::<eustress_common::classes::ColdStreamed>();
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
    // HLOD shares the streaming gate: merged-cell proxies only run for a
    // large Space (the same condition that turns residency streaming on).
    mut hlod: ResMut<super::hlod::HlodState>,
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
        // HLOD on for the large Space: every non-empty cell renders as ONE
        // persistent merged proxy so the WHOLE map draws, while residency
        // keeps only the near ring live (HLOD hides the proxies it owns). The
        // one-time non-empty-cell enumeration is armed/re-armed by HLOD's own
        // SpaceRoot-change reset (`sys_hlod_reset_on_space_change`), so just
        // flipping `enabled` here is enough to start it for this Space.
        hlod.enabled = true;
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
    hlod.enabled = false; // small Space: everything boot-loaded → no proxies
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

/// The mirror's change-detection filter. `Changed<Instance>` catches
/// renames (the display name lives on `Instance.name`, which the baker
/// persists). `Added<BinaryDirty>` catches typed config-component edits
/// (UICorner, AudioReverb, …) that touch none of the cross-cutting
/// components — see [`mark_binary_dirty`]. `Without<SpawnedDuringPlayMode>`
/// drops ephemeral play-spawned parts (never persisted, nothing to clean
/// up on Stop). Hoisted to a named alias so the cached `SystemState` type
/// in [`mirror_binary_ecs_changes`] stays legible.
type MirrorChangeFilter = (
    Or<(
        Changed<Transform>,
        Changed<BasePart>,
        Changed<Tags>,
        Changed<Instance>,
        Added<BinaryDirty>,
    )>,
    Without<crate::play_mode::SpawnedDuringPlayMode>,
);

/// One gate-passing entity the mirror decided to persist this frame:
/// its `Entity` plus the recomputed transform baseline to write back into
/// its [`BinaryEcsInstance`] once the core is persisted. Collected under an
/// immutable world borrow so the subsequent `&World` bake (which reads
/// arbitrary typed components via the spawner) does not conflict.
struct PendingMirror {
    entity: Entity,
    stored_id: u64,
    old_pos: [f32; 3],
    new_pos: [f32; 3],
    new_rot: [f32; 4],
    new_scale: [f32; 3],
    uuid: String,
    mesh: String,
}

/// Persist live edits to binary-ECS entities back into the `entities`
/// partition. Value-gated so the Avian same-value `Changed<Transform>`
/// storm does zero work on idle anchored parts.
///
/// ## Why this is an exclusive `&World` system
///
/// Persisting the Wave 6/7 config classes losslessly means re-baking each
/// changed entity through its spawner's `export_to_toml(world, entity)`
/// (see [`core_from_entity`] / [`fold_spawner_properties`]), which needs
/// `&World` — a plain `Query` cannot also borrow `&World`. So the mirror
/// runs exclusively and drives change-detection through a cached
/// [`QueryState`] instead of a system-param `Query`. The work is the same
/// value-gated set as before; at scale the residency manager bounds the
/// resident (hence query-visited) set, so the exclusive single-threaded
/// cost stays proportional to *edited* entities, not world size.
///
/// The pass is three sequential borrows (no aliasing): (1) immutable —
/// walk the change-filtered query, value-gate, and collect
/// [`PendingMirror`]s; (2) immutable — bake each via `core_from_entity`
/// (reads the typed config component through the registry) + encode +
/// write to the DB; (3) mutable — advance the surviving entities'
/// [`BinaryEcsInstance`] baselines and clear their [`BinaryDirty`] markers.
#[allow(clippy::type_complexity)]
fn mirror_binary_ecs_changes(
    world: &mut World,
    // Cached across runs so the change-detection ticks persist (an
    // exclusive system cannot take a system-param `Query`; `Local` is the
    // supported way to hold a `SystemState` between exclusive invocations).
    mut query_state: Local<
        bevy::ecs::system::SystemState<
            Query<
                'static,
                'static,
                (
                    Entity,
                    Ref<'static, Instance>,
                    Ref<'static, Transform>,
                    Option<Ref<'static, BasePart>>,
                    Option<Ref<'static, Tags>>,
                    Option<&'static crate::spawn::MeshSource>,
                    &'static BinaryEcsInstance,
                    Has<BinaryDirty>,
                ),
                MirrorChangeFilter,
            >,
        >,
    >,
) {
    // Cheap resource gates first (read straight off the world — an
    // exclusive system has no system-param injection).
    if world
        .get_resource::<LoadInProgress>()
        .map(|l| l.active)
        .unwrap_or(false)
    {
        return;
    }
    // Play mode is ephemeral: physics moves bodies and scripts spawn parts
    // that must NOT persist. Bail entirely while playing so a `Stop` always
    // restores the pre-play Fjall state (no leaked / mutated cores).
    if let Some(state) = world.get_resource::<State<crate::play_mode::PlayModeState>>() {
        if *state.get() != crate::play_mode::PlayModeState::Editing {
            return;
        }
    }
    if !active_db::is_active() {
        return;
    }

    // ── Borrow 1 (immutable): change-detect + value-gate + collect. ──
    let mut pending: Vec<PendingMirror> = Vec::new();
    {
        let q = query_state.get(world);
        for (entity, instance, transform, base, tags, mesh_src, bin, dirty) in q.iter() {
            let new_pos = transform.translation.to_array();
            let new_rot = [
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ];
            let new_scale = transform.scale.to_array();

            // Cheap value-gate FIRST (no alloc). The Avian transform-sync
            // re-writes anchored bodies to the SAME value every frame,
            // tripping `Changed<Transform>`; skip when nothing actually
            // moved AND no part property changed. `Ref::is_changed` on
            // BasePart/Tags is false during that storm (Avian touches
            // Transform, not BasePart).
            let tf_changed = new_pos != bin.morton_pos
                || new_rot != bin.last_rot
                || new_scale != bin.last_scale;
            // `!is_added()` skips the spawn frame: a just-loaded entity's
            // BasePart/Tags read as "changed" the frame after spawn, but
            // the data already lives in the DB (we loaded it FROM there), so
            // persisting again would be a redundant write spike across the
            // whole grid. A genuine later edit has is_changed && !is_added.
            // A rename (or any other Instance edit) is a genuine change; the
            // `!is_added()` guard skips the post-spawn frame like
            // BasePart/Tags.
            let inst_changed = instance.is_changed() && !instance.is_added();
            // `dirty` (a `BinaryDirty` marker present this frame) forces a
            // re-bake regardless of the transform/part gates: it's the
            // explicit "a typed config field was edited" signal, and the
            // changed field lives on a component the value-gate never reads.
            let props_changed = inst_changed
                || dirty
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

            pending.push(PendingMirror {
                entity,
                stored_id: bin.stored_id,
                old_pos: bin.morton_pos,
                new_pos,
                new_rot,
                new_scale,
                uuid: instance.uuid.clone(),
                mesh: mesh_src
                    .map(|m| m.path.clone())
                    .unwrap_or_else(|| FALLBACK_MESH.to_string()),
            });
        }
    }
    if pending.is_empty() {
        return;
    }

    // ── Borrow 2 (immutable): bake (incl. typed config fields via the
    // spawner) + encode + persist. Collect the entities whose write
    // succeeded so borrow 3 advances only their baselines. ──
    // The registry is normally present (the engine mounts `ClassRegistryPlugin`
    // alongside `WorldDbPlugin`). If a stripped-down/headless config runs the
    // mirror without it, fall back to the class-agnostic baker so we still
    // persist transforms instead of panicking — the typed-field fold is simply
    // skipped there.
    let registry = world.get_resource::<ClassRegistry>();
    let mut persisted: Vec<usize> = Vec::with_capacity(pending.len());
    for (idx, p) in pending.iter().enumerate() {
        let entity_ref = world.entity(p.entity);
        let Some(instance) = entity_ref.get::<Instance>() else {
            continue;
        };
        let Some(transform) = entity_ref.get::<Transform>() else {
            continue;
        };
        let base = entity_ref.get::<BasePart>();
        let tags = entity_ref.get::<Tags>();

        let core = match registry {
            Some(reg) => core_from_entity(
                world, p.entity, reg, instance, transform, base, tags, &p.mesh,
            ),
            None => core_from_components(instance, transform, base, tags, &p.mesh),
        };
        let encoded = match encode_instance_core(&core) {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    target: "eustress_engine::world_db",
                    error = %e, stored_id = p.stored_id,
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
        let uuid_bytes = eustress_common::instance_create::uuid_hex_to_bytes(&p.uuid);
        if active_db::mirror_binary_core(
            p.stored_id,
            uuid_bytes.as_ref(),
            p.old_pos,
            p.new_pos,
            &encoded,
        ) {
            persisted.push(idx);
        }
    }

    // ── Borrow 3 (mutable): advance value-gate baselines + clear the
    // per-edit dirty markers for everything we persisted. ──
    for &idx in &persisted {
        let p = &pending[idx];
        if let Ok(mut em) = world.get_entity_mut(p.entity) {
            if let Some(mut bin) = em.get_mut::<BinaryEcsInstance>() {
                bin.morton_pos = p.new_pos;
                bin.last_rot = p.new_rot;
                bin.last_scale = p.new_scale;
            }
            // Drop the one-shot edit marker so a later typed edit re-fires.
            em.remove::<BinaryDirty>();
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
        // HLOD (merged-cell proxies) — the whole-map render. Shares the
        // streaming gate with residency (enabled together for a large Space).
        .init_resource::<super::hlod::HlodState>()
        .init_resource::<super::hlod::HlodConfig>()
        // Order is load-bearing (Phase 2 risk R1): boot-load decides the
        // streaming mode → residency loads camera-local cells → HLOD plans
        // (one-time enumeration + build-queue drain; camera-independent now)
        // → the mirror PERSISTS edits → residency evicts far cells → HLOD
        // toggles proxy VISIBILITY (hide the cells residency owns, show the
        // rest). The mirror MUST precede residency eviction so an edited
        // entity's core is written before it is despawned. HLOD's visibility
        // pass is last so its keep-box is resolved against the freshest camera
        // box (and after residency has (re)spawned a now-near cell's
        // individuals, so the proxy hides in lockstep — no double-render, and
        // never a gap since the proxy is only HIDDEN, never despawned).
        .add_systems(
            Update,
            (
                load_binary_ecs_instances,
                super::residency::sys_residency_load,
                super::hlod::sys_hlod_plan,
                mirror_binary_ecs_changes,
                super::residency::sys_residency_evict,
                super::hlod::sys_hlod_visibility,
            )
                .chain(),
        )
        // HLOD merge-collect (polls finished worker builds + spawns proxies)
        // and the Space-switch teardown run outside the ordered chain — they
        // have no ordering constraint with the load/evict lifecycle (a proxy
        // spawned this frame can't be evicted before next frame's plan, and
        // the reset only fires on a genuine SpaceRoot change).
        .add_systems(
            Update,
            (
                super::hlod::sys_hlod_collect,
                super::hlod::sys_hlod_reset_on_space_change,
            ),
        )
        .add_systems(Update, drain_pending_binary_recreate)
        // Phase 4 — select-to-stream-in for the virtual DB Explorer. Runs
        // before eviction so a just-streamed entity is registered/pinned in
        // the same frame's selection pass (the spawn itself is deferred, so
        // it can't be evicted this frame regardless).
        .add_systems(
            Update,
            sys_stream_in_on_select.before(super::residency::sys_residency_evict),
        )
        // M0 (diagnostics): periodic live entity-count-by-type log
        // (binary vs streaming vs total). Env-gated on EUSTRESS_PROFILE;
        // returns immediately (one OnceLock read) in a normal run.
        .add_systems(Update, super::residency::sys_entity_count_diag);
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

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::class_registry::ClassRegistry;
    use eustress_common::classes::{ClassName, Instance, UICorner};
    use eustress_common::ui_types::UDim;

    /// THE binary-persistence-gap regression gate (IMPORT_STORAGE_AND_
    /// PORTABILITY.md §"BINARY-PERSISTENCE GAP"): a typed config-component
    /// field (`UICorner.corner_radius`) must SURVIVE a binary save. We bake
    /// the live entity via [`core_from_entity`] (the `&World` baker the save
    /// mirror uses), round-trip the bytes through rkyv, then `arch_to_instance`
    /// and assert the field is present in `def.extra` — exactly the cold-tail
    /// path the load side reads. Before the fix the baker dropped it
    /// (`extra: HashMap::new()`), so this asserted `[0.0, 8.0]` was lost.
    #[test]
    fn typed_config_field_survives_binary_save_roundtrip() {
        // Minimal world: spawn a UICorner-classed entity carrying a non-default
        // corner_radius, with the cross-cutting Instance the export reads.
        let mut world = World::new();
        let entity = world
            .spawn((
                Instance {
                    name: "Round".to_string(),
                    class_name: ClassName::UICorner,
                    archivable: true,
                    id: 0,
                    uuid: String::new(),
                    ai: false,
                },
                Transform::default(),
                UICorner {
                    corner_radius: UDim::new(0.0, 8.0),
                },
            ))
            .id();

        // Registry holding the real UICorner spawner — `core_from_entity`
        // dispatches to its `export_to_toml` to harvest the [properties] table.
        let mut registry = ClassRegistry::default();
        registry.register(crate::spawners::ui_layout::UICornerSpawner);

        // Re-read the cross-cutting refs (immutable borrows; `core_from_entity`
        // also borrows &world immutably — no conflict).
        let instance = world.get::<Instance>(entity).unwrap().clone();
        let transform = *world.get::<Transform>(entity).unwrap();

        // Bake via the &World save path (folds the spawner's typed properties).
        let core = core_from_entity(
            &world,
            entity,
            &registry,
            &instance,
            &transform,
            None,
            None,
            "", // UICorner is non-visual: no mesh
        );

        // Full binary round-trip: encode → decode (proves it survives the
        // rkyv archive the Fjall `entities` partition actually stores).
        let bytes = encode_instance_core(&core).expect("encode core");
        let core2 = decode_instance_core(&bytes).expect("decode core");
        assert_eq!(core, core2, "rkyv round-trip must be byte-stable");

        // Decode side: arch_to_instance restores def.extra (EXTRA_KEY tail).
        let def = arch_instance::arch_to_instance(&core2);

        // The class-specific field must be present under
        // def.extra["properties"]["corner_radius"] = [scale, offset].
        let props = def
            .extra
            .get("properties")
            .expect("def.extra must carry the spawner [properties] table");
        let corner = props
            .get("corner_radius")
            .and_then(|v| v.as_array())
            .expect("corner_radius must survive in [properties]");
        let scale = corner.first().and_then(|v| v.as_float()).unwrap();
        let offset = corner.get(1).and_then(|v| v.as_float()).unwrap();
        assert_eq!(scale, 0.0, "corner_radius.scale must survive binary save");
        assert_eq!(offset, 8.0, "corner_radius.offset must survive binary save");
    }

    /// A bare primitive (no registered typed config component) must come out
    /// of the new `&World` path with NO folded `[properties]` — i.e. the fold
    /// is a strict no-op when no spawner is registered, preserving the
    /// pre-fold byte shape (and create==load parity) for the 10M-part case.
    ///
    /// (We assert the structural invariant — `def.extra` has no `properties`
    /// key — rather than `assert_eq!` against `core_from_components`: both
    /// bakers stamp an independent `chrono::Utc::now()` into the metadata
    /// tail, so a whole-core equality could flake on a clock tick between the
    /// two calls. The hot typed fields ARE compared for equality below.)
    #[test]
    fn bare_primitive_unchanged_by_fold() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Instance {
                    name: "Brick".to_string(),
                    class_name: ClassName::Part,
                    archivable: true,
                    id: 0,
                    uuid: String::new(),
                    ai: false,
                },
                Transform::from_xyz(1.0, 2.0, 3.0),
            ))
            .id();
        // Empty registry: no spawner for Part here → fold must do nothing.
        let registry = ClassRegistry::default();
        let instance = world.get::<Instance>(entity).unwrap().clone();
        let transform = *world.get::<Transform>(entity).unwrap();

        let core = core_from_entity(
            &world,
            entity,
            &registry,
            &instance,
            &transform,
            None,
            None,
            "parts/block.glb",
        );
        // Hot typed fields are exactly what the class-agnostic baker produces.
        let baseline = core_from_components(&instance, &transform, None, None, "parts/block.glb");
        assert_eq!(core.class_name, baseline.class_name);
        assert_eq!(core.mesh, baseline.mesh);
        assert_eq!(core.t, baseline.t);

        // And no `[properties]` snuck into the cold tail.
        let def = arch_instance::arch_to_instance(&core);
        assert!(
            !def.extra.contains_key("properties"),
            "fold must be a no-op when no typed config spawner is registered"
        );
    }
}
