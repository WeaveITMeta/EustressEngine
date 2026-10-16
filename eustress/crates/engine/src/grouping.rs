//! Group (Ctrl+G) and Ungroup (Ctrl+U) on saved folders.
//!
//! A saved object is a folder with an `_instance.toml`, and the folder nesting
//! is the hierarchy on disk. Ctrl+G creates a Model folder in the folder the
//! selection shares and moves each selected folder into it. Ctrl+U moves a
//! container's children out to the container's own folder and trashes the
//! container's folder. Both keep the Space database's tree in step (the moves go
//! through `undo::move_folder_and_rebase`), and each records one undo step whose
//! undo and redo repeat the same disk work in reverse
//! (`undo::Action::GroupFolders` and `undo::Action::UngroupFolders`). After a
//! reopen the Model and its children, or the container and its file, are where
//! the last action left them.
//!
//! Ctrl+G refuses objects that are not saved folders (a flat file, a script, an
//! object held only in memory): nothing could move them on disk, and a reopen
//! would undo the group. Ctrl+U dissolves a container that is not a saved folder
//! in memory only, as it always has, and says so.
//!
//! Undo and redo find entities by path, never by id: a trashed and restored
//! Model is a new entity. A folder's parent is the entity of the folder it sits
//! in, or none for a service root (Workspace, Lighting, ...), which is a root of
//! the hierarchy.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use eustress_common::classes::{ClassName, Instance};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::selection_sync::SelectionSyncManager;
use crate::space::instance_loader::{InstanceFile, MeshFold};
use crate::space::LoadedFromFile;
use crate::space::service_loader::ServiceComponent;
use crate::ui::MenuActionEvent;
use crate::undo::{FolderMove, FolderNode, GroupMember, UngroupedContainer};

/// The record a folder-form object keeps inside its folder.
const INSTANCE_FILE: &str = "_instance.toml";

/// Per-session selection id string (`"{index}v{generation}"`), the form the
/// `SelectionManager` stores.
fn entity_id_str(e: Entity) -> String {
    format!("{}v{}", e.index(), e.generation())
}

/// Whether `toml` is a folder's own record (`<folder>/_instance.toml`).
fn is_folder_form(toml: &Path) -> bool {
    toml.file_name().and_then(|n| n.to_str()) == Some(INSTANCE_FILE)
}

/// The folder of a saved object: its record's folder, when that folder is on
/// disk. `None` for an object with no folder.
fn folder_of(world: &World, entity: Entity) -> Option<PathBuf> {
    let file = world.get::<InstanceFile>(entity)?;
    if !is_folder_form(&file.toml_path) {
        return None;
    }
    let dir = file.toml_path.parent()?.to_path_buf();
    dir.is_dir().then_some(dir)
}

/// What a selected object is on disk. Only a saved folder can be grouped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OnDisk {
    /// A folder with its record: it can be grouped.
    Folder,
    /// Not saved yet: no record and no file.
    Unsaved,
    /// A script or a GUI file (`LoadedFromFile`).
    LooseFile,
    /// A single record file beside its siblings (a flat file from an older Space).
    FlatFile,
    /// A record that exists only in the Space database.
    DatabaseOnly,
}

/// Classify a selected object by what is on disk for it.
fn on_disk(file: Option<&InstanceFile>, loaded: Option<&LoadedFromFile>) -> OnDisk {
    match (file, loaded) {
        (Some(file), _) if is_folder_form(&file.toml_path) => {
            if file.toml_path.is_file() {
                OnDisk::Folder
            } else {
                OnDisk::DatabaseOnly
            }
        }
        (Some(file), _) => {
            if file.toml_path.is_file() {
                OnDisk::FlatFile
            } else {
                OnDisk::DatabaseOnly
            }
        }
        (None, Some(_)) => OnDisk::LooseFile,
        (None, None) => OnDisk::Unsaved,
    }
}

/// The file of a loose object (a script or a GUI file) when that file is on disk,
/// so it can move with its container's folder.
fn loose_file_of(world: &World, entity: Entity) -> Option<PathBuf> {
    let loaded = world.get::<LoadedFromFile>(entity)?;
    loaded.path.is_file().then(|| loaded.path.clone())
}

/// "it" for one object, "them" for more than one.
fn pronoun(count: usize) -> &'static str {
    if count == 1 {
        "it"
    } else {
        "them"
    }
}

/// The toast for a Ctrl+G selection that holds objects which cannot be grouped
/// yet, or `None` when every selected object can be. Each kind says what to do.
/// When kinds are mixed, the first one in this order is named.
fn group_refusal(unsaved: usize, loose: usize, flat: usize, database: usize) -> Option<String> {
    if unsaved > 0 {
        return Some(format!(
            "Save first, then group. {unsaved} selected {} not saved yet. Nothing was grouped.",
            if unsaved == 1 { "object is" } else { "objects are" },
        ));
    }
    let (count, kind) = if loose > 0 {
        (loose, "Scripts and loose files cannot be grouped yet.")
    } else if flat > 0 {
        (flat, "Single-file objects cannot be grouped yet.")
    } else if database > 0 {
        (database, "Objects that live only in the Space database cannot be grouped yet.")
    } else {
        return None;
    };
    Some(format!(
        "{kind} Take {} out of the selection, then group the rest. Nothing was grouped.",
        pronoun(count),
    ))
}

/// The entity of a saved folder, found by its folder or by its `_instance.toml`.
/// A service root has no entity: it is a root of the hierarchy.
fn folder_entity(world: &World, dir: &Path) -> Option<Entity> {
    if crate::space::is_protected_service_path(dir) {
        return None;
    }
    let registry = world.get_resource::<crate::space::SpaceFileRegistry>()?;
    registry
        .get_entity(&dir.join(INSTANCE_FILE))
        .or_else(|| registry.get_entity(dir))
}

