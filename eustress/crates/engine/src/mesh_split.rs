//! The editor's `MeshSplitParts` and `MeshSplitEdges` actions: split the selected
//! Part's imported mesh into pieces in a new Model, and move the original to the
//! trash. One undo step restores the original and removes the Model.
//!
//! The planning (what the pieces are, and what is refused) is
//! `eustress_tools::mesh_tools`. This module writes the Model and its pieces,
//! moves the original with `space::trash`, and records the undo step the way
//! `csg.rs` does for a boolean.
//!
//! An MCP client or the Workshop reaches these through `invoke_action`, which the
//! permission gate classes as Destructive.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use eustress_common::instance_create::{create_instance, unique_entity_name, InstanceOverrides};
use eustress_tools::mesh_tools::{self, PiecePlan, Split};

use crate::keybindings::Action;
use crate::notifications::NotificationManager;
use crate::selection_sync::SelectionSyncManager;
use crate::space::instance_loader::InstanceFile;
use crate::space::trash::{trash_destination, trash_instance, TrashRequest};
use crate::space::SpaceFileRegistry;
use crate::ui::MenuActionEvent;

pub struct MeshSplitPlugin;

impl Plugin for MeshSplitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_mesh_split_actions);
    }
}

fn handle_mesh_split_actions(
    mut events: MessageReader<MenuActionEvent>,
    mut commands: Commands,
    selection: Option<Res<SelectionSyncManager>>,
    instance_files: Query<(Entity, &InstanceFile)>,
    mut notifications: ResMut<NotificationManager>,
    mut undo: Option<ResMut<crate::undo::UndoStack>>,
    mut registry: Option<ResMut<SpaceFileRegistry>>,
    space_root_res: Option<Res<crate::space::SpaceRoot>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
) {
    let Some(selection) = selection else {
        return;
    };
    let play_state = play_state.map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    for event in events.read() {
        let split = match event.action {
            Action::MeshSplitParts => Split::Parts,
            Action::MeshSplitEdges => Split::Edges,
            _ => continue,
        };
        // During Play a split waits for Stop: it writes a Model and its
        // pieces and moves the original to the trash. It is refused where it
        // is read, so the request is consumed and never runs after Stop.
        if let Some(notice) = crate::keybindings::play_refusal(play_state, event.action) {
            commands.queue(move |world: &mut World| crate::keybindings::say_play_refusal(world, notice));
            continue;
        }
        let space_root = crate::space::open_space_root(space_root_res.as_deref());
        let ids = selection.0.read().get_selected();
        let [id] = ids.as_slice() else {
            notifications.warning(format!("Select one Part to split (have {})", ids.len()));
            continue;
        };
        let Some((entity, file)) = instance_files.iter().find(|(e, _)| &entity_id_str(*e) == id) else {
            notifications.warning("The selected item has no folder on disk to split".to_string());
            continue;
        };
        let Some(folder) = file.toml_path.parent().map(Path::to_path_buf) else {
            continue;
        };
        let Some(base) = folder.parent().map(Path::to_path_buf) else {
            continue;
        };

        let universe_root = crate::space::universe_root_for_path(&space_root).unwrap_or_else(|| space_root.clone());
        let source = match mesh_tools::read_source(&folder, &space_root, &universe_root) {
            Ok(source) => source,
            Err(why) => {
                notifications.warning(why);
                continue;
            }
        };
        let plans = match mesh_tools::plan_pieces(&source, split) {
            Ok(plans) => plans,
            Err(why) => {
                notifications.warning(why);
                continue;
            }
        };

        // The Model and the pieces are written before anything moves, so a
        // failure here leaves the original where it was.
        let model_request = format!("{} segments", source.name);
        let created = match create_instance(
            &base,
            "Model",
            Some(model_request.as_str()),
            InstanceOverrides { display_name: Some(model_request.clone()), ..Default::default() },
        ) {
            Ok(created) => created,
            Err(e) => {
                notifications.warning(format!("Could not make the Model: {e}"));
                continue;
            }
        };
        let model_dir = created.folder_path.clone();
        if let Err(why) = write_pieces(&model_dir, &plans) {
            // Take the half-written Model back to the trash, so no partial Parts remain.
            let _ = send_to_trash(&model_dir);
            notifications.warning(format!("{why}; nothing was split"));
            continue;
        }

        // The original goes to the trash through the one shared path (Delete's).
        let original_trash = trash_destination(&folder);
        let request = TrashRequest {
            entity,
            source: &folder,
            record: &file.toml_path,
            destination: &original_trash,
        };
        if let Err(e) = trash_instance(&request, &mut commands, registry.as_deref_mut()) {
            let _ = send_to_trash(&model_dir);
            notifications.warning(format!("Could not move {} to the trash: {e}; nothing was split", source.name));
            continue;
        }
        commands.entity(entity).despawn();

        // One undo step: the original comes back and the Model goes to the trash.
        let model_trash = trash_destination(&model_dir);
        match undo.as_mut() {
            Some(u) => {
                u.push_labeled(
                    format!("Split {} ({} parts)", source.name, plans.len()),
                    crate::undo::Action::Batch {
                        actions: vec![
                            crate::undo::Action::TrashEntities {
                                paths: vec![(folder.clone(), original_trash)],
                            },
                            crate::undo::Action::SpawnFolders {
                                folders: vec![(model_dir.clone(), model_trash)],
                            },
                        ],
                    },
                );
            }
            None => {
                notifications.warning(format!("Split {}: this split cannot be undone", source.name));
            }
        }

        notifications.success(format!(
            "Split {} into {} parts in {}. Ctrl+Z restores it.",
            source.name,
            plans.len(),
            created.folder_name
        ));
    }
}

