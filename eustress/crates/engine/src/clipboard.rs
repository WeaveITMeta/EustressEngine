use bevy::prelude::*;
use bevy::prelude::Message;
use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::collections::HashMap;
use crate::classes::{
    Instance, ClassName, BasePart, Part, MeshPart, Model,
    EustressPointLight, EustressSpotLight, EustressDirectionalLight, SurfaceLight,
    Sound, Attachment, ParticleEmitter, Beam, Decal, SpecialMesh,
    BillboardGui, TextLabel,
};
use crate::serialization::scene::CurrentScenePath;
use crate::ui::BevySelectionManager;

// ============================================================================
// Clipboard Serialization Types
// ============================================================================

/// Serializable entity data for clipboard operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntityData2 {
    /// Unique ID for this entity
    pub id: u32,
    /// Entity name
    pub name: String,
    /// Class name (Part, Model, etc.)
    pub class: String,
    /// Parent entity ID (None for root entities)
    pub parent: Option<u32>,
    /// Transform data
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    /// All properties as key-value pairs
    pub properties: HashMap<String, serde_json::Value>,
    /// Parameters (if any)
    pub parameters: Option<serde_json::Value>,
}

// ============================================================================
// Clipboard Entity Types
// ============================================================================

/// Stored entity data for clipboard - supports all entity types
#[derive(Clone)]
pub struct ClipboardEntity {
    /// Core instance data (required for all entities)
    pub instance: Instance,
    /// Entity name
    pub name: String,
    /// Transform at time of copy
    pub transform: Transform,
    /// Entity-specific data
    pub data: ClipboardEntityData,
    /// Children (for Models/Folders)
    pub children: Vec<ClipboardEntity>,
    /// Original entity ID (for hierarchy reconstruction)
    pub original_entity: Option<Entity>,
}

/// Entity-specific data variants
#[derive(Clone)]
pub enum ClipboardEntityData {
    /// Part with BasePart and Part components
    Part {
        basepart: BasePart,
        part: Part,
    },
    /// MeshPart with custom mesh
    MeshPart {
        basepart: BasePart,
        meshpart: MeshPart,
    },
    /// Model container (children stored separately)
    Model {
        model: Model,
    },
    /// Folder container
    Folder,
    /// Point light
    PointLight {
        light: EustressPointLight,
    },
    /// Spot light
    SpotLight {
        light: EustressSpotLight,
    },
    /// Directional light
    DirectionalLight {
        light: EustressDirectionalLight,
    },
    /// Surface light
    SurfaceLight {
        light: SurfaceLight,
    },
    /// Sound
    Sound {
        sound: Sound,
    },
    /// Attachment
    Attachment {
        attachment: Attachment,
    },
    /// Particle emitter
    ParticleEmitter {
        emitter: ParticleEmitter,
    },
    /// Beam
    Beam {
        beam: Beam,
    },
    /// Decal
    Decal {
        decal: Decal,
    },
    /// Special mesh
    SpecialMesh {
        mesh: SpecialMesh,
    },
    /// Billboard GUI
    BillboardGui {
        gui: BillboardGui,
    },
    /// Text label
    TextLabel {
        label: TextLabel,
    },
    /// Generic/unknown entity (just transform)
    Generic,
}

impl ClipboardEntity {
    /// Create a new clipboard entity from components
    pub fn new(instance: Instance, name: String, transform: Transform, data: ClipboardEntityData) -> Self {
        Self {
            instance,
            name,
            transform,
            data,
            children: Vec::new(),
            original_entity: None,
        }
    }
    
    /// Add a child entity
    pub fn add_child(&mut self, child: ClipboardEntity) {
        self.children.push(child);
    }
    
    /// Check if this is a container (Model/Folder)
    pub fn is_container(&self) -> bool {
        matches!(self.data, ClipboardEntityData::Model { .. } | ClipboardEntityData::Folder)
    }
    
    /// Get the bounding box top (for stacking)
    pub fn get_top(&self) -> f32 {
        match &self.data {
            ClipboardEntityData::Part { basepart, .. } |
            ClipboardEntityData::MeshPart { basepart, .. } => {
                self.transform.translation.y + basepart.size.y * 0.5
            }
            _ => self.transform.translation.y + 0.5, // Default 1 unit height
        }
    }
    
