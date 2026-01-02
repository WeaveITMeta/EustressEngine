#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use crate::parts::{PartData, PartType};
use crate::rendering::BevyPartManager;

/// Maximum number of undo/redo actions to keep
const MAX_HISTORY_SIZE: usize = 50;

/// Action types that can be undone/redone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Create a new part
    CreatePart {
        id: u32,
        part_type: PartType,
        position: Vec3,
        parent: Option<u32>,
    },
    
    /// Delete a part
    DeletePart {
        data: PartData,
    },
    
    /// Move a part
    MovePart {
        id: u32,
        old_position: Vec3,
        new_position: Vec3,
    },
    
    /// Rotate a part
    RotatePart {
        id: u32,
        old_rotation: Vec3,
        new_rotation: Vec3,
    },
    
    /// Scale a part
    ScalePart {
        id: u32,
        old_scale: Vec3,
        new_scale: Vec3,
    },
    
    /// Change part color
    ChangeColor {
        id: u32,
        old_color: [f32; 4],
        new_color: [f32; 4],
    },
    
    /// Group parts together
    GroupParts {
        parent_id: u32,
        child_ids: Vec<u32>,
        old_parents: Vec<Option<u32>>,
    },
    
    /// Ungroup parts
    UngroupParts {
        parent_id: u32,
        child_ids: Vec<u32>,
        new_parents: Vec<Option<u32>>,
    },
    
    /// Batch multiple actions together (for multi-select operations)
    Batch {
        actions: Vec<Action>,
    },
    
    /// Change a property on a single entity
    ChangeProperty {
        id: u32,
        property: String,
        old_value: PropertyValueSnapshot,
        new_value: PropertyValueSnapshot,
    },
    
    /// Change a property on multiple entities (for multi-select)
    ChangePropertyMulti {
        /// Entity IDs and their old values
        entities: Vec<(u32, PropertyValueSnapshot)>,
        property: String,
        new_value: PropertyValueSnapshot,
    },
    
    /// Change Parameters component on an entity
    ChangeParameters {
        id: u32,
        /// Serialized old Parameters (JSON)
        old_params: String,
        /// Serialized new Parameters (JSON)
        new_params: String,
    },
    
    /// Change Parameters on multiple entities
    ChangeParametersMulti {
        /// Entity IDs and their old Parameters (serialized JSON)
        entities: Vec<(u32, String)>,
        /// New Parameters to apply (serialized JSON)
        new_params: String,
    },
    
    /// Change Folder domain configuration
    ChangeFolderDomain {
        id: u32,
        old_domain: Option<String>,
        new_domain: Option<String>,
        old_source_override: Option<String>,
        new_source_override: Option<String>,
    },
    
    /// Change Folder sync configuration
    ChangeFolderSyncConfig {
        id: u32,
        /// Serialized old DomainSyncConfig (JSON)
        old_config: Option<String>,
        /// Serialized new DomainSyncConfig (JSON)
        new_config: Option<String>,
    },
    
    /// Change Attributes on an entity
    ChangeAttributes {
        id: u32,
        /// Serialized old Attributes (JSON)
        old_attrs: String,
        /// Serialized new Attributes (JSON)
        new_attrs: String,
    },
    
    /// Change Tags on an entity
    ChangeTags {
        id: u32,
        old_tags: Vec<String>,
        new_tags: Vec<String>,
    },
    
    /// Add a single attribute
    AddAttribute {
        id: u32,
        key: String,
        /// Serialized AttributeValue (JSON)
        value: String,
    },
    
    /// Remove a single attribute
    RemoveAttribute {
        id: u32,
        key: String,
        /// Serialized old AttributeValue (JSON) for undo
        old_value: String,
    },
    
    /// Add a tag
    AddTag {
        id: u32,
        tag: String,
    },
    
    /// Remove a tag
    RemoveTag {
        id: u32,
        tag: String,
    },
    
    /// Transform multiple entities (move/rotate) - uses Entity bits for ECS compatibility
    TransformEntities {
        /// Entity bits and their old transforms (translation, rotation)
        old_transforms: Vec<(u64, [f32; 3], [f32; 4])>,
        /// Entity bits and their new transforms
        new_transforms: Vec<(u64, [f32; 3], [f32; 4])>,
    },
    
    /// Scale multiple entities (resize) - stores position and size changes
    ScaleEntities {
        /// Entity bits and their old state (translation, size)
        old_states: Vec<(u64, [f32; 3], [f32; 3])>,
        /// Entity bits and their new state (translation, size)
        new_states: Vec<(u64, [f32; 3], [f32; 3])>,
    },
}

