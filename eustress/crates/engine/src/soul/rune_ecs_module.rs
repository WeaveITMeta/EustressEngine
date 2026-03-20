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
//! 5. **Raycasting API** — workspace_raycast / workspace_raycast_all via SpatialQuery bridge

use bevy::prelude::*;
use std::sync::Arc;

use crate::spatial_query_bridge::{
    ScriptSpatialQuery, RaycastParams, RaycastResult,
};

#[cfg(feature = "realism-scripting")]
use rune::{Module, ContextError, runtime::Function};

#[cfg(feature = "realism-scripting")]
use crate::ui::rune_ecs_bindings::ECSBindings;

// ============================================================================
// Thread-local bridge for Rune functions (can't access Bevy system params)
// ============================================================================

/// Thread-local holder for the ScriptSpatialQuery bridge.
/// Set before Rune script execution, cleared after.
thread_local! {
    static SPATIAL_BRIDGE: std::cell::RefCell<Option<ScriptSpatialQuery>> = std::cell::RefCell::new(None);
}

/// Install the spatial query bridge for the current thread before Rune execution.
/// Call this from the Bevy system that runs Rune scripts.
pub fn set_spatial_bridge(bridge: ScriptSpatialQuery) {
    SPATIAL_BRIDGE.with(|cell: &std::cell::RefCell<Option<ScriptSpatialQuery>>| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Clear the spatial query bridge after Rune execution completes.
pub fn clear_spatial_bridge() {
    SPATIAL_BRIDGE.with(|cell: &std::cell::RefCell<Option<ScriptSpatialQuery>>| {
        *cell.borrow_mut() = None;
    });
}

/// Access the spatial query bridge from a Rune function.
fn with_spatial_bridge<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&ScriptSpatialQuery) -> R,
{
    SPATIAL_BRIDGE.with(|cell: &std::cell::RefCell<Option<ScriptSpatialQuery>>| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => callback(bridge),
            None => {
                warn!("[Rune Script] Spatial query bridge not available — raycast ignored");
                fallback
            }
        }
    })
}

// ============================================================================
// Module Registration
// ============================================================================

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
    
    // Raycasting — workspace:Raycast equivalent for Rune
    module.ty::<Vector3>()?;
    module.ty::<RaycastResultRune>()?;
    module.ty::<RaycastParamsRune>()?;
    module.function_meta(workspace_raycast)?;
    module.function_meta(workspace_raycast_all)?;
    
    // Install the bindings as a constant
    module.constant("BINDINGS", bindings)?;
    
    Ok(module)
}

// ============================================================================
// Entity Component Access (existing stubs)
// ============================================================================

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

// ============================================================================
// Logging
// ============================================================================

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

// ============================================================================
// Raycasting API — Rune interface to spatial_query_bridge
// ============================================================================
//
// ## Rune Script Usage (Roblox-compatible):
// ```rune
// use eustress::{Vector3, RaycastParams};
//
// pub fn main() {
//     // Basic raycast from position going down
//     let origin = Vector3::new(0.0, 50.0, 0.0);
//     let direction = Vector3::new(0.0, -100.0, 0.0);
//     let result = eustress::workspace_raycast(origin, direction);
//     
//     if let Some(hit) = result {
//         eustress::log_info(&format!("Hit {} at {}", hit.instance, hit.position));
//         eustress::log_info(&format!("Distance: {}, Material: {}", hit.distance, hit.material));
//     }
//
//     // With custom params
//     let mut params = RaycastParams::new();
//     params.add_exclude("Baseplate");
//     params.max_distance = 500.0;
//     params.ignore_water = true;
//     
//     let result = eustress::workspace_raycast(origin, direction, params);
// }
// ```

// ============================================================================
// Vector3 — Roblox-compatible 3D vector type for Rune
// ============================================================================

/// 3D vector matching Roblox Vector3 API.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct Vector3 {
    #[rune(get, set)]
    pub x: f64,
    #[rune(get, set)]
    pub y: f64,
    #[rune(get, set)]
    pub z: f64,
}

#[cfg(feature = "realism-scripting")]
impl Vector3 {
    #[rune::function(path = Self::new)]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn to_array(&self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

// ============================================================================
// RaycastResult — Roblox-compatible result type for Rune
// ============================================================================

/// Raycast hit result matching Roblox RaycastResult API.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct RaycastResultRune {
    /// Entity name (closest to Roblox Instance)
    #[rune(get)]
    pub instance: String,
    /// Bevy entity ID (Eustress extension)
    #[rune(get)]
    pub entity_id: i64,
    /// Hit position in world space
    #[rune(get)]
    pub position: Vector3,
    /// Surface normal at hit point
    #[rune(get)]
    pub normal: Vector3,
    /// Distance from ray origin to hit
    #[rune(get)]
    pub distance: f64,
    /// Material name
    #[rune(get)]
    pub material: String,
}

