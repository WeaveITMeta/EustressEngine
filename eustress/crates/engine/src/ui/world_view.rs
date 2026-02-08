//! World View - Central system for UI panels to access World state safely
//! 
//! Architecture:
//! - UIWorldSnapshot: Read-only view of World state, updated each frame
//! - UIActionQueue: Actions from UI to be applied to World
//! - Systems: extract_world_snapshot (before UI), apply_ui_actions (after UI)

use bevy::prelude::*;
use bevy::ecs::world::EntityWorldMut;
use bevy::log::{info, warn};
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use crate::classes::{Instance, ClassName, BasePart};
use crate::play_mode::{PlayModeState, SpawnedDuringPlayMode};
use super::BevySelectionManager;

// ============================================================================
// World Snapshot - Read-only view for UI
// ============================================================================

/// Snapshot of an entity for UI display
#[derive(Clone, Debug)]
pub struct EntitySnapshot {
    pub entity: Entity,
    pub name: String,
    pub class_name: ClassName,
    pub parent: Option<Entity>,
    pub children: Vec<Entity>,
    /// Which service this entity belongs to (if explicitly set)
    pub service_owner: Option<super::explorer::ServiceType>,
    
    // === Transform/Geometry (BasePart) ===
    /// Position if it has BasePart
    pub position: Option<Vec3>,
    /// Orientation/Rotation in degrees (Euler angles) if it has BasePart
    pub orientation: Option<Vec3>,
    /// Size if it has BasePart  
    pub size: Option<Vec3>,
    /// Density in kg/m³ if it has BasePart
    pub density: Option<f32>,
    /// Mass in kg if it has BasePart
    pub mass: Option<f32>,
    /// Assembly mass in kg (computed total mass of all BasePart descendants)
    pub assembly_mass: Option<f32>,
    
    // === Appearance (BasePart) ===
    /// Color if it has BasePart
    pub color: Option<Color>,
    /// Material type if it has BasePart
    pub material: Option<crate::classes::Material>,
    /// Transparency if it has BasePart (0-1)
    pub transparency: Option<f32>,
    /// Reflectance if it has BasePart (0-1)
    pub reflectance: Option<f32>,
    
    // === Behavior (BasePart) ===
    /// Anchored if it has BasePart
    pub anchored: Option<bool>,
    /// CanCollide if it has BasePart
    pub can_collide: Option<bool>,
    /// CanTouch if it has BasePart
    pub can_touch: Option<bool>,
    /// Locked if it has BasePart
    pub locked: Option<bool>,
    
    // === Instance Properties ===
    /// AI training opt-in flag
    pub ai: Option<bool>,
    
    // === Attributes & Tags ===
    /// Attributes key-value pairs
    pub attributes: std::collections::HashMap<String, eustress_common::attributes::AttributeValue>,
    /// Tags set
    pub tags: std::collections::HashSet<String>,
    /// Has Parameters component
    pub has_parameters: bool,
    /// Data source type from Parameters component (if present)
    pub data_source_type: Option<eustress_common::parameters::DataSourceType>,
    
    // === Atmosphere Properties ===
    /// Atmosphere density (0.0 - 1.0)
    pub atmosphere_density: Option<f32>,
    /// Atmosphere offset
    pub atmosphere_offset: Option<f32>,
    /// Atmosphere glare (0.0 - 1.0)
    pub atmosphere_glare: Option<f32>,
    /// Atmosphere haze (0.0 - 1.0)
    pub atmosphere_haze: Option<f32>,
    /// Atmosphere color [r, g, b, a]
    pub atmosphere_color: Option<[f32; 4]>,
    /// Atmosphere decay color [r, g, b, a]
    pub atmosphere_decay: Option<[f32; 4]>,
    
    // === BillboardGui Properties ===
    pub billboard_active: Option<bool>,
    pub billboard_enabled: Option<bool>,
    pub billboard_always_on_top: Option<bool>,
    pub billboard_size: Option<bevy::math::Vec2>,
    pub billboard_units_offset: Option<bevy::math::Vec3>,
    pub billboard_max_distance: Option<f32>,
    pub billboard_brightness: Option<f32>,
    pub billboard_light_influence: Option<f32>,
    
    // === TextLabel Properties ===
    pub textlabel_text: Option<String>,
    pub textlabel_font_size: Option<f32>,
    pub textlabel_visible: Option<bool>,
    pub textlabel_text_color: Option<Color>,
    pub textlabel_background_transparency: Option<f32>,
}

/// Complete snapshot of World state for UI panels
#[derive(Resource, Default)]
pub struct UIWorldSnapshot {
    /// All entities with Instance component, keyed by Entity
    pub entities: HashMap<Entity, EntitySnapshot>,
    /// Root entities (no parent)
    pub roots: Vec<Entity>,
    /// Currently selected entity IDs (as strings for compatibility)
    pub selected: Vec<String>,
    /// Selected entities as Entity handles
    pub selected_entities: Vec<Entity>,
    /// Frame number when snapshot was taken
    pub frame: u64,
}

// ============================================================================
// LCD (Least Common Denominator) Values for Multi-Selection
// ============================================================================

/// LCD value that tracks whether all selected entities have the same value
#[derive(Clone, Debug)]
pub enum LcdValue<T> {
    /// All entities have the same value
    Same(T),
    /// Values differ across entities (show "—" or "(mixed)")
    Mixed,
    /// No entities have this property
    None,
}

impl<T: PartialEq + Clone> LcdValue<T> {
    /// Create LCD value from iterator of optional values
    pub fn from_iter<I: Iterator<Item = Option<T>>>(iter: I) -> Self {
        let values: Vec<Option<T>> = iter.collect();
        if values.is_empty() {
            return LcdValue::None;
        }
        
        // Filter to only Some values
        let some_values: Vec<&T> = values.iter().filter_map(|v| v.as_ref()).collect();
        
        if some_values.is_empty() {
            return LcdValue::None;
        }
        
        // Check if all values are the same
        let first = some_values[0];
        if some_values.iter().all(|v| *v == first) {
            LcdValue::Same(first.clone())
        } else {
            LcdValue::Mixed
        }
    }
}

/// LCD snapshot for multi-selection properties display
#[derive(Clone, Debug, Default)]
pub struct LcdSnapshot {
    /// Common class name (None if mixed classes)
    pub class_name: Option<ClassName>,
    /// Position LCD
    pub position: LcdValue<Vec3>,
    /// Orientation LCD
    pub orientation: LcdValue<Vec3>,
    /// Size LCD
    pub size: LcdValue<Vec3>,
    /// Color LCD
    pub color: LcdValue<Color>,
    /// Material LCD
    pub material: LcdValue<crate::classes::Material>,
    /// Transparency LCD
    pub transparency: LcdValue<f32>,
    /// Reflectance LCD
    pub reflectance: LcdValue<f32>,
    /// Anchored LCD
    pub anchored: LcdValue<bool>,
    /// CanCollide LCD
    pub can_collide: LcdValue<bool>,
    /// CanTouch LCD
    pub can_touch: LcdValue<bool>,
    /// Locked LCD
    pub locked: LcdValue<bool>,
}

impl Default for LcdValue<Vec3> {
    fn default() -> Self { LcdValue::None }
}
impl Default for LcdValue<Color> {
    fn default() -> Self { LcdValue::None }
}
impl Default for LcdValue<crate::classes::Material> {
    fn default() -> Self { LcdValue::None }
}
impl Default for LcdValue<f32> {
    fn default() -> Self { LcdValue::None }
}
impl Default for LcdValue<bool> {
    fn default() -> Self { LcdValue::None }
}

impl UIWorldSnapshot {
    /// Get entity snapshot by Entity
    pub fn get(&self, entity: Entity) -> Option<&EntitySnapshot> {
        self.entities.get(&entity)
    }
    
