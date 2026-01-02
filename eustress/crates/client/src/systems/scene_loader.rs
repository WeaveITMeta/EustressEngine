//! Scene loading - Supports both JSON and RON formats
//! - JSON: eustress_propertyaccess (Engine/Studio format)
//! - RON: eustress_v3 (Unified format from common)

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// Re-export unified RON format from common
use eustress_common::Scene as RonScene;
use eustress_common::{Entity as RonEntity, EntityClass};

// ============================================================================
// JSON Scene Format (same as engine/src/serialization/scene.rs)
// ============================================================================

/// Scene file format (JSON-based, PropertyAccess-driven)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonScene {
    pub format: String,
    pub metadata: JsonSceneMetadata,
    pub entities: Vec<JsonEntityData>,
}

/// Scene metadata (JSON)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonSceneMetadata {
    pub name: String,
    pub description: String,
    pub author: String,
    pub created: String,
    pub modified: String,
    pub engine_version: String,
}

/// Entity data in JSON scene file
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonEntityData {
    pub id: u32,
    pub class: String,
    pub parent: Option<u32>,
    pub properties: HashMap<String, serde_json::Value>,
    pub children: Vec<u32>,
}

// ============================================================================
// Unified Scene Enum (supports both formats)
// ============================================================================

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum LoadedSceneData {
    Json(JsonScene),
    Ron(RonScene),
}

// ============================================================================
// Resources and Events
// ============================================================================

/// Resource to track loaded scene
#[derive(Resource, Default)]
pub struct LoadedScene {
    pub path: Option<PathBuf>,
    pub data: Option<LoadedSceneData>,
}

/// Message to trigger scene loading
#[derive(bevy::prelude::Message)]
pub struct LoadSceneEvent {
    pub path: PathBuf,
}

// ============================================================================
// Scene Loader System
// ============================================================================

/// System to load scenes from JSON or RON files
pub fn scene_loader_system(
    mut events: MessageReader<LoadSceneEvent>,
    mut loaded: ResMut<LoadedScene>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for event in events.read() {
        info!("📂 Loading scene from: {:?}", event.path);
        
        let scene_data = match std::fs::read_to_string(&event.path) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to read scene file: {}", e);
                continue;
            }
        };
        
        // Detect format by extension
        let extension = event.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        match extension.as_str() {
            "ron" => {
                // Parse as RON (unified format)
                match ron::from_str::<RonScene>(&scene_data) {
                    Ok(scene) => {
                        info!("✅ Loaded RON scene: {} by {}", scene.metadata.name, scene.metadata.author);
                        info!("🌍 Theme: {}", scene.global_theme);
                        info!("📦 {} entities to spawn", scene.entities.len());
                        
                        for entity in &scene.entities {
                            spawn_ron_entity(&mut commands, &mut meshes, &mut materials, entity);
                        }
                        
                        loaded.data = Some(LoadedSceneData::Ron(scene));
                        loaded.path = Some(event.path.clone());
                    }
                    Err(e) => {
                        error!("Failed to parse RON scene: {}", e);
                    }
                }
            }
            "json" | _ => {
                // Parse as JSON (engine format)
                match serde_json::from_str::<JsonScene>(&scene_data) {
                    Ok(scene) => {
                        if scene.format != "eustress_propertyaccess" {
                            error!("Unknown JSON format: {}. Expected 'eustress_propertyaccess'", scene.format);
                            continue;
                        }
                        
                        info!("✅ Loaded JSON scene: {} by {}", scene.metadata.name, scene.metadata.author);
                        info!("📦 {} entities to spawn", scene.entities.len());
                        
                        for entity in &scene.entities {
                            spawn_json_entity(&mut commands, &mut meshes, &mut materials, entity);
                        }
                        
                        loaded.data = Some(LoadedSceneData::Json(scene));
                        loaded.path = Some(event.path.clone());
                    }
                    Err(e) => {
                        error!("Failed to parse JSON scene: {}", e);
                    }
                }
            }
        }
    }
}

// ============================================================================
// JSON Entity Spawning
// ============================================================================

/// Spawn an entity from JSON scene data
fn spawn_json_entity(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    entity: &JsonEntityData,
) {
    // Get name from properties
    let name = entity.properties.get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("Entity")
        .to_string();
    
    // Get transform from CFrame property
    let transform = extract_json_transform(&entity.properties);
    
    // Spawn based on class
    match entity.class.as_str() {
        "Part" => spawn_json_part(commands, meshes, materials, entity, &name, transform),
        "Model" | "Folder" => spawn_container(commands, &name, transform),
        "PointLight" => spawn_json_point_light(commands, entity, &name, transform),
        "SpotLight" => spawn_json_spot_light(commands, entity, &name, transform),
        "Camera" => {
            info!("  ⏭️  Skipping camera: {}", name);
        }
        _ => {
            info!("  ❓ Unknown class '{}': {}", entity.class, name);
            commands.spawn((
                transform,
                Visibility::default(),
                Name::new(name),
            ));
        }
    }
}

