// Selection Sync - Keeps DynamicPropertiesPanel in sync with selected entities and services
// Phase 2, Week 1

use bevy::prelude::*;
use super::{BevySelectionManager, DynamicPropertiesPanel, ExplorerExpanded};
use crate::rendering::PartEntity;
use crate::classes::Instance;

/// Track last selection to avoid unnecessary updates
#[derive(Resource, Default)]
pub struct SelectionSyncState {
    last_selection: Option<String>,
    last_service: Option<super::explorer::ServiceType>,
}

/// Get the part_id for an entity (from PartEntity or Instance)
fn get_part_id(entity: Entity, part_entity: Option<&PartEntity>, instance: Option<&Instance>) -> Option<String> {
    // Prefer PartEntity.part_id if available and non-empty
    if let Some(pe) = part_entity {
        if !pe.part_id.is_empty() {
            return Some(pe.part_id.clone());
        }
    }
    // Fall back to Instance.name if available
    if let Some(inst) = instance {
        return Some(inst.name.clone());
    }
    None
}

/// System to sync selection from SelectionManager to DynamicPropertiesPanel
/// Uses part_id or Instance.name to find the corresponding entity
/// Also syncs service selection from ExplorerExpanded
pub fn sync_selection_to_properties(
    selection_manager: Res<BevySelectionManager>,
    explorer_expanded: Res<ExplorerExpanded>,
    mut dynamic_properties: ResMut<DynamicPropertiesPanel>,
    mut sync_state: ResMut<SelectionSyncState>,
    // Query entities with PartEntity OR Instance
    entity_query: Query<(Entity, Option<&PartEntity>, Option<&Instance>)>,
) {
    // First, check if a service is selected (takes priority)
    if let Some(service) = explorer_expanded.selected_service {
        if sync_state.last_service != Some(service) {
            info!("Selection sync: Service selected: {:?}", service);
            dynamic_properties.selected_service = Some(service);
            dynamic_properties.selected_entity = None; // Clear entity selection
            sync_state.last_service = Some(service);
            sync_state.last_selection = None;
        }
        return;
    } else if sync_state.last_service.is_some() {
        // Service was deselected
        info!("Selection sync: Service deselected");
        dynamic_properties.selected_service = None;
        sync_state.last_service = None;
    }
    
    let selected = selection_manager.0.read().get_selected();
    
    if selected.is_empty() {
        // No selection - clear state
        if dynamic_properties.selected_entity.is_some() {
            info!("Selection sync: Clearing entity selection (empty)");
            dynamic_properties.selected_entity = None;
        }
        sync_state.last_selection = None;
        return;
    }
    
    // Get first selected part_id (properties panel shows single selection)
    let first_selected = &selected[0];
    
    // Skip if selection hasn't changed
    if sync_state.last_selection.as_ref() == Some(first_selected) {
        return;
    }
    
    info!("Selection sync: Looking for entity with ID '{}'", first_selected);
    
    // Find entity by part_id or Instance.name
    let mut found_entity = None;
    for (entity, part_entity, instance) in entity_query.iter() {
        if let Some(part_id) = get_part_id(entity, part_entity, instance) {
            if &part_id == first_selected {
                info!("Selection sync: Found by part_id/name: {:?}", entity);
                found_entity = Some(entity);
                break;
            }
        }
    }
    
    // Also try to find by entity ID format (e.g., "123v4")
    if found_entity.is_none() {
        for (entity, _, _) in entity_query.iter() {
            // Format entity ID the same way we store it
            let entity_id = format!("{}v{}", entity.index(), entity.generation());
            if &entity_id == first_selected {
                info!("Selection sync: Found by entity ID format: {:?}", entity);
                found_entity = Some(entity);
                break;
            }
        }
    }
    
    if let Some(entity) = found_entity {
        // Update selected entity if changed
        if dynamic_properties.selected_entity != Some(entity) {
            info!("Selection sync: Setting selected entity to {:?}", entity);
            dynamic_properties.selected_entity = Some(entity);
            dynamic_properties.selected_service = None; // Clear service selection
        }
        sync_state.last_selection = Some(first_selected.clone());
    } else {
        // Part not found - log and clear
        warn!("Selection sync: Entity '{}' not found in query!", first_selected);
        if dynamic_properties.selected_entity.is_some() {
            dynamic_properties.selected_entity = None;
        }
        sync_state.last_selection = Some(first_selected.clone());
    }
}

/// Plugin for selection synchronization
pub struct SelectionSyncPlugin;

impl Plugin for SelectionSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionSyncState>()
            .add_systems(Update, sync_selection_to_properties);
    }
}