    /// Check if entity is selected
    pub fn is_selected(&self, entity: Entity) -> bool {
        self.selected_entities.contains(&entity)
    }
    
    /// Compute LCD (Least Common Denominator) snapshot for all selected entities
    /// Returns values that are common across all selected entities, or Mixed if they differ
    pub fn compute_lcd(&self) -> LcdSnapshot {
        let selected: Vec<&EntitySnapshot> = self.selected_entities.iter()
            .filter_map(|e| self.entities.get(e))
            .collect();
        
        if selected.is_empty() {
            return LcdSnapshot::default();
        }
        
        // Check if all have same class
        let first_class = selected[0].class_name;
        let class_name = if selected.iter().all(|e| e.class_name == first_class) {
            Some(first_class)
        } else {
            None
        };
        
        LcdSnapshot {
            class_name,
            position: LcdValue::from_iter(selected.iter().map(|e| e.position)),
            orientation: LcdValue::from_iter(selected.iter().map(|e| e.orientation)),
            size: LcdValue::from_iter(selected.iter().map(|e| e.size)),
            color: LcdValue::from_iter(selected.iter().map(|e| e.color)),
            material: LcdValue::from_iter(selected.iter().map(|e| e.material)),
            transparency: LcdValue::from_iter(selected.iter().map(|e| e.transparency)),
            reflectance: LcdValue::from_iter(selected.iter().map(|e| e.reflectance)),
            anchored: LcdValue::from_iter(selected.iter().map(|e| e.anchored)),
            can_collide: LcdValue::from_iter(selected.iter().map(|e| e.can_collide)),
            can_touch: LcdValue::from_iter(selected.iter().map(|e| e.can_touch)),
            locked: LcdValue::from_iter(selected.iter().map(|e| e.locked)),
        }
    }
    
    /// Get children of an entity
    pub fn children_of(&self, entity: Entity) -> Vec<&EntitySnapshot> {
        self.entities.get(&entity)
            .map(|e| e.children.iter()
                .filter_map(|c| self.entities.get(c))
                .collect())
            .unwrap_or_default()
    }
    
    /// Get all root entities
    pub fn root_entities(&self) -> Vec<&EntitySnapshot> {
        self.roots.iter()
            .filter_map(|e| self.entities.get(e))
            .collect()
    }
}

// ============================================================================
// UI Action Queue - Actions from UI to apply to World
// ============================================================================

/// Actions that UI can request
#[derive(Clone, Debug)]
pub enum UIAction {
    /// Select entities (replaces current selection)
    Select(Vec<Entity>),
    /// Add to selection
    AddToSelection(Entity),
    /// Remove from selection
    RemoveFromSelection(Entity),
    /// Clear selection
    ClearSelection,
    /// Clear service selection (when selecting an entity)
    ClearServiceSelection,
    /// Select a service (for viewing properties)
    SelectService(crate::ui::explorer::ServiceType),
    /// Delete entities
    Delete(Vec<Entity>),
    /// Spawn a new part
    SpawnPart {
        part_type: crate::classes::PartType,
        position: Vec3,
    },
    /// Spawn a spawn point
    SpawnSpawnPoint {
        position: Vec3,
    },
    /// Spawn a point light
    SpawnPointLight {
        position: Vec3,
    },
    /// Spawn a spot light
    SpawnSpotLight {
        position: Vec3,
    },
    /// Spawn an instance into a service
    SpawnIntoService {
        service: crate::ui::explorer::ServiceType,
        class_name: crate::classes::ClassName,
    },
    /// Rename entity
    Rename {
        entity: Entity,
        new_name: String,
    },
    /// Set parent
    SetParent {
        entity: Entity,
        parent: Option<Entity>,
    },
    /// Reparent entity (drag-and-drop in Explorer)
    Reparent {
        child: Entity,
        new_parent: Entity,
    },
    /// Duplicate entity
    Duplicate(Entity),
    /// Toggle explorer node expansion
    ToggleExpanded(Entity),
    /// Toggle service node expansion in explorer
    ToggleServiceExpanded(crate::ui::explorer::ServiceType),
    /// Open a Soul Script in the editor
    OpenScript(Entity),
    /// Set a property value on a single entity
    SetProperty {
        entity: Entity,
        property: String,
        value: crate::classes::PropertyValue,
    },
    /// Set a property value on multiple entities (for multi-select editing)
    SetPropertyMulti {
        entities: Vec<Entity>,
        property: String,
        value: crate::classes::PropertyValue,
    },
    
    // === Attributes Actions ===
    /// Set an attribute value
    SetAttribute {
        entity: Entity,
        key: String,
        value: eustress_common::attributes::AttributeValue,
    },
    /// Remove an attribute
    RemoveAttribute {
        entity: Entity,
        key: String,
    },
    /// Open add attribute dialog
    OpenAddAttributeDialog(Entity),
    
    // === Tags Actions ===
    /// Add a tag
    AddTag {
        entity: Entity,
        tag: String,
    },
    /// Remove a tag
    RemoveTag {
        entity: Entity,
        tag: String,
    },
    /// Open add tag dialog
    OpenAddTagDialog(Entity),
    
    // === Parameters Actions ===
    /// Add parameters component with a data source type
    AddParameters {
        entity: Entity,
        source_type: eustress_common::parameters::DataSourceType,
    },
    /// Remove parameters component
    RemoveParameters(Entity),
    /// Open parameters editor
    OpenParametersEditor(Entity),
    /// Open add parameters modal (to select data source type)
    OpenAddParametersDialog(Entity),
    
    // === Clipboard Actions ===
    /// Copy entities to clipboard
    Copy(Vec<Entity>),
    /// Cut entities to clipboard
    Cut(Vec<Entity>),
    /// Paste into entity
    PasteInto(Entity),
    /// Begin rename (focus rename field)
    BeginRename(Entity),
    
    // === Atmosphere Actions ===
    /// Set atmosphere preset (clear_day, sunset, foggy)
    SetAtmospherePreset {
        entity: Entity,
        preset: String,
    },
    
    // === Plugin Actions ===
    /// Trigger a plugin action by ID (e.g., "mindspace:add_label")
    PluginAction(String),
}

/// Queue of actions from UI to be processed
#[derive(Resource, Default)]
pub struct UIActionQueue {
    actions: Vec<UIAction>,
}

// ============================================================================
// Properties Panel Modal State
// ============================================================================

/// Modal dialogs for Properties panel
#[derive(Resource, Default)]
pub struct PropertiesModalState {
    /// Add Tag modal
    pub add_tag_open: bool,
    pub add_tag_entity: Option<Entity>,
    pub add_tag_custom: String,
    
    /// Add Attribute modal
    pub add_attr_open: bool,
    pub add_attr_entity: Option<Entity>,
    pub add_attr_name: String,
    pub add_attr_type: String,
    pub add_attr_value_str: String,
    pub add_attr_value_num: f64,
    pub add_attr_value_bool: bool,
    pub add_attr_value_vec3: [f32; 3],
    pub add_attr_value_vec2: [f32; 2],
    pub add_attr_value_color: [f32; 3],
    pub add_attr_value_int: i64,
    pub add_attr_value_brick_color: u32,
    pub add_attr_value_udim2: [f32; 4],  // scale_x, offset_x, scale_y, offset_y
    pub add_attr_value_rect: [f32; 4],   // min_x, min_y, max_x, max_y
    pub add_attr_value_font_family: String,
    pub add_attr_value_font_weight: u16,
    pub add_attr_value_range: [f64; 2],  // min, max
    
    /// Add Parameters modal
    pub add_params_open: bool,
    pub add_params_entity: Option<Entity>,
    pub add_params_category: String,
}

impl PropertiesModalState {
    pub fn open_add_tag(&mut self, entity: Entity) {
        self.add_tag_open = true;
        self.add_tag_entity = Some(entity);
        self.add_tag_custom.clear();
    }
    
