use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::rendering::PartEntity;
use crate::classes::{Instance, ClassName};
use crate::commands::SelectionManager;
use std::sync::Arc;
use parking_lot::RwLock;

/// Classes that are abstract/non-visual and should not show selection boxes
const ABSTRACT_CLASSES: &[ClassName] = &[
    ClassName::Atmosphere,
    ClassName::Star,
    ClassName::Moon,
    ClassName::Sky,
    ClassName::SoulScript,
    ClassName::Folder,
];

/// Resource wrapper for SelectionManager in selection sync
#[derive(Resource)]
pub struct SelectionSyncManager(pub Arc<RwLock<SelectionManager>>);

/// Tracks the last-seen generation so we can skip frames where nothing changed.
#[derive(Resource, Default)]
struct SelectionGeneration(u64);

/// Tracks previous selection state for undo/redo recording.
#[derive(Resource, Default)]
struct PreviousSelection(Vec<String>);

/// Plugin to synchronize SelectionManager state with Bevy Selected components.
/// When Selected is added/removed, SelectionBoxPlugin spawns/despawns adornment meshes.
pub struct SelectionSyncPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
}

impl Plugin for SelectionSyncPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(SelectionSyncManager(self.selection_manager.clone()))
            .init_resource::<SelectionGeneration>()
            .init_resource::<PreviousSelection>()
            .add_systems(Update, (record_selection_history, sync_selection_components).chain());
    }
}

/// Get the part_id for an entity (from PartEntity or Instance)
fn get_part_id(entity: Entity, _part_entity: Option<&PartEntity>, instance: Option<&Instance>) -> Option<String> {
    if _part_entity.is_some() || instance.is_some() {
        return Some(format!("{}v{}", entity.index(), entity.generation()));
    }
    None
}

/// Check if an instance is an abstract/non-visual class that shouldn't show selection
fn is_abstract_celestial(instance: Option<&Instance>) -> bool {
    if let Some(inst) = instance {
        ABSTRACT_CLASSES.contains(&inst.class_name)
    } else {
        false
    }
}

/// Record selection changes into CommandHistory for Ctrl+Z/Y cycling.
/// Runs BEFORE sync_selection_components so the "previous" state is captured first.
fn record_selection_history(
    selection_manager: Option<Res<SelectionSyncManager>>,
    last_gen: Res<SelectionGeneration>,
    mut prev: ResMut<PreviousSelection>,
    mut history: Option<ResMut<crate::commands::CommandHistory>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(ref mut history) = history else { return };
    let mgr = selection_manager.0.read();
    let current_gen = mgr.generation();

    // Only record when generation changes
    if current_gen == last_gen.0 {
        return;
    }

    let current_ids = mgr.get_selected();
    drop(mgr);

    // Skip recording if previous and current are identical (can happen on first frame)
    if prev.0 == current_ids {
        return;
    }

    // Don't record into history via world — just push directly to the stack.
    // We can't call history.execute() because that requires &mut World.
    // Instead, push a Selection command with no-op execute (the sync system handles the actual state).
    let cmd = crate::commands::SelectionCommand::new(prev.0.clone(), current_ids.clone());
    history.push_selection(cmd);

    // Update previous state
    prev.0 = current_ids;
}

/// System to add/remove `Selected` components based on SelectionManager state.
/// Uses generation tracking to skip the O(n) entity scan when selection hasn't changed.
/// When `Selected` is added, SelectionBoxPlugin reacts via `Added<Selected>` to spawn meshes.
/// When `Selected` is removed, `RemovedComponents<Selected>` triggers despawn.
fn sync_selection_components(
    mut commands: Commands,
    selection_manager: Option<Res<SelectionSyncManager>>,
    mut last_gen: ResMut<SelectionGeneration>,
    unselected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), (Without<Selected>, Or<(With<eustress_common::default_scene::PartEntityMarker>, With<PartEntity>, With<Instance>)>)>,
    selected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), With<Selected>>,
) {
    let Some(selection_manager) = selection_manager else {
        return;
    };
    let mgr = selection_manager.0.read();
    let current_gen = mgr.generation();

    // Fast path: nothing changed since last frame
    if current_gen == last_gen.0 {
        return;
    }
    last_gen.0 = current_gen;

    let selected_ids = mgr.get_selected();
    drop(mgr);
    let selected_set: std::collections::HashSet<String> = selected_ids.into_iter().collect();

    // Add Selected to newly selected entities
    for (entity, part_entity, instance) in &unselected_query {
        if is_abstract_celestial(instance) {
            continue;
        }
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if selected_set.contains(&part_id) {
                commands.entity(entity).insert(Selected);
            }
        }
    }

    // Remove Selected from deselected entities
    for (entity, part_entity, instance) in &selected_query {
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if !selected_set.contains(&part_id) {
                commands.entity(entity).remove::<Selected>();
            }
        }
    }
}
