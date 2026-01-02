//! Property Command - Undo/Redo for property changes

#![allow(dead_code)]

use bevy::prelude::*;
#[allow(unused_imports)]
use crate::classes::{PropertyValue, Instance, Folder};
#[allow(unused_imports)]
use crate::properties::PropertyAccess;

/// Command for changing a property value
#[derive(Clone, Debug)]
pub struct PropertyCommand {
    pub entity: Entity,
    pub property_name: String,
    pub old_value: PropertyValue,
    pub new_value: PropertyValue,
    pub description: String,
}

impl PropertyCommand {
    /// Create a new property change command
    pub fn new(
        entity: Entity,
        property_name: impl Into<String>,
        old_value: PropertyValue,
        new_value: PropertyValue,
    ) -> Self {
        let prop_name = property_name.into();
        let description = format!("Change {}", prop_name);
        
        Self {
            entity,
            property_name: prop_name,
            old_value,
            new_value,
            description,
        }
    }
    
    /// Execute the command (apply new value)
    pub fn execute(&self, world: &mut World) -> Result<(), String> {
        self.set_property(world, self.new_value.clone())
    }
    
    /// Undo the command (restore old value)
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        self.set_property(world, self.old_value.clone())
    }
    
    /// Helper to set property on any component type
    fn set_property(&self, world: &mut World, value: PropertyValue) -> Result<(), String> {
        // Try each component type that supports PropertyAccess
        use crate::classes::*;
        
        // Instance
        if let Some(mut instance) = world.get_mut::<Instance>(self.entity) {
            if instance.has_property(&self.property_name) {
                return instance.set_property(&self.property_name, value);
            }
        }
        
        // BasePart
        if let Some(mut base_part) = world.get_mut::<BasePart>(self.entity) {
            if base_part.has_property(&self.property_name) {
                return base_part.set_property(&self.property_name, value);
            }
        }
        
        // Part
        if let Some(mut part) = world.get_mut::<Part>(self.entity) {
            if part.has_property(&self.property_name) {
                return part.set_property(&self.property_name, value);
            }
        }
        
        // MeshPart
        if let Some(mut mesh_part) = world.get_mut::<MeshPart>(self.entity) {
            if mesh_part.has_property(&self.property_name) {
                return mesh_part.set_property(&self.property_name, value);
            }
        }
        
        // Model
        if let Some(mut model) = world.get_mut::<Model>(self.entity) {
            if model.has_property(&self.property_name) {
                return model.set_property(&self.property_name, value);
            }
        }
        
        // Humanoid
        if let Some(mut humanoid) = world.get_mut::<Humanoid>(self.entity) {
            if humanoid.has_property(&self.property_name) {
                return humanoid.set_property(&self.property_name, value);
            }
        }
        
        // Camera
        if let Some(mut camera) = world.get_mut::<crate::classes::EustressCamera>(self.entity) {
            if camera.has_property(&self.property_name) {
                return camera.set_property(&self.property_name, value);
            }
        }
        
        // PointLight
        if let Some(mut light) = world.get_mut::<crate::classes::EustressPointLight>(self.entity) {
            if light.has_property(&self.property_name) {
                return light.set_property(&self.property_name, value);
            }
        }
        
        // SpotLight
        if let Some(mut light) = world.get_mut::<crate::classes::EustressSpotLight>(self.entity) {
            if light.has_property(&self.property_name) {
                return light.set_property(&self.property_name, value);
            }
        }
        
        // SurfaceLight
        if let Some(mut light) = world.get_mut::<SurfaceLight>(self.entity) {
            if light.has_property(&self.property_name) {
                return light.set_property(&self.property_name, value);
            }
        }
        
        // Sound
        if let Some(mut sound) = world.get_mut::<Sound>(self.entity) {
            if sound.has_property(&self.property_name) {
                return sound.set_property(&self.property_name, value);
            }
        }
        
        // Attachment
        if let Some(mut attachment) = world.get_mut::<Attachment>(self.entity) {
            if attachment.has_property(&self.property_name) {
                return attachment.set_property(&self.property_name, value);
            }
        }
        
        // WeldConstraint
        if let Some(mut weld) = world.get_mut::<WeldConstraint>(self.entity) {
            if weld.has_property(&self.property_name) {
                return weld.set_property(&self.property_name, value);
            }
        }
        
        // Motor6D
        if let Some(mut motor) = world.get_mut::<Motor6D>(self.entity) {
            if motor.has_property(&self.property_name) {
                return motor.set_property(&self.property_name, value);
            }
        }
        
        // ParticleEmitter
        if let Some(mut emitter) = world.get_mut::<ParticleEmitter>(self.entity) {
            if emitter.has_property(&self.property_name) {
                return emitter.set_property(&self.property_name, value);
            }
        }
        
        // Beam
        if let Some(mut beam) = world.get_mut::<Beam>(self.entity) {
            if beam.has_property(&self.property_name) {
                return beam.set_property(&self.property_name, value);
            }
        }
        
        // SpecialMesh
        if let Some(mut mesh) = world.get_mut::<SpecialMesh>(self.entity) {
            if mesh.has_property(&self.property_name) {
                return mesh.set_property(&self.property_name, value);
            }
        }
        
        // Decal
        if let Some(mut decal) = world.get_mut::<Decal>(self.entity) {
            if decal.has_property(&self.property_name) {
                return decal.set_property(&self.property_name, value);
            }
        }
        
        // Animator
        if let Some(mut animator) = world.get_mut::<Animator>(self.entity) {
            if animator.has_property(&self.property_name) {
                return animator.set_property(&self.property_name, value);
            }
        }
        
        // KeyframeSequence
        if let Some(mut keyframe) = world.get_mut::<KeyframeSequence>(self.entity) {
            if keyframe.has_property(&self.property_name) {
                return keyframe.set_property(&self.property_name, value);
            }
        }
        
        // Terrain
        if let Some(mut terrain) = world.get_mut::<Terrain>(self.entity) {
            if terrain.has_property(&self.property_name) {
                return terrain.set_property(&self.property_name, value);
            }
        }
        
        // Sky
        if let Some(mut sky) = world.get_mut::<Sky>(self.entity) {
            if sky.has_property(&self.property_name) {
                return sky.set_property(&self.property_name, value);
            }
        }
        
        // UnionOperation
        if let Some(mut union) = world.get_mut::<UnionOperation>(self.entity) {
            if union.has_property(&self.property_name) {
                return union.set_property(&self.property_name, value);
            }
        }
        
        // BillboardGui
        if let Some(mut billboard_gui) = world.get_mut::<BillboardGui>(self.entity) {
            if billboard_gui.has_property(&self.property_name) {
                return billboard_gui.set_property(&self.property_name, value);
            }
        }
        
        // TextLabel
        if let Some(mut text_label) = world.get_mut::<TextLabel>(self.entity) {
            if text_label.has_property(&self.property_name) {
                return text_label.set_property(&self.property_name, value);
            }
        }
        
        // Folder
        if let Some(mut folder) = world.get_mut::<Folder>(self.entity) {
            if folder.has_property(&self.property_name) {
                return folder.set_property(&self.property_name, value);
            }
        }
        
        Err(format!("Property '{}' not found on entity", self.property_name))
    }
    
    /// Check if this command can be merged with another
    pub fn can_merge(&self, other: &PropertyCommand) -> bool {
        self.entity == other.entity && self.property_name == other.property_name
    }
    
    /// Merge with another command (used for continuous edits like sliders)
    pub fn merge(&mut self, other: PropertyCommand) {
        // Keep old_value from first command, new_value from last command
        self.new_value = other.new_value;
        self.description = format!("Change {} (merged)", self.property_name);
    }
}

/// Batch command for multiple property changes
#[derive(Clone, Debug)]
pub struct BatchCommand {
    pub commands: Vec<PropertyCommand>,
    pub description: String,
}

impl BatchCommand {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            commands: Vec::new(),
            description: description.into(),
        }
    }
    
    pub fn add(&mut self, command: PropertyCommand) {
        self.commands.push(command);
    }
    
    pub fn execute(&self, world: &mut World) -> Result<(), String> {
        for cmd in &self.commands {
            cmd.execute(world)?;
        }
        Ok(())
    }
    
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        // Undo in reverse order
        for cmd in self.commands.iter().rev() {
            cmd.undo(world)?;
        }
        Ok(())
    }
}