    pub fn open_add_attribute(&mut self, entity: Entity) {
        self.add_attr_open = true;
        self.add_attr_entity = Some(entity);
        self.add_attr_name.clear();
        self.add_attr_type = "String".to_string();
        self.add_attr_value_str.clear();
        self.add_attr_value_num = 0.0;
        self.add_attr_value_bool = false;
        self.add_attr_value_vec3 = [0.0, 0.0, 0.0];
        self.add_attr_value_vec2 = [0.0, 0.0];
        self.add_attr_value_color = [1.0, 1.0, 1.0];
        self.add_attr_value_int = 0;
        self.add_attr_value_brick_color = 194;
        self.add_attr_value_udim2 = [0.0, 0.0, 0.0, 0.0];
        self.add_attr_value_rect = [0.0, 0.0, 1.0, 1.0];
        self.add_attr_value_font_family = "SourceSans".to_string();
        self.add_attr_value_font_weight = 400;
        self.add_attr_value_range = [0.0_f64, 1.0_f64];
    }
    
    pub fn open_add_parameters(&mut self, entity: Entity) {
        self.add_params_open = true;
        self.add_params_entity = Some(entity);
        self.add_params_category = "General Data Formats".to_string();
    }
    
    pub fn close_all(&mut self) {
        self.add_tag_open = false;
        self.add_attr_open = false;
        self.add_params_open = false;
    }
}

impl UIActionQueue {
    /// Add an action to the queue
    pub fn push(&mut self, action: UIAction) {
        self.actions.push(action);
    }
    