/// Write each piece as a Part folder inside `model_dir`: its mesh first, its
/// record last, so the watcher never sees a record that points at a missing mesh.
fn write_pieces(model_dir: &Path, plans: &[PiecePlan]) -> Result<(), String> {
    for plan in plans {
        let piece_dir = model_dir.join(unique_entity_name(model_dir, &plan.name));
        std::fs::create_dir_all(&piece_dir).map_err(|e| format!("could not make {}: {e}", plan.name))?;
        eustress_cad::write_glb(&piece_dir.join("mesh.glb"), &plan.mesh, None)
            .map_err(|e| format!("could not write {}'s mesh: {e}", plan.name))?;
        let text = toml::to_string(&plan.record).map_err(|e| format!("{}: {e}", plan.name))?;
        std::fs::write(piece_dir.join("_instance.toml"), text)
            .map_err(|e| format!("could not write {}'s record: {e}", plan.name))?;
    }
    Ok(())
}

/// Move `path` to its trash location (the same one a Delete uses), so a folder
/// this change made and must take back leaves no permanent delete behind.
fn send_to_trash(path: &Path) -> Result<PathBuf, String> {
    let dest = trash_destination(path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("trash: {e}"))?;
    }
    std::fs::rename(path, &dest).map_err(|e| format!("trash: {e}"))?;
    Ok(dest)
}

/// The id the selection holds for an entity (as `csg.rs` reads it).
fn entity_id_str(e: Entity) -> String {
    format!("{}v{}", e.index(), e.generation())
}

/// During Play a mesh split is refused where it is read, so the request is
/// consumed and nothing is written.
#[cfg(test)]
mod play_refusal_tests {
    use super::*;
    use crate::play_mode::PlayModeState;

    /// Run one split request in `state`, and return the toasts and the Output
    /// panel's lines it left.
    fn split(state: PlayModeState, action: Action) -> (Vec<String>, Vec<String>) {
        let mut app = App::new();
        app.add_message::<MenuActionEvent>();
        app.insert_resource(State::new(state));
        // `new()`, not the derived default, which keeps no notice.
        app.insert_resource(NotificationManager::new());
        app.init_resource::<crate::ui::OutputConsole>();
        app.insert_resource(SelectionSyncManager(std::sync::Arc::new(parking_lot::RwLock::new(crate::commands::SelectionManager::default()))));
        // An explicit root, so no fallback ever reads the settings file.
        app.insert_resource(crate::space::SpaceRoot(std::env::temp_dir().join("eustress_play_gate_split")));
        app.add_systems(Update, handle_mesh_split_actions);
        app.world_mut().write_message(MenuActionEvent::new(action));
        app.update();
        let world = app.world();
        (
            world.resource::<NotificationManager>().notifications.iter().map(|n| n.message.clone()).collect(),
            world.resource::<crate::ui::OutputConsole>().entries.iter().map(|e| e.message.clone()).collect(),
        )
    }

    #[test]
    fn play_refuses_a_split_and_editing_runs_it() {
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            for action in [Action::MeshSplitParts, Action::MeshSplitEdges] {
                let notice = crate::keybindings::play_refusal(state, action).unwrap();
                assert_eq!(split(state, action), (vec![notice.clone()], vec![notice]), "{state:?} {action:?}");
            }
        }
        // While editing the split runs, and with nothing selected says so.
        assert_eq!(split(PlayModeState::Editing, Action::MeshSplitParts).0, ["Select one Part to split (have 0)"]);
    }
}