/// Snapshot of a property value for undo/redo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValueSnapshot {
    String(String),
    Float(f32),
    Bool(bool),
    Vector3([f32; 3]),
    Color([f32; 4]),
    Material(String),
}

impl Action {
    /// Get a human-readable description of the action
    pub fn description(&self) -> String {
        match self {
            Action::CreatePart { .. } => "Create Part".to_string(),
            Action::DeletePart { .. } => "Delete Part".to_string(),
            Action::MovePart { .. } => "Move Part".to_string(),
            Action::RotatePart { .. } => "Rotate Part".to_string(),
            Action::ScalePart { .. } => "Scale Part".to_string(),
            Action::ChangeColor { .. } => "Change Color".to_string(),
            Action::GroupParts { child_ids, .. } => format!("Group {} Parts", child_ids.len()),
            Action::UngroupParts { child_ids, .. } => format!("Ungroup {} Parts", child_ids.len()),
            Action::Batch { actions } => format!("Batch ({} actions)", actions.len()),
            Action::ChangeProperty { property, .. } => format!("Change {}", property),
            Action::ChangePropertyMulti { entities, property, .. } => format!("Change {} on {} objects", property, entities.len()),
            Action::ChangeParameters { .. } => "Change Parameters".to_string(),
            Action::ChangeParametersMulti { entities, .. } => format!("Change Parameters on {} objects", entities.len()),
            Action::ChangeFolderDomain { new_domain, .. } => {
                match new_domain {
                    Some(d) => format!("Set domain to '{}'", d),
                    None => "Clear domain".to_string(),
                }
            }
            Action::ChangeFolderSyncConfig { .. } => "Change Folder sync config".to_string(),
            Action::ChangeAttributes { .. } => "Change Attributes".to_string(),
            Action::ChangeTags { .. } => "Change Tags".to_string(),
            Action::AddAttribute { key, .. } => format!("Add attribute '{}'", key),
            Action::RemoveAttribute { key, .. } => format!("Remove attribute '{}'", key),
            Action::AddTag { tag, .. } => format!("Add tag '{}'", tag),
            Action::RemoveTag { tag, .. } => format!("Remove tag '{}'", tag),
            Action::TransformEntities { old_transforms, .. } => format!("Transform {} objects", old_transforms.len()),
            Action::ScaleEntities { old_states, .. } => format!("Scale {} objects", old_states.len()),
        }
    }
}

/// Undo/Redo stack resource
#[derive(Resource, Default)]
pub struct UndoStack {
    /// Stack of undoable actions
    history: VecDeque<Action>,
    /// Current position in history (for redo)
    current_index: usize,
}

impl UndoStack {
    /// Push a new action onto the stack
    pub fn push(&mut self, action: Action) {
        // Remove any actions after current index (they were undone)
        self.history.truncate(self.current_index);
        
        // Add new action
        self.history.push_back(action);
        
        // Maintain max size
        if self.history.len() > MAX_HISTORY_SIZE {
            self.history.pop_front();
        } else {
            self.current_index += 1;
        }
    }
    
    /// Check if we can undo
    pub fn can_undo(&self) -> bool {
        self.current_index > 0
    }
    
    /// Check if we can redo
    pub fn can_redo(&self) -> bool {
        self.current_index < self.history.len()
    }
    
    /// Get the action to undo (if any)
    pub fn undo(&mut self) -> Option<Action> {
        if self.can_undo() {
            self.current_index -= 1;
            self.history.get(self.current_index).cloned()
        } else {
            None
        }
    }
    
    /// Get the action to redo (if any)
    pub fn redo(&mut self) -> Option<Action> {
        if self.can_redo() {
            let action = self.history.get(self.current_index).cloned();
            self.current_index += 1;
            action
        } else {
            None
        }
    }
    
    /// Clear the entire history
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_index = 0;
    }
    
    /// Get the description of the last action (for UI display)
    pub fn last_action_description(&self) -> Option<String> {
        if self.current_index > 0 {
            self.history.get(self.current_index - 1).map(|a| a.description())
        } else {
            None
        }
    }
    
    /// Get the description of the next redo action (for UI display)
    pub fn next_redo_description(&self) -> Option<String> {
        if self.can_redo() {
            self.history.get(self.current_index).map(|a| a.description())
        } else {
            None
        }
    }
}

/// Event to request undo
#[derive(Message)]
pub struct UndoEvent;

/// Event to request redo
#[derive(Message)]
pub struct RedoEvent;

/// Plugin for undo/redo functionality
pub struct UndoPlugin;

impl Plugin for UndoPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<UndoStack>()
            .add_message::<UndoEvent>()
            .add_message::<RedoEvent>()
            .add_systems(Update, (handle_undo_events, handle_redo_events));
    }
}