    /// Take all actions (clears the queue)
    pub fn drain(&mut self) -> Vec<UIAction> {
        std::mem::take(&mut self.actions)
    }
    
    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Calculate assembly mass by recursively summing BasePart masses of all descendants
fn calculate_assembly_mass(entity: Entity, snapshot: &UIWorldSnapshot) -> Option<f32> {
    let mut total_mass = 0.0_f32;
    let mut has_mass = false;
    
    // Helper function for recursive traversal
    fn traverse_children(entity: Entity, snapshot: &UIWorldSnapshot, total_mass: &mut f32, has_mass: &mut bool) {
        if let Some(entity_snapshot) = snapshot.entities.get(&entity) {
            // Add this entity's mass if it has BasePart
            if let Some(mass) = entity_snapshot.mass {
                *total_mass += mass;
                *has_mass = true;
            }
            
            // Recursively traverse children
            for child in &entity_snapshot.children {
                traverse_children(*child, snapshot, total_mass, has_mass);
            }
        }
    }
    
    traverse_children(entity, snapshot, &mut total_mass, &mut has_mass);
    
    if has_mass {
        Some(total_mass)
    } else {
        None
    }
}

/// System to extract World state into UIWorldSnapshot
/// Runs BEFORE UI systems
pub fn extract_world_snapshot(
    mut snapshot: ResMut<UIWorldSnapshot>,
    query: Query<(
        Entity, 
        &Instance, 
        Option<&BasePart>, 
        Option<&Children>, 
        Option<&ChildOf>, 
        Option<&super::ServiceOwner>,
        Option<&eustress_common::attributes::Attributes>,
        Option<&eustress_common::attributes::Tags>,
        Option<&eustress_common::parameters::Parameters>,
        Option<&eustress_common::classes::Atmosphere>,
        Option<&crate::classes::BillboardGui>,
        Option<&crate::classes::TextLabel>,
    )>,
    selection_manager: Option<Res<BevySelectionManager>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Clear previous snapshot
    snapshot.entities.clear();
    snapshot.roots.clear();
    
    // Get current selection
    let selected = selection_manager.0.read().get_selected();
    snapshot.selected = selected.clone();
    snapshot.selected_entities.clear();
    
    // Build entity map
    for (entity, instance, basepart, children, parent, service_owner, attributes, tags, parameters, atmosphere, billboard_gui, text_label) in query.iter() {
        let child_entities: Vec<Entity> = children
            .map(|c| c.to_vec())
            .unwrap_or_default();
        
        // Extract orientation as Euler angles in degrees
        let orientation = basepart.map(|bp| {
            let (x, y, z) = bp.cframe.rotation.to_euler(bevy::math::EulerRot::XYZ);
            Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
        });
        
        let entity_snapshot = EntitySnapshot {
            entity,
            name: instance.name.clone(),
            class_name: instance.class_name,
            parent: parent.map(|p| p.parent()),
            children: child_entities,
            service_owner: service_owner.map(|so| so.0),
            // Transform/Geometry
            position: basepart.map(|bp| bp.cframe.translation),
            orientation,
            size: basepart.map(|bp| bp.size),
            density: basepart.map(|bp| bp.density),
            mass: basepart.map(|bp| bp.mass),
            assembly_mass: None, // Computed after all entities are added
            // Appearance
            color: basepart.map(|bp| bp.color),
            material: basepart.map(|bp| bp.material),
            transparency: basepart.map(|bp| bp.transparency),
            reflectance: basepart.map(|bp| bp.reflectance),
            // Behavior
            anchored: basepart.map(|bp| bp.anchored),
            can_collide: basepart.map(|bp| bp.can_collide),
            can_touch: basepart.map(|bp| bp.can_touch),
            locked: basepart.map(|bp| bp.locked),
            // Instance properties
            ai: Some(instance.ai),
            // Attributes & Tags
            attributes: attributes.map(|a| a.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default(),
            tags: tags.map(|t| t.iter().cloned().collect()).unwrap_or_default(),
            has_parameters: parameters.is_some(),
            data_source_type: parameters.and_then(|p| p.sources.values().next().map(|s| s.source_type.clone())),
            // Atmosphere properties
            atmosphere_density: atmosphere.map(|a| a.density),
            atmosphere_offset: atmosphere.map(|a| a.offset),
            atmosphere_glare: atmosphere.map(|a| a.glare),
            atmosphere_haze: atmosphere.map(|a| a.haze),
            atmosphere_color: atmosphere.map(|a| a.color),
            atmosphere_decay: atmosphere.map(|a| a.decay),
            // BillboardGui properties
            billboard_active: billboard_gui.map(|g| g.active),
            billboard_enabled: billboard_gui.map(|g| g.enabled),
            billboard_always_on_top: billboard_gui.map(|g| g.always_on_top),
            billboard_size: billboard_gui.map(|g| bevy::math::Vec2::new(g.size[0], g.size[1])),
            billboard_units_offset: billboard_gui.map(|g| bevy::math::Vec3::new(g.units_offset[0], g.units_offset[1], g.units_offset[2])),
            billboard_max_distance: billboard_gui.map(|g| g.max_distance),
            billboard_brightness: billboard_gui.map(|g| g.brightness),
            billboard_light_influence: billboard_gui.map(|g| g.light_influence),
            // TextLabel properties
            textlabel_text: text_label.map(|t| t.text.clone()),
            textlabel_font_size: text_label.map(|t| t.font_size),
            textlabel_visible: text_label.map(|t| t.visible),
            textlabel_text_color: text_label.map(|t| Color::srgb(t.text_color3[0], t.text_color3[1], t.text_color3[2])),
            textlabel_background_transparency: text_label.map(|t| t.background_transparency),
        };
        
        snapshot.entities.insert(entity, entity_snapshot);
        
        // Track roots
        if parent.is_none() {
            snapshot.roots.push(entity);
        }
        
        // Track selected entities - use consistent "indexvgeneration" format
        let entity_str = format!("{}v{}", entity.index(), entity.generation());
        if selected.contains(&entity_str) {
            snapshot.selected_entities.push(entity);
        }
    }
    
    // Sort roots alphabetically - need to collect names first to avoid borrow conflict
    let mut root_names: Vec<(Entity, String)> = snapshot.roots.iter()
        .map(|e| (*e, snapshot.entities.get(e).map(|ent| ent.name.to_lowercase()).unwrap_or_default()))
        .collect();
    root_names.sort_by(|a, b| a.1.cmp(&b.1));
    snapshot.roots = root_names.into_iter().map(|(e, _)| e).collect();
    
    // Compute assembly_mass for Model/Folder entities (sum of all descendant BasePart masses)
    let model_folder_entities: Vec<Entity> = snapshot.entities.iter()
        .filter(|(_, e)| matches!(e.class_name, ClassName::Model | ClassName::Folder))
        .map(|(entity, _)| *entity)
        .collect();
    
    for entity in model_folder_entities {
        let assembly_mass = calculate_assembly_mass(entity, &snapshot);
        if let Some(entity_snapshot) = snapshot.entities.get_mut(&entity) {
            entity_snapshot.assembly_mass = assembly_mass;
        }
    }
    
    snapshot.frame += 1;
}

/// System to apply UI actions to World
/// Runs AFTER UI systems
pub fn apply_ui_actions(
    action_queue: Option<ResMut<UIActionQueue>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_manager: Option<Res<BevySelectionManager>>,
    expanded: Option<ResMut<super::ExplorerExpanded>>,
    query: Query<(Entity, &Instance, Option<&BasePart>)>,
    play_mode_state: Option<Res<State<PlayModeState>>>,
    undo_stack: Option<ResMut<crate::undo::UndoStack>>,
    instance_query: Query<&Instance>,
    basepart_query: Query<&BasePart>,
    modal_state: Option<ResMut<PropertiesModalState>>,
    script_editor_state: Option<ResMut<super::script_editor::ScriptEditorState>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(mut action_queue) = action_queue else { return };
    let Some(mut expanded) = expanded else { return };
    let Some(play_mode_state) = play_mode_state else { return };
    let Some(mut undo_stack) = undo_stack else { return };
    let Some(mut modal_state) = modal_state else { return };
    let Some(mut script_editor_state) = script_editor_state else { return };
    let is_playing = *play_mode_state.get() != PlayModeState::Editing;
    for action in action_queue.drain() {
        match action {
            UIAction::Select(entities) => {
                let sm = selection_manager.0.write();
                sm.clear();
                for entity in entities {
                    // Use consistent entity ID format: "indexvgeneration" (e.g., "123v4")
                    let entity_id = format!("{}v{}", entity.index(), entity.generation());
                    info!("UIAction::Select - selecting entity ID: {}", entity_id);
                    sm.select(entity_id);
                }
                // Clear service selection when selecting entities
                expanded.deselect_service();
            }
            UIAction::AddToSelection(entity) => {
                let entity_id = format!("{}v{}", entity.index(), entity.generation());
                selection_manager.0.write().select(entity_id);
                expanded.deselect_service();
            }
            UIAction::RemoveFromSelection(entity) => {
                let entity_id = format!("{}v{}", entity.index(), entity.generation());
                selection_manager.0.write().toggle_selection(entity_id);
            }
            UIAction::ClearSelection => {
                selection_manager.0.write().clear();
                expanded.deselect_service();
            }
            UIAction::ClearServiceSelection => {
                expanded.deselect_service();
            }
            UIAction::SelectService(service) => {
                // Clear entity selection and select the service
                selection_manager.0.write().clear();
                expanded.select_service(service);
            }
            UIAction::Delete(entities) => {
                for entity in entities {
                    commands.entity(entity).despawn();
                }
                selection_manager.0.write().clear();
            }
            UIAction::SpawnPart { part_type, position } => {
                // Spawn the part directly using the spawn module
                use crate::classes::*;
                use crate::spawn::spawn_part;
                
                info!("🔧 apply_ui_actions: Processing SpawnPart with type {:?}", part_type);
                
                // Determine part name and size based on type
                let (part_name, size) = match part_type {
                    PartType::Block => ("Block", Vec3::new(4.0, 1.2, 2.0)),
                    PartType::Ball => ("Ball", Vec3::new(4.0, 4.0, 4.0)),
                    PartType::Cylinder => ("Cylinder", Vec3::new(2.0, 4.0, 2.0)),
                    PartType::Wedge => ("Wedge", Vec3::new(4.0, 1.0, 2.0)),
                    PartType::CornerWedge => ("CornerWedge", Vec3::new(2.0, 2.0, 2.0)),
                    PartType::Cone => ("Cone", Vec3::new(2.0, 4.0, 2.0)),
                };
                
                // Calculate actual position (center + half height to sit on ground)
                let actual_position = position + Vec3::new(0.0, size.y / 2.0, 0.0);
                
                let instance = Instance {
                    name: part_name.to_string(),
                    class_name: ClassName::Part,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                
                let base_part = BasePart {
                    cframe: Transform::from_translation(actual_position),
                    size,
                    color: Color::srgb(0.5, 0.5, 0.5),
                    can_collide: true,
                    ..Default::default()
                };
                
                let part = Part { shape: part_type };
                
                // We need meshes and materials - queue this for the spawn system instead
                // by writing a SpawnPartEvent message
                info!("✨ Spawning {} at {:?}", part_name, actual_position);
                
                // Store spawn request for the spawn_events system to handle
                // (we can't access meshes/materials here, so we use the event system)
                commands.queue(move |world: &mut World| {
                    world.write_message(super::SpawnPartEvent {
                        part_type,
                        position,
                    });
                });
            }
            UIAction::SpawnSpawnPoint { position } => {
                use crate::classes::*;
                
                let actual_position = position + Vec3::new(0.0, 0.5, 0.0);
                
                let instance = Instance {
                    name: "SpawnLocation".to_string(),
                    class_name: ClassName::SpawnLocation,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                
                let base_part = BasePart {
                    cframe: Transform::from_translation(actual_position),
                    size: Vec3::new(6.0, 1.0, 6.0),
                    color: Color::srgb(0.2, 0.8, 0.2),
                    can_collide: true,
                    anchored: true,
                    ..Default::default()
                };
                
                let part = Part { shape: PartType::Block };
                let position_copy = position;
                let part_type = PartType::Block;
                
                info!("✨ Spawning SpawnLocation at {:?}", actual_position);
                
                commands.queue(move |world: &mut World| {
                    world.write_message(super::SpawnPartEvent {
                        part_type,
                        position: position_copy,
                    });
                });
            }
            UIAction::SpawnPointLight { position } => {
                use crate::classes::*;
                use crate::spawn::spawn_point_light;
                
                let actual_position = position + Vec3::new(0.0, 3.0, 0.0);
                
                let instance = Instance {
                    name: "PointLight".to_string(),
                    class_name: ClassName::PointLight,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                
                let light = EustressPointLight::default();
                let transform = Transform::from_translation(actual_position);
                
                info!("✨ Spawning PointLight at {:?}", actual_position);
                
                let entity = spawn_point_light(&mut commands, instance, light, transform);
                
                // Mark as spawned during play mode if applicable
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
            }
            UIAction::SpawnSpotLight { position } => {
                use crate::classes::*;
                use crate::spawn::spawn_spot_light;
                
                let actual_position = position + Vec3::new(0.0, 3.0, 0.0);
                
                let instance = Instance {
                    name: "SpotLight".to_string(),
                    class_name: ClassName::SpotLight,
                    archivable: true,
                    id: (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() % u32::MAX as u128) as u32,
                    ..Default::default()
                };
                
                let light = EustressSpotLight::default();
                let transform = Transform::from_translation(actual_position)
                    .looking_at(position, Vec3::Y);
                
                info!("✨ Spawning SpotLight at {:?}", actual_position);
                
                let entity = spawn_spot_light(&mut commands, instance, light, transform);
                
                // Mark as spawned during play mode if applicable
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
            }
            UIAction::SpawnIntoService { service, class_name } => {
                // Spawn a new instance of the given class into the service
                info!("UI requested spawn {:?} into {:?}", class_name, service);
                
                // Generate unique ID
                let id = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() % u32::MAX as u128) as u32;
                
                // Create base instance
                let instance = Instance {
                    name: class_name.as_str().to_string(),
                    class_name,
                    archivable: true,
                    id,
                    ..Default::default()
                };
                
                // ServiceOwner tracks which service this entity belongs to
                let service_owner = super::ServiceOwner(service);
                
                // Spawn with appropriate components based on class
                match class_name {
                    ClassName::SoulScript => {
                        commands.spawn((
                            instance,
                            service_owner,
                            crate::soul::SoulScriptData::default(),
                            Name::new("SoulScript"),
                        ));
                        info!("✨ Spawned SoulScript into {:?}", service);
                    }
                    ClassName::Folder => {
                        commands.spawn((
                            instance,
                            service_owner,
                            Name::new("Folder"),
                        ));
                        info!("✨ Spawned Folder into {:?}", service);
                    }
                    ClassName::Model => {
                        commands.spawn((
                            instance,
                            service_owner,
                            Name::new("Model"),
                        ));
                        info!("✨ Spawned Model into {:?}", service);
                    }
                    ClassName::Part => {
                        // Spawn a basic part with mesh, material, and physics
                        let base_part = BasePart {
                            cframe: bevy::prelude::Transform::from_translation(bevy::prelude::Vec3::new(0.0, 1.0, 0.0)),
                            size: bevy::prelude::Vec3::new(4.0, 1.0, 2.0),
                            color: bevy::prelude::Color::srgb(0.639, 0.635, 0.647), // Default gray
                            can_collide: true,
                            anchored: true,
                            ..Default::default()
                        };
                        let part = crate::classes::Part { shape: crate::classes::PartType::Block };
                        
                        // Use spawn_part to create mesh and material
                        let entity = crate::spawn::spawn_part(
                            &mut commands,
                            &mut meshes,
                            &mut materials,
                            instance,
                            base_part,
                            part,
                        );
                        // Add ServiceOwner after spawning
                        commands.entity(entity).insert(service_owner);
                        // Mark as spawned during play mode (will be despawned on stop)
                        if is_playing {
                            commands.entity(entity).insert(SpawnedDuringPlayMode);
                        }
                        info!("✨ Spawned Part with 3D mesh into {:?}", service);
                    }
                    ClassName::Sound => {
                        commands.spawn((
                            instance,
                            service_owner,
                            Name::new("Sound"),
                        ));
                        info!("✨ Spawned Sound into {:?}", service);
                    }
                    ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight => {
                        commands.spawn((
                            instance,
                            service_owner,
                            Name::new(class_name.as_str()),
                        ));
                        info!("✨ Spawned {:?} into {:?}", class_name, service);
                    }
                    _ => {
                        // Generic spawn for other types
                        commands.spawn((
                            instance,
                            service_owner,
                            Name::new(class_name.as_str()),
                        ));
                        info!("✨ Spawned {:?} into {:?}", class_name, service);
                    }
                }
            }
            UIAction::Rename { entity, new_name } => {
                commands.entity(entity).insert(Name::new(new_name));
            }
            UIAction::SetParent { entity, parent } => {
                if let Some(parent_entity) = parent {
                    commands.entity(entity).insert(ChildOf(parent_entity));
                } else {
                    commands.entity(entity).remove::<ChildOf>();
                }
            }
            UIAction::Reparent { child, new_parent } => {
                // Remove old parent and set new parent
                commands.entity(child)
                    .remove::<ChildOf>()
                    .insert(ChildOf(new_parent));
                info!("🔗 Reparented {:?} to {:?}", child, new_parent);
            }
            UIAction::Duplicate(entity) => {
                // Queue a deferred command to properly clone the entity with all components
                // Clone at same position (no offset)
                commands.queue(move |world: &mut World| {
                    clone_entity_recursive(world, entity, None, Vec3::ZERO);
                });
            }
            UIAction::ToggleExpanded(entity) => {
                expanded.toggle(entity);
            }
            UIAction::ToggleServiceExpanded(service) => {
                expanded.toggle_service(service);
            }
            UIAction::OpenScript(entity) => {
                // Script editor is now handled by Slint UI
                info!("OpenScript action for {:?} - handled by Slint", entity);
            }
            UIAction::SetProperty { entity, property, value } => {
                #[allow(unused_imports)]
                use crate::classes::PropertyValue;
                
                info!("SetProperty action: {} = {:?} for entity {:?}", property, value, entity);
                
                // Apply property change based on property name
                match property.as_str() {
                    "Name" => {
                        if let PropertyValue::String(name) = &value {
                            // Update both Name component and Instance.name
                            commands.entity(entity).insert(Name::new(name.clone()));
                            commands.entity(entity).insert(PendingPropertyChange {
                                property: property.clone(),
                                value: value.clone(),
                            });
                        }
                    }
                    "AI" => {
                        if let PropertyValue::Bool(ai_enabled) = &value {
                            // Update Instance.ai field
                            commands.entity(entity).insert(PendingPropertyChange {
                                property: property.clone(),
                                value: value.clone(),
                            });
                        }
                    }
                    "Position" | "Orientation" | "Size" | "Color" | "Material" | 
                    "Transparency" | "Reflectance" | "Anchored" | "CanCollide" | 
                    "CanTouch" | "Locked" => {
                        // These need to be applied to BasePart component
                        // We'll use a deferred approach with a marker component
                        commands.entity(entity).insert(PendingPropertyChange {
                            property: property.clone(),
                            value: value.clone(),
                        });
                    }
                    // Atmosphere properties
                    "AtmosphereDensity" | "AtmosphereOffset" | "AtmosphereGlare" | 
                    "AtmosphereHaze" | "AtmosphereColor" | "AtmosphereDecay" => {
                        commands.entity(entity).insert(PendingPropertyChange {
                            property: property.clone(),
                            value: value.clone(),
                        });
                    }
                    _ => {
                        warn!("Unknown property: {}", property);
                    }
                }
            }
            UIAction::SetPropertyMulti { entities, property, value } => {
                #[allow(unused_imports)]
                use crate::classes::PropertyValue;
                use crate::undo::{Action, PropertyValueSnapshot, UndoStack};
                
                info!("SetPropertyMulti action: {} = {:?} for {} entities", property, value, entities.len());
                
                // Capture old values for undo before applying changes
                let mut old_values: Vec<(u32, PropertyValueSnapshot)> = Vec::new();
                
                for &entity in &entities {
                    if let Some(instance) = instance_query.get(entity).ok() {
                        let old_value = capture_property_value(entity, &property, &instance_query, &basepart_query);
                        if let Some(old_val) = old_value {
                            old_values.push((instance.id, old_val));
                        }
                    }
                }
                
                // Convert new value to snapshot for undo
                let new_value_snapshot = property_value_to_snapshot(&property, &value);
                
                // Push to undo stack if we have old values
                if !old_values.is_empty() {
                    if let Some(new_snap) = new_value_snapshot {
                        undo_stack.push(Action::ChangePropertyMulti {
                            entities: old_values,
                            property: property.clone(),
                            new_value: new_snap,
                        });
                    }
                }
                
                // Apply property change to ALL entities
                for entity in entities {
                    match property.as_str() {
                        "Name" => {
                            // Name is typically unique per entity, skip for multi-select
                            // Or apply with suffix like "Part_1", "Part_2", etc.
                            warn!("Name property not supported for multi-select");
                        }
                        "Position" | "Orientation" | "Size" | "Color" | "Material" | 
                        "Transparency" | "Reflectance" | "Anchored" | "CanCollide" | 
                        "CanTouch" | "Locked" => {
                            // Apply to each entity
                            commands.entity(entity).insert(PendingPropertyChange {
                                property: property.clone(),
                                value: value.clone(),
                            });
                        }
                        _ => {
                            warn!("Unknown property: {}", property);
                        }
                    }
                }
            }
            
            // === Attributes Actions ===
            UIAction::SetAttribute { entity, key, value } => {
                info!("SetAttribute: {} = {:?} on {:?}", key, value, entity);
                if let Ok(mut entity_cmds) = commands.get_entity(entity) {
                    entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
                        if let Some(mut attrs) = entity_mut.get_mut::<eustress_common::attributes::Attributes>() {
                            attrs.set(&key, value.clone());
                        } else {
                            // Add Attributes component if it doesn't exist
                            let mut new_attrs = eustress_common::attributes::Attributes::new();
                            new_attrs.set(&key, value.clone());
                            entity_mut.insert(new_attrs);
                        }
                    });
                }
            }
            UIAction::RemoveAttribute { entity, key } => {
                info!("RemoveAttribute: {} on {:?}", key, entity);
                if let Ok(mut entity_cmds) = commands.get_entity(entity) {
                    entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
                        if let Some(mut attrs) = entity_mut.get_mut::<eustress_common::attributes::Attributes>() {
                            attrs.remove(&key);
                        }
                    });
                }
            }
            UIAction::OpenAddAttributeDialog(entity) => {
                info!("OpenAddAttributeDialog for {:?}", entity);
                modal_state.open_add_attribute(entity);
            }
            
            // === Tags Actions ===
            UIAction::AddTag { entity, tag } => {
                info!("AddTag: {} on {:?}", tag, entity);
                if let Ok(mut entity_cmds) = commands.get_entity(entity) {
                    entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
                        if let Some(mut tags) = entity_mut.get_mut::<eustress_common::attributes::Tags>() {
                            tags.add(&tag);
                        } else {
                            // Add Tags component if it doesn't exist
                            let mut new_tags = eustress_common::attributes::Tags::new();
                            new_tags.add(&tag);
                            entity_mut.insert(new_tags);
                        }
                    });
                }
            }
            UIAction::RemoveTag { entity, tag } => {
                info!("RemoveTag: {} on {:?}", tag, entity);
                if let Ok(mut entity_cmds) = commands.get_entity(entity) {
                    entity_cmds.queue(move |mut entity_mut: EntityWorldMut| {
                        if let Some(mut tags) = entity_mut.get_mut::<eustress_common::attributes::Tags>() {
                            tags.remove(&tag);
                        }
                    });
                }
            }
            UIAction::OpenAddTagDialog(entity) => {
                info!("OpenAddTagDialog for {:?}", entity);
                modal_state.open_add_tag(entity);
            }
            
