//! # ECS Bindings
//!
//! Bindings between Rune scripts and Bevy ECS.
//!
//! ## Usage
//!
//! Scripts can query and modify entity properties through
//! the `entity` module functions.

use bevy::prelude::*;
use std::collections::HashMap;

/// Entity handle for scripts
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScriptEntity(pub u64);

impl From<Entity> for ScriptEntity {
    fn from(entity: Entity) -> Self {
        Self(entity.to_bits())
    }
}

impl From<ScriptEntity> for Entity {
    fn from(script_entity: ScriptEntity) -> Self {
        Entity::from_bits(script_entity.0).unwrap_or(Entity::PLACEHOLDER)
    }
}

/// Script context for ECS access
pub struct ScriptContext {
    /// Entity property cache (for read operations)
    entity_cache: HashMap<u64, EntityData>,
    /// Pending property updates
    pending_updates: Vec<PropertyUpdate>,
    /// Pending force applications
    pending_forces: Vec<ForceApplication>,
}

/// Cached entity data for scripts
#[derive(Clone, Default)]
pub struct EntityData {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub mass: f64,
    pub temperature: f64,
    pub pressure: f64,
    pub entropy: f64,
    pub stress: f64,
}

/// Property update from script
#[derive(Clone)]
pub struct PropertyUpdate {
    pub entity_id: u64,
    pub property: String,
    pub value: PropertyValue,
}

/// Property value types
#[derive(Clone)]
pub enum PropertyValue {
    Float(f64),
    Vec3([f64; 3]),
    Bool(bool),
}

/// Force application from script
#[derive(Clone)]
pub struct ForceApplication {
    pub entity_id: u64,
    pub force: [f64; 3],
    pub is_impulse: bool,
}

impl Default for ScriptContext {
    fn default() -> Self {
        Self {
            entity_cache: HashMap::new(),
            pending_updates: Vec::new(),
            pending_forces: Vec::new(),
        }
    }
}

impl ScriptContext {
    /// Create new context
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Cache entity data for script access
    pub fn cache_entity(&mut self, entity_id: u64, data: EntityData) {
        self.entity_cache.insert(entity_id, data);
    }
    
    /// Get cached entity data
    pub fn get_entity(&self, entity_id: u64) -> Option<&EntityData> {
        self.entity_cache.get(&entity_id)
    }
    
    /// Queue a property update
    pub fn queue_update(&mut self, update: PropertyUpdate) {
        self.pending_updates.push(update);
    }
    
    /// Queue a force application
    pub fn queue_force(&mut self, force: ForceApplication) {
        self.pending_forces.push(force);
    }
    
    /// Take pending updates
    pub fn take_updates(&mut self) -> Vec<PropertyUpdate> {
        std::mem::take(&mut self.pending_updates)
    }
    
    /// Take pending forces
    pub fn take_forces(&mut self) -> Vec<ForceApplication> {
        std::mem::take(&mut self.pending_forces)
    }
    
    /// Clear all cached data
    pub fn clear(&mut self) {
        self.entity_cache.clear();
        self.pending_updates.clear();
        self.pending_forces.clear();
    }
}

/// Entity query functions for scripts
pub mod entity {
    use super::*;
    
    /// Get entity position
    pub fn get_position(ctx: &ScriptContext, entity_id: u64) -> Option<[f64; 3]> {
        ctx.get_entity(entity_id).map(|e| e.position)
    }
    
    /// Get entity velocity
    pub fn get_velocity(ctx: &ScriptContext, entity_id: u64) -> Option<[f64; 3]> {
        ctx.get_entity(entity_id).map(|e| e.velocity)
    }
    
    /// Get entity mass
    pub fn get_mass(ctx: &ScriptContext, entity_id: u64) -> Option<f64> {
        ctx.get_entity(entity_id).map(|e| e.mass)
    }
    
    /// Get entity temperature
    pub fn get_temperature(ctx: &ScriptContext, entity_id: u64) -> Option<f64> {
        ctx.get_entity(entity_id).map(|e| e.temperature)
    }
    
    /// Get entity pressure
    pub fn get_pressure(ctx: &ScriptContext, entity_id: u64) -> Option<f64> {
        ctx.get_entity(entity_id).map(|e| e.pressure)
    }
    