/// System to handle undo events (Modern ECS with World access)
pub fn handle_undo_events(world: &mut World) {
    // Get events
    let mut undo_events = world.resource_mut::<Messages<UndoEvent>>();
    let events: Vec<_> = undo_events.drain().collect();
    drop(undo_events);
    
    if events.is_empty() {
        return;
    }
    
    let mut undo_stack = world.resource_mut::<UndoStack>();
    let actions: Vec<_> = events.iter().filter_map(|_| undo_stack.undo()).collect();
    drop(undo_stack);
    
    let had_actions = !actions.is_empty();
    
    for action in actions {
        info!("Undoing: {}", action.description());
        apply_undo_ecs(&action, world);
        
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.info(format!("↶ Undid: {}", action.description()));
    }
    
    // Show warning if there was nothing to undo
    if !events.is_empty() && !had_actions {
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.warning("Nothing to undo");
    }
}

/// System to handle redo events (Modern ECS with World access)
fn handle_redo_events(world: &mut World) {
    // Get events
    let mut redo_events = world.resource_mut::<Messages<RedoEvent>>();
    let events: Vec<_> = redo_events.drain().collect();
    drop(redo_events);
    
    if events.is_empty() {
        return;
    }
    
    let mut undo_stack = world.resource_mut::<UndoStack>();
    let actions: Vec<_> = events.iter().filter_map(|_| undo_stack.redo()).collect();
    drop(undo_stack);
    
    let had_actions = !actions.is_empty();
    
    for action in actions {
        info!("Redoing: {}", action.description());
        apply_redo_ecs(&action, world);
        
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.info(format!("↷ Redid: {}", action.description()));
    }
    
    // Show warning if there was nothing to redo
    if !events.is_empty() && !had_actions {
        let mut notifications = world.resource_mut::<crate::notifications::NotificationManager>();
        notifications.warning("Nothing to redo");
    }
}

/// Public function to apply undo action (called from keyboard shortcuts)
pub fn apply_undo_action(action: &Action, world: &mut World) {
    apply_undo_ecs(action, world);
}

/// Public function to apply redo action (called from keyboard shortcuts)
pub fn apply_redo_action(action: &Action, world: &mut World) {
    apply_redo_ecs(action, world);
}

/// Apply the inverse of an action using modern ECS (for undo)
fn apply_undo_ecs(action: &Action, world: &mut World) {
    #[allow(unused_imports)]
    use crate::classes::BasePart;
    
    match action {
        Action::DeletePart { data } => {
            // Undo delete = recreate entity
            let mut entity = world.spawn((
                crate::classes::Instance {
                    name: data.name.clone(),
                    class_name: crate::classes::ClassName::Part,
                    archivable: true,
                    id: data.id,
                },
                Name::new(data.name.clone()),
            ));
            
            // Add BasePart if it existed
            if let Some(transform_data) = data.parent {
                // Restore transform/basepart
                entity.insert(Transform::from_translation(Vec3::from(data.position)));
            }
        }
        Action::MovePart { id, old_position, .. } => {
            // Restore old position - query by ID and update Transform
            let mut query = world.query::<(&crate::classes::Instance, &mut Transform)>();
            for (instance, mut transform) in query.iter_mut(world) {
                if instance.id == *id {
                    transform.translation = *old_position;
                    break;
                }
            }
        }
        Action::ChangeProperty { id, property, old_value, .. } => {
            // Restore old property value
            apply_property_value_to_entity(*id, property, old_value, world);
        }
        Action::ChangePropertyMulti { entities, property, .. } => {
            // Restore old property values for each entity
            for (id, old_value) in entities {
                apply_property_value_to_entity(*id, property, old_value, world);
            }
        }
        Action::Batch { actions } => {
            // Undo all actions in reverse order
            for action in actions.iter().rev() {
                apply_undo_ecs(action, world);
            }
        }
        Action::ChangeParameters { id, old_params, .. } => {
            // Restore old Parameters
            apply_parameters_to_entity(*id, old_params, world);
        }
        Action::ChangeParametersMulti { entities, .. } => {
            // Restore old Parameters for each entity
            for (id, old_params) in entities {
                apply_parameters_to_entity(*id, old_params, world);
            }
        }
        Action::ChangeFolderDomain { id, old_domain, old_source_override, .. } => {
            apply_folder_domain(*id, old_domain.clone(), old_source_override.clone(), world);
        }
        Action::ChangeFolderSyncConfig { id, old_config, .. } => {
            apply_folder_sync_config(*id, old_config.clone(), world);
        }
        Action::ChangeAttributes { id, old_attrs, .. } => {
            apply_attributes_to_entity(*id, old_attrs, world);
        }
        Action::ChangeTags { id, old_tags, .. } => {
            apply_tags_to_entity(*id, old_tags.clone(), world);
        }
        Action::AddAttribute { id, key, .. } => {
            // Undo add = remove
            remove_attribute_from_entity(*id, key, world);
        }
        Action::RemoveAttribute { id, key, old_value } => {
            // Undo remove = add back
            add_attribute_to_entity(*id, key, old_value, world);
        }
        Action::AddTag { id, tag } => {
            // Undo add = remove
            remove_tag_from_entity(*id, tag, world);
        }
        Action::RemoveTag { id, tag } => {
            // Undo remove = add back
            add_tag_to_entity(*id, tag, world);
        }
        Action::TransformEntities { old_transforms, .. } => {
            // Restore old transforms
            for (entity_bits, old_pos, old_rot) in old_transforms {
                let entity = Entity::from_bits(*entity_bits);
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*old_pos);
                        transform.rotation = Quat::from_array(*old_rot);
                    }
                    // Also update BasePart.cframe if present
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*old_pos);
                        bp.cframe.rotation = Quat::from_array(*old_rot);
                    }
                }
            }
        }
        Action::ScaleEntities { old_states, .. } => {
            // Restore old positions and sizes, and regenerate meshes
            for (entity_bits, old_pos, old_size) in old_states {
                let entity = Entity::from_bits(*entity_bits);
                let size = Vec3::from_array(*old_size);
                
                // Get part shape for mesh regeneration
                let part_shape = world.get::<crate::classes::Part>(entity)
                    .map(|p| p.shape);
                
                // Update transform and BasePart
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*old_pos);
                    }
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*old_pos);
                        bp.size = size;
                    }
                }
                
                // Regenerate mesh at new size
                if let Some(shape) = part_shape {
                    let new_mesh = world.resource_scope(|_world, mut meshes: Mut<Assets<Mesh>>| {
                        match shape {
                            crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                            crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                            crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                            _ => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                        }
                    });
                    if let Some(mut mesh3d) = world.get_mut::<Mesh3d>(entity) {
                        mesh3d.0 = new_mesh;
                    }
                }
            }
        }
        _ => {
            // TODO: Implement other action types as needed
            warn!("Undo not yet implemented for: {}", action.description());
        }
    }
}