    /// Get the bounding box bottom
    pub fn get_bottom(&self) -> f32 {
        match &self.data {
            ClipboardEntityData::Part { basepart, .. } |
            ClipboardEntityData::MeshPart { basepart, .. } => {
                self.transform.translation.y - basepart.size.y * 0.5
            }
            _ => self.transform.translation.y - 0.5,
        }
    }
    
    /// Get center position
    pub fn get_center(&self) -> Vec3 {
        self.transform.translation
    }
}

// ============================================================================
// Clipboard Resource
// ============================================================================

/// Clipboard resource for storing copied entities
#[derive(Resource)]
pub struct Clipboard {
    /// Copied entities (flat list, hierarchy preserved in children)
    pub entities: Vec<ClipboardEntity>,
    /// Track the top of the last paste for proper stacking
    pub last_paste_top: f32,
    /// Center of copied selection (for relative positioning)
    pub copy_center: Vec3,
    /// Paste offset counter (for multiple pastes)
    pub paste_count: u32,
    /// Original entity IDs that were copied (for checking if still selected)
    pub copied_entity_ids: Vec<String>,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            last_paste_top: f32::NEG_INFINITY,
            copy_center: Vec3::ZERO,
            paste_count: 0,
            copied_entity_ids: Vec::new(),
        }
    }
}

// ============================================================================
// Editor Clipboard - Cross-Scene Support
// ============================================================================

/// Paste mode for cross-scene operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasteMode {
    /// Paste with original IDs (may conflict)
    #[default]
    Normal,
    /// Regenerate all entity IDs to avoid conflicts
    NewIds,
    /// Paste cancelled
    Cancelled,
}

/// Cross-scene paste modal state
#[derive(Debug, Clone, Default)]
pub struct CrossScenePasteModal {
    /// Whether the modal is open
    pub open: bool,
    /// Source scene name for display
    pub source_scene_name: String,
    /// User's choice
    pub choice: Option<PasteMode>,
}

/// Enhanced clipboard with cross-scene support and serialization
#[derive(Resource)]
pub struct EditorClipboard {
    /// Serialized entity data
    pub entities: Vec<ClipboardEntityData2>,
    /// Source scene path (for cross-scene awareness)
    pub source_scene: Option<PathBuf>,
    /// Timestamp of copy operation
    pub copied_at: Option<std::time::Instant>,
    /// Include Parameters/Attributes/Tags in copy
    pub include_metadata: bool,
    /// Center of copied selection (for relative positioning)
    pub copy_center: Vec3,
    /// Paste offset counter (for multiple pastes)
    pub paste_count: u32,
    /// Original entity IDs that were copied
    pub copied_entity_ids: Vec<String>,
    /// Cross-scene paste modal state
    pub cross_scene_modal: CrossScenePasteModal,
    /// Cut mode (delete originals after paste)
    pub is_cut: bool,
    /// Entity ID mapping (old -> new) for hierarchy reconstruction
    pub id_mapping: HashMap<u32, u32>,
}

impl Default for EditorClipboard {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            source_scene: None,
            copied_at: None,
            include_metadata: true,
            copy_center: Vec3::ZERO,
            paste_count: 0,
            copied_entity_ids: Vec::new(),
            cross_scene_modal: CrossScenePasteModal::default(),
            is_cut: false,
            id_mapping: HashMap::new(),
        }
    }
}

impl EditorClipboard {
    /// Check if clipboard is empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    
    /// Get entity count
    pub fn count(&self) -> usize {
        self.entities.len()
    }
    
    /// Clear the clipboard
    pub fn clear(&mut self) {
        self.entities.clear();
        self.source_scene = None;
        self.copied_at = None;
        self.copy_center = Vec3::ZERO;
        self.paste_count = 0;
        self.copied_entity_ids.clear();
        self.is_cut = false;
        self.id_mapping.clear();
    }
    
