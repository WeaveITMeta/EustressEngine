#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

// Part type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartType {
    Cube,
    Sphere,
    Cylinder,
    Wedge,
    CornerWedge,
    Cone,
}

impl PartType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cube" | "block" | "part" => Some(PartType::Cube),
            "sphere" | "ball" => Some(PartType::Sphere),
            "cylinder" => Some(PartType::Cylinder),
            "wedge" => Some(PartType::Wedge),
            "cornerwedge" => Some(PartType::CornerWedge),
            "cone" => Some(PartType::Cone),
            _ => None,
        }
    }
}

// Material types (Roblox-inspired)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Material {
    Plastic,
    SmoothPlastic,
    Wood,
    WoodPlanks,
    Metal,
    CorrodedMetal,
    DiamondPlate,
    Foil,
    Grass,
    Concrete,
    Brick,
    Granite,
    Marble,
    Slate,
    Sand,
    Fabric,
    Glass,
    Neon,
    Ice,
}

impl Default for Material {
    fn default() -> Self {
        Material::Plastic
    }
}

// Part component for entities
#[derive(Component, Debug, Clone)]
pub struct Part {
    pub part_type: PartType,
    pub size: Vec3,
    pub color: Color,
    pub material: Material,
    pub anchored: bool,
    pub transparency: f32,
    pub can_collide: bool,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            part_type: PartType::Cube,
            size: Vec3::new(4.0, 1.0, 2.0), // Default Roblox part size
            color: Color::srgb(0.6, 0.6, 0.6), // Medium gray
            material: Material::Plastic,
            anchored: false,
            transparency: 0.0,
            can_collide: true,
        }
    }
}

// Part data for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartData {
    pub id: u32,
    pub name: String,
    pub part_type: PartType,
    pub position: [f32; 3],
    pub rotation: [f32; 3], // Euler angles in degrees
    pub size: [f32; 3],
    pub color: [f32; 4], // RGBA
    pub material: Material,
    pub anchored: bool,
    pub transparency: f32,
    pub can_collide: bool,
    pub parent: Option<u32>,
    pub locked: bool, // If true, part cannot be selected or moved
}

// Part manager for tracking created parts
pub struct PartManager {
    pub parts: Arc<Mutex<HashMap<u32, PartData>>>,
    pub next_id: Arc<Mutex<u32>>,
}

impl Default for PartManager {
    fn default() -> Self {
        Self {
            parts: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        }
    }
}

impl PartManager {
    pub fn create_part(&self, part_type: PartType, position: Vec3, name: Option<String>) -> u32 {
        let mut next_id = self.next_id.lock().unwrap();
        let id = *next_id;
        *next_id += 1;
        drop(next_id);

        let part_name = name.unwrap_or_else(|| format!("{:?}", part_type));
        
        let part_data = PartData {
            id,
            name: part_name,
            part_type,
            position: position.to_array(),
            rotation: [0.0, 0.0, 0.0],
            size: match part_type {
                PartType::Cube => [4.0, 1.0, 2.0],
                PartType::Sphere => [4.0, 4.0, 4.0],
                PartType::Cylinder => [4.0, 4.0, 4.0],
                PartType::Wedge => [4.0, 1.0, 2.0],
                PartType::CornerWedge => [2.0, 2.0, 2.0],
                PartType::Cone => [4.0, 4.0, 4.0],
            },
            color: [0.6, 0.6, 0.6, 1.0], // Medium gray
            material: Material::Plastic,
            anchored: false,
            transparency: 0.0,
            can_collide: true,
            parent: None,
            locked: false, // Unlocked by default
        };

        let mut parts = self.parts.lock().unwrap();
        parts.insert(id, part_data);

        id
    }

    pub fn get_part(&self, id: u32) -> Option<PartData> {
        let parts = self.parts.lock().unwrap();
        parts.get(&id).cloned()
    }
    
    /// Update a part using a closure
    pub fn with_part_mut<F, R>(&self, id: u32, f: F) -> Option<R>
    where
        F: FnOnce(&mut PartData) -> R,
    {
        let mut parts = self.parts.lock().unwrap();
        parts.get_mut(&id).map(f)
    }