/// Apply an action using modern ECS (for redo)
fn apply_redo_ecs(action: &Action, world: &mut World) {
    match action {
        Action::DeletePart { data } => {
            // Redo delete = despawn entity
            let mut query = world.query::<(Entity, &crate::classes::Instance)>();
            let entity_to_despawn: Option<Entity> = query
                .iter(world)
                .find(|(_, instance)| instance.id == data.id)
                .map(|(entity, _)| entity);
            
            if let Some(entity) = entity_to_despawn {
                world.despawn(entity);
            }
        }
        Action::MovePart { id, new_position, .. } => {
            // Apply new position
            let mut query = world.query::<(&crate::classes::Instance, &mut Transform)>();
            for (instance, mut transform) in query.iter_mut(world) {
                if instance.id == *id {
                    transform.translation = *new_position;
                    break;
                }
            }
        }
        Action::ChangeProperty { id, property, new_value, .. } => {
            // Apply new property value
            apply_property_value_to_entity(*id, property, new_value, world);
        }
        Action::ChangePropertyMulti { entities, property, new_value } => {
            // Apply new property value to all entities
            for (id, _) in entities {
                apply_property_value_to_entity(*id, property, new_value, world);
            }
        }
        Action::Batch { actions } => {
            // Redo all actions in order
            for action in actions {
                apply_redo_ecs(action, world);
            }
        }
        Action::ChangeParameters { id, new_params, .. } => {
            // Apply new Parameters
            apply_parameters_to_entity(*id, new_params, world);
        }
        Action::ChangeParametersMulti { entities, new_params } => {
            // Apply new Parameters to all entities
            for (id, _) in entities {
                apply_parameters_to_entity(*id, new_params, world);
            }
        }
        Action::ChangeFolderDomain { id, new_domain, new_source_override, .. } => {
            apply_folder_domain(*id, new_domain.clone(), new_source_override.clone(), world);
        }
        Action::ChangeFolderSyncConfig { id, new_config, .. } => {
            apply_folder_sync_config(*id, new_config.clone(), world);
        }
        Action::ChangeAttributes { id, new_attrs, .. } => {
            apply_attributes_to_entity(*id, new_attrs, world);
        }
        Action::ChangeTags { id, new_tags, .. } => {
            apply_tags_to_entity(*id, new_tags.clone(), world);
        }
        Action::AddAttribute { id, key, value } => {
            // Redo add = add
            add_attribute_to_entity(*id, key, value, world);
        }
        Action::RemoveAttribute { id, key, .. } => {
            // Redo remove = remove
            remove_attribute_from_entity(*id, key, world);
        }
        Action::AddTag { id, tag } => {
            // Redo add = add
            add_tag_to_entity(*id, tag, world);
        }
        Action::RemoveTag { id, tag } => {
            // Redo remove = remove
            remove_tag_from_entity(*id, tag, world);
        }
        Action::TransformEntities { new_transforms, .. } => {
            // Apply new transforms
            for (entity_bits, new_pos, new_rot) in new_transforms {
                let entity = Entity::from_bits(*entity_bits);
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*new_pos);
                        transform.rotation = Quat::from_array(*new_rot);
                    }
                    // Also update BasePart.cframe if present
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*new_pos);
                        bp.cframe.rotation = Quat::from_array(*new_rot);
                    }
                }
            }
        }
        Action::ScaleEntities { new_states, .. } => {
            // Apply new positions and sizes, and regenerate meshes
            for (entity_bits, new_pos, new_size) in new_states {
                let entity = Entity::from_bits(*entity_bits);
                let size = Vec3::from_array(*new_size);
                
                // Get part shape for mesh regeneration
                let part_shape = world.get::<crate::classes::Part>(entity)
                    .map(|p| p.shape);
                
                // Update transform and BasePart
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut transform) = entity_mut.get_mut::<Transform>() {
                        transform.translation = Vec3::from_array(*new_pos);
                    }
                    if let Some(mut bp) = entity_mut.get_mut::<crate::classes::BasePart>() {
                        bp.cframe.translation = Vec3::from_array(*new_pos);
                        bp.size = size;
                    }
                }
                
                // Regenerate mesh at new size
                if let Some(shape) = part_shape {
                    let new_mesh = world.resource_scope(|_world, mut meshes: Mut<Assets<Mesh>>| {
                        match shape {
                            crate::classes::PartType::Block => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                            crate::classes::PartType::Ball => meshes.add(bevy::math::primitives::Sphere::new(size.x / 2.0)),
                            crate::classes::PartType::Cylinder => meshes.add(bevy::math::primitives::Cylinder::new(size.x / 2.0, size.y)),
                            _ => meshes.add(bevy::math::primitives::Cuboid::from_size(size)),
                        }
                    });
                    if let Some(mut mesh3d) = world.get_mut::<Mesh3d>(entity) {
                        mesh3d.0 = new_mesh;
                    }
                }
            }
        }
        _ => {
            // TODO: Implement other action types as needed
            warn!("Redo not yet implemented for: {}", action.description());
        }
    }
}