/// The entity that children of a folder hang from: `Ok(None)` for a service
/// root, the folder's own entity otherwise. A folder that is not loaded is an
/// error, so nothing is grouped beside a hierarchy it was not meant for.
fn holder_of(world: &World, dir: &Path) -> Result<Option<Entity>, String> {
    if crate::space::is_protected_service_path(dir) {
        return Ok(None);
    }
    folder_entity(world, dir)
        .map(Some)
        .ok_or_else(|| format!("the folder {} is not loaded", dir.display()))
}

/// The deepest folder that every path sits in, or `None` when they share none.
fn common_folder(paths: &[PathBuf]) -> Option<PathBuf> {
    let (first, rest) = paths.split_first()?;
    let mut common: Vec<std::path::Component> = first.components().collect();
    for path in rest {
        let parts: Vec<std::path::Component> = path.components().collect();
        let shared = common
            .iter()
            .zip(parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        common.truncate(shared);
    }
    if common.is_empty() {
        return None;
    }
    Some(common.iter().collect())
}

/// Whether an ancestor of `entity` is one of `selected`: such an object moves
/// with its ancestor, so it is not grouped on its own.
fn has_selected_ancestor(world: &World, entity: Entity, selected: &[Entity]) -> bool {
    let mut current = entity;
    while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
        if selected.contains(&parent) {
            return true;
        }
        current = parent;
    }
    false
}

/// An entity's local translation and rotation, relative to its parent.
fn local_pose(world: &World, entity: Entity) -> (Vec3, Quat) {
    world
        .get::<Transform>(entity)
        .map(|t| (t.translation, t.rotation))
        .unwrap_or((Vec3::ZERO, Quat::IDENTITY))
}

/// Parent `entity` under `parent`, or make it a root of the hierarchy.
fn set_parent(world: &mut World, entity: Entity, parent: Option<Entity>) {
    if world.get_entity(entity).is_err() {
        return;
    }
    match parent {
        Some(parent) if world.get_entity(parent).is_ok() => {
            world.entity_mut(entity).insert(ChildOf(parent));
        }
        _ => {
            world.entity_mut(entity).remove::<ChildOf>();
        }
    }
}

/// Set an entity's local translation and rotation. Its scale is left alone.
fn set_local(world: &mut World, entity: Entity, translation: [f32; 3], rotation: [f32; 4]) {
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = Vec3::from_array(translation);
        transform.rotation = Quat::from_array(rotation);
    }
}

/// Replace a record's text on disk. The record is marked as an engine write first,
/// so the watcher's rename event is taken as ours and not as an external edit. The
/// text goes to a temporary file whose extension `tree_tracks` does not name (`.tmp`),
/// which the seed and the startup reconcile skip, and is renamed over the record, so
/// a reader never sees half of a file.
fn write_text_atomic(world: &mut World, path: &Path, text: &str) -> Result<(), String> {
    if let Some(mut written) = world.get_resource_mut::<crate::space::file_watcher::RecentlyWrittenFiles>() {
        written.mark_written(path.to_path_buf());
    }
    let tmp = path.with_file_name("_instance.toml.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))
}

/// Write a folder's local pose into its record: into the file where there is
/// one, and into the database alone for an object that has no file. The same
/// two places a part is saved. The database copy follows the file.
fn persist_local(world: &mut World, toml: &Path, translation: [f32; 3], rotation: [f32; 4]) -> Result<(), String> {
    let (t, r) = (Vec3::from_array(translation), Quat::from_array(rotation));
    let edit = |text: &str| -> Result<Option<String>, String> {
        let mut def = crate::space::instance_loader::load_instance_definition_from_str(text)?;
        crate::space::instance_loader::set_authored_transform(&mut def, t, r, None);
        crate::space::instance_loader::signed_write_text(Some(text), &mut def, None)
    };
    if !toml.is_file() {
        crate::space::active_db::update_instance_text(toml, edit)?;
        return Ok(());
    }
    let current =
        std::fs::read_to_string(toml).map_err(|e| format!("read {}: {e}", toml.display()))?;
    if let Some(text) = edit(&current)? {
        write_text_atomic(world, toml, &text)?;
        let _ = crate::space::active_db::put_tree_file(toml, text.as_bytes());
    }
    Ok(())
}

/// Put a folder's record on disk before the folder is trashed, so undo can
/// bring the whole folder back. A record held only in the database is written
/// out first.
fn ensure_on_disk(world: &mut World, toml: &Path) -> Result<(), String> {
    if toml.is_file() {
        return Ok(());
    }
    if let Some(text) = crate::space::active_db::get_instance_text(toml) {
        write_text_atomic(world, toml, &text)?;
    }
    Ok(())
}

/// The record of a new Model: no asset, class Model, identity pose. The caller
/// sets the pose.
fn model_definition() -> crate::space::instance_loader::InstanceDefinition {
    let now = chrono::Utc::now().to_rfc3339();
    crate::space::instance_loader::InstanceDefinition {
        plasma: None,
        asset: None,
        transform: crate::space::instance_loader::TransformData {
            position: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
        },
        properties: crate::space::instance_loader::InstanceProperties {
            color: [163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0, 1.0],
            transparency: 0.0,
            anchored: true,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            material: "Plastic".to_string(),
            locked: false,
            physics: None,
            respect_gltf_materials: false,
            destructible: false,
        },
        metadata: crate::space::instance_loader::InstanceMetadata {
            class_name: "Model".to_string(),
            archivable: true,
            name: None,
            created: now.clone(),
            last_modified: now,
            created_by: None,
            modifications: Vec::new(),
            unit: None,
            uuid: None,
        },
        material: None,
        thermodynamic: None,
        electrochemical: None,
        ui: None,
        attributes: None,
        tags: None,
        parameters: None,
        extra: std::collections::HashMap::new(),
    }
}

/// Create a Model folder in `holder`, on disk and in the database, with its
/// record at `translation` and `rotation` relative to `holder`. Returns the
/// folder. The entity is spawned by the caller.
fn create_model_folder(
    world: &mut World,
    holder: &Path,
    translation: Vec3,
    rotation: Quat,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(holder).map_err(|e| format!("create {}: {e}", holder.display()))?;
    let name = crate::space::instance_loader::unique_entity_name(holder, "Model");
    let dir = holder.join(&name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut def = model_definition();
    crate::space::instance_loader::set_authored_transform(&mut def, translation, rotation, None);
    let stamp = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);
    let text = crate::space::instance_loader::signed_write_text(None, &mut def, stamp.as_ref())?
        .ok_or_else(|| "the new Model had no record to write".to_string())?;
    write_text_atomic(world, &dir.join(INSTANCE_FILE), &text)?;
    crate::space::active_db::seed_subtree(&dir);
    Ok(dir)
}

/// The record of a saved folder, read from its file.
fn load_node_def(dir: &Path) -> Result<crate::space::instance_loader::InstanceDefinition, String> {
    let toml = dir.join(INSTANCE_FILE);
    let text = std::fs::read_to_string(&toml).map_err(|e| format!("read {}: {e}", toml.display()))?;
    crate::space::instance_loader::load_instance_definition_from_str(&text)
}

/// The service a folder belongs to: the first folder under the Space root.
fn service_of(world: &World, dir: &Path) -> String {
    world
        .get_resource::<crate::space::SpaceRoot>()
        .and_then(|root| dir.strip_prefix(&root.0).ok())
        .and_then(|rel| rel.iter().next())
        .and_then(|name| name.to_str())
        .unwrap_or("Workspace")
        .to_string()
}

/// Load a saved folder into the ECS under `parent`, and register its folder and
/// its record, so the file watcher does not spawn it a second time. `None` when
/// it cannot be loaded (logged).
fn spawn_folder_node(world: &mut World, dir: &Path, parent: Option<Entity>) -> Option<Entity> {
    let toml = dir.join(INSTANCE_FILE);
    let def = match load_node_def(dir) {
        Ok(def) => def,
        Err(e) => {
            warn!("Group: could not read {}: {e}", toml.display());
            return None;
        }
    };
    let entity = match crate::tools_smart::spawn_instance_from_world(world, &toml, def) {
        Ok(entity) => entity,
        Err(e) => {
            warn!("Group: could not load {}: {e}", dir.display());
            return None;
        }
    };
    set_parent(world, entity, parent);
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Model")
        .to_string();
    let service = service_of(world, dir);
    let modified = std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let metadata = |path: PathBuf| crate::space::file_loader::FileMetadata {
        path,
        file_type: crate::space::file_loader::FileType::Toml,
        service: service.clone(),
        name: name.clone(),
        size: 0,
        modified,
        children: Vec::new(),
    };
    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        registry.register(dir.to_path_buf(), entity, metadata(dir.to_path_buf()));
        registry.register(toml.clone(), entity, metadata(toml.clone()));
    }
    Some(entity)
}

