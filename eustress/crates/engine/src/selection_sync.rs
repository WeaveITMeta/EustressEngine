use bevy::prelude::*;
use crate::selection_box::SelectionBox;
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

/// Plugin to synchronize SelectionManager state with Bevy SelectionBox components
pub struct SelectionSyncPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
}

impl Plugin for SelectionSyncPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(SelectionSyncManager(self.selection_manager.clone()))
            .init_resource::<SelectionGeneration>()
            .add_systems(Update, sync_selection_boxes);
    }
}

/// Get the part_id for an entity (from PartEntity or Instance)
fn get_part_id(entity: Entity, _part_entity: Option<&PartEntity>, instance: Option<&Instance>) -> Option<String> {
    // Always use entity ID format — must match part_selection_system's entity_to_id_string()
    if _part_entity.is_some() || instance.is_some() {
        return Some(format!("{}v{}", entity.index(), entity.generation()));
    }
    None
}

/// Check if an instance is an abstract/non-visual class that shouldn't show selection boxes
fn is_abstract_celestial(instance: Option<&Instance>) -> bool {
    if let Some(inst) = instance {
        ABSTRACT_CLASSES.contains(&inst.class_name)
    } else {
        false
    }
}

/// System to add/remove SelectionBox components based on SelectionManager state.
/// Uses generation tracking to skip the O(n) entity scan when selection hasn't changed.
fn sync_selection_boxes(
    mut commands: Commands,
    selection_manager: Option<Res<SelectionSyncManager>>,
    mut last_gen: ResMut<SelectionGeneration>,
    // Query entities that could be selected — matches part_selection_system's filter
    unselected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), (Without<SelectionBox>, Or<(With<PartEntity>, With<Instance>)>)>,
    selected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), With<SelectionBox>>,
) {
    let Some(selection_manager) = selection_manager else {
        warn!("SelectionSyncManager missing — selection outlines disabled");
        return;
    };
    let mgr = selection_manager.0.read();
    let current_gen = mgr.generation();

    // Fast path: nothing changed since last frame — skip entirely
    if current_gen == last_gen.0 {
        return;
    }
    last_gen.0 = current_gen;

    let selected_ids = mgr.get_selected();
    drop(mgr);
    let selected_set: std::collections::HashSet<String> = selected_ids.into_iter().collect();

    info!("[sel-sync] gen={} selected={:?} unselected_count={}", current_gen, selected_set, unselected_query.iter().count());

    let mut matched = 0;
    // Add SelectionBox to newly selected entities
    for (entity, part_entity, instance) in &unselected_query {
        // Skip abstract celestial services - they don't get selection boxes
        if is_abstract_celestial(instance) {
            continue;
        }

        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if selected_set.contains(&part_id) {
                commands.entity(entity).insert(SelectionBox);
                matched += 1;
                info!("[sel-sync] matched entity {:?} with id '{}'", entity, part_id);
            }
        }
    }
    if matched == 0 && !selected_set.is_empty() {
        // Log first 10 entity IDs to see what format they actually have
        let sample_ids: Vec<String> = unselected_query.iter().take(10)
            .filter_map(|(e, pe, inst)| get_part_id(e, pe, inst))
            .collect();
        warn!("[sel-sync] no entities matched selected IDs! wanted={:?} available_sample={:?}", selected_set, sample_ids);
    }

    // Remove SelectionBox from deselected entities
    for (entity, part_entity, instance) in &selected_query {
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if !selected_set.contains(&part_id) {
                commands.entity(entity).remove::<SelectionBox>();
            }
        }
    }
}