    /// Check if this is a cross-scene paste
    pub fn is_cross_scene(&self, current_scene: Option<&PathBuf>) -> bool {
        match (&self.source_scene, current_scene) {
            (Some(source), Some(current)) => source != current,
            (Some(_), None) => true, // Pasting into unsaved scene
            (None, Some(_)) => true, // Copied from unsaved scene
            (None, None) => false,   // Both unsaved, same "scene"
        }
    }
    
    /// Get source scene name for display
    pub fn source_scene_name(&self) -> String {
        self.source_scene
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    }
    
    /// Get paste offset for current paste operation
    pub fn get_paste_offset(&self) -> Vec3 {
        // Offset each paste by 2 units in X and Z
        let offset = self.paste_count as f32 * 2.0;
        Vec3::new(offset, 0.0, offset)
    }
    
    /// Increment paste counter
    pub fn increment_paste_count(&mut self) {
        self.paste_count += 1;
    }
    
    /// Reset paste counter (called after new copy)
    pub fn reset_paste_count(&mut self) {
        self.paste_count = 0;
    }
    
    /// Generate a new unique entity ID
    pub fn generate_new_id(&mut self) -> u32 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u32;
        // Mix with a counter to ensure uniqueness
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        timestamp.wrapping_add(counter)
    }
    
    /// Remap entity IDs for paste with new IDs
    pub fn remap_ids(&mut self) {
        self.id_mapping.clear();
        
        // First pass: generate new IDs
        for entity_data in &self.entities {
            let old_id = entity_data.id;
            let new_id = Self::generate_new_id_static();
            self.id_mapping.insert(old_id, new_id);
        }
        
        // Second pass: apply new IDs
        for entity_data in &mut self.entities {
            if let Some(&new_id) = self.id_mapping.get(&entity_data.id) {
                entity_data.id = new_id;
            }
            
            // Update parent references
            if let Some(old_parent) = entity_data.parent {
                if let Some(&new_parent) = self.id_mapping.get(&old_parent) {
                    entity_data.parent = Some(new_parent);
                }
            }
        }
    }
    
    /// Generate a new unique entity ID (static version)
    fn generate_new_id_static() -> u32 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u32;
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let counter = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        timestamp.wrapping_add(counter)
    }
}

impl Clipboard {
    /// Copy entities to clipboard
    pub fn copy(&mut self, entities: Vec<ClipboardEntity>) {
        // Calculate center of all entities
        if !entities.is_empty() {
            let mut center = Vec3::ZERO;
            let mut count = 0;
            for entity in &entities {
                center += entity.transform.translation;
                count += 1;
            }
            self.copy_center = center / count as f32;
        } else {
            self.copy_center = Vec3::ZERO;
        }
        
        self.entities = entities;
        self.last_paste_top = f32::NEG_INFINITY;
        self.paste_count = 0;
        // Note: copied_entity_ids should be set separately by the caller
    }
    
    /// Copy entities to clipboard with their original entity IDs
    pub fn copy_with_ids(&mut self, entities: Vec<ClipboardEntity>, entity_ids: Vec<String>) {
        self.copy(entities);
        self.copied_entity_ids = entity_ids;
    }
    
    /// Check if any of the originally copied entities are still selected
    pub fn are_originals_selected(&self, current_selection: &[String]) -> bool {
        // If no originals tracked, assume not selected
        if self.copied_entity_ids.is_empty() {
            return false;
        }
        // Check if ANY of the original copied entities are in the current selection
        self.copied_entity_ids.iter().any(|id| current_selection.contains(id))
    }
    
    /// Get entities for pasting (cloned)
    pub fn paste(&self) -> Vec<ClipboardEntity> {
        self.entities.clone()
    }
    
    /// Get paste offset for current paste operation
    pub fn get_paste_offset(&self) -> Vec3 {
        // Offset each paste by 2 units upward (Y axis only)
        let y_offset = self.paste_count as f32 * 2.0;
        Vec3::new(0.0, y_offset, 0.0)
    }
    
    /// Increment paste counter
    pub fn increment_paste_count(&mut self) {
        self.paste_count += 1;
    }
    
    pub fn set_last_paste_top(&mut self, top: f32) {
        self.last_paste_top = top;
    }
    
