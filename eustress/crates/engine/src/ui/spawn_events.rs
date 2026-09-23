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
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut file_registry: Option<ResMut<crate::space::file_loader::SpaceFileRegistry>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    // Resources for the scalable binary-ECS create path (C1). Present in
    // every build (the disk loader uses them too); only read under the
    // `world-db` feature below.
    mut material_registry: ResMut<crate::space::material_loader::MaterialRegistry>,
    mut mesh_cache: ResMut<crate::space::instance_loader::PrimitiveMeshCache>,
    services: Query<(Entity, &crate::space::service_loader::ServiceComponent)>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
) {
    let Some(selection_manager) = selection_manager else { return };
    let Some(mut notifications) = notifications else { return };
    let Some(play_mode_state) = play_mode_state else { return };
    let is_playing = *play_mode_state.get() != PlayModeState::Editing;
    // Without the world-db feature the binary create path is compiled out;
    // touch the binary-only params so the no-feature build stays quiet.
    #[cfg(not(feature = "world-db"))]
    {
        let _ = (&material_registry, &mesh_cache, &services, &undo_stack);
    }
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

        // Primitive mesh path — used by both the binary create path and
        // the legacy TOML save below.
        let mesh_path = crate::spawn::part_type_to_glb_path(&event.part_type);

        // Default to the SCALABLE representation (SCALING_ARCHITECTURE.md
        // §0.5 C1): a bare Part with a primitive `parts/*.glb` mesh
        // persists as a binary-ECS rkyv core in Fjall, not a TOML folder.
        // Custom-mesh / file-natured classes, play mode, or a Space with
        // no active DB fall through to the legacy TOML create path below.
        // `representation_for_part` carries the V-Cell custom-mesh guard.
        let mut persisted_binary = false;
        let mut binary_entity: Option<Entity> = None;
        #[cfg(feature = "world-db")]
        {
            if !is_playing && crate::space::active_db::is_active() {
                if let Some(ref sr) = space_root {
                    let rep = crate::space::representation::representation_for_part(
                        "Part",
                        Some(mesh_path),
                        None,
                    );
                    if rep == crate::space::representation::Representation::BinaryEcs {
                        if let Some(ws) = services
                            .iter()
                            .find(|(_, s)| s.class_name == "Workspace")
                            .map(|(e, _)| e)
                        {
                            let def = build_binary_part_def(
                                part_name,
                                mesh_path,
                                actual_position,
                                size,
                                &base_part,
                            );
                            if let Some(spawned) =
                                crate::space::world_db_binary::spawn_binary_instance(
                                    &mut commands,
                                    &asset_server,
                                    &mut materials,
                                    &mut material_registry,
                                    &mut mesh_cache,
                                    &sr.0,
                                    ws,
                                    def,
                                )
                            {
                                binary_entity = Some(spawned.entity);
                                persisted_binary = true;
                                // Record undo: Ctrl+Z despawns + purges the
                                // core; redo re-spawns from the stored def
                                // (same uuid → same stored_id).
                                if let Ok(def_json) = serde_json::to_string(&spawned.def) {
                                    undo_stack.push(crate::undo::Action::CreateBinaryInstance {
                                        stored_id: spawned.stored_id,
                                        def_json,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Spawn from the .glb (legacy/file-system path) unless the scalable
        // binary core already created + spawned the entity above.
        let spawned_entity = match binary_entity {
            Some(e) => e,
            None => crate::spawn::spawn_part_glb(
                &mut commands,
                &asset_server,
                &mut materials,
                instance,
                base_part,
                part,
            ),
        };

        // Mark as spawned during play mode (will be despawned on stop)
        if is_playing {
            commands.entity(spawned_entity).insert(SpawnedDuringPlayMode);
        }
        
        // Select the newly spawned entity
        {
            let selection = selection_manager.0.write();
            selection.clear();
            // Format entity as string for selection manager (e.g., "123v4")
            let entity_str = format!("{}v{}", spawned_entity.index(), spawned_entity.generation());
            selection.select(entity_str);
        }
        
        // Focus camera on the new entity. Not in orthographic: there the
        // orbit distance is the zoom, so refocusing would jump the view to a
        // part-sized close-up every time something is inserted into a layout.
        if let Some(mut camera) = camera_query.iter_mut().next().filter(|c| !c.wants_ortho()) {
            camera.pivot = actual_position;
            // Set a comfortable viewing distance based on part size
            let part_size = size.length();
            camera.distance = (part_size * 3.0).max(10.0);
            info!("📷 Camera focused on new {} at {:?}", part_name, actual_position);
        }
        
        // Auto-save TOML for the new part. Routes through the canonical
        // pipeline: the Part class template provides the base body and
        // the override block patches in this spawn's transform + mesh.
        // Synchronous ECS spawn already happened above; registering the
        // returned TOML path in `SpaceFileRegistry` makes the file
        // watcher's `is_loaded(path)` check skip its own spawn pass.
        if !is_playing && !persisted_binary {
            if let Some(ref sr) = space_root {
                let workspace_dir = sr.0.join("Workspace");
                // Stage 7: stamp the Space-default authoring unit into
                // the new TOML so the file records its provenance. None
                // when the Space hasn't declared a default — the engine
                // then treats the entity as engine-native meters.
                let space_unit = eustress_common::project_manifest::read_space_default_unit(&sr.0);
                let authored = space_unit.as_deref()
                    .and_then(eustress_common::units::Unit::from_symbol)
                    .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
                let pos_auth = eustress_common::units::engine_to_authored_vec3_f32(
                    [actual_position.x, actual_position.y, actual_position.z], authored,
                );
                let scale_auth = eustress_common::units::engine_to_authored_vec3_f32(
                    [size.x, size.y, size.z], authored,
                );
                let overrides = eustress_common::instance_create::InstanceOverrides {
                    display_name: Some(part_name.to_string()),
                    position: Some(pos_auth),
                    scale: Some(scale_auth),
                    asset_mesh: Some(mesh_path.to_string()),
                    unit_symbol: space_unit,
                    ..Default::default()
                };
                match eustress_common::instance_create::create_instance(
                    &workspace_dir,
                    "Part",
                    Some(part_name),
                    overrides,
                ) {
                    Ok(created) => {
                        let final_path = created.toml_path.clone();

                        commands.entity(spawned_entity).insert(
                            crate::space::instance_loader::InstanceFile {
                                toml_path: final_path.clone(),
                                mesh_path: std::path::PathBuf::from(mesh_path),
                                name: part_name.to_string(),
                            }
                        );

                        // Tag with LoadedFromFile so Explorer classification routes
                        // this entity into the Workspace service bucket — otherwise
                        // the spawned part ends up as an unclassified root node.
                        commands.entity(spawned_entity).insert(
                            crate::space::file_loader::LoadedFromFile {
                                path: final_path.clone(),
                                file_type: crate::space::file_loader::FileType::Toml,
                                service: "Workspace".to_string(),
                            }
                        );

                        // Register in SpaceFileRegistry so the file watcher's
                        // `is_loaded(path)` check returns true when it sees the
                        // newly-written TOML — prevents duplicate spawning.
                        if let Some(ref mut registry) = file_registry {
                            let toml_size = std::fs::metadata(&final_path)
                                .map(|m| m.len())
                                .unwrap_or(0);
                            registry.register(
                                final_path.clone(),
                                spawned_entity,
                                crate::space::file_loader::FileMetadata {
                                    path: final_path.clone(),
                                    file_type: crate::space::file_loader::FileType::Toml,
                                    service: "Workspace".to_string(),
                                    name: part_name.to_string(),
                                    size: toml_size,
                                    modified: std::time::SystemTime::now(),
                                    children: Vec::new(),
                                },
                            );
                        }

                        if let Some(ref mut es) = explorer_state {
                            es.needs_immediate_sync = true;
                        }

                        info!("💾 Auto-saved {:?}", final_path.file_name().unwrap_or_default());
                    }
                    Err(e) => warn!("Failed to auto-save part TOML: {}", e),
                }
            }
        }

        notifications.success(format!("Added {} (selected)", part_name));
        info!("✨ Spawned {} at {:?}, entity: {:?}", part_name, actual_position, spawned_entity);
    }
}

/// Build the parse-model `InstanceDefinition` for a freshly-inserted bare
/// Part, in ENGINE-NATIVE units (binary cores store meters directly — no
/// authoring-unit conversion, unlike the TOML path). `spawn_binary_instance`
/// mints the uuid, so it is left `None` here. Field set mirrors
/// `world_db_binary::core_from_components` exactly so create == load.
#[cfg(feature = "world-db")]
fn build_binary_part_def(
    name: &str,
    mesh: &str,
    position: Vec3,
    size: Vec3,
    base: &BasePart,
) -> crate::space::instance_loader::InstanceDefinition {
    use crate::space::instance_loader::{
        AssetReference, InstanceDefinition, InstanceMetadata, InstanceProperties, TransformData,
    };
    let c = base.color.to_srgba();
    let now = chrono::Utc::now().to_rfc3339();
    InstanceDefinition {
        nuclear: None,
        plasma: None,
        asset: Some(AssetReference {
            mesh: mesh.to_string(),
            scene: "Scene0".to_string(),
        }),
        transform: TransformData {
            position: [position.x, position.y, position.z],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [size.x, size.y, size.z],
        },
        properties: InstanceProperties {
            color: [c.red, c.green, c.blue, c.alpha],
            transparency: base.transparency,
            anchored: base.anchored,
            can_collide: base.can_collide,
            cast_shadow: base.cast_shadow,
            reflectance: base.reflectance,
            material: base.material.as_str().to_string(),
            locked: base.locked,
            physics: None,
            respect_gltf_materials: false,
            destructible: base.destructible,
        },
        metadata: InstanceMetadata {
            class_name: "Part".to_string(),
            archivable: true,
            name: Some(name.to_string()),
            created: now.clone(),
            last_modified: now,
            created_by: None,
            modifications: Vec::new(),
            unit: None,
            uuid: None,
        },
        material: None,
        thermodynamic: None,
        electrochemical: None,
        ui: None,
        attributes: None,
        tags: None,
        parameters: None,
        extra: std::collections::HashMap::new(),
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
        let selection = selection_manager.0.write();
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

/// Event: run the full worldgen pipeline (`generate_world` → `export_to_space`
/// → disk hydrate → spawn) for the CURRENT Space. Fired from the Terrain
/// panel's Generate World button (drain arm in `slint_ui.rs`).
#[derive(Message)]
pub struct GenerateWorldEvent {
    pub spec: eustress_common::terrain::worldgen::pipeline::WorldSpec,
}

/// Event: write a dead-flat terrain plate ("baseplate") into the CURRENT
/// Space and spawn it. Fired from the Terrain ribbon's Generate > Flat
/// button (drain arm in `slint_ui.rs`).
///
/// Deliberately NOT a background task like [`GenerateWorldEvent`]: a flat
/// plate simulates nothing, so the write + hydrate finishes inside one
/// frame at the sizes the ribbon offers. It still takes the same
/// single-flight gate, because it despawns and replaces the live terrain.
#[derive(Message)]
pub struct GenerateFlatTerrainEvent {
    pub spec: eustress_common::terrain::worldgen::export::FlatSpec,
}

/// In-flight worldgen background task + coarse phase text for the panel.
///
/// LOOP-5: initialized in `SpawnEventsPlugin` (which the ACTIVE
/// `SlintUiPlugin` adds) — never in the legacy `StudioUiPlugin`.
#[derive(Resource, Default)]
pub struct WorldgenTask {
    /// The generation+export task running on `AsyncComputeTaskPool`.
    /// `Ok` payload = the `Workspace/Terrain` dir it wrote + the summary.
    pub task: Option<bevy::tasks::Task<Result<(std::path::PathBuf, eustress_common::terrain::worldgen::export::ExportSummary), String>>>,
    /// Human-readable phase shown in the panel while the task runs.
    pub status: String,
    /// True from task completion until `TerrainGenerationQueue` drains —
    /// the chunk-meshing tail of the busy window.
    pub meshing: bool,
}

impl WorldgenTask {
    /// The panel-level busy gate: generation OR the meshing tail.
    pub fn is_busy(&self) -> bool {
        self.task.is_some() || self.meshing
    }
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
        // Region and Fill have no stroke behaviour — `apply_brush_to_chunk`
        // matches them and does nothing. Arming one would leave a lit-up
        // button that never moves the ground, so say so and keep whichever
        // brush was already armed.
        if matches!(event.mode, BrushMode::Region | BrushMode::Fill) {
            notifications.info(format!(
                "{:?} brush is on the roadmap but not built yet, so the armed brush is unchanged",
                event.mode
            ));
            continue;
        }
        brush.mode = event.mode;
        // Auto-enable edit mode when selecting a brush tool
        if *mode != TerrainMode::Editor {
            *mode = TerrainMode::Editor;
            notifications.info(format!("Terrain Edit Mode: ON, Brush: {}", event.mode.label()));
        } else {
            notifications.info(format!("Terrain Brush: {}", event.mode.label()));
        }
    }
}

/// System to handle heightmap import events
///
/// Pipeline: file dialog path, elevation import, centred R16 chunks and
/// `_terrain.toml` in the loader's format, then `hydrate_terrain_from_disk`
/// and `spawn_terrain`, so the live terrain is exactly what Save and a
/// Space reopen see.
pub fn handle_import_terrain(
    mut import_events: MessageReader<ImportTerrainEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let Some(mut notifications) = notifications else { return };
    for event in import_events.read() {
        let path = std::path::Path::new(&event.path);
        if !path.exists() {
            notifications.error(format!("Heightmap file not found: {}", event.path));
            continue;
        }
        // The chunk files go into the OPEN Space. `default_space_root()`
        // re-reads the last-opened path from settings, which can name a
        // different Space after an in-session switch and would overwrite
        // that Space's terrain.
        let Some(ref sr) = space_root else {
            notifications.error("Import Heightmap: no open Space");
            continue;
        };
        let terrain_dir = sr.0.join("Workspace").join("Terrain");

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
                let config = &result.config;
                let data = &result.data;
                let res = config.chunk_resolution as usize;
                let chunks_dir = terrain_dir.join("chunks");

                // Step 2: Lay the import out in the loader's own format. The
                // importer's cache spans its full chunk count, `full * res`
                // per axis, while the loader and Save address a centred grid
                // of `2n + 1` chunks at signed coordinates `-n..=n`. The
                // smallest n with `2n + 1 >= full` on both axes holds it, and
                // `view_distance = n * chunk_size` makes `to_terrain_config`
                // re-derive exactly n on load.
                let (full_x, full_z) = result.chunk_count;
                let n = (full_x.max(full_z) / 2).max(1);

                // The import holds WORLD heights (height_scale 1.0, offset
                // 0.0), but R16 samples are normalized, so encode against a
                // band spanning the imported range and write that same band
                // into the toml below. It always holds Y = 0: an all-positive
                // heightmap keeps the Y it imported at, a negative one keeps
                // its depth, and the padding around a non-square import
                // sits at 0.
                let (lowest, highest) = data.height_cache.iter().fold(
                    (f32::INFINITY, f32::NEG_INFINITY),
                    |(lo, hi), &h| (lo.min(h), hi.max(h)),
                );
                let band_floor = if lowest.is_finite() { lowest.min(0.0) } else { 0.0 };
                let band_range = if highest.is_finite() {
                    (highest.max(0.0) - band_floor).max(1.0)
                } else {
                    1.0
                };
                let disk_config = TerrainConfig {
                    chunks_x: n,
                    chunks_z: n,
                    view_distance: n as f32 * config.chunk_size,
                    height_offset: band_floor,
                    height_scale: band_range,
                    ..config.clone()
                };

                // Centre the imported grid the way the importer numbers its
                // chunks (`cx - full / 2`), so importer chunk 0 lands on
                // loader chunk `-full / 2`.
                let mut disk_data = TerrainData::procedural();
                disk_data.resize_cache(&disk_config);
                let padding = disk_config.normalized_height(0.0);
                disk_data.height_cache.iter_mut().for_each(|h| *h = padding);
                let off_x = (n - full_x / 2) as usize * res;
                let off_z = (n - full_z / 2) as usize * res;
                let (src_w, src_h) = (data.cache_width as usize, data.cache_height as usize);
                let dst_w = disk_data.cache_width as usize;
                let dst_h = disk_data.cache_height as usize;
                for src_z in 0..src_h {
                    let dst_z = off_z + src_z;
                    if dst_z >= dst_h {
                        break;
                    }
                    for src_x in 0..src_w {
                        let dst_x = off_x + src_x;
                        if dst_x >= dst_w {
                            break;
                        }
                        let Some(&h) = data.height_cache.get(src_z * src_w + src_x) else { continue };
                        disk_data.height_cache[dst_z * dst_w + dst_x] =
                            disk_config.normalized_height(config.world_height(h));
                    }
                }

                // Step 3: Clear the previous terrain's files. Chunk files
                // beyond the new grid would outlive it, old material maps
                // (legacy splatmaps included) would paint the new ground on
                // load, and old volume bricks would carve the old caves
                // into it.
                for (dir, extension) in [
                    (chunks_dir.clone(), "r16"),
                    (terrain_dir.join(eustress_common::terrain::toml_loader::MATMAP_DIR), "png"),
                    (terrain_dir.join(eustress_common::terrain::toml_loader::LEGACY_SPLATMAP_DIR), "png"),
                    (eustress_common::terrain::volume::volume_dir(&terrain_dir), "vbk"),
                ] {
                    let Ok(entries) = std::fs::read_dir(&dir) else { continue };
                    for entry in entries.flatten() {
                        let file = entry.path();
                        if file.extension().and_then(|e| e.to_str()) == Some(extension) {
                            if let Err(e) = std::fs::remove_file(&file) {
                                if e.kind() != std::io::ErrorKind::NotFound {
                                    warn!("Import Heightmap: could not remove stale {:?}: {}", file, e);
                                }
                            }
                        }
                    }
                }
                // A generated world's default layers go with its ground: its
                // lakes would flood their whole footprints over the import.
                // Layers the user made stay.
                eustress_common::terrain::worldgen::default_layers::clear_generated_layers(&sr.0);

                // Step 4: Write every chunk of the grid, the same span
                // `save_terrain_to_disk` writes on Save.
                let n_i = n as i32;
                let positions: Vec<IVec2> = (-n_i..=n_i)
                    .flat_map(|cz| (-n_i..=n_i).map(move |cx| IVec2::new(cx, cz)))
                    .collect();
                if let Err(e) = eustress_common::terrain::toml_loader::save_chunks_to_disk(
                    &terrain_dir,
                    &disk_config,
                    &disk_data,
                    &positions,
                ) {
                    notifications.error(format!("Import Heightmap: could not write chunks: {e}"));
                    error!("Heightmap import chunk write failed for {:?}: {}", terrain_dir, e);
                    continue;
                }
                // An elevation file carries no materials, so every writer's
                // matmap contract is met with all-Grass maps. A failure here
                // is not fatal: the loader reads a chunk with no matmap as
                // Grass too.
                eustress_common::terrain::height_query::ensure_material_cache(&mut disk_data);
                if let Err(e) = eustress_common::terrain::toml_loader::save_material_chunks_to_disk(
                    &terrain_dir,
                    &disk_config,
                    &disk_data,
                    &positions,
                ) {
                    warn!("Import Heightmap: could not write material maps for {:?}: {}", terrain_dir, e);
                }

                // Step 5: Write _terrain.toml config
                let terrain_toml = format!(
                    r#"# Auto-generated from imported heightmap: {}
[terrain]
chunk_size = {:.1}
chunk_resolution = {}
height_scale = {:?}
height_offset = {:?}
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
                    disk_config.chunk_size,
                    disk_config.chunk_resolution,
                    disk_config.height_scale,
                    disk_config.height_offset,
                    disk_config.view_distance,
                    disk_config.lod_levels,
                    disk_config.lod_distances,
                );
                if let Err(e) = std::fs::write(terrain_dir.join("_terrain.toml"), terrain_toml) {
                    notifications.error(format!("Import Heightmap: could not write _terrain.toml: {e}"));
                    error!("Heightmap import toml write failed for {:?}: {}", terrain_dir, e);
                    continue;
                }

                // Step 6: Spawn from what was just written, so the live
                // config and band are exactly the ones Save writes back
                // under this toml (the same round trip worldgen and the flat
                // plate take). The old terrain goes only once the new one
                // loaded.
                match crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir) {
                    Ok(terrain) => {
                        for entity in existing_terrain.iter() {
                            commands.entity(entity).despawn();
                        }
                        let entity = terrain.spawn(&mut commands, &mut meshes, &mut materials);
                        commands.entity(entity).insert(crate::terrain_disk_load::DiskSourcedTerrain);
                    }
                    Err(e) => {
                        notifications.error(format!("Import Heightmap: wrote files but load failed: {e}"));
                        error!("Heightmap import load-back failed for {:?}: {}", terrain_dir, e);
                        continue;
                    }
                }

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

/// System: kick off a Generate World request on the async compute pool.
///
/// The task runs `worldgen::pipeline::generate_world` (rayon inside,
/// deterministic, engine-free) then `worldgen::export::export_to_space`
/// into the LIVE Space's `Workspace/Terrain/`, never `default_space_root()`,
/// which re-reads the last-opened path from settings and can name a
/// different Space after an in-session switch. No ECS access inside the task.
pub fn handle_generate_world(
    mut request_events: MessageReader<GenerateWorldEvent>,
    mut worldgen: ResMut<WorldgenTask>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    queue: Option<Res<eustress_common::terrain::TerrainGenerationQueue>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    let mut notifications = notifications;
    for event in request_events.read() {
        // Single-flight gate: a running task, the meshing tail, OR any
        // in-flight chunk queue (spawn_terrain would clobber
        // TerrainGenerationQueue and orphan its pending chunks).
        let queue_busy = queue.as_deref().map(|q| q.is_generating()).unwrap_or(false);
        if worldgen.is_busy() || queue_busy {
            if let Some(ref mut n) = notifications {
                n.warning("Terrain generation already running — wait for it to finish");
            }
            continue;
        }
        let Some(ref sr) = space_root else {
            if let Some(ref mut n) = notifications {
                n.error("Generate World: no open Space");
            }
            continue;
        };
        let space_root_path = sr.0.clone();
        let spec = event.spec.clone();
        let seed = spec.seed;

        worldgen.status = format!(
            "Generating world (seed {}, {}\u{d7}{} regions)\u{2026}",
            seed, spec.regions_x, spec.regions_z
        );
        worldgen.task = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            // Pure CPU work — both calls are engine-free and deterministic.
            let world = eustress_common::terrain::worldgen::pipeline::generate_world(&spec);
            let summary = eustress_common::terrain::worldgen::export::export_to_space(
                &world,
                &space_root_path,
            )?;
            Ok((space_root_path.join("Workspace").join("Terrain"), summary))
        }));
        if let Some(ref mut n) = notifications {
            n.info(format!("Generating world (seed {seed})\u{2026}"));
        }
        info!("🌍 Worldgen started: seed {}, {}x{} regions", seed, event.spec.regions_x, event.spec.regions_z);
    }
}

/// System: poll the worldgen task; on success hydrate the exported terrain
/// from disk (SIGNED centered chunk coords via `toml_loader`) and spawn it
/// through the SAME `spawn_terrain` path heightmap import uses. The
/// already-registered `process_terrain_generation_queue` / `chunk_spawn_system`
/// chain (EngineTerrainPlugin) meshes it over frames.
pub fn poll_worldgen_task(
    mut commands: Commands,
    mut worldgen: ResMut<WorldgenTask>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    queue: Option<Res<eustress_common::terrain::TerrainGenerationQueue>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    use bevy::tasks::{block_on, futures_lite::future};
    let mut notifications = notifications;

    // Meshing tail: clear busy once the chunk queue drains.
    if worldgen.task.is_none() && worldgen.meshing {
        let still_meshing = queue.as_deref().map(|q| q.is_generating()).unwrap_or(false);
        if !still_meshing {
            worldgen.meshing = false;
            worldgen.status.clear();
        }
        return;
    }

    let Some(task) = worldgen.task.as_mut() else { return };
    let Some(result) = block_on(future::poll_once(task)) else { return };
    worldgen.task = None;

    match result {
        Ok((terrain_dir, summary)) => {
            // Hydrate exactly what export wrote (export.rs INTEGRATOR NOTE):
            // toml → config → resize_cache → load_chunks_from_disk (signed
            // centered [-N,+N] addressing, matching export by construction).
            match crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir) {
                Ok(terrain) => {
                    let chunk_files = terrain.chunk_files;
                    // Despawn existing terrain (any source) — single-rooted.
                    for entity in existing_terrain.iter() {
                        commands.entity(entity).despawn();
                    }
                    let entity = terrain.spawn(&mut commands, &mut meshes, &mut materials);
                    // Disk-sourced: Space-switch cleanup + the auto-loader's
                    // latch semantics treat it like any disk terrain.
                    commands.entity(entity).insert(crate::terrain_disk_load::DiskSourcedTerrain);
                    worldgen.meshing = true;
                    worldgen.status = "Spawning terrain chunks\u{2026}".to_string();
                    if let Some(ref mut n) = notifications {
                        n.success(format!(
                            "World generated: {} chunks written ({} material maps, {} default layers), meshing\u{2026}",
                            summary.chunks_written, summary.matmaps_written, summary.layers_written
                        ));
                    }
                    info!(
                        "🌍 Worldgen exported {} chunks / {} material maps / {} default layers ({} bytes), loaded {} chunk files from {:?}",
                        summary.chunks_written, summary.matmaps_written, summary.layers_written,
                        summary.bytes_written, chunk_files, terrain_dir
                    );
                }
                Err(e) => {
                    worldgen.status.clear();
                    if let Some(ref mut n) = notifications {
                        n.error(format!("Generate World: export wrote but load failed: {e}"));
                    }
                    error!("Worldgen load-back failed for {:?}: {}", terrain_dir, e);
                }
            }
        }
        Err(e) => {
            worldgen.status.clear();
            if let Some(ref mut n) = notifications {
                n.error(format!("Generate World failed: {e}"));
            }
            error!("Worldgen failed: {}", e);
        }
    }
}

/// System: write + load a flat terrain plate for the live Space.
///
/// Runs the SAME disk round-trip the worldgen exporter and heightmap import
/// use (`export_flat_to_space` -> `hydrate_terrain_from_disk` ->
/// `spawn_terrain`), so the plate persists across a Space reload and the
/// auto-loader treats it like any other disk terrain.
pub fn handle_generate_flat_terrain(
    mut request_events: MessageReader<GenerateFlatTerrainEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut worldgen: ResMut<WorldgenTask>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    queue: Option<Res<eustress_common::terrain::TerrainGenerationQueue>>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    use eustress_common::terrain::worldgen::export::export_flat_to_space;

    let mut notifications = notifications;
    for event in request_events.read() {
        // Same single-flight gate as handle_generate_world: replacing the
        // terrain while chunks are still queued would orphan them.
        let queue_busy = queue.as_deref().map(|q| q.is_generating()).unwrap_or(false);
        if worldgen.is_busy() || queue_busy {
            if let Some(ref mut n) = notifications {
                n.warning("Terrain generation already running, wait for it to finish");
            }
            continue;
        }
        let Some(ref sr) = space_root else {
            if let Some(ref mut n) = notifications {
                n.error("Flat Terrain: no open Space");
            }
            continue;
        };

        let extent = event.spec.total_extent_m();
        let terrain_dir = sr.0.join("Workspace").join("Terrain");

        let summary = match export_flat_to_space(&event.spec, &sr.0) {
            Ok(summary) => summary,
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.error(format!("Flat Terrain failed: {e}"));
                }
                error!("Flat terrain export failed: {}", e);
                continue;
            }
        };

        match crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir) {
            Ok(terrain) => {
                let chunk_files = terrain.chunk_files;
                for entity in existing_terrain.iter() {
                    commands.entity(entity).despawn();
                }
                let entity = terrain.spawn(&mut commands, &mut meshes, &mut materials);
                commands
                    .entity(entity)
                    .insert(crate::terrain_disk_load::DiskSourcedTerrain);
                worldgen.meshing = true;
                worldgen.status = "Spawning terrain chunks\u{2026}".to_string();
                if let Some(ref mut n) = notifications {
                    n.success(format!(
                        "Flat terrain: {:.0}\u{d7}{:.0} m at Y={:.1}, {} chunks, meshing\u{2026}",
                        extent, extent, event.spec.height_m, summary.chunks_written
                    ));
                }
                info!(
                    "\u{1f9f1} Flat terrain: {:.0}x{:.0} m at Y={:.1}, wrote {} chunks / {} material maps, loaded {} chunk files",
                    extent, extent, event.spec.height_m,
                    summary.chunks_written, summary.matmaps_written, chunk_files
                );
            }
            Err(e) => {
                if let Some(ref mut n) = notifications {
                    n.error(format!("Flat Terrain: wrote files but load failed: {e}"));
                }
                error!("Flat terrain load-back failed for {:?}: {}", terrain_dir, e);
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
            // Worldgen (Generate World): message + single-flight task
            // resource. LOOP-5: registered HERE (SpawnEventsPlugin is added
            // by the ACTIVE SlintUiPlugin), never in the legacy StudioUiPlugin.
            .add_message::<GenerateWorldEvent>()
            // Flat plate (Generate > Flat). Registered in the same block for
            // the same LOOP-5 reason: `DrainEventWriters` takes a NON-Option
            // writer for it, so a missing registration fails the drain's
            // param validation and silently kills EVERY UI button.
            .add_message::<GenerateFlatTerrainEvent>()
            .init_resource::<WorldgenTask>()
            .add_systems(Update, (
                handle_spawn_terrain_events,
                handle_toggle_terrain_edit,
                handle_set_terrain_brush,
                handle_import_terrain,
                handle_generate_world,
                handle_generate_flat_terrain,
                poll_worldgen_task,
            ));
    }
}
