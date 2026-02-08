//! Scene Serialization - PropertyAccess-based JSON format
//! The primary and only save format

#![allow(dead_code)]

use bevy::prelude::*;
use bevy::pbr::decal::ForwardDecalMaterial;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::classes::*;
use crate::spawn::*;
use crate::soul::{SoulScriptData, SoulBuildStatus};
use eustress_common::{Attributes, AttributeValue, Tags, Parameters};
use eustress_common::classes::{
    Document, ImageAsset, VideoAsset, ScrollingFrame, ElasticBehavior, ScrollDirection, ScrollBarInset, BorderMode,
    VideoFrame, DocumentFrame, WebFrame, PageDisplayMode, Frame, ImageLabel, ImageButton, TextButton, ScaleType,
    TextXAlignment, TextYAlignment, ScreenGui, SurfaceGui, TextBox, ViewportFrame, ZIndexBehavior, ScreenInsets,
    NormalId, HorizontalAlignment, VerticalAlignment,
};

// ============================================================================
// Current Scene Tracking
// ============================================================================

/// Resource to track the currently loaded scene path
#[derive(Resource, Default, Clone)]
pub struct CurrentScenePath(pub Option<PathBuf>);

/// Scene file format (JSON-based, PropertyAccess-driven)
#[derive(Serialize, Deserialize, Debug)]
pub struct Scene {
    /// Format identifier
    pub format: String,
    
    /// Metadata about the scene
    pub metadata: SceneMetadata,
    
    /// All entities in the scene
    pub entities: Vec<EntityData>,
    
    /// Global data sources configuration (SoulService level)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_sources: Option<serde_json::Value>,
    
    /// Domain configurations (entity type templates)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_configs: Option<serde_json::Value>,
    
    /// Global variables for template substitution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_variables: Option<HashMap<String, String>>,
}

/// Scene metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SceneMetadata {
    pub name: String,
    pub description: String,
    pub author: String,
    pub created: String,
    pub modified: String,
    pub engine_version: String,
}

impl Default for SceneMetadata {
    fn default() -> Self {
        Self {
            name: "Untitled Scene".to_string(),
            description: String::new(),
            author: String::new(),
            created: chrono::Local::now().to_rfc3339(),
            modified: chrono::Local::now().to_rfc3339(),
            engine_version: "0.1.0".to_string(),
        }
    }
}

/// Entity data in scene file
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityData {
    /// Unique ID for this entity
    pub id: u32,
    
    /// Class name (Part, Model, Humanoid, etc.)
    pub class: String,
    
    /// Parent entity ID (None for root entities)
    pub parent: Option<u32>,
    
    /// All properties as key-value pairs
    pub properties: HashMap<String, serde_json::Value>,
    
    /// Child entity IDs
    pub children: Vec<u32>,
    
    /// Attributes (dynamic key-value pairs) - Roblox-style
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
    
    /// Tags (CollectionService-style) - for entity grouping/querying
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    
    /// Parameters (external data source configuration)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            format: "eustress_propertyaccess".to_string(),
            metadata: SceneMetadata::default(),
            entities: Vec::new(),
            global_sources: None,
            domain_configs: None,
            global_variables: None,
        }
    }
}

/// Convert AttributeValue to JSON Value
fn attribute_value_to_json(value: &AttributeValue) -> serde_json::Value {
    match value {
        AttributeValue::String(s) => serde_json::json!({"type": "String", "value": s}),
        AttributeValue::Number(n) => serde_json::json!({"type": "Number", "value": n}),
        AttributeValue::Bool(b) => serde_json::json!({"type": "Bool", "value": b}),
        AttributeValue::Int(i) => serde_json::json!({"type": "Int", "value": i}),
        AttributeValue::Vector3(v) => serde_json::json!({"type": "Vector3", "value": [v.x, v.y, v.z]}),
        AttributeValue::Vector2(v) => serde_json::json!({"type": "Vector2", "value": [v.x, v.y]}),
        AttributeValue::Color(c) => {
            let srgba = c.to_srgba();
            serde_json::json!({"type": "Color", "value": [srgba.red, srgba.green, srgba.blue, srgba.alpha]})
        }
        AttributeValue::CFrame(t) => {
            let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
            serde_json::json!({
                "type": "CFrame",
                "value": {
                    "position": [t.translation.x, t.translation.y, t.translation.z],
                    "rotation": [x.to_degrees(), y.to_degrees(), z.to_degrees()]
                }
            })
        }
        AttributeValue::BrickColor(bc) => serde_json::json!({"type": "BrickColor", "value": bc}),
        AttributeValue::EntityRef(e) => serde_json::json!({"type": "EntityRef", "value": e}),
        AttributeValue::Color3(c) => {
            let srgba = c.to_srgba();
            serde_json::json!({"type": "Color3", "value": [srgba.red, srgba.green, srgba.blue]})
        }
        AttributeValue::Object(e) => serde_json::json!({"type": "Object", "value": e}),
        AttributeValue::UDim2 { x_scale, x_offset, y_scale, y_offset } => {
            serde_json::json!({"type": "UDim2", "value": [x_scale, x_offset, y_scale, y_offset]})
        }
        AttributeValue::Rect { min, max } => {
            serde_json::json!({"type": "Rect", "value": {"min": [min.x, min.y], "max": [max.x, max.y]}})
        }
        AttributeValue::Font { family, weight, style } => {
            serde_json::json!({"type": "Font", "value": {"family": family, "weight": weight, "style": style}})
        }
        AttributeValue::NumberRange { min, max } => {
            serde_json::json!({"type": "NumberRange", "value": [min, max]})
        }
        AttributeValue::NumberSequence(keypoints) => {
            let kps: Vec<_> = keypoints.iter().map(|kp| {
                serde_json::json!({"time": kp.time, "value": kp.value, "envelope": kp.envelope})
            }).collect();
            serde_json::json!({"type": "NumberSequence", "value": kps})
        }
        AttributeValue::ColorSequence(keypoints) => {
            let kps: Vec<_> = keypoints.iter().map(|kp| {
                let c = kp.color.to_srgba();
                serde_json::json!({"time": kp.time, "color": [c.red, c.green, c.blue, c.alpha]})
            }).collect();
            serde_json::json!({"type": "ColorSequence", "value": kps})
        }
    }
}

/// Convert JSON Value to AttributeValue
fn json_to_attribute_value(json: &serde_json::Value) -> Option<AttributeValue> {
    let obj = json.as_object()?;
    let type_str = obj.get("type")?.as_str()?;
    let value = obj.get("value")?;
    
    match type_str {
        "String" => value.as_str().map(|s| AttributeValue::String(s.to_string())),
        "Number" => value.as_f64().map(AttributeValue::Number),
        "Bool" => value.as_bool().map(AttributeValue::Bool),
        "Int" => value.as_i64().map(AttributeValue::Int),
        "Vector3" => {
            let arr = value.as_array()?;
            Some(AttributeValue::Vector3(Vec3::new(
                arr.get(0)?.as_f64()? as f32,
                arr.get(1)?.as_f64()? as f32,
                arr.get(2)?.as_f64()? as f32,
            )))
        }
        "Vector2" => {
            let arr = value.as_array()?;
            Some(AttributeValue::Vector2(Vec2::new(
                arr.get(0)?.as_f64()? as f32,
                arr.get(1)?.as_f64()? as f32,
            )))
        }
        "Color" => {
            let arr = value.as_array()?;
            Some(AttributeValue::Color(Color::srgba(
                arr.get(0)?.as_f64()? as f32,
                arr.get(1)?.as_f64()? as f32,
                arr.get(2)?.as_f64()? as f32,
                arr.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            )))
        }
        "BrickColor" => value.as_u64().map(|bc| AttributeValue::BrickColor(bc as u32)),
        "NumberRange" => {
            let arr = value.as_array()?;
            Some(AttributeValue::NumberRange {
                min: arr.get(0)?.as_f64()?,
                max: arr.get(1)?.as_f64()?,
            })
        }
        _ => None, // Other types can be added as needed
    }
}

/// Convert PropertyValue to JSON Value
fn property_to_json(value: PropertyValue) -> serde_json::Value {
    match value {
        PropertyValue::String(s) => serde_json::json!(s),
        PropertyValue::Float(f) => serde_json::json!(f),
        PropertyValue::Int(i) => serde_json::json!(i),
        PropertyValue::Bool(b) => serde_json::json!(b),
        PropertyValue::Vector2(v) => serde_json::json!([v[0], v[1]]),
        PropertyValue::Vector3(v) => serde_json::json!([v.x, v.y, v.z]),
        PropertyValue::Color(c) => {
            let srgba = c.to_srgba();
            serde_json::json!([srgba.red, srgba.green, srgba.blue, srgba.alpha])
        }
        PropertyValue::Color3(c) => serde_json::json!([c[0], c[1], c[2]]),
        PropertyValue::Transform(t) => {
            let (x, y, z) = t.rotation.to_euler(EulerRot::XYZ);
            serde_json::json!({
                "position": [t.translation.x, t.translation.y, t.translation.z],
                "rotation": [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                "scale": [t.scale.x, t.scale.y, t.scale.z],
            })
        }
        PropertyValue::Material(m) => serde_json::json!(format!("{:?}", m)),
        PropertyValue::Enum(e) => serde_json::json!(e),
    }
}

/// Convert JSON Value to PropertyValue
fn json_to_property(json: &serde_json::Value, property_type: &str) -> Option<PropertyValue> {
    match property_type {
        "String" => json.as_str().map(|s| PropertyValue::String(s.to_string())),
        "Float" => json.as_f64().map(|f| PropertyValue::Float(f as f32)),
        "Int" => json.as_i64().map(|i| PropertyValue::Int(i as i32)),
        "Bool" => json.as_bool().map(PropertyValue::Bool),
        "Vector3" => {
            json.as_array().and_then(|arr| {
                if arr.len() >= 3 {
                    Some(PropertyValue::Vector3(Vec3::new(
                        arr[0].as_f64()? as f32,
                        arr[1].as_f64()? as f32,
                        arr[2].as_f64()? as f32,
                    )))
                } else {
                    None
                }
            })
        }
        "Color" => {
            json.as_array().and_then(|arr| {
                if arr.len() >= 3 {
                    Some(PropertyValue::Color(Color::srgba(
                        arr[0].as_f64()? as f32,
                        arr[1].as_f64()? as f32,
                        arr[2].as_f64()? as f32,
                        arr.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                    )))
                } else {
                    None
                }
            })
        }
        "Transform" => {
            json.as_object().and_then(|obj| {
                let pos = obj.get("position")?.as_array()?;
                let rot = obj.get("rotation")?.as_array()?;
                let scale = obj.get("scale")?.as_array()?;
                
                Some(PropertyValue::Transform(Transform {
                    translation: Vec3::new(
                        pos[0].as_f64()? as f32,
                        pos[1].as_f64()? as f32,
                        pos[2].as_f64()? as f32,
                    ),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        (rot[0].as_f64()? as f32).to_radians(),
                        (rot[1].as_f64()? as f32).to_radians(),
                        (rot[2].as_f64()? as f32).to_radians(),
                    ),
                    scale: Vec3::new(
                        scale[0].as_f64()? as f32,
                        scale[1].as_f64()? as f32,
                        scale[2].as_f64()? as f32,
                    ),
                }))
            })
        }
        "Enum" => json.as_str().map(|s| PropertyValue::Enum(s.to_string())),
        _ => None,
    }
}