            // === Parameters Actions ===
            UIAction::AddParameters { entity, source_type } => {
                info!("AddParameters: {:?} on {:?}", source_type, entity);
                let mut params = eustress_common::parameters::Parameters::default();
                // Add a default source config with the specified type
                params.sources.insert("default".to_string(), eustress_common::parameters::DataSourceConfig {
                    source_type,
                    auth: eustress_common::parameters::AuthType::None,
                    anonymization: eustress_common::parameters::AnonymizationMode::None,
                    update_mode: eustress_common::parameters::UpdateMode::Manual,
                    mappings: Vec::new(),
                });
                commands.entity(entity).insert(params);
            }
            UIAction::RemoveParameters(entity) => {
                info!("RemoveParameters on {:?}", entity);
                commands.entity(entity).remove::<eustress_common::parameters::Parameters>();
            }
            UIAction::OpenParametersEditor(entity) => {
                info!("OpenParametersEditor for {:?}", entity);
                // Get entity name for the tab
                if let Ok(instance) = instance_query.get(entity) {
                    script_editor_state.open_parameters_editor(entity, &instance.name);
                } else {
                    script_editor_state.open_parameters_editor(entity, "Entity");
                }
            }
            UIAction::OpenAddParametersDialog(entity) => {
                info!("OpenAddParametersDialog for {:?}", entity);
                modal_state.open_add_parameters(entity);
            }
            UIAction::Copy(entities) => {
                info!("Copy {:?} entities to clipboard", entities.len());
                // TODO: Implement clipboard storage
            }
            UIAction::Cut(entities) => {
                info!("Cut {:?} entities to clipboard", entities.len());
                // TODO: Implement clipboard storage with delete after paste
            }
            UIAction::PasteInto(parent) => {
                // Calculate mouse position in 3D space for paste target
                // This requires access to camera and window resources
                // We need to defer this to a system that has access to these resources

                // For now, queue a deferred paste operation that will calculate the position
                commands.queue(move |world: &mut World| {
                    // Get clipboard resource
                    let clipboard = world.get_resource::<crate::clipboard::EditorClipboard>();
                    if let Some(clipboard) = clipboard {
                        if clipboard.is_empty() {
                            return;
                        }

                        // Get camera and window for ray calculation
                        let mut cameras = world.query::<(&bevy::prelude::Camera, &bevy::prelude::GlobalTransform)>();
                        let windows = world.query::<&bevy::prelude::Window>()
                            .iter(world)
                            .find(|w| w.focused);

                        if let (Some((camera, camera_transform)), Some(window)) = (cameras.iter(world).next(), windows) {
                            if let Some(cursor_pos) = window.cursor_position() {
                                // Get ray from cursor
                                if let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) {
                                    // Find intersection with ground plane (Y=0) or use a default position
                                    let target_pos = if let Some(t) = crate::math_utils::ray_plane_intersection(ray.origin, *ray.direction, Vec3::ZERO, Vec3::Y) {
                                        let hit_pos = ray.origin + *ray.direction * t;
                                        // Add small offset above ground to prevent z-fighting
                                        hit_pos + Vec3::new(0.0, 0.01, 0.0)
                                    } else {
                                        // Fallback: position in front of camera
                                        camera_transform.translation() + camera_transform.forward() * 5.0
                                    };

                                    // Send paste event with calculated position
                                    world.write_message(crate::clipboard::PasteEvent {
                                        mode: crate::clipboard::PasteMode::Normal,
                                        target_position: Some(target_pos),
                                    });

                                    info!("📋 Pasting at mouse position: {:?}", target_pos);
                                } else {
                                    // Fallback: paste at default location
                                    world.write_message(crate::clipboard::PasteEvent {
                                        mode: crate::clipboard::PasteMode::Normal,
                                        target_position: None,
                                    });
                                    warn!("Failed to calculate ray from cursor, using default paste position");
                                }
                            } else {
                                // No cursor position available, use default
                                world.write_message(crate::clipboard::PasteEvent {
                                    mode: crate::clipboard::PasteMode::Normal,
                                    target_position: None,
                                });
                                warn!("No cursor position available for paste, using default position");
                            }
                        } else {
                            // No camera/window available, use default
                            world.write_message(crate::clipboard::PasteEvent {
                                mode: crate::clipboard::PasteMode::Normal,
                                target_position: None,
                            });
                            warn!("No camera/window available for paste position calculation, using default");
                        }
                    } else {
                        warn!("No clipboard available for paste operation");
                    }
                });
            }
            UIAction::BeginRename(entity) => {
                info!("Begin rename for {:?}", entity);
                // TODO: Focus the rename text field in Properties panel
            }
            UIAction::SetAtmospherePreset { entity, preset } => {
                info!("SetAtmospherePreset: {} for {:?}", preset, entity);
                commands.entity(entity).insert(PendingAtmospherePreset { preset });
            }
            UIAction::PluginAction(action_id) => {
                info!("PluginAction: {}", action_id);
                // Send plugin action event for the plugin system to handle
                commands.queue(move |world: &mut World| {
                    world.write_message(crate::studio_plugins::PluginActionEvent { action_id });
                });
            }
        }
    }
}