/// Apply a property value snapshot to an entity by ID
fn apply_property_value_to_entity(id: u32, property: &str, value: &PropertyValueSnapshot, world: &mut World) {
    use crate::classes::{BasePart, Instance};
    
    // Find entity by Instance ID
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for property undo/redo", id);
        return;
    };
    
    match (property, value) {
        ("Name", PropertyValueSnapshot::String(name)) => {
            if let Some(mut inst) = world.get_mut::<Instance>(entity) {
                inst.name = name.clone();
            }
        }
        ("Position", PropertyValueSnapshot::Vector3(pos)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.cframe.translation = Vec3::from_array(*pos);
            }
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.translation = Vec3::from_array(*pos);
            }
        }
        ("Orientation", PropertyValueSnapshot::Vector3(rot)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.cframe.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    rot[0].to_radians(),
                    rot[1].to_radians(),
                    rot[2].to_radians(),
                );
            }
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                transform.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    rot[0].to_radians(),
                    rot[1].to_radians(),
                    rot[2].to_radians(),
                );
            }
        }
        ("Size", PropertyValueSnapshot::Vector3(size)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.size = Vec3::from_array(*size);
            }
        }
        ("Color", PropertyValueSnapshot::Color(rgba)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
        }
        ("Material", PropertyValueSnapshot::Material(mat_str)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                // Parse material from string
                bp.material = match mat_str.as_str() {
                    "Plastic" => crate::classes::Material::Plastic,
                    "SmoothPlastic" => crate::classes::Material::SmoothPlastic,
                    "Wood" => crate::classes::Material::Wood,
                    "WoodPlanks" => crate::classes::Material::WoodPlanks,
                    "Metal" => crate::classes::Material::Metal,
                    "CorrodedMetal" => crate::classes::Material::CorrodedMetal,
                    "DiamondPlate" => crate::classes::Material::DiamondPlate,
                    "Foil" => crate::classes::Material::Foil,
                    "Grass" => crate::classes::Material::Grass,
                    "Concrete" => crate::classes::Material::Concrete,
                    "Brick" => crate::classes::Material::Brick,
                    "Granite" => crate::classes::Material::Granite,
                    "Marble" => crate::classes::Material::Marble,
                    "Slate" => crate::classes::Material::Slate,
                    "Sand" => crate::classes::Material::Sand,
                    "Fabric" => crate::classes::Material::Fabric,
                    "Glass" => crate::classes::Material::Glass,
                    "Neon" => crate::classes::Material::Neon,
                    "Ice" => crate::classes::Material::Ice,
                    _ => crate::classes::Material::Plastic,
                };
            }
        }
        ("Transparency", PropertyValueSnapshot::Float(t)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.transparency = *t;
            }
        }
        ("Reflectance", PropertyValueSnapshot::Float(r)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.reflectance = *r;
            }
        }
        ("Anchored", PropertyValueSnapshot::Bool(a)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.anchored = *a;
            }
        }
        ("CanCollide", PropertyValueSnapshot::Bool(c)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.can_collide = *c;
            }
        }
        ("CanTouch", PropertyValueSnapshot::Bool(ct)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.can_touch = *ct;
            }
        }
        ("Locked", PropertyValueSnapshot::Bool(l)) => {
            if let Some(mut bp) = world.get_mut::<BasePart>(entity) {
                bp.locked = *l;
            }
        }
        _ => {
            warn!("Unknown property for undo/redo: {}", property);
        }
    }
}

