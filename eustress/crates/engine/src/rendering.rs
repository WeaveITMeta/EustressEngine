#![allow(dead_code)]

use bevy::prelude::*;
use bevy::core_pipeline::tonemapping::Tonemapping;
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use crate::classes::{Instance, BasePart, Part as ClassPart};
use crate::parts::PartType;

#[cfg(not(target_arch = "wasm32"))]
use crate::commands::{SelectionManager, TransformManager}; // Production managers

/// Event fired when a part changes (for efficient updates)
/// 
/// **Performance**: Fire this event whenever you modify a part's transform
/// to trigger only the necessary updates instead of checking all parts every frame.
/// 
/// Example:
/// ```rust
/// // After modifying a part's position/rotation/scale:
/// events.write(PartChanged { part_id: "123".to_string() });
/// ```
#[derive(Message, Clone)]
pub struct PartChanged {
    pub part_id: String,
}

/// Component to track which part this entity represents
#[derive(Component)]
pub struct PartEntity {
    pub part_id: String,
}

/// Component for selection highlighting
#[derive(Component)]
pub struct SelectionHighlight;

/// Resource to track spawned entities
#[derive(Resource)]
pub struct PartEntities {
    pub map: HashMap<String, Entity>,
}

impl Default for PartEntities {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

/// Legacy PartManager wrapper - DEPRECATED, use Instance/BasePart/Part components instead
#[derive(Resource)]
pub struct BevyPartManager(pub Arc<RwLock<crate::parts::PartManager>>);

/// Resource wrapping SelectionManager for Bevy access (native only)
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Clone)]
pub struct BevySelectionManager(pub Arc<RwLock<SelectionManager>>);

/// Resource wrapping TransformManager for Bevy access (native only)
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Clone)]
pub struct BevyTransformManager(pub Arc<RwLock<TransformManager>>);

/// Plugin for rendering parts in the Bevy viewport (uses ECS queries)
pub struct PartRenderingPlugin {
    #[cfg(not(target_arch = "wasm32"))]
    pub selection_manager: Arc<RwLock<SelectionManager>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub transform_manager: Arc<RwLock<TransformManager>>,
}

impl Plugin for PartRenderingPlugin {
    fn build(&self, app: &mut App) {
        // Only add selection/transform managers for native builds
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.insert_resource(BevySelectionManager(self.selection_manager.clone()))
               .insert_resource(BevyTransformManager(self.transform_manager.clone()));
        }
        
        app.init_resource::<PartEntities>()
            .add_message::<PartChanged>();  // Register event for efficient updates
            // Phase 3: Systems moved to ECS queries
            // Rendering now handled by Bevy's standard rendering pipeline
            // Parts are spawned via classes::spawn_part() with proper PbrBundle
        
        // Only add selection highlighting for native builds
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, update_selection_highlights);
    }
}

/// Setup initial scene with lighting and ground
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Add camera (Studio viewport camera) - Bevy 0.17 required components
    // Use Reinhard tonemapping to avoid magenta bug from missing LUT textures
    commands.spawn((
        Camera3d::default(),
        Tonemapping::Reinhard,
        Transform::from_xyz(10.0, 10.0, 15.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    
    // Add directional light (sun) - Bevy 0.17 required components
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(50.0, 100.0, 50.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Add ambient light (Bevy 0.18 - GlobalAmbientLight is the Resource)
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });

    // Add ground plane (baseplate) - Bevy 0.17 required components
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(100.0, 100.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.3, 0.3),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
}

/// Spawn 3D meshes for new parts
/// DEPRECATED: Use new ECS spawn functions from classes.rs instead
#[allow(dead_code)]
fn spawn_new_parts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    part_manager: Res<BevyPartManager>,
    mut part_entities: ResMut<PartEntities>,
    existing_parts: Query<&PartEntity>,
) {
    // Get all parts from manager
    let parts = match part_manager.0.read().list_parts() {
        Ok(parts) => parts,
        Err(e) => {
            warn!("Failed to list parts: {}", e);
            return;
        }
    };

    // Track which part IDs already have entities
    let existing_ids: Vec<String> = existing_parts.iter()
        .map(|p| p.part_id.clone())
        .collect();

    // Spawn meshes for new parts
    for part in parts {
        if existing_ids.contains(&part.id.to_string()) {
            continue; // Already spawned
        }

        // Check if this is a Model (container only, no geometry)
        // Models are identified by having a name that starts with "Model"
        let is_model = part.name.starts_with("Model");
        
        if is_model {
            // Models are logical containers - spawn entity without geometry (Bevy 0.17)
            let entity = commands.spawn((
                Transform {
                    translation: Vec3::new(part.position[0], part.position[1], part.position[2]),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        part.rotation[0].to_radians(),
                        part.rotation[1].to_radians(),
                        part.rotation[2].to_radians(),
                    ),
                    scale: Vec3::new(part.size[0], part.size[1], part.size[2]),
                },
                Visibility::default(),
                PartEntity {
                    part_id: part.id.to_string(),
                },
            )).id();
            
            part_entities.map.insert(part.id.to_string(), entity);
            info!("Spawned container entity (no mesh) for model: {}", part.id);
        } else {
            // Regular parts - spawn with geometry
            let mesh = match part.part_type {
                PartType::Cube => meshes.add(Cuboid::default()),
                PartType::Sphere => meshes.add(Sphere::default().mesh().ico(5).unwrap()),
                PartType::Cylinder => meshes.add(Cylinder::default()),
                PartType::Wedge => meshes.add(Cuboid::default()), // TODO: Proper wedge mesh
                PartType::CornerWedge => meshes.add(Cuboid::default()), // TODO: Proper corner wedge
                PartType::Cone => meshes.add(Cylinder::default()), // TODO: Proper cone mesh
            };

            let color = Color::srgba(
                part.color[0],
                part.color[1],
                part.color[2],
                1.0 - part.transparency,
            );

            let material = materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.5,
                metallic: 0.1,
                alpha_mode: if part.transparency > 0.0 {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                ..default()
            });

            let entity = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform {
                    translation: Vec3::new(part.position[0], part.position[1], part.position[2]),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        part.rotation[0].to_radians(),
                        part.rotation[1].to_radians(),
                        part.rotation[2].to_radians(),
                    ),
                    scale: Vec3::new(part.size[0], part.size[1], part.size[2]),
                },
                PartEntity {
                    part_id: part.id.to_string(),
                },
            )).id();

            part_entities.map.insert(part.id.to_string(), entity);
            info!("Spawned entity for part: {}", part.id);
        }
    }
}