    pub fn get_last_paste_top(&self) -> f32 {
        self.last_paste_top
    }
    
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
    
    pub fn clear(&mut self) {
        self.entities.clear();
        self.last_paste_top = f32::NEG_INFINITY;
        self.copy_center = Vec3::ZERO;
        self.paste_count = 0;
        self.copied_entity_ids.clear();
    }
    
    /// Get total entity count (including children)
    pub fn total_count(&self) -> usize {
        fn count_recursive(entities: &[ClipboardEntity]) -> usize {
            entities.iter().map(|e| 1 + count_recursive(&e.children)).sum()
        }
        count_recursive(&self.entities)
    }
}

// ============================================================================
// Clipboard Events
// ============================================================================

/// Event to trigger copy operation
#[derive(Event, Message, Debug, Clone)]
pub struct CopyEvent {
    /// Whether this is a cut operation
    pub is_cut: bool,
}

/// Event to trigger paste operation
#[derive(Event, Message, Debug, Clone)]
pub struct PasteEvent {
    /// Paste mode (normal or with new IDs)
    pub mode: PasteMode,
    /// Target position (None = use offset from copy center)
    pub target_position: Option<Vec3>,
}

/// Event to trigger duplicate operation
#[derive(Event, Message, Debug, Clone)]
pub struct DuplicateEvent;

/// Event fired when paste completes (for undo integration)
#[derive(Event, Message, Debug, Clone)]
pub struct PasteCompletedEvent {
    /// IDs of newly created entities
    pub created_entity_ids: Vec<String>,
}

// ============================================================================
// Clipboard Systems
// ============================================================================

/// System to handle copy/cut operations (simplified query)
pub fn handle_copy_event(
    mut events: MessageReader<CopyEvent>,
    mut clipboard: ResMut<EditorClipboard>,
    mut old_clipboard: ResMut<Clipboard>,
    selection: Option<Res<BevySelectionManager>>,
    query: Query<(Entity, &Instance, &Transform, Option<&BasePart>)>,
    current_scene: Option<Res<CurrentScenePath>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    let Some(selection) = selection else { return };
    for event in events.read() {
        let selected_ids = selection.0.read().get_selected();
        
        if selected_ids.is_empty() {
            notifications.warning("Nothing selected to copy");
            continue;
        }
        
        // Clear previous clipboard
        clipboard.clear();
        clipboard.is_cut = event.is_cut;
        clipboard.copied_at = Some(std::time::Instant::now());
        
        // Set source scene
        if let Some(ref scene_path) = current_scene {
            clipboard.source_scene = scene_path.0.clone();
        }
        
        // Calculate center of selection
        let mut center = Vec3::ZERO;
        let mut count = 0;
        
        // Collect selected entities
        let mut entity_data_list = Vec::new();
        let mut old_clipboard_entities = Vec::new();
        
        for (entity, instance, transform, basepart) in query.iter() {
            let entity_id = format!("{}v{}", entity.index(), entity.generation());
            
            if !selected_ids.contains(&entity_id) {
                continue;
            }
            
            center += transform.translation;
            count += 1;
            
            // Create ClipboardEntityData2 for serialization
            let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
            let mut properties = HashMap::new();
            
            // Add component-specific properties
            if let Some(bp) = basepart {
                properties.insert("size".to_string(), 
                    serde_json::json!([bp.size.x, bp.size.y, bp.size.z]));
                properties.insert("color".to_string(),
                    serde_json::json!([bp.color.to_srgba().red, bp.color.to_srgba().green, 
                                       bp.color.to_srgba().blue, bp.color.to_srgba().alpha]));
                properties.insert("transparency".to_string(), 
                    serde_json::json!(bp.transparency));
                properties.insert("anchored".to_string(), 
                    serde_json::json!(bp.anchored));
                properties.insert("can_collide".to_string(), 
                    serde_json::json!(bp.can_collide));
            }
            
            let entity_data = ClipboardEntityData2 {
                id: instance.id,
                name: instance.name.clone(),
                class: instance.class_name.as_str().to_string(),
                parent: None,
                position: [transform.translation.x, transform.translation.y, transform.translation.z],
                rotation: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                scale: [transform.scale.x, transform.scale.y, transform.scale.z],
                properties,
                parameters: None,
            };
            
            entity_data_list.push(entity_data);
            clipboard.copied_entity_ids.push(entity_id.clone());
            
            // Also populate old clipboard for backward compatibility
            let clipboard_data = if basepart.is_some() {
                ClipboardEntityData::Part {
                    basepart: basepart.cloned().unwrap_or_default(),
                    part: Part::default(),
                }
            } else {
                ClipboardEntityData::Generic
            };
            
            let mut clip_entity = ClipboardEntity::new(
                instance.clone(),
                instance.name.clone(),
                *transform,
                clipboard_data,
            );
            clip_entity.original_entity = Some(entity);
            old_clipboard_entities.push(clip_entity);
        }
        
        if count > 0 {
            clipboard.copy_center = center / count as f32;
            clipboard.entities = entity_data_list;
            
            // Update old clipboard too
            old_clipboard.copy_with_ids(old_clipboard_entities, clipboard.copied_entity_ids.clone());
            old_clipboard.copy_center = clipboard.copy_center;
            
            let action = if event.is_cut { "Cut" } else { "Copied" };
            notifications.info(format!("{} {} object(s)", action, count));
        }
    }
}