/// Apply serialized Parameters JSON to an entity by ID
fn apply_parameters_to_entity(id: u32, params_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::parameters::Parameters;
    
    // Find entity by Instance ID
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Parameters undo/redo", id);
        return;
    };
    
    // Deserialize Parameters from JSON
    match serde_json::from_str::<Parameters>(params_json) {
        Ok(params) => {
            // Insert or replace Parameters component
            world.entity_mut(entity).insert(params);
            info!("Applied Parameters to entity {}", id);
        }
        Err(e) => {
            warn!("Failed to deserialize Parameters for undo/redo: {}", e);
        }
    }
}

/// Create an undo action for a Parameters change
pub fn create_parameters_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_params: &eustress_common::parameters::Parameters,
) -> Option<Action> {
    let old_json = serde_json::to_string(old_params).ok()?;
    let new_json = serde_json::to_string(new_params).ok()?;
    
    Some(Action::ChangeParameters {
        id,
        old_params: old_json,
        new_params: new_json,
    })
}

/// Create an undo action for Parameters changes on multiple entities
pub fn create_parameters_multi_change_action(
    entities_old: Vec<(u32, &eustress_common::parameters::Parameters)>,
    new_params: &eustress_common::parameters::Parameters,
) -> Option<Action> {
    let new_json = serde_json::to_string(new_params).ok()?;
    
    let entities: Vec<(u32, String)> = entities_old
        .into_iter()
        .filter_map(|(id, params)| {
            serde_json::to_string(params).ok().map(|json| (id, json))
        })
        .collect();
    
    if entities.is_empty() {
        return None;
    }
    
    Some(Action::ChangeParametersMulti {
        entities,
        new_params: new_json,
    })
}

// ============================================================================
// Folder Domain/SyncConfig Helpers
// ============================================================================

/// Apply domain configuration to an entity's Parameters
fn apply_folder_domain(id: u32, domain: Option<String>, source_override: Option<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::parameters::Parameters;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for domain undo/redo", id);
        return;
    };
    
    if let Some(mut params) = world.get_mut::<Parameters>(entity) {
        params.domain = domain.unwrap_or_default();
        params.global_source_ref = source_override;
        info!("Applied domain to entity {}", id);
    } else {
        warn!("Entity {} does not have Parameters component", id);
    }
}

/// Apply sync config to an entity's Parameters
fn apply_folder_sync_config(id: u32, config_json: Option<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::classes::DomainSyncConfig;
    use eustress_common::parameters::Parameters;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for sync config undo/redo", id);
        return;
    };
    
    if let Some(mut params) = world.get_mut::<Parameters>(entity) {
        params.sync_config = config_json
            .and_then(|json| serde_json::from_str::<DomainSyncConfig>(&json).ok());
        info!("Applied sync config to entity {}", id);
    } else {
        warn!("Entity {} does not have Parameters component", id);
    }
}

// ============================================================================
// Attributes Helpers
// ============================================================================