    pub fn update_part(&self, id: u32, updates: PartUpdate) -> Result<(), String> {
        let mut parts = self.parts.lock().unwrap();
        let part = parts.get_mut(&id).ok_or("Part not found")?;

        if let Some(name) = updates.name {
            part.name = name;
        }
        if let Some(position) = updates.position {
            part.position = position;
        }
        if let Some(rotation) = updates.rotation {
            part.rotation = rotation;
        }
        if let Some(size) = updates.size {
            part.size = size;
        }
        if let Some(color) = updates.color {
            part.color = color;
        }
        if let Some(material) = updates.material {
            part.material = material;
        }
        if let Some(anchored) = updates.anchored {
            part.anchored = anchored;
        }
        if let Some(transparency) = updates.transparency {
            part.transparency = transparency.clamp(0.0, 1.0);
        }
        if let Some(can_collide) = updates.can_collide {
            part.can_collide = can_collide;
        }
        if let Some(parent) = updates.parent {
            part.parent = parent;
        }
        if let Some(locked) = updates.locked {
            part.locked = locked;
        }

        Ok(())
    }

    pub fn delete_part(&self, id: u32) -> Result<(), String> {
        let mut parts = self.parts.lock().unwrap();
        parts.remove(&id).ok_or("Part not found")?;
        Ok(())
    }

    pub fn duplicate_part(&self, id: u32) -> Result<u32, String> {
        let original = {
            let parts = self.parts.lock().unwrap();
            parts.get(&id).cloned().ok_or("Part not found")?
        };

        let mut next_id = self.next_id.lock().unwrap();
        let new_id = *next_id;
        *next_id += 1;
        drop(next_id);

        let mut new_part = original.clone();
        new_part.id = new_id;
        new_part.name = format!("{} (Copy)", original.name);
        // Duplicate in place (no position offset)

        let mut parts = self.parts.lock().unwrap();
        parts.insert(new_id, new_part);

        Ok(new_id)
    }

    pub fn list_parts(&self) -> Result<Vec<PartData>, String> {
        let parts = self.parts.lock().unwrap();
        Ok(parts.values().cloned().collect())
    }
    
    /// Store a part with a specific ID (for loading scenes)
    pub fn store_part(&self, part: PartData) -> Result<(), String> {
        let mut parts = self.parts.lock().unwrap();
        let mut next_id = self.next_id.lock().unwrap();
        
        // Get the ID before moving the part
        let part_id = part.id;
        
        // Insert the part
        parts.insert(part_id, part);
        
        // Update next_id if necessary
        if part_id >= *next_id {
            *next_id = part_id + 1;
        }
        
        Ok(())
    }

    pub fn set_parent(&self, child_id: u32, parent_id: Option<u32>) -> Result<(), String> {
        let mut parts = self.parts.lock().unwrap();
        
        // Verify child exists
        if !parts.contains_key(&child_id) {
            return Err("Child part not found".to_string());
        }
        
        // Verify parent exists if specified
        if let Some(pid) = parent_id {
            if !parts.contains_key(&pid) {
                return Err("Parent part not found".to_string());
            }
        }

        // Now update the child's parent
        if let Some(child) = parts.get_mut(&child_id) {
            child.parent = parent_id;
        }
        
        Ok(())
    }
}

// Partial update struct for updating only specific fields
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PartUpdate {
    pub name: Option<String>,
    pub position: Option<[f32; 3]>,
    pub rotation: Option<[f32; 3]>,
    pub size: Option<[f32; 3]>,
    pub color: Option<[f32; 4]>,
    pub material: Option<Material>,
    pub anchored: Option<bool>,
    pub transparency: Option<f32>,
    pub can_collide: Option<bool>,
    pub parent: Option<Option<u32>>, // Option<Option> to allow unsetting parent
    pub locked: Option<bool>, // Lock/unlock part
}

// Spawn part system for Bevy
// NOTE: This is a placeholder. PartManager is a Tauri State, not a Bevy Resource.
// For actual spawning, see rendering.rs which uses BevyPartManager wrapper.
#[allow(dead_code)]
pub fn spawn_part_system(
    _commands: Commands,
    _meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
) {
    // This would spawn new parts into the Bevy world
    // For now, it's a placeholder for future integration
}