/// System to handle paste operations
pub fn handle_paste_event(
    mut events: MessageReader<PasteEvent>,
    mut clipboard: ResMut<EditorClipboard>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    current_scene: Option<Res<CurrentScenePath>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    mut paste_completed: MessageWriter<PasteCompletedEvent>,
) {
    for event in events.read() {
        if clipboard.is_empty() {
            notifications.warning("Clipboard is empty");
            continue;
        }
        
        // Check for cross-scene paste
        let current_path = current_scene.as_ref().and_then(|s| s.0.as_ref());
        if clipboard.is_cross_scene(current_path) && event.mode == PasteMode::Normal {
            // Open cross-scene modal instead of pasting directly
            clipboard.cross_scene_modal.open = true;
            clipboard.cross_scene_modal.source_scene_name = clipboard.source_scene_name();
            clipboard.cross_scene_modal.choice = None;
            continue;
        }
        
        if event.mode == PasteMode::Cancelled {
            continue;
        }
        
        // Remap IDs if requested
        if event.mode == PasteMode::NewIds {
            clipboard.remap_ids();
        }
        
        // Calculate paste offset
        let offset = event.target_position
            .map(|pos| pos - clipboard.copy_center)
            .unwrap_or_else(|| clipboard.get_paste_offset());
        
        let mut created_ids = Vec::new();
        
        // Spawn entities from clipboard
        for entity_data in &clipboard.entities {
            let spawned_id = spawn_entity_from_data(
                &mut commands,
                &asset_server,
                &mut meshes,
                &mut materials,
                entity_data,
                offset,
            );
            
            if let Some(id) = spawned_id {
                created_ids.push(id);
            }
        }
        
        clipboard.increment_paste_count();
        
        // Record for undo
        // TODO: Create proper undo action for paste
        
        // Fire completion event
        paste_completed.write(PasteCompletedEvent {
            created_entity_ids: created_ids.clone(),
        });
        
        notifications.info(format!("Pasted {} object(s)", clipboard.entities.len()));
        
        // If this was a cut, clear the clipboard
        if clipboard.is_cut {
            clipboard.clear();
        }
    }
}

/// System to handle duplicate operations (copy + paste in one step)
pub fn handle_duplicate_event(
    mut events: MessageReader<DuplicateEvent>,
    mut copy_events: MessageWriter<CopyEvent>,
    mut paste_events: MessageWriter<PasteEvent>,
    _clipboard: Res<EditorClipboard>,
) {
    for _event in events.read() {
        // First copy
        copy_events.write(CopyEvent { is_cut: false });
        
        // Then paste with new IDs (to avoid conflicts)
        paste_events.write(PasteEvent {
            mode: PasteMode::NewIds,
            target_position: None,
        });
    }
}

