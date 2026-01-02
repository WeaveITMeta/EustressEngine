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
    ClassName::Sun,
    ClassName::Moon,
    ClassName::Sky,
    ClassName::SoulScript,
    ClassName::Folder,
];

/// Resource wrapper for SelectionManager in selection sync
#[derive(Resource)]
pub struct SelectionSyncManager(pub Arc<RwLock<SelectionManager>>);

/// Plugin to synchronize SelectionManager state with Bevy SelectionBox components
pub struct SelectionSyncPlugin {
    pub selection_manager: Arc<RwLock<SelectionManager>>,
}

impl Plugin for SelectionSyncPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(SelectionSyncManager(self.selection_manager.clone()))
            .add_systems(Update, sync_selection_boxes);
    }
}

/// Get the part_id for an entity (from PartEntity or Instance)
fn get_part_id(entity: Entity, part_entity: Option<&PartEntity>, instance: Option<&Instance>) -> Option<String> {
    // Prefer PartEntity.part_id if available and non-empty
    if let Some(pe) = part_entity {
        if !pe.part_id.is_empty() {
            return Some(pe.part_id.clone());
        }
    }
    // Fall back to entity ID format if Instance exists
    if instance.is_some() {
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

/// System to add/remove SelectionBox components based on SelectionManager state
/// Supports both PartEntity (legacy) and Instance (modern) components
/// Excludes abstract celestial services (Atmosphere, Sun, Moon, Sky) from selection boxes
fn sync_selection_boxes(
    mut commands: Commands,
    selection_manager: Res<SelectionSyncManager>,
    // Query entities that could be selected (have PartEntity OR Instance)
    unselected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), (Without<SelectionBox>, Or<(With<PartEntity>, With<Instance>)>)>,
    selected_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>), With<SelectionBox>>,
) {
    let selected_ids = selection_manager.0.read().get_selected();
    let selected_set: std::collections::HashSet<String> = selected_ids.into_iter().collect();
    
    // Add SelectionBox to newly selected entities
    for (entity, part_entity, instance) in &unselected_query {
        // Skip abstract celestial services - they don't get selection boxes
        if is_abstract_celestial(instance) {
            continue;
        }
        
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if selected_set.contains(&part_id) {
                commands.entity(entity).insert(SelectionBox);
            }
        }
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