/// Bring a trashed folder back to `dir`, and seed the database with it. `false`
/// when it cannot.
fn restore_folder(dir: &Path, trash: &Path) -> bool {
    if !trash.exists() || dir.exists() {
        return false;
    }
    if let Some(parent) = dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !crate::undo::rename_with_retry(trash, dir) {
        return false;
    }
    crate::space::active_db::seed_subtree(dir);
    true
}

/// Move each member's folder to its far side, parent it under the folder that
/// now holds it, and write its pose. `forward` moves `from` to `to`; undo moves
/// back, last member first. Returns the members that moved.
pub(crate) fn relocate(world: &mut World, moves: &[FolderMove], forward: bool) -> Vec<FolderMove> {
    let ordered: Vec<&FolderMove> = if forward {
        moves.iter().collect()
    } else {
        moves.iter().rev().collect()
    };
    let mut done = Vec::new();
    for member in ordered {
        let (src, dst, translation, rotation) = if forward {
            (&member.from, &member.to, member.to_translation, member.to_rotation)
        } else {
            (&member.to, &member.from, member.from_translation, member.from_rotation)
        };
        if !crate::undo::move_folder_and_rebase(world, src, dst) {
            warn!("Group: could not move {:?} to {:?}", src, dst);
            continue;
        }
        let holder = dst.parent().map(Path::to_path_buf).unwrap_or_default();
        let parent = folder_entity(world, &holder);
        let entity = folder_entity(world, dst);
        let fold = entity.and_then(|e| world.get::<MeshFold>(e).copied());
        if let Some(entity) = entity {
            set_parent(world, entity, parent);
            set_local(world, entity, translation, rotation);
        }
        // A folder's record holds the file position, and a folded mesh's offset is
        // part of the drawn position only, so it is taken back out here. A loose
        // file has no record to write.
        if dst.is_dir() {
            let file_translation = crate::space::instance_loader::file_position(
                Vec3::from_array(translation),
                Quat::from_array(rotation),
                fold.as_ref(),
            );
            if let Err(e) = persist_local(world, &dst.join(INSTANCE_FILE), file_translation.to_array(), rotation) {
                warn!("Group: {e}");
            }
        }
        done.push(member.clone());
    }
    done
}

/// Undo of Ctrl+G: the members go back to their folders, and the Model is
/// trashed.
pub(crate) fn undo_group_folders(world: &mut World, model: &FolderNode, members: &[FolderMove]) {
    relocate(world, members, false);
    crate::undo::trash_created_folder(world, &model.dir, &model.trash, &[]);
    crate::undo::mark_explorer_dirty(world);
}