/// Marker component for pending atmosphere preset changes
#[derive(Component, Clone)]
pub struct PendingAtmospherePreset {
    pub preset: String,
}

/// Marker component for pending property changes
#[derive(Component, Clone)]
pub struct PendingPropertyChange {
    pub property: String,
    pub value: crate::classes::PropertyValue,
}

/// Marker component indicating mesh needs to be regenerated
#[derive(Component)]
pub struct NeedsMeshRegeneration;

/// System to apply pending property changes
pub fn apply_pending_property_changes(
    mut commands: Commands,
    query: Query<(Entity, &PendingPropertyChange)>,
    mut instance_query: Query<&mut Instance>,
    mut basepart_query: Query<&mut BasePart>,
    mut transform_query: Query<&mut Transform>,
) {
    use crate::classes::PropertyValue;
    
    for (entity, pending) in query.iter() {
        let mut applied = false;
        
        // Try to apply to Instance
        if let Ok(mut inst) = instance_query.get_mut(entity) {
            match (pending.property.as_str(), &pending.value) {
                ("Name", PropertyValue::String(name)) => {
                    inst.name = name.clone();
                    applied = true;
                }
                ("AI", PropertyValue::Bool(ai_enabled)) => {
                    inst.ai = *ai_enabled;
                    info!("Set AI training opt-in to {} for entity {:?}", ai_enabled, entity);
                    applied = true;
                }
                _ => {}
            }
        }
        
        // Try to apply to BasePart AND Transform (for visual updates)
        if let Ok(mut bp) = basepart_query.get_mut(entity) {
            match (pending.property.as_str(), &pending.value) {
                ("Position", PropertyValue::Vector3(pos)) => {
                    bp.cframe.translation = *pos;
                    // ALSO update Transform for visual rendering
                    if let Ok(mut transform) = transform_query.get_mut(entity) {
                        transform.translation = *pos;
                    }
                    applied = true;
                }
                ("Orientation", PropertyValue::Vector3(orient)) => {
                    // Convert degrees to radians and create rotation quaternion
                    let rotation = Quat::from_euler(
                        bevy::math::EulerRot::XYZ,
                        orient.x.to_radians(),
                        orient.y.to_radians(),
                        orient.z.to_radians(),
                    );
                    bp.cframe.rotation = rotation;
                    // ALSO update Transform for visual rendering
                    if let Ok(mut transform) = transform_query.get_mut(entity) {
                        transform.rotation = rotation;
                    }
                    applied = true;
                }
                ("Size", PropertyValue::Vector3(size)) => {
                    // Validate size - must be positive and reasonable
                    let valid_size = Vec3::new(
                        size.x.max(0.1).min(10000.0),
                        size.y.max(0.1).min(10000.0),
                        size.z.max(0.1).min(10000.0),
                    );
                    bp.size = valid_size;
                    // Size changes need mesh regeneration - mark for update
                    // The mesh will be regenerated by a separate system
                    commands.entity(entity).insert(NeedsMeshRegeneration);
                    applied = true;
                }
                ("Color", PropertyValue::Color(color)) => {
                    bp.color = *color;
                    // Note: Color changes need material update - handled elsewhere
                    applied = true;
                }
                ("Material", PropertyValue::Material(mat)) => {
                    bp.material = *mat;
                    // Material changes may need visual update
                    applied = true;
                }
                ("Transparency", PropertyValue::Float(t)) => {
                    bp.transparency = *t;
                    applied = true;
                }
                ("Reflectance", PropertyValue::Float(r)) => {
                    bp.reflectance = *r;
                    applied = true;
                }
                ("Anchored", PropertyValue::Bool(a)) => {
                    bp.anchored = *a;
                    info!("Set Anchored to {} for entity {:?}", a, entity);
                    applied = true;
                }
                ("CanCollide", PropertyValue::Bool(c)) => {
                    bp.can_collide = *c;
                    info!("Set CanCollide to {} for entity {:?}", c, entity);
                    applied = true;
                }
                ("CanTouch", PropertyValue::Bool(ct)) => {
                    bp.can_touch = *ct;
                    applied = true;
                }
                ("Locked", PropertyValue::Bool(l)) => {
                    bp.locked = *l;
                    applied = true;
                }
                _ => {}
            }
        }
        
        // Note: Atmosphere properties are handled by apply_pending_atmosphere_changes system
        // which has access to the Atmosphere component query
        
        // Always remove the pending change marker after processing
        commands.entity(entity).remove::<PendingPropertyChange>();
        
        if !applied {
            warn!("Failed to apply property '{}' to entity {:?}", pending.property, entity);
        }
    }
}