/// Helper function to spawn an entity from ClipboardEntityData2
fn spawn_entity_from_data(
    commands: &mut Commands,
    asset_server: &AssetServer,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    data: &ClipboardEntityData2,
    offset: Vec3,
) -> Option<String> {
    use crate::spawn::*;
    
    // Parse class name
    let class_name = match ClassName::from_str(&data.class) {
        Ok(cn) => cn,
        Err(_) => {
            warn!("Unknown class name: {}", data.class);
            return None;
        }
    };
    
    // Get transform
    let transform = Transform {
        translation: Vec3::new(data.position[0], data.position[1], data.position[2]) + offset,
        rotation: Quat::from_euler(
            EulerRot::XYZ,
            data.rotation[0].to_radians(),
            data.rotation[1].to_radians(),
            data.rotation[2].to_radians(),
        ),
        scale: Vec3::new(data.scale[0], data.scale[1], data.scale[2]),
    };
    
    let instance = Instance {
        name: data.name.clone(),
        class_name,
        archivable: true,
        id: data.id,
        ..Default::default()
    };
    
    let entity = match class_name {
        ClassName::Part => {
            // Extract properties
            let size = data.properties.get("size")
                .and_then(|v| v.as_array())
                .map(|a| Vec3::new(
                    a.get(0).and_then(|v| v.as_f64()).unwrap_or(4.0) as f32,
                    a.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                    a.get(2).and_then(|v| v.as_f64()).unwrap_or(2.0) as f32,
                ))
                .unwrap_or(Vec3::new(4.0, 1.0, 2.0));
            
            let color = data.properties.get("color")
                .and_then(|v| v.as_array())
                .map(|a| Color::srgba(
                    a.get(0).and_then(|v| v.as_f64()).unwrap_or(0.639) as f32,
                    a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.635) as f32,
                    a.get(2).and_then(|v| v.as_f64()).unwrap_or(0.647) as f32,
                    a.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                ))
                .unwrap_or(Color::srgba(0.639, 0.635, 0.647, 1.0));
            
            let mut basepart = BasePart::default();
            basepart.size = size;
            basepart.color = color;
            basepart.cframe = transform;
            basepart.anchored = data.properties.get("anchored")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            basepart.can_collide = data.properties.get("can_collide")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            
            let part = Part::default();
            
            Some(spawn_part_glb(commands, asset_server, materials, instance, basepart, part))
        }
        ClassName::Model => {
            Some(spawn_model(commands, instance, Model::default()))
        }
        ClassName::Folder => {
            Some(spawn_folder(commands, instance))
        }
        ClassName::PointLight => {
            Some(spawn_point_light(commands, instance, EustressPointLight::default(), transform))
        }
        ClassName::SpotLight => {
            Some(spawn_spot_light(commands, instance, EustressSpotLight::default(), transform))
        }
        _ => {
            // Generic spawn for unsupported types
            warn!("Paste not fully implemented for {:?}", class_name);
            None
        }
    };
    
    entity.map(|e| format!("{}v{}", e.index(), e.generation()))
}

/// System to render cross-scene paste modal
/// Note: Modal UI is now handled by Slint
pub fn render_cross_scene_modal(
    mut clipboard: ResMut<EditorClipboard>,
    mut paste_events: MessageWriter<PasteEvent>,
) {
    // Cross-scene paste modal is now handled by Slint UI
    // For now, auto-paste with new IDs when cross-scene is detected
    if clipboard.cross_scene_modal.open {
        clipboard.cross_scene_modal.open = false;
        paste_events.write(PasteEvent {
            mode: PasteMode::NewIds,
            target_position: None,
        });
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for clipboard system
pub struct ClipboardPlugin;

impl Plugin for ClipboardPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(Clipboard::default())
            .insert_resource(EditorClipboard::default())
            .add_message::<CopyEvent>()
            .add_message::<PasteEvent>()
            .add_message::<DuplicateEvent>()
            .add_message::<PasteCompletedEvent>()
            .add_systems(Update, (
                handle_copy_event,
                handle_paste_event,
                handle_duplicate_event,
                render_cross_scene_modal,
            ));
    }
}