// ============================================================================
// RON Entity Spawning
// ============================================================================

/// Spawn an entity from RON scene data
fn spawn_ron_entity(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    entity: &RonEntity,
) {
    let transform = Transform {
        translation: Vec3::new(
            entity.transform.position[0],
            entity.transform.position[1],
            entity.transform.position[2],
        ),
        rotation: Quat::from_xyzw(
            entity.transform.rotation[0],
            entity.transform.rotation[1],
            entity.transform.rotation[2],
            entity.transform.rotation[3],
        ),
        scale: Vec3::new(
            entity.transform.scale[0],
            entity.transform.scale[1],
            entity.transform.scale[2],
        ),
    };
    
    match &entity.class {
        EntityClass::Part(part) => {
            let size = Vec3::new(part.size[0], part.size[1], part.size[2]);
            let color = Color::srgba(part.color[0], part.color[1], part.color[2], part.color[3]);
            
            let mesh = match part.shape.as_str() {
                "Ball" => meshes.add(Sphere::new(size.x / 2.0)),
                "Cylinder" => meshes.add(Cylinder::new(size.x / 2.0, size.y)),
                _ => meshes.add(Cuboid::new(size.x, size.y, size.z)),
            };
            
            let material = materials.add(StandardMaterial {
                base_color: color.with_alpha(1.0 - part.transparency),
                metallic: part.reflectance,
                perceptual_roughness: 1.0 - part.reflectance,
                alpha_mode: if part.transparency > 0.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
                ..default()
            });
            
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                Name::new(entity.name.clone()),
            ));
            info!("  🧱 Part: {} at {:?}", entity.name, transform.translation);
        }
        EntityClass::Model(_) | EntityClass::Folder => {
            commands.spawn((
                transform,
                Visibility::default(),
                Name::new(entity.name.clone()),
            ));
            info!("  📁 Container: {}", entity.name);
        }
        EntityClass::PointLight(light) => {
            let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
            commands.spawn((
                PointLight {
                    color,
                    intensity: light.brightness * 1000.0,
                    range: light.range,
                    shadows_enabled: light.shadows,
                    ..default()
                },
                transform,
                Name::new(entity.name.clone()),
            ));
            info!("  💡 PointLight: {}", entity.name);
        }
        EntityClass::SpotLight(light) => {
            let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
            commands.spawn((
                SpotLight {
                    color,
                    intensity: light.brightness * 1000.0,
                    range: light.range,
                    outer_angle: light.angle.to_radians(),
                    inner_angle: (light.angle * 0.8).to_radians(),
                    ..default()
                },
                transform,
                Name::new(entity.name.clone()),
            ));
            info!("  🔦 SpotLight: {}", entity.name);
        }
        EntityClass::NPC(_) => {
            let mesh = meshes.add(Cylinder::new(0.5, 2.0));
            let material = materials.add(Color::srgb(0.8, 0.6, 0.4));
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                Name::new(entity.name.clone()),
            ));
            info!("  🧑 NPC: {}", entity.name);
        }
        EntityClass::Portal(_) => {
            let mesh = meshes.add(Plane3d::default().mesh().size(2.0, 3.0));
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(0.5, 0.3, 0.8, 0.7),
                alpha_mode: AlphaMode::Blend,
                emissive: bevy::color::LinearRgba::new(0.5, 0.3, 0.8, 1.0),
                ..default()
            });
            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                Name::new(entity.name.clone()),
            ));
            info!("  🌀 Portal: {}", entity.name);
        }
        EntityClass::Trigger(_) => {
            commands.spawn((
                transform,
                Visibility::default(),
                Name::new(entity.name.clone()),
            ));
            info!("  ⚡ Trigger: {}", entity.name);
        }
        EntityClass::Camera(_) => {
            info!("  ⏭️  Skipping camera: {}", entity.name);
        }
        _ => {
            commands.spawn((
                transform,
                Visibility::default(),
                Name::new(entity.name.clone()),
            ));
            info!("  ❓ Other: {}", entity.name);
        }
    }
}

// ============================================================================
// JSON Helper Functions
// ============================================================================