/// System to apply pending Atmosphere property changes
pub fn apply_pending_atmosphere_changes(
    mut commands: Commands,
    property_query: Query<(Entity, &PendingPropertyChange)>,
    preset_query: Query<(Entity, &PendingAtmospherePreset)>,
    mut atmosphere_query: Query<&mut eustress_common::classes::Atmosphere>,
) {
    use crate::classes::PropertyValue;
    
    // Apply property changes
    for (entity, pending) in property_query.iter() {
        if let Ok(mut atmosphere) = atmosphere_query.get_mut(entity) {
            let applied = match (pending.property.as_str(), &pending.value) {
                ("AtmosphereDensity", PropertyValue::Float(v)) => {
                    atmosphere.density = *v;
                    true
                }
                ("AtmosphereOffset", PropertyValue::Float(v)) => {
                    atmosphere.offset = *v;
                    true
                }
                ("AtmosphereGlare", PropertyValue::Float(v)) => {
                    atmosphere.glare = *v;
                    true
                }
                ("AtmosphereHaze", PropertyValue::Float(v)) => {
                    atmosphere.haze = *v;
                    true
                }
                ("AtmosphereColor", PropertyValue::Color(c)) => {
                    // Convert Bevy Color to [f32; 4]
                    let rgba = c.to_srgba();
                    atmosphere.color = [rgba.red, rgba.green, rgba.blue, rgba.alpha];
                    true
                }
                ("AtmosphereDecay", PropertyValue::Color(c)) => {
                    // Convert Bevy Color to [f32; 4]
                    let rgba = c.to_srgba();
                    atmosphere.decay = [rgba.red, rgba.green, rgba.blue, rgba.alpha];
                    true
                }
                _ => false,
            };
            
            if applied {
                info!("Applied Atmosphere property '{}' to entity {:?}", pending.property, entity);
                commands.entity(entity).remove::<PendingPropertyChange>();
            }
        }
    }
    
    // Apply presets
    for (entity, preset) in preset_query.iter() {
        if let Ok(mut atmosphere) = atmosphere_query.get_mut(entity) {
            match preset.preset.as_str() {
                "clear_day" => {
                    *atmosphere = eustress_common::classes::Atmosphere::clear_day();
                    info!("Applied Clear Day preset to Atmosphere {:?}", entity);
                }
                "sunset" => {
                    *atmosphere = eustress_common::classes::Atmosphere::sunset();
                    info!("Applied Sunset preset to Atmosphere {:?}", entity);
                }
                "foggy" => {
                    *atmosphere = eustress_common::classes::Atmosphere::foggy();
                    info!("Applied Foggy preset to Atmosphere {:?}", entity);
                }
                _ => {
                    warn!("Unknown atmosphere preset: {}", preset.preset);
                }
            }
            commands.entity(entity).remove::<PendingAtmospherePreset>();
        }
    }
}

