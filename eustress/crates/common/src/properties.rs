//! # Property Access Implementations
//!
//! PropertyAccess trait implementations for all class types.
//! This must be in the common crate to satisfy Rust's orphan rules.

use bevy::prelude::*;
use crate::classes::*;

// ============================================================================
// PropertyAccess Implementation for Instance
// ============================================================================

impl PropertyAccess for Instance {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Name" => Some(PropertyValue::String(self.name.clone())),
            "ClassName" => Some(PropertyValue::String(self.class_name.as_str().to_string())),
            "Archivable" => Some(PropertyValue::Bool(self.archivable)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Name", PropertyValue::String(s)) => {
                self.name = s;
                Ok(())
            }
            ("Archivable", PropertyValue::Bool(b)) => {
                self.archivable = b;
                Ok(())
            }
            ("ClassName", _) => Err("ClassName is read-only".to_string()),
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor {
                name: "Name".to_string(),
                property_type: "string".to_string(),
                read_only: false,
                category: "Data".to_string(),
            },
            PropertyDescriptor {
                name: "ClassName".to_string(),
                property_type: "string".to_string(),
                read_only: true,
                category: "Data".to_string(),
            },
            PropertyDescriptor {
                name: "Archivable".to_string(),
                property_type: "bool".to_string(),
                read_only: false,
                category: "Data".to_string(),
            },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for BasePart
// ============================================================================

impl PropertyAccess for BasePart {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Position" => Some(PropertyValue::Vector3(self.cframe.translation)),
            "Size" => Some(PropertyValue::Vector3(self.size)),
            "Orientation" => {
                let (x, y, z) = self.cframe.rotation.to_euler(EulerRot::XYZ);
                Some(PropertyValue::Vector3(Vec3::new(
                    x.to_degrees(),
                    y.to_degrees(),
                    z.to_degrees(),
                )))
            }
            "Color" => Some(PropertyValue::Color(self.color)),
            "Material" => Some(PropertyValue::Enum(format!("{:?}", self.material))),
            "Transparency" => Some(PropertyValue::Float(self.transparency)),
            "Reflectance" => Some(PropertyValue::Float(self.reflectance)),
            "Anchored" => Some(PropertyValue::Bool(self.anchored)),
            "CanCollide" => Some(PropertyValue::Bool(self.can_collide)),
            "CanTouch" => Some(PropertyValue::Bool(self.can_touch)),
            "Density" => Some(PropertyValue::Float(self.density)),
            "Mass" => Some(PropertyValue::Float(self.mass)),
            "AssemblyLinearVelocity" => Some(PropertyValue::Vector3(self.assembly_linear_velocity)),
            "AssemblyAngularVelocity" => Some(PropertyValue::Vector3(self.assembly_angular_velocity)),
            "CollisionGroup" => Some(PropertyValue::String(self.collision_group.clone())),
            "Locked" => Some(PropertyValue::Bool(self.locked)),
            "Deformation" => Some(PropertyValue::Bool(self.deformation)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Position", PropertyValue::Vector3(v)) => {
                self.cframe.translation = v;
                Ok(())
            }
            ("Size", PropertyValue::Vector3(v)) => {
                if v.x <= 0.0 || v.y <= 0.0 || v.z <= 0.0 {
                    Err("Size must be positive".to_string())
                } else {
                    self.size = v;
                    Ok(())
                }
            }
            ("Orientation", PropertyValue::Vector3(v)) => {
                self.cframe.rotation = Quat::from_euler(
                    EulerRot::XYZ,
                    v.x.to_radians(),
                    v.y.to_radians(),
                    v.z.to_radians(),
                );
                Ok(())
            }
            ("Color", PropertyValue::Color(c)) => {
                self.color = c;
                Ok(())
            }
            ("Transparency", PropertyValue::Float(f)) => {
                self.transparency = f.clamp(0.0, 1.0);
                Ok(())
            }
            ("Reflectance", PropertyValue::Float(f)) => {
                self.reflectance = f.clamp(0.0, 1.0);
                Ok(())
            }
            ("Anchored", PropertyValue::Bool(b)) => {
                self.anchored = b;
                Ok(())
            }
            ("CanCollide", PropertyValue::Bool(b)) => {
                self.can_collide = b;
                Ok(())
            }
            ("CanTouch", PropertyValue::Bool(b)) => {
                self.can_touch = b;
                Ok(())
            }
            ("AssemblyLinearVelocity", PropertyValue::Vector3(v)) => {
                self.assembly_linear_velocity = v;
                Ok(())
            }
            ("AssemblyAngularVelocity", PropertyValue::Vector3(v)) => {
                self.assembly_angular_velocity = v;
                Ok(())
            }
            ("CollisionGroup", PropertyValue::String(s)) => {
                self.collision_group = s;
                Ok(())
            }
            ("Locked", PropertyValue::Bool(b)) => {
                self.locked = b;
                Ok(())
            }
            ("Deformation", PropertyValue::Bool(b)) => {
                self.deformation = b;
                Ok(())
            }
            ("Density", PropertyValue::Float(f)) => {
                if f < 0.0 {
                    Err("Density cannot be negative".to_string())
                } else {
                    self.set_density(f);
                    Ok(())
                }
            }
            ("Mass", PropertyValue::Float(f)) => {
                if f < 0.0 {
                    Err("Mass cannot be negative".to_string())
                } else {
                    self.set_mass(f);
                    Ok(())
                }
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Anchored".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "CanCollide".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "CanTouch".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Locked".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Color".to_string(), property_type: "Color3".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Material".to_string(), property_type: "Material".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Reflectance".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Transparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Position".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
            PropertyDescriptor { name: "Orientation".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
            PropertyDescriptor { name: "Size".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
            PropertyDescriptor { name: "Density".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "Mass".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "Deformation".to_string(), property_type: "bool".to_string(), read_only: false, category: "Physics".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Part
// ============================================================================

impl PropertyAccess for Part {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Shape" => Some(PropertyValue::Enum(format!("{:?}", self.shape))),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Shape", PropertyValue::Enum(s)) => {
                if let Some(shape) = PartType::from_str(&s) {
                    self.shape = shape;
                    Ok(())
                } else {
                    Err(format!("Invalid shape: {}", s))
                }
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![PropertyDescriptor { name: "Shape".to_string(), property_type: "PartType".to_string(), read_only: false, category: "Data".to_string() }]
    }
}

// ============================================================================
// PropertyAccess Implementation for Model
// ============================================================================

impl PropertyAccess for Model {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "PrimaryPart" => Some(PropertyValue::Int(self.primary_part.unwrap_or(0) as i32)),
            "WorldPivot" => Some(PropertyValue::Transform(self.world_pivot)),
            "AssemblyMass" => Some(PropertyValue::Float(self.assembly_mass)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("PrimaryPart", PropertyValue::Int(id)) => {
                self.primary_part = if id > 0 { Some(id as u32) } else { None };
                Ok(())
            }
            ("WorldPivot", _) => Err("WorldPivot is computed (read-only)".to_string()),
            ("AssemblyMass", _) => Err("AssemblyMass is computed (read-only)".to_string()),
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "PrimaryPart".to_string(), property_type: "BasePart?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "WorldPivot".to_string(), property_type: "CFrame".to_string(), read_only: true, category: "Transform".to_string() },
            PropertyDescriptor { name: "AssemblyMass".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Humanoid
// ============================================================================

impl PropertyAccess for Humanoid {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "WalkSpeed" => Some(PropertyValue::Float(self.walk_speed)),
            "JumpPower" => Some(PropertyValue::Float(self.jump_power)),
            "HipHeight" => Some(PropertyValue::Float(self.hip_height)),
            "Health" => Some(PropertyValue::Float(self.health)),
            "MaxHealth" => Some(PropertyValue::Float(self.max_health)),
            "AutoRotate" => Some(PropertyValue::Bool(self.auto_rotate)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("WalkSpeed", PropertyValue::Float(f)) => { self.walk_speed = f.max(0.0); Ok(()) }
            ("JumpPower", PropertyValue::Float(f)) => { self.jump_power = f.max(0.0); Ok(()) }
            ("HipHeight", PropertyValue::Float(f)) => { self.hip_height = f; Ok(()) }
            ("Health", PropertyValue::Float(f)) => { self.health = f.clamp(0.0, self.max_health); Ok(()) }
            ("MaxHealth", PropertyValue::Float(f)) => { self.max_health = f.max(0.0); self.health = self.health.min(self.max_health); Ok(()) }
            ("AutoRotate", PropertyValue::Bool(b)) => { self.auto_rotate = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "WalkSpeed".to_string(), property_type: "float".to_string(), read_only: false, category: "Character".to_string() },
            PropertyDescriptor { name: "JumpPower".to_string(), property_type: "float".to_string(), read_only: false, category: "Character".to_string() },
            PropertyDescriptor { name: "Health".to_string(), property_type: "float".to_string(), read_only: false, category: "State".to_string() },
            PropertyDescriptor { name: "MaxHealth".to_string(), property_type: "float".to_string(), read_only: false, category: "State".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Attachment
// ============================================================================

impl PropertyAccess for Attachment {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Position" => Some(PropertyValue::Vector3(self.position)),
            "Orientation" => Some(PropertyValue::Vector3(self.orientation)),
            "CFrame" => Some(PropertyValue::Transform(self.cframe)),
            "Name" => Some(PropertyValue::String(self.name.clone())),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Position", PropertyValue::Vector3(v)) => { self.position = v; self.cframe.translation = v; Ok(()) }
            ("Orientation", PropertyValue::Vector3(v)) => {
                self.orientation = v;
                self.cframe.rotation = Quat::from_euler(EulerRot::XYZ, v.x.to_radians(), v.y.to_radians(), v.z.to_radians());
                Ok(())
            }
            ("Name", PropertyValue::String(s)) => { self.name = s; Ok(()) }
            ("CFrame", _) => Err("CFrame is computed (read-only)".to_string()),
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Name".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Position".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
            PropertyDescriptor { name: "Orientation".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for WeldConstraint
// ============================================================================

impl PropertyAccess for WeldConstraint {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Part0" => Some(PropertyValue::Int(self.part0.unwrap_or(0) as i32)),
            "Part1" => Some(PropertyValue::Int(self.part1.unwrap_or(0) as i32)),
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Part0", PropertyValue::Int(id)) => { self.part0 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("Part1", PropertyValue::Int(id)) => { self.part1 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Part0".to_string(), property_type: "BasePart?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Part1".to_string(), property_type: "BasePart?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Motor6D
// ============================================================================

impl PropertyAccess for Motor6D {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Part0" => Some(PropertyValue::Int(self.part0.unwrap_or(0) as i32)),
            "Part1" => Some(PropertyValue::Int(self.part1.unwrap_or(0) as i32)),
            "DesiredAngle" => Some(PropertyValue::Float(self.desired_angle)),
            "MaxVelocity" => Some(PropertyValue::Float(self.max_velocity)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Part0", PropertyValue::Int(id)) => { self.part0 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("Part1", PropertyValue::Int(id)) => { self.part1 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("DesiredAngle", PropertyValue::Float(f)) => { self.desired_angle = f; Ok(()) }
            ("MaxVelocity", PropertyValue::Float(f)) => { self.max_velocity = f.max(0.0); Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Part0".to_string(), property_type: "BasePart?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Part1".to_string(), property_type: "BasePart?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "DesiredAngle".to_string(), property_type: "float".to_string(), read_only: false, category: "Motion".to_string() },
            PropertyDescriptor { name: "MaxVelocity".to_string(), property_type: "float".to_string(), read_only: false, category: "Motion".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Sound
// ============================================================================

impl PropertyAccess for Sound {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "SoundId" => Some(PropertyValue::String(self.sound_id.clone())),
            "Volume" => Some(PropertyValue::Float(self.volume)),
            "Pitch" => Some(PropertyValue::Float(self.pitch)),
            "Looped" => Some(PropertyValue::Bool(self.looped)),
            "Playing" => Some(PropertyValue::Bool(self.playing)),
            "Spatial" => Some(PropertyValue::Bool(self.spatial)),
            "RollOffMaxDistance" => Some(PropertyValue::Float(self.roll_off_max_distance)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("SoundId", PropertyValue::String(s)) => { self.sound_id = s; Ok(()) }
            ("Volume", PropertyValue::Float(f)) => { self.volume = f.clamp(0.0, 1.0); Ok(()) }
            ("Pitch", PropertyValue::Float(f)) => { self.pitch = f.clamp(0.5, 2.0); Ok(()) }
            ("Looped", PropertyValue::Bool(b)) => { self.looped = b; Ok(()) }
            ("Playing", PropertyValue::Bool(b)) => { self.playing = b; Ok(()) }
            ("Spatial", PropertyValue::Bool(b)) => { self.spatial = b; Ok(()) }
            ("RollOffMaxDistance", PropertyValue::Float(f)) => { self.roll_off_max_distance = f.max(0.0); Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "SoundId".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Volume".to_string(), property_type: "float".to_string(), read_only: false, category: "Playback".to_string() },
            PropertyDescriptor { name: "Looped".to_string(), property_type: "bool".to_string(), read_only: false, category: "Playback".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for EustressCamera
// ============================================================================

impl PropertyAccess for EustressCamera {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "FieldOfView" => Some(PropertyValue::Float(self.field_of_view)),
            "NearClipPlane" => Some(PropertyValue::Float(self.near_clip)),
            "FarClipPlane" => Some(PropertyValue::Float(self.far_clip)),
            "CameraType" => Some(PropertyValue::Enum(self.camera_type.clone())),
            "CameraSubject" => Some(PropertyValue::Int(self.camera_subject.unwrap_or(0) as i32)),
            "MaxZoomDistance" => Some(PropertyValue::Float(self.max_zoom_distance)),
            "MinZoomDistance" => Some(PropertyValue::Float(self.min_zoom_distance)),
            "HeadLocked" => Some(PropertyValue::Bool(self.head_locked)),
            "HeadScale" => Some(PropertyValue::Float(self.head_scale)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("FieldOfView", PropertyValue::Float(f)) => { self.field_of_view = f.clamp(1.0, 120.0); Ok(()) }
            ("NearClipPlane", PropertyValue::Float(f)) => { self.near_clip = f.clamp(0.001, 100.0); Ok(()) }
            ("FarClipPlane", PropertyValue::Float(f)) => { self.far_clip = f.clamp(100.0, 1000000.0); Ok(()) }
            ("CameraType", PropertyValue::Enum(s)) => { self.camera_type = s; Ok(()) }
            ("CameraSubject", PropertyValue::Int(id)) => { self.camera_subject = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("MaxZoomDistance", PropertyValue::Float(f)) => { self.max_zoom_distance = f.clamp(0.5, 10000.0); Ok(()) }
            ("MinZoomDistance", PropertyValue::Float(f)) => { self.min_zoom_distance = f.clamp(0.5, 100.0); Ok(()) }
            ("HeadLocked", PropertyValue::Bool(b)) => { self.head_locked = b; Ok(()) }
            ("HeadScale", PropertyValue::Float(f)) => { self.head_scale = f.clamp(0.1, 10.0); Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "FieldOfView".to_string(), property_type: "float".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "NearClipPlane".to_string(), property_type: "float".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "FarClipPlane".to_string(), property_type: "float".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "CameraType".to_string(), property_type: "CameraType".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "MaxZoomDistance".to_string(), property_type: "float".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MinZoomDistance".to_string(), property_type: "float".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "HeadLocked".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "HeadScale".to_string(), property_type: "float".to_string(), read_only: false, category: "Behavior".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for EustressPointLight
// ============================================================================

impl PropertyAccess for EustressPointLight {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "Color" => Some(PropertyValue::Color(self.color)),
            "Range" => Some(PropertyValue::Float(self.range)),
            "Shadows" => Some(PropertyValue::Bool(self.shadows)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Brightness", PropertyValue::Float(f)) => { self.brightness = f.max(0.0); Ok(()) }
            ("Color", PropertyValue::Color(c)) => { self.color = c; Ok(()) }
            ("Range", PropertyValue::Float(f)) => { self.range = f.max(0.0); Ok(()) }
            ("Shadows", PropertyValue::Bool(b)) => { self.shadows = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Color".to_string(), property_type: "Color".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Range".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Shadows".to_string(), property_type: "bool".to_string(), read_only: false, category: "Light".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for EustressSpotLight
// ============================================================================

impl PropertyAccess for EustressSpotLight {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "Color" => Some(PropertyValue::Color(self.color)),
            "Range" => Some(PropertyValue::Float(self.range)),
            "Shadows" => Some(PropertyValue::Bool(self.shadows)),
            "Angle" => Some(PropertyValue::Float(self.angle)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Brightness", PropertyValue::Float(f)) => { self.brightness = f.max(0.0); Ok(()) }
            ("Color", PropertyValue::Color(c)) => { self.color = c; Ok(()) }
            ("Range", PropertyValue::Float(f)) => { self.range = f.max(0.0); Ok(()) }
            ("Shadows", PropertyValue::Bool(b)) => { self.shadows = b; Ok(()) }
            ("Angle", PropertyValue::Float(f)) => { self.angle = f.clamp(0.0, 180.0); Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Color".to_string(), property_type: "Color".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Angle".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for MeshPart
// ============================================================================

impl PropertyAccess for MeshPart {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "MeshId" => Some(PropertyValue::String(self.mesh_id.clone())),
            "TextureID" => Some(PropertyValue::String(self.texture_id.clone())),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("MeshId", PropertyValue::String(s)) => { self.mesh_id = s; Ok(()) }
            ("TextureID", PropertyValue::String(s)) => { self.texture_id = s; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "MeshId".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "TextureID".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for ParticleEmitter
// ============================================================================

impl PropertyAccess for ParticleEmitter {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            // Emission
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            "Rate" => Some(PropertyValue::Float(self.rate)),
            "MaxParticles" => Some(PropertyValue::Int(self.max_particles as i32)),
            "BurstMode" => Some(PropertyValue::Bool(self.burst_mode)),
            "BurstCount" => Some(PropertyValue::Int(self.burst_count as i32)),
            "BurstInterval" => Some(PropertyValue::Float(self.burst_interval)),
            // Emission Shape
            "EmissionShape" => Some(PropertyValue::Enum(format!("{:?}", self.emission_shape))),
            "EmissionSize" => Some(PropertyValue::Vector3(self.emission_size)),
            "SurfaceOnly" => Some(PropertyValue::Bool(self.surface_only)),
            // Lifetime
            "LifetimeMin" => Some(PropertyValue::Float(self.lifetime.0)),
            "LifetimeMax" => Some(PropertyValue::Float(self.lifetime.1)),
            "EmitDelay" => Some(PropertyValue::Float(self.emit_delay)),
            "EmitDuration" => Some(PropertyValue::Float(self.emit_duration)),
            // Movement
            "SpeedMin" => Some(PropertyValue::Float(self.speed.0)),
            "SpeedMax" => Some(PropertyValue::Float(self.speed.1)),
            "SpreadAngle" => Some(PropertyValue::Vector3(Vec3::new(self.spread_angle.x, self.spread_angle.y, 0.0))),
            "Direction" => Some(PropertyValue::Vector3(self.direction)),
            "Acceleration" => Some(PropertyValue::Vector3(self.acceleration)),
            "UseGravity" => Some(PropertyValue::Bool(self.use_gravity)),
            "GravityScale" => Some(PropertyValue::Float(self.gravity_scale)),
            "Drag" => Some(PropertyValue::Float(self.drag)),
            "VelocityInheritance" => Some(PropertyValue::Float(self.velocity_inheritance)),
            // Collision
            "CollisionEnabled" => Some(PropertyValue::Bool(self.collision_enabled)),
            "CollisionBounce" => Some(PropertyValue::Float(self.collision_bounce)),
            "CollisionFriction" => Some(PropertyValue::Float(self.collision_friction)),
            "CollisionKill" => Some(PropertyValue::Bool(self.collision_kill)),
            "CollisionRadius" => Some(PropertyValue::Float(self.collision_radius)),
            // Texture
            "Texture" => Some(PropertyValue::String(self.texture.clone())),
            "FlipbookColumns" => Some(PropertyValue::Int(self.flipbook_columns as i32)),
            "FlipbookRows" => Some(PropertyValue::Int(self.flipbook_rows as i32)),
            "FlipbookFPS" => Some(PropertyValue::Float(self.flipbook_fps)),
            "FlipbookRandomStart" => Some(PropertyValue::Bool(self.flipbook_random_start)),
            // Size
            "SizeMin" => Some(PropertyValue::Float(self.size.0)),
            "SizeMax" => Some(PropertyValue::Float(self.size.1)),
            "UniformSize" => Some(PropertyValue::Bool(self.uniform_size)),
            "StretchFactor" => Some(PropertyValue::Float(self.stretch_factor)),
            // Color
            "BlendMode" => Some(PropertyValue::Enum(format!("{:?}", self.blend_mode))),
            // Rotation
            "RotationMin" => Some(PropertyValue::Float(self.rotation.0)),
            "RotationMax" => Some(PropertyValue::Float(self.rotation.1)),
            "RotationSpeedMin" => Some(PropertyValue::Float(self.rotation_speed.0)),
            "RotationSpeedMax" => Some(PropertyValue::Float(self.rotation_speed.1)),
            "FaceCamera" => Some(PropertyValue::Bool(self.face_camera)),
            // Lighting
            "LightEmission" => Some(PropertyValue::Bool(self.light_emission)),
            "LightRange" => Some(PropertyValue::Float(self.light_range)),
            "LightBrightness" => Some(PropertyValue::Float(self.light_brightness)),
            // Advanced
            "WindInfluence" => Some(PropertyValue::Float(self.wind_influence)),
            "NoiseStrength" => Some(PropertyValue::Float(self.noise_strength)),
            "NoiseFrequency" => Some(PropertyValue::Float(self.noise_frequency)),
            "DepthSort" => Some(PropertyValue::Bool(self.depth_sort)),
            "LocalSpace" => Some(PropertyValue::Bool(self.local_space)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            // Emission
            ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }
            ("Rate", PropertyValue::Float(f)) => { self.rate = f.max(0.0); Ok(()) }
            ("MaxParticles", PropertyValue::Int(i)) => { self.max_particles = i.max(1) as u32; Ok(()) }
            ("BurstMode", PropertyValue::Bool(b)) => { self.burst_mode = b; Ok(()) }
            ("BurstCount", PropertyValue::Int(i)) => { self.burst_count = i.max(1) as u32; Ok(()) }
            ("BurstInterval", PropertyValue::Float(f)) => { self.burst_interval = f.max(0.01); Ok(()) }
            // Emission Shape
            ("EmissionShape", PropertyValue::Enum(s)) => {
                self.emission_shape = match s.as_str() {
                    "Point" => EmissionShape::Point,
                    "Sphere" => EmissionShape::Sphere,
                    "Box" => EmissionShape::Box,
                    "Cone" => EmissionShape::Cone,
                    "Cylinder" => EmissionShape::Cylinder,
                    "Ring" => EmissionShape::Ring,
                    "Disc" => EmissionShape::Disc,
                    _ => return Err(format!("Unknown emission shape: {}", s)),
                };
                Ok(())
            }
            ("EmissionSize", PropertyValue::Vector3(v)) => { self.emission_size = v; Ok(()) }
            ("SurfaceOnly", PropertyValue::Bool(b)) => { self.surface_only = b; Ok(()) }
            // Lifetime
            ("LifetimeMin", PropertyValue::Float(f)) => { self.lifetime.0 = f.max(0.01); Ok(()) }
            ("LifetimeMax", PropertyValue::Float(f)) => { self.lifetime.1 = f.max(self.lifetime.0); Ok(()) }
            ("EmitDelay", PropertyValue::Float(f)) => { self.emit_delay = f.max(0.0); Ok(()) }
            ("EmitDuration", PropertyValue::Float(f)) => { self.emit_duration = f.max(0.0); Ok(()) }
            // Movement
            ("SpeedMin", PropertyValue::Float(f)) => { self.speed.0 = f.max(0.0); Ok(()) }
            ("SpeedMax", PropertyValue::Float(f)) => { self.speed.1 = f.max(self.speed.0); Ok(()) }
            ("SpreadAngle", PropertyValue::Vector3(v)) => { self.spread_angle = Vec2::new(v.x.clamp(0.0, 180.0), v.y.clamp(0.0, 180.0)); Ok(()) }
            ("Direction", PropertyValue::Vector3(v)) => { self.direction = v.normalize_or_zero(); Ok(()) }
            ("Acceleration", PropertyValue::Vector3(v)) => { self.acceleration = v; Ok(()) }
            ("UseGravity", PropertyValue::Bool(b)) => { self.use_gravity = b; Ok(()) }
            ("GravityScale", PropertyValue::Float(f)) => { self.gravity_scale = f; Ok(()) }
            ("Drag", PropertyValue::Float(f)) => { self.drag = f.clamp(0.0, 1.0); Ok(()) }
            ("VelocityInheritance", PropertyValue::Float(f)) => { self.velocity_inheritance = f.clamp(0.0, 1.0); Ok(()) }
            // Collision
            ("CollisionEnabled", PropertyValue::Bool(b)) => { self.collision_enabled = b; Ok(()) }
            ("CollisionBounce", PropertyValue::Float(f)) => { self.collision_bounce = f.clamp(0.0, 1.0); Ok(()) }
            ("CollisionFriction", PropertyValue::Float(f)) => { self.collision_friction = f.clamp(0.0, 1.0); Ok(()) }
            ("CollisionKill", PropertyValue::Bool(b)) => { self.collision_kill = b; Ok(()) }
            ("CollisionRadius", PropertyValue::Float(f)) => { self.collision_radius = f.max(0.1); Ok(()) }
            // Texture
            ("Texture", PropertyValue::String(s)) => { self.texture = s; Ok(()) }
            ("FlipbookColumns", PropertyValue::Int(i)) => { self.flipbook_columns = i.max(1) as u32; Ok(()) }
            ("FlipbookRows", PropertyValue::Int(i)) => { self.flipbook_rows = i.max(1) as u32; Ok(()) }
            ("FlipbookFPS", PropertyValue::Float(f)) => { self.flipbook_fps = f.max(0.0); Ok(()) }
            ("FlipbookRandomStart", PropertyValue::Bool(b)) => { self.flipbook_random_start = b; Ok(()) }
            // Size
            ("SizeMin", PropertyValue::Float(f)) => { self.size.0 = f.max(0.01); Ok(()) }
            ("SizeMax", PropertyValue::Float(f)) => { self.size.1 = f.max(self.size.0); Ok(()) }
            ("UniformSize", PropertyValue::Bool(b)) => { self.uniform_size = b; Ok(()) }
            ("StretchFactor", PropertyValue::Float(f)) => { self.stretch_factor = f.max(0.0); Ok(()) }
            // Color
            ("BlendMode", PropertyValue::Enum(s)) => {
                self.blend_mode = match s.as_str() {
                    "Alpha" => ParticleBlendMode::Alpha,
                    "Additive" => ParticleBlendMode::Additive,
                    "Multiply" => ParticleBlendMode::Multiply,
                    "Premultiplied" => ParticleBlendMode::Premultiplied,
                    _ => return Err(format!("Unknown blend mode: {}", s)),
                };
                Ok(())
            }
            // Rotation
            ("RotationMin", PropertyValue::Float(f)) => { self.rotation.0 = f; Ok(()) }
            ("RotationMax", PropertyValue::Float(f)) => { self.rotation.1 = f; Ok(()) }
            ("RotationSpeedMin", PropertyValue::Float(f)) => { self.rotation_speed.0 = f; Ok(()) }
            ("RotationSpeedMax", PropertyValue::Float(f)) => { self.rotation_speed.1 = f; Ok(()) }
            ("FaceCamera", PropertyValue::Bool(b)) => { self.face_camera = b; Ok(()) }
            // Lighting
            ("LightEmission", PropertyValue::Bool(b)) => { self.light_emission = b; Ok(()) }
            ("LightRange", PropertyValue::Float(f)) => { self.light_range = f.max(0.0); Ok(()) }
            ("LightBrightness", PropertyValue::Float(f)) => { self.light_brightness = f.max(0.0); Ok(()) }
            // Advanced
            ("WindInfluence", PropertyValue::Float(f)) => { self.wind_influence = f.clamp(0.0, 1.0); Ok(()) }
            ("NoiseStrength", PropertyValue::Float(f)) => { self.noise_strength = f.max(0.0); Ok(()) }
            ("NoiseFrequency", PropertyValue::Float(f)) => { self.noise_frequency = f.max(0.01); Ok(()) }
            ("DepthSort", PropertyValue::Bool(b)) => { self.depth_sort = b; Ok(()) }
            ("LocalSpace", PropertyValue::Bool(b)) => { self.local_space = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            // Emission
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Emission".to_string() },
            PropertyDescriptor { name: "Rate".to_string(), property_type: "float".to_string(), read_only: false, category: "Emission".to_string() },
            PropertyDescriptor { name: "MaxParticles".to_string(), property_type: "int".to_string(), read_only: false, category: "Emission".to_string() },
            PropertyDescriptor { name: "BurstMode".to_string(), property_type: "bool".to_string(), read_only: false, category: "Emission".to_string() },
            PropertyDescriptor { name: "BurstCount".to_string(), property_type: "int".to_string(), read_only: false, category: "Emission".to_string() },
            PropertyDescriptor { name: "BurstInterval".to_string(), property_type: "float".to_string(), read_only: false, category: "Emission".to_string() },
            // Shape
            PropertyDescriptor { name: "EmissionShape".to_string(), property_type: "enum".to_string(), read_only: false, category: "Shape".to_string() },
            PropertyDescriptor { name: "EmissionSize".to_string(), property_type: "vector3".to_string(), read_only: false, category: "Shape".to_string() },
            PropertyDescriptor { name: "SurfaceOnly".to_string(), property_type: "bool".to_string(), read_only: false, category: "Shape".to_string() },
            // Lifetime
            PropertyDescriptor { name: "LifetimeMin".to_string(), property_type: "float".to_string(), read_only: false, category: "Lifetime".to_string() },
            PropertyDescriptor { name: "LifetimeMax".to_string(), property_type: "float".to_string(), read_only: false, category: "Lifetime".to_string() },
            PropertyDescriptor { name: "EmitDelay".to_string(), property_type: "float".to_string(), read_only: false, category: "Lifetime".to_string() },
            PropertyDescriptor { name: "EmitDuration".to_string(), property_type: "float".to_string(), read_only: false, category: "Lifetime".to_string() },
            // Movement
            PropertyDescriptor { name: "SpeedMin".to_string(), property_type: "float".to_string(), read_only: false, category: "Movement".to_string() },
            PropertyDescriptor { name: "SpeedMax".to_string(), property_type: "float".to_string(), read_only: false, category: "Movement".to_string() },
            PropertyDescriptor { name: "SpreadAngle".to_string(), property_type: "vector3".to_string(), read_only: false, category: "Movement".to_string() },
            PropertyDescriptor { name: "Direction".to_string(), property_type: "vector3".to_string(), read_only: false, category: "Movement".to_string() },
            PropertyDescriptor { name: "Acceleration".to_string(), property_type: "vector3".to_string(), read_only: false, category: "Movement".to_string() },
            PropertyDescriptor { name: "UseGravity".to_string(), property_type: "bool".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "GravityScale".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "Drag".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "VelocityInheritance".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            // Collision
            PropertyDescriptor { name: "CollisionEnabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Collision".to_string() },
            PropertyDescriptor { name: "CollisionBounce".to_string(), property_type: "float".to_string(), read_only: false, category: "Collision".to_string() },
            PropertyDescriptor { name: "CollisionFriction".to_string(), property_type: "float".to_string(), read_only: false, category: "Collision".to_string() },
            PropertyDescriptor { name: "CollisionKill".to_string(), property_type: "bool".to_string(), read_only: false, category: "Collision".to_string() },
            PropertyDescriptor { name: "CollisionRadius".to_string(), property_type: "float".to_string(), read_only: false, category: "Collision".to_string() },
            // Texture
            PropertyDescriptor { name: "Texture".to_string(), property_type: "string".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "FlipbookColumns".to_string(), property_type: "int".to_string(), read_only: false, category: "Flipbook".to_string() },
            PropertyDescriptor { name: "FlipbookRows".to_string(), property_type: "int".to_string(), read_only: false, category: "Flipbook".to_string() },
            PropertyDescriptor { name: "FlipbookFPS".to_string(), property_type: "float".to_string(), read_only: false, category: "Flipbook".to_string() },
            PropertyDescriptor { name: "FlipbookRandomStart".to_string(), property_type: "bool".to_string(), read_only: false, category: "Flipbook".to_string() },
            // Size
            PropertyDescriptor { name: "SizeMin".to_string(), property_type: "float".to_string(), read_only: false, category: "Size".to_string() },
            PropertyDescriptor { name: "SizeMax".to_string(), property_type: "float".to_string(), read_only: false, category: "Size".to_string() },
            PropertyDescriptor { name: "UniformSize".to_string(), property_type: "bool".to_string(), read_only: false, category: "Size".to_string() },
            PropertyDescriptor { name: "StretchFactor".to_string(), property_type: "float".to_string(), read_only: false, category: "Size".to_string() },
            // Color
            PropertyDescriptor { name: "BlendMode".to_string(), property_type: "enum".to_string(), read_only: false, category: "Color".to_string() },
            // Rotation
            PropertyDescriptor { name: "RotationMin".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "RotationMax".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "RotationSpeedMin".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "RotationSpeedMax".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "FaceCamera".to_string(), property_type: "bool".to_string(), read_only: false, category: "Rotation".to_string() },
            // Lighting
            PropertyDescriptor { name: "LightEmission".to_string(), property_type: "bool".to_string(), read_only: false, category: "Lighting".to_string() },
            PropertyDescriptor { name: "LightRange".to_string(), property_type: "float".to_string(), read_only: false, category: "Lighting".to_string() },
            PropertyDescriptor { name: "LightBrightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Lighting".to_string() },
            // Advanced
            PropertyDescriptor { name: "WindInfluence".to_string(), property_type: "float".to_string(), read_only: false, category: "Advanced".to_string() },
            PropertyDescriptor { name: "NoiseStrength".to_string(), property_type: "float".to_string(), read_only: false, category: "Advanced".to_string() },
            PropertyDescriptor { name: "NoiseFrequency".to_string(), property_type: "float".to_string(), read_only: false, category: "Advanced".to_string() },
            PropertyDescriptor { name: "DepthSort".to_string(), property_type: "bool".to_string(), read_only: false, category: "Advanced".to_string() },
            PropertyDescriptor { name: "LocalSpace".to_string(), property_type: "bool".to_string(), read_only: false, category: "Advanced".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Animator
// ============================================================================

impl PropertyAccess for Animator {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "PreferredAnimationSpeed" => Some(PropertyValue::Float(self.preferred_animation_speed)),
            "RigType" => Some(PropertyValue::Enum(format!("{:?}", self.rig_type))),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("PreferredAnimationSpeed", PropertyValue::Float(f)) => { self.preferred_animation_speed = f.clamp(0.1, 10.0); Ok(()) }
            ("RigType", PropertyValue::Enum(s)) => {
                self.rig_type = match s.as_str() {
                    "Humanoid" => RigType::Humanoid,
                    "R15" => RigType::R15,
                    "R6" => RigType::R6,
                    "Custom" => RigType::Custom,
                    _ => return Err(format!("Invalid RigType: {}", s)),
                };
                Ok(())
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "PreferredAnimationSpeed".to_string(), property_type: "float".to_string(), read_only: false, category: "Animation".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Folder
// ============================================================================

impl PropertyAccess for Folder {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "AssemblyMass" => Some(PropertyValue::Float(self.assembly_mass)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, _value: PropertyValue) -> Result<(), String> { 
        match name {
            "AssemblyMass" => Err("AssemblyMass is computed (read-only)".to_string()),
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "AssemblyMass".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Sky
// ============================================================================

impl PropertyAccess for Sky {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "StarCount" => Some(PropertyValue::Int(self.star_count as i32)),
            "CelestialBodiesShown" => Some(PropertyValue::Bool(self.celestial_bodies_shown)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("StarCount", PropertyValue::Int(i)) => { self.star_count = i.max(0) as u32; Ok(()) }
            ("CelestialBodiesShown", PropertyValue::Bool(b)) => { self.celestial_bodies_shown = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "StarCount".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "CelestialBodiesShown".to_string(), property_type: "bool".to_string(), read_only: false, category: "Appearance".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Terrain
// ============================================================================

impl PropertyAccess for Terrain {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "WaterWaveSize" => Some(PropertyValue::Float(self.water_wave_size)),
            "WaterTransparency" => Some(PropertyValue::Float(self.water_transparency)),
            "WaterColor" => Some(PropertyValue::Color(self.water_color)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("WaterWaveSize", PropertyValue::Float(f)) => { self.water_wave_size = f.clamp(0.0, 1.0); Ok(()) }
            ("WaterTransparency", PropertyValue::Float(f)) => { self.water_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("WaterColor", PropertyValue::Color(c)) => { self.water_color = c; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "WaterWaveSize".to_string(), property_type: "float".to_string(), read_only: false, category: "Water".to_string() },
            PropertyDescriptor { name: "WaterTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Water".to_string() },
            PropertyDescriptor { name: "WaterColor".to_string(), property_type: "Color".to_string(), read_only: false, category: "Water".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Beam
// ============================================================================

impl PropertyAccess for Beam {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Attachment0" => Some(PropertyValue::Int(self.attachment0.unwrap_or(0) as i32)),
            "Attachment1" => Some(PropertyValue::Int(self.attachment1.unwrap_or(0) as i32)),
            "Color" => Some(PropertyValue::Color(self.color_at(0.0))),
            "Width0" => Some(PropertyValue::Float(self.width0)),
            "Width1" => Some(PropertyValue::Float(self.width1)),
            "Segments" => Some(PropertyValue::Int(self.segments as i32)),
            "CurveSize0" => Some(PropertyValue::Float(self.curve_size0)),
            "CurveSize1" => Some(PropertyValue::Float(self.curve_size1)),
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "LightEmission" => Some(PropertyValue::Float(self.light_emission)),
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Attachment0", PropertyValue::Int(id)) => { self.attachment0 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("Attachment1", PropertyValue::Int(id)) => { self.attachment1 = if id > 0 { Some(id as u32) } else { None }; Ok(()) }
            ("Color", PropertyValue::Color(c)) => { self.color_sequence = vec![(0.0, c), (1.0, c)]; Ok(()) }
            ("Width0", PropertyValue::Float(w)) => { self.width0 = w; Ok(()) }
            ("Width1", PropertyValue::Float(w)) => { self.width1 = w; Ok(()) }
            ("Segments", PropertyValue::Int(s)) => { self.segments = s as u32; Ok(()) }
            ("CurveSize0", PropertyValue::Float(c)) => { self.curve_size0 = c; Ok(()) }
            ("CurveSize1", PropertyValue::Float(c)) => { self.curve_size1 = c; Ok(()) }
            ("Brightness", PropertyValue::Float(b)) => { self.brightness = b; Ok(()) }
            ("LightEmission", PropertyValue::Float(l)) => { self.light_emission = l; Ok(()) }
            ("Enabled", PropertyValue::Bool(e)) => { self.enabled = e; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Attachment0".to_string(), property_type: "Attachment?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Attachment1".to_string(), property_type: "Attachment?".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Color".to_string(), property_type: "Color".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Width0".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Width1".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Segments".to_string(), property_type: "int".to_string(), read_only: false, category: "Shape".to_string() },
            PropertyDescriptor { name: "CurveSize0".to_string(), property_type: "float".to_string(), read_only: false, category: "Shape".to_string() },
            PropertyDescriptor { name: "CurveSize1".to_string(), property_type: "float".to_string(), read_only: false, category: "Shape".to_string() },
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "LightEmission".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for SurfaceLight
// ============================================================================

impl PropertyAccess for SurfaceLight {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "Color" => Some(PropertyValue::Color(self.color)),
            "Range" => Some(PropertyValue::Float(self.range)),
            "Shadows" => Some(PropertyValue::Bool(self.shadows)),
            "Face" => Some(PropertyValue::String(self.face.clone())),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Brightness", PropertyValue::Float(f)) => { self.brightness = f.max(0.0); Ok(()) }
            ("Color", PropertyValue::Color(c)) => { self.color = c; Ok(()) }
            ("Range", PropertyValue::Float(f)) => { self.range = f.max(0.0); Ok(()) }
            ("Shadows", PropertyValue::Bool(b)) => { self.shadows = b; Ok(()) }
            ("Face", PropertyValue::String(s)) => { self.face = s; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Color".to_string(), property_type: "Color".to_string(), read_only: false, category: "Light".to_string() },
            PropertyDescriptor { name: "Face".to_string(), property_type: "Face".to_string(), read_only: false, category: "Light".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for SpecialMesh
// ============================================================================

impl PropertyAccess for SpecialMesh {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "MeshType" => Some(PropertyValue::Enum(format!("{:?}", self.mesh_type))),
            "Scale" => Some(PropertyValue::Vector3(self.scale)),
            "MeshId" => Some(PropertyValue::String(self.mesh_id.clone())),
            "Offset" => Some(PropertyValue::Vector3(self.offset)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("MeshType", PropertyValue::Enum(s)) => {
                self.mesh_type = match s.as_str() {
                    "FileMesh" => MeshType::FileMesh,
                    "Head" => MeshType::Head,
                    "Torso" => MeshType::Torso,
                    "Brick" => MeshType::Brick,
                    "Sphere" => MeshType::Sphere,
                    "Cylinder" => MeshType::Cylinder,
                    _ => return Err(format!("Invalid MeshType: {}", s)),
                };
                Ok(())
            }
            ("Scale", PropertyValue::Vector3(v)) => { self.scale = v; Ok(()) }
            ("MeshId", PropertyValue::String(s)) => { self.mesh_id = s; Ok(()) }
            ("Offset", PropertyValue::Vector3(v)) => { self.offset = v; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "MeshType".to_string(), property_type: "MeshType".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Scale".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Transform".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for Decal
// ============================================================================

impl PropertyAccess for Decal {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Texture" => Some(PropertyValue::String(self.texture.clone())),
            "Face" => Some(PropertyValue::Enum(format!("{:?}", self.face))),
            "Transparency" => Some(PropertyValue::Float(self.transparency)),
            "ZIndex" => Some(PropertyValue::Int(self.z_index)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Texture", PropertyValue::String(s)) => { self.texture = s; Ok(()) }
            ("Face", PropertyValue::Enum(s)) => {
                self.face = match s.as_str() {
                    "Top" => Face::Top,
                    "Bottom" => Face::Bottom,
                    "Front" => Face::Front,
                    "Back" => Face::Back,
                    "Left" => Face::Left,
                    "Right" => Face::Right,
                    _ => return Err(format!("Invalid Face: {}", s)),
                };
                Ok(())
            }
            ("Transparency", PropertyValue::Float(f)) => { self.transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("ZIndex", PropertyValue::Int(i)) => { self.z_index = i; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Texture".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Face".to_string(), property_type: "Face".to_string(), read_only: false, category: "Data".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for KeyframeSequence
// ============================================================================

impl PropertyAccess for KeyframeSequence {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Looped" => Some(PropertyValue::Bool(self.looped)),
            "Priority" => Some(PropertyValue::Enum(format!("{:?}", self.priority))),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Looped", PropertyValue::Bool(b)) => { self.looped = b; Ok(()) }
            ("Priority", PropertyValue::Enum(s)) => {
                self.priority = match s.as_str() {
                    "Core" => AnimationPriority::Core,
                    "Idle" => AnimationPriority::Idle,
                    "Movement" => AnimationPriority::Movement,
                    "Action" => AnimationPriority::Action,
                    _ => return Err(format!("Invalid AnimationPriority: {}", s)),
                };
                Ok(())
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Looped".to_string(), property_type: "bool".to_string(), read_only: false, category: "Animation".to_string() },
            PropertyDescriptor { name: "Priority".to_string(), property_type: "AnimationPriority".to_string(), read_only: false, category: "Animation".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for UnionOperation
// ============================================================================

impl PropertyAccess for UnionOperation {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Operation" => Some(PropertyValue::Enum(format!("{:?}", self.operation))),
            "UsePartColor" => Some(PropertyValue::Bool(self.use_part_color)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Operation", PropertyValue::Enum(s)) => {
                self.operation = match s.as_str() {
                    "Union" => CSGOperation::Union,
                    "Subtract" => CSGOperation::Subtract,
                    "Intersect" => CSGOperation::Intersect,
                    _ => return Err(format!("Invalid CSGOperation: {}", s)),
                };
                Ok(())
            }
            ("UsePartColor", PropertyValue::Bool(b)) => { self.use_part_color = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Operation".to_string(), property_type: "CSGOperation".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "UsePartColor".to_string(), property_type: "bool".to_string(), read_only: false, category: "Appearance".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for BillboardGui
// ============================================================================

impl PropertyAccess for BillboardGui {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Active" => Some(PropertyValue::Bool(self.active)),
            "AlwaysOnTop" => Some(PropertyValue::Bool(self.always_on_top)),
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            "ClipsDescendants" => Some(PropertyValue::Bool(self.clips_descendants)),
            "MaxDistance" => Some(PropertyValue::Float(self.max_distance)),
            "DistanceLowerLimit" => Some(PropertyValue::Float(self.distance_lower_limit)),
            "DistanceUpperLimit" => Some(PropertyValue::Float(self.distance_upper_limit)),
            "DistanceStep" => Some(PropertyValue::Float(self.distance_step)),
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "LightInfluence" => Some(PropertyValue::Float(self.light_influence)),
            "Size" => Some(PropertyValue::Vector2(self.size)),
            "UnitsOffset" => Some(PropertyValue::Vector3(Vec3::new(self.units_offset[0], self.units_offset[1], self.units_offset[2]))),
            "CurrentDistance" => Some(PropertyValue::Float(self.current_distance)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Active", PropertyValue::Bool(v)) => { self.active = v; Ok(()) }
            ("AlwaysOnTop", PropertyValue::Bool(v)) => { self.always_on_top = v; Ok(()) }
            ("Enabled", PropertyValue::Bool(v)) => { self.enabled = v; Ok(()) }
            ("ClipsDescendants", PropertyValue::Bool(v)) => { self.clips_descendants = v; Ok(()) }
            ("MaxDistance", PropertyValue::Float(v)) => { self.max_distance = v.max(0.0); Ok(()) }
            ("DistanceLowerLimit", PropertyValue::Float(v)) => { self.distance_lower_limit = v.max(0.0); Ok(()) }
            ("DistanceUpperLimit", PropertyValue::Float(v)) => { self.distance_upper_limit = v.max(0.0); Ok(()) }
            ("DistanceStep", PropertyValue::Float(v)) => { self.distance_step = v.max(0.0); Ok(()) }
            ("Brightness", PropertyValue::Float(v)) => { self.brightness = v.max(0.0); Ok(()) }
            ("LightInfluence", PropertyValue::Float(v)) => { self.light_influence = v.clamp(0.0, 1.0); Ok(()) }
            ("Size", PropertyValue::Vector2(v)) => { self.size = v; Ok(()) }
            ("UnitsOffset", PropertyValue::Vector3(v)) => { self.units_offset = [v.x, v.y, v.z]; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Active".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "AlwaysOnTop".to_string(), property_type: "bool".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "ClipsDescendants".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MaxDistance".to_string(), property_type: "float".to_string(), read_only: false, category: "Distance".to_string() },
            PropertyDescriptor { name: "DistanceLowerLimit".to_string(), property_type: "float".to_string(), read_only: false, category: "Distance".to_string() },
            PropertyDescriptor { name: "DistanceUpperLimit".to_string(), property_type: "float".to_string(), read_only: false, category: "Distance".to_string() },
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "LightInfluence".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Size".to_string(), property_type: "Vector2".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "UnitsOffset".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "CurrentDistance".to_string(), property_type: "float".to_string(), read_only: true, category: "Data".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess Implementation for TextLabel
// ============================================================================

impl PropertyAccess for TextLabel {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Text" => Some(PropertyValue::String(self.text.clone())),
            "RichText" => Some(PropertyValue::Bool(self.rich_text)),
            "TextScaled" => Some(PropertyValue::Bool(self.text_scaled)),
            "TextWrapped" => Some(PropertyValue::Bool(self.text_wrapped)),
            "Font" => Some(PropertyValue::String(format!("{:?}", self.font))),
            "FontSize" => Some(PropertyValue::Float(self.font_size)),
            "LineHeight" => Some(PropertyValue::Float(self.line_height)),
            "TextColor3" => Some(PropertyValue::Color3(self.text_color3)),
            "TextTransparency" => Some(PropertyValue::Float(self.text_transparency)),
            "TextStrokeColor3" => Some(PropertyValue::Color3(self.text_stroke_color3)),
            "TextStrokeTransparency" => Some(PropertyValue::Float(self.text_stroke_transparency)),
            "BackgroundColor3" => Some(PropertyValue::Color3(self.background_color3)),
            "BackgroundTransparency" => Some(PropertyValue::Float(self.background_transparency)),
            "BorderColor3" => Some(PropertyValue::Color3(self.border_color3)),
            "BorderSizePixel" => Some(PropertyValue::Int(self.border_size_pixel)),
            "Size" => Some(PropertyValue::Vector2(self.size)),
            "Position" => Some(PropertyValue::Vector2(self.position)),
            "Active" => Some(PropertyValue::Bool(self.active)),
            "Visible" => Some(PropertyValue::Bool(self.visible)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Text", PropertyValue::String(v)) => { self.text = v; Ok(()) }
            ("RichText", PropertyValue::Bool(v)) => { self.rich_text = v; Ok(()) }
            ("TextScaled", PropertyValue::Bool(v)) => { self.text_scaled = v; Ok(()) }
            ("TextWrapped", PropertyValue::Bool(v)) => { self.text_wrapped = v; Ok(()) }
            ("Font", PropertyValue::String(v)) => {
                self.font = match v.as_str() {
                    "SourceSans" => crate::classes::Font::SourceSans,
                    "RobotoMono" => crate::classes::Font::RobotoMono,
                    "GothamBold" => crate::classes::Font::GothamBold,
                    "GothamLight" => crate::classes::Font::GothamLight,
                    "Fantasy" => crate::classes::Font::Fantasy,
                    "Bangers" => crate::classes::Font::Bangers,
                    "Merriweather" => crate::classes::Font::Merriweather,
                    "Nunito" => crate::classes::Font::Nunito,
                    "Ubuntu" => crate::classes::Font::Ubuntu,
                    _ => return Err(format!("Unknown font: {}", v)),
                };
                Ok(())
            }
            ("FontSize", PropertyValue::Float(v)) => { self.font_size = v.max(1.0); Ok(()) }
            ("LineHeight", PropertyValue::Float(v)) => { self.line_height = v.max(0.1); Ok(()) }
            ("TextColor3", PropertyValue::Color3(v)) => { self.text_color3 = v; Ok(()) }
            ("TextTransparency", PropertyValue::Float(v)) => { self.text_transparency = v.clamp(0.0, 1.0); Ok(()) }
            ("TextStrokeColor3", PropertyValue::Color3(v)) => { self.text_stroke_color3 = v; Ok(()) }
            ("TextStrokeTransparency", PropertyValue::Float(v)) => { self.text_stroke_transparency = v.clamp(0.0, 1.0); Ok(()) }
            ("BackgroundColor3", PropertyValue::Color3(v)) => { self.background_color3 = v; Ok(()) }
            ("BackgroundTransparency", PropertyValue::Float(v)) => { self.background_transparency = v.clamp(0.0, 1.0); Ok(()) }
            ("BorderColor3", PropertyValue::Color3(v)) => { self.border_color3 = v; Ok(()) }
            ("BorderSizePixel", PropertyValue::Int(v)) => { self.border_size_pixel = v.max(0); Ok(()) }
            ("Size", PropertyValue::Vector2(v)) => { self.size = v; Ok(()) }
            ("Position", PropertyValue::Vector2(v)) => { self.position = v; Ok(()) }
            ("Active", PropertyValue::Bool(v)) => { self.active = v; Ok(()) }
            ("Visible", PropertyValue::Bool(v)) => { self.visible = v; Ok(()) }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Text".to_string(), property_type: "string".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "RichText".to_string(), property_type: "bool".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "TextScaled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "TextWrapped".to_string(), property_type: "bool".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "Font".to_string(), property_type: "Font".to_string(), read_only: false, category: "Font".to_string() },
            PropertyDescriptor { name: "FontSize".to_string(), property_type: "float".to_string(), read_only: false, category: "Font".to_string() },
            PropertyDescriptor { name: "LineHeight".to_string(), property_type: "float".to_string(), read_only: false, category: "Font".to_string() },
            PropertyDescriptor { name: "TextColor3".to_string(), property_type: "Color3".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "TextTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "TextStrokeColor3".to_string(), property_type: "Color3".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "TextStrokeTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BackgroundColor3".to_string(), property_type: "Color3".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BackgroundTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BorderColor3".to_string(), property_type: "Color3".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BorderSizePixel".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Size".to_string(), property_type: "Vector2".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Position".to_string(), property_type: "Vector2".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
        ]
    }
}

// ============================================================================
// PropertyAccess - Remaining Classes (Minimal implementations)
// ============================================================================

impl PropertyAccess for PVInstance {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Pivot" => Some(PropertyValue::Transform(self.pivot)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Pivot", PropertyValue::Transform(t)) => { self.pivot = t; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Pivot".to_string(), property_type: "CFrame".to_string(), read_only: false, category: "Transform".to_string() }] }
}

impl PropertyAccess for EustressDirectionalLight {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Brightness" => Some(PropertyValue::Float(self.brightness)), "Shadows" => Some(PropertyValue::Bool(self.shadows)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Brightness", PropertyValue::Float(f)) => { self.brightness = f; Ok(()) }, ("Shadows", PropertyValue::Bool(b)) => { self.shadows = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Light".to_string() }] }
}

impl PropertyAccess for Atmosphere {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Density" => Some(PropertyValue::Float(self.density)), "Haze" => Some(PropertyValue::Float(self.haze)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Density", PropertyValue::Float(f)) => { self.density = f; Ok(()) }, ("Haze", PropertyValue::Float(f)) => { self.haze = f; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Density".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() }] }
}

impl PropertyAccess for Clouds {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Enabled" => Some(PropertyValue::Bool(self.enabled)), "Density" => Some(PropertyValue::Float(self.density)), "Coverage" => Some(PropertyValue::Float(self.coverage)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }, ("Density", PropertyValue::Float(f)) => { self.density = f; Ok(()) }, ("Coverage", PropertyValue::Float(f)) => { self.coverage = f; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() }, PropertyDescriptor { name: "Density".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() }] }
}

impl PropertyAccess for Sun {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Enabled" => Some(PropertyValue::Bool(self.enabled)), "TimeOfDay" => Some(PropertyValue::Float(self.time_of_day)), "CastShadows" => Some(PropertyValue::Bool(self.cast_shadows)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }, ("TimeOfDay", PropertyValue::Float(f)) => { self.time_of_day = f; Ok(()) }, ("CastShadows", PropertyValue::Bool(b)) => { self.cast_shadows = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() }, PropertyDescriptor { name: "TimeOfDay".to_string(), property_type: "float".to_string(), read_only: false, category: "Time".to_string() }] }
}

impl PropertyAccess for Moon {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Enabled" => Some(PropertyValue::Bool(self.enabled)), "LunarDay" => Some(PropertyValue::Float(self.lunar_day)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }, ("LunarDay", PropertyValue::Float(f)) => { self.lunar_day = f; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() }] }
}

impl PropertyAccess for Lighting {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "TimeOfDay" => Some(PropertyValue::Float(self.time_of_day)), "Brightness" => Some(PropertyValue::Float(self.brightness)), "FogEnabled" => Some(PropertyValue::Bool(self.fog_enabled)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("TimeOfDay", PropertyValue::Float(f)) => { self.time_of_day = f; Ok(()) }, ("Brightness", PropertyValue::Float(f)) => { self.brightness = f; Ok(()) }, ("FogEnabled", PropertyValue::Bool(b)) => { self.fog_enabled = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "TimeOfDay".to_string(), property_type: "float".to_string(), read_only: false, category: "Time".to_string() }, PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Ambient".to_string() }] }
}

impl PropertyAccess for WorkspaceComponent {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Gravity" => Some(PropertyValue::Float(self.gravity)), "PhysicsEnabled" => Some(PropertyValue::Bool(self.physics_enabled)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Gravity", PropertyValue::Float(f)) => { self.gravity = f; Ok(()) }, ("PhysicsEnabled", PropertyValue::Bool(b)) => { self.physics_enabled = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Gravity".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() }] }
}

impl PropertyAccess for SpawnLocation {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Enabled" => Some(PropertyValue::Bool(self.enabled)), "Priority" => Some(PropertyValue::Int(self.priority)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }, ("Priority", PropertyValue::Int(i)) => { self.priority = i; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Data".to_string() }] }
}

impl PropertyAccess for Seat {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Disabled" => Some(PropertyValue::Bool(self.disabled)),
            "SeatOffset" => Some(PropertyValue::Vector3(self.seat_offset)),
            "Occupant" => Some(PropertyValue::Int(-1)), // TODO: Track occupant entity when CharacterBody is set up
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Disabled", PropertyValue::Bool(b)) => { self.disabled = b; Ok(()) },
            ("SeatOffset", PropertyValue::Vector3(v)) => { self.seat_offset = v; Ok(()) },
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Disabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "SeatOffset".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Occupant".to_string(), property_type: "int".to_string(), read_only: true, category: "Data".to_string() },
        ]
    }
}

impl PropertyAccess for VehicleSeat {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Disabled" => Some(PropertyValue::Bool(self.disabled)),
            "MaxSpeed" => Some(PropertyValue::Float(self.max_speed)),
            "Torque" => Some(PropertyValue::Float(self.torque)),
            "TurnSpeed" => Some(PropertyValue::Float(self.turn_speed)),
            "Throttle" => Some(PropertyValue::Float(self.throttle)),
            "Steer" => Some(PropertyValue::Float(self.steer)),
            "Handbrake" => Some(PropertyValue::Bool(self.handbrake)),
            "Gear" => Some(PropertyValue::Int(self.gear)),
            "RPM" => Some(PropertyValue::Float(self.rpm)),
            "SeatOffset" => Some(PropertyValue::Vector3(self.seat_offset)),
            "Occupant" => Some(PropertyValue::Int(-1)), // TODO: Track occupant entity when CharacterBody is set up
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Disabled", PropertyValue::Bool(b)) => { self.disabled = b; Ok(()) },
            ("MaxSpeed", PropertyValue::Float(f)) => { self.max_speed = f; Ok(()) },
            ("Torque", PropertyValue::Float(f)) => { self.torque = f; Ok(()) },
            ("TurnSpeed", PropertyValue::Float(f)) => { self.turn_speed = f; Ok(()) },
            ("Throttle", PropertyValue::Float(f)) => { self.throttle = f; Ok(()) },
            ("Steer", PropertyValue::Float(f)) => { self.steer = f; Ok(()) },
            ("Handbrake", PropertyValue::Bool(b)) => { self.handbrake = b; Ok(()) },
            ("Gear", PropertyValue::Int(i)) => { self.gear = i; Ok(()) },
            ("SeatOffset", PropertyValue::Vector3(v)) => { self.seat_offset = v; Ok(()) },
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Disabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MaxSpeed".to_string(), property_type: "float".to_string(), read_only: false, category: "Vehicle".to_string() },
            PropertyDescriptor { name: "Torque".to_string(), property_type: "float".to_string(), read_only: false, category: "Vehicle".to_string() },
            PropertyDescriptor { name: "TurnSpeed".to_string(), property_type: "float".to_string(), read_only: false, category: "Vehicle".to_string() },
            PropertyDescriptor { name: "Throttle".to_string(), property_type: "float".to_string(), read_only: false, category: "Input".to_string() },
            PropertyDescriptor { name: "Steer".to_string(), property_type: "float".to_string(), read_only: false, category: "Input".to_string() },
            PropertyDescriptor { name: "Handbrake".to_string(), property_type: "bool".to_string(), read_only: false, category: "Input".to_string() },
            PropertyDescriptor { name: "Gear".to_string(), property_type: "int".to_string(), read_only: false, category: "Vehicle".to_string() },
            PropertyDescriptor { name: "RPM".to_string(), property_type: "float".to_string(), read_only: true, category: "Vehicle".to_string() },
            PropertyDescriptor { name: "SeatOffset".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "Occupant".to_string(), property_type: "int".to_string(), read_only: true, category: "Data".to_string() },
        ]
    }
}

impl PropertyAccess for Team {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name { "Name" => Some(PropertyValue::String(self.name.clone())), "AutoAssignable" => Some(PropertyValue::Bool(self.auto_assignable)), _ => None }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) { ("Name", PropertyValue::String(s)) => { self.name = s; Ok(()) }, ("AutoAssignable", PropertyValue::Bool(b)) => { self.auto_assignable = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Name".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() }] }
}

impl PropertyAccess for SurfaceGui {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Active" => Some(PropertyValue::Bool(self.active)),
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            "Face" => Some(PropertyValue::Enum(format!("{:?}", self.face))),
            "CanvasSize" => Some(PropertyValue::Vector2([self.canvas_size[0], self.canvas_size[1]])),
            "AlwaysOnTop" => Some(PropertyValue::Bool(self.always_on_top)),
            "Brightness" => Some(PropertyValue::Float(self.brightness)),
            "LightInfluence" => Some(PropertyValue::Float(self.light_influence)),
            "PixelsPerUnit" => Some(PropertyValue::Float(self.pixels_per_unit)),
            "ClipsDescendants" => Some(PropertyValue::Bool(self.clips_descendants)),
            "MaxDistance" => Some(PropertyValue::Float(self.max_distance)),
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Active", PropertyValue::Bool(b)) => { self.active = b; Ok(()) },
            ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) },
            ("AlwaysOnTop", PropertyValue::Bool(b)) => { self.always_on_top = b; Ok(()) },
            ("Brightness", PropertyValue::Float(f)) => { self.brightness = f; Ok(()) },
            ("LightInfluence", PropertyValue::Float(f)) => { self.light_influence = f; Ok(()) },
            ("PixelsPerUnit", PropertyValue::Float(f)) => { self.pixels_per_unit = f; Ok(()) },
            ("ClipsDescendants", PropertyValue::Bool(b)) => { self.clips_descendants = b; Ok(()) },
            ("MaxDistance", PropertyValue::Float(f)) => { self.max_distance = f; Ok(()) },
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Active".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Face".to_string(), property_type: "enum".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "CanvasSize".to_string(), property_type: "Vector2".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "AlwaysOnTop".to_string(), property_type: "bool".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Brightness".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "LightInfluence".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "PixelsPerUnit".to_string(), property_type: "float".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "ClipsDescendants".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MaxDistance".to_string(), property_type: "float".to_string(), read_only: false, category: "Data".to_string() },
        ]
    }
}

impl PropertyAccess for ScreenGui {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            "DisplayOrder" => Some(PropertyValue::Int(self.display_order)),
            "IgnoreGuiInset" => Some(PropertyValue::Bool(self.ignore_gui_inset)),
            "ResetOnSpawn" => Some(PropertyValue::Bool(self.reset_on_spawn)),
            "ClipsDescendants" => Some(PropertyValue::Bool(self.clips_descendants)),
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Enabled", PropertyValue::Bool(b)) => { self.enabled = b; Ok(()) }
            ("DisplayOrder", PropertyValue::Int(i)) => { self.display_order = i; Ok(()) }
            ("IgnoreGuiInset", PropertyValue::Bool(b)) => { self.ignore_gui_inset = b; Ok(()) }
            ("ResetOnSpawn", PropertyValue::Bool(b)) => { self.reset_on_spawn = b; Ok(()) }
            ("ClipsDescendants", PropertyValue::Bool(b)) => { self.clips_descendants = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Enabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "DisplayOrder".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "IgnoreGuiInset".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "ResetOnSpawn".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "ClipsDescendants".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
        ]
    }
}

impl PropertyAccess for Frame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Visible" => Some(PropertyValue::Bool(self.visible)),
            "ZIndex" => Some(PropertyValue::Int(self.z_index)),
            "BackgroundTransparency" => Some(PropertyValue::Float(self.background_transparency)),
            "BorderSizePixel" => Some(PropertyValue::Int(self.border_size_pixel)),
            "ClipsDescendants" => Some(PropertyValue::Bool(self.clips_descendants)),
            "Rotation" => Some(PropertyValue::Float(self.rotation)),
            "LayoutOrder" => Some(PropertyValue::Int(self.layout_order)),
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }
            ("ZIndex", PropertyValue::Int(i)) => { self.z_index = i; Ok(()) }
            ("BackgroundTransparency", PropertyValue::Float(f)) => { self.background_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("BorderSizePixel", PropertyValue::Int(i)) => { self.border_size_pixel = i.max(0); Ok(()) }
            ("ClipsDescendants", PropertyValue::Bool(b)) => { self.clips_descendants = b; Ok(()) }
            ("Rotation", PropertyValue::Float(f)) => { self.rotation = f; Ok(()) }
            ("LayoutOrder", PropertyValue::Int(i)) => { self.layout_order = i; Ok(()) }
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "ZIndex".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BackgroundTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BorderSizePixel".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "ClipsDescendants".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Rotation".to_string(), property_type: "float".to_string(), read_only: false, category: "Transform".to_string() },
            PropertyDescriptor { name: "LayoutOrder".to_string(), property_type: "int".to_string(), read_only: false, category: "Layout".to_string() },
        ]
    }
}

impl PropertyAccess for ScrollingFrame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "ScrollingEnabled" => Some(PropertyValue::Bool(self.scrolling_enabled)), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("ScrollingEnabled", PropertyValue::Bool(b)) => { self.scrolling_enabled = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for ImageLabel {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Image" => Some(PropertyValue::String(self.image.clone())), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Image", PropertyValue::String(s)) => { self.image = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for TextButton {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Visible" => Some(PropertyValue::Bool(self.visible)),
            "Active" => Some(PropertyValue::Bool(self.active)),
            "Text" => Some(PropertyValue::String(self.text.clone())),
            "FontSize" => Some(PropertyValue::Float(self.font_size)),
            "TextTransparency" => Some(PropertyValue::Float(self.text_transparency)),
            "BackgroundTransparency" => Some(PropertyValue::Float(self.background_transparency)),
            "BorderSizePixel" => Some(PropertyValue::Int(self.border_size_pixel)),
            "ZIndex" => Some(PropertyValue::Int(self.z_index)),
            "AutoButtonColor" => Some(PropertyValue::Bool(self.auto_button_color)),
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }
            ("Active", PropertyValue::Bool(b)) => { self.active = b; Ok(()) }
            ("Text", PropertyValue::String(s)) => { self.text = s; Ok(()) }
            ("FontSize", PropertyValue::Float(f)) => { self.font_size = f.max(1.0); Ok(()) }
            ("TextTransparency", PropertyValue::Float(f)) => { self.text_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("BackgroundTransparency", PropertyValue::Float(f)) => { self.background_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("BorderSizePixel", PropertyValue::Int(i)) => { self.border_size_pixel = i.max(0); Ok(()) }
            ("ZIndex", PropertyValue::Int(i)) => { self.z_index = i; Ok(()) }
            ("AutoButtonColor", PropertyValue::Bool(b)) => { self.auto_button_color = b; Ok(()) }
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Active".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Text".to_string(), property_type: "string".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "FontSize".to_string(), property_type: "float".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "TextTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "BackgroundTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BorderSizePixel".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "ZIndex".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "AutoButtonColor".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
        ]
    }
}

impl PropertyAccess for ImageButton {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Image" => Some(PropertyValue::String(self.image.clone())), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Image", PropertyValue::String(s)) => { self.image = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for VideoFrame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Video" => Some(PropertyValue::String(self.video.clone())), "Playing" => Some(PropertyValue::Bool(self.playing)), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Video", PropertyValue::String(s)) => { self.video = s; Ok(()) }, ("Playing", PropertyValue::Bool(b)) => { self.playing = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for DocumentFrame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Document" => Some(PropertyValue::String(self.document.clone())), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Document", PropertyValue::String(s)) => { self.document = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for WebFrame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Url" => Some(PropertyValue::String(self.url.clone())), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Url", PropertyValue::String(s)) => { self.url = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for TextBox {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Visible" => Some(PropertyValue::Bool(self.visible)),
            "Text" => Some(PropertyValue::String(self.text.clone())),
            "PlaceholderText" => Some(PropertyValue::String(self.placeholder_text.clone())),
            "FontSize" => Some(PropertyValue::Float(self.font_size)),
            "TextEditable" => Some(PropertyValue::Bool(self.text_editable)),
            "ClearTextOnFocus" => Some(PropertyValue::Bool(self.clear_text_on_focus)),
            "MultiLine" => Some(PropertyValue::Bool(self.multi_line)),
            "MaxLength" => Some(PropertyValue::Int(self.max_length)),
            "TextTransparency" => Some(PropertyValue::Float(self.text_transparency)),
            "BackgroundTransparency" => Some(PropertyValue::Float(self.background_transparency)),
            "ZIndex" => Some(PropertyValue::Int(self.z_index)),
            "BorderSizePixel" => Some(PropertyValue::Int(self.border_size_pixel)),
            _ => None
        }
    }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }
            ("Text", PropertyValue::String(s)) => { self.text = s; Ok(()) }
            ("PlaceholderText", PropertyValue::String(s)) => { self.placeholder_text = s; Ok(()) }
            ("FontSize", PropertyValue::Float(f)) => { self.font_size = f.max(1.0); Ok(()) }
            ("TextEditable", PropertyValue::Bool(b)) => { self.text_editable = b; Ok(()) }
            ("ClearTextOnFocus", PropertyValue::Bool(b)) => { self.clear_text_on_focus = b; Ok(()) }
            ("MultiLine", PropertyValue::Bool(b)) => { self.multi_line = b; Ok(()) }
            ("MaxLength", PropertyValue::Int(i)) => { self.max_length = i; Ok(()) }
            ("TextTransparency", PropertyValue::Float(f)) => { self.text_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("BackgroundTransparency", PropertyValue::Float(f)) => { self.background_transparency = f.clamp(0.0, 1.0); Ok(()) }
            ("ZIndex", PropertyValue::Int(i)) => { self.z_index = i; Ok(()) }
            ("BorderSizePixel", PropertyValue::Int(i)) => { self.border_size_pixel = i.max(0); Ok(()) }
            _ => Err(format!("Unknown property: {}", name))
        }
    }
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "Text".to_string(), property_type: "string".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "PlaceholderText".to_string(), property_type: "string".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "FontSize".to_string(), property_type: "float".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "TextEditable".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "ClearTextOnFocus".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MultiLine".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "MaxLength".to_string(), property_type: "int".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "TextTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Text".to_string() },
            PropertyDescriptor { name: "BackgroundTransparency".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "ZIndex".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "BorderSizePixel".to_string(), property_type: "int".to_string(), read_only: false, category: "Appearance".to_string() },
        ]
    }
}

impl PropertyAccess for ViewportFrame {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "Visible" => Some(PropertyValue::Bool(self.visible)), "Ambient" => Some(PropertyValue::Bool(self.ambient)), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("Visible", PropertyValue::Bool(b)) => { self.visible = b; Ok(()) }, ("Ambient", PropertyValue::Bool(b)) => { self.ambient = b; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "Visible".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() }] }
}

impl PropertyAccess for Document {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "SourcePath" => Some(PropertyValue::String(self.source_path.clone())), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("SourcePath", PropertyValue::String(s)) => { self.source_path = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "SourcePath".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() }] }
}

impl PropertyAccess for ImageAsset {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "SourcePath" => Some(PropertyValue::String(self.source_path.clone())), "Width" => Some(PropertyValue::Int(self.width as i32)), "Height" => Some(PropertyValue::Int(self.height as i32)), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("SourcePath", PropertyValue::String(s)) => { self.source_path = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "SourcePath".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() }] }
}

impl PropertyAccess for VideoAsset {
    fn get_property(&self, name: &str) -> Option<PropertyValue> { match name { "SourcePath" => Some(PropertyValue::String(self.source_path.clone())), "Duration" => Some(PropertyValue::Float(self.duration)), _ => None } }
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> { match (name, value) { ("SourcePath", PropertyValue::String(s)) => { self.source_path = s; Ok(()) }, _ => Err(format!("Unknown: {}", name)) } }
    fn list_properties(&self) -> Vec<PropertyDescriptor> { vec![PropertyDescriptor { name: "SourcePath".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() }] }
}

// ============================================================================
// PropertyAccess for Orbital Coordinate Grid Classes
// ============================================================================

impl PropertyAccess for SolarSystem {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "TimeScale" => Some(PropertyValue::Float(self.time_scale as f32)),
            "GravityConstant" => Some(PropertyValue::Float(self.gravity_constant as f32)),
            "SimulationActive" => Some(PropertyValue::Bool(self.simulation_active)),
            "BarycenterX" => Some(PropertyValue::Float(self.barycenter_ecef[0] as f32)),
            "BarycenterY" => Some(PropertyValue::Float(self.barycenter_ecef[1] as f32)),
            "BarycenterZ" => Some(PropertyValue::Float(self.barycenter_ecef[2] as f32)),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("TimeScale", PropertyValue::Float(f)) => {
                if f < 0.0 { return Err("TimeScale must be non-negative".to_string()); }
                self.time_scale = f as f64;
                Ok(())
            }
            ("SimulationActive", PropertyValue::Bool(b)) => {
                self.simulation_active = b;
                Ok(())
            }
            ("GravityConstant", _) => Err("GravityConstant is read-only".to_string()),
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "TimeScale".to_string(), property_type: "float".to_string(), read_only: false, category: "Simulation".to_string() },
            PropertyDescriptor { name: "GravityConstant".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
            PropertyDescriptor { name: "SimulationActive".to_string(), property_type: "bool".to_string(), read_only: false, category: "Simulation".to_string() },
            PropertyDescriptor { name: "BarycenterX".to_string(), property_type: "float".to_string(), read_only: true, category: "Orbital".to_string() },
            PropertyDescriptor { name: "BarycenterY".to_string(), property_type: "float".to_string(), read_only: true, category: "Orbital".to_string() },
            PropertyDescriptor { name: "BarycenterZ".to_string(), property_type: "float".to_string(), read_only: true, category: "Orbital".to_string() },
        ]
    }
}

impl PropertyAccess for CelestialBodyClass {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Mass" => Some(PropertyValue::Float(self.mass as f32)),
            "Radius" => Some(PropertyValue::Float(self.radius as f32)),
            "GM" => Some(PropertyValue::Float(self.gm as f32)),
            "RotationPeriod" => Some(PropertyValue::Float(self.rotation_period as f32)),
            "AxialTilt" => Some(PropertyValue::Float(self.axial_tilt)),
            "RotationAngle" => Some(PropertyValue::Float(self.rotation_angle)),
            "AtmosphereHeight" => Some(PropertyValue::Float(self.atmosphere_height)),
            "Gravitational" => Some(PropertyValue::Bool(self.gravitational)),
            "SemiMajorAxis" => Some(PropertyValue::Float(self.semi_major_axis as f32)),
            "Eccentricity" => Some(PropertyValue::Float(self.eccentricity as f32)),
            "Inclination" => Some(PropertyValue::Float(self.inclination)),
            "SurfaceGravity" => Some(PropertyValue::Float(self.surface_gravity() as f32)),
            "EscapeVelocity" => Some(PropertyValue::Float(self.escape_velocity() as f32)),
            "GlobalECEF" => Some(PropertyValue::Vector3(Vec3::new(
                self.global_ecef[0] as f32,
                self.global_ecef[1] as f32,
                self.global_ecef[2] as f32,
            ))),
            "OrbitalVelocity" => Some(PropertyValue::Vector3(Vec3::new(
                self.orbital_velocity[0] as f32,
                self.orbital_velocity[1] as f32,
                self.orbital_velocity[2] as f32,
            ))),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("Mass", PropertyValue::Float(f)) => {
                if f <= 0.0 { return Err("Mass must be positive".to_string()); }
                self.mass = f as f64;
                self.gm = self.mass * 6.67430e-11; // Update GM
                Ok(())
            }
            ("Radius", PropertyValue::Float(f)) => {
                if f <= 0.0 { return Err("Radius must be positive".to_string()); }
                self.radius = f as f64;
                Ok(())
            }
            ("RotationPeriod", PropertyValue::Float(f)) => {
                if f <= 0.0 { return Err("RotationPeriod must be positive".to_string()); }
                self.rotation_period = f as f64;
                Ok(())
            }
            ("AxialTilt", PropertyValue::Float(f)) => {
                self.axial_tilt = f.clamp(-180.0, 180.0);
                Ok(())
            }
            ("RotationAngle", PropertyValue::Float(f)) => {
                self.rotation_angle = f % 360.0;
                Ok(())
            }
            ("AtmosphereHeight", PropertyValue::Float(f)) => {
                self.atmosphere_height = f.max(0.0);
                Ok(())
            }
            ("Gravitational", PropertyValue::Bool(b)) => {
                self.gravitational = b;
                Ok(())
            }
            ("SemiMajorAxis", PropertyValue::Float(f)) => {
                self.semi_major_axis = f.max(0.0) as f64;
                Ok(())
            }
            ("Eccentricity", PropertyValue::Float(f)) => {
                self.eccentricity = f.clamp(0.0, 1.0) as f64;
                Ok(())
            }
            ("Inclination", PropertyValue::Float(f)) => {
                self.inclination = f.clamp(-180.0, 180.0);
                Ok(())
            }
            ("SurfaceGravity", _) | ("EscapeVelocity", _) | ("GM", _) => {
                Err("Computed property is read-only".to_string())
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Mass".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "Radius".to_string(), property_type: "float".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "GM".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
            PropertyDescriptor { name: "SurfaceGravity".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
            PropertyDescriptor { name: "EscapeVelocity".to_string(), property_type: "float".to_string(), read_only: true, category: "Physics".to_string() },
            PropertyDescriptor { name: "RotationPeriod".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "AxialTilt".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "RotationAngle".to_string(), property_type: "float".to_string(), read_only: false, category: "Rotation".to_string() },
            PropertyDescriptor { name: "AtmosphereHeight".to_string(), property_type: "float".to_string(), read_only: false, category: "Appearance".to_string() },
            PropertyDescriptor { name: "Gravitational".to_string(), property_type: "bool".to_string(), read_only: false, category: "Physics".to_string() },
            PropertyDescriptor { name: "GlobalECEF".to_string(), property_type: "Vector3".to_string(), read_only: true, category: "Orbital".to_string() },
            PropertyDescriptor { name: "OrbitalVelocity".to_string(), property_type: "Vector3".to_string(), read_only: true, category: "Orbital".to_string() },
            PropertyDescriptor { name: "SemiMajorAxis".to_string(), property_type: "float".to_string(), read_only: false, category: "Orbital".to_string() },
            PropertyDescriptor { name: "Eccentricity".to_string(), property_type: "float".to_string(), read_only: false, category: "Orbital".to_string() },
            PropertyDescriptor { name: "Inclination".to_string(), property_type: "float".to_string(), read_only: false, category: "Orbital".to_string() },
        ]
    }
}

impl PropertyAccess for RegionChunk {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Latitude" => Some(PropertyValue::Float(self.center_geodetic[0] as f32)),
            "Longitude" => Some(PropertyValue::Float(self.center_geodetic[1] as f32)),
            "Altitude" => Some(PropertyValue::Float(self.center_geodetic[2] as f32)),
            "BoundsExtents" => Some(PropertyValue::Vector3(self.bounds_extents)),
            "TileLevel" => Some(PropertyValue::Int(self.tile_level as i32)),
            "TileFace" => Some(PropertyValue::Int(self.tile_face as i32)),
            "TileX" => Some(PropertyValue::Int(self.tile_x as i32)),
            "TileY" => Some(PropertyValue::Int(self.tile_y as i32)),
            "GSOverlayEnabled" => Some(PropertyValue::Bool(self.gs_overlay_enabled)),
            "GISDataRef" => Some(PropertyValue::String(self.gis_data_ref.clone())),
            "HeightmapResolution" => Some(PropertyValue::Int(self.heightmap_resolution as i32)),
            "WaterLevel" => Some(PropertyValue::Float(self.water_level)),
            "IsAbstract" => Some(PropertyValue::Bool(self.is_abstract)),
            "Active" => Some(PropertyValue::Bool(self.active)),
            "CustomGravity" => self.custom_gravity.map(PropertyValue::Vector3),
            "ParentOffset" => self.parent_offset.map(PropertyValue::Vector3),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match (name, value) {
            ("BoundsExtents", PropertyValue::Vector3(v)) => {
                if v.x <= 0.0 || v.y <= 0.0 || v.z <= 0.0 {
                    return Err("BoundsExtents must be positive".to_string());
                }
                self.bounds_extents = v;
                Ok(())
            }
            ("GSOverlayEnabled", PropertyValue::Bool(b)) => {
                self.gs_overlay_enabled = b;
                Ok(())
            }
            ("GISDataRef", PropertyValue::String(s)) => {
                self.gis_data_ref = s;
                Ok(())
            }
            ("HeightmapResolution", PropertyValue::Int(i)) => {
                if i < 1 { return Err("HeightmapResolution must be positive".to_string()); }
                self.heightmap_resolution = i as u32;
                Ok(())
            }
            ("WaterLevel", PropertyValue::Float(f)) => {
                self.water_level = f;
                Ok(())
            }
            ("Active", PropertyValue::Bool(b)) => {
                self.active = b;
                Ok(())
            }
            ("CustomGravity", PropertyValue::Vector3(v)) => {
                self.custom_gravity = Some(v);
                Ok(())
            }
            ("Latitude", _) | ("Longitude", _) | ("Altitude", _) |
            ("TileLevel", _) | ("TileFace", _) | ("TileX", _) | ("TileY", _) |
            ("IsAbstract", _) => {
                Err("Coordinate properties are read-only (use from_geodetic)".to_string())
            }
            _ => Err(format!("Unknown property: {}", name)),
        }
    }
    
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            PropertyDescriptor { name: "Latitude".to_string(), property_type: "float".to_string(), read_only: true, category: "Geodetic".to_string() },
            PropertyDescriptor { name: "Longitude".to_string(), property_type: "float".to_string(), read_only: true, category: "Geodetic".to_string() },
            PropertyDescriptor { name: "Altitude".to_string(), property_type: "float".to_string(), read_only: true, category: "Geodetic".to_string() },
            PropertyDescriptor { name: "BoundsExtents".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "TileLevel".to_string(), property_type: "int".to_string(), read_only: true, category: "Tiling".to_string() },
            PropertyDescriptor { name: "TileFace".to_string(), property_type: "int".to_string(), read_only: true, category: "Tiling".to_string() },
            PropertyDescriptor { name: "TileX".to_string(), property_type: "int".to_string(), read_only: true, category: "Tiling".to_string() },
            PropertyDescriptor { name: "TileY".to_string(), property_type: "int".to_string(), read_only: true, category: "Tiling".to_string() },
            PropertyDescriptor { name: "GSOverlayEnabled".to_string(), property_type: "bool".to_string(), read_only: false, category: "Rendering".to_string() },
            PropertyDescriptor { name: "GISDataRef".to_string(), property_type: "string".to_string(), read_only: false, category: "Data".to_string() },
            PropertyDescriptor { name: "HeightmapResolution".to_string(), property_type: "int".to_string(), read_only: false, category: "Terrain".to_string() },
            PropertyDescriptor { name: "WaterLevel".to_string(), property_type: "float".to_string(), read_only: false, category: "Terrain".to_string() },
            PropertyDescriptor { name: "IsAbstract".to_string(), property_type: "bool".to_string(), read_only: true, category: "Data".to_string() },
            PropertyDescriptor { name: "Active".to_string(), property_type: "bool".to_string(), read_only: false, category: "Behavior".to_string() },
            PropertyDescriptor { name: "CustomGravity".to_string(), property_type: "Vector3".to_string(), read_only: false, category: "Physics".to_string() },
        ]
    }
}