/// Save a scene to JSON file using PropertyAccess
pub fn save_scene(
    world: &mut World,
    path: &Path,
    metadata: Option<SceneMetadata>,
) -> crate::serialization::Result<()> {
    use eustress_common::parameters::GlobalParametersRegistry;
    
    let mut scene = Scene {
        format: "eustress_propertyaccess".to_string(),
        metadata: metadata.unwrap_or_default(),
        entities: Vec::new(),
        global_sources: None,
        domain_configs: None,
        global_variables: None,
    };
    
    // Update modified time
    scene.metadata.modified = chrono::Local::now().to_rfc3339();
    
    // Serialize GlobalParametersRegistry if present
    if let Some(registry) = world.get_resource::<GlobalParametersRegistry>() {
        if !registry.sources.is_empty() {
            scene.global_sources = Some(serde_json::to_value(&registry.sources).unwrap_or_default());
        }
        if !registry.domains.is_empty() {
            scene.domain_configs = Some(serde_json::to_value(&registry.domains).unwrap_or_default());
        }
        if !registry.global_variables.is_empty() {
            // Convert serde_json::Value to String for serialization
            let string_vars: HashMap<String, String> = registry.global_variables
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect();
            scene.global_variables = Some(string_vars);
        }
    }
    
    // Query all entities with Instance component
    let mut query = world.query::<(Entity, &Instance)>();
    
    for (entity, instance) in query.iter(world) {
        let mut entity_data = EntityData {
            id: instance.id,
            class: instance.class_name.as_str().to_string(),
            parent: None, // TODO: Get from hierarchy
            properties: HashMap::new(),
            children: Vec::new(), // TODO: Get from hierarchy
            attributes: HashMap::new(),
            tags: Vec::new(),
            parameters: None,
        };
        
        // Collect properties based on class
        collect_entity_properties(world, entity, instance, &mut entity_data);
        
        scene.entities.push(entity_data);
    }
    
    // Write to file
    let json = serde_json::to_string_pretty(&scene)?;
    std::fs::write(path, json)?;
    
    Ok(())
}

