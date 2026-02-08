//! # Default Scene - Shared between Engine Studio and Client
//!
//! This module provides the default starter scene in both formats:
//! - `Scene` struct for serialization/deserialization (RON files)
//! - Bevy spawn functions for runtime entity creation
//!
//! This ensures both the editor and player start with the exact same scene.

use bevy::prelude::*;
use crate::scene::{
    Scene, SceneMetadata, AtmosphereSettings, Entity as SceneEntity,
    EntityClass, PartData, TransformData, NodeCategory, DetailLevel,
    NetworkOwnershipRule, WorkspaceSettings, PlayerSettings, OrbitalSettings,
};
use crate::classes::{Instance, ClassName, BasePart, Part, PartType, Material as PartMaterial};
use crate::attributes::{Attributes, Tags};
use std::collections::HashMap;

/// Component to track which part this entity represents (for selection)
#[derive(Component, Clone, Debug)]
pub struct PartEntityMarker {
    pub part_id: String,
}

// ============================================================================
// Scene Data Format
// ============================================================================

/// Create the default starter scene as a `Scene` struct.
/// 
/// This is the canonical definition - use this for saving/loading.
/// Contains:
/// - Baseplate: 512x1x512 dark gray platform
/// - Welcome Cube: 2x2x2 green cube sitting on the baseplate
pub fn default_scene() -> Scene {
    Scene {
        format: "eustress_v3".to_string(),
        metadata: SceneMetadata {
            name: "Untitled Scene".to_string(),
            description: "A new Eustress scene".to_string(),
            author: String::new(),
            created: String::new(),
            modified: String::new(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            tags: vec!["starter".to_string()],
        },
        global_theme: "modern minimalist".to_string(),
        atmosphere: AtmosphereSettings::default(),
        workspace_settings: WorkspaceSettings::default(),
        player_settings: PlayerSettings::default(),
        spawn_locations: Vec::new(),
        orbital_settings: OrbitalSettings::default(),
        entities: vec![
            // Baseplate - ID 1
            SceneEntity {
                id: 1,
                name: "Baseplate".to_string(),
                parent: None,
                children: vec![2], // Welcome Cube is child
                class: EntityClass::Part(PartData {
                    size: [512.0, 1.0, 512.0],
                    color: [0.2, 0.2, 0.22, 1.0], // Dark gray
                    material: "SmoothPlastic".to_string(),
                    shape: "Block".to_string(),
                    transparency: 0.0,
                    reflectance: 0.4,
                    anchored: true,
                    can_collide: true,
                    cast_shadow: true,
                }),
                transform: TransformData {
                    position: [0.0, -0.5, 0.0], // Top surface at Y=0
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0, 1.0, 1.0],
                },
                network_ownership: NetworkOwnershipRule::ServerOnly,
                prompt: String::new(),
                detail_level: DetailLevel::Low,
                category: NodeCategory::Terrain, // Baseplate is ground/terrain
                quest_flags: HashMap::new(),
                generated_mesh_id: None,
                generated_texture_id: None,
                generated_lods: Vec::new(),
                generation_status: crate::scene::GenerationStatus::NotRequested,
                archivable: true,
                ai: false,
            },
            // Welcome Cube - ID 2
            // Size: 1.96133m (Space Grade Ready: 9.80665 / 5)
            SceneEntity {
                id: 2,
                name: "Welcome Cube".to_string(),
                parent: Some(1), // Child of Baseplate
                children: Vec::new(),
                class: EntityClass::Part(PartData {
                    size: [1.96133, 1.96133, 1.96133],
                    color: [0.4, 0.9, 0.6, 1.0], // Green
                    material: "Plastic".to_string(),
                    shape: "Block".to_string(),
                    transparency: 0.0,
                    reflectance: 0.0,
                    anchored: false,
                    can_collide: true,
                    cast_shadow: true,
                }),
                transform: TransformData {
                    position: [0.0, 0.980665, 0.0], // Sitting on baseplate (half of 1.96133)
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0, 1.0, 1.0],
                },
                network_ownership: NetworkOwnershipRule::ClientClaimable,
                prompt: String::new(),
                detail_level: DetailLevel::Medium,
                category: NodeCategory::Prop,
                quest_flags: HashMap::new(),
                generated_mesh_id: None,
                generated_texture_id: None,
                generated_lods: Vec::new(),
                generation_status: crate::scene::GenerationStatus::NotRequested,
                archivable: true,
                ai: false,
            },
        ],
        connections: Vec::new(),
    }
}

/// Serialize the default scene to RON format
pub fn default_scene_ron() -> String {
    let scene = default_scene();
    ron::ser::to_string_pretty(&scene, ron::ser::PrettyConfig::default())
        .unwrap_or_else(|_| "// Error serializing scene".to_string())
}

// ============================================================================
// Bevy Runtime Spawning
// ============================================================================

