//! Phase 3.5 — explicit promote / demote between the binary-ECS and
//! FileSystem representations.
//!
//! **Promote** materializes a bare binary-ECS Part (rkyv `ArchInstanceCore` in
//! Fjall) into an on-disk `Workspace/<Name>/_instance.toml` folder, so it
//! becomes path-addressable, hand-editable, file-attachable, and Copy-Path
//! yields a real disk path. **Demote** folds a (bare, artifact-free) TOML
//! folder back into a binary core. Both are EXPLICIT (Phase 3 decided an
//! MCP/bridge edit keeps an entity binary; you opt into disk) and operate on a
//! RESIDENT entity in place: the SAME live `Entity` keeps its
//! `Instance`/`Transform`/`BasePart`/`Mesh3d`/`MeshMaterial3d`/`Tags` — only the
//! persistence backing (marker components + DB stores + disk folder) changes,
//! so there is ZERO visual change (constraint C2).
//!
//! The auto-demote-on-last-artifact-removal trigger is deferred (it races the
//! file-watcher's atomic-write replace pattern); these are the explicit
//! mechanisms the bridge `entity.promote`/`entity.demote` RPCs call.

#![cfg(feature = "world-db")]

use std::path::PathBuf;

use bevy::prelude::*;
use eustress_common::classes::{BasePart, Instance};
use eustress_common::Tags;
use eustress_worlddb::encode_instance_core;

use super::active_db;
use super::arch_instance::arch_to_instance;
use super::file_loader::{FileMetadata, FileType, LoadedFromFile, SpaceFileRegistry};
use super::file_watcher::RecentlyWrittenFiles;
use super::gui_loader::write_atomic;
use super::instance_create::{create_instance, InstanceOverrides};
use super::instance_loader::InstanceFile;
use super::representation::{folder_has_attached_artifacts, representation_for_part, Representation};
use super::world_db_binary::{core_from_components, BinaryEcsInstance};
use super::SpaceRoot;
use crate::spawn::MeshSource;

/// How a promote/demote names its target entity.
pub enum EntityRef {
    /// A live `Entity` (e.g. the selected one, from the Slint "Export to disk").
    Entity(Entity),
    /// A persistent uuid hex (the bridge/MCP path). Must be RESIDENT.
    Uuid(String),
}

/// The fields a promote/demote needs, snapshotted out of the live entity so the
/// world borrow is released before the fs/DB work + the structural swap.
struct Snapshot {
    instance: Instance,
    transform: Transform,
    base: Option<BasePart>,
    tags: Option<Tags>,
    mesh: String,
    is_binary: bool,
    /// Present iff the entity has the binary marker (promote source).
    stored_id: Option<u64>,
    morton_pos: Option<[f32; 3]>,
    /// Present iff the entity has an `InstanceFile` (demote source / FS state).
    instance_file_toml: Option<PathBuf>,
}

fn resolve_entity(world: &mut World, target: &EntityRef) -> Result<Entity, String> {
    match target {
        EntityRef::Entity(e) => {
            if world.get_entity(*e).is_ok() {
                Ok(*e)
            } else {
                Err("target entity is not live (despawned?)".into())
            }
        }
        EntityRef::Uuid(u) => {
            let mut q = world.query::<(Entity, &Instance)>();
            q.iter(world)
                .find(|(_, i)| &i.uuid == u)
                .map(|(e, _)| e)
                .ok_or_else(|| {
                    format!("uuid {u} is not resident — stream it in first (select its DB row), then retry")
                })
        }
    }
}

fn snapshot(world: &World, entity: Entity) -> Result<Snapshot, String> {
    let e = world.get_entity(entity).map_err(|_| "entity not live".to_string())?;
    let instance = e
        .get::<Instance>()
        .ok_or("entity has no Instance component")?
        .clone();
    let transform = e.get::<Transform>().copied().unwrap_or_default();
    let base = e.get::<BasePart>().cloned();
    let tags = e.get::<Tags>().cloned();
    let mesh = e.get::<MeshSource>().map(|m| m.path.clone()).unwrap_or_default();
    let bin = e.get::<BinaryEcsInstance>();
    let inst_file = e.get::<InstanceFile>();
    Ok(Snapshot {
        instance,
        transform,
        base,
        tags,
        mesh,
        is_binary: bin.is_some(),
        stored_id: bin.map(|b| b.stored_id),
        morton_pos: bin.map(|b| b.morton_pos),
        instance_file_toml: inst_file.map(|f| f.toml_path.clone()),
    })
}