/// Collect all properties for an entity
fn collect_entity_properties(
    world: &World,
    entity: Entity,
    instance: &Instance,
    entity_data: &mut EntityData,
) {
    // Always add Instance properties
    let props = instance.list_properties();
    if !props.is_empty() {
        for prop_desc in props {
            if let Some(value) = instance.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add BasePart properties if present
    if let Some(base_part) = world.get::<BasePart>(entity) {
        for prop_desc in base_part.list_properties() {
            if let Some(value) = base_part.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Part properties if present
    if let Some(part) = world.get::<Part>(entity) {
        for prop_desc in part.list_properties() {
            if let Some(value) = part.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add MeshPart properties if present
    if let Some(mesh_part) = world.get::<MeshPart>(entity) {
        for prop_desc in mesh_part.list_properties() {
            if let Some(value) = mesh_part.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Model properties if present
    if let Some(model) = world.get::<Model>(entity) {
        for prop_desc in model.list_properties() {
            if let Some(value) = model.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Humanoid properties if present
    if let Some(humanoid) = world.get::<Humanoid>(entity) {
        for prop_desc in humanoid.list_properties() {
            if let Some(value) = humanoid.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Camera properties if present
    if let Some(camera) = world.get::<EustressCamera>(entity) {
        for prop_desc in camera.list_properties() {
            if let Some(value) = camera.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add PointLight properties if present
    if let Some(light) = world.get::<EustressPointLight>(entity) {
        for prop_desc in light.list_properties() {
            if let Some(value) = light.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add SpotLight properties if present
    if let Some(light) = world.get::<EustressSpotLight>(entity) {
        for prop_desc in light.list_properties() {
            if let Some(value) = light.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add SurfaceLight properties if present
    if let Some(light) = world.get::<SurfaceLight>(entity) {
        for prop_desc in light.list_properties() {
            if let Some(value) = light.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Sound properties if present
    if let Some(sound) = world.get::<Sound>(entity) {
        for prop_desc in sound.list_properties() {
            if let Some(value) = sound.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Attachment properties if present
    if let Some(attachment) = world.get::<Attachment>(entity) {
        for prop_desc in attachment.list_properties() {
            if let Some(value) = attachment.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add WeldConstraint properties if present
    if let Some(weld) = world.get::<WeldConstraint>(entity) {
        for prop_desc in weld.list_properties() {
            if let Some(value) = weld.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Motor6D properties if present
    if let Some(motor) = world.get::<Motor6D>(entity) {
        for prop_desc in motor.list_properties() {
            if let Some(value) = motor.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add ParticleEmitter properties if present
    if let Some(emitter) = world.get::<ParticleEmitter>(entity) {
        for prop_desc in emitter.list_properties() {
            if let Some(value) = emitter.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Beam properties if present
    if let Some(beam) = world.get::<Beam>(entity) {
        for prop_desc in beam.list_properties() {
            if let Some(value) = beam.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add SpecialMesh properties if present
    if let Some(mesh) = world.get::<SpecialMesh>(entity) {
        for prop_desc in mesh.list_properties() {
            if let Some(value) = mesh.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Decal properties if present
    if let Some(decal) = world.get::<Decal>(entity) {
        for prop_desc in decal.list_properties() {
            if let Some(value) = decal.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Animator properties if present
    if let Some(animator) = world.get::<Animator>(entity) {
        for prop_desc in animator.list_properties() {
            if let Some(value) = animator.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add KeyframeSequence properties if present
    if let Some(keyframe) = world.get::<KeyframeSequence>(entity) {
        for prop_desc in keyframe.list_properties() {
            if let Some(value) = keyframe.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Terrain properties if present
    if let Some(terrain) = world.get::<Terrain>(entity) {
        for prop_desc in terrain.list_properties() {
            if let Some(value) = terrain.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Sky properties if present
    if let Some(sky) = world.get::<Sky>(entity) {
        for prop_desc in sky.list_properties() {
            if let Some(value) = sky.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add UnionOperation properties if present
    if let Some(union) = world.get::<UnionOperation>(entity) {
        for prop_desc in union.list_properties() {
            if let Some(value) = union.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add BillboardGui properties if present
    if let Some(billboard_gui) = world.get::<BillboardGui>(entity) {
        for prop_desc in billboard_gui.list_properties() {
            if let Some(value) = billboard_gui.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add TextLabel properties if present
    if let Some(text_label) = world.get::<TextLabel>(entity) {
        for prop_desc in text_label.list_properties() {
            if let Some(value) = text_label.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add Folder properties if present
    if let Some(folder) = world.get::<Folder>(entity) {
        for prop_desc in folder.list_properties() {
            if let Some(value) = folder.get_property(&prop_desc.name) {
                entity_data.properties.insert(
                    prop_desc.name.clone(),
                    property_to_json(value),
                );
            }
        }
    }
    
    // Add SoulScriptData properties if present
    if let Some(soul_script) = world.get::<SoulScriptData>(entity) {
        // Store the source code
        entity_data.properties.insert(
            "Source".to_string(),
            serde_json::json!(soul_script.source),
        );
        
        // Store generated code if present
        if let Some(ref generated) = soul_script.generated_code {
            entity_data.properties.insert(
                "GeneratedCode".to_string(),
                serde_json::json!(generated),
            );
        }
        
        // Store build status
        entity_data.properties.insert(
            "BuildStatus".to_string(),
            serde_json::json!(format!("{:?}", soul_script.build_status)),
        );
        
        // Store errors if any
        if !soul_script.errors.is_empty() {
            entity_data.properties.insert(
                "Errors".to_string(),
                serde_json::json!(soul_script.errors),
            );
        }
    }
    
    // =========================================================================
    // Attributes, Tags, and Parameters (new in Phase 3)
    // =========================================================================
    
    // Add Attributes if present
    if let Some(attributes) = world.get::<Attributes>(entity) {
        for (key, value) in attributes.iter() {
            entity_data.attributes.insert(key.clone(), attribute_value_to_json(value));
        }
    }
    
    // Add Tags if present
    if let Some(tags) = world.get::<Tags>(entity) {
        entity_data.tags = tags.iter().cloned().collect();
    }
    
    // Add Parameters if present
    if let Some(parameters) = world.get::<Parameters>(entity) {
        entity_data.parameters = Some(serde_json::to_value(parameters).unwrap_or_default());
    }
}

/// Load a scene from JSON file
pub fn load_scene(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    decal_materials: &mut ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    asset_server: &Res<AssetServer>,
    path: &Path,
) -> crate::serialization::Result<Scene> {
    use eustress_common::parameters::{GlobalParametersRegistry, GlobalDataSource, DomainConfig};
    
    // Read file
    let json = std::fs::read_to_string(path)?;
    
    // Parse JSON
    let scene: Scene = serde_json::from_str(&json)?;
    
    // Verify format
    if scene.format != "eustress_propertyaccess" {
        return Err(crate::serialization::SerializationError::InvalidFormat(
            format!("Expected 'eustress_propertyaccess', got '{}'", scene.format)
        ));
    }
    
    // Restore GlobalParametersRegistry from scene
    let mut registry = GlobalParametersRegistry::new();
    
    if let Some(ref sources_json) = scene.global_sources {
        if let Ok(sources) = serde_json::from_value::<Vec<GlobalDataSource>>(sources_json.clone()) {
            registry.sources = sources;
        }
    }
    
    if let Some(ref domains_json) = scene.domain_configs {
        if let Ok(domains) = serde_json::from_value::<Vec<DomainConfig>>(domains_json.clone()) {
            registry.domains = domains;
        }
    }
    
    if let Some(ref vars) = scene.global_variables {
        // Convert HashMap<String, String> to HashMap<String, serde_json::Value>
        registry.global_variables = vars
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
    }
    
    // Insert/update the registry resource
    commands.insert_resource(registry);
    
    // Track spawned entities for hierarchy restoration
    let mut entity_map: HashMap<u32, Entity> = HashMap::new();
    
    // First pass: Spawn all entities
    for entity_data in &scene.entities {
        let entity = spawn_entity_from_data(
            commands,
            meshes,
            materials,
            decal_materials,
            asset_server,
            entity_data,
        )?;
        
        entity_map.insert(entity_data.id, entity);
    }
    
    // Second pass: Restore hierarchy
    for entity_data in &scene.entities {
        if let Some(parent_id) = entity_data.parent {
            if let (Some(&entity), Some(&parent_entity)) = (
                entity_map.get(&entity_data.id),
                entity_map.get(&parent_id),
            ) {
                commands.entity(parent_entity).add_child(entity);
            }
        }
    }
    
    Ok(scene)
}

/// Spawn an entity from EntityData
fn spawn_entity_from_data(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    decal_materials: &mut ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    asset_server: &Res<AssetServer>,
    data: &EntityData,
) -> crate::serialization::Result<Entity> {
    // Parse class name
    let class_name = ClassName::from_str(&data.class)
        .map_err(|_| crate::serialization::SerializationError::InvalidClass(
            data.class.clone()
        ))?;
    
    // Create Instance
    let instance = Instance {
        id: data.id,
        name: data.properties.get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unnamed")
            .to_string(),
        class_name,
        archivable: data.properties.get("Archivable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        ..Default::default()
    };
    
    // Spawn based on class using spawn helpers
    let entity = match class_name {
        ClassName::Part => {
            let base_part = basepart_from_properties(&data.properties);
            let part = part_from_properties(&data.properties);
            spawn_part(commands, meshes, materials, instance, base_part, part)
        }
        ClassName::MeshPart => {
            let base_part = basepart_from_properties(&data.properties);
            let mesh_part = meshpart_from_properties(&data.properties);
            spawn_mesh_part(commands, meshes, asset_server, materials, instance, base_part, mesh_part)
        }
        ClassName::Model => {
            let model = model_from_properties(&data.properties);
            spawn_model(commands, instance, model)
        }
        ClassName::Humanoid => {
            let humanoid = humanoid_from_properties(&data.properties);
            spawn_humanoid(commands, instance, humanoid, Entity::PLACEHOLDER)
        }
        ClassName::Camera => {
            let camera = camera_from_properties(&data.properties);
            let transform = transform_from_properties(&data.properties);
            spawn_camera(commands, instance, camera, transform)
        }
        ClassName::PointLight => {
            let light = pointlight_from_properties(&data.properties);
            let transform = transform_from_properties(&data.properties);
            spawn_point_light(commands, instance, light, transform)
        }
        ClassName::SpotLight => {
            let light = spotlight_from_properties(&data.properties);
            let transform = transform_from_properties(&data.properties);
            spawn_spot_light(commands, instance, light, transform)
        }
        ClassName::SurfaceLight => {
            let light = surfacelight_from_properties(&data.properties);
            spawn_surface_light(commands, instance, light, Entity::PLACEHOLDER)
        }
        ClassName::Sound => {
            let sound = sound_from_properties(&data.properties);
            spawn_sound(commands, asset_server, instance, sound, Entity::PLACEHOLDER)
        }
        ClassName::Attachment => {
            let attachment = attachment_from_properties(&data.properties);
            spawn_attachment(commands, instance, attachment, Entity::PLACEHOLDER)
        }
        ClassName::WeldConstraint => {
            let weld = weldconstraint_from_properties(&data.properties);
            spawn_weld_constraint(commands, instance, weld)
        }
        ClassName::Motor6D => {
            let motor = motor6d_from_properties(&data.properties);
            spawn_motor6d(commands, instance, motor)
        }
        ClassName::ParticleEmitter => {
            let emitter = particleemitter_from_properties(&data.properties);
            spawn_particle_emitter(commands, instance, emitter, Entity::PLACEHOLDER)
        }
        ClassName::Beam => {
            let beam = beam_from_properties(&data.properties);
            spawn_beam(commands, instance, beam)
        }
        ClassName::SpecialMesh => {
            let mesh = specialmesh_from_properties(&data.properties);
            spawn_special_mesh(commands, instance, mesh, Entity::PLACEHOLDER)
        }
        ClassName::Decal => {
            let decal = decal_from_properties(&data.properties);
            spawn_decal(commands, asset_server, decal_materials, instance, decal, Transform::default())
        }
        ClassName::Animator => {
            let animator = animator_from_properties(&data.properties);
            spawn_animator(commands, instance, animator, Entity::PLACEHOLDER)
        }
        ClassName::KeyframeSequence => {
            let keyframe = keyframesequence_from_properties(&data.properties);
            spawn_keyframe_sequence(commands, instance, keyframe)
        }
        ClassName::Terrain => {
            let terrain = terrain_from_properties(&data.properties);
            spawn_terrain(commands, instance, terrain)
        }
        ClassName::Sky => {
            let sky = sky_from_properties(&data.properties);
            spawn_sky(commands, instance, sky)
        }
        ClassName::UnionOperation => {
            let base_part = basepart_from_properties(&data.properties);
            let union = unionoperation_from_properties(&data.properties);
            spawn_union(commands, meshes, materials, instance, base_part, union)
        }
        ClassName::BillboardGui => {
            let billboard_gui = billboardgui_from_properties(&data.properties);
            spawn_billboard_gui(commands, instance, billboard_gui)
        }
        ClassName::TextLabel => {
            let text_label = textlabel_from_properties(&data.properties);
            spawn_text_label(commands, instance, text_label)
        }
        ClassName::Folder => {
            let folder = folder_from_properties(&data.properties);
            spawn_folder_with_config(commands, instance, folder)
        }
        ClassName::SoulScript => {
            let soul_script = soulscript_from_properties(&data.properties);
            spawn_soul_script(commands, instance, soul_script)
        }
        ClassName::Document => {
            let document = document_from_properties(&data.properties);
            spawn_document(commands, instance, document)
        }
        ClassName::ImageAsset => {
            let image = image_asset_from_properties(&data.properties);
            spawn_image_asset(commands, instance, image)
        }
        ClassName::VideoAsset => {
            let video = video_asset_from_properties(&data.properties);
            spawn_video_asset(commands, instance, video)
        }
        ClassName::ScrollingFrame => {
            let scrolling_frame = scrollingframe_from_properties(&data.properties);
            spawn_scrolling_frame(commands, instance, scrolling_frame)
        }
        ClassName::VideoFrame => {
            let video_frame = videoframe_from_properties(&data.properties);
            spawn_video_frame(commands, instance, video_frame)
        }
        ClassName::DocumentFrame => {
            let document_frame = documentframe_from_properties(&data.properties);
            spawn_document_frame(commands, instance, document_frame)
        }
        ClassName::WebFrame => {
            let web_frame = webframe_from_properties(&data.properties);
            spawn_web_frame(commands, instance, web_frame)
        }
        ClassName::Frame => {
            let frame = frame_from_properties(&data.properties);
            spawn_frame(commands, instance, frame)
        }
        ClassName::ImageLabel => {
            let image_label = imagelabel_from_properties(&data.properties);
            spawn_image_label(commands, instance, image_label)
        }
        ClassName::ImageButton => {
            let image_button = imagebutton_from_properties(&data.properties);
            spawn_image_button(commands, instance, image_button)
        }
        ClassName::TextButton => {
            let text_button = textbutton_from_properties(&data.properties);
            spawn_text_button(commands, instance, text_button)
        }
        ClassName::ScreenGui => {
            let screen_gui = screengui_from_properties(&data.properties);
            spawn_screen_gui(commands, instance, screen_gui)
        }
        ClassName::SurfaceGui => {
            let surface_gui = surfacegui_from_properties(&data.properties);
            spawn_surface_gui(commands, instance, surface_gui)
        }
        ClassName::TextBox => {
            let text_box = textbox_from_properties(&data.properties);
            spawn_text_box(commands, instance, text_box)
        }
        ClassName::ViewportFrame => {
            let viewport_frame = viewportframe_from_properties(&data.properties);
            spawn_viewport_frame(commands, instance, viewport_frame)
        }
        // Fallback for base classes
        _ => {
            let name = instance.name.clone();
            commands.spawn((instance, Name::new(name))).id()
        }
    };
    
    // =========================================================================
    // Restore Attributes, Tags, and Parameters (Phase 3)
    // =========================================================================
    
    // Restore Attributes if present
    if !data.attributes.is_empty() {
        let mut attributes = Attributes::new();
        for (key, json_value) in &data.attributes {
            if let Some(attr_value) = json_to_attribute_value(json_value) {
                attributes.set(key, attr_value);
            }
        }
        commands.entity(entity).insert(attributes);
    }
    
    // Restore Tags if present
    if !data.tags.is_empty() {
        let mut tags = Tags::new();
        for tag in &data.tags {
            tags.add(tag);
        }
        commands.entity(entity).insert(tags);
    }
    
    // Restore Parameters if present
    if let Some(ref params_json) = data.parameters {
        if let Ok(parameters) = serde_json::from_value::<Parameters>(params_json.clone()) {
            commands.entity(entity).insert(parameters);
        }
    }
    
    Ok(entity)
}

/// Reconstruct BasePart from properties
fn basepart_from_properties(props: &HashMap<String, serde_json::Value>) -> BasePart {
    let mut base_part = BasePart::default();
    
    // Size
    if let Some(size_json) = props.get("Size") {
        if let Some(size) = json_to_property(size_json, "Vector3") {
            let _ = base_part.set_property("Size", size);
        }
    }
    
    // Color
    if let Some(color_json) = props.get("Color") {
        if let Some(color) = json_to_property(color_json, "Color") {
            let _ = base_part.set_property("Color", color);
        }
    }
    
    // Material
    if let Some(mat_json) = props.get("Material") {
        if let Some(mat) = json_to_property(mat_json, "Enum") {
            let _ = base_part.set_property("Material", mat);
        }
    }
    
    // TODO: Add more properties
    
    base_part
}

/// Reconstruct Part from properties
fn part_from_properties(props: &HashMap<String, serde_json::Value>) -> Part {
    let mut part = Part::default();
    
    if let Some(shape_json) = props.get("Shape") {
        if let Some(shape) = json_to_property(shape_json, "Enum") {
            let _ = part.set_property("Shape", shape);
        }
    }
    
    part
}

/// Reconstruct Model from properties
fn model_from_properties(props: &HashMap<String, serde_json::Value>) -> Model {
    let mut model = Model::default();
    
    if let Some(primary_json) = props.get("PrimaryPart") {
        if let Some(primary) = json_to_property(primary_json, "Int") {
            let _ = model.set_property("PrimaryPart", primary);
        }
    }
    
    model
}

/// Helper: Reconstruct Transform from properties
fn transform_from_properties(props: &HashMap<String, serde_json::Value>) -> Transform {
    if let Some(cframe_json) = props.get("CFrame") {
        if let Some(PropertyValue::Transform(t)) = json_to_property(cframe_json, "Transform") {
            return t;
        }
    }
    Transform::default()
}

/// Helper: Extract size from properties
fn size_from_properties(props: &HashMap<String, serde_json::Value>) -> Vec3 {
    if let Some(size_json) = props.get("Size") {
        if let Some(PropertyValue::Vector3(v)) = json_to_property(size_json, "Vector3") {
            return v;
        }
    }
    Vec3::ONE
}

// Reconstruction functions for all remaining classes
fn meshpart_from_properties(props: &HashMap<String, serde_json::Value>) -> MeshPart {
    let mut mesh_part = MeshPart::default();
    
    if let Some(mesh_id) = props.get("MeshId").and_then(|v| v.as_str()) {
        let _ = mesh_part.set_property("MeshId", PropertyValue::String(mesh_id.to_string()));
    }
    
    if let Some(texture_id) = props.get("TextureID").and_then(|v| v.as_str()) {
        let _ = mesh_part.set_property("TextureID", PropertyValue::String(texture_id.to_string()));
    }
    
    mesh_part
}

fn humanoid_from_properties(props: &HashMap<String, serde_json::Value>) -> Humanoid {
    let mut humanoid = Humanoid::default();
    
    if let Some(health) = props.get("Health").and_then(|v| v.as_f64()) {
        let _ = humanoid.set_property("Health", PropertyValue::Float(health as f32));
    }
    
    if let Some(max_health) = props.get("MaxHealth").and_then(|v| v.as_f64()) {
        let _ = humanoid.set_property("MaxHealth", PropertyValue::Float(max_health as f32));
    }
    
    if let Some(walk_speed) = props.get("WalkSpeed").and_then(|v| v.as_f64()) {
        let _ = humanoid.set_property("WalkSpeed", PropertyValue::Float(walk_speed as f32));
    }
    
    if let Some(jump_power) = props.get("JumpPower").and_then(|v| v.as_f64()) {
        let _ = humanoid.set_property("JumpPower", PropertyValue::Float(jump_power as f32));
    }
    
    if let Some(rig_type) = props.get("RigType").and_then(|v| v.as_str()) {
        let _ = humanoid.set_property("RigType", PropertyValue::Enum(rig_type.to_string()));
    }
    
    humanoid
}

fn camera_from_properties(props: &HashMap<String, serde_json::Value>) -> EustressCamera {
    let mut camera = EustressCamera::default();
    
    if let Some(fov) = props.get("FieldOfView").and_then(|v| v.as_f64()) {
        let _ = camera.set_property("FieldOfView", PropertyValue::Float(fov as f32));
    }
    
    if let Some(camera_type) = props.get("CameraType").and_then(|v| v.as_str()) {
        let _ = camera.set_property("CameraType", PropertyValue::String(camera_type.to_string()));
    }
    
    camera
}

fn pointlight_from_properties(props: &HashMap<String, serde_json::Value>) -> EustressPointLight {
    let mut light = EustressPointLight::default();
    
    if let Some(brightness) = props.get("Brightness").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Brightness", PropertyValue::Float(brightness as f32));
    }
    
    if let Some(range) = props.get("Range").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Range", PropertyValue::Float(range as f32));
    }
    
    if let Some(color_json) = props.get("Color") {
        if let Some(color) = json_to_property(color_json, "Color") {
            let _ = light.set_property("Color", color);
        }
    }
    
    if let Some(shadows) = props.get("Shadows").and_then(|v| v.as_bool()) {
        let _ = light.set_property("Shadows", PropertyValue::Bool(shadows));
    }
    
    light
}

fn spotlight_from_properties(props: &HashMap<String, serde_json::Value>) -> EustressSpotLight {
    let mut light = EustressSpotLight::default();
    
    if let Some(brightness) = props.get("Brightness").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Brightness", PropertyValue::Float(brightness as f32));
    }
    
    if let Some(range) = props.get("Range").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Range", PropertyValue::Float(range as f32));
    }
    
    if let Some(angle) = props.get("Angle").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Angle", PropertyValue::Float(angle as f32));
    }
    
    if let Some(color_json) = props.get("Color") {
        if let Some(color) = json_to_property(color_json, "Color") {
            let _ = light.set_property("Color", color);
        }
    }
    
    light
}

fn surfacelight_from_properties(props: &HashMap<String, serde_json::Value>) -> SurfaceLight {
    let mut light = SurfaceLight::default();
    
    if let Some(brightness) = props.get("Brightness").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Brightness", PropertyValue::Float(brightness as f32));
    }
    
    if let Some(range) = props.get("Range").and_then(|v| v.as_f64()) {
        let _ = light.set_property("Range", PropertyValue::Float(range as f32));
    }
    
    if let Some(face) = props.get("Face").and_then(|v| v.as_str()) {
        let _ = light.set_property("Face", PropertyValue::Enum(face.to_string()));
    }
    
    if let Some(color_json) = props.get("Color") {
        if let Some(color) = json_to_property(color_json, "Color") {
            let _ = light.set_property("Color", color);
        }
    }
    
    light
}

fn sound_from_properties(props: &HashMap<String, serde_json::Value>) -> Sound {
    let mut sound = Sound::default();
    
    if let Some(sound_id) = props.get("SoundId").and_then(|v| v.as_str()) {
        let _ = sound.set_property("SoundId", PropertyValue::String(sound_id.to_string()));
    }
    
    if let Some(volume) = props.get("Volume").and_then(|v| v.as_f64()) {
        let _ = sound.set_property("Volume", PropertyValue::Float(volume as f32));
    }
    
    if let Some(pitch) = props.get("Pitch").and_then(|v| v.as_f64()) {
        let _ = sound.set_property("Pitch", PropertyValue::Float(pitch as f32));
    }
    
    if let Some(looped) = props.get("Looped").and_then(|v| v.as_bool()) {
        let _ = sound.set_property("Looped", PropertyValue::Bool(looped));
    }
    
    if let Some(playing) = props.get("Playing").and_then(|v| v.as_bool()) {
        let _ = sound.set_property("Playing", PropertyValue::Bool(playing));
    }
    
    sound
}

fn attachment_from_properties(props: &HashMap<String, serde_json::Value>) -> Attachment {
    let mut attachment = Attachment::default();
    
    if let Some(cframe_json) = props.get("CFrame") {
        if let Some(cframe) = json_to_property(cframe_json, "Transform") {
            let _ = attachment.set_property("CFrame", cframe);
        }
    }
    
    if let Some(visible) = props.get("Visible").and_then(|v| v.as_bool()) {
        let _ = attachment.set_property("Visible", PropertyValue::Bool(visible));
    }
    
    attachment
}

fn weldconstraint_from_properties(props: &HashMap<String, serde_json::Value>) -> WeldConstraint {
    let mut weld = WeldConstraint::default();
    
    if let Some(part0) = props.get("Part0").and_then(|v| v.as_i64()) {
        let _ = weld.set_property("Part0", PropertyValue::Int(part0 as i32));
    }
    
    if let Some(part1) = props.get("Part1").and_then(|v| v.as_i64()) {
        let _ = weld.set_property("Part1", PropertyValue::Int(part1 as i32));
    }
    
    if let Some(enabled) = props.get("Enabled").and_then(|v| v.as_bool()) {
        let _ = weld.set_property("Enabled", PropertyValue::Bool(enabled));
    }
    
    weld
}

fn motor6d_from_properties(props: &HashMap<String, serde_json::Value>) -> Motor6D {
    let mut motor = Motor6D::default();
    
    if let Some(c0_json) = props.get("C0") {
        if let Some(c0) = json_to_property(c0_json, "Transform") {
            let _ = motor.set_property("C0", c0);
        }
    }
    
    if let Some(c1_json) = props.get("C1") {
        if let Some(c1) = json_to_property(c1_json, "Transform") {
            let _ = motor.set_property("C1", c1);
        }
    }
    
    if let Some(part0) = props.get("Part0").and_then(|v| v.as_i64()) {
        let _ = motor.set_property("Part0", PropertyValue::Int(part0 as i32));
    }
    
    if let Some(part1) = props.get("Part1").and_then(|v| v.as_i64()) {
        let _ = motor.set_property("Part1", PropertyValue::Int(part1 as i32));
    }
    
    motor
}

fn particleemitter_from_properties(props: &HashMap<String, serde_json::Value>) -> ParticleEmitter {
    let mut emitter = ParticleEmitter::default();
    
    if let Some(rate) = props.get("Rate").and_then(|v| v.as_f64()) {
        let _ = emitter.set_property("Rate", PropertyValue::Float(rate as f32));
    }
    
    if let Some(lifetime_min) = props.get("LifetimeMin").and_then(|v| v.as_f64()) {
        let _ = emitter.set_property("LifetimeMin", PropertyValue::Float(lifetime_min as f32));
    }
    
    if let Some(lifetime_max) = props.get("LifetimeMax").and_then(|v| v.as_f64()) {
        let _ = emitter.set_property("LifetimeMax", PropertyValue::Float(lifetime_max as f32));
    }
    
    if let Some(speed) = props.get("Speed").and_then(|v| v.as_f64()) {
        let _ = emitter.set_property("Speed", PropertyValue::Float(speed as f32));
    }
    
    if let Some(enabled) = props.get("Enabled").and_then(|v| v.as_bool()) {
        let _ = emitter.set_property("Enabled", PropertyValue::Bool(enabled));
    }
    
    emitter
}

fn beam_from_properties(props: &HashMap<String, serde_json::Value>) -> Beam {
    let mut beam = Beam::default();
    
    if let Some(attachment0) = props.get("Attachment0").and_then(|v| v.as_i64()) {
        let _ = beam.set_property("Attachment0", PropertyValue::Int(attachment0 as i32));
    }
    
    if let Some(attachment1) = props.get("Attachment1").and_then(|v| v.as_i64()) {
        let _ = beam.set_property("Attachment1", PropertyValue::Int(attachment1 as i32));
    }
    
    if let Some(width0) = props.get("Width0").and_then(|v| v.as_f64()) {
        let _ = beam.set_property("Width0", PropertyValue::Float(width0 as f32));
    }
    
    if let Some(width1) = props.get("Width1").and_then(|v| v.as_f64()) {
        let _ = beam.set_property("Width1", PropertyValue::Float(width1 as f32));
    }
    
    if let Some(enabled) = props.get("Enabled").and_then(|v| v.as_bool()) {
        let _ = beam.set_property("Enabled", PropertyValue::Bool(enabled));
    }
    
    beam
}

fn specialmesh_from_properties(props: &HashMap<String, serde_json::Value>) -> SpecialMesh {
    let mut mesh = SpecialMesh::default();
    
    if let Some(mesh_type) = props.get("MeshType").and_then(|v| v.as_str()) {
        let _ = mesh.set_property("MeshType", PropertyValue::Enum(mesh_type.to_string()));
    }
    
    if let Some(scale_json) = props.get("Scale") {
        if let Some(scale) = json_to_property(scale_json, "Vector3") {
            let _ = mesh.set_property("Scale", scale);
        }
    }
    
    if let Some(offset_json) = props.get("Offset") {
        if let Some(offset) = json_to_property(offset_json, "Vector3") {
            let _ = mesh.set_property("Offset", offset);
        }
    }
    
    mesh
}

fn decal_from_properties(props: &HashMap<String, serde_json::Value>) -> Decal {
    let mut decal = Decal::default();
    
    if let Some(texture) = props.get("Texture").and_then(|v| v.as_str()) {
        let _ = decal.set_property("Texture", PropertyValue::String(texture.to_string()));
    }
    
    if let Some(face) = props.get("Face").and_then(|v| v.as_str()) {
        let _ = decal.set_property("Face", PropertyValue::Enum(face.to_string()));
    }
    
    if let Some(transparency) = props.get("Transparency").and_then(|v| v.as_f64()) {
        let _ = decal.set_property("Transparency", PropertyValue::Float(transparency as f32));
    }
    
    // New Bevy 0.16+ ForwardDecal properties
    if let Some(depth_fade) = props.get("DepthFadeFactor").and_then(|v| v.as_f64()) {
        decal.depth_fade_factor = depth_fade as f32;
    }
    
    if let Some(color) = props.get("Color").and_then(|v| v.as_array()) {
        if color.len() >= 3 {
            decal.color = [
                color[0].as_f64().unwrap_or(1.0) as f32,
                color[1].as_f64().unwrap_or(1.0) as f32,
                color[2].as_f64().unwrap_or(1.0) as f32,
                color.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            ];
        }
    }
    
    decal
}

fn animator_from_properties(_props: &HashMap<String, serde_json::Value>) -> Animator {
    // Animator has no properties beyond Instance
    Animator::default()
}

fn keyframesequence_from_properties(props: &HashMap<String, serde_json::Value>) -> KeyframeSequence {
    let mut keyframe = KeyframeSequence::default();
    
    if let Some(priority) = props.get("Priority").and_then(|v| v.as_str()) {
        let _ = keyframe.set_property("Priority", PropertyValue::Enum(priority.to_string()));
    }
    
    if let Some(looped) = props.get("Loop").and_then(|v| v.as_bool()) {
        let _ = keyframe.set_property("Loop", PropertyValue::Bool(looped));
    }
    
    keyframe
}

#[allow(dead_code)]
fn terrain_from_properties(props: &HashMap<String, serde_json::Value>) -> Terrain {
    let mut terrain = Terrain::default();
    
    if let Some(water_wave_size) = props.get("WaterWaveSize").and_then(|v| v.as_f64()) {
        let _ = terrain.set_property("WaterWaveSize", PropertyValue::Float(water_wave_size as f32));
    }
    
    if let Some(water_transparency) = props.get("WaterTransparency").and_then(|v| v.as_f64()) {
        let _ = terrain.set_property("WaterTransparency", PropertyValue::Float(water_transparency as f32));
    }
    
    if let Some(water_color_json) = props.get("WaterColor") {
        if let Some(water_color) = json_to_property(water_color_json, "Color") {
            let _ = terrain.set_property("WaterColor", water_color);
        }
    }
    
    terrain
}

fn sky_from_properties(props: &HashMap<String, serde_json::Value>) -> Sky {
    let mut sky = Sky::default();
    
    if let Some(skybox_up) = props.get("SkyboxUp").and_then(|v| v.as_str()) {
        let _ = sky.set_property("SkyboxUp", PropertyValue::String(skybox_up.to_string()));
    }
    
    if let Some(skybox_dn) = props.get("SkyboxDn").and_then(|v| v.as_str()) {
        let _ = sky.set_property("SkyboxDn", PropertyValue::String(skybox_dn.to_string()));
    }
    
    if let Some(sun_angular_size) = props.get("SunAngularSize").and_then(|v| v.as_f64()) {
        let _ = sky.set_property("SunAngularSize", PropertyValue::Float(sun_angular_size as f32));
    }
    
    sky
}

fn unionoperation_from_properties(props: &HashMap<String, serde_json::Value>) -> UnionOperation {
    let mut union = UnionOperation::default();
    
    if let Some(operation) = props.get("Operation").and_then(|v| v.as_str()) {
        let _ = union.set_property("Operation", PropertyValue::Enum(operation.to_string()));
    }
    
    union
}

fn billboardgui_from_properties(props: &HashMap<String, serde_json::Value>) -> BillboardGui {
    let mut billboard_gui = BillboardGui::default();
    
    // Behavior
    if let Some(active) = props.get("Active").and_then(|v| v.as_bool()) {
        let _ = billboard_gui.set_property("Active", PropertyValue::Bool(active));
    }
    if let Some(always_on_top) = props.get("AlwaysOnTop").and_then(|v| v.as_bool()) {
        let _ = billboard_gui.set_property("AlwaysOnTop", PropertyValue::Bool(always_on_top));
    }
    if let Some(enabled) = props.get("Enabled").and_then(|v| v.as_bool()) {
        let _ = billboard_gui.set_property("Enabled", PropertyValue::Bool(enabled));
    }
    
    // Distance
    if let Some(max_distance) = props.get("MaxDistance").and_then(|v| v.as_f64()) {
        let _ = billboard_gui.set_property("MaxDistance", PropertyValue::Float(max_distance as f32));
    }
    
    // Appearance
    if let Some(brightness) = props.get("Brightness").and_then(|v| v.as_f64()) {
        let _ = billboard_gui.set_property("Brightness", PropertyValue::Float(brightness as f32));
    }
    if let Some(light_influence) = props.get("LightInfluence").and_then(|v| v.as_f64()) {
        let _ = billboard_gui.set_property("LightInfluence", PropertyValue::Float(light_influence as f32));
    }
    
    // Size/Position
    if let Some(size_json) = props.get("Size") {
        if let Some(size) = json_to_property(size_json, "Vector3") {
            let _ = billboard_gui.set_property("Size", size);
        }
    }
    if let Some(units_offset_json) = props.get("UnitsOffset") {
        if let Some(units_offset) = json_to_property(units_offset_json, "Vector3") {
            let _ = billboard_gui.set_property("UnitsOffset", units_offset);
        }
    }
    
    billboard_gui
}

fn textlabel_from_properties(props: &HashMap<String, serde_json::Value>) -> TextLabel {
    let mut text_label = TextLabel::default();
    
    // Text Content
    if let Some(text) = props.get("Text").and_then(|v| v.as_str()) {
        let _ = text_label.set_property("Text", PropertyValue::String(text.to_string()));
    }
    if let Some(rich_text) = props.get("RichText").and_then(|v| v.as_bool()) {
        let _ = text_label.set_property("RichText", PropertyValue::Bool(rich_text));
    }
    if let Some(text_scaled) = props.get("TextScaled").and_then(|v| v.as_bool()) {
        let _ = text_label.set_property("TextScaled", PropertyValue::Bool(text_scaled));
    }
    if let Some(text_wrapped) = props.get("TextWrapped").and_then(|v| v.as_bool()) {
        let _ = text_label.set_property("TextWrapped", PropertyValue::Bool(text_wrapped));
    }
    
    // Font
    if let Some(font_size) = props.get("FontSize").and_then(|v| v.as_f64()) {
        let _ = text_label.set_property("FontSize", PropertyValue::Float(font_size as f32));
    }
    if let Some(line_height) = props.get("LineHeight").and_then(|v| v.as_f64()) {
        let _ = text_label.set_property("LineHeight", PropertyValue::Float(line_height as f32));
    }
    
    // Colors
    if let Some(text_color_json) = props.get("TextColor3") {
        if let Some(text_color) = json_to_property(text_color_json, "Color") {
            let _ = text_label.set_property("TextColor3", text_color);
        }
    }
    if let Some(text_transparency) = props.get("TextTransparency").and_then(|v| v.as_f64()) {
        let _ = text_label.set_property("TextTransparency", PropertyValue::Float(text_transparency as f32));
    }
    if let Some(bg_color_json) = props.get("BackgroundColor3") {
        if let Some(bg_color) = json_to_property(bg_color_json, "Color") {
            let _ = text_label.set_property("BackgroundColor3", bg_color);
        }
    }
    if let Some(bg_transparency) = props.get("BackgroundTransparency").and_then(|v| v.as_f64()) {
        let _ = text_label.set_property("BackgroundTransparency", PropertyValue::Float(bg_transparency as f32));
    }
    
    // Layout
    if let Some(size_json) = props.get("Size") {
        if let Some(size) = json_to_property(size_json, "Vector3") {
            let _ = text_label.set_property("Size", size);
        }
    }
    if let Some(visible) = props.get("Visible").and_then(|v| v.as_bool()) {
        let _ = text_label.set_property("Visible", PropertyValue::Bool(visible));
    }
    
    text_label
}

fn folder_from_properties(_props: &HashMap<String, serde_json::Value>) -> Folder {
    // Folder now has assembly_mass - domain scope is handled by Parameters component
    Folder::default()
}

fn document_from_properties(props: &HashMap<String, serde_json::Value>) -> Document {
    use eustress_common::classes::{Document, DocumentType, AssetSourceType};
    
    let document_type = props.get("document_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "PDF" => DocumentType::PDF,
            "DOCX" => DocumentType::DOCX,
            "PPTX" => DocumentType::PPTX,
            "XLSX" => DocumentType::XLSX,
            "GoogleDoc" => DocumentType::GoogleDoc,
            "GoogleSheet" => DocumentType::GoogleSheet,
            "GoogleSlide" => DocumentType::GoogleSlide,
            "Markdown" => DocumentType::Markdown,
            "PlainText" => DocumentType::PlainText,
            "RTF" => DocumentType::RTF,
            _ => DocumentType::PDF,
        })
        .unwrap_or(DocumentType::PDF);
    
    let source_type = props.get("source_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "LocalPath" => AssetSourceType::LocalPath,
            "CloudUrl" => AssetSourceType::CloudUrl,
            "AssetPipeline" => AssetSourceType::AssetPipeline,
            "ExternalUrl" => AssetSourceType::ExternalUrl,
            "Embedded" => AssetSourceType::Embedded,
            _ => AssetSourceType::LocalPath,
        })
        .unwrap_or(AssetSourceType::LocalPath);
    
    Document {
        document_type,
        source_type,
        source_path: props.get("source_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        asset_id: props.get("asset_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_bucket: props.get("cloud_bucket").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_key: props.get("cloud_key").and_then(|v| v.as_str()).map(|s| s.to_string()),
        file_size: props.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
        page_count: props.get("page_count").and_then(|v| v.as_u64()).map(|n| n as u32),
        last_modified: props.get("last_modified").and_then(|v| v.as_u64()).unwrap_or(0),
        content_hash: props.get("content_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
        auto_sync: props.get("auto_sync").and_then(|v| v.as_bool()).unwrap_or(false),
    }
}

fn image_asset_from_properties(props: &HashMap<String, serde_json::Value>) -> ImageAsset {
    use eustress_common::classes::{ImageAsset, ImageFormat, AssetSourceType};
    
    let format = props.get("format")
        .and_then(|v| v.as_str())
        .map(|s| ImageFormat::from_extension(s))
        .unwrap_or(ImageFormat::PNG);
    
    let source_type = props.get("source_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "LocalPath" => AssetSourceType::LocalPath,
            "CloudUrl" => AssetSourceType::CloudUrl,
            "AssetPipeline" => AssetSourceType::AssetPipeline,
            "ExternalUrl" => AssetSourceType::ExternalUrl,
            "Embedded" => AssetSourceType::Embedded,
            _ => AssetSourceType::LocalPath,
        })
        .unwrap_or(AssetSourceType::LocalPath);
    
    ImageAsset {
        format,
        source_type,
        source_path: props.get("source_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        asset_id: props.get("asset_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_bucket: props.get("cloud_bucket").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_key: props.get("cloud_key").and_then(|v| v.as_str()).map(|s| s.to_string()),
        width: props.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        height: props.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        file_size: props.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
        animated: props.get("animated").and_then(|v| v.as_bool()).unwrap_or(false),
        frame_count: props.get("frame_count").and_then(|v| v.as_u64()).map(|n| n as u32),
        content_hash: props.get("content_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
        auto_sync: props.get("auto_sync").and_then(|v| v.as_bool()).unwrap_or(false),
        thumbnail_id: props.get("thumbnail_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
    }
}

fn video_asset_from_properties(props: &HashMap<String, serde_json::Value>) -> VideoAsset {
    use eustress_common::classes::{VideoAsset, VideoFormat, AssetSourceType};
    
    let format = props.get("format")
        .and_then(|v| v.as_str())
        .map(|s| VideoFormat::from_extension(s))
        .unwrap_or(VideoFormat::MP4);
    
    let source_type = props.get("source_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "LocalPath" => AssetSourceType::LocalPath,
            "CloudUrl" => AssetSourceType::CloudUrl,
            "AssetPipeline" => AssetSourceType::AssetPipeline,
            "ExternalUrl" => AssetSourceType::ExternalUrl,
            "Embedded" => AssetSourceType::Embedded,
            _ => AssetSourceType::LocalPath,
        })
        .unwrap_or(AssetSourceType::LocalPath);
    
    VideoAsset {
        format,
        source_type,
        source_path: props.get("source_path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        asset_id: props.get("asset_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_bucket: props.get("cloud_bucket").and_then(|v| v.as_str()).map(|s| s.to_string()),
        cloud_key: props.get("cloud_key").and_then(|v| v.as_str()).map(|s| s.to_string()),
        width: props.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        height: props.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        duration: props.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        frame_rate: props.get("frame_rate").and_then(|v| v.as_f64()).unwrap_or(30.0) as f32,
        file_size: props.get("file_size").and_then(|v| v.as_u64()).unwrap_or(0),
        has_audio: props.get("has_audio").and_then(|v| v.as_bool()).unwrap_or(true),
        content_hash: props.get("content_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
        auto_sync: props.get("auto_sync").and_then(|v| v.as_bool()).unwrap_or(false),
        thumbnail_id: props.get("thumbnail_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        streaming_url: props.get("streaming_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
        looping: props.get("looping").and_then(|v| v.as_bool()).unwrap_or(false),
        autoplay: props.get("autoplay").and_then(|v| v.as_bool()).unwrap_or(false),
        volume: props.get("volume").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
    }
}

fn spawn_document(commands: &mut Commands, instance: Instance, document: Document) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        document,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

fn spawn_image_asset(commands: &mut Commands, instance: Instance, image: ImageAsset) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        image,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

fn spawn_video_asset(commands: &mut Commands, instance: Instance, video: VideoAsset) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        video,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

fn scrollingframe_from_properties(props: &HashMap<String, serde_json::Value>) -> ScrollingFrame {
    let border_mode = props.get("border_mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Outline" => BorderMode::Outline,
            "Middle" => BorderMode::Middle,
            "Inset" => BorderMode::Inset,
            _ => BorderMode::Outline,
        })
        .unwrap_or(BorderMode::Outline);
    
    let elastic_behavior = props.get("elastic_behavior")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Never" => ElasticBehavior::Never,
            "Always" => ElasticBehavior::Always,
            "WhenScrollable" => ElasticBehavior::WhenScrollable,
            _ => ElasticBehavior::Never,
        })
        .unwrap_or(ElasticBehavior::Never);
    
    let scroll_direction = props.get("scroll_direction")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "XY" => ScrollDirection::XY,
            "X" => ScrollDirection::X,
            "Y" => ScrollDirection::Y,
            _ => ScrollDirection::XY,
        })
        .unwrap_or(ScrollDirection::XY);
    
    let vertical_scroll_bar_inset = props.get("vertical_scroll_bar_inset")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "None" => ScrollBarInset::None,
            "ScrollBar" => ScrollBarInset::ScrollBar,
            "Always" => ScrollBarInset::Always,
            _ => ScrollBarInset::None,
        })
        .unwrap_or(ScrollBarInset::None);
    
    let horizontal_scroll_bar_inset = props.get("horizontal_scroll_bar_inset")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "None" => ScrollBarInset::None,
            "ScrollBar" => ScrollBarInset::ScrollBar,
            "Always" => ScrollBarInset::Always,
            _ => ScrollBarInset::None,
        })
        .unwrap_or(ScrollBarInset::None);
    
    ScrollingFrame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        border_mode,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([200.0, 200.0]),
        canvas_size: json_to_vec2(props.get("canvas_size")).unwrap_or([0.0, 0.0]),
        canvas_position: json_to_vec2(props.get("canvas_position")).unwrap_or([0.0, 0.0]),
        scroll_bar_enabled_x: props.get("scroll_bar_enabled_x").and_then(|v| v.as_bool()).unwrap_or(true),
        scroll_bar_enabled_y: props.get("scroll_bar_enabled_y").and_then(|v| v.as_bool()).unwrap_or(true),
        scroll_bar_image_color3: json_to_color3(props.get("scroll_bar_image_color3")).unwrap_or([0.3, 0.3, 0.3]),
        scroll_bar_image_transparency: props.get("scroll_bar_image_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        scroll_bar_thickness: props.get("scroll_bar_thickness").and_then(|v| v.as_i64()).unwrap_or(12) as i32,
        scrolling_enabled: props.get("scrolling_enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        top_image: props.get("top_image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        mid_image: props.get("mid_image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        bottom_image: props.get("bottom_image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        elastic_behavior,
        scroll_direction,
        vertical_scroll_bar_inset,
        horizontal_scroll_bar_inset,
    }
}

fn spawn_scrolling_frame(commands: &mut Commands, instance: Instance, scrolling_frame: ScrollingFrame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        scrolling_frame,
        Name::new(name),
        Attributes::new(),
        Tags::new(),
    )).id()
}

/// Helper to parse [f32; 2] from JSON
fn json_to_vec2(value: Option<&serde_json::Value>) -> Option<[f32; 2]> {
    value.and_then(|v| {
        if let Some(arr) = v.as_array() {
            if arr.len() >= 2 {
                Some([
                    arr[0].as_f64().unwrap_or(0.0) as f32,
                    arr[1].as_f64().unwrap_or(0.0) as f32,
                ])
            } else {
                None
            }
        } else {
            None
        }
    })
}

/// Helper to parse [f32; 3] vector from JSON
fn json_to_vec3(value: Option<&serde_json::Value>) -> Option<[f32; 3]> {
    value.and_then(|v| {
        if let Some(arr) = v.as_array() {
            if arr.len() >= 3 {
                Some([
                    arr[0].as_f64().unwrap_or(0.0) as f32,
                    arr[1].as_f64().unwrap_or(0.0) as f32,
                    arr[2].as_f64().unwrap_or(0.0) as f32,
                ])
            } else {
                None
            }
        } else {
            None
        }
    })
}

/// Helper to parse [f32; 3] color from JSON
fn json_to_color3(value: Option<&serde_json::Value>) -> Option<[f32; 3]> {
    value.and_then(|v| {
        if let Some(arr) = v.as_array() {
            if arr.len() >= 3 {
                Some([
                    arr[0].as_f64().unwrap_or(0.0) as f32,
                    arr[1].as_f64().unwrap_or(0.0) as f32,
                    arr[2].as_f64().unwrap_or(0.0) as f32,
                ])
            } else {
                None
            }
        } else {
            None
        }
    })
}

// ============================================================================
// VideoFrame Serialization
// ============================================================================

fn videoframe_from_properties(props: &HashMap<String, serde_json::Value>) -> VideoFrame {
    let scale_type = props.get("scale_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Stretch" => ScaleType::Stretch,
            "Slice" => ScaleType::Slice,
            "Tile" => ScaleType::Tile,
            "Fit" => ScaleType::Fit,
            "Crop" => ScaleType::Crop,
            _ => ScaleType::Fit,
        })
        .unwrap_or(ScaleType::Fit);
    
    VideoFrame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([0.0, 0.0, 0.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([320.0, 180.0]),
        video: props.get("video").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        autoplay: props.get("autoplay").and_then(|v| v.as_bool()).unwrap_or(false),
        looping: props.get("looping").and_then(|v| v.as_bool()).unwrap_or(false),
        volume: props.get("volume").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        playback_speed: props.get("playback_speed").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        time_position: props.get("time_position").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        playing: props.get("playing").and_then(|v| v.as_bool()).unwrap_or(false),
        show_controls: props.get("show_controls").and_then(|v| v.as_bool()).unwrap_or(true),
        scale_type,
        video_color3: json_to_color3(props.get("video_color3")).unwrap_or([1.0, 1.0, 1.0]),
        video_transparency: props.get("video_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    }
}

fn spawn_video_frame(commands: &mut Commands, instance: Instance, video_frame: VideoFrame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, video_frame, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// DocumentFrame Serialization
// ============================================================================

fn documentframe_from_properties(props: &HashMap<String, serde_json::Value>) -> DocumentFrame {
    let page_mode = props.get("page_mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "SinglePage" => PageDisplayMode::SinglePage,
            "Continuous" => PageDisplayMode::Continuous,
            "TwoPage" => PageDisplayMode::TwoPage,
            "FitWidth" => PageDisplayMode::FitWidth,
            "FitPage" => PageDisplayMode::FitPage,
            _ => PageDisplayMode::SinglePage,
        })
        .unwrap_or(PageDisplayMode::SinglePage);
    
    DocumentFrame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([400.0, 500.0]),
        document: props.get("document").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        current_page: props.get("current_page").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        zoom: props.get("zoom").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        show_controls: props.get("show_controls").and_then(|v| v.as_bool()).unwrap_or(true),
        selectable: props.get("selectable").and_then(|v| v.as_bool()).unwrap_or(true),
        scroll_position: json_to_vec2(props.get("scroll_position")).unwrap_or([0.0, 0.0]),
        scrolling_enabled: props.get("scrolling_enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        page_mode,
        document_color3: json_to_color3(props.get("document_color3")).unwrap_or([1.0, 1.0, 1.0]),
        document_transparency: props.get("document_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    }
}

fn spawn_document_frame(commands: &mut Commands, instance: Instance, document_frame: DocumentFrame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, document_frame, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// WebFrame Serialization
// ============================================================================

fn webframe_from_properties(props: &HashMap<String, serde_json::Value>) -> WebFrame {
    WebFrame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([0.1, 0.1, 0.1]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.2, 0.2, 0.2]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([800.0, 600.0]),
        url: props.get("url").and_then(|v| v.as_str()).unwrap_or("about:blank").to_string(),
        interactive: props.get("interactive").and_then(|v| v.as_bool()).unwrap_or(true),
        javascript_enabled: props.get("javascript_enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        navigation_enabled: props.get("navigation_enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        zoom: props.get("zoom").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        scrollbars_visible: props.get("scrollbars_visible").and_then(|v| v.as_bool()).unwrap_or(true),
        loading: false,
        title: props.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        transparent: props.get("transparent").and_then(|v| v.as_bool()).unwrap_or(false),
        resolution_scale: props.get("resolution_scale").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        user_agent: props.get("user_agent").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    }
}

fn spawn_web_frame(commands: &mut Commands, instance: Instance, web_frame: WebFrame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, web_frame, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// Frame Serialization
// ============================================================================

fn frame_from_properties(props: &HashMap<String, serde_json::Value>) -> Frame {
    let border_mode = props.get("border_mode")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Outline" => BorderMode::Outline,
            "Middle" => BorderMode::Middle,
            "Inset" => BorderMode::Inset,
            _ => BorderMode::Outline,
        })
        .unwrap_or(BorderMode::Outline);
    
    Frame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        border_mode,
        clips_descendants: props.get("clips_descendants").and_then(|v| v.as_bool()).unwrap_or(true),
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([100.0, 100.0]),
    }
}

fn spawn_frame(commands: &mut Commands, instance: Instance, frame: Frame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, frame, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// ImageLabel Serialization
// ============================================================================

fn imagelabel_from_properties(props: &HashMap<String, serde_json::Value>) -> ImageLabel {
    let scale_type = props.get("scale_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Stretch" => ScaleType::Stretch,
            "Slice" => ScaleType::Slice,
            "Tile" => ScaleType::Tile,
            "Fit" => ScaleType::Fit,
            "Crop" => ScaleType::Crop,
            _ => ScaleType::Stretch,
        })
        .unwrap_or(ScaleType::Stretch);
    
    ImageLabel {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        image: props.get("image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        image_color3: json_to_color3(props.get("image_color3")).unwrap_or([1.0, 1.0, 1.0]),
        image_transparency: props.get("image_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        scale_type,
        slice_center: json_to_vec4(props.get("slice_center")).unwrap_or([0.0, 0.0, 0.0, 0.0]),
        slice_scale: props.get("slice_scale").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        tile_size: json_to_vec2(props.get("tile_size")).unwrap_or([1.0, 1.0]),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([100.0, 100.0]),
    }
}

fn spawn_image_label(commands: &mut Commands, instance: Instance, image_label: ImageLabel) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, image_label, Name::new(name), Attributes::new(), Tags::new())).id()
}

/// Helper to parse [f32; 4] from JSON
fn json_to_vec4(value: Option<&serde_json::Value>) -> Option<[f32; 4]> {
    value.and_then(|v| {
        if let Some(arr) = v.as_array() {
            if arr.len() >= 4 {
                Some([
                    arr[0].as_f64().unwrap_or(0.0) as f32,
                    arr[1].as_f64().unwrap_or(0.0) as f32,
                    arr[2].as_f64().unwrap_or(0.0) as f32,
                    arr[3].as_f64().unwrap_or(0.0) as f32,
                ])
            } else {
                None
            }
        } else {
            None
        }
    })
}

// ============================================================================
// ImageButton Serialization
// ============================================================================

fn imagebutton_from_properties(props: &HashMap<String, serde_json::Value>) -> ImageButton {
    let scale_type = props.get("scale_type")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Stretch" => ScaleType::Stretch,
            "Slice" => ScaleType::Slice,
            "Tile" => ScaleType::Tile,
            "Fit" => ScaleType::Fit,
            "Crop" => ScaleType::Crop,
            _ => ScaleType::Stretch,
        })
        .unwrap_or(ScaleType::Stretch);
    
    ImageButton {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        active: props.get("active").and_then(|v| v.as_bool()).unwrap_or(true),
        auto_button_color: props.get("auto_button_color").and_then(|v| v.as_bool()).unwrap_or(true),
        image: props.get("image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        hover_image: props.get("hover_image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        pressed_image: props.get("pressed_image").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        image_color3: json_to_color3(props.get("image_color3")).unwrap_or([1.0, 1.0, 1.0]),
        image_transparency: props.get("image_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        scale_type,
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([100.0, 100.0]),
    }
}

fn spawn_image_button(commands: &mut Commands, instance: Instance, image_button: ImageButton) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, image_button, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// TextButton Serialization
// ============================================================================

fn textbutton_from_properties(props: &HashMap<String, serde_json::Value>) -> TextButton {
    let text_x_alignment = props.get("text_x_alignment")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Left" => TextXAlignment::Left,
            "Center" => TextXAlignment::Center,
            "Right" => TextXAlignment::Right,
            _ => TextXAlignment::Center,
        })
        .unwrap_or(TextXAlignment::Center);
    
    let text_y_alignment = props.get("text_y_alignment")
        .and_then(|v| v.as_str())
        .map(|s| match s {
            "Top" => TextYAlignment::Top,
            "Center" => TextYAlignment::Center,
            "Bottom" => TextYAlignment::Bottom,
            _ => TextYAlignment::Center,
        })
        .unwrap_or(TextYAlignment::Center);
    
    TextButton {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        active: props.get("active").and_then(|v| v.as_bool()).unwrap_or(true),
        auto_button_color: props.get("auto_button_color").and_then(|v| v.as_bool()).unwrap_or(true),
        text: props.get("text").and_then(|v| v.as_str()).unwrap_or("Button").to_string(),
        font_size: props.get("font_size").and_then(|v| v.as_f64()).unwrap_or(14.0) as f32,
        text_color3: json_to_color3(props.get("text_color3")).unwrap_or([0.1, 0.1, 0.1]),
        text_transparency: props.get("text_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        text_stroke_color3: json_to_color3(props.get("text_stroke_color3")).unwrap_or([0.0, 0.0, 0.0]),
        text_stroke_transparency: props.get("text_stroke_transparency").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        text_x_alignment,
        text_y_alignment,
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([0.8, 0.8, 0.8]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.1, 0.1, 0.1]),
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        rotation: props.get("rotation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([200.0, 50.0]),
    }
}

fn spawn_text_button(commands: &mut Commands, instance: Instance, text_button: TextButton) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, text_button, Name::new(name), Attributes::new(), Tags::new())).id()
}

/// Spawn BillboardGui (3D camera-facing GUI)
fn spawn_billboard_gui(
    commands: &mut Commands,
    instance: Instance,
    billboard_gui: BillboardGui,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        billboard_gui,
        Name::new(name),
    )).id()
}

/// Spawn TextLabel (text display element)
fn spawn_text_label(
    commands: &mut Commands,
    instance: Instance,
    text_label: TextLabel,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        Transform::default(),
        Visibility::default(),
        instance,
        text_label,
        Name::new(name),
    )).id()
}

/// Load a scene from JSON file using exclusive World access
/// Returns the number of entities loaded
pub fn load_scene_from_world(
    world: &mut World,
    path: &Path,
) -> crate::serialization::Result<usize> {
    // Read and parse the scene file
    let json = std::fs::read_to_string(path)?;
    let scene: Scene = serde_json::from_str(&json)?;
    
    // Verify format
    if scene.format != "eustress_propertyaccess" {
        return Err(crate::serialization::SerializationError::InvalidFormat(
            format!("Expected 'eustress_propertyaccess', got '{}'", scene.format)
        ));
    }
    
    // Spawn all entities directly in World
    let entity_count = scene.entities.len();
    
    for entity_data in scene.entities {
        // Extract name from properties
        let name = entity_data.properties.get("Name")
            .and_then(|v| v.as_str())
            .unwrap_or("Entity")
            .to_string();
        
        // Parse class name - simple match for now
        let class_name = match entity_data.class.as_str() {
            "Part" => crate::classes::ClassName::Part,
            "Model" => crate::classes::ClassName::Model,
            "Folder" => crate::classes::ClassName::Folder,
            "SoulScript" => crate::classes::ClassName::SoulScript,
            _ => crate::classes::ClassName::Instance,
        };
        
        // Create base instance
        let instance = crate::classes::Instance {
            name: name.clone(),
            class_name,
            archivable: true,
            id: entity_data.id,
            ..Default::default()
        };
        
        // Spawn with class-specific components
        if class_name == crate::classes::ClassName::SoulScript {
            let soul_script = soulscript_from_properties(&entity_data.properties);
            world.spawn((
                instance,
                Name::new(name),
                soul_script,
            ));
        } else {
            world.spawn((
                instance,
                Name::new(name),
            ));
        }
    }
    
    Ok(entity_count)
}

/// Reconstruct SoulScriptData from properties
fn soulscript_from_properties(props: &HashMap<String, serde_json::Value>) -> SoulScriptData {
    let mut soul_script = SoulScriptData::default();
    
    // Source code
    if let Some(source) = props.get("Source").and_then(|v| v.as_str()) {
        soul_script.source = source.to_string();
    }
    
    // Generated code
    if let Some(generated) = props.get("GeneratedCode").and_then(|v| v.as_str()) {
        soul_script.generated_code = Some(generated.to_string());
    }
    
    // Build status
    if let Some(status) = props.get("BuildStatus").and_then(|v| v.as_str()) {
        soul_script.build_status = match status {
            "NotBuilt" => SoulBuildStatus::NotBuilt,
            "Building" => SoulBuildStatus::NotBuilt, // Reset to NotBuilt on load
            "Built" => SoulBuildStatus::Built,
            "Failed" => SoulBuildStatus::Failed,
            "Stale" => SoulBuildStatus::Stale,
            _ => SoulBuildStatus::NotBuilt,
        };
    }
    
    // Errors
    if let Some(errors) = props.get("Errors").and_then(|v| v.as_array()) {
        soul_script.errors = errors.iter()
            .filter_map(|e| e.as_str().map(|s| s.to_string()))
            .collect();
    }
    
    soul_script
}

/// Spawn a SoulScript entity
fn spawn_soul_script(commands: &mut Commands, instance: Instance, soul_script: SoulScriptData) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        instance,
        Name::new(name),
        soul_script,
    )).id()
}

// ============================================================================
// ScreenGui Serialization
// ============================================================================

fn screengui_from_properties(props: &HashMap<String, serde_json::Value>) -> ScreenGui {
    ScreenGui {
        enabled: props.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        display_order: props.get("display_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        ignore_gui_inset: props.get("ignore_gui_inset").and_then(|v| v.as_bool()).unwrap_or(false),
        reset_on_spawn: props.get("reset_on_spawn").and_then(|v| v.as_bool()).unwrap_or(true),
        z_index_behavior: props.get("z_index_behavior")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Global" => ZIndexBehavior::Global,
                _ => ZIndexBehavior::Sibling,
            })
            .unwrap_or(ZIndexBehavior::Sibling),
        clips_descendants: props.get("clips_descendants").and_then(|v| v.as_bool()).unwrap_or(true),
        screen_insets: props.get("screen_insets")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "DeviceSafeInsets" => ScreenInsets::DeviceSafeInsets,
                "CoreUISafeInsets" => ScreenInsets::CoreUISafeInsets,
                _ => ScreenInsets::None,
            })
            .unwrap_or(ScreenInsets::CoreUISafeInsets),
    }
}

fn spawn_screen_gui(commands: &mut Commands, instance: Instance, screen_gui: ScreenGui) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, screen_gui, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// SurfaceGui Serialization
// ============================================================================

fn surfacegui_from_properties(props: &HashMap<String, serde_json::Value>) -> SurfaceGui {
    SurfaceGui {
        active: props.get("active").and_then(|v| v.as_bool()).unwrap_or(true),
        enabled: props.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
        adornee: None, // Entity references need special handling
        face: props.get("face")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Top" => NormalId::Top,
                "Bottom" => NormalId::Bottom,
                "Left" => NormalId::Left,
                "Right" => NormalId::Right,
                "Back" => NormalId::Back,
                _ => NormalId::Front,
            })
            .unwrap_or(NormalId::Front),
        canvas_size: json_to_vec2(props.get("canvas_size")).unwrap_or([800.0, 600.0]),
        always_on_top: props.get("always_on_top").and_then(|v| v.as_bool()).unwrap_or(false),
        brightness: props.get("brightness").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
        light_influence: props.get("light_influence").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        pixels_per_unit: props.get("pixels_per_unit").and_then(|v| v.as_f64()).unwrap_or(50.0) as f32,
        z_index_behavior: props.get("z_index_behavior")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Global" => ZIndexBehavior::Global,
                _ => ZIndexBehavior::Sibling,
            })
            .unwrap_or(ZIndexBehavior::Sibling),
        clips_descendants: props.get("clips_descendants").and_then(|v| v.as_bool()).unwrap_or(true),
        max_distance: props.get("max_distance").and_then(|v| v.as_f64()).unwrap_or(1000.0) as f32,
        horizontal_alignment: props.get("horizontal_alignment")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Left" => HorizontalAlignment::Left,
                "Right" => HorizontalAlignment::Right,
                _ => HorizontalAlignment::Center,
            })
            .unwrap_or(HorizontalAlignment::Center),
        vertical_alignment: props.get("vertical_alignment")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "Top" => VerticalAlignment::Top,
                "Bottom" => VerticalAlignment::Bottom,
                _ => VerticalAlignment::Center,
            })
            .unwrap_or(VerticalAlignment::Center),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([1.0, 1.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([0.0, 0.0]),
    }
}

fn spawn_surface_gui(commands: &mut Commands, instance: Instance, surface_gui: SurfaceGui) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, surface_gui, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// TextBox Serialization
// ============================================================================

fn textbox_from_properties(props: &HashMap<String, serde_json::Value>) -> TextBox {
    TextBox {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        text: props.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        placeholder_text: props.get("placeholder_text").and_then(|v| v.as_str()).unwrap_or("Enter text...").to_string(),
        focused: false, // Don't persist focus state
        text_editable: props.get("text_editable").and_then(|v| v.as_bool()).unwrap_or(true),
        clear_text_on_focus: props.get("clear_text_on_focus").and_then(|v| v.as_bool()).unwrap_or(false),
        multi_line: props.get("multi_line").and_then(|v| v.as_bool()).unwrap_or(false),
        max_length: props.get("max_length").and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
        font_size: props.get("font_size").and_then(|v| v.as_f64()).unwrap_or(14.0) as f32,
        text_color3: json_to_color3(props.get("text_color3")).unwrap_or([0.0, 0.0, 0.0]),
        text_transparency: props.get("text_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        placeholder_color3: json_to_color3(props.get("placeholder_color3")).unwrap_or([0.5, 0.5, 0.5]),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([1.0, 1.0, 1.0]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        border_color3: json_to_color3(props.get("border_color3")).unwrap_or([0.3, 0.3, 0.3]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([200.0, 30.0]),
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        border_size_pixel: props.get("border_size_pixel").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
    }
}

fn spawn_text_box(commands: &mut Commands, instance: Instance, text_box: TextBox) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, text_box, Name::new(name), Attributes::new(), Tags::new())).id()
}

// ============================================================================
// ViewportFrame Serialization
// ============================================================================

fn viewportframe_from_properties(props: &HashMap<String, serde_json::Value>) -> ViewportFrame {
    ViewportFrame {
        visible: props.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
        background_color3: json_to_color3(props.get("background_color3")).unwrap_or([0.1, 0.1, 0.1]),
        background_transparency: props.get("background_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
        z_index: props.get("z_index").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        layout_order: props.get("layout_order").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        anchor_point: json_to_vec2(props.get("anchor_point")).unwrap_or([0.0, 0.0]),
        position_scale: json_to_vec2(props.get("position_scale")).unwrap_or([0.0, 0.0]),
        position_offset: json_to_vec2(props.get("position_offset")).unwrap_or([0.0, 0.0]),
        size_scale: json_to_vec2(props.get("size_scale")).unwrap_or([0.0, 0.0]),
        size_offset: json_to_vec2(props.get("size_offset")).unwrap_or([200.0, 200.0]),
        current_camera: None, // Entity references need special handling
        ambient: props.get("ambient").and_then(|v| v.as_bool()).unwrap_or(true),
        light_color: json_to_color3(props.get("light_color")).unwrap_or([1.0, 1.0, 1.0]),
        light_direction: json_to_vec3(props.get("light_direction")).unwrap_or([0.0, -1.0, 0.0]),
        image_transparency: props.get("image_transparency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
    }
}

fn spawn_viewport_frame(commands: &mut Commands, instance: Instance, viewport_frame: ViewportFrame) -> Entity {
    let name = instance.name.clone();
    commands.spawn((instance, viewport_frame, Name::new(name), Attributes::new(), Tags::new())).id()
}
