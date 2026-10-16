//! The engine bridge's tree edits: `instance.rename`, `instance.reparent`,
//! `instance.clone` and `instance.create`. The MCP `rename_instance`,
//! `reparent_instance`, `clone_instance` and `create_instance` tools call
//! them.
//!
//! Held to `properties.set`'s rules: Edit only
//! ([`super::properties::editing_only`]), named by uuid, only under a
//! content service ([`super::attributes::content_root`]), and one undo step
//! per call. The disk work is the undo system's own folder move
//! ([`crate::undo::move_folder_and_rebase`]): it guards the watcher, moves
//! the folder (or flat file), rekeys the world database, and rebases the
//! record paths of the instance and everything under it, which Studio's
//! Properties-panel rename does not do for descendants.

use bevy::prelude::*;
use serde_json::{json, Value};

use eustress_common::classes::Instance;

use super::parameters::{resolve, target_json};
use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::InstanceFile;
use crate::undo::{Action, PropertyValueSnapshot};

/// Longest instance name a rename takes, in characters.
const MAX_NAME_CHARS: usize = 100;

fn answer(req: &BridgeRequest, result: Result<Value, String>) -> BridgeResponse {
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    }
}

fn uuid_param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` names the instance by its uuid"))
}

/// An instance stored in the world database (a synthetic `__bin_` record).
fn is_binary(world: &World, entity: Entity) -> bool {
    #[cfg(feature = "world-db")]
    if world.get::<crate::space::world_db_binary::BinaryEcsInstance>(entity).is_some() {
        return true;
    }
    world.get::<InstanceFile>(entity).is_some_and(|f| f.toml_path.to_string_lossy().contains("__bin_"))
}

fn rel_of(world: &World, path: &std::path::Path) -> Option<String> {
    let root = world.get_resource::<crate::space::SpaceRoot>()?;
    Some(path.strip_prefix(&root.0).ok()?.to_string_lossy().replace('\\', "/"))
}

/// A name the caller chose for an instance: one ordinary file name (the
/// tools' sandbox rules: no separators, no ':', no Windows device name, no
/// trailing dot or space, no control character), at most
/// [`MAX_NAME_CHARS`].
pub(super) fn instance_name(raw: &str) -> Result<String, String> {
    let name = eustress_tools::sandbox::plain_name(raw).map_err(|e| e.to_string())?;
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("a name is at most {MAX_NAME_CHARS} characters"));
    }
    Ok(name)
}

/// Where a record moves when its instance is renamed `name`: a folder-form
/// instance (`Spine/_instance.toml`) renames its folder; a flat one
/// (`Spine.part.toml`) renames the file, keeping its extension chain.
pub(super) fn renamed_path(toml_path: &std::path::Path, name: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let file_name = toml_path.file_name()?.to_string_lossy().to_string();
    if file_name == "_instance.toml" {
        let folder = toml_path.parent()?;
        Some((folder.to_path_buf(), folder.parent()?.join(name)))
    } else {
        let ext = file_name.split_once('.').map(|(_, e)| e.to_string())?;
        Some((toml_path.to_path_buf(), toml_path.parent()?.join(format!("{name}.{ext}"))))
    }
}

/// Content services whose instances never reach a player.
pub(super) const SERVER_ONLY: &[&str] = &["ServerScriptService", "ServerStorage"];

/// Moving (or copying) from `from_root` into `to_root` would hand server
/// code or server-only data to every player: refused unless the call says
/// `expose_to_clients: true` (which the write log records).
pub(super) fn check_exposure(from_root: &str, to_root: &str, expose_to_clients: bool) -> Result<(), String> {
    if SERVER_ONLY.contains(&from_root) && !SERVER_ONLY.contains(&to_root) && !expose_to_clients {
        return Err(format!(
            "{from_root} is never sent to players and {to_root} is; pass expose_to_clients: true to move it there on purpose"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// instance.rename
// ---------------------------------------------------------------------------

/// `instance.rename`: `uuid`, `name`.
pub fn rename(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, do_rename(world, &req.params))
}

fn do_rename(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let uuid = uuid_param(params, "uuid")?;
    let t = resolve(world, &json!({ "uuid": uuid }))?;
    if super::attributes::CONTENT_ROOTS.contains(&t.class.as_str()) {
        return Err(format!("{} is a service and keeps its name", t.class));
    }
    let name = instance_name(params.get("name").and_then(Value::as_str).ok_or("`name` is the new name")?)?;
    let (old_name, id) = world.get::<Instance>(t.entity).map(|i| (i.name.clone(), i.id)).ok_or("not an instance")?;
    if name == old_name {
        return Ok(json!({ "instance": target_json(&t), "changed": false }));
    }
    if id == 0 {
        return Err("this instance has no undo id, so the rename could not be undone; rename it in the Explorer".to_string());
    }

    let mut moves = Vec::new();
    let mut saved_to = None;
    if is_binary(world, t.entity) {
        // The database mirror saves the name with the instance's core.
        let rel = world.get::<InstanceFile>(t.entity).and_then(|f| rel_of(world, &f.toml_path)).unwrap_or_default();
        super::attributes::content_root(&rel)?;
    } else {
        let file = world
            .get::<InstanceFile>(t.entity)
            .map(|f| f.toml_path.clone())
            .ok_or("this instance has no record of its own on disk, so the new name could not be saved")?;
        let rel = rel_of(world, &file).ok_or("the instance's record is outside the open Space")?;
        super::attributes::content_root(&rel)?;
        let (from, to) = renamed_path(&file, &name).ok_or("the instance's record has no name to change")?;
        if to.exists() {
            return Err(format!("'{}' already exists beside it", to.file_name().unwrap_or_default().to_string_lossy()));
        }
        if !crate::undo::move_folder_and_rebase(world, &from, &to) {
            return Err("the record could not be renamed on disk (the engine log says why); nothing changed".to_string());
        }
        // A record that states its own name says the new one.
        let toml = world.get::<InstanceFile>(t.entity).map(|f| f.toml_path.clone()).unwrap_or_default();
        if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&toml) {
            if def.metadata.name.is_some() {
                def.metadata.name = Some(name.clone());
                let stamp = world.get_resource::<crate::auth::AuthState>().and_then(crate::space::instance_loader::current_stamp);
                crate::space::instance_loader::write_instance_definition_signed(&toml, &mut def, stamp.as_ref())?;
            }
        }
        saved_to = rel_of(world, &toml);
        moves.push((from, to));
    }

    if let Some(mut inst) = world.get_mut::<Instance>(t.entity) {
        inst.name = name.clone();
    }
    world.entity_mut(t.entity).insert(Name::new(name.clone()));
    let rename_step = Action::ChangeProperty {
        id,
        property: "Name".to_string(),
        old_value: PropertyValueSnapshot::String(old_name),
        new_value: PropertyValueSnapshot::String(name.clone()),
    };
    let action = if moves.is_empty() {
        rename_step
    } else {
        // Undo puts the name back, then the folder: moving it back refuses,
        // and logs, if something now holds the old name; it never overwrites.
        Action::Batch { actions: vec![Action::MoveFolders { moves }, rename_step] }
    };
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(format!("Rename to {name}"), action);
    }
    info!("bridge instance.rename uuid={uuid}");
    Ok(json!({ "instance": target_json(&t), "changed": true, "name": name, "saved_to": saved_to, "undo_steps": 1 }))
}

// ---------------------------------------------------------------------------
// instance.reparent
// ---------------------------------------------------------------------------

/// `instance.reparent`: `uuid`, and the new parent as `new_parent_uuid` or
/// `new_parent_service` (a content service's root); `expose_to_clients`.
/// The instance keeps its place in the world: its local pose is rewritten
/// for the new parent.
pub fn reparent(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, do_reparent(world, &req.params))
}

/// The folder an instance's children live in, and the entity they hang from
/// (`None`: a service root, whose children sit at the scene root, as Group
/// leaves them).
fn holder(world: &mut World, params: &Value) -> Result<(std::path::PathBuf, Option<Entity>), String> {
    let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
    if let Some(service) = params.get("new_parent_service").and_then(Value::as_str) {
        if !super::attributes::CONTENT_ROOTS.contains(&service) {
            return Err(format!("'{service}' is not a content service"));
        }
        return Ok((root.join(service), None));
    }
    let uuid = uuid_param(params, "new_parent_uuid")?;
    let p = resolve(world, &json!({ "uuid": uuid }))?;
    if !is_binary(world, p.entity) {
        if let Some(file) = world.get::<InstanceFile>(p.entity) {
            if file.toml_path.file_name().is_some_and(|n| n == "_instance.toml") {
                let dir = file.toml_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                return Ok((dir, Some(p.entity)));
            }
            return Err(format!("{} is a single-file record and cannot hold other instances", p.name));
        }
    }
    if let Some(lff) = world.get::<crate::space::LoadedFromFile>(p.entity) {
        if lff.path.is_dir() {
            return Ok((lff.path.clone(), Some(p.entity)));
        }
    }
    Err(format!("{} has no folder on disk to hold other instances", p.name))
}

fn do_reparent(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let uuid = uuid_param(params, "uuid")?;
    let t = resolve(world, &json!({ "uuid": uuid }))?;
    if super::attributes::CONTENT_ROOTS.contains(&t.class.as_str()) {
        return Err(format!("{} is a service and stays where it is", t.class));
    }
    let expose = params.get("expose_to_clients").and_then(Value::as_bool).unwrap_or(false);
    if is_binary(world, t.entity) {
        return Err("this instance is stored in the world database; move it in the Explorer".to_string());
    }
    let file = world
        .get::<InstanceFile>(t.entity)
        .map(|f| f.toml_path.clone())
        .ok_or("this instance has no record of its own on disk, so the move could not be saved")?;
    let from = if file.file_name().is_some_and(|n| n == "_instance.toml") {
        file.parent().map(std::path::Path::to_path_buf).unwrap_or_default()
    } else {
        file.clone()
    };
    let (dir, new_parent) = holder(world, params)?;
    if new_parent.is_some() && crate::space::is_protected_service_path(&dir) {
        return Err("that folder belongs to the engine".to_string());
    }
    if dir.starts_with(&from) {
        return Err("an instance cannot move inside itself".to_string());
    }
    let dest = dir.join(from.file_name().ok_or("the record has no name")?);
    if dest.exists() {
        return Err(format!("'{}' already exists there", dest.file_name().unwrap_or_default().to_string_lossy()));
    }
    let from_rel = rel_of(world, &from).ok_or("the instance's record is outside the open Space")?;
    let dest_rel = rel_of(world, &dest).ok_or("the new parent is outside the open Space")?;
    let from_root = super::attributes::content_root(&format!("{from_rel}/_instance.toml"))?.to_string();
    let to_root = super::attributes::content_root(&format!("{dest_rel}/_instance.toml"))?.to_string();
    check_exposure(&from_root, &to_root, expose)?;

    // The instance keeps its place in the world.
    let (_, world_rot, world_pos) = world
        .get::<GlobalTransform>(t.entity)
        .ok_or("the instance has no world pose")?
        .to_scale_rotation_translation();
    let parent_gt = new_parent.and_then(|p| world.get::<GlobalTransform>(p).copied());
    let new_local =
        super::instance_edit::moved_local((world_pos, world_rot), parent_gt.as_ref(), Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO);
    let old_local = world.get::<Transform>(t.entity).map(|t| (t.translation, t.rotation)).ok_or("no pose")?;
    let old_parent = world.get::<ChildOf>(t.entity).map(|c| c.parent());

    if !crate::undo::move_folder_and_rebase(world, &from, &dest) {
        return Err("the record could not be moved on disk (the engine log says why); nothing changed".to_string());
    }
    match new_parent {
        Some(p) => {
            world.entity_mut(t.entity).insert(ChildOf(p));
        }
        None => {
            world.entity_mut(t.entity).remove::<ChildOf>();
        }
    }
    if let Some(mut tf) = world.get_mut::<Transform>(t.entity) {
        tf.translation = new_local.0;
        tf.rotation = new_local.1;
    }
    if let Some(mut bp) = world.get_mut::<crate::classes::BasePart>(t.entity) {
        bp.cframe.translation = new_local.0;
        bp.cframe.rotation = new_local.1;
    }
    let toml = world.get::<InstanceFile>(t.entity).map(|f| f.toml_path.clone()).unwrap_or_default();
    let stamp = world.get_resource::<crate::auth::AuthState>().and_then(crate::space::instance_loader::current_stamp);
    let saved = crate::space::instance_loader::load_instance_definition(&toml).and_then(|mut def| {
        crate::space::instance_loader::set_authored_transform(&mut def, new_local.0, new_local.1, None);
        crate::space::instance_loader::write_instance_definition_signed(&toml, &mut def, stamp.as_ref())
    });
    if let Err(e) = saved {
        warn!("bridge instance.reparent: moved, but the new pose was not saved: {e}");
    }

    let bits = t.entity.to_bits();
    let action = Action::Batch {
        actions: vec![
            Action::ReparentEntities {
                entries: vec![crate::undo::ReparentEntry {
                    entity_bits: bits,
                    old_parent_bits: old_parent.map(Entity::to_bits),
                    new_parent_bits: new_parent.map(Entity::to_bits),
                    old_path: from.clone(),
                    new_path: dest.clone(),
                }],
            },
            // Undone first (a batch undoes in reverse): the old local pose,
            // then the old parent and folder.
            Action::TransformEntities {
                old_transforms: vec![(bits, old_local.0.to_array(), old_local.1.to_array())],
                new_transforms: vec![(bits, new_local.0.to_array(), new_local.1.to_array())],
            },
        ],
    };
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(format!("Move {}", t.name), action);
    }
    crate::undo::mark_explorer_dirty(world);
    info!("bridge instance.reparent uuid={uuid} {from_root} -> {to_root}{}", if expose { " (expose_to_clients)" } else { "" });
    Ok(json!({
        "instance": target_json(&t),
        "path": rel_of(world, &toml),
        "from_root": from_root,
        "to_root": to_root,
        "undo_steps": 1,
    }))
}


// ---------------------------------------------------------------------------
// instance.clone
// ---------------------------------------------------------------------------

/// Most instance records one clone copies.
pub(super) const MAX_CLONE_RECORDS: usize = 5_000;
/// Most bytes one clone copies.
pub(super) const MAX_CLONE_BYTES: u64 = 512 * 1024 * 1024;

/// A symbolic link or, on Windows, a junction (std reports a mount point as
/// a link too): a clone never follows one, since it could reach outside the
/// Space and copy what is there into it, and from there into a publish.
/// Other reparse points (a OneDrive placeholder) are ordinary files here.
pub(super) fn is_link(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// The records and bytes under `src`, refusing any link and anything past
/// the caps. Nothing is written.
pub(super) fn survey(src: &std::path::Path) -> Result<(usize, u64), String> {
    let mut records = 0usize;
    let mut bytes = 0u64;
    let mut stack = vec![src.to_path_buf()];
    while let Some(p) = stack.pop() {
        let meta = std::fs::symlink_metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if is_link(&meta) {
            return Err(format!("{} is a link; a clone does not copy links", p.display()));
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))? {
                stack.push(entry.map_err(|e| e.to_string())?.path());
            }
        } else {
            bytes += meta.len();
            if p.extension().is_some_and(|e| e == "toml") {
                records += 1;
            }
            if records > MAX_CLONE_RECORDS {
                return Err(format!("more than {MAX_CLONE_RECORDS} records; clone a smaller part of the tree"));
            }
            if bytes > MAX_CLONE_BYTES {
                return Err("more than 512 MB; clone a smaller part of the tree".to_string());
            }
        }
    }
    Ok((records, bytes))
}

/// Copy `src` (a folder or one file) to `dst`, refusing a link met on the
/// way (the survey checked, and this checks again, so a link made in between
/// is not followed either).
pub(super) fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(src).map_err(|e| format!("{}: {e}", src.display()))?;
    if is_link(&meta) {
        return Err(format!("{} is a link; a clone does not copy links", src.display()));
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        for entry in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(drop).map_err(|e| format!("{}: {e}", src.display()))
    }
}

/// Move a finished copy into place, making its parent folder first: a
/// content service with nothing in it yet has no folder on disk.
pub(super) fn place_copy(staged: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::rename(staged, dest).map_err(|e| format!("{}: {e}", dest.display()))
}

/// When this engine started: a staging folder older than that was left by
/// an earlier run that stopped mid-clone.
pub(super) fn session_start() -> std::time::SystemTime {
    static START: std::sync::OnceLock<std::time::SystemTime> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::SystemTime::now)
}

/// Remove clone staging folders an earlier run left in `space_root` (a clone
/// interrupted by a crash): only folders directly under
/// `.eustress/clone-staging/`, only ones last changed before `before`, and
/// never through a link (a link there is left alone). Returns how many went.
pub(super) fn sweep_clone_staging(space_root: &std::path::Path, before: std::time::SystemTime) -> usize {
    let staging = space_root.join(".eustress").join("clone-staging");
    let Ok(meta) = std::fs::symlink_metadata(&staging) else { return 0 };
    if is_link(&meta) || !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(&staging) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
        if is_link(&meta) || !meta.is_dir() {
            continue;
        }
        if meta.modified().is_ok_and(|m| m < before) && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Sweep the open Space's stale clone staging when a Space opens.
pub(super) fn sweep_on_space_open(root: Res<crate::space::SpaceRoot>) {
    let removed = sweep_clone_staging(&root.0, session_start());
    if removed > 0 {
        info!("bridge: removed {removed} clone staging folder(s) an earlier run left");
    }
}

/// Give every instance record under `dir` a fresh uuid, as a pasted copy
/// gets: two instances never share one.
fn fresh_uuids(dir: &std::path::Path) -> Result<usize, String> {
    let mut changed = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for entry in std::fs::read_dir(&p).map_err(|e| e.to_string())? {
                stack.push(entry.map_err(|e| e.to_string())?.path());
            }
        } else if p.extension().is_some_and(|e| e == "toml") {
            // A TOML file that is not an instance record is left as it is.
            if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&p) {
                if def.metadata.uuid.is_some() {
                    def.metadata.uuid = Some(uuid::Uuid::new_v4().simple().to_string());
                    crate::space::instance_loader::write_instance_definition_signed(&p, &mut def, None)?;
                    changed += 1;
                }
            }
        }
    }
    Ok(changed)
}

/// `instance.clone`: `uuid`; optional `new_parent_uuid` or
/// `new_parent_service` (default: beside the original), `name`, and
/// `expose_to_clients`. The copy is written to disk, every record with a
/// fresh uuid, and the file watcher spawns it on a following frame.
pub fn clone(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, do_clone(world, &req.params))
}

fn do_clone(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let uuid = uuid_param(params, "uuid")?;
    let t = resolve(world, &json!({ "uuid": uuid }))?;
    if super::attributes::CONTENT_ROOTS.contains(&t.class.as_str()) {
        return Err(format!("{} is a service and is not copied", t.class));
    }
    if is_binary(world, t.entity) {
        return Err("this instance is stored in the world database; duplicate it in the Explorer".to_string());
    }
    let expose = params.get("expose_to_clients").and_then(Value::as_bool).unwrap_or(false);
    let file = world
        .get::<InstanceFile>(t.entity)
        .map(|f| f.toml_path.clone())
        .ok_or("this instance has no record of its own on disk to copy")?;
    let folder_form = file.file_name().is_some_and(|n| n == "_instance.toml");
    let src = if folder_form { file.parent().map(std::path::Path::to_path_buf).unwrap_or_default() } else { file.clone() };
    let dir = if params.get("new_parent_uuid").is_some() || params.get("new_parent_service").is_some() {
        holder(world, params)?.0
    } else {
        src.parent().map(std::path::Path::to_path_buf).ok_or("the record has no folder")?
    };
    if dir.starts_with(&src) {
        return Err("a copy cannot go inside the original".to_string());
    }
    let space_root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
    let src_rel = rel_of(world, &src).ok_or("the instance's record is outside the open Space")?;
    let from_root = super::attributes::content_root(&format!("{src_rel}/_instance.toml"))?.to_string();
    let dir_rel = rel_of(world, &dir).ok_or("the new parent is outside the open Space")?;
    let to_root = super::attributes::content_root(&format!("{dir_rel}/x/_instance.toml"))?.to_string();
    check_exposure(&from_root, &to_root, expose)?;
    let (records, bytes) = survey(&src)?;

    let base = match params.get("name").and_then(Value::as_str) {
        Some(raw) => instance_name(raw)?,
        None => t.name.clone(),
    };
    let dest = if folder_form {
        dir.join(eustress_common::instance_create::unique_entity_name(&dir, &base))
    } else {
        let file_name = src.file_name().unwrap_or_default().to_string_lossy().to_string();
        let ext = file_name.split_once('.').map(|(_, e)| e.to_string()).unwrap_or_else(|| "toml".to_string());
        let stem = eustress_common::instance_create::unique_entity_name(&dir, &base);
        dir.join(format!("{stem}.{ext}"))
    };
    if dest.exists() {
        return Err(format!("'{}' already exists there", dest.file_name().unwrap_or_default().to_string_lossy()));
    }

    // Built in the Space's own hidden folder, then moved into place in one
    // rename, so the watcher sees the finished copy, fresh uuids and all.
    let staging = space_root.join(".eustress").join("clone-staging").join(uuid::Uuid::new_v4().simple().to_string());
    let staged = staging.join(dest.file_name().unwrap_or_default());
    let built = std::fs::create_dir_all(&staging)
        .map_err(|e| e.to_string())
        .and_then(|()| copy_tree(&src, &staged))
        .and_then(|()| fresh_uuids(&staged));
    let moved = built.and_then(|n| place_copy(&staged, &dest).map(|()| n));
    let _ = std::fs::remove_dir_all(&staging);
    let renewed = moved?;

    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(format!("Duplicate {}", t.name), Action::spawn_folders(&space_root, &[dest.clone()]));
    }
    info!("bridge instance.clone uuid={uuid} {from_root} -> {to_root} records={records}{}", if expose { " (expose_to_clients)" } else { "" });
    Ok(json!({
        "source": target_json(&t),
        "path": rel_of(world, &dest),
        "records": records,
        "fresh_uuids": renewed,
        "bytes": bytes,
        "undo_steps": 1,
        "note": "The file watcher spawns the copy on a following frame; one undo step removes it.",
    }))
}

// ---------------------------------------------------------------------------
// instance.create
// ---------------------------------------------------------------------------

/// Classes created through another tool, with its own checks.
fn create_elsewhere(class: &str) -> Option<&'static str> {
    const SCRIPTS: &[&str] = &["Script", "LocalScript", "ModuleScript", "RuneScript", "LuauScript", "SoulScript"];
    if SCRIPTS.contains(&class) || class.ends_with("Script") {
        return Some("create_script");
    }
    if class.starts_with("Terrain") {
        return Some("terrain_layer_create");
    }
    None
}

/// `instance.create`: `class`, the parent as `parent_uuid` or
/// `parent_service`, and optional `name`, `position` [x, y, z] (local, metres)
/// and `asset_mesh` (a mesh inside the Space). Written from the class's
/// template; the file watcher spawns it on a following frame.
pub fn create(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    answer(req, do_create(world, &req.params))
}

fn do_create(world: &mut World, params: &Value) -> Result<Value, String> {
    super::properties::editing_only(world)?;
    let class = params.get("class").and_then(Value::as_str).map(str::trim).ok_or("`class` is the class to create")?;
    if let Some(tool) = create_elsewhere(class) {
        return Err(format!("{class} is created with the {tool} tool"));
    }
    if eustress_common::datamodel::is_service_class(class) {
        return Err(format!("{class} is a service; there is one of each"));
    }
    if !eustress_common::datamodel::record::is_known_class(class) {
        return Err(format!("'{class}' is not a class this engine knows"));
    }
    let mut parent_params = params.clone();
    if let Some(obj) = parent_params.as_object_mut() {
        if let Some(v) = obj.remove("parent_uuid") {
            obj.insert("new_parent_uuid".into(), v);
        }
        if let Some(v) = obj.remove("parent_service") {
            obj.insert("new_parent_service".into(), v);
        }
    }
    let (dir, _) = holder(world, &parent_params)?;
    let space_root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
    let dir_rel = rel_of(world, &dir).ok_or("the parent is outside the open Space")?;
    super::attributes::content_root(&format!("{dir_rel}/x/_instance.toml"))?;
    let name = match params.get("name").and_then(Value::as_str) {
        Some(raw) => Some(instance_name(raw)?),
        None => None,
    };
    let mut overrides = eustress_common::instance_create::InstanceOverrides::default();
    if let Some(p) = params.get("position") {
        let a = p.as_array().filter(|a| a.len() == 3).ok_or("`position` is [x, y, z]")?;
        let mut v = [0f32; 3];
        for (slot, x) in v.iter_mut().zip(a) {
            *slot = x.as_f64().filter(|f| f.is_finite() && f.abs() < 1.0e7).ok_or("`position` holds finite numbers")? as f32;
        }
        overrides.position = Some(v);
    }
    if let Some(mesh) = params.get("asset_mesh").and_then(Value::as_str) {
        let rel = eustress_tools::sandbox::relative(mesh).map_err(|e| e.to_string())?;
        if !space_root.join(&rel).is_file() {
            return Err(format!("{mesh} is not a file in this Space"));
        }
        overrides.asset_mesh = Some(rel.to_string_lossy().replace('\\', "/"));
    }
    overrides.display_name = name.clone();
    let created = eustress_common::instance_create::create_instance(&dir, class, name.as_deref(), overrides)
        .map_err(|e| format!("{e:?}"))?;
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.push_labeled(format!("Insert {class}"), Action::spawn_folders(&space_root, &[created.folder_path.clone()]));
    }
    info!("bridge instance.create class={class} under {dir_rel}");
    Ok(json!({
        "class": class,
        "name": created.folder_name,
        "path": rel_of(world, &created.toml_path),
        "undo_steps": 1,
        "note": "The file watcher spawns it on a following frame; one undo step removes it.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_clone_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plain_tree_is_surveyed_and_copied() {
        let root = scratch("plain");
        std::fs::create_dir_all(root.join("src/Wheel")).unwrap();
        std::fs::write(root.join("src/_instance.toml"), "[metadata]\n").unwrap();
        std::fs::write(root.join("src/Wheel/_instance.toml"), "[metadata]\n").unwrap();
        std::fs::write(root.join("src/Wheel/mesh.glb"), [0u8; 10]).unwrap();
        assert_eq!(survey(&root.join("src")).unwrap(), (2, 2 * 11 + 10));
        copy_tree(&root.join("src"), &root.join("dst")).unwrap();
        assert!(root.join("dst/Wheel/mesh.glb").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A junction inside the copied folder that points outside it is never
    /// followed: neither the survey nor the copy reads through it.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_never_copied() {
        let root = scratch("junction");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/_instance.toml"), "[metadata]\n").unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("outside/secret.txt"), "not yours").unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            // Backslashes throughout: cmd reads a "/link" as a switch.
            .arg(root.join("src").join("link"))
            .arg(root.join("outside"))
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed: {made:?}");
        assert!(is_link(&std::fs::symlink_metadata(root.join("src/link")).unwrap()), "a junction is a link");
        assert!(survey(&root.join("src")).unwrap_err().contains("link"));
        assert!(copy_tree(&root.join("src"), &root.join("dst")).is_err());
        assert!(!root.join("dst/link/secret.txt").exists(), "nothing was read through the junction");
        let _ = std::fs::remove_dir_all(root);
    }

    /// The copy checks each entry itself, so a link made after the survey is
    /// not followed either: a link to a file outside is refused, and what it
    /// points at never lands in the copy.
    #[cfg(windows)]
    #[test]
    fn a_file_link_is_never_copied() {
        let root = scratch("filelink");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("outside.txt"), "not yours").unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink"])
            .arg(root.join("src").join("link.txt"))
            .arg(root.join("outside.txt"))
            .output()
            .unwrap();
        if !made.status.success() {
            // Creating a file link needs Developer Mode or elevation.
            eprintln!("skipped: this machine cannot create a file link");
            return;
        }
        assert!(copy_tree(&root.join("src"), &root.join("dst")).is_err());
        assert!(!root.join("dst").join("link.txt").exists(), "the linked file was not copied");
        let _ = std::fs::remove_dir_all(root);
    }

    /// A copy placed into a service that has no folder yet lands there: the
    /// parent is made first (a missing parent was os error 3).
    #[test]
    fn a_copy_lands_in_a_service_with_no_folder_yet() {
        let root = scratch("place");
        std::fs::create_dir_all(root.join("staging").join("Spine")).unwrap();
        std::fs::write(root.join("staging").join("Spine").join("_instance.toml"), "x").unwrap();
        assert!(!root.join("Workspace").exists(), "the service has no folder yet");
        place_copy(&root.join("staging").join("Spine"), &root.join("Workspace").join("Spine")).unwrap();
        assert!(root.join("Workspace").join("Spine").join("_instance.toml").is_file());
        assert!(!root.join("staging").join("Spine").exists(), "moved, not copied");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scripts_and_terrain_layers_are_made_by_their_own_tools() {
        assert_eq!(create_elsewhere("Script"), Some("create_script"));
        assert_eq!(create_elsewhere("ModuleScript"), Some("create_script"));
        assert_eq!(create_elsewhere("TerrainSpline"), Some("terrain_layer_create"));
        assert_eq!(create_elsewhere("Part"), None);
        assert_eq!(create_elsewhere("Attachment"), None);
    }

    #[test]
    fn server_only_content_reaches_players_only_on_purpose() {
        assert!(check_exposure("ServerStorage", "Workspace", false).is_err());
        assert!(check_exposure("ServerScriptService", "ReplicatedStorage", false).is_err());
        assert!(check_exposure("ServerStorage", "Workspace", true).is_ok());
        assert!(check_exposure("ServerStorage", "ServerScriptService", false).is_ok());
        assert!(check_exposure("Workspace", "ServerStorage", false).is_ok());
        assert!(check_exposure("Workspace", "Lighting", false).is_ok());
        assert!(check_exposure("ServerScriptService", "ServerStorage", false).is_ok(), "both stay on the server");
    }

    /// Only stale folders directly under clone-staging go; a newer one, a
    /// link there, and anything outside clone-staging stay.
    #[test]
    fn stale_clone_staging_is_swept_and_nothing_else() {
        let root = scratch("sweep");
        let staging = root.join(".eustress").join("clone-staging");
        std::fs::create_dir_all(staging.join("old1").join("Spine")).unwrap();
        std::fs::write(staging.join("old1").join("Spine").join("_instance.toml"), "x").unwrap();
        std::fs::create_dir_all(root.join(".eustress").join("trash").join("keep")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("outside").join("keep.txt"), "mine").unwrap();
        #[cfg(windows)]
        let linked = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(staging.join("link"))
            .arg(root.join("outside"))
            .output()
            .is_ok_and(|o| o.status.success());
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(root.join("outside"), staging.join("link")).is_ok();
        // Everything there now counts as older than a session that starts later.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        assert_eq!(sweep_clone_staging(&root, later), 1, "only the stale staging folder");
        assert!(!staging.join("old1").exists());
        assert_eq!(std::fs::read_to_string(root.join("outside").join("keep.txt")).unwrap(), "mine");
        assert!(root.join(".eustress").join("trash").join("keep").is_dir(), "nothing outside clone-staging");
        if linked {
            assert!(staging.join("link").exists(), "a link is left alone, not followed or removed");
        }
        // One made this session is not stale.
        std::fs::create_dir_all(staging.join("new1")).unwrap();
        assert_eq!(sweep_clone_staging(&root, std::time::SystemTime::now() - std::time::Duration::from_secs(60)), 0);
        assert!(staging.join("new1").is_dir());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_form_record_renames_its_folder_and_a_flat_one_its_file() {
        let (from, to) = renamed_path(Path::new("S/ServerStorage/Cars/Spine/_instance.toml"), "Chassis").unwrap();
        assert_eq!((from.as_path(), to.as_path()), (Path::new("S/ServerStorage/Cars/Spine"), Path::new("S/ServerStorage/Cars/Chassis")));
        let (from, to) = renamed_path(Path::new("S/Workspace/Base.part.toml"), "Ground").unwrap();
        assert_eq!((from.as_path(), to.as_path()), (Path::new("S/Workspace/Base.part.toml"), Path::new("S/Workspace/Ground.part.toml")));
    }

    #[test]
    fn a_name_is_one_plain_file_name() {
        assert_eq!(instance_name(" Chassis ").as_deref(), Ok("Chassis"), "surrounding spaces are trimmed");
        assert_eq!(instance_name("trail ").as_deref(), Ok("trail"));
        for bad in ["", "a/b", "a\\b", "..", "CON", "nul.txt", "x:y", "trail.", "tab\there", &"x".repeat(101)] {
            assert!(instance_name(bad).is_err(), "{bad:?}");
        }
    }
}

/// The bridge path on a bare World with the resources it reads.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::play_mode::PlayModeState;
    use crate::space::SpaceRoot;
    use crate::undo::{apply_undo_action, UndoStack};
    use eustress_common::classes::ClassName;
    use std::path::PathBuf;

    /// ServerStorage/Cars/Spine (a Part) holding A0 (an Attachment), both
    /// with records, plus a sibling Bumper.
    fn space(tag: &str) -> (World, Entity, Entity, PathBuf) {
        let root = std::env::temp_dir().join(format!("eus_rename_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut world = World::new();
        world.insert_resource(SpaceRoot(root.clone()));
        world.insert_resource(UndoStack::default());
        world.insert_resource(State::new(PlayModeState::Editing));
        let spawn = |world: &mut World, rel: &str, name: &str, class: ClassName, id: u32, parent: Option<Entity>| {
            let record = root.join(rel);
            std::fs::create_dir_all(record.parent().unwrap()).unwrap();
            std::fs::write(&record, format!("[metadata]\nclass_name = \"{class:?}\"\n")).unwrap();
            let mut e = world.spawn((
                Instance { name: name.into(), class_name: class, archivable: true, id, ai: false, uuid: format!("{name}-uuid") },
                InstanceFile { toml_path: record, mesh_path: PathBuf::new(), name: name.into() },
                Transform::IDENTITY,
                GlobalTransform::IDENTITY,
            ));
            if let Some(p) = parent {
                e.insert(ChildOf(p));
            }
            e.id()
        };
        let spine = spawn(&mut world, "ServerStorage/Cars/Spine/_instance.toml", "Spine", ClassName::Part, 1, None);
        let a0 = spawn(&mut world, "ServerStorage/Cars/Spine/A0/_instance.toml", "A0", ClassName::Attachment, 2, Some(spine));
        spawn(&mut world, "ServerStorage/Cars/Bumper/_instance.toml", "Bumper", ClassName::Part, 3, None);
        (world, spine, a0, root)
    }

    #[test]
    fn a_rename_moves_the_folder_and_everything_under_it_and_undoes() {
        let (mut world, spine, a0, root) = space("move");
        let reply = do_rename(&mut world, &json!({ "uuid": "Spine-uuid", "name": "Chassis" })).unwrap();
        assert_eq!(reply["saved_to"], "ServerStorage/Cars/Chassis/_instance.toml");
        assert!(root.join("ServerStorage/Cars/Chassis/_instance.toml").is_file());
        assert!(!root.join("ServerStorage/Cars/Spine").exists());
        assert_eq!(world.get::<Instance>(spine).unwrap().name, "Chassis");
        assert_eq!(
            world.get::<InstanceFile>(a0).unwrap().toml_path,
            root.join("ServerStorage/Cars/Chassis/A0/_instance.toml"),
            "the child's record path follows the folder"
        );
        let action = world.resource_mut::<UndoStack>().undo().expect("one undo step");
        apply_undo_action(&action, &mut world);
        assert!(root.join("ServerStorage/Cars/Spine/_instance.toml").is_file(), "undo moves the folder back");
        assert_eq!(world.get::<Instance>(spine).unwrap().name, "Spine");
        assert_eq!(world.get::<InstanceFile>(a0).unwrap().toml_path, root.join("ServerStorage/Cars/Spine/A0/_instance.toml"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_rename_onto_a_sibling_a_bad_name_or_during_play_changes_nothing() {
        let (mut world, spine, _, root) = space("refuse");
        assert!(do_rename(&mut world, &json!({ "uuid": "Spine-uuid", "name": "Bumper" })).unwrap_err().contains("already exists"));
        assert!(do_rename(&mut world, &json!({ "uuid": "Spine-uuid", "name": "COM1" })).is_err());
        world.insert_resource(State::new(PlayModeState::Playing));
        assert!(do_rename(&mut world, &json!({ "uuid": "Spine-uuid", "name": "Chassis" })).unwrap_err().contains("Play"));
        assert_eq!(world.get::<Instance>(spine).unwrap().name, "Spine");
        assert!(root.join("ServerStorage/Cars/Spine/_instance.toml").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    /// A Model under ServerStorage at world (100, 0, 0), to move things into.
    fn garage(world: &mut World, root: &std::path::Path) -> Entity {
        let record = root.join("ServerStorage/Garage/_instance.toml");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "[metadata]\nclass_name = \"Model\"\n").unwrap();
        let t = Transform::from_xyz(100.0, 0.0, 0.0);
        world
            .spawn((
                Instance { name: "Garage".into(), class_name: ClassName::Model, archivable: true, id: 9, ai: false, uuid: "Garage-uuid".into() },
                InstanceFile { toml_path: record, mesh_path: PathBuf::new(), name: "Garage".into() },
                t,
                GlobalTransform::from(t),
            ))
            .id()
    }

    #[test]
    fn a_reparent_keeps_the_world_pose_and_undoes() {
        let (mut world, spine, a0, root) = space("reparent");
        let garage = garage(&mut world, &root);
        // Spine sits at world (5, 0, 0).
        *world.get_mut::<Transform>(spine).unwrap() = Transform::from_xyz(5.0, 0.0, 0.0);
        *world.get_mut::<GlobalTransform>(spine).unwrap() = GlobalTransform::from(Transform::from_xyz(5.0, 0.0, 0.0));
        let reply = do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_uuid": "Garage-uuid" })).unwrap();
        assert_eq!(reply["path"], "ServerStorage/Garage/Spine/_instance.toml");
        assert_eq!(world.get::<ChildOf>(spine).map(|c| c.parent()), Some(garage));
        assert_eq!(world.get::<Transform>(spine).unwrap().translation, Vec3::new(-95.0, 0.0, 0.0), "same place in the world");
        assert_eq!(world.get::<InstanceFile>(a0).unwrap().toml_path, root.join("ServerStorage/Garage/Spine/A0/_instance.toml"));
        let saved = crate::space::instance_loader::load_instance_definition(&root.join("ServerStorage/Garage/Spine/_instance.toml")).unwrap();
        assert!((saved.transform.position[0] + 95.0).abs() < 1e-3, "the new local pose is saved");
        let action = world.resource_mut::<UndoStack>().undo().expect("one undo step");
        apply_undo_action(&action, &mut world);
        assert!(root.join("ServerStorage/Cars/Spine/_instance.toml").is_file(), "undo moves it back");
        assert!(world.get::<ChildOf>(spine).is_none());
        assert_eq!(world.get::<Transform>(spine).unwrap().translation, Vec3::new(5.0, 0.0, 0.0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn server_content_moves_to_a_player_root_only_on_purpose() {
        let (mut world, spine, _, root) = space("expose");
        let err = do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_service": "Workspace" })).unwrap_err();
        assert!(err.contains("expose_to_clients"), "{err}");
        assert!(root.join("ServerStorage/Cars/Spine/_instance.toml").is_file(), "nothing moved");
        do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_service": "Workspace", "expose_to_clients": true })).unwrap();
        assert!(root.join("Workspace/Spine/_instance.toml").is_file());
        assert!(world.get::<ChildOf>(spine).is_none(), "a service root's children sit at the scene root");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_move_inside_itself_onto_a_name_or_to_a_non_content_root_is_refused() {
        let (mut world, _, _, root) = space("inside");
        assert!(do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_uuid": "A0-uuid" })).unwrap_err().contains("inside itself"));
        assert!(do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_service": "CoreGui" })).is_err());
        std::fs::create_dir_all(root.join("ServerStorage/Garage/Spine")).unwrap();
        garage(&mut world, &root);
        assert!(do_reparent(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_uuid": "Garage-uuid" })).unwrap_err().contains("already exists"));
        assert!(root.join("ServerStorage/Cars/Spine/_instance.toml").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    fn uuid_of(path: &std::path::Path) -> Option<String> {
        crate::space::instance_loader::load_instance_definition(path).ok()?.metadata.uuid
    }

    #[test]
    fn a_clone_is_a_copy_with_fresh_uuids_and_one_undo_step() {
        let (mut world, _, _, root) = space("clone");
        let spine = root.join("ServerStorage/Cars/Spine/_instance.toml");
        std::fs::write(&spine, "[metadata]\nclass_name = \"Part\"\nuuid = \"0123456789abcdef0123456789abcdef\"\n").unwrap();
        let reply = do_clone(&mut world, &json!({ "uuid": "Spine-uuid" })).unwrap();
        let path = reply["path"].as_str().unwrap().to_string();
        assert_ne!(path, "ServerStorage/Cars/Spine", "a new name beside the original");
        let copy = root.join(&path).join("_instance.toml");
        assert!(copy.is_file() && root.join(&path).join("A0/_instance.toml").is_file(), "the whole subtree");
        assert_ne!(uuid_of(&copy), uuid_of(&spine), "a fresh uuid");
        assert!(uuid_of(&copy).is_some());
        assert!(world.resource_mut::<UndoStack>().undo().is_some(), "one undo step");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_clone_from_server_storage_to_the_workspace_needs_the_flag() {
        let (mut world, _, _, root) = space("clone_expose");
        assert!(do_clone(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_service": "Workspace" })).unwrap_err().contains("expose_to_clients"));
        assert!(!root.join("Workspace/Spine").exists());
        do_clone(&mut world, &json!({ "uuid": "Spine-uuid", "new_parent_service": "Workspace", "expose_to_clients": true })).unwrap();
        assert!(root.join("Workspace/Spine/_instance.toml").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_writes_a_known_class_and_refuses_scripts_services_and_unknowns() {
        let (mut world, _, _, root) = space("create");
        let reply = do_create(&mut world, &json!({ "class": "Part", "parent_uuid": "Spine-uuid", "name": "Bolt" })).unwrap();
        assert!(root.join(reply["path"].as_str().unwrap()).is_file(), "{reply}");
        assert!(world.resource_mut::<UndoStack>().undo().is_some());
        assert!(do_create(&mut world, &json!({ "class": "Script", "parent_service": "ServerScriptService" })).unwrap_err().contains("create_script"));
        assert!(do_create(&mut world, &json!({ "class": "Workspace", "parent_service": "Workspace" })).is_err());
        assert!(do_create(&mut world, &json!({ "class": "NotAClass", "parent_service": "Workspace" })).is_err());
        assert!(do_create(&mut world, &json!({ "class": "Part", "parent_service": "CoreGui" })).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Undo never overwrites: if something now holds the old name, the folder
    /// stays where it is.
    #[test]
    fn undo_does_not_overwrite_what_now_holds_the_old_name() {
        let (mut world, _, _, root) = space("occupied");
        do_rename(&mut world, &json!({ "uuid": "Spine-uuid", "name": "Chassis" })).unwrap();
        std::fs::create_dir_all(root.join("ServerStorage/Cars/Spine")).unwrap();
        std::fs::write(root.join("ServerStorage/Cars/Spine/keep.txt"), "mine").unwrap();
        let action = world.resource_mut::<UndoStack>().undo().unwrap();
        apply_undo_action(&action, &mut world);
        assert_eq!(std::fs::read_to_string(root.join("ServerStorage/Cars/Spine/keep.txt")).unwrap(), "mine");
        assert!(root.join("ServerStorage/Cars/Chassis/_instance.toml").is_file(), "the renamed folder stays");
        let _ = std::fs::remove_dir_all(root);
    }
}