/// Redo of Ctrl+G: the Model comes back from the trash and the members move
/// into it again.
pub(crate) fn redo_group_folders(world: &mut World, model: &FolderNode, members: &[FolderMove]) {
    if !restore_folder(&model.dir, &model.trash) {
        warn!("Redo group: {:?} could not be restored from the trash", model.dir);
        return;
    }
    let parent = folder_entity(world, &model.parent_dir);
    let _ = spawn_folder_node(world, &model.dir, parent);
    relocate(world, members, true);
    crate::undo::mark_explorer_dirty(world);
}

/// Undo of Ctrl+U: the container comes back from the trash, and its children
/// move back into it.
pub(crate) fn undo_ungroup_folders(
    world: &mut World,
    container: &FolderNode,
    members: &[FolderMove],
) {
    if !restore_folder(&container.dir, &container.trash) {
        warn!("Undo ungroup: {:?} could not be restored from the trash", container.dir);
        return;
    }
    let parent = folder_entity(world, &container.parent_dir);
    let _ = spawn_folder_node(world, &container.dir, parent);
    relocate(world, members, false);
    crate::undo::mark_explorer_dirty(world);
}

/// Redo of Ctrl+U: the children move out again, and the container is trashed.
pub(crate) fn redo_ungroup_folders(
    world: &mut World,
    container: &FolderNode,
    members: &[FolderMove],
) {
    relocate(world, members, true);
    crate::undo::trash_created_folder(world, &container.dir, &container.trash, &[]);
    crate::undo::mark_explorer_dirty(world);
}

/// A selected object as Ctrl+G reads it: its folder, its world pose and its
/// local pose.
struct GroupInput {
    dir: PathBuf,
    world_t: Vec3,
    world_r: Quat,
    local_t: Vec3,
    local_r: Quat,
}

fn notify_warning(world: &mut World, text: String) {
    if let Some(mut notes) = world.get_resource_mut::<NotificationManager>() {
        notes.warning(text);
    }
}

fn notify_info(world: &mut World, text: String) {
    if let Some(mut notes) = world.get_resource_mut::<NotificationManager>() {
        notes.info(text);
    }
}

fn select_entities(world: &World, entities: &[Entity]) {
    if entities.is_empty() {
        return;
    }
    if let Some(selection) = world.get_resource::<SelectionSyncManager>() {
        selection
            .0
            .write()
            .set_selected(entities.iter().map(|e| entity_id_str(*e)).collect());
    }
}

/// Record one history step: a single action as it is, several under one batch.
fn push_undo(world: &mut World, label: String, mut actions: Vec<crate::undo::Action>) {
    let action = if actions.len() == 1 {
        actions.remove(0)
    } else {
        crate::undo::Action::Batch { actions }
    };
    if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
        undo.push_labeled(label, action);
    }
}

/// Ctrl+G on saved folders: a Model folder is created in the folder the
/// selection shares, and each selected folder moves into it. One undo step.
fn group_selection(world: &mut World, members: Vec<Entity>) {
    let Some(space_root) = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|root| root.0.clone())
    else {
        notify_warning(world, "Ctrl+G needs an open Space.".to_string());
        return;
    };

    // A member inside another selected member moves with it, so it is left out.
    let mut inputs: Vec<GroupInput> = Vec::new();
    for entity in members.iter().copied() {
        if has_selected_ancestor(world, entity, &members) {
            continue;
        }
        let Some(dir) = folder_of(world, entity) else {
            continue;
        };
        let (_, world_r, world_t) = world
            .get::<GlobalTransform>(entity)
            .map(|g| g.to_scale_rotation_translation())
            .unwrap_or((Vec3::ONE, Quat::IDENTITY, Vec3::ZERO));
        let (local_t, local_r) = local_pose(world, entity);
        inputs.push(GroupInput {
            dir,
            world_t,
            world_r,
            local_t,
            local_r,
        });
    }
    if inputs.len() < 2 {
        notify_warning(world, "Select at least 2 objects to group (Ctrl+G).".to_string());
        return;
    }

    // The Model goes in the deepest folder the members share.
    let parents: Vec<PathBuf> = inputs
        .iter()
        .filter_map(|input| input.dir.parent().map(Path::to_path_buf))
        .collect();
    let Some(holder_dir) = common_folder(&parents) else {
        notify_warning(world, "Ctrl+G: the selection shares no folder.".to_string());
        return;
    };
    let holder = match holder_of(world, &holder_dir) {
        Ok(holder) => holder,
        Err(message) => {
            notify_warning(world, format!("Ctrl+G: {message}."));
            return;
        }
    };

    // The Model sits at the members' mean position, in its holder's frame.
    let center = inputs.iter().map(|input| input.world_t).sum::<Vec3>() / inputs.len() as f32;
    let (model_t, model_r) = match holder.and_then(|h| world.get::<GlobalTransform>(h).copied()) {
        Some(holder_gt) => {
            let local = GlobalTransform::from_translation(center).reparented_to(&holder_gt);
            (local.translation, local.rotation)
        }
        None => (center, Quat::IDENTITY),
    };

    let model_dir = match create_model_folder(world, &holder_dir, model_t, model_r) {
        Ok(dir) => dir,
        Err(message) => {
            notify_warning(world, format!("Ctrl+G: {message}."));
            return;
        }
    };
    let model_trash = crate::undo::Action::reserve_trash_path(&space_root, &model_dir);
    let Some(model_entity) = spawn_folder_node(world, &model_dir, holder) else {
        crate::undo::trash_created_folder(world, &model_dir, &model_trash, &[]);
        notify_warning(
            world,
            "Ctrl+G: the new Model could not be loaded, so nothing was grouped.".to_string(),
        );
        return;
    };

    let mut moves: Vec<FolderMove> = Vec::new();
    for input in &inputs {
        let Some(name) = input.dir.file_name().map(|n| n.to_os_string()) else {
            continue;
        };
        // The Model has identity rotation and unit scale, so a member's local
        // pose is its world pose less the Model's position.
        let local_t = input.world_t - center;
        moves.push(FolderMove {
            from: input.dir.clone(),
            to: model_dir.join(&name),
            from_translation: input.local_t.to_array(),
            from_rotation: input.local_r.to_array(),
            to_translation: local_t.to_array(),
            to_rotation: input.world_r.to_array(),
        });
    }
    let moved = relocate(world, &moves, true);
    if moved.is_empty() {
        crate::undo::trash_created_folder(
            world,
            &model_dir,
            &model_trash,
            &[model_entity.to_bits()],
        );
        notify_warning(
            world,
            "Ctrl+G: no object could be moved into the new Model, so nothing was grouped."
                .to_string(),
        );
        return;
    }

    let count = moved.len();
    info!("Group: moved {count} folders into {:?}", model_dir);
    let model = FolderNode {
        dir: model_dir,
        trash: model_trash,
        parent_dir: holder_dir,
    };
    push_undo(
        world,
        format!("Group {count} objects"),
        vec![crate::undo::Action::GroupFolders {
            model,
            members: moved,
        }],
    );
    select_entities(world, &[model_entity]);
    crate::undo::mark_explorer_dirty(world);
    notify_info(world, format!("Grouped {count} objects into a Model"));
}