/// Space-relative, forward-slashed path for a DB key.
fn rel_of(space_root: &std::path::Path, abs: &std::path::Path) -> Option<String> {
    abs.strip_prefix(space_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// Materialize a binary-ECS entity into an on-disk `Workspace/<Name>/` TOML
/// folder. Returns the new folder path. Errors (with zero mutation) if the
/// target isn't resident, isn't binary-backed (already FileSystem / custom
/// mesh), or no DB is active.
pub fn promote_to_filesystem(world: &mut World, target: EntityRef) -> Result<PathBuf, String> {
    if !active_db::is_active() {
        return Err("no active world DB — promote needs a converted (.eustress) Space".into());
    }
    let entity = resolve_entity(world, &target)?;
    let snap = snapshot(world, entity)?;
    if !snap.is_binary {
        return Err("entity is not binary-backed (already a FileSystem/custom-mesh entity)".into());
    }
    let stored_id = snap.stored_id.ok_or("binary entity missing stored_id")?;
    let morton_pos = snap.morton_pos.unwrap_or([0.0; 3]);
    let uuid_hex = snap.instance.uuid.clone();
    let uuid_bytes = eustress_common::instance_create::uuid_hex_to_bytes(&uuid_hex)
        .ok_or("entity uuid is malformed — refusing to promote (identity would be lost)")?;

    // Project live components → core → editable InstanceDefinition (pin the uuid).
    let core = core_from_components(
        &snap.instance,
        &snap.transform,
        snap.base.as_ref(),
        snap.tags.as_ref(),
        &snap.mesh,
    );
    let class = core.class_name.clone();
    let name = snap.instance.name.clone();
    let mut def = arch_to_instance(&core);
    def.metadata.uuid = Some(uuid_hex.clone());

    let synthetic_rel = format!("Workspace/__bin_{}_{:016x}/_instance.toml", class, stored_id);

    // ── Disk phase (no world borrow) ──
    let space_root = world
        .get_resource::<SpaceRoot>()
        .ok_or("no SpaceRoot resource")?
        .0
        .clone();
    let dest_dir = space_root.join("Workspace");
    let t = snap.transform;
    let overrides = InstanceOverrides {
        display_name: Some(name.clone()),
        position: Some(t.translation),
        rotation: Some([t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w]),
        scale: Some(t.scale),
        color_rgba: snap.base.as_ref().map(|b| {
            let c = b.color.to_srgba();
            [c.red, c.green, c.blue, c.alpha]
        }),
        material: snap.base.as_ref().map(|b| b.material_name.clone()).filter(|s| !s.is_empty()),
        anchored: snap.base.as_ref().map(|b| b.anchored),
        can_collide: snap.base.as_ref().map(|b| b.can_collide),
        asset_mesh: if snap.mesh.is_empty() { None } else { Some(snap.mesh.clone()) },
        uuid: Some(uuid_hex.clone()),
        ..Default::default()
    };
    let created = create_instance(&dest_dir, &class, Some(&name), overrides)
        .map_err(|e| format!("create_instance failed: {e}"))?;

    // Cold-tail preservation: overwrite the template-derived TOML with the full
    // InstanceDefinition (carries material/attrs/extra a core may hold). If
    // serialization fails, the create_instance output stays (loader-valid base).
    let toml_bytes = match toml::to_string_pretty(&def) {
        Ok(full) => {
            let _ = write_atomic(&created.toml_path, full.as_bytes());
            full.into_bytes()
        }
        Err(_) => std::fs::read(&created.toml_path).unwrap_or_default(),
    };
    let real_rel = rel_of(&space_root, &created.toml_path)
        .unwrap_or_else(|| format!("Workspace/{}/_instance.toml", created.folder_name));

    // ── DB phase ── drop the binary stores (incl. the Morton core → no
    // boot-load resurrection), then register the real-path FileSystem identity.
    let core_bytes = encode_instance_core(&core).map_err(|e| format!("encode core: {e}"))?;
    active_db::delete_binary_instance(stored_id, &uuid_bytes, &class, morton_pos, &synthetic_rel);
    active_db::write_filesystem_identity(&uuid_bytes, &class, &real_rel, &core_bytes, &toml_bytes);

    // ── Live-entity swap ── pre-register the folder so the watcher's Create is
    // a no-op; flip the marker components. Entity stays live + visually unchanged.
    if let Some(mut rw) = world.get_resource_mut::<RecentlyWrittenFiles>() {
        rw.mark_written(created.toml_path.clone());
    }
    if let Some(mut reg) = world.get_resource_mut::<SpaceFileRegistry>() {
        reg.register(
            created.folder_path.clone(),
            entity,
            FileMetadata {
                path: created.folder_path.clone(),
                file_type: FileType::Directory,
                service: "Workspace".to_string(),
                name: name.clone(),
                size: 0,
                modified: std::time::SystemTime::now(),
                children: Vec::new(),
            },
        );
    }
    {
        let mut em = world.entity_mut(entity);
        em.remove::<BinaryEcsInstance>(); // stops mirror_binary_ecs_changes for this entity
        em.insert(LoadedFromFile {
            path: created.folder_path.clone(),
            file_type: FileType::Directory,
            service: "Workspace".to_string(),
        });
        em.insert(InstanceFile {
            toml_path: created.toml_path.clone(),
            mesh_path: PathBuf::new(),
            name,
        });
    }

    info!(
        target: "eustress_engine::promote",
        uuid = %uuid_hex, ?entity, folder = %created.folder_path.display(),
        "promoted binary-ECS entity → FileSystem TOML folder"
    );
    Ok(created.folder_path)
}

/// Fold a bare, artifact-free FileSystem Part folder back into a binary core.
/// Errors (zero mutation) if the target isn't resident, is already binary, is
/// file-natured / custom-mesh, or its folder still has attached artifacts.
pub fn demote_to_binary(world: &mut World, target: EntityRef) -> Result<(), String> {
    if !active_db::is_active() {
        return Err("no active world DB".into());
    }
    let entity = resolve_entity(world, &target)?;
    let snap = snapshot(world, entity)?;
    if snap.is_binary {
        return Err("entity is already binary-backed".into());
    }
    let toml_path = snap
        .instance_file_toml
        .clone()
        .ok_or("entity is not FileSystem-backed (no InstanceFile)")?;
    if toml_path.to_string_lossy().contains("__bin_") {
        return Err("entity has a synthetic (binary) InstanceFile — nothing to demote".into());
    }
    let folder = toml_path
        .parent()
        .ok_or("InstanceFile has no parent folder")?
        .to_path_buf();

    let core = core_from_components(
        &snap.instance,
        &snap.transform,
        snap.base.as_ref(),
        snap.tags.as_ref(),
        &snap.mesh,
    );
    let class = core.class_name.clone();

    // Guards: must be a binary-eligible class+mesh AND have no attached files.
    let mesh_opt = if snap.mesh.is_empty() { None } else { Some(snap.mesh.as_str()) };
    if representation_for_part(&class, mesh_opt, None) != Representation::BinaryEcs {
        return Err("class/mesh is not binary-eligible (file-natured or custom mesh)".into());
    }
    if folder_has_attached_artifacts(&folder) {
        return Err("folder still has attached file artifacts — remove them before demoting".into());
    }

    let uuid_hex = snap.instance.uuid.clone();
    let uuid_bytes = eustress_common::instance_create::uuid_hex_to_bytes(&uuid_hex)
        .ok_or("entity uuid is malformed")?;
    let stored_id = u64::from_be_bytes(
        uuid_bytes[0..8].try_into().expect("uuid is 16 bytes; [0..8] is 8"),
    );
    let pos = [
        snap.transform.translation.x,
        snap.transform.translation.y,
        snap.transform.translation.z,
    ];
    let synthetic_rel = format!("Workspace/__bin_{}_{:016x}/_instance.toml", class, stored_id);

    let space_root = world
        .get_resource::<SpaceRoot>()
        .ok_or("no SpaceRoot resource")?
        .0
        .clone();
    let real_rel = rel_of(&space_root, &toml_path).unwrap_or_default();

    // ── DB phase ── re-create the binary stores; drop the real-path identity.
    let core_bytes = encode_instance_core(&core).map_err(|e| format!("encode core: {e}"))?;
    active_db::create_binary_instance(stored_id, &uuid_bytes, &class, pos, &core_bytes, &synthetic_rel);
    active_db::remove_filesystem_identity(&real_rel);

    // ── Live-entity swap ── unregister BEFORE the disk delete so the watcher's
    // Remove finds no entity (no despawn). Re-attach the binary marker + a
    // synthetic InstanceFile (the boot-load shape).
    if let Some(mut reg) = world.get_resource_mut::<SpaceFileRegistry>() {
        reg.unregister_file(&folder);
        reg.unregister_entity(entity);
    }
    let synthetic_abs = space_root
        .join("Workspace")
        .join(format!("__bin_{}_{:016x}", class, stored_id))
        .join("_instance.toml");
    {
        let mut em = world.entity_mut(entity);
        em.remove::<LoadedFromFile>();
        em.remove::<InstanceFile>();
        // Construct the marker inline (fields are pub) rather than via
        // `BinaryEcsInstance::from_core` — that keeps this commit independent of
        // world_db_binary.rs, which a co-agent is concurrently rewriting. Same
        // value-gate baseline from_core would set.
        em.insert(BinaryEcsInstance {
            stored_id,
            morton_pos: core.t,
            last_rot: core.r,
            last_scale: core.s,
        });
        em.insert(InstanceFile {
            toml_path: synthetic_abs,
            mesh_path: PathBuf::new(),
            name: snap.instance.name.clone(),
        });
    }

    // ── Disk teardown (after unregister; best-effort) ──
    if let Err(e) = std::fs::remove_dir_all(&folder) {
        warn!(
            target: "eustress_engine::promote",
            folder = %folder.display(), error = %e,
            "demote: could not remove the disk folder (DB is already authoritative-binary)"
        );
    }
    info!(
        target: "eustress_engine::promote",
        uuid = %uuid_hex, ?entity, "demoted FileSystem entity → binary-ECS core"
    );
    Ok(())
}