    /// Get entity entropy
    pub fn get_entropy(ctx: &ScriptContext, entity_id: u64) -> Option<f64> {
        ctx.get_entity(entity_id).map(|e| e.entropy)
    }
    
    /// Get entity stress (von Mises)
    pub fn get_stress(ctx: &ScriptContext, entity_id: u64) -> Option<f64> {
        ctx.get_entity(entity_id).map(|e| e.stress)
    }
    
    /// Set entity temperature
    pub fn set_temperature(ctx: &mut ScriptContext, entity_id: u64, value: f64) {
        ctx.queue_update(PropertyUpdate {
            entity_id,
            property: "temperature".to_string(),
            value: PropertyValue::Float(value),
        });
    }
    
    /// Set entity pressure
    pub fn set_pressure(ctx: &mut ScriptContext, entity_id: u64, value: f64) {
        ctx.queue_update(PropertyUpdate {
            entity_id,
            property: "pressure".to_string(),
            value: PropertyValue::Float(value),
        });
    }
    
    /// Apply force to entity
    pub fn apply_force(ctx: &mut ScriptContext, entity_id: u64, fx: f64, fy: f64, fz: f64) {
        ctx.queue_force(ForceApplication {
            entity_id,
            force: [fx, fy, fz],
            is_impulse: false,
        });
    }
    
    /// Apply impulse to entity
    pub fn apply_impulse(ctx: &mut ScriptContext, entity_id: u64, ix: f64, iy: f64, iz: f64) {
        ctx.queue_force(ForceApplication {
            entity_id,
            force: [ix, iy, iz],
            is_impulse: true,
        });
    }
}

/// Apply pending script updates to ECS world
pub fn apply_script_updates(
    world: &mut World,
    updates: Vec<PropertyUpdate>,
    forces: Vec<ForceApplication>,
) {
    use crate::realism::particles::components::{ThermodynamicState, KineticState};
    
    // Apply property updates
    for update in updates {
        let entity = Entity::from_bits(update.entity_id);
        if entity.is_none() {
            continue;
        }
        let entity = entity.unwrap();
        
        match update.property.as_str() {
            "temperature" => {
                if let PropertyValue::Float(value) = update.value {
                    if let Some(mut thermo) = world.get_mut::<ThermodynamicState>(entity) {
                        thermo.temperature = value as f32;
                    }
                }
            }
            "pressure" => {
                if let PropertyValue::Float(value) = update.value {
                    if let Some(mut thermo) = world.get_mut::<ThermodynamicState>(entity) {
                        thermo.pressure = value as f32;
                    }
                }
            }
            _ => {}
        }
    }
    
    // Apply forces
    for force_app in forces {
        let entity = Entity::from_bits(force_app.entity_id);
        if entity.is_none() {
            continue;
        }
        let entity = entity.unwrap();
        
        if let Some(mut kinetic) = world.get_mut::<KineticState>(entity) {
            let force = Vec3::new(
                force_app.force[0] as f32,
                force_app.force[1] as f32,
                force_app.force[2] as f32,
            );
            
            if force_app.is_impulse {
                // Get mass for impulse calculation
                if let Some(particle) = world.get::<crate::realism::particles::components::Particle>(entity) {
                    kinetic.apply_impulse(force, particle.mass);
                }
            } else {
                kinetic.apply_force(force);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_script_context() {
        let mut ctx = ScriptContext::new();
        
        ctx.cache_entity(1, EntityData {
            temperature: 300.0,
            pressure: 101325.0,
            ..default()
        });
        
        assert_eq!(entity::get_temperature(&ctx, 1), Some(300.0));
        assert_eq!(entity::get_pressure(&ctx, 1), Some(101325.0));
        assert_eq!(entity::get_temperature(&ctx, 2), None);
    }
    
    #[test]
    fn test_queue_updates() {
        let mut ctx = ScriptContext::new();
        
        entity::set_temperature(&mut ctx, 1, 350.0);
        entity::apply_force(&mut ctx, 1, 10.0, 0.0, 0.0);
        
        let updates = ctx.take_updates();
        let forces = ctx.take_forces();
        
        assert_eq!(updates.len(), 1);
        assert_eq!(forces.len(), 1);
    }
}
