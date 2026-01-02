use crate::parts::{PartManager, PartType, PartData, PartUpdate, Material};
// Removed: use tauri::State;
use serde_json::Value;

/// Create a new part
// Removed tauri::command - now called directly from egui UI
pub fn create_part(
    part_type: String,
    x: Option<f32>,
    y: Option<f32>,
    z: Option<f32>,
    name: Option<String>,
    part_manager: State<PartManager>,
) -> Result<u32, String> {
    let ptype = PartType::from_str(&part_type)
        .ok_or_else(|| format!("Invalid part type: {}", part_type))?;
    
    let position = bevy::prelude::Vec3::new(
        x.unwrap_or(0.0),
        y.unwrap_or(5.0),
        z.unwrap_or(0.0),
    );

    let id = part_manager.create_part(ptype, position, name);
    Ok(id)
}

/// Get part data by ID
// Removed tauri::command - now called directly from egui UI
pub fn get_part(
    part_id: u32,
    part_manager: State<PartManager>,
) -> Result<PartData, String> {
    part_manager.get_part(part_id)
        .ok_or_else(|| format!("Part {} not found", part_id))
}

/// List all parts
// Removed tauri::command - now called directly from egui UI
pub fn list_parts(
    part_manager: State<PartManager>,
) -> Result<Vec<PartData>, String> {
    part_manager.list_parts()
}

/// Update part properties
// Removed tauri::command - now called directly from egui UI
pub fn update_part_property(
    part_id: u32,
    property: String,
    value: Value,
    part_manager: State<PartManager>,
) -> Result<(), String> {
    let mut update = PartUpdate::default();

    match property.as_str() {
        "name" => {
            update.name = Some(value.as_str()
                .ok_or("Invalid name value")?
                .to_string());
        }
        "position" => {
            let arr = value.as_array().ok_or("Position must be an array")?;
            if arr.len() != 3 {
                return Err("Position must have 3 elements".to_string());
            }
            update.position = Some([
                arr[0].as_f64().ok_or("Invalid X")? as f32,
                arr[1].as_f64().ok_or("Invalid Y")? as f32,
                arr[2].as_f64().ok_or("Invalid Z")? as f32,
            ]);
        }
        "rotation" => {
            let arr = value.as_array().ok_or("Rotation must be an array")?;
            if arr.len() != 3 {
                return Err("Rotation must have 3 elements".to_string());
            }
            update.rotation = Some([
                arr[0].as_f64().ok_or("Invalid pitch")? as f32,
                arr[1].as_f64().ok_or("Invalid yaw")? as f32,
                arr[2].as_f64().ok_or("Invalid roll")? as f32,
            ]);
        }
        "size" => {
            let arr = value.as_array().ok_or("Size must be an array")?;
            if arr.len() != 3 {
                return Err("Size must have 3 elements".to_string());
            }
            update.size = Some([
                arr[0].as_f64().ok_or("Invalid width")? as f32,
                arr[1].as_f64().ok_or("Invalid height")? as f32,
                arr[2].as_f64().ok_or("Invalid depth")? as f32,
            ]);
        }
        "color" => {
            let arr = value.as_array().ok_or("Color must be an array")?;
            if arr.len() < 3 || arr.len() > 4 {
                return Err("Color must have 3 or 4 elements".to_string());
            }
            update.color = Some([
                arr[0].as_f64().ok_or("Invalid R")? as f32,
                arr[1].as_f64().ok_or("Invalid G")? as f32,
                arr[2].as_f64().ok_or("Invalid B")? as f32,
                arr.get(3).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            ]);
        }
        "material" => {
            let mat_str = value.as_str().ok_or("Invalid material value")?;
            let material = match mat_str.to_lowercase().as_str() {
                "plastic" => Material::Plastic,
                "smoothplastic" => Material::SmoothPlastic,
                "wood" => Material::Wood,
                "woodplanks" => Material::WoodPlanks,
                "metal" => Material::Metal,
                "corrodedmetal" => Material::CorrodedMetal,
                "diamondplate" => Material::DiamondPlate,
                "foil" => Material::Foil,
                "grass" => Material::Grass,
                "concrete" => Material::Concrete,
                "brick" => Material::Brick,
                "granite" => Material::Granite,
                "marble" => Material::Marble,
                "slate" => Material::Slate,
                "sand" => Material::Sand,
                "fabric" => Material::Fabric,
                "glass" => Material::Glass,
                "neon" => Material::Neon,
                "ice" => Material::Ice,
                _ => return Err(format!("Unknown material: {}", mat_str)),
            };
            update.material = Some(material);
        }
        "anchored" => {
            update.anchored = Some(value.as_bool().ok_or("Invalid anchored value")?);
        }
        "transparency" => {
            update.transparency = Some(value.as_f64().ok_or("Invalid transparency")? as f32);
        }
        "can_collide" | "canCollide" => {
            update.can_collide = Some(value.as_bool().ok_or("Invalid canCollide value")?);
        }
        _ => return Err(format!("Unknown property: {}", property)),
    }

    part_manager.update_part(part_id, update)
}

/// Delete a part
// Removed tauri::command - now called directly from egui UI
pub fn delete_part(
    part_id: u32,
    part_manager: State<PartManager>,
) -> Result<(), String> {
    part_manager.delete_part(part_id)
}

/// Duplicate a part
// Removed tauri::command - now called directly from egui UI
pub fn duplicate_part(
    part_id: u32,
    part_manager: State<PartManager>,
) -> Result<u32, String> {
    part_manager.duplicate_part(part_id)
}

/// Set part parent (for hierarchy)
// Removed tauri::command - now called directly from egui UI
pub fn set_part_parent(
    child_id: u32,
    parent_id: Option<u32>,
    part_manager: State<PartManager>,
) -> Result<(), String> {
    part_manager.set_parent(child_id, parent_id)
}

/// Batch create multiple parts (for performance)
// Removed tauri::command - now called directly from egui UI
pub fn create_parts_batch(
    parts: Vec<(String, f32, f32, f32)>, // (type, x, y, z)
    part_manager: State<PartManager>,
) -> Result<Vec<u32>, String> {
    let mut ids = Vec::new();
    
    for (part_type, x, y, z) in parts {
        let ptype = PartType::from_str(&part_type)
            .ok_or_else(|| format!("Invalid part type: {}", part_type))?;
        
        let position = bevy::prelude::Vec3::new(x, y, z);
        let id = part_manager.create_part(ptype, position, None);
        ids.push(id);
    }
    
    Ok(ids)
}

/// Get parts by parent (for hierarchy queries)
// Removed tauri::command - now called directly from egui UI
pub fn get_children_parts(
    parent_id: Option<u32>,
    part_manager: State<PartManager>,
) -> Result<Vec<PartData>, String> {
    let all_parts = part_manager.list_parts()?;
    Ok(all_parts.into_iter()
        .filter(|p| p.parent == parent_id)
        .collect())
}
