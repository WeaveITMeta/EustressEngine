// Spawn Events - Handle spawning new entities in the scene
use bevy::prelude::*;
use crate::classes::{Instance, ClassName, BasePart, Part, PartType};
use crate::ui::BevySelectionManager;
use crate::camera_controller::EustressCamera;
use crate::play_mode::{PlayModeState, SpawnedDuringPlayMode};
use eustress_common::terrain::{TerrainConfig, TerrainData, TerrainMode, TerrainBrush, BrushMode, spawn_terrain, TerrainRoot};

/// Event to spawn a new part in the scene
#[derive(Message)]
pub struct SpawnPartEvent {
    pub part_type: PartType,
    pub position: Vec3,
}

impl Default for SpawnPartEvent {
    fn default() -> Self {
        Self {
            part_type: PartType::Block,
            position: Vec3::new(0.0, 0.0, 0.0), // Spawn on ground (centered on baseplate)
        }
    }
}

/// Event to paste a part with full properties (from clipboard)
#[derive(Message, Clone)]
pub struct PastePartEvent {
    pub name: String,
    pub part_type: PartType,
    pub position: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
    pub color: Color,
    pub material: crate::classes::Material,
    pub transparency: f32,
    pub reflectance: f32,
    pub anchored: bool,
    pub can_collide: bool,
    pub locked: bool,
}

/// System to handle spawn part events
pub fn handle_spawn_part_events(
    mut spawn_events: MessageReader<SpawnPartEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    selection_manager: Res<BevySelectionManager>,
    mut camera_query: Query<&mut EustressCamera>,
    play_mode_state: Res<State<PlayModeState>>,
) {
    let is_playing = *play_mode_state.get() != PlayModeState::Editing;
    for event in spawn_events.read() {
        // Determine part name based on type
        let part_name = match event.part_type {
            PartType::Block => "Block",
            PartType::Ball => "Ball",
            PartType::Cylinder => "Cylinder",
            PartType::Wedge => "Wedge",
            PartType::CornerWedge => "CornerWedge",
            PartType::Cone => "Cone",
        };
        
        // Determine default size based on type
        let size = match event.part_type {
            PartType::Ball => Vec3::new(4.0, 4.0, 4.0),
            PartType::Block => Vec3::new(4.0, 1.2, 2.0),
            PartType::Cylinder => Vec3::new(2.0, 4.0, 2.0),
            PartType::Wedge => Vec3::new(4.0, 1.0, 2.0),
            PartType::CornerWedge => Vec3::new(2.0, 2.0, 2.0),
            PartType::Cone => Vec3::new(2.0, 4.0, 2.0),
        };
        
        // Create Instance
        let instance = Instance {
            name: part_name.to_string(),
            class_name: ClassName::Part,
            archivable: true,
            id: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() % u32::MAX as u128) as u32, // Generate ID from timestamp
        };
        
        // Calculate actual position (center + half height to sit on ground)
        let actual_position = event.position + Vec3::new(0.0, size.y / 2.0, 0.0);
        
        // Create BasePart with proper positioning
        let base_part = BasePart {
            cframe: Transform::from_translation(actual_position),
            size,
            pivot_offset: Transform::IDENTITY,
            color: Color::srgb(0.5, 0.5, 0.5), // Default gray
            material: crate::classes::Material::Plastic,
            transparency: 0.0,
            reflectance: 0.0,
            can_collide: true,
            can_touch: true,
            locked: false,
            anchored: false,
            assembly_linear_velocity: Vec3::ZERO,
            assembly_angular_velocity: Vec3::ZERO,
            custom_physical_properties: None,
            collision_group: "Default".to_string(),
            density: 700.0,
            mass: 0.0,
        };
        
        // Create Part
        let part = Part {
            shape: event.part_type,
        };
        
        // Use the proper spawn_part function to create mesh and all components
        // This returns the Entity that was spawned
        let spawned_entity = crate::spawn::spawn_part(
            &mut commands,
            &mut meshes,
            &mut materials,
            instance,
            base_part,
            part,
        );
        
        // Mark as spawned during play mode (will be despawned on stop)
        if is_playing {
            commands.entity(spawned_entity).insert(SpawnedDuringPlayMode);
        }
        
        // Select the newly spawned entity
        {
            let mut selection = selection_manager.0.write();
            selection.clear();
            // Format entity as string for selection manager (e.g., "123v4")
            let entity_str = format!("{}v{}", spawned_entity.index(), spawned_entity.generation());
            selection.select(entity_str);
        }
        
        // Focus camera on the new entity
        if let Some(mut camera) = camera_query.iter_mut().next() {
            camera.pivot = actual_position;
            // Set a comfortable viewing distance based on part size
            let part_size = size.length();
            camera.distance = (part_size * 3.0).max(10.0);
            info!("📷 Camera focused on new {} at {:?}", part_name, actual_position);
        }
        
        notifications.success(format!("Added {} (selected)", part_name));
        info!("✨ Spawned {} at {:?}, entity: {:?}", part_name, actual_position, spawned_entity);
    }
}

