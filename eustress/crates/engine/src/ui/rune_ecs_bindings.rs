//! # Rune ECS Bindings
//!
//! Exposes ECS component data to Rune scripts for reading simulation state.
//! Generalized system for any component type, not hardcoded to specific use cases.
//!
//! ## Table of Contents
//!
//! 1. **ECSBindings** — Resource holding component state accessible from scripts
//! 2. **ComponentSnapshot** — Serialized component data for script access
//! 3. **Systems** — Sync ECS components to bindings

use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use eustress_common::realism::particles::components::{ElectrochemicalState, ThermodynamicState};
use eustress_common::realism::materials::properties::MaterialProperties;

/// Shared ECS state accessible from Rune scripts
#[derive(Resource, Clone)]
pub struct ECSBindings {
    /// Entity data by name (e.g., "VCell_Cathode_SulfurVACNT")
    pub entities: Arc<RwLock<HashMap<String, EntitySnapshot>>>,
    
    /// Aggregated simulation values (e.g., "battery.voltage", "battery.soc")
    pub simulation: Arc<RwLock<HashMap<String, f64>>>,
}

impl Default for ECSBindings {
    fn default() -> Self {
        Self {
            entities: Arc::new(RwLock::new(HashMap::new())),
            simulation: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Snapshot of an entity's component data for script access
#[derive(Debug, Clone, Default)]
pub struct EntitySnapshot {
    pub name: String,
    pub position: [f32; 3],
    pub electrochemical: Option<ElectrochemicalSnapshot>,
    pub thermodynamic: Option<ThermodynamicSnapshot>,
    pub material: Option<MaterialSnapshot>,
}

/// Snapshot of ElectrochemicalState component
#[derive(Debug, Clone, Default)]
pub struct ElectrochemicalSnapshot {
    pub voltage: f32,
    pub terminal_voltage: f32,
    pub capacity_ah: f32,
    pub soc: f32,
    pub current: f32,
    pub internal_resistance: f32,
    pub ionic_conductivity: f32,
    pub cycle_count: u32,
    pub c_rate: f32,
    pub capacity_retention: f32,
    pub heat_generation: f32,
    pub dendrite_risk: f32,
}

/// Snapshot of ThermodynamicState component
#[derive(Debug, Clone, Default)]
pub struct ThermodynamicSnapshot {
    pub temperature: f32,
    pub pressure: f32,
    pub volume: f32,
    pub internal_energy: f32,
    pub entropy: f32,
    pub enthalpy: f32,
    pub moles: f32,
}

/// Snapshot of MaterialProperties component
#[derive(Debug, Clone, Default)]
pub struct MaterialSnapshot {
    pub name: String,
    pub density: f32,
    pub thermal_conductivity: f32,
    pub specific_heat: f32,
    pub young_modulus: f32,
}

impl ECSBindings {
    /// Get a simulation value by key
    pub fn get_sim(&self, key: &str) -> f64 {
        self.simulation.read()
            .map(|s| s.get(key).copied().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
    
    /// Set a simulation value
    pub fn set_sim(&self, key: &str, value: f64) {
        if let Ok(mut sim) = self.simulation.write() {
            sim.insert(key.to_string(), value);
        }
    }
    
    /// Get entity snapshot by name
    pub fn get_entity(&self, name: &str) -> Option<EntitySnapshot> {
        self.entities.read()
            .ok()
            .and_then(|e| e.get(name).cloned())
    }
    
    /// Get electrochemical voltage for an entity
    pub fn get_voltage(&self, entity_name: &str) -> f32 {
        self.get_entity(entity_name)
            .and_then(|e| e.electrochemical)
            .map(|ec| ec.voltage)
            .unwrap_or(0.0)
    }
    
    /// Get electrochemical SOC for an entity
    pub fn get_soc(&self, entity_name: &str) -> f32 {
        self.get_entity(entity_name)
            .and_then(|e| e.electrochemical)
            .map(|ec| ec.soc)
            .unwrap_or(0.0)
    }
    
    /// Get temperature for an entity
    pub fn get_temperature(&self, entity_name: &str) -> f32 {
        self.get_entity(entity_name)
            .and_then(|e| e.thermodynamic)
            .map(|t| t.temperature)
            .unwrap_or(298.15)
    }
    
    /// Get dendrite risk for an entity
    pub fn get_dendrite_risk(&self, entity_name: &str) -> f32 {
        self.get_entity(entity_name)
            .and_then(|e| e.electrochemical)
            .map(|ec| ec.dendrite_risk)
            .unwrap_or(0.0)
    }
}

/// Plugin for Rune ECS bindings
pub struct RuneECSBindingsPlugin;

impl Plugin for RuneECSBindingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ECSBindings>()
            .add_systems(Update, sync_ecs_to_bindings);
    }
}

/// Sync ECS components to bindings for script access
fn sync_ecs_to_bindings(
    bindings: Res<ECSBindings>,
    query: Query<(
        Entity,
        &Name,
        Option<&Transform>,
        Option<&ElectrochemicalState>,
        Option<&ThermodynamicState>,
        Option<&MaterialProperties>,
    )>,
) {
    let mut entities_map = HashMap::new();
    
    let mut total_voltage = 0.0f32;
    let mut total_capacity = 0.0f32;
    let mut total_current = 0.0f32;
    let mut avg_soc = 0.0f32;
    let mut avg_temp = 0.0f32;
    let mut max_dendrite_risk = 0.0f32;
    let mut echem_count = 0u32;
    let mut thermo_count = 0u32;
    
    for (_entity, name, transform, echem, thermo, material) in query.iter() {
        let mut snapshot = EntitySnapshot {
            name: name.to_string(),
            position: transform.map(|t| t.translation.into()).unwrap_or([0.0, 0.0, 0.0]),
            ..Default::default()
        };
        
        if let Some(ec) = echem {
            snapshot.electrochemical = Some(ElectrochemicalSnapshot {
                voltage: ec.voltage,
                terminal_voltage: ec.terminal_voltage,
                capacity_ah: ec.capacity_ah,
                soc: ec.soc,
                current: ec.current,
                internal_resistance: ec.internal_resistance,
                ionic_conductivity: ec.ionic_conductivity,
                cycle_count: ec.cycle_count,
                c_rate: ec.c_rate,
                capacity_retention: ec.capacity_retention,
                heat_generation: ec.heat_generation,
                dendrite_risk: ec.dendrite_risk,
            });
            
            total_voltage += ec.voltage;
            total_capacity += ec.capacity_ah;
            total_current = ec.current;
            avg_soc += ec.soc;
            max_dendrite_risk = max_dendrite_risk.max(ec.dendrite_risk);
            echem_count += 1;
        }
        
        if let Some(th) = thermo {
            snapshot.thermodynamic = Some(ThermodynamicSnapshot {
                temperature: th.temperature,
                pressure: th.pressure,
                volume: th.volume,
                internal_energy: th.internal_energy,
                entropy: th.entropy,
                enthalpy: th.enthalpy,
                moles: th.moles,
            });
            
            avg_temp += th.temperature;
            thermo_count += 1;
        }
        
        if let Some(mat) = material {
            snapshot.material = Some(MaterialSnapshot {
                name: mat.name.clone(),
                density: mat.density,
                thermal_conductivity: mat.thermal_conductivity,
                specific_heat: mat.specific_heat,
                young_modulus: mat.young_modulus,
            });
        }
        
        entities_map.insert(name.to_string(), snapshot);
    }
    
    if let Ok(mut entities) = bindings.entities.write() {
        *entities = entities_map;
    }
    
    if let Ok(mut sim) = bindings.simulation.write() {
        if echem_count > 0 {
            sim.insert("battery.voltage".to_string(), total_voltage as f64);
            sim.insert("battery.capacity".to_string(), total_capacity as f64);
            sim.insert("battery.current".to_string(), total_current as f64);
            sim.insert("battery.soc".to_string(), (avg_soc / echem_count as f32) as f64);
            sim.insert("battery.dendrite_risk".to_string(), max_dendrite_risk as f64);
            sim.insert("battery.power".to_string(), (total_voltage * total_current) as f64);
            
            let c_rate = if total_capacity > 0.0 { total_current.abs() / total_capacity } else { 0.0 };
            sim.insert("battery.c_rate".to_string(), c_rate as f64);
        }
        
        if thermo_count > 0 {
            sim.insert("battery.temperature".to_string(), (avg_temp / thermo_count as f32) as f64);
            sim.insert("battery.temperature_c".to_string(), ((avg_temp / thermo_count as f32) - 273.15) as f64);
        }
    }
}