/// Apply serialized attributes to an entity
fn apply_attributes_to_entity(id: u32, attrs_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::{Attributes, AttributeValue};
    use std::collections::HashMap;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Attributes undo/redo", id);
        return;
    };
    
    match serde_json::from_str::<HashMap<String, AttributeValue>>(attrs_json) {
        Ok(values) => {
            let mut attrs = Attributes::new();
            for (key, value) in values {
                attrs.set(key, value);
            }
            world.entity_mut(entity).insert(attrs);
            info!("Applied Attributes to entity {}", id);
        }
        Err(e) => {
            warn!("Failed to deserialize Attributes for undo/redo: {}", e);
        }
    }
}

/// Add a single attribute to an entity
fn add_attribute_to_entity(id: u32, key: &str, value_json: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::{Attributes, AttributeValue};
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for add attribute", id);
        return;
    };
    
    if let Ok(value) = serde_json::from_str::<AttributeValue>(value_json) {
        if let Some(mut attrs) = world.get_mut::<Attributes>(entity) {
            attrs.set(key.to_string(), value);
        } else {
            let mut attrs = Attributes::new();
            attrs.set(key.to_string(), value);
            world.entity_mut(entity).insert(attrs);
        }
    }
}

/// Remove a single attribute from an entity
fn remove_attribute_from_entity(id: u32, key: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Attributes;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for remove attribute", id);
        return;
    };
    
    if let Some(mut attrs) = world.get_mut::<Attributes>(entity) {
        attrs.remove(key);
    }
}

// ============================================================================
// Tags Helpers
// ============================================================================

/// Apply tags to an entity
fn apply_tags_to_entity(id: u32, tag_list: Vec<String>, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for Tags undo/redo", id);
        return;
    };
    
    let mut tags = Tags::new();
    for tag in tag_list {
        tags.add(tag);
    }
    world.entity_mut(entity).insert(tags);
    info!("Applied Tags to entity {}", id);
}

/// Add a single tag to an entity
fn add_tag_to_entity(id: u32, tag: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for add tag", id);
        return;
    };
    
    if let Some(mut tags) = world.get_mut::<Tags>(entity) {
        tags.add(tag.to_string());
    } else {
        let mut tags = Tags::new();
        tags.add(tag.to_string());
        world.entity_mut(entity).insert(tags);
    }
}

/// Remove a single tag from an entity
fn remove_tag_from_entity(id: u32, tag: &str, world: &mut World) {
    use crate::classes::Instance;
    use eustress_common::attributes::Tags;
    
    let entity = {
        let mut query = world.query::<(Entity, &Instance)>();
        query.iter(world).find(|(_, inst)| inst.id == id).map(|(e, _)| e)
    };
    
    let Some(entity) = entity else {
        warn!("Entity with ID {} not found for remove tag", id);
        return;
    };
    
    if let Some(mut tags) = world.get_mut::<Tags>(entity) {
        tags.remove(tag);
    }
}

// ============================================================================
// Action Creation Helpers
// ============================================================================

/// Create an undo action for a domain change (now uses Parameters)
pub fn create_folder_domain_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_domain: Option<String>,
    new_source_override: Option<String>,
) -> Action {
    Action::ChangeFolderDomain {
        id,
        old_domain: if old_params.domain.is_empty() { None } else { Some(old_params.domain.clone()) },
        new_domain,
        old_source_override: old_params.global_source_ref.clone(),
        new_source_override,
    }
}

/// Create an undo action for a sync config change (now uses Parameters)
pub fn create_folder_sync_config_change_action(
    id: u32,
    old_params: &eustress_common::parameters::Parameters,
    new_config: Option<&eustress_common::classes::DomainSyncConfig>,
) -> Option<Action> {
    let old_config = old_params.sync_config.as_ref()
        .and_then(|c| serde_json::to_string(c).ok());
    let new_config = new_config
        .and_then(|c| serde_json::to_string(c).ok());
    
    Some(Action::ChangeFolderSyncConfig {
        id,
        old_config,
        new_config,
    })
}

/// Create an undo action for adding an attribute
pub fn create_add_attribute_action(
    id: u32,
    key: String,
    value: &eustress_common::attributes::AttributeValue,
) -> Option<Action> {
    let value_json = serde_json::to_string(value).ok()?;
    Some(Action::AddAttribute { id, key, value: value_json })
}

/// Create an undo action for removing an attribute
pub fn create_remove_attribute_action(
    id: u32,
    key: String,
    old_value: &eustress_common::attributes::AttributeValue,
) -> Option<Action> {
    let old_value_json = serde_json::to_string(old_value).ok()?;
    Some(Action::RemoveAttribute { id, key, old_value: old_value_json })
}

/// Create an undo action for adding a tag
pub fn create_add_tag_action(id: u32, tag: String) -> Action {
    Action::AddTag { id, tag }
}

/// Create an undo action for removing a tag
pub fn create_remove_tag_action(id: u32, tag: String) -> Action {
    Action::RemoveTag { id, tag }
}