/// System to handle paste part events (from clipboard with full properties)
pub fn handle_paste_part_events(
    mut paste_events: MessageReader<PastePartEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_manager: Res<BevySelectionManager>,
    play_mode_state: Res<State<PlayModeState>>,
) {
    let is_playing = *play_mode_state.get() != PlayModeState::Editing;
    
    // Collect all pasted entity IDs for selection
    let mut pasted_entities: Vec<Entity> = Vec::new();
    
    for event in paste_events.read() {
        // Create Instance with the original name
        let instance = Instance {
            name: event.name.clone(),
            class_name: ClassName::Part,
            archivable: true,
            id: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() % u32::MAX as u128) as u32,
        };
        
        // Create BasePart with all the copied properties
        let base_part = BasePart {
            cframe: Transform {
                translation: event.position,
                rotation: event.rotation,
                scale: Vec3::ONE,
            },
            size: event.size,
            pivot_offset: Transform::IDENTITY,
            color: event.color,
            material: event.material,
            transparency: event.transparency,
            reflectance: event.reflectance,
            can_collide: event.can_collide,
            can_touch: true,
            locked: event.locked,
            anchored: event.anchored,
            assembly_linear_velocity: Vec3::ZERO,
            assembly_angular_velocity: Vec3::ZERO,
            custom_physical_properties: None,
            collision_group: "Default".to_string(),
            density: 700.0,
            mass: 0.0,
        };
        
        // Create Part with the original shape
        let part = Part {
            shape: event.part_type,
        };
        
        // Spawn the part with all properties preserved
        let spawned_entity = crate::spawn::spawn_part(
            &mut commands,
            &mut meshes,
            &mut materials,
            instance,
            base_part,
            part,
        );
        
        // Mark as spawned during play mode (will be despawned on stop)
        if is_playing {
            commands.entity(spawned_entity).insert(SpawnedDuringPlayMode);
        }
        
        pasted_entities.push(spawned_entity);
        info!("📋 Pasted {} at {:?}", event.name, event.position);
    }
    
    // Select all pasted entities
    if !pasted_entities.is_empty() {
        let mut selection = selection_manager.0.write();
        selection.clear();
        for entity in &pasted_entities {
            let entity_str = format!("{}v{}", entity.index(), entity.generation());
            selection.select(entity_str);
        }
    }
}

// ============================================================================
// Terrain Events
// ============================================================================

/// Event to spawn/generate terrain
#[derive(Message)]
pub struct SpawnTerrainEvent {
    pub config: TerrainConfig,
}

impl Default for SpawnTerrainEvent {
    fn default() -> Self {
        Self {
            config: TerrainConfig::default(),
        }
    }
}

/// Event to toggle terrain edit mode
#[derive(Message)]
pub struct ToggleTerrainEditEvent;

/// Event to set terrain brush mode
#[derive(Message)]
pub struct SetTerrainBrushEvent {
    pub mode: BrushMode,
}

/// Event to import terrain heightmap
/// TODO: Implement heightmap import handler
#[derive(Message)]
#[allow(dead_code)]
pub struct ImportTerrainEvent {
    pub path: String,
}

/// Event to export terrain heightmap
/// TODO: Implement heightmap export handler
#[derive(Message)]
#[allow(dead_code)]
pub struct ExportTerrainEvent {
    pub path: String,
}

/// System to handle spawn terrain events
pub fn handle_spawn_terrain_events(
    mut spawn_events: MessageReader<SpawnTerrainEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for event in spawn_events.read() {
        // Remove existing terrain
        for entity in existing_terrain.iter() {
            commands.entity(entity).despawn();
        }
        
        // Spawn new terrain
        let data = TerrainData::procedural();
        let _terrain = spawn_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            event.config.clone(),
            data,
        );
        
        notifications.success("Generated terrain");
    }
}

/// System to handle terrain edit toggle
pub fn handle_toggle_terrain_edit(
    mut toggle_events: MessageReader<ToggleTerrainEditEvent>,
    mut mode: ResMut<TerrainMode>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for _event in toggle_events.read() {
        *mode = match *mode {
            TerrainMode::Render => {
                notifications.info("Terrain Edit Mode: ON");
                TerrainMode::Editor
            }
            TerrainMode::Editor => {
                notifications.info("Terrain Edit Mode: OFF");
                TerrainMode::Render
            }
        };
    }
}

/// System to handle terrain brush mode changes
pub fn handle_set_terrain_brush(
    mut brush_events: MessageReader<SetTerrainBrushEvent>,
    mut brush: ResMut<TerrainBrush>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for event in brush_events.read() {
        brush.mode = event.mode;
        notifications.info(format!("Terrain Brush: {:?}", event.mode));
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for spawn events
pub struct SpawnEventsPlugin;

impl Plugin for SpawnEventsPlugin {
    fn build(&self, app: &mut App) {
        app
            // Part events
            .add_message::<SpawnPartEvent>()
            .add_message::<PastePartEvent>()
            .add_systems(Update, (handle_spawn_part_events, handle_paste_part_events))
            // Terrain events
            .add_message::<SpawnTerrainEvent>()
            .add_message::<ToggleTerrainEditEvent>()
            .add_message::<SetTerrainBrushEvent>()
            .add_message::<ImportTerrainEvent>()
            .add_message::<ExportTerrainEvent>()
            .add_systems(Update, (
                handle_spawn_terrain_events,
                handle_toggle_terrain_edit,
                handle_set_terrain_brush,
            ));
    }
}