/// System to regenerate meshes and colliders when size changes
pub fn regenerate_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    query: Query<(Entity, &BasePart, Option<&crate::classes::Part>, &Mesh3d), With<NeedsMeshRegeneration>>,
) {
    use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
    use avian3d::prelude::Collider;
    
    for (entity, basepart, part_opt, _mesh3d) in query.iter() {
        let size = basepart.size;
        // Use half-extents for collider (Avian uses half-size)
        let half = size * 0.5;
        
        // Regenerate mesh and collider based on part type
        let (new_mesh, new_collider) = if let Some(part) = part_opt {
            match part.shape {
                crate::classes::PartType::Block => (
                    meshes.add(Cuboid::from_size(size)),
                    Collider::cuboid(half.x, half.y, half.z),
                ),
                crate::classes::PartType::Ball => (
                    meshes.add(Sphere::new(size.x / 2.0)),
                    Collider::sphere(size.x / 2.0),
                ),
                crate::classes::PartType::Cylinder => (
                    meshes.add(Cylinder::new(size.x / 2.0, size.y)),
                    Collider::cylinder(size.y / 2.0, size.x / 2.0),
                ),
                _ => (
                    meshes.add(Cuboid::from_size(size)),
                    Collider::cuboid(half.x, half.y, half.z),
                ),
            }
        } else {
            (
                meshes.add(Cuboid::from_size(size)),
                Collider::cuboid(half.x, half.y, half.z),
            )
        };
        
        // Update the mesh handle and collider
        commands.entity(entity)
            .insert(Mesh3d(new_mesh))
            .insert(new_collider);
        
        // Remove the marker
        commands.entity(entity).remove::<NeedsMeshRegeneration>();
        
        info!("Regenerated mesh and collider for entity {:?} with size {:?}", entity, size);
    }
}

// ============================================================================
// Undo/Redo Helper Functions
// ============================================================================

/// Capture the current value of a property for undo
fn capture_property_value(
    entity: Entity,
    property: &str,
    instance_query: &Query<&Instance>,
    basepart_query: &Query<&BasePart>,
) -> Option<crate::undo::PropertyValueSnapshot> {
    use crate::undo::PropertyValueSnapshot;
    
    match property {
        "Name" => {
            instance_query.get(entity).ok().map(|inst| PropertyValueSnapshot::String(inst.name.clone()))
        }
        "Position" => {
            basepart_query.get(entity).ok().map(|bp| {
                let pos = bp.cframe.translation;
                PropertyValueSnapshot::Vector3([pos.x, pos.y, pos.z])
            })
        }
        "Orientation" => {
            basepart_query.get(entity).ok().map(|bp| {
                let (x, y, z) = bp.cframe.rotation.to_euler(EulerRot::XYZ);
                PropertyValueSnapshot::Vector3([x.to_degrees(), y.to_degrees(), z.to_degrees()])
            })
        }
        "Size" => {
            basepart_query.get(entity).ok().map(|bp| {
                PropertyValueSnapshot::Vector3([bp.size.x, bp.size.y, bp.size.z])
            })
        }
        "Color" => {
            basepart_query.get(entity).ok().map(|bp| {
                let rgba = bp.color.to_srgba();
                PropertyValueSnapshot::Color([rgba.red, rgba.green, rgba.blue, rgba.alpha])
            })
        }
        "Material" => {
            basepart_query.get(entity).ok().map(|bp| {
                PropertyValueSnapshot::Material(format!("{:?}", bp.material))
            })
        }
        "Transparency" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Float(bp.transparency))
        }
        "Reflectance" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Float(bp.reflectance))
        }
        "Anchored" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Bool(bp.anchored))
        }
        "CanCollide" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Bool(bp.can_collide))
        }
        "CanTouch" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Bool(bp.can_touch))
        }
        "Locked" => {
            basepart_query.get(entity).ok().map(|bp| PropertyValueSnapshot::Bool(bp.locked))
        }
        _ => None,
    }
}

/// Convert a PropertyValue to a PropertyValueSnapshot for undo
fn property_value_to_snapshot(
    property: &str,
    value: &crate::classes::PropertyValue,
) -> Option<crate::undo::PropertyValueSnapshot> {
    use crate::classes::PropertyValue;
    use crate::undo::PropertyValueSnapshot;
    
    match (property, value) {
        ("Name", PropertyValue::String(s)) => Some(PropertyValueSnapshot::String(s.clone())),
        ("Position" | "Orientation" | "Size", PropertyValue::Vector3(v)) => {
            Some(PropertyValueSnapshot::Vector3([v.x, v.y, v.z]))
        }
        ("Color", PropertyValue::Color(c)) => {
            let rgba = c.to_srgba();
            Some(PropertyValueSnapshot::Color([rgba.red, rgba.green, rgba.blue, rgba.alpha]))
        }
        ("Material", PropertyValue::Material(m)) => {
            Some(PropertyValueSnapshot::Material(format!("{:?}", m)))
        }
        ("Transparency" | "Reflectance", PropertyValue::Float(f)) => {
            Some(PropertyValueSnapshot::Float(*f))
        }
        ("Anchored" | "CanCollide" | "CanTouch" | "Locked", PropertyValue::Bool(b)) => {
            Some(PropertyValueSnapshot::Bool(*b))
        }
        _ => None,
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<UIWorldSnapshot>()
            .init_resource::<UIActionQueue>()
            .init_resource::<PropertiesModalState>()
            .init_resource::<super::script_editor::ScriptEditorState>()
            // Extract snapshot BEFORE UI (in PreUpdate or early Update)
            .add_systems(PreUpdate, extract_world_snapshot)
            // Apply actions AFTER UI (in PostUpdate or late Update)
            .add_systems(PostUpdate, apply_ui_actions)
            // Apply pending property changes
            .add_systems(PostUpdate, apply_pending_property_changes.after(apply_ui_actions))
            // Apply pending Atmosphere property changes
            .add_systems(PostUpdate, apply_pending_atmosphere_changes.after(apply_ui_actions))
            // Regenerate meshes after property changes
            .add_systems(PostUpdate, regenerate_meshes.after(apply_pending_property_changes));
    }
}

// ============================================================================
// Entity Cloning Utilities
// ============================================================================

/// Recursively clone an entity and all its children
/// Returns the new root entity
fn clone_entity_recursive(
    world: &mut World,
    source: Entity,
    new_parent: Option<Entity>,
    offset: Vec3,
) -> Option<Entity> {
    use crate::classes::*;
    
    // Get source entity components - must read all before mutating world
    let instance = world.get::<Instance>(source)?.clone();
    let basepart = world.get::<BasePart>(source).cloned();
    let part = world.get::<Part>(source).cloned();
    let model = world.get::<Model>(source).cloned();
    let name_comp = world.get::<Name>(source).cloned();
    
    // Get children - entities that have ChildOf pointing to source
    let mut children: Vec<Entity> = Vec::new();
    for (entity, child_of) in world.query::<(Entity, &ChildOf)>().iter(world) {
        if child_of.0 == source {
            children.push(entity);
        }
    }
    
    // Create new instance
    let mut new_instance = instance.clone();
    new_instance.name = format!("{} (copy)", instance.name);
    new_instance.id = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() % u32::MAX as u128) as u32;
    
    // Spawn the new entity
    let new_entity = world.spawn_empty().id();
    
    // Insert Instance
    world.entity_mut(new_entity).insert(new_instance);
    
    // Insert Name
    if let Some(n) = name_comp {
        let new_name = format!("{} (copy)", n.as_str());
        world.entity_mut(new_entity).insert(Name::new(new_name));
    }
    
    // Insert Model if present
    if let Some(m) = model {
        world.entity_mut(new_entity).insert(m);
    }
    
    // Insert BasePart with offset if present
    if let Some(mut bp) = basepart {
        bp.cframe.translation += offset;
        world.entity_mut(new_entity).insert(bp);
        
        // Insert Part if present (for mesh generation)
        if let Some(p) = part {
            world.entity_mut(new_entity).insert(p);
            // Mark for mesh regeneration
            world.entity_mut(new_entity).insert(NeedsMeshRegeneration);
        }
    }
    
    // Set parent if specified
    if let Some(parent) = new_parent {
        world.entity_mut(new_entity).insert(ChildOf(parent));
    }
    
    // Recursively clone children
    let child_count = children.len();
    for child in children {
        clone_entity_recursive(world, child, Some(new_entity), offset);
    }
    
    info!("📋 Cloned entity {:?} -> {:?} with {} children", source, new_entity, child_count);
    
    Some(new_entity)
}