/// Despawn entities for parts that have been deleted  
/// DEPRECATED: Use new ECS systems instead
#[allow(dead_code)]
fn despawn_deleted_parts(
    mut commands: Commands,
    part_manager: Res<BevyPartManager>,
    mut part_entities: ResMut<PartEntities>,
) {
    // Get current part IDs from PartManager
    let pm = part_manager.0.read();
    let parts = match pm.list_parts() {
        Ok(parts) => parts,
        Err(e) => {
            warn!("Failed to list parts for cleanup: {}", e);
            return;
        }
    };
    
    let current_part_ids: HashSet<String> = parts
        .iter()
        .map(|p| p.id.to_string())
        .collect();
    
    // Find entities that no longer have corresponding parts
    let mut to_remove = Vec::new();
    for (part_id, entity) in part_entities.map.iter() {
        if !current_part_ids.contains(part_id) {
            // Part was deleted, despawn its entity
            commands.entity(*entity).despawn();
            to_remove.push(part_id.clone());
            info!("Despawned entity for deleted part: {}", part_id);
        }
    }
    
    // Remove from tracking map
    for part_id in to_remove {
        part_entities.map.remove(&part_id);
    }
}

/// Event-driven transform updates - only updates changed parts!
/// This is MUCH faster than iterating all parts every frame
/// DEPRECATED: Use new ECS systems instead
#[allow(dead_code)]
fn update_part_transforms_event_driven(
    mut ev_changed: MessageReader<PartChanged>,
    part_manager: Res<BevyPartManager>,
    part_entities: Res<PartEntities>,
    mut query: Query<&mut Transform>,
) {
    for ev in ev_changed.read() {
        // Parse part ID from String to u32
        let part_id: u32 = match ev.part_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        
        // Get the entity for this part
        let entity = match part_entities.map.get(&ev.part_id) {
            Some(&e) => e,
            None => continue,
        };
        
        // Get current part data
        let part = match part_manager.0.read().get_part(part_id) {
            Some(p) => p,
            None => continue,
        };

        // Get the transform component
        if let Ok(mut transform) = query.get_mut(entity) {
            // Update transform
            transform.translation = Vec3::new(part.position[0], part.position[1], part.position[2]);
            transform.rotation = Quat::from_euler(
                EulerRot::XYZ,
                part.rotation[0].to_radians(),
                part.rotation[1].to_radians(),
                part.rotation[2].to_radians(),
            );
            transform.scale = Vec3::new(part.size[0], part.size[1], part.size[2]);
        }
    }
}

/// Legacy function for reference - replaced by event-driven system
#[allow(dead_code)]
fn update_part_transforms_old(
    part_manager: Res<BevyPartManager>,
    _part_entities: Res<PartEntities>,
    mut query: Query<(&PartEntity, &mut Transform)>,
) {
    // This old approach iterates ALL parts EVERY frame - very slow!
    for (part_entity, mut transform) in query.iter_mut() {
        let part_id: u32 = match part_entity.part_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        
        let part = match part_manager.0.read().get_part(part_id) {
            Some(p) => p,
            None => continue,
        };

        let target_translation = Vec3::new(part.position[0], part.position[1], part.position[2]);
        let target_rotation = Quat::from_euler(
            EulerRot::XYZ,
            part.rotation[0].to_radians(),
            part.rotation[1].to_radians(),
            part.rotation[2].to_radians(),
        );
        let target_scale = Vec3::new(part.size[0], part.size[1], part.size[2]);

        if transform.translation.distance(target_translation) > 0.001
            || transform.rotation.angle_between(target_rotation) > 0.001
            || transform.scale.distance(target_scale) > 0.001
        {
            transform.translation = target_translation;
            transform.rotation = target_rotation;
            transform.scale = target_scale;
        }
    }
}

/// Update selection highlights (native only - requires SelectionManager)
#[cfg(not(target_arch = "wasm32"))]
fn update_selection_highlights(
    mut commands: Commands,
    selection_manager: Option<Res<BevySelectionManager>>,
    part_entities: Res<PartEntities>,
    highlighted: Query<Entity, With<SelectionHighlight>>,
    _parts: Query<&PartEntity>,
) {
    let Some(selection_manager) = selection_manager else { return };
    // Get currently selected part IDs
    let selected = selection_manager.0.read().get_selected();

    // Remove all existing highlights
    for entity in highlighted.iter() {
        commands.entity(entity).remove::<SelectionHighlight>();
    }

    // Add highlights to selected parts
    for part_id in selected {
        if let Some(&entity) = part_entities.map.get(&part_id) {
            commands.entity(entity).insert(SelectionHighlight);
            // TODO: Add visual highlight material/glow
        }
    }
}
