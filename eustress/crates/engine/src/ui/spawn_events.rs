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

/// System to handle spawn part events (file-system-first: loads .glb meshes)
pub fn handle_spawn_part_events(
    mut spawn_events: MessageReader<SpawnPartEvent>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
    selection_manager: Option<Res<BevySelectionManager>>,
    mut camera_query: Query<&mut EustressCamera>,
    play_mode_state: Option<Res<State<PlayModeState>>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(mut notifications) = notifications else { return };
    let Some(play_mode_state) = play_mode_state else { return };
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
                .unwrap_or_default()
                .as_nanos() % u32::MAX as u128) as u32, // Generate ID from timestamp
            ..Default::default()
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
            ..Default::default()
        };
        
        // Create Part
        let part = Part {
            shape: event.part_type,
        };
        
        // Spawn part from .glb file (file-system-first: mesh loaded via AssetServer)
        let spawned_entity = crate::spawn::spawn_part_glb(
            &mut commands,
            &asset_server,
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

/// System to handle paste part events (from clipboard with full properties, file-system-first)
pub fn handle_paste_part_events(
    mut paste_events: MessageReader<PastePartEvent>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection_manager: Option<Res<BevySelectionManager>>,
    play_mode_state: Option<Res<State<PlayModeState>>>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(play_mode_state) = play_mode_state else { return };
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
                .unwrap_or_default()
                .as_nanos() % u32::MAX as u128) as u32,
            ..Default::default()
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
            ..Default::default()
        };
        
        // Create Part with the original shape
        let part = Part {
            shape: event.part_type,
        };
        
        // Spawn part from .glb file (file-system-first: mesh loaded via AssetServer)
        let spawned_entity = crate::spawn::spawn_part_glb(
            &mut commands,
            &asset_server,
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

/// Event to import terrain heightmap from file
#[derive(Message)]
pub struct ImportTerrainEvent {
    pub path: String,
}

/// Event to export terrain heightmap to file
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
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(mut notifications) = notifications else { return };
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
    mode: Option<ResMut<TerrainMode>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(mut mode) = mode else { return };
    let Some(mut notifications) = notifications else { return };
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
/// Auto-enables Editor mode when a brush is selected so toolbar buttons work immediately.
pub fn handle_set_terrain_brush(
    mut brush_events: MessageReader<SetTerrainBrushEvent>,
    brush: Option<ResMut<TerrainBrush>>,
    mode: Option<ResMut<TerrainMode>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(mut brush) = brush else { return };
    let Some(mut mode) = mode else { return };
    let Some(mut notifications) = notifications else { return };
    for event in brush_events.read() {
        brush.mode = event.mode;
        // Auto-enable edit mode when selecting a brush tool
        if *mode != TerrainMode::Editor {
            *mode = TerrainMode::Editor;
            notifications.info(format!("Terrain Edit Mode: ON — Brush: {:?}", event.mode));
        } else {
            notifications.info(format!("Terrain Brush: {:?}", event.mode));
        }
    }
}

/// System to handle heightmap import events
///
/// Pipeline: file dialog path → elevation import → chunk → save R16 → spawn terrain
pub fn handle_import_terrain(
    mut import_events: MessageReader<ImportTerrainEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(mut notifications) = notifications else { return };
    for event in import_events.read() {
        let path = std::path::Path::new(&event.path);
        if !path.exists() {
            notifications.error(format!("Heightmap file not found: {}", event.path));
            continue;
        }

        // Step 1: Import elevation data from any supported format
        let import_config = eustress_common::pointcloud::ElevationImportConfig {
            chunk_size: 64.0,
            chunk_resolution: 64,
            height_scale: 50.0,
            vertical_exaggeration: 1.0,
            height_offset: 0.0,
            generate_lods: true,
            lod_levels: 4,
            fill_nodata: true,
            nodata_fill_value: 0.0,
            smooth_terrain: false,
            smooth_iterations: 0,
            coord_system: eustress_common::pointcloud::ElevationCoordSystem::Local,
        };

        match eustress_common::pointcloud::import_elevation_to_terrain(path, &import_config) {
            Ok(result) => {
                // Step 2: Remove existing terrain
                for entity in existing_terrain.iter() {
                    commands.entity(entity).despawn();
                }

                // Step 3: Save imported height cache as chunked R16 files
                let space_root = crate::space::default_space_root();
                let terrain_dir = space_root.join("Workspace").join("Terrain");
                let chunks_dir = terrain_dir.join("chunks");
                let _ = std::fs::create_dir_all(&chunks_dir);

                // Save per-chunk R16 files from the imported height cache
                let config = &result.config;
                let data = &result.data;
                let resolution = config.chunk_resolution;
                for cz in 0..(config.chunks_z * 2) {
                    for cx in 0..(config.chunks_x * 2) {
                        let chunk_path = chunks_dir.join(format!("x{}_z{}.r16", cx, cz));
                        let start_x = cx * resolution;
                        let start_z = cz * resolution;

                        // Extract chunk heightmap from the full cache
                        let mut chunk_heights = vec![0u8; (resolution * resolution * 2) as usize];
                        for z in 0..resolution {
                            for x in 0..resolution {
                                let src_x = start_x + x;
                                let src_z = start_z + z;
                                let src_idx = (src_z * data.cache_width + src_x) as usize;
                                let dst_idx = (z * resolution + x) as usize;
                                let height_val = if src_idx < data.height_cache.len() {
                                    data.height_cache[src_idx]
                                } else {
                                    0.0
                                };
                                let raw = (height_val.clamp(0.0, 1.0) * 65535.0) as u16;
                                let bytes = raw.to_le_bytes();
                                chunk_heights[dst_idx * 2] = bytes[0];
                                chunk_heights[dst_idx * 2 + 1] = bytes[1];
                            }
                        }
                        let _ = std::fs::write(&chunk_path, &chunk_heights);
                    }
                }

                // Step 4: Write _terrain.toml config
                let terrain_toml = format!(
                    r#"# Auto-generated from imported heightmap: {}
[terrain]
chunk_size = {:.1}
chunk_resolution = {}
height_scale = {:.1}
seed = 0

[streaming]
view_distance = {:.1}
cull_margin = 200.0
chunks_per_frame = 4

[lod]
levels = {}
distances = {:?}
"#,
                    path.display(),
                    config.chunk_size,
                    config.chunk_resolution,
                    import_config.height_scale,
                    config.view_distance,
                    config.lod_levels,
                    config.lod_distances,
                );
                let _ = std::fs::write(terrain_dir.join("_terrain.toml"), terrain_toml);

                // Step 5: Spawn terrain from imported data
                let _terrain = spawn_terrain(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    result.config,
                    result.data,
                );

                let warnings_str = if result.warnings.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", result.warnings.join(", "))
                };
                notifications.success(format!(
                    "Imported heightmap: {}x{} chunks, elevation {:.0}–{:.0}m{}",
                    result.chunk_count.0, result.chunk_count.1,
                    result.elevation_bounds.0, result.elevation_bounds.1,
                    warnings_str,
                ));
                info!(
                    "Imported terrain from {:?}: {}x{} chunks, saved R16 to {:?}",
                    path, result.chunk_count.0, result.chunk_count.1, chunks_dir
                );
            }
            Err(e) => {
                notifications.error(format!("Failed to import heightmap: {}", e));
                error!("Heightmap import failed for {:?}: {}", path, e);
            }
        }
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
                handle_import_terrain,
            ));
    }
}