/// OLD LEGACY IMPLEMENTATION (kept for reference)
/// Apply the inverse of an action (for undo)
#[allow(dead_code, unused_variables)]
fn apply_undo(_action: &Action, _part_manager: &BevyPartManager) {
    // use crate::parts::PartUpdate;
    
    // OLD IMPLEMENTATION USING LEGACY BevyPartManager:
    /*
    
    match _action {
        Action::CreatePart { id, .. } => {
            // Undo create = delete
            let pm = _part_manager.0.write();
            let _ = pm.delete_part(*id);
        }
        Action::DeletePart { data } => {
            // Undo delete = recreate
            let pm = _part_manager.0.write();
            let new_id = pm.create_part(data.part_type, Vec3::from(data.position), Some(data.name.clone()));
            // Restore parent relationship if it had one
            if let Some(parent_id) = data.parent {
                let _ = pm.update_part(new_id, PartUpdate {
                    parent: Some(Some(parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::MovePart { id, old_position, .. } => {
            // Restore old position
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                position: Some(old_position.to_array()),
                ..Default::default()
            });
        }
        Action::RotatePart { id, old_rotation, .. } => {
            // Restore old rotation
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                rotation: Some(old_rotation.to_array()),
                ..Default::default()
            });
        }
        Action::ScalePart { id, old_scale, .. } => {
            // Restore old scale
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                size: Some(old_scale.to_array()),
                ..Default::default()
            });
        }
        Action::ChangeColor { id, old_color, .. } => {
            // Restore old color
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                color: Some(*old_color),
                ..Default::default()
            });
        }
        Action::GroupParts { child_ids, old_parents, .. } => {
            // Restore old parent relationships
            let pm = _part_manager.0.write();
            for (child_id, old_parent) in child_ids.iter().zip(old_parents.iter()) {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(*old_parent),
                    ..Default::default()
                });
            }
        }
        Action::UngroupParts { parent_id, child_ids, .. } => {
            // Restore grouped state - undo ungroup means re-group
            let pm = _part_manager.0.write();
            for child_id in child_ids {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::Batch { actions } => {
            // Undo batch in reverse order
            for action in actions.iter().rev() {
                apply_undo(action, _part_manager);
            }
        }
    }
    */
}

/// Apply an action (for redo)
/// TODO: Refactor to use ECS queries instead of BevyPartManager
#[allow(dead_code, unused_variables)]
fn apply_redo(_action: &Action, _part_manager: &BevyPartManager) {
    // use crate::parts::PartUpdate;
    
    // TODO: Reimplement using ECS queries
    /* OLD IMPLEMENTATION USING LEGACY BevyPartManager:
    
    match _action {
        Action::CreatePart { part_type, position, parent, .. } => {
            // Redo create
            let pm = _part_manager.0.write();
            let new_id = pm.create_part(*part_type, *position, None);
            // Set parent relationship if specified
            if let Some(parent_id) = parent {
                let _ = pm.update_part(new_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::DeletePart { data } => {
            // Redo delete
            let pm = _part_manager.0.write();
            let _ = pm.delete_part(data.id);
        }
        Action::MovePart { id, new_position, .. } => {
            // Apply new position
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                position: Some(new_position.to_array()),
                ..Default::default()
            });
        }
        Action::RotatePart { id, new_rotation, .. } => {
            // Apply new rotation
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                rotation: Some(new_rotation.to_array()),
                ..Default::default()
            });
        }
        Action::ScalePart { id, new_scale, .. } => {
            // Apply new scale
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                size: Some(new_scale.to_array()),
                ..Default::default()
            });
        }
        Action::ChangeColor { id, new_color, .. } => {
            // Apply new color
            let pm = _part_manager.0.write();
            let _ = pm.update_part(*id, PartUpdate {
                color: Some(*new_color),
                ..Default::default()
            });
        }
        Action::GroupParts { parent_id, child_ids, .. } => {
            // Apply grouping
            let pm = _part_manager.0.write();
            for child_id in child_ids {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(Some(*parent_id)),
                    ..Default::default()
                });
            }
        }
        Action::UngroupParts { child_ids, new_parents, .. } => {
            // Apply ungrouping
            let pm = _part_manager.0.write();
            for (child_id, new_parent) in child_ids.iter().zip(new_parents.iter()) {
                let _ = pm.update_part(*child_id, PartUpdate {
                    parent: Some(*new_parent),
                    ..Default::default()
                });
            }
        }
        Action::Batch { actions } => {
            // Redo batch in order
            for action in actions {
                apply_redo(action, _part_manager);
            }
        }
    }
    */
}