// ============================================================================
// RaycastParams — Roblox-compatible filter params for Rune
// ============================================================================

/// Raycast filter parameters matching Roblox RaycastParams API.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct RaycastParamsRune {
    /// Filter mode: true = exclude listed, false = include only listed
    #[rune(get, set)]
    pub exclude_mode: bool,
    /// Entity names to filter
    filter_names: Vec<String>,
    /// Ignore water volumes
    #[rune(get, set)]
    pub ignore_water: bool,
    /// Respect can_collide property
    #[rune(get, set)]
    pub respect_can_collide: bool,
    /// Maximum ray distance
    #[rune(get, set)]
    pub max_distance: f64,
}

#[cfg(feature = "realism-scripting")]
impl RaycastParamsRune {
    #[rune::function(path = Self::new)]
    pub fn new() -> Self {
        Self {
            exclude_mode: true,
            filter_names: Vec::new(),
            ignore_water: false,
            respect_can_collide: true,
            max_distance: 1000.0,
        }
    }

    /// Add an entity name to exclude from raycast results.
    #[rune::function]
    pub fn add_exclude(&mut self, name: String) {
        self.exclude_mode = true;
        self.filter_names.push(name);
    }

    /// Add an entity name to include-only list.
    #[rune::function]
    pub fn add_include(&mut self, name: String) {
        self.exclude_mode = false;
        self.filter_names.push(name);
    }

    /// Convert to bridge RaycastParams.
    fn to_bridge_params(&self) -> RaycastParams {
        let mut params = RaycastParams::new();
        params.exclude_mode = self.exclude_mode;
        params.filter_names = self.filter_names.clone();
        params.ignore_water = self.ignore_water;
        params.respect_can_collide = self.respect_can_collide;
        params.max_distance = self.max_distance as f32;
        params
    }
}

/// Cast a single ray and return the closest hit (Roblox-compatible API).
/// 
/// ## Rune: `let result = eustress::workspace_raycast(origin, direction);`
/// ## Rune: `let result = eustress::workspace_raycast(origin, direction, params);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn workspace_raycast(
    origin: Vector3,
    direction: Vector3,
    params: Option<RaycastParamsRune>,
) -> Option<RaycastResultRune> {
    let bridge_params = params.map(|p| p.to_bridge_params()).unwrap_or_default();
    let origin_arr = origin.to_array();
    let direction_arr = direction.to_array();

    // Submit request and poll (result available from previous frame's processing)
    let result: Option<RaycastResult> = with_spatial_bridge(None, |bridge| {
        let request_id = bridge.submit_raycast(origin_arr, direction_arr, bridge_params);
        bridge.poll_raycast(request_id).flatten()
    });

    result.map(|hit| RaycastResultRune {
        instance: hit.entity_name,
        entity_id: hit.entity_id as i64,
        position: Vector3::new(
            hit.position[0] as f64,
            hit.position[1] as f64,
            hit.position[2] as f64,
        ),
        normal: Vector3::new(
            hit.normal[0] as f64,
            hit.normal[1] as f64,
            hit.normal[2] as f64,
        ),
        distance: hit.distance as f64,
        material: hit.material,
    })
}

/// Cast a ray and return all hits up to max_hits, sorted by distance (Roblox-compatible API).
///
/// ## Rune: `let hits = eustress::workspace_raycast_all(origin, direction, params, 10);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn workspace_raycast_all(
    origin: Vector3,
    direction: Vector3,
    params: Option<RaycastParamsRune>,
    max_hits: i64,
) -> Vec<RaycastResultRune> {
    let bridge_params = params.map(|p| p.to_bridge_params()).unwrap_or_default();
    let origin_arr = origin.to_array();
    let direction_arr = direction.to_array();
    let max = max_hits.max(1) as u32;

    let results: Vec<RaycastResult> = with_spatial_bridge(Vec::new(), |bridge| {
        let request_id = bridge.submit_raycast_all(origin_arr, direction_arr, bridge_params, max);
        bridge.poll_raycast_all(request_id).unwrap_or_default()
    });

    results.into_iter().map(|hit| RaycastResultRune {
        instance: hit.entity_name,
        entity_id: hit.entity_id as i64,
        position: Vector3::new(
            hit.position[0] as f64,
            hit.position[1] as f64,
            hit.position[2] as f64,
        ),
        normal: Vector3::new(
            hit.normal[0] as f64,
            hit.normal[1] as f64,
            hit.normal[2] as f64,
        ),
        distance: hit.distance as f64,
        material: hit.material,
    }).collect()
}

/// Stub module when feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn create_ecs_module() -> Result<(), ()> {
    Ok(())
}
