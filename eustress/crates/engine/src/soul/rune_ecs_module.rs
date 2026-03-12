//! # Rune ECS Module
//!
//! Zero-copy ECS access for Rune scripts via native modules.
//!
//! ## Table of Contents
//!
//! 1. **Module Registration** — Register ECS functions with Rune context
//! 2. **Entity Access** — Get/set component data from scripts
//! 3. **Query Functions** — Parallel ECS queries from Rune
//! 4. **Zero-Copy Design** — Direct access to Arc<RwLock<T>> without serialization

use bevy::prelude::*;
use std::sync::Arc;

#[cfg(feature = "realism-scripting")]
use rune::{Module, ContextError, runtime::Function};

#[cfg(feature = "realism-scripting")]
use crate::ui::rune_ecs_bindings::ECSBindings;

/// Create the Eustress ECS module for Rune scripts
#[cfg(feature = "realism-scripting")]
pub fn create_ecs_module(bindings: ECSBindings) -> Result<Module, ContextError> {
    let mut module = Module::with_crate("eustress")?;
    module.ty::<ECSBindings>()?;
    
    // Entity component access
    module.function_meta(get_voltage)?;
    module.function_meta(get_soc)?;
    module.function_meta(get_temperature)?;
    module.function_meta(get_dendrite_risk)?;
    
    // Simulation values
    module.function_meta(get_sim_value)?;
    module.function_meta(set_sim_value)?;
    
    // Logging
    module.function_meta(log_info)?;
    module.function_meta(log_warn)?;
    module.function_meta(log_error)?;
    
    // Install the bindings as a constant
    module.constant("BINDINGS", bindings)?;
    
    Ok(module)
}

/// Get voltage for an entity by name
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_voltage(entity_name: &str) -> f32 {
    // Access via thread-local or global bindings
    // In production, this would use rune::Any to pass bindings
    0.0 // Placeholder
}

/// Get state of charge for an entity
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_soc(entity_name: &str) -> f32 {
    0.0 // Placeholder
}

/// Get temperature for an entity
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_temperature(entity_name: &str) -> f32 {
    298.15 // Placeholder
}

/// Get dendrite risk for an entity
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_dendrite_risk(entity_name: &str) -> f32 {
    0.0 // Placeholder
}

/// Get a simulation value by key
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_sim_value(key: &str) -> f64 {
    0.0 // Placeholder
}

/// Set a simulation value
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn set_sim_value(key: &str, value: f64) {
    // Placeholder
}

/// Log info message from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_info(message: &str) {
    info!("[Rune Script] {}", message);
}

/// Log warning from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_warn(message: &str) {
    warn!("[Rune Script] {}", message);
}

/// Log error from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_error(message: &str) {
    error!("[Rune Script] {}", message);
}

/// Stub module when feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn create_ecs_module() -> Result<(), ()> {
    Ok(())
}