/// Spawn the default baseplate - dark gray 512x1x512 platform
/// Material matches Roblox SmoothPlastic with subtle specular highlights
pub fn spawn_baseplate(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> bevy::ecs::entity::Entity {
    let baseplate_size = Vec3::new(512.0, 1.0, 512.0);
    
    // Create Instance component for selection/explorer
    let instance = Instance {
        name: "Baseplate".to_string(),
        class_name: ClassName::Part,
        archivable: true,
        id: 1,
        ai: false,
    };
    
    // Create BasePart component for part properties
    let base_part = BasePart {
        size: baseplate_size,
        color: Color::srgba(0.2, 0.2, 0.22, 1.0),
        material: PartMaterial::SmoothPlastic,
        transparency: 0.0,
        reflectance: 0.4,
        anchored: true,
        can_collide: true,
        locked: true, // Baseplate is locked by default
        cframe: Transform::from_xyz(0.0, -0.5, 0.0),
        ..default()
    };
    
    // Create Part component for shape
    let part = Part {
        shape: PartType::Block,
    };
    
    let mesh = meshes.add(Cuboid::new(baseplate_size.x, baseplate_size.y, baseplate_size.z));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.2, 0.22, 1.0),
        perceptual_roughness: 0.7,
        metallic: 0.0,
        reflectance: 0.4,
        ..default()
    });
    
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, -0.5, 0.0),
        instance,
        base_part,
        part,
        PartEntityMarker { part_id: "Baseplate".to_string() },
        Name::new("Baseplate"),
        Attributes::new(),
        Tags::new(),
    )).id()
}

/// Spawn the welcome cube - green 1.96133x1.96133x1.96133 cube at origin (Space Grade Ready)
/// Material matches Roblox Plastic with specular highlights
pub fn spawn_welcome_cube(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> bevy::ecs::entity::Entity {
    // Space Grade Ready: 9.80665 / 5 = 1.96133m
    let cube_size = Vec3::new(1.96133, 1.96133, 1.96133);
    
    // Create Instance component for selection/explorer
    let instance = Instance {
        name: "Welcome Cube".to_string(),
        class_name: ClassName::Part,
        archivable: true,
        id: 2,
        ai: false,
    };
    
    // Create BasePart component for part properties
    let base_part = BasePart {
        size: cube_size,
        color: Color::srgba(0.4, 0.9, 0.6, 1.0),
        material: PartMaterial::Plastic,
        transparency: 0.0,
        reflectance: 0.0,
        anchored: false,
        can_collide: true,
        locked: false,
        cframe: Transform::from_xyz(0.0, 0.980665, 0.0), // Half of 1.96133
        ..default()
    };
    
    // Create Part component for shape
    let part = Part {
        shape: PartType::Block,
    };
    
    let mesh = meshes.add(Cuboid::new(cube_size.x, cube_size.y, cube_size.z));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.9, 0.6, 1.0),
        perceptual_roughness: 0.8,
        metallic: 0.0,
        ..default()
    });
    
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, 0.980665, 0.0),
        instance,
        base_part,
        part,
        PartEntityMarker { part_id: "Welcome Cube".to_string() },
        Name::new("Welcome Cube"),
        Attributes::new(),
        Tags::new(),
    )).id()
}

/// Spawn the complete default scene (baseplate + welcome cube)
/// Returns (baseplate_entity, cube_entity)
pub fn spawn_default_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> (bevy::ecs::entity::Entity, bevy::ecs::entity::Entity) {
    let baseplate = spawn_baseplate(commands, meshes, materials);
    let cube = spawn_welcome_cube(commands, meshes, materials);
    (baseplate, cube)
}

/// Spawn entities from a Scene struct into the Bevy world
/// 
/// This is the bridge between the serialized Scene format and runtime entities.
/// Returns a mapping of scene entity IDs to Bevy Entity handles.
pub fn spawn_scene(
    scene: &Scene,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> HashMap<u32, bevy::ecs::entity::Entity> {
    let mut entity_map = HashMap::new();
    
    for scene_entity in &scene.entities {
        let bevy_entity = spawn_scene_entity(scene_entity, commands, meshes, materials);
        entity_map.insert(scene_entity.id, bevy_entity);
    }
    
    // TODO: Set up parent-child relationships using entity_map
    
    entity_map
}

/// Spawn a single scene entity
fn spawn_scene_entity(
    entity: &SceneEntity,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> bevy::ecs::entity::Entity {
    let transform: Transform = entity.transform.clone().into();
    
    match &entity.class {
        EntityClass::Part(part_data) => {
            let size = Vec3::new(part_data.size[0], part_data.size[1], part_data.size[2]);
            let color = Color::srgba(part_data.color[0], part_data.color[1], part_data.color[2], part_data.color[3]);
            
            // Create Instance component for selection/explorer
            let instance = Instance {
                name: entity.name.clone(),
                class_name: ClassName::Part,
                archivable: entity.archivable,
                id: entity.id,
                ai: entity.ai,
            };
            
            // Create BasePart component for part properties
            let base_part = BasePart {
                size,
                color,
                material: PartMaterial::from_string(&part_data.material),
                transparency: part_data.transparency,
                reflectance: part_data.reflectance,
                anchored: part_data.anchored,
                can_collide: part_data.can_collide,
                locked: false,
                cframe: transform,
                ..default()
            };
            
            // Create Part component for shape
            let part = Part {
                shape: PartType::from_string(&part_data.shape),
            };
            
            // DISABLED for Bevy 0.19 - Mesh3d Bundle API changed
            // TODO: Fix Mesh3d Bundle usage for Bevy 0.19
            commands.spawn((
                transform,
                instance,
                base_part,
                part,
                PartEntityMarker { part_id: entity.name.clone() },
                Name::new(entity.name.clone()),
                Attributes::new(),
                Tags::new(),
            )).id()
        }
        EntityClass::Folder => {
            commands.spawn((
                transform,
                Name::new(entity.name.clone()),
            )).id()
        }
        // TODO: Handle other entity classes
        _ => {
            commands.spawn((
                transform,
                Name::new(entity.name.clone()),
            )).id()
        }
    }
}