/// Ctrl+U on saved containers: each container's children move out beside it, and
/// the container's folder is trashed. One undo step per container, recorded
/// together under one history row. A container that cannot be dissolved is left as
/// it is, with a toast that says why.
fn ungroup_folders(world: &mut World, containers: Vec<Entity>) {
    let Some(space_root) = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|root| root.0.clone())
    else {
        notify_warning(world, "Ctrl+U needs an open Space.".to_string());
        return;
    };
    let mut freed: Vec<Entity> = Vec::new();
    let mut actions: Vec<crate::undo::Action> = Vec::new();
    let mut count = 0usize;
    for container in containers {
        let name = world
            .get::<Instance>(container)
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let Some(dir) = folder_of(world, container) else {
            notify_warning(
                world,
                format!(
                    "Ctrl+U skipped {name}: it is stored only in the Space database, so it cannot be ungrouped yet. Nothing was ungrouped there."
                ),
            );
            continue;
        };
        let Some(holder_dir) = dir.parent().map(Path::to_path_buf) else {
            continue;
        };
        let holder = match holder_of(world, &holder_dir) {
            Ok(holder) => holder,
            Err(message) => {
                notify_warning(
                    world,
                    format!("Ctrl+U skipped {name}: {message}. Nothing was ungrouped there."),
                );
                continue;
            }
        };
        let kids: Vec<Entity> = world
            .get::<Children>(container)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        if kids.is_empty() {
            continue;
        }

        // Each child is a saved folder or a loose file, and its name is free beside
        // the container.
        let parent_gt = holder
            .and_then(|h| world.get::<GlobalTransform>(h).copied())
            .unwrap_or_default();
        let mut moves: Vec<FolderMove> = Vec::new();
        let mut refusal: Option<String> = None;
        for kid in &kids {
            let Some(kid_path) = folder_of(world, *kid).or_else(|| loose_file_of(world, *kid))
            else {
                refusal = Some(
                    "one of its children is not saved as a folder or a file. Save that child first, then ungroup."
                        .to_string(),
                );
                break;
            };
            let Some(child_name) = kid_path.file_name().map(|n| n.to_os_string()) else {
                refusal = Some("one of its children has no name.".to_string());
                break;
            };
            let to = holder_dir.join(&child_name);
            if to.exists() {
                refusal = Some(format!(
                    "something named {} is already beside it.",
                    child_name.to_string_lossy()
                ));
                break;
            }
            let (from_t, from_r) = local_pose(world, *kid);
            let child_world = world
                .get::<GlobalTransform>(*kid)
                .copied()
                .unwrap_or_default();
            let new_local = child_world.reparented_to(&parent_gt);
            moves.push(FolderMove {
                from: kid_path,
                to,
                from_translation: from_t.to_array(),
                from_rotation: from_r.to_array(),
                to_translation: new_local.translation.to_array(),
                to_rotation: new_local.rotation.to_array(),
            });
        }
        if let Some(reason) = refusal {
            notify_warning(
                world,
                format!("Ctrl+U skipped {name}: {reason} Nothing was ungrouped there."),
            );
            continue;
        }

        // The container's record takes its current pose before the folder is
        // trashed, so undo restores what was on screen.
        let toml = dir.join(INSTANCE_FILE);
        let (container_t, container_r) = local_pose(world, container);
        let saved = match ensure_on_disk(world, &toml) {
            Ok(()) => persist_local(world, &toml, container_t.to_array(), container_r.to_array()),
            Err(e) => Err(e),
        };
        if let Err(e) = saved {
            notify_warning(
                world,
                format!("Ctrl+U skipped {name}: {e}. Nothing was ungrouped there."),
            );
            continue;
        }

        let done = relocate(world, &moves, true);
        if done.len() < moves.len() {
            relocate(world, &done, false);
            notify_warning(
                world,
                format!(
                    "Ctrl+U skipped {name}: not every child could be moved. Nothing was ungrouped there."
                ),
            );
            continue;
        }
        let trash = crate::undo::Action::reserve_trash_path(&space_root, &dir);
        if !crate::undo::trash_created_folder(world, &dir, &trash, &[container.to_bits()]) {
            relocate(world, &done, false);
            notify_warning(
                world,
                format!(
                    "Ctrl+U skipped {name}: its folder could not be trashed. Nothing was ungrouped there."
                ),
            );
            continue;
        }
        freed.extend(kids.iter().copied());
        count += 1;
        actions.push(crate::undo::Action::UngroupFolders {
            container: FolderNode {
                dir,
                trash,
                parent_dir: holder_dir,
            },
            members: done,
        });
    }
    if actions.is_empty() {
        return;
    }
    let label = format!(
        "Ungroup {count} container{}",
        if count == 1 { "" } else { "s" }
    );
    push_undo(world, label, actions);
    select_entities(world, &freed);
    crate::undo::mark_explorer_dirty(world);
    notify_info(
        world,
        format!(
            "Ungrouped {count} container{}",
            if count == 1 { "" } else { "s" }
        ),
    );
}