/// Extract transform from CFrame property (JSON format)
fn extract_json_transform(props: &HashMap<String, serde_json::Value>) -> Transform {
    if let Some(cframe) = props.get("CFrame") {
        let pos = cframe.get("position")
            .and_then(|p| p.as_array())
            .map(|arr| Vec3::new(
                arr.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            ))
            .unwrap_or(Vec3::ZERO);
        
        let rot = cframe.get("rotation")
            .and_then(|r| r.as_array())
            .map(|arr| {
                let x = arr.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let y = arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let z = arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                Quat::from_euler(EulerRot::XYZ, x.to_radians(), y.to_radians(), z.to_radians())
            })
            .unwrap_or(Quat::IDENTITY);
        
        let scale = cframe.get("scale")
            .and_then(|s| s.as_array())
            .map(|arr| Vec3::new(
                arr.get(0).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                arr.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                arr.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            ))
            .unwrap_or(Vec3::ONE);
        
        Transform {
            translation: pos,
            rotation: rot,
            scale,
        }
    } else {
        Transform::default()
    }
}

/// Spawn a Part entity (JSON format)
fn spawn_json_part(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    entity: &JsonEntityData,
    name: &str,
    transform: Transform,
) {
    // Get size
    let size = entity.properties.get("Size")
        .and_then(|v| v.as_array())
        .map(|arr| Vec3::new(
            arr.get(0).and_then(|v| v.as_f64()).unwrap_or(2.0) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(2.0) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(2.0) as f32,
        ))
        .unwrap_or(Vec3::splat(2.0));
    
    // Get color
    let color = entity.properties.get("Color")
        .and_then(|v| v.as_array())
        .map(|arr| Color::srgba(
            arr.get(0).and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(0.7) as f32,
            arr.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        ))
        .unwrap_or(Color::srgb(0.7, 0.7, 0.7));
    
    // Get shape
    let shape = entity.properties.get("Shape")
        .and_then(|v| v.as_str())
        .unwrap_or("Block");
    
    // Create mesh based on shape
    let mesh = match shape {
        "Ball" => meshes.add(Sphere::new(size.x / 2.0)),
        "Cylinder" => meshes.add(Cylinder::new(size.x / 2.0, size.y)),
        "Wedge" => meshes.add(Cuboid::new(size.x, size.y, size.z)), // TODO: proper wedge
        _ => meshes.add(Cuboid::new(size.x, size.y, size.z)),
    };
    
    // Get transparency and reflectance
    let transparency = entity.properties.get("Transparency")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    
    let reflectance = entity.properties.get("Reflectance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    
    let material = materials.add(StandardMaterial {
        base_color: color.with_alpha(1.0 - transparency),
        metallic: reflectance,
        perceptual_roughness: 1.0 - reflectance,
        alpha_mode: if transparency > 0.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    });
    
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        transform,
        Name::new(name.to_string()),
    ));
    
    info!("  🧱 Part: {} at {:?} size {:?}", name, transform.translation, size);
}

/// Spawn a container (Model/Folder)
fn spawn_container(commands: &mut Commands, name: &str, transform: Transform) {
    commands.spawn((
        transform,
        Visibility::default(),
        Name::new(name.to_string()),
    ));
    info!("  📁 Container: {}", name);
}

/// Spawn a PointLight (JSON format)
fn spawn_json_point_light(
    commands: &mut Commands,
    entity: &JsonEntityData,
    name: &str,
    transform: Transform,
) {
    let brightness = entity.properties.get("Brightness")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;
    
    let range = entity.properties.get("Range")
        .and_then(|v| v.as_f64())
        .unwrap_or(16.0) as f32;
    
    let color = entity.properties.get("Color")
        .and_then(|v| v.as_array())
        .map(|arr| Color::srgb(
            arr.get(0).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        ))
        .unwrap_or(Color::WHITE);
    
    let shadows = entity.properties.get("Shadows")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    commands.spawn((
        PointLight {
            color,
            intensity: brightness * 1000.0,
            range,
            shadows_enabled: shadows,
            ..default()
        },
        transform,
        Name::new(name.to_string()),
    ));
    
    info!("  💡 PointLight: {} brightness={}", name, brightness);
}

/// Spawn a SpotLight (JSON format)
fn spawn_json_spot_light(
    commands: &mut Commands,
    entity: &JsonEntityData,
    name: &str,
    transform: Transform,
) {
    let brightness = entity.properties.get("Brightness")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;
    
    let range = entity.properties.get("Range")
        .and_then(|v| v.as_f64())
        .unwrap_or(16.0) as f32;
    
    let angle = entity.properties.get("Angle")
        .and_then(|v| v.as_f64())
        .unwrap_or(45.0) as f32;
    
    let color = entity.properties.get("Color")
        .and_then(|v| v.as_array())
        .map(|arr| Color::srgb(
            arr.get(0).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            arr.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            arr.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        ))
        .unwrap_or(Color::WHITE);
    
    commands.spawn((
        SpotLight {
            color,
            intensity: brightness * 1000.0,
            range,
            outer_angle: angle.to_radians(),
            inner_angle: (angle * 0.8).to_radians(),
            ..default()
        },
        transform,
        Name::new(name.to_string()),
    ));
    
    info!("  🔦 SpotLight: {} brightness={}", name, brightness);
}