/// Ctrl+G: group the selection. Only saved folders can be grouped. Anything else
/// is refused with a toast that says what to do (see `group_refusal`).
fn handle_group_action(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    q: Query<(
        Entity,
        &Instance,
        Option<&ServiceComponent>,
        Option<&InstanceFile>,
        Option<&LoadedFromFile>,
    )>,
    mut notifications: ResMut<NotificationManager>,
) {
    let Some(selection) = selection else {
        return;
    };
    for event in events.read() {
        if !matches!(event.action, Action::Group) {
            continue;
        }
        let selected: HashSet<String> = selection.0.read().get_selected().into_iter().collect();
        if selected.is_empty() {
            notifications.warning("Select objects to group (Ctrl+G).");
            continue;
        }

        let mut members: Vec<Entity> = Vec::new();
        let (mut unsaved, mut loose, mut flat, mut database) = (0usize, 0usize, 0usize, 0usize);
        for (e, inst, svc, file, loaded) in q.iter() {
            if !selected.contains(&entity_id_str(e)) {
                continue;
            }
            if svc.is_some() || inst.class_name.is_adornment() {
                continue;
            }
            match on_disk(file, loaded) {
                OnDisk::Folder => members.push(e),
                OnDisk::Unsaved => unsaved += 1,
                OnDisk::LooseFile => loose += 1,
                OnDisk::FlatFile => flat += 1,
                OnDisk::DatabaseOnly => database += 1,
            }
        }
        if let Some(message) = group_refusal(unsaved, loose, flat, database) {
            notifications.warning(message);
            continue;
        }
        if members.len() < 2 {
            notifications.warning("Select at least 2 objects to group (Ctrl+G).");
            continue;
        }
        commands.queue(move |world: &mut World| group_selection(world, members));
    }
}

/// Ctrl+U: dissolve each selected Model or Folder. A saved container moves its
/// children out on disk. Any other container is dissolved in memory, as it always
/// was.
fn handle_ungroup_action(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    // `InstanceFile` rides along in this query: it tells a saved container from
    // one that exists only in memory.
    instances: Query<(Entity, &Instance, Option<&InstanceFile>)>,
    children_q: Query<&Children>,
    child_of_q: Query<&ChildOf>,
    global_q: Query<&GlobalTransform>,
    // LOCAL transforms: `global_q` gives the world frame the reparent math needs,
    // this gives the frame undo has to restore. (Plain `//`: rustc rejects `///`
    // on a function parameter.)
    local_q: Query<&Transform>,
    mut notifications: ResMut<NotificationManager>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
) {
    let Some(selection) = selection else {
        return;
    };
    for event in events.read() {
        if !matches!(event.action, Action::Ungroup) {
            continue;
        }
        let selected: HashSet<String> = selection.0.read().get_selected().into_iter().collect();
        if selected.is_empty() {
            notifications.warning("Select a Model/Folder to ungroup (Ctrl+U)");
            continue;
        }

        let mut freed: Vec<String> = Vec::new();
        // One `UngroupedContainer` per dissolved in-memory container: the whole
        // multi-select becomes ONE undo entry, not one per Model.
        let mut undo_containers: Vec<UngroupedContainer> = Vec::new();
        // Saved containers, dissolved on disk by `ungroup_folders`.
        let mut folder_containers: Vec<Entity> = Vec::new();
        // In-memory containers that also have a folder on disk (a flat file).
        let mut folder_backed = 0usize;
        for (model_e, inst, inst_file) in instances.iter() {
            if !selected.contains(&entity_id_str(model_e)) {
                continue;
            }
            if !matches!(inst.class_name, ClassName::Model | ClassName::Folder) {
                continue;
            }
            let Ok(children) = children_q.get(model_e) else {
                continue;
            };
            // Bevy 0.19 `Children::iter()` yields `Entity` by value. Collect
            // up-front so the borrow on `children` ends before we issue commands.
            let kids: Vec<Entity> = children.iter().collect();
            if kids.is_empty() {
                continue;
            }
            if inst_file.is_some_and(|f| is_folder_form(&f.toml_path)) {
                // Its children are checked on disk when the container is dissolved.
                folder_containers.push(model_e);
                continue;
            }

            // In-memory container: raise the children one level, as before.
            let grandparent = child_of_q.get(model_e).ok().map(|c| c.0);
            let container_local = local_q
                .get(model_e)
                .ok()
                .copied()
                .unwrap_or(Transform::IDENTITY);
            let mut undo_children: Vec<GroupMember> = Vec::with_capacity(kids.len());
            for child in kids {
                // The local the child had INSIDE the container: the value undo
                // has to put back. Read before the rewrite below overwrites it.
                let old_local = local_q.get(child).ok().copied().unwrap_or(Transform::IDENTITY);
                // Preserve world transform across the re-parent. Bevy's
                // `reparented_to` / `compute_transform` do the math. Falls back
                // to the old local for children with no `GlobalTransform`.
                let mut new_local = old_local;
                if let Ok(child_gt) = global_q.get(child) {
                    new_local = match grandparent.and_then(|gp| global_q.get(gp).ok()) {
                        Some(gp_gt) => child_gt.reparented_to(gp_gt),
                        None => child_gt.compute_transform(),
                    };
                    commands.entity(child).insert(new_local);
                }
                match grandparent {
                    Some(gp) => {
                        commands.entity(child).insert(ChildOf(gp));
                    }
                    None => {
                        commands.entity(child).remove::<ChildOf>();
                    }
                }
                undo_children.push(GroupMember {
                    entity_bits: child.to_bits(),
                    // Recorded for completeness only: the pre-ungroup parent IS
                    // the container, and undo re-spawns that under a fresh id.
                    old_parent_bits: Some(model_e.to_bits()),
                    old_translation: old_local.translation.to_array(),
                    old_rotation: old_local.rotation.to_array(),
                    old_scale: old_local.scale.to_array(),
                    new_translation: new_local.translation.to_array(),
                    new_rotation: new_local.rotation.to_array(),
                    new_scale: new_local.scale.to_array(),
                });
                freed.push(entity_id_str(child));
            }
            if inst_file.is_some() {
                folder_backed += 1;
            }
            undo_containers.push(UngroupedContainer {
                class_name: inst.class_name.as_str().to_string(),
                name: inst.name.clone(),
                parent_bits: grandparent.map(|gp| gp.to_bits()),
                translation: container_local.translation.to_array(),
                rotation: container_local.rotation.to_array(),
                scale: container_local.scale.to_array(),
                children: undo_children,
            });
            // Children detached above (queued first); the Model is now
            // childless when this despawn applies.
            commands.entity(model_e).despawn();
        }

        let has_folders = !folder_containers.is_empty();
        if has_folders {
            commands.queue(move |world: &mut World| ungroup_folders(world, folder_containers));
        }
        if undo_containers.is_empty() {
            if !has_folders {
                notifications.warning("Ungroup: select a Model or Folder (Ctrl+U)");
            }
            continue;
        }

        // ONE entry for the whole selection, however many containers it held.
        // Ungroup touches no files, so the ECS payload is the complete record of
        // the scene-graph half.
        let container_count = undo_containers.len();
        let freed_count = freed.len();
        match undo {
            Some(ref mut u) => u.push_labeled(
                format!(
                    "Ungroup {} container{}",
                    container_count,
                    if container_count == 1 { "" } else { "s" },
                ),
                crate::undo::Action::UngroupEntities {
                    containers: undo_containers,
                },
            ),
            None => warn!(
                "Ungroup: no UndoStack resource, so dissolving {container_count} container(s) was NOT recorded and cannot be undone"
            ),
        }

        if !freed.is_empty() {
            selection.0.write().set_selected(freed);
        }
        if folder_backed > 0 {
            notifications.warning(format!(
                "Ungrouped {} container(s). {} had a flat file that stays on disk, so Ctrl+Z restores the grouping in the scene but not its file link.",
                container_count, folder_backed,
            ));
        } else {
            notifications.info(format!("Ungrouped {} container(s)", container_count));
        }
        info!(
            "Ungroup: dissolved {} container(s), freed {} children",
            container_count, freed_count,
        );
    }
}

/// Plugin wiring Group/Ungroup. Add in `main`.
pub struct GroupingPlugin;

impl Plugin for GroupingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (handle_group_action, handle_ungroup_action).run_if(crate::play_mode::editor_input_enabled),
        );
    }
}

#[cfg(test)]
mod folder_group_tests {
    use super::*;

    const IDENTITY_ROT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress_group_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Workspace")).unwrap();
        root
    }

    /// A saved part in `dir` whose record holds the position `x`.
    fn part(dir: &Path, x: f32) {
        std::fs::create_dir_all(dir).unwrap();
        let text = format!(
            "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [{x}, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n"
        );
        std::fs::write(dir.join(INSTANCE_FILE), text).unwrap();
    }

    /// The x position the record in `dir` holds.
    fn x_of(dir: &Path) -> f32 {
        let text = std::fs::read_to_string(dir.join(INSTANCE_FILE)).unwrap();
        crate::space::instance_loader::load_instance_definition_from_str(&text)
            .unwrap()
            .transform
            .position[0]
    }

    /// Ctrl+G on three parts at x = 0, 2 and 4 with the Model at x = 2: the
    /// parts move into the Model folder, and their records hold their poses
    /// relative to it. Returns the Model folder, its trash path and the moves.
    fn group_three(world: &mut World, root: &Path) -> (PathBuf, PathBuf, Vec<FolderMove>) {
        let holder = root.join("Workspace");
        let names = ["A", "B", "C"];
        for (i, name) in names.iter().enumerate() {
            part(&holder.join(name), i as f32 * 2.0);
        }
        let model = create_model_folder(world, &holder, Vec3::new(2.0, 0.0, 0.0), Quat::IDENTITY).unwrap();
        let moves: Vec<FolderMove> = names
            .iter()
            .enumerate()
            .map(|(i, name)| FolderMove {
                from: holder.join(name),
                to: model.join(name),
                from_translation: [i as f32 * 2.0, 0.0, 0.0],
                from_rotation: IDENTITY_ROT,
                to_translation: [i as f32 * 2.0 - 2.0, 0.0, 0.0],
                to_rotation: IDENTITY_ROT,
            })
            .collect();
        let done = relocate(world, &moves, true);
        assert_eq!(done.len(), 3, "every part moved");
        let trash = root.join(".eustress").join("trash").join("stamp").join("Model");
        (model, trash, done)
    }

    /// Ctrl+G moves the parts into a Model folder with their poses relative to
    /// it, and undo puts them back and trashes the Model.
    #[test]
    fn grouping_moves_the_parts_into_a_model_and_undo_returns_them() {
        let root = scratch("group");
        let mut world = World::new();
        let (model, trash, done) = group_three(&mut world, &root);
        let holder = root.join("Workspace");
        for name in ["A", "B", "C"] {
            assert!(model.join(name).join(INSTANCE_FILE).is_file(), "{name} is not in the Model");
            assert!(!holder.join(name).exists(), "{name} is still beside the Model");
        }
        assert!((x_of(&model.join("A")) + 2.0).abs() < 1e-5, "A is not relative to the Model");
        assert!((x_of(&model.join("C")) - 2.0).abs() < 1e-5, "C is not relative to the Model");

        let node = FolderNode {
            dir: model.clone(),
            trash: trash.clone(),
            parent_dir: holder.clone(),
        };
        undo_group_folders(&mut world, &node, &done);
        for name in ["A", "B", "C"] {
            assert!(holder.join(name).join(INSTANCE_FILE).is_file(), "{name} did not come back");
        }
        assert!(!model.exists(), "the Model folder is still on disk");
        assert!(trash.join(INSTANCE_FILE).is_file(), "the Model is not in the trash");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Redo of Ctrl+G brings the trashed Model back and moves the parts in again.
    #[test]
    fn redoing_a_group_restores_the_model_and_moves_the_parts_in() {
        let root = scratch("redo");
        let mut world = World::new();
        let (model, trash, done) = group_three(&mut world, &root);
        let holder = root.join("Workspace");
        let node = FolderNode {
            dir: model.clone(),
            trash: trash.clone(),
            parent_dir: holder.clone(),
        };
        undo_group_folders(&mut world, &node, &done);
        redo_group_folders(&mut world, &node, &done);
        for name in ["A", "B", "C"] {
            assert!(model.join(name).join(INSTANCE_FILE).is_file(), "{name} is not back in the Model");
            assert!(!holder.join(name).exists(), "{name} is beside the Model again");
        }
        assert!(model.join(INSTANCE_FILE).is_file(), "the Model did not come back");
        assert!(!trash.exists(), "the trashed Model is still there");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ctrl+U moves a container's children out and trashes the container; undo
    /// brings the container and its children back, and redo does it again.
    #[test]
    fn ungrouping_moves_the_children_out_and_undo_brings_the_container_back() {
        let root = scratch("ungroup");
        let holder = root.join("Workspace");
        let container = holder.join("Box");
        part(&container.join("A"), 1.0);
        part(&container.join("B"), 3.0);
        std::fs::write(
            container.join(INSTANCE_FILE),
            "[metadata]\nclass_name = \"Model\"\n\n[transform]\nposition = [0.0, 0.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n",
        )
        .unwrap();
        let mut world = World::new();
        let moves = vec![
            FolderMove {
                from: container.join("A"),
                to: holder.join("A"),
                from_translation: [1.0, 0.0, 0.0],
                from_rotation: IDENTITY_ROT,
                to_translation: [1.0, 0.0, 0.0],
                to_rotation: IDENTITY_ROT,
            },
            FolderMove {
                from: container.join("B"),
                to: holder.join("B"),
                from_translation: [3.0, 0.0, 0.0],
                from_rotation: IDENTITY_ROT,
                to_translation: [3.0, 0.0, 0.0],
                to_rotation: IDENTITY_ROT,
            },
        ];
        let done = relocate(&mut world, &moves, true);
        assert_eq!(done.len(), 2, "both children moved out");
        let trash = root.join(".eustress").join("trash").join("stamp").join("Box");
        assert!(
            crate::undo::trash_created_folder(&mut world, &container, &trash, &[]),
            "the container was not trashed"
        );
        assert!(!container.exists(), "the container is still on disk");

        let node = FolderNode {
            dir: container.clone(),
            trash: trash.clone(),
            parent_dir: holder.clone(),
        };
        undo_ungroup_folders(&mut world, &node, &done);
        assert!(container.join(INSTANCE_FILE).is_file(), "the container did not come back");
        assert!(container.join("A").join(INSTANCE_FILE).is_file(), "A is not back in the container");
        assert!(container.join("B").join(INSTANCE_FILE).is_file(), "B is not back in the container");
        assert!(!holder.join("A").exists() && !holder.join("B").exists(), "children are still outside");

        redo_ungroup_folders(&mut world, &node, &done);
        assert!(holder.join("A").join(INSTANCE_FILE).is_file(), "A did not leave again");
        assert!(!container.exists(), "the container is still on disk after redo");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Every refusal says what to do: an unsaved object says "Save first, then
    /// group", and the other kinds say which objects to take out of the selection.
    #[test]
    fn a_refused_group_says_what_to_do() {
        let unsaved = group_refusal(1, 0, 0, 0).unwrap();
        assert!(unsaved.contains("Save first, then group"), "{unsaved}");
        assert!(group_refusal(0, 0, 0, 0).is_none(), "nothing refused, nothing to say");
        let loose = group_refusal(0, 2, 0, 0).unwrap();
        assert!(loose.contains("Take them out of the selection"), "{loose}");
        let flat = group_refusal(0, 0, 1, 0).unwrap();
        assert!(flat.contains("Take it out of the selection"), "{flat}");
        let database = group_refusal(0, 0, 0, 3).unwrap();
        assert!(database.contains("Space database"), "{database}");
    }

    /// A selected object is classed by what is on disk for it: a folder with its
    /// record groups, a record with no file is database-only, and nothing at all
    /// is unsaved.
    #[test]
    fn selected_objects_are_classed_by_what_is_on_disk() {
        let root = scratch("classes");
        let dir = root.join("Workspace").join("Brick");
        part(&dir, 0.0);
        let folder = crate::space::instance_loader::InstanceFile {
            toml_path: dir.join(INSTANCE_FILE),
            mesh_path: PathBuf::new(),
            name: "Brick".to_string(),
        };
        assert_eq!(on_disk(Some(&folder), None), OnDisk::Folder);
        let gone = crate::space::instance_loader::InstanceFile {
            toml_path: root.join("Workspace").join("Gone").join(INSTANCE_FILE),
            mesh_path: PathBuf::new(),
            name: "Gone".to_string(),
        };
        assert_eq!(on_disk(Some(&gone), None), OnDisk::DatabaseOnly);
        assert_eq!(on_disk(None, None), OnDisk::Unsaved);
        let _ = std::fs::remove_dir_all(&root);
    }
}
