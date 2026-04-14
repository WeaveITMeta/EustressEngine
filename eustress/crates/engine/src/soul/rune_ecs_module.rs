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
pub use eustress_common::events::{set_event_bus_for_rune, clear_event_bus_for_rune, EventBus};
#[cfg(feature = "realism-scripting")]
use eustress_common::events::event_bus_rune_module;

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

/// Thread-local holder for ECS bindings during script execution.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static ECS_BINDINGS: std::cell::RefCell<Option<ECSBindings>> = std::cell::RefCell::new(None);
}

/// Install ECS bindings for the current thread before Rune execution.
#[cfg(feature = "realism-scripting")]
pub fn set_ecs_bindings(bindings: ECSBindings) {
    ECS_BINDINGS.with(|cell| {
        *cell.borrow_mut() = Some(bindings);
    });
}

/// Clear ECS bindings after Rune execution completes.
#[cfg(feature = "realism-scripting")]
pub fn clear_ecs_bindings() {
    ECS_BINDINGS.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Access ECS bindings from a Rune function.
#[cfg(feature = "realism-scripting")]
fn with_ecs_bindings<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&ECSBindings) -> R,
{
    ECS_BINDINGS.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bindings) => callback(bindings),
            None => {
                warn!("[Rune Script] ECS bindings not available");
                fallback
            }
        }
    })
}

/// Create the Eustress ECS module for Rune scripts
#[cfg(feature = "realism-scripting")]
pub fn create_ecs_module() -> Result<Module, ContextError> {
    let mut module: Module = Module::new();
    
    // Entity component access
    module.function_meta(get_voltage)?;
    module.function_meta(get_soc)?;
    module.function_meta(get_temperature)?;
    module.function_meta(get_dendrite_risk)?;
    
    // Simulation values
    module.function_meta(get_sim_value)?;
    module.function_meta(set_sim_value)?;
    module.function_meta(list_sim_values)?;

    // Entity query + file access (bridge with MCP tools)
    module.function_meta(query_workspace_entities)?;
    module.function_meta(read_space_file)?;
    module.function_meta(write_space_file)?;
    module.function_meta(query_material_properties)?;
    
    // Logging
    module.function_meta(log_info)?;
    module.function_meta(log_warn)?;
    module.function_meta(log_error)?;
    
    // Data types — Roblox-compatible
    module.ty::<Vector3>()?;
    module.ty::<Color3>()?;
    module.ty::<CFrame>()?;
    
    // Raycasting — workspace:Raycast equivalent for Rune
    module.ty::<RaycastResultRune>()?;
    module.ty::<RaycastParamsRune>()?;
    module.function_meta(workspace_raycast)?;
    module.function_meta(workspace_raycast_all)?;
    
    // Instance API — Roblox-compatible instance manipulation
    module.ty::<InstanceRune>()?;
    module.function_meta(instance_new)?;
    
    // TweenService API — Property animation
    module.ty::<TweenInfoRune>()?;
    module.ty::<TweenRune>()?;
    module.function_meta(tween_info_new)?;
    module.function_meta(tween_info_full)?;
    module.function_meta(tween_service_create)?;
    
    // task library — Coroutine scheduling
    module.function_meta(task_wait)?;
    module.function_meta(task_spawn)?;
    module.function_meta(task_defer)?;
    module.function_meta(task_delay)?;
    module.function_meta(task_cancel)?;
    
    // UserInputService API — Input handling
    module.ty::<InputObjectRune>()?;
    module.function_meta(is_key_down)?;
    module.function_meta(is_mouse_button_pressed)?;
    module.function_meta(get_mouse_location)?;
    module.function_meta(get_mouse_delta)?;
    
    // UDim/UDim2 types — UI dimensions
    module.ty::<UDim>()?;
    module.ty::<UDim2>()?;
    
    // P2: DataStoreService API
    module.ty::<DataStoreRune>()?;
    module.ty::<OrderedDataStoreRune>()?;
    module.function_meta(datastore_service_get)?;
    module.function_meta(datastore_service_get_ordered)?;
    module.function_meta(datastore_get)?;
    module.function_meta(datastore_set)?;
    module.function_meta(datastore_remove)?;
    module.function_meta(datastore_increment)?;
    module.function_meta(ordered_datastore_get_sorted)?;
    
    // P2: HttpService API — Full Roblox Parity
    module.ty::<HttpResponseRune>()?;
    module.function_meta(http_get_async)?;
    module.function_meta(http_post_async)?;
    module.function_meta(http_request_async)?;
    module.function_meta(http_url_encode)?;
    module.function_meta(http_generate_guid)?;
    module.function_meta(http_json_encode)?;
    module.function_meta(http_json_decode)?;
    
    // P2: CollectionService API (tags)
    module.function_meta(collection_add_tag)?;
    module.function_meta(collection_remove_tag)?;
    module.function_meta(collection_has_tag)?;
    module.function_meta(collection_get_tagged)?;
    
    // P2: Sound API
    module.ty::<SoundRune>()?;
    module.function_meta(sound_play)?;
    module.function_meta(sound_stop)?;
    module.function_meta(sound_set_volume)?;

    // MarketplaceService — Roblox-compatible marketplace API (Tickets currency)
    module.ty::<ProductInfoRune>()?;
    module.ty::<PlayerRune>()?;
    module.function_meta(marketplace_prompt_purchase)?;
    module.function_meta(marketplace_get_product_info)?;
    module.function_meta(marketplace_player_owns_game_pass)?;
    module.function_meta(marketplace_get_ticket_balance)?;
    module.function_meta(players_get_player_by_user_id)?;
    module.function_meta(players_get_local_player)?;

    // RunService API — environment queries
    module.function_meta(run_service_is_client)?;
    module.function_meta(run_service_is_server)?;
    module.function_meta(run_service_is_studio)?;
    module.function_meta(run_service_is_running)?;

    // BasePart property access
    module.function_meta(part_set_position)?;
    module.function_meta(part_set_rotation)?;
    module.function_meta(part_set_size)?;
    module.function_meta(part_set_anchored)?;
    module.function_meta(part_set_color)?;
    module.function_meta(part_set_material)?;
    module.function_meta(part_set_transparency)?;
    module.function_meta(part_set_can_collide)?;
    module.function_meta(instance_delete)?;

    // Attribute system
    module.function_meta(instance_set_attribute)?;
    module.function_meta(instance_get_attribute)?;

    // Workspace properties
    module.function_meta(workspace_get_gravity)?;
    module.function_meta(workspace_set_gravity)?;

    // Camera API
    module.function_meta(camera_get_position)?;
    module.function_meta(camera_get_look_vector)?;
    module.function_meta(camera_get_fov)?;
    module.function_meta(camera_set_fov)?;
    module.function_meta(camera_screen_point_to_ray)?;

    // Mouse API
    module.function_meta(mouse_get_hit)?;
    module.function_meta(mouse_get_target)?;

    // Physics forces
    module.function_meta(part_apply_impulse)?;
    module.function_meta(part_apply_angular_impulse)?;
    module.function_meta(part_get_mass)?;
    module.function_meta(part_get_velocity)?;
    module.function_meta(part_set_velocity)?;

    // GUI scripting API — Roblox-compatible UI manipulation
    module.function_meta(gui_set_text)?;
    module.function_meta(gui_get_text)?;
    module.function_meta(gui_set_visible)?;
    module.function_meta(gui_set_bg_color)?;
    module.function_meta(gui_set_text_color)?;
    module.function_meta(gui_set_border_color)?;
    module.function_meta(gui_set_position)?;
    module.function_meta(gui_set_size)?;
    module.function_meta(gui_set_font_size)?;

    Ok(module)
}

/// Build and return the `event_bus` Rune module.
/// Register alongside `create_ecs_module()` in the Rune context builder.
#[cfg(feature = "realism-scripting")]
pub fn create_event_bus_module() -> Result<rune::Module, rune::ContextError> {
    event_bus_rune_module()
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

// Shared simulation value storage — accessible by both Rune scripts and MCP tools.
thread_local! {
    /// Simulation watchpoint values: key → f64.
    /// Set by scripts via set_sim_value(), read by scripts and MCP tools.
    pub static SIM_VALUES: std::cell::RefCell<std::collections::HashMap<String, f64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Get a simulation watchpoint value by key.
/// Returns 0.0 if the key does not exist.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_sim_value(key: &str) -> f64 {
    SIM_VALUES.with(|sv| sv.borrow().get(key).copied().unwrap_or(0.0))
}

/// Set a simulation watchpoint value.
/// Both Rune scripts and MCP tools can read values set here.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn set_sim_value(key: &str, value: f64) {
    SIM_VALUES.with(|sv| {
        sv.borrow_mut().insert(key.to_string(), value);
    });
}

/// List all simulation watchpoint keys and their current values.
/// Returns a Vec of (key, value) pairs.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn list_sim_values() -> Vec<(String, f64)> {
    SIM_VALUES.with(|sv| {
        sv.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect()
    })
}

/// Query entities in the current Space's Workspace folder.
/// Returns a Vec of (name, class) pairs for all .part.toml and .glb.toml files.
/// Optionally filter by class_name.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn query_workspace_entities(class_filter: Option<String>) -> Vec<(String, String)> {
    SPACE_ROOT.with(|root| {
        let root = root.borrow();
        let workspace = root.as_ref()
            .map(|r| r.join("Workspace"))
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&workspace) {
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                // Determine TOML path: folder/_instance.toml or flat .part.toml/.glb.toml
                let toml_path = if path.is_dir() {
                    let inst = path.join("_instance.toml");
                    if inst.exists() { inst } else { continue; }
                } else if fname.ends_with(".part.toml") || fname.ends_with(".glb.toml") {
                    path.clone()
                } else {
                    continue;
                };

                if let Ok(content) = std::fs::read_to_string(&toml_path) {
                    if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                        let class = val.get("metadata")
                            .and_then(|m| m.get("class_name"))
                            .and_then(|c| c.as_str())
                            .unwrap_or("Unknown");
                        let display_name = val.get("metadata")
                            .and_then(|m| m.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or(fname);
                        if let Some(ref filter) = class_filter {
                            if class != filter.as_str() { continue; }
                        }
                        results.push((display_name.to_string(), class.to_string()));
                    }
                }
            }
        }
        results
    })
}

/// Read a file from the Space directory.
/// Path is relative to the Space root. Returns file content as a String.
/// Returns empty string if the file doesn't exist or is binary.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn read_space_file(relative_path: &str) -> String {
    SPACE_ROOT.with(|root| {
        let root = root.borrow();
        let base = root.as_ref()
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        // Reject path traversal
        if relative_path.contains("..") {
            return String::new();
        }
        let path = base.join(relative_path);
        if !path.starts_with(&base) {
            return String::new();
        }
        std::fs::read_to_string(&path).unwrap_or_default()
    })
}

// Thread-local for Space root path (set before script execution)
thread_local! {
    pub static SPACE_ROOT: std::cell::RefCell<Option<std::path::PathBuf>> =
        std::cell::RefCell::new(None);
}

/// Set the Space root path for the current thread before Rune execution.
pub fn set_space_root(path: std::path::PathBuf) {
    SPACE_ROOT.with(|r| *r.borrow_mut() = Some(path));
}

/// Clear the Space root path after Rune execution.
pub fn clear_space_root() {
    SPACE_ROOT.with(|r| *r.borrow_mut() = None);
}

/// Write a file to the Space directory.
/// Path is relative to Space root. Rejects `..` traversal.
/// Returns true on success, false on failure.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn write_space_file(relative_path: &str, content: &str) -> bool {
    if relative_path.contains("..") { return false; }
    SPACE_ROOT.with(|root| {
        let root = root.borrow();
        let base = match root.as_ref() {
            Some(r) => r.clone(),
            None => return false,
        };
        let path = base.join(relative_path);
        if !path.starts_with(&base) { return false; }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&path, content).is_ok()
    })
}

/// Query material PBR properties by preset name.
/// Returns (roughness, metallic, reflectance) tuple.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn query_material_properties(material_name: &str) -> (f32, f32, f32) {
    let mat = eustress_common::classes::Material::from_string(material_name);
    mat.pbr_params()
}

// ============================================================================
// Logging
// ============================================================================

/// Log info message from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_info(message: &str) {
    info!("[Rune Script] {}", message);
    eustress_common::gui::push_script_log(
        eustress_common::gui::ScriptLogLevel::Info,
        format!("[Rune] {}", message),
    );
}

/// Log warning from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_warn(message: &str) {
    warn!("[Rune Script] {}", message);
    eustress_common::gui::push_script_log(
        eustress_common::gui::ScriptLogLevel::Warn,
        format!("[Rune] {}", message),
    );
}

/// Log error from script
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_error(message: &str) {
    error!("[Rune Script] {}", message);
    eustress_common::gui::push_script_log(
        eustress_common::gui::ScriptLogLevel::Error,
        format!("[Rune] {}", message),
    );
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

// Implement TryClone for Rune 0.14 compatibility
#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for Vector3 {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl Vector3 {
    pub const ZERO: Vector3 = Vector3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const ONE: Vector3 = Vector3 { x: 1.0, y: 1.0, z: 1.0 };

    /// Rust-side constructor (not wrapped by rune macro)
    pub fn create(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[rune::function(path = Self::new)]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    #[rune::function(instance)]
    pub fn magnitude(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    #[rune::function(instance, path = Self::unit)]
    pub fn unit(&self) -> Self {
        let mag = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if mag > 0.0 {
            Self { x: self.x / mag, y: self.y / mag, z: self.z / mag }
        } else {
            Self::ZERO
        }
    }

    #[rune::function(instance)]
    pub fn dot(&self, other: &Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[rune::function(instance)]
    pub fn cross(&self, other: &Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    #[rune::function(instance)]
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            x: self.x + (goal.x - self.x) * alpha,
            y: self.y + (goal.y - self.y) * alpha,
            z: self.z + (goal.z - self.z) * alpha,
        }
    }

    #[rune::function(instance)]
    pub fn add(&self, other: &Self) -> Self {
        Self { x: self.x + other.x, y: self.y + other.y, z: self.z + other.z }
    }

    #[rune::function(instance)]
    pub fn sub(&self, other: &Self) -> Self {
        Self { x: self.x - other.x, y: self.y - other.y, z: self.z - other.z }
    }

    #[rune::function(instance)]
    pub fn mul(&self, scalar: f64) -> Self {
        Self { x: self.x * scalar, y: self.y * scalar, z: self.z * scalar }
    }

    #[rune::function(instance)]
    pub fn div(&self, scalar: f64) -> Self {
        Self { x: self.x / scalar, y: self.y / scalar, z: self.z / scalar }
    }

    #[rune::function(instance)]
    pub fn neg(&self) -> Self {
        Self { x: -self.x, y: -self.y, z: -self.z }
    }

    fn to_array(&self) -> [f32; 3] {
        [self.x as f32, self.y as f32, self.z as f32]
    }
}

// ============================================================================
// Color3 — Roblox-compatible RGB color type for Rune
// ============================================================================

/// RGB color matching Roblox Color3 API.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct Color3 {
    #[rune(get, set)]
    pub r: f64,
    #[rune(get, set)]
    pub g: f64,
    #[rune(get, set)]
    pub b: f64,
}

// Implement TryClone for Rune 0.14 compatibility
#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for Color3 {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl Color3 {
    #[rune::function(path = Self::new)]
    pub fn new(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    #[rune::function(path = Self::from_rgb)]
    pub fn from_rgb(r: i64, g: i64, b: i64) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
        }
    }

    #[rune::function(path = Self::from_hsv)]
    pub fn from_hsv(h: f64, s: f64, v: f64) -> Self {
        if s <= 0.0 {
            return Self { r: v, g: v, b: v };
        }
        let h = (h % 1.0) * 6.0;
        let i = h.floor() as i32;
        let f = h - i as f64;
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let t = v * (1.0 - s * (1.0 - f));
        match i % 6 {
            0 => Self { r: v, g: t, b: p },
            1 => Self { r: q, g: v, b: p },
            2 => Self { r: p, g: v, b: t },
            3 => Self { r: p, g: q, b: v },
            4 => Self { r: t, g: p, b: v },
            _ => Self { r: v, g: p, b: q },
        }
    }

    #[rune::function(instance)]
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        Self {
            r: self.r + (goal.r - self.r) * alpha,
            g: self.g + (goal.g - self.g) * alpha,
            b: self.b + (goal.b - self.b) * alpha,
        }
    }

    #[rune::function(instance)]
    pub fn to_hsv(&self) -> (f64, f64, f64) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let delta = max - min;
        let v = max;
        let s = if max > 0.0 { delta / max } else { 0.0 };
        let h = if delta <= 0.0 {
            0.0
        } else if max == self.r {
            ((self.g - self.b) / delta) % 6.0 / 6.0
        } else if max == self.g {
            ((self.b - self.r) / delta + 2.0) / 6.0
        } else {
            ((self.r - self.g) / delta + 4.0) / 6.0
        };
        (if h < 0.0 { h + 1.0 } else { h }, s, v)
    }
}

// ============================================================================
// CFrame — Roblox-compatible coordinate frame for Rune
// ============================================================================

/// Coordinate frame (position + rotation) matching Roblox CFrame API.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct CFrame {
    #[rune(get)]
    pub position: Vector3,
    // Rotation stored as quaternion [x, y, z, w]
    qx: f64,
    qy: f64,
    qz: f64,
    qw: f64,
}

// Implement TryClone for Rune 0.14 compatibility
#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for CFrame {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl CFrame {
    #[rune::function(path = Self::new)]
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            position: Vector3 { x, y, z },
            qx: 0.0, qy: 0.0, qz: 0.0, qw: 1.0,
        }
    }

    #[rune::function(path = Self::from_position)]
    pub fn from_position(pos: Vector3) -> Self {
        Self {
            position: pos,
            qx: 0.0, qy: 0.0, qz: 0.0, qw: 1.0,
        }
    }

    #[rune::function(path = Self::angles)]
    pub fn angles(rx: f64, ry: f64, rz: f64) -> Self {
        let (sx, cx) = (rx * 0.5).sin_cos();
        let (sy, cy) = (ry * 0.5).sin_cos();
        let (sz, cz) = (rz * 0.5).sin_cos();
        Self {
            position: Vector3::ZERO,
            qx: sx * cy * cz - cx * sy * sz,
            qy: cx * sy * cz + sx * cy * sz,
            qz: cx * cy * sz - sx * sy * cz,
            qw: cx * cy * cz + sx * sy * sz,
        }
    }

    #[rune::function(path = Self::look_at)]
    pub fn look_at(position: Vector3, target: Vector3) -> Self {
        let look_raw = Vector3 {
            x: target.x - position.x,
            y: target.y - position.y,
            z: target.z - position.z,
        };
        let look_mag = (look_raw.x * look_raw.x + look_raw.y * look_raw.y + look_raw.z * look_raw.z).sqrt();
        
        if look_mag < 1e-10 {
            return Self { position, qx: 0.0, qy: 0.0, qz: 0.0, qw: 1.0 };
        }
        
        let look = Vector3 { x: look_raw.x / look_mag, y: look_raw.y / look_mag, z: look_raw.z / look_mag };

        let up = Vector3 { x: 0.0, y: 1.0, z: 0.0 };
        // Cross product: up x look
        let right_raw = Vector3 {
            x: up.y * look.z - up.z * look.y,
            y: up.z * look.x - up.x * look.z,
            z: up.x * look.y - up.y * look.x,
        };
        let right_mag = (right_raw.x * right_raw.x + right_raw.y * right_raw.y + right_raw.z * right_raw.z).sqrt();
        let right = if right_mag > 1e-10 {
            Vector3 { x: right_raw.x / right_mag, y: right_raw.y / right_mag, z: right_raw.z / right_mag }
        } else {
            Vector3 { x: 1.0, y: 0.0, z: 0.0 }
        };
        // Cross product: look x right
        let actual_up = Vector3 {
            x: look.y * right.z - look.z * right.y,
            y: look.z * right.x - look.x * right.z,
            z: look.x * right.y - look.y * right.x,
        };

        // Convert rotation matrix to quaternion
        let trace = right.x + actual_up.y + (-look.z);
        let (qx, qy, qz, qw) = if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            (
                (actual_up.z - (-look.y)) * s,
                ((-look.x) - right.z) * s,
                (right.y - actual_up.x) * s,
                0.25 / s,
            )
        } else if right.x > actual_up.y && right.x > (-look.z) {
            let s = 2.0 * (1.0 + right.x - actual_up.y - (-look.z)).sqrt();
            (
                0.25 * s,
                (actual_up.x + right.y) / s,
                ((-look.x) + right.z) / s,
                (actual_up.z - (-look.y)) / s,
            )
        } else if actual_up.y > (-look.z) {
            let s = 2.0 * (1.0 + actual_up.y - right.x - (-look.z)).sqrt();
            (
                (actual_up.x + right.y) / s,
                0.25 * s,
                ((-look.y) + actual_up.z) / s,
                ((-look.x) - right.z) / s,
            )
        } else {
            let s = 2.0 * (1.0 + (-look.z) - right.x - actual_up.y).sqrt();
            (
                ((-look.x) + right.z) / s,
                ((-look.y) + actual_up.z) / s,
                0.25 * s,
                (right.y - actual_up.x) / s,
            )
        };

        Self { position, qx, qy, qz, qw }
    }

    #[rune::function(instance)]
    pub fn x(&self) -> f64 { self.position.x }

    #[rune::function(instance)]
    pub fn y(&self) -> f64 { self.position.y }

    #[rune::function(instance)]
    pub fn z(&self) -> f64 { self.position.z }

    #[rune::function(instance)]
    pub fn look_vector(&self) -> Vector3 {
        // Rotate -Z axis by quaternion
        let x = 2.0 * (self.qx * self.qz + self.qw * self.qy);
        let y = 2.0 * (self.qy * self.qz - self.qw * self.qx);
        let z = 1.0 - 2.0 * (self.qx * self.qx + self.qy * self.qy);
        Vector3 { x: -x, y: -y, z: -z }
    }

    #[rune::function(instance)]
    pub fn right_vector(&self) -> Vector3 {
        // Rotate +X axis by quaternion
        let x = 1.0 - 2.0 * (self.qy * self.qy + self.qz * self.qz);
        let y = 2.0 * (self.qx * self.qy + self.qw * self.qz);
        let z = 2.0 * (self.qx * self.qz - self.qw * self.qy);
        Vector3 { x, y, z }
    }

    #[rune::function(instance)]
    pub fn up_vector(&self) -> Vector3 {
        // Rotate +Y axis by quaternion
        let x = 2.0 * (self.qx * self.qy - self.qw * self.qz);
        let y = 1.0 - 2.0 * (self.qx * self.qx + self.qz * self.qz);
        let z = 2.0 * (self.qy * self.qz + self.qw * self.qx);
        Vector3 { x, y, z }
    }

    #[rune::function(instance)]
    pub fn inverse(&self) -> Self {
        // Conjugate quaternion and negate position
        // Transform origin to object space
        let px = self.position.x;
        let py = self.position.y;
        let pz = self.position.z;
        // Rotate by conjugate quaternion
        let inv_x = -(-self.qx * 2.0 * (self.qx * px + self.qy * py + self.qz * pz) + px * (self.qw * self.qw + self.qx * self.qx - self.qy * self.qy - self.qz * self.qz) + 2.0 * self.qw * (self.qy * pz - self.qz * py));
        let inv_y = -(-self.qy * 2.0 * (self.qx * px + self.qy * py + self.qz * pz) + py * (self.qw * self.qw - self.qx * self.qx + self.qy * self.qy - self.qz * self.qz) + 2.0 * self.qw * (self.qz * px - self.qx * pz));
        let inv_z = -(-self.qz * 2.0 * (self.qx * px + self.qy * py + self.qz * pz) + pz * (self.qw * self.qw - self.qx * self.qx - self.qy * self.qy + self.qz * self.qz) + 2.0 * self.qw * (self.qx * py - self.qy * px));
        Self {
            position: Vector3 { x: inv_x, y: inv_y, z: inv_z },
            qx: -self.qx,
            qy: -self.qy,
            qz: -self.qz,
            qw: self.qw,
        }
    }

    #[rune::function(instance)]
    pub fn point_to_world_space(&self, point: &Vector3) -> Vector3 {
        // Rotate point by quaternion then add position
        let px = point.x;
        let py = point.y;
        let pz = point.z;
        
        // q * p * q^-1
        let tx = 2.0 * (self.qy * pz - self.qz * py);
        let ty = 2.0 * (self.qz * px - self.qx * pz);
        let tz = 2.0 * (self.qx * py - self.qy * px);
        
        Vector3 {
            x: px + self.qw * tx + self.qy * tz - self.qz * ty + self.position.x,
            y: py + self.qw * ty + self.qz * tx - self.qx * tz + self.position.y,
            z: pz + self.qw * tz + self.qx * ty - self.qy * tx + self.position.z,
        }
    }

    #[rune::function(instance)]
    pub fn point_to_object_space(&self, point: &Vector3) -> Vector3 {
        // Subtract position then rotate by inverse quaternion
        let px = point.x - self.position.x;
        let py = point.y - self.position.y;
        let pz = point.z - self.position.z;
        
        // q^-1 * p * q (conjugate = negate xyz)
        let tx = 2.0 * (-self.qy * pz + self.qz * py);
        let ty = 2.0 * (-self.qz * px + self.qx * pz);
        let tz = 2.0 * (-self.qx * py + self.qy * px);
        
        Vector3 {
            x: px + self.qw * tx - self.qy * tz + self.qz * ty,
            y: py + self.qw * ty - self.qz * tx + self.qx * tz,
            z: pz + self.qw * tz - self.qx * ty + self.qy * tx,
        }
    }

    #[rune::function(instance)]
    pub fn lerp(&self, goal: &Self, alpha: f64) -> Self {
        // Inline position lerp
        let pos = Vector3 {
            x: self.position.x + (goal.position.x - self.position.x) * alpha,
            y: self.position.y + (goal.position.y - self.position.y) * alpha,
            z: self.position.z + (goal.position.z - self.position.z) * alpha,
        };
        
        // SLERP for quaternion
        let mut dot = self.qx * goal.qx + self.qy * goal.qy + self.qz * goal.qz + self.qw * goal.qw;
        let (gx, gy, gz, gw) = if dot < 0.0 {
            dot = -dot;
            (-goal.qx, -goal.qy, -goal.qz, -goal.qw)
        } else {
            (goal.qx, goal.qy, goal.qz, goal.qw)
        };

        let (qx, qy, qz, qw) = if dot > 0.9995 {
            // Linear interpolation for close quaternions
            let qx = self.qx + alpha * (gx - self.qx);
            let qy = self.qy + alpha * (gy - self.qy);
            let qz = self.qz + alpha * (gz - self.qz);
            let qw = self.qw + alpha * (gw - self.qw);
            let len = (qx*qx + qy*qy + qz*qz + qw*qw).sqrt();
            (qx/len, qy/len, qz/len, qw/len)
        } else {
            let theta_0 = dot.acos();
            let theta = theta_0 * alpha;
            let sin_theta = theta.sin();
            let sin_theta_0 = theta_0.sin();
            let s0 = (theta_0 - theta).cos() - dot * sin_theta / sin_theta_0;
            let s1 = sin_theta / sin_theta_0;
            (
                s0 * self.qx + s1 * gx,
                s0 * self.qy + s1 * gy,
                s0 * self.qz + s1 * gz,
                s0 * self.qw + s1 * gw,
            )
        };

        Self { position: pos, qx, qy, qz, qw }
    }

    #[rune::function(instance)]
    pub fn mul(&self, other: &Self) -> Self {
        // Quaternion multiplication
        let qx = self.qw * other.qx + self.qx * other.qw + self.qy * other.qz - self.qz * other.qy;
        let qy = self.qw * other.qy - self.qx * other.qz + self.qy * other.qw + self.qz * other.qx;
        let qz = self.qw * other.qz + self.qx * other.qy - self.qy * other.qx + self.qz * other.qw;
        let qw = self.qw * other.qw - self.qx * other.qx - self.qy * other.qy - self.qz * other.qz;
        
        // Transform other's position by self (inline point_to_world_space)
        let p = &other.position;
        let tx = 2.0 * (self.qy * p.z - self.qz * p.y);
        let ty = 2.0 * (self.qz * p.x - self.qx * p.z);
        let tz = 2.0 * (self.qx * p.y - self.qy * p.x);
        let pos = Vector3 {
            x: p.x + self.qw * tx + (self.qy * tz - self.qz * ty) + self.position.x,
            y: p.y + self.qw * ty + (self.qz * tx - self.qx * tz) + self.position.y,
            z: p.z + self.qw * tz + (self.qx * ty - self.qy * tx) + self.position.z,
        };
        
        Self { position: pos, qx, qy, qz, qw }
    }

    #[rune::function(instance)]
    pub fn add(&self, offset: &Vector3) -> Self {
        Self {
            position: Vector3 {
                x: self.position.x + offset.x,
                y: self.position.y + offset.y,
                z: self.position.z + offset.z,
            },
            qx: self.qx, qy: self.qy, qz: self.qz, qw: self.qw,
        }
    }

    #[rune::function(instance)]
    pub fn sub(&self, offset: &Vector3) -> Self {
        Self {
            position: Vector3 {
                x: self.position.x - offset.x,
                y: self.position.y - offset.y,
                z: self.position.z - offset.z,
            },
            qx: self.qx, qy: self.qy, qz: self.qz, qw: self.qw,
        }
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

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for RaycastResultRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
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
impl rune::alloc::clone::TryClone for RaycastParamsRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
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
        position: Vector3::create(
            hit.position[0] as f64,
            hit.position[1] as f64,
            hit.position[2] as f64,
        ),
        normal: Vector3::create(
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
        position: Vector3::create(
            hit.position[0] as f64,
            hit.position[1] as f64,
            hit.position[2] as f64,
        ),
        normal: Vector3::create(
            hit.normal[0] as f64,
            hit.normal[1] as f64,
            hit.normal[2] as f64,
        ),
        distance: hit.distance as f64,
        material: hit.material,
    }).collect()
}

// ============================================================================
// Instance API — Rune wrappers for shared InstanceRegistry
// ============================================================================

/// Thread-local holder for the InstanceRegistry.
/// Set before Rune script execution, cleared after.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static INSTANCE_REGISTRY: std::cell::RefCell<Option<std::sync::Arc<std::sync::RwLock<eustress_common::scripting::InstanceRegistry>>>> = std::cell::RefCell::new(None);
}

/// Install the instance registry for the current thread before Rune execution.
#[cfg(feature = "realism-scripting")]
pub fn set_instance_registry(registry: std::sync::Arc<std::sync::RwLock<eustress_common::scripting::InstanceRegistry>>) {
    INSTANCE_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Some(registry);
    });
}

/// Clear the instance registry after Rune execution completes.
#[cfg(feature = "realism-scripting")]
pub fn clear_instance_registry() {
    INSTANCE_REGISTRY.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Access the instance registry from a Rune function.
#[cfg(feature = "realism-scripting")]
fn with_instance_registry<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&std::sync::Arc<std::sync::RwLock<eustress_common::scripting::InstanceRegistry>>) -> R,
{
    INSTANCE_REGISTRY.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(registry) => callback(registry),
            None => {
                warn!("[Rune Script] Instance registry not available");
                fallback
            }
        }
    })
}

/// Rune-compatible Instance reference.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct InstanceRune {
    #[rune(get)]
    pub entity_id: i64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for InstanceRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

#[cfg(feature = "realism-scripting")]
impl InstanceRune {
    /// Get the instance name
    #[rune::function(instance)]
    pub fn name(&self) -> String {
        with_instance_registry(String::new(), |registry| {
            let reg = registry.read().unwrap();
            reg.get(self.entity_id as u64)
                .map(|i| i.name.clone())
                .unwrap_or_default()
        })
    }

    /// Set the instance name
    #[rune::function(instance)]
    pub fn set_name(&self, name: String) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(instance) = reg.get_mut(self.entity_id as u64) {
                instance.name = name;
            }
        });
    }

    /// Get the class name
    #[rune::function(instance)]
    pub fn class_name(&self) -> String {
        with_instance_registry(String::new(), |registry| {
            let reg = registry.read().unwrap();
            reg.get(self.entity_id as u64)
                .map(|i| i.class_name.clone())
                .unwrap_or_default()
        })
    }

    /// Check if instance is of a specific class
    #[rune::function(instance)]
    pub fn is_a(&self, class_name: &str) -> bool {
        with_instance_registry(false, |registry| {
            let reg = registry.read().unwrap();
            if let Some(instance) = reg.get(self.entity_id as u64) {
                if instance.class_name == class_name {
                    return true;
                }
                // Check inheritance
                match class_name {
                    "Instance" => true,
                    "BasePart" => matches!(instance.class_name.as_str(), 
                        "Part" | "MeshPart" | "WedgePart" | "SpawnLocation"),
                    "PVInstance" => matches!(instance.class_name.as_str(),
                        "Part" | "MeshPart" | "Model"),
                    _ => false,
                }
            } else {
                false
            }
        })
    }

    /// Get parent instance
    #[rune::function(instance)]
    pub fn parent(&self) -> Option<InstanceRune> {
        with_instance_registry(None, |registry| {
            let reg = registry.read().unwrap();
            reg.get(self.entity_id as u64)
                .and_then(|i| {
                    if i.parent_id != 0 {
                        Some(InstanceRune { entity_id: i.parent_id as i64 })
                    } else {
                        None
                    }
                })
        })
    }

    /// Get children
    #[rune::function(instance)]
    pub fn get_children(&self) -> Vec<InstanceRune> {
        with_instance_registry(Vec::new(), |registry| {
            let reg = registry.read().unwrap();
            reg.get(self.entity_id as u64)
                .map(|i| {
                    i.children.iter()
                        .map(|&id| InstanceRune { entity_id: id as i64 })
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Find first child with name
    #[rune::function(instance)]
    pub fn find_first_child(&self, name: &str) -> Option<InstanceRune> {
        with_instance_registry(None, |registry| {
            let reg = registry.read().unwrap();
            if let Some(instance) = reg.get(self.entity_id as u64) {
                for &child_id in &instance.children {
                    if let Some(child) = reg.get(child_id) {
                        if child.name == name {
                            return Some(InstanceRune { entity_id: child_id as i64 });
                        }
                    }
                }
            }
            None
        })
    }

    /// Find first child of class
    #[rune::function(instance)]
    pub fn find_first_child_of_class(&self, class_name: &str) -> Option<InstanceRune> {
        with_instance_registry(None, |registry| {
            let reg = registry.read().unwrap();
            if let Some(instance) = reg.get(self.entity_id as u64) {
                for &child_id in &instance.children {
                    if let Some(child) = reg.get(child_id) {
                        if child.class_name == class_name {
                            return Some(InstanceRune { entity_id: child_id as i64 });
                        }
                    }
                }
            }
            None
        })
    }

    /// Set a Vector3 property (Position, Size)
    #[rune::function(instance)]
    pub fn set_vector3(&self, key: String, value: Vector3) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(inst) = reg.get_mut(self.entity_id as u64) {
                inst.properties.insert(key, eustress_common::scripting::PropertyValue::Vector3(
                    eustress_common::scripting::types::Vector3 { x: value.x, y: value.y, z: value.z }
                ));
            }
        });
    }

    /// Set a Color3 property (Color)
    #[rune::function(instance)]
    pub fn set_color3(&self, key: String, value: Color3) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(inst) = reg.get_mut(self.entity_id as u64) {
                inst.properties.insert(key, eustress_common::scripting::PropertyValue::Color3(
                    eustress_common::scripting::types::Color3 { r: value.r, g: value.g, b: value.b }
                ));
            }
        });
    }

    /// Set a string property (Material)
    #[rune::function(instance)]
    pub fn set_string(&self, key: String, value: String) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(inst) = reg.get_mut(self.entity_id as u64) {
                inst.properties.insert(key, eustress_common::scripting::PropertyValue::String(value));
            }
        });
    }

    /// Set a bool property (Anchored, CanCollide)
    #[rune::function(instance)]
    pub fn set_bool(&self, key: String, value: bool) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(inst) = reg.get_mut(self.entity_id as u64) {
                inst.properties.insert(key, eustress_common::scripting::PropertyValue::Bool(value));
            }
        });
    }

    /// Set a float property (Transparency, Reflectance)
    #[rune::function(instance)]
    pub fn set_float(&self, key: String, value: f64) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            if let Some(inst) = reg.get_mut(self.entity_id as u64) {
                inst.properties.insert(key, eustress_common::scripting::PropertyValue::Float(value));
            }
        });
    }

    /// Destroy the instance
    #[rune::function(instance)]
    pub fn destroy(&self) {
        with_instance_registry((), |registry| {
            let mut reg = registry.write().unwrap();
            reg.remove(self.entity_id as u64);
        });
    }

    /// Clone the instance
    #[rune::function(instance, path = Self::clone_instance)]
    pub fn clone_instance(&self) -> Option<InstanceRune> {
        with_instance_registry(None, |registry| {
            let mut reg = registry.write().unwrap();
            let source_data = reg.get(self.entity_id as u64).map(|s| {
                (s.archivable, s.class_name.clone(), s.name.clone())
            });
            if let Some((archivable, class_name, name)) = source_data {
                if !archivable {
                    return None;
                }
                let new_id = reg.next_entity_id();
                let new_instance = eustress_common::scripting::InstanceData::new(
                    new_id,
                    &class_name,
                    &name,
                );
                reg.insert(new_instance);
                Some(InstanceRune { entity_id: new_id as i64 })
            } else {
                None
            }
        })
    }
}

/// Execute a Rune script from the command bar with full ECS module support.
/// Sets up a temporary InstanceRegistry, runs the script, drains created instances.
#[cfg(feature = "realism-scripting")]
pub fn execute_rune_oneshot(source: &str) -> Result<Vec<eustress_common::luau::runtime::LuauCreatedInstance>, String> {
    let instance_registry = std::sync::Arc::new(std::sync::RwLock::new(
        eustress_common::scripting::InstanceRegistry::default()
    ));
    set_instance_registry(instance_registry.clone());

    let modules: Vec<rune::Module> = match create_ecs_module() {
        Ok(m) => vec![m],
        Err(e) => { warn!("Failed to create ECS module: {:?}", e); vec![] }
    };

    let r = eustress_common::soul::rune_runtime::execute_oneshot(&modules, source, "command_bar");

    // Drain created instances
    let mut created = Vec::new();
    {
        let reg = instance_registry.read().unwrap();
        for (_id, inst_data) in reg.iter() {
            use eustress_common::scripting::PropertyValue as PV;
            let pos = inst_data.properties.get("Position")
                .and_then(|v| if let PV::Vector3(vec) = v { Some([vec.x as f32, vec.y as f32, vec.z as f32]) } else { None })
                .unwrap_or([0.0, 0.5, 0.0]);
            let size = inst_data.properties.get("Size")
                .and_then(|v| if let PV::Vector3(vec) = v { Some([vec.x as f32, vec.y as f32, vec.z as f32]) } else { None })
                .unwrap_or([4.0, 1.0, 2.0]);
            let color = inst_data.properties.get("Color")
                .and_then(|v| if let PV::Color3(c) = v { Some([c.r as f32, c.g as f32, c.b as f32, 1.0]) } else { None })
                .unwrap_or([0.639, 0.635, 0.647, 1.0]);
            let material = inst_data.properties.get("Material")
                .and_then(|v| if let PV::String(s) = v { Some(s.clone()) } else { None })
                .unwrap_or_else(|| "Plastic".to_string());
            let anchored = inst_data.properties.get("Anchored")
                .and_then(|v| if let PV::Bool(b) = v { Some(*b) } else { None })
                .unwrap_or(false);

            created.push(eustress_common::luau::runtime::LuauCreatedInstance {
                class_name: inst_data.class_name.clone(),
                name: inst_data.name.clone(),
                position: pos, size, color, material,
                transparency: 0.0, anchored, can_collide: true,
            });
        }
    }
    clear_instance_registry();

    r.map(|_| created)
}

/// Stub when realism-scripting is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn execute_rune_oneshot(_source: &str) -> Result<Vec<eustress_common::luau::runtime::LuauCreatedInstance>, String> {
    Err("Rune scripting requires the realism-scripting feature. Use Luau instead.".to_string())
}

/// Create a new instance of the given class.
///
/// ## Rune: `let part = Instance::new("Part");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn instance_new(class_name: &str) -> Option<InstanceRune> {
    with_instance_registry(None, |registry| {
        let mut reg = registry.write().unwrap();
        let entity_id = reg.create(class_name, None);
        Some(InstanceRune { entity_id: entity_id as i64 })
    })
}

// ============================================================================
// TweenService API — Property Animation (P1)
// ============================================================================

/// Thread-local holder for the TweenService.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static TWEEN_SERVICE: std::cell::RefCell<Option<std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TweenService>>>> = std::cell::RefCell::new(None);
}

/// Install the tween service for the current thread before Rune execution.
#[cfg(feature = "realism-scripting")]
pub fn set_tween_service(service: std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TweenService>>) {
    TWEEN_SERVICE.with(|cell| {
        *cell.borrow_mut() = Some(service);
    });
}

/// Clear the tween service after Rune execution.
#[cfg(feature = "realism-scripting")]
pub fn clear_tween_service() {
    TWEEN_SERVICE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Access the tween service from a Rune function.
#[cfg(feature = "realism-scripting")]
fn with_tween_service<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TweenService>>) -> R,
{
    TWEEN_SERVICE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(service) => callback(service),
            None => {
                warn!("[Rune Script] TweenService not available");
                fallback
            }
        }
    })
}

/// Rune-compatible TweenInfo.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct TweenInfoRune {
    #[rune(get)]
    pub time: f64,
    #[rune(get)]
    pub easing_style: i32,
    #[rune(get)]
    pub easing_direction: i32,
    #[rune(get)]
    pub repeat_count: i32,
    #[rune(get)]
    pub reverses: bool,
    #[rune(get)]
    pub delay_time: f64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for TweenInfoRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl TweenInfoRune {
    /// Convert to shared TweenInfo
    pub fn to_shared(&self) -> eustress_common::scripting::TweenInfo {
        use eustress_common::scripting::{TweenInfo, EasingStyle, EasingDirection};
        
        let style = match self.easing_style {
            0 => EasingStyle::Linear,
            1 => EasingStyle::Sine,
            2 => EasingStyle::Quad,
            3 => EasingStyle::Cubic,
            4 => EasingStyle::Quart,
            5 => EasingStyle::Quint,
            6 => EasingStyle::Exponential,
            7 => EasingStyle::Circular,
            8 => EasingStyle::Back,
            9 => EasingStyle::Elastic,
            10 => EasingStyle::Bounce,
            _ => EasingStyle::Linear,
        };
        
        let direction = match self.easing_direction {
            0 => EasingDirection::In,
            1 => EasingDirection::Out,
            2 => EasingDirection::InOut,
            _ => EasingDirection::Out,
        };
        
        TweenInfo {
            time: self.time,
            easing_style: style,
            easing_direction: direction,
            repeat_count: self.repeat_count,
            reverses: self.reverses,
            delay_time: self.delay_time,
        }
    }
}

/// Create a new TweenInfo with default values.
/// 
/// ## Rune: `let info = eustress::tween_info_new(1.0);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn tween_info_new(time: f64) -> TweenInfoRune {
    TweenInfoRune {
        time,
        easing_style: 0,       // Linear
        easing_direction: 1,   // Out
        repeat_count: 0,
        reverses: false,
        delay_time: 0.0,
    }
}

/// Create a new TweenInfo with full parameters.
/// Note: Rune Function trait supports max 5 args. This wraps 6 into a helper.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn tween_info_full(
    time: f64,
    easing_style: i32,
    easing_direction: i32,
    repeat_count: i32,
    reverses_and_delay: f64, // pack: if >= 100, reverses=true and delay=value-100
) -> TweenInfoRune {
    let reverses = reverses_and_delay >= 100.0;
    let delay_time = if reverses { reverses_and_delay - 100.0 } else { reverses_and_delay };
    TweenInfoRune {
        time,
        easing_style,
        easing_direction,
        repeat_count,
        reverses,
        delay_time,
    }
}

/// Rune-compatible Tween handle.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct TweenRune {
    #[rune(get)]
    pub id: i64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for TweenRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

#[cfg(feature = "realism-scripting")]
impl TweenRune {
    /// Play the tween
    #[rune::function(instance)]
    pub fn play(&self) {
        with_tween_service((), |service| {
            if let Ok(svc) = service.read() {
                // Access the tween by ID and play it
                // The shared TweenService stores tweens internally
            }
        });
    }

    /// Pause the tween
    #[rune::function(instance)]
    pub fn pause(&self) {
        // Pause implementation via service
    }

    /// Cancel the tween
    #[rune::function(instance)]
    pub fn cancel(&self) {
        // Cancel implementation via service
    }

    /// Get current status (0=Playing, 1=Paused, 2=Cancelled, 3=Completed)
    #[rune::function(instance)]
    pub fn status(&self) -> i32 {
        0 // Placeholder
    }
}

/// Create a tween via TweenService.
/// 
/// ## Rune: `let tween = TweenService::Create(info);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn tween_service_create(info: TweenInfoRune) -> TweenRune {
    with_tween_service(TweenRune { id: 0 }, |service| {
        let mut svc = service.write().unwrap();
        let tween = svc.create(info.to_shared());
        TweenRune { id: tween.id() as i64 }
    })
}

// ============================================================================
// task library — Coroutine Scheduling (P1)
// ============================================================================

/// Thread-local holder for the TaskScheduler.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static TASK_SCHEDULER: std::cell::RefCell<Option<std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TaskScheduler>>>> = std::cell::RefCell::new(None);
}

/// Install the task scheduler for the current thread.
#[cfg(feature = "realism-scripting")]
pub fn set_task_scheduler(scheduler: std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TaskScheduler>>) {
    TASK_SCHEDULER.with(|cell| {
        *cell.borrow_mut() = Some(scheduler);
    });
}

/// Clear the task scheduler.
#[cfg(feature = "realism-scripting")]
pub fn clear_task_scheduler() {
    TASK_SCHEDULER.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Access the task scheduler from a Rune function.
#[cfg(feature = "realism-scripting")]
fn with_task_scheduler<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&std::sync::Arc<std::sync::RwLock<eustress_common::scripting::TaskScheduler>>) -> R,
{
    TASK_SCHEDULER.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(scheduler) => callback(scheduler),
            None => {
                warn!("[Rune Script] TaskScheduler not available");
                fallback
            }
        }
    })
}

/// Wait for n seconds.
/// 
/// ## Rune: `task::wait(1.0);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn task_wait(seconds: f64) -> f64 {
    with_task_scheduler(seconds, |scheduler| {
        let sched = scheduler.read().unwrap();
        sched.wait(seconds)
    })
}

/// Spawn a task immediately (placeholder - Rune doesn't support closures easily).
/// 
/// ## Rune: `let id = task::spawn();`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn task_spawn() -> i64 {
    // In a real implementation, this would take a Rune function
    // For now, return a placeholder task ID
    0
}

/// Defer a task to end of frame (placeholder).
/// 
/// ## Rune: `let id = task::defer();`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn task_defer() -> i64 {
    0
}

/// Delay a task by n seconds (placeholder).
/// 
/// ## Rune: `let id = task::delay(2.0);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn task_delay(seconds: f64) -> i64 {
    let _ = seconds;
    0
}

/// Cancel a task by ID.
/// 
/// ## Rune: `task::cancel(task_id);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn task_cancel(task_id: i64) {
    with_task_scheduler((), |scheduler| {
        let sched = scheduler.read().unwrap();
        sched.cancel(task_id as u64);
    });
}

// ============================================================================
// UserInputService API — Input Handling (P1)
// ============================================================================

/// Thread-local holder for input state.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static INPUT_STATE: std::cell::RefCell<InputState> = std::cell::RefCell::new(InputState::default());
}

/// Input state snapshot for scripts.
#[cfg(feature = "realism-scripting")]
#[derive(Default)]
pub struct InputState {
    pub keys_down: std::collections::HashSet<i32>,
    pub mouse_buttons_down: std::collections::HashSet<i32>,
    pub mouse_position: (f64, f64),
    pub mouse_delta: (f64, f64),
}

/// Update input state before script execution.
#[cfg(feature = "realism-scripting")]
pub fn update_input_state(
    keys_down: std::collections::HashSet<i32>,
    mouse_buttons_down: std::collections::HashSet<i32>,
    mouse_position: (f64, f64),
    mouse_delta: (f64, f64),
) {
    INPUT_STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.keys_down = keys_down;
        state.mouse_buttons_down = mouse_buttons_down;
        state.mouse_position = mouse_position;
        state.mouse_delta = mouse_delta;
    });
}

/// Rune-compatible InputObject.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct InputObjectRune {
    #[rune(get)]
    pub key_code: i32,
    #[rune(get)]
    pub user_input_type: i32,
    #[rune(get)]
    pub position_x: f64,
    #[rune(get)]
    pub position_y: f64,
    #[rune(get)]
    pub delta_x: f64,
    #[rune(get)]
    pub delta_y: f64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for InputObjectRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

/// Check if a key is currently pressed.
/// 
/// ## Rune: `let down = UserInputService::IsKeyDown(KeyCode::W);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn is_key_down(key_code: i32) -> bool {
    INPUT_STATE.with(|cell| {
        let state = cell.borrow();
        state.keys_down.contains(&key_code)
    })
}

/// Check if a mouse button is pressed.
/// 
/// ## Rune: `let down = UserInputService::IsMouseButtonPressed(0);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn is_mouse_button_pressed(button: i32) -> bool {
    INPUT_STATE.with(|cell| {
        let state = cell.borrow();
        state.mouse_buttons_down.contains(&button)
    })
}

/// Get current mouse location.
/// 
/// ## Rune: `let (x, y) = UserInputService::GetMouseLocation();`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_mouse_location() -> (f64, f64) {
    INPUT_STATE.with(|cell| {
        let state = cell.borrow();
        state.mouse_position
    })
}

/// Get mouse delta since last frame.
/// 
/// ## Rune: `let (dx, dy) = UserInputService::GetMouseDelta();`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn get_mouse_delta() -> (f64, f64) {
    INPUT_STATE.with(|cell| {
        let state = cell.borrow();
        state.mouse_delta
    })
}

// ============================================================================
// UDim/UDim2 Types — UI Dimensions (P1)
// ============================================================================

/// UDim — Single dimension with scale and offset.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct UDim {
    #[rune(get)]
    pub scale: f64,
    #[rune(get)]
    pub offset: f64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for UDim {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl UDim {
    #[rune::function(path = Self::new)]
    pub fn new(scale: f64, offset: f64) -> Self {
        Self { scale, offset }
    }

    /// Add two UDims
    #[rune::function(instance)]
    pub fn add(&self, other: &UDim) -> UDim {
        UDim {
            scale: self.scale + other.scale,
            offset: self.offset + other.offset,
        }
    }

    /// Subtract two UDims
    #[rune::function(instance)]
    pub fn sub(&self, other: &UDim) -> UDim {
        UDim {
            scale: self.scale - other.scale,
            offset: self.offset - other.offset,
        }
    }
}

/// UDim2 — 2D dimension with X and Y UDims.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct UDim2 {
    #[rune(get)]
    pub x_scale: f64,
    #[rune(get)]
    pub x_offset: f64,
    #[rune(get)]
    pub y_scale: f64,
    #[rune(get)]
    pub y_offset: f64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for UDim2 {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(*self)
    }
}

#[cfg(feature = "realism-scripting")]
impl UDim2 {
    #[rune::function(path = Self::new)]
    pub fn new(x_scale: f64, x_offset: f64, y_scale: f64, y_offset: f64) -> Self {
        Self { x_scale, x_offset, y_scale, y_offset }
    }

    /// Create from scale only
    #[rune::function(path = Self::from_scale)]
    pub fn from_scale(x_scale: f64, y_scale: f64) -> Self {
        Self { x_scale, x_offset: 0.0, y_scale, y_offset: 0.0 }
    }

    /// Create from offset only
    #[rune::function(path = Self::from_offset)]
    pub fn from_offset(x_offset: f64, y_offset: f64) -> Self {
        Self { x_scale: 0.0, x_offset, y_scale: 0.0, y_offset }
    }

    /// Get X as UDim
    #[rune::function(instance)]
    pub fn x(&self) -> UDim {
        UDim { scale: self.x_scale, offset: self.x_offset }
    }

    /// Get Y as UDim
    #[rune::function(instance)]
    pub fn y(&self) -> UDim {
        UDim { scale: self.y_scale, offset: self.y_offset }
    }

    /// Add two UDim2s
    #[rune::function(instance)]
    pub fn add(&self, other: &UDim2) -> UDim2 {
        UDim2 {
            x_scale: self.x_scale + other.x_scale,
            x_offset: self.x_offset + other.x_offset,
            y_scale: self.y_scale + other.y_scale,
            y_offset: self.y_offset + other.y_offset,
        }
    }

    /// Subtract two UDim2s
    #[rune::function(instance)]
    pub fn sub(&self, other: &UDim2) -> UDim2 {
        UDim2 {
            x_scale: self.x_scale - other.x_scale,
            x_offset: self.x_offset - other.x_offset,
            y_scale: self.y_scale - other.y_scale,
            y_offset: self.y_offset - other.y_offset,
        }
    }

    /// Linear interpolation
    #[rune::function(instance)]
    pub fn lerp(&self, goal: &UDim2, alpha: f64) -> UDim2 {
        UDim2 {
            x_scale: self.x_scale + (goal.x_scale - self.x_scale) * alpha,
            x_offset: self.x_offset + (goal.x_offset - self.x_offset) * alpha,
            y_scale: self.y_scale + (goal.y_scale - self.y_scale) * alpha,
            y_offset: self.y_offset + (goal.y_offset - self.y_offset) * alpha,
        }
    }
}

// ============================================================================
// P2: DataStoreService API — AWS DynamoDB Backend
// ============================================================================

/// Thread-local holder for the DataStoreService.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static DATASTORE_SERVICE: std::cell::RefCell<Option<std::sync::Arc<std::sync::RwLock<eustress_common::scripting::DataStoreService>>>> = std::cell::RefCell::new(None);
}

/// Install the datastore service for the current thread.
#[cfg(feature = "realism-scripting")]
pub fn set_datastore_service(service: std::sync::Arc<std::sync::RwLock<eustress_common::scripting::DataStoreService>>) {
    DATASTORE_SERVICE.with(|cell| {
        *cell.borrow_mut() = Some(service);
    });
}

/// Clear the datastore service.
#[cfg(feature = "realism-scripting")]
pub fn clear_datastore_service() {
    DATASTORE_SERVICE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Access the datastore service from a Rune function.
#[cfg(feature = "realism-scripting")]
fn with_datastore_service<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&std::sync::Arc<std::sync::RwLock<eustress_common::scripting::DataStoreService>>) -> R,
{
    DATASTORE_SERVICE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(service) => callback(service),
            None => {
                warn!("[Rune Script] DataStoreService not available");
                fallback
            }
        }
    })
}

/// Rune-compatible DataStore handle.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct DataStoreRune {
    #[rune(get)]
    pub name: String,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for DataStoreRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

/// Rune-compatible OrderedDataStore handle.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct OrderedDataStoreRune {
    #[rune(get)]
    pub name: String,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for OrderedDataStoreRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

/// Get a DataStore by name.
/// 
/// ## Rune: `let store = DataStoreService::GetDataStore("PlayerData");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_service_get(name: &str, scope: Option<String>) -> DataStoreRune {
    DataStoreRune {
        name: match scope {
            Some(s) => format!("{}_{}", name, s),
            None => name.to_string(),
        },
    }
}

/// Get an OrderedDataStore by name.
/// 
/// ## Rune: `let store = DataStoreService::GetOrderedDataStore("Leaderboard");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_service_get_ordered(name: &str, scope: Option<String>) -> OrderedDataStoreRune {
    OrderedDataStoreRune {
        name: match scope {
            Some(s) => format!("{}_{}", name, s),
            None => name.to_string(),
        },
    }
}

/// Get a value from a DataStore.
/// 
/// ## Rune: `let value = DataStore::GetAsync(store, "key");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_get(store: &DataStoreRune, key: &str) -> Option<String> {
    with_datastore_service(None, |service| {
        let svc = service.read().unwrap();
        let ds = svc.get_data_store(&store.name, None);
        ds.get_async(key).ok().flatten()
    })
}

/// Set a value in a DataStore.
/// 
/// ## Rune: `DataStore::SetAsync(store, "key", "value");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_set(store: &DataStoreRune, key: &str, value: &str) -> bool {
    with_datastore_service(false, |service| {
        let svc = service.read().unwrap();
        let ds = svc.get_data_store(&store.name, None);
        ds.set_async(key, value).is_ok()
    })
}

/// Remove a value from a DataStore.
/// 
/// ## Rune: `DataStore::RemoveAsync(store, "key");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_remove(store: &DataStoreRune, key: &str) -> Option<String> {
    with_datastore_service(None, |service| {
        let svc = service.read().unwrap();
        let ds = svc.get_data_store(&store.name, None);
        ds.remove_async(key).ok().flatten()
    })
}

/// Increment a numeric value in a DataStore.
/// 
/// ## Rune: `let new_value = DataStore::IncrementAsync(store, "coins", 10);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn datastore_increment(store: &DataStoreRune, key: &str, delta: i64) -> i64 {
    with_datastore_service(0, |service| {
        let svc = service.read().unwrap();
        let ds = svc.get_data_store(&store.name, None);
        ds.increment_async(key, delta).unwrap_or(0)
    })
}

/// Rune-compatible DataStoreEntry for sorted results.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct DataStoreEntryRune {
    #[rune(get)]
    pub key: String,
    #[rune(get)]
    pub value: i64,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for DataStoreEntryRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

/// Get sorted entries from an OrderedDataStore.
/// 
/// ## Rune: `let entries = OrderedDataStore::GetSortedAsync(store, false, 10);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn ordered_datastore_get_sorted(
    store: &OrderedDataStoreRune,
    ascending: bool,
    page_size: i64,
) -> Vec<DataStoreEntryRune> {
    with_datastore_service(Vec::new(), |service| {
        let svc = service.read().unwrap();
        let ds = svc.get_ordered_data_store(&store.name, None);
        ds.get_sorted_async(ascending, page_size as usize, None, None)
            .unwrap_or_default()
            .into_iter()
            .map(|e| DataStoreEntryRune { key: e.key, value: e.value })
            .collect()
    })
}

// ============================================================================
// P2: HttpService API — Full Roblox Parity
// ============================================================================

/// HTTP GET request.
/// 
/// ## Rune: `let response = HttpService::GetAsync("https://api.example.com/data");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_get_async(url: &str) -> Option<String> {
    match ureq::get(url).call() {
        Ok(response) => response.into_string().ok(),
        Err(_) => None,
    }
}

/// HTTP POST request.
/// 
/// ## Rune: `let response = HttpService::PostAsync("https://api.example.com/data", body);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_post_async(url: &str, body: &str) -> Option<String> {
    match ureq::post(url)
        .set("Content-Type", "application/json")
        .send_string(body)
    {
        Ok(response) => response.into_string().ok(),
        Err(_) => None,
    }
}

/// Rune-compatible HTTP response object.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct HttpResponseRune {
    #[rune(get)]
    pub success: bool,
    #[rune(get)]
    pub status_code: i64,
    #[rune(get)]
    pub status_message: String,
    #[rune(get)]
    pub body: String,
    #[rune(get)]
    /// Headers as a single serialized string "key:value\nkey:value" (Rune TryClone compatible)
    pub headers: String,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for HttpResponseRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

/// Advanced HTTP request with custom method, headers, and body.
/// 
/// ## Rune: 
/// ```rune
/// let response = HttpService::RequestAsync({
///     "Url": "https://api.example.com/data",
///     "Method": "PUT",
///     "Headers": { "Authorization": "Bearer token" },
///     "Body": "{\"key\": \"value\"}"
/// });
/// ```
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_request_async(
    url: &str,
    method: Option<String>,
    headers: Option<std::collections::HashMap<String, String>>,
    body: Option<String>,
) -> HttpResponseRune {
    let method_str = method.as_deref().unwrap_or("GET");
    
    let mut request = match method_str.to_uppercase().as_str() {
        "GET" => ureq::get(url),
        "POST" => ureq::post(url),
        "PUT" => ureq::put(url),
        "DELETE" => ureq::delete(url),
        "PATCH" => ureq::patch(url),
        "HEAD" => ureq::head(url),
        _ => ureq::get(url),
    };
    
    // Apply custom headers
    if let Some(hdrs) = &headers {
        for (key, value) in hdrs {
            request = request.set(key, value);
        }
    }
    
    // Set default content-type for body requests
    if body.is_some() && !headers.as_ref().map(|h| h.contains_key("Content-Type")).unwrap_or(false) {
        request = request.set("Content-Type", "application/json");
    }
    
    let result = match &body {
        Some(b) => request.send_string(b),
        None => request.call(),
    };
    
    match result {
        Ok(response) => {
            let status = response.status();
            let status_text = response.status_text().to_string();
            
            // Collect headers as "key:value\n" string (Rune TryClone compatible)
            let mut response_headers = String::new();
            for name in response.headers_names() {
                if let Some(value) = response.header(&name) {
                    response_headers.push_str(&format!("{}:{}\n", name, value));
                }
            }
            
            let body_text = response.into_string().unwrap_or_default();
            
            HttpResponseRune {
                success: status >= 200 && status < 300,
                status_code: status as i64,
                status_message: status_text,
                body: body_text,
                headers: response_headers,
            }
        }
        Err(ureq::Error::Status(code, response)) => {
            let status_text = response.status_text().to_string();
            let body_text = response.into_string().unwrap_or_default();
            
            HttpResponseRune {
                success: false,
                status_code: code as i64,
                status_message: status_text,
                body: body_text,
                headers: String::new(),
            }
        }
        Err(_) => {
            HttpResponseRune {
                success: false,
                status_code: 0,
                status_message: "Connection failed".to_string(),
                body: String::new(),
                headers: String::new(),
            }
        }
    }
}

/// URL-encode a string for safe use in URLs.
/// 
/// ## Rune: `let encoded = HttpService::UrlEncode("hello world");` // "hello%20world"
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// Generate a GUID/UUID string.
/// 
/// ## Rune: `let id = HttpService::GenerateGUID(false);` // "a1b2c3d4-e5f6-..."
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_generate_guid(wrap_in_curly_braces: bool) -> String {
    let uuid = uuid::Uuid::new_v4();
    if wrap_in_curly_braces {
        format!("{{{}}}", uuid)
    } else {
        uuid.to_string()
    }
}

/// Encode a value to JSON string.
/// 
/// ## Rune: `let json = HttpService::JSONEncode(data);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_json_encode(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Decode JSON string to value.
/// 
/// ## Rune: `let data = HttpService::JSONDecode(json);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn http_json_decode(json: &str) -> Option<String> {
    let trimmed = json.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        Some(trimmed[1..trimmed.len()-1].to_string())
    } else {
        Some(trimmed.to_string())
    }
}

// ============================================================================
// P2: CollectionService API (Tags)
// ============================================================================

/// Thread-local tag storage for entities.
#[cfg(feature = "realism-scripting")]
thread_local! {
    static ENTITY_TAGS: std::cell::RefCell<std::collections::HashMap<i64, std::collections::HashSet<String>>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Add a tag to an entity.
/// 
/// ## Rune: `CollectionService::AddTag(entity_id, "Enemy");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn collection_add_tag(entity_id: i64, tag: &str) {
    ENTITY_TAGS.with(|cell| {
        let mut tags = cell.borrow_mut();
        tags.entry(entity_id)
            .or_insert_with(std::collections::HashSet::new)
            .insert(tag.to_string());
    });
}

/// Remove a tag from an entity.
/// 
/// ## Rune: `CollectionService::RemoveTag(entity_id, "Enemy");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn collection_remove_tag(entity_id: i64, tag: &str) {
    ENTITY_TAGS.with(|cell| {
        let mut tags = cell.borrow_mut();
        if let Some(entity_tags) = tags.get_mut(&entity_id) {
            entity_tags.remove(tag);
        }
    });
}

/// Check if an entity has a tag.
/// 
/// ## Rune: `let is_enemy = CollectionService::HasTag(entity_id, "Enemy");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn collection_has_tag(entity_id: i64, tag: &str) -> bool {
    ENTITY_TAGS.with(|cell| {
        let tags = cell.borrow();
        tags.get(&entity_id)
            .map(|t| t.contains(tag))
            .unwrap_or(false)
    })
}

/// Get all entities with a specific tag.
/// 
/// ## Rune: `let enemies = CollectionService::GetTagged("Enemy");`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn collection_get_tagged(tag: &str) -> Vec<i64> {
    ENTITY_TAGS.with(|cell| {
        let tags = cell.borrow();
        tags.iter()
            .filter(|(_, entity_tags)| entity_tags.contains(tag))
            .map(|(id, _)| *id)
            .collect()
    })
}

// ============================================================================
// P2: Sound API
// ============================================================================

/// Rune-compatible Sound handle.
#[cfg(feature = "realism-scripting")]
#[derive(Debug, Clone, rune::Any)]
pub struct SoundRune {
    #[rune(get)]
    pub entity_id: i64,
    #[rune(get)]
    pub sound_id: String,
    #[rune(get)]
    pub volume: f64,
    #[rune(get)]
    pub playing: bool,
    #[rune(get)]
    pub looped: bool,
}

#[cfg(feature = "realism-scripting")]
impl rune::alloc::clone::TryClone for SoundRune {
    fn try_clone(&self) -> Result<Self, rune::alloc::Error> {
        Ok(self.clone())
    }
}

#[cfg(feature = "realism-scripting")]
impl SoundRune {
    pub fn new(entity_id: i64, sound_id: &str) -> Self {
        Self {
            entity_id,
            sound_id: sound_id.to_string(),
            volume: 1.0,
            playing: false,
            looped: false,
        }
    }
}

/// Play a sound.
/// 
/// ## Rune: `Sound::Play(sound);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn sound_play(sound: &mut SoundRune) {
    sound.playing = true;
    // TODO: Wire to Bevy audio system
    info!("[Sound] Playing: {}", sound.sound_id);
}

/// Stop a sound.
/// 
/// ## Rune: `Sound::Stop(sound);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn sound_stop(sound: &mut SoundRune) {
    sound.playing = false;
    info!("[Sound] Stopped: {}", sound.sound_id);
}

/// Set sound volume.
/// 
/// ## Rune: `Sound::SetVolume(sound, 0.5);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn sound_set_volume(sound: &mut SoundRune, volume: f64) {
    sound.volume = volume.clamp(0.0, 1.0);
}

// ============================================================================
// MarketplaceService — Roblox-compatible marketplace API for Tickets
// ============================================================================

/// Product info returned by MarketplaceService:GetProductInfo()
#[cfg(feature = "realism-scripting")]
#[derive(Debug, rune::Any)]
struct ProductInfoRune {
    #[rune(get)] product_id: i64,
    #[rune(get)] name: String,
    #[rune(get)] description: String,
    #[rune(get)] price_in_tickets: i64,
    #[rune(get)] is_for_sale: bool,
    #[rune(get)] product_type: String, // "GamePass" | "DeveloperProduct"
}

/// Player info for scripting
#[cfg(feature = "realism-scripting")]
#[derive(Debug, rune::Any)]
struct PlayerRune {
    #[rune(get)] user_id: i64,
    #[rune(get)] name: String,
    #[rune(get)] entity_id: i64,
    #[rune(get)] ticket_balance: i64,
}

/// Thread-local marketplace bridge
thread_local! {
    static MARKETPLACE_BRIDGE: std::cell::RefCell<Option<MarketplaceBridge>> = std::cell::RefCell::new(None);
}

/// Marketplace bridge data — set by the Bevy system before script execution
#[derive(Clone)]
pub struct MarketplaceBridge {
    pub game_passes: std::collections::HashMap<i64, (String, String, i64, bool)>, // id → (name, desc, price, for_sale)
    pub dev_products: std::collections::HashMap<i64, (String, String, i64, bool)>,
    pub player_passes: std::collections::HashMap<i64, std::collections::HashSet<i64>>, // entity_id → owned pass IDs
    pub player_tickets: std::collections::HashMap<i64, i64>, // entity_id → ticket balance
    pub player_info: std::collections::HashMap<i64, (i64, String)>, // entity_id → (user_id, username)
}

pub fn set_marketplace_bridge(bridge: MarketplaceBridge) {
    MARKETPLACE_BRIDGE.with(|cell| *cell.borrow_mut() = Some(bridge));
}

pub fn clear_marketplace_bridge() {
    MARKETPLACE_BRIDGE.with(|cell| *cell.borrow_mut() = None);
}

fn with_marketplace<F, R>(fallback: R, callback: F) -> R
where F: FnOnce(&MarketplaceBridge) -> R {
    MARKETPLACE_BRIDGE.with(|cell| {
        match cell.borrow().as_ref() {
            Some(bridge) => callback(bridge),
            None => {
                warn!("[Rune Script] MarketplaceService not available");
                fallback
            }
        }
    })
}

/// MarketplaceService:PromptPurchase(player, productId)
/// Triggers a purchase prompt for the player. Returns true if the prompt was shown.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn marketplace_prompt_purchase(player_entity_id: i64, product_id: i64) -> bool {
    info!("[Rune] MarketplaceService:PromptPurchase({}, {})", player_entity_id, product_id);
    with_marketplace(false, |bridge| {
        bridge.game_passes.contains_key(&product_id) || bridge.dev_products.contains_key(&product_id)
    })
}

/// MarketplaceService:GetProductInfo(productId)
/// Returns product info or None if product doesn't exist.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn marketplace_get_product_info(product_id: i64) -> Option<ProductInfoRune> {
    with_marketplace(None, |bridge| {
        if let Some((name, desc, price, for_sale)) = bridge.game_passes.get(&product_id) {
            Some(ProductInfoRune {
                product_id,
                name: name.clone(),
                description: desc.clone(),
                price_in_tickets: *price,
                is_for_sale: *for_sale,
                product_type: "GamePass".to_string(),
            })
        } else if let Some((name, desc, price, for_sale)) = bridge.dev_products.get(&product_id) {
            Some(ProductInfoRune {
                product_id,
                name: name.clone(),
                description: desc.clone(),
                price_in_tickets: *price,
                is_for_sale: *for_sale,
                product_type: "DeveloperProduct".to_string(),
            })
        } else {
            None
        }
    })
}

/// MarketplaceService:PlayerOwnsGamePass(player, passId)
/// Returns true if the player owns the specified game pass.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn marketplace_player_owns_game_pass(player_entity_id: i64, pass_id: i64) -> bool {
    with_marketplace(false, |bridge| {
        bridge.player_passes
            .get(&player_entity_id)
            .map(|passes| passes.contains(&pass_id))
            .unwrap_or(false)
    })
}

/// MarketplaceService:GetTicketBalance(player)
/// Returns the player's current Ticket balance.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn marketplace_get_ticket_balance(player_entity_id: i64) -> i64 {
    with_marketplace(0, |bridge| {
        *bridge.player_tickets.get(&player_entity_id).unwrap_or(&0)
    })
}

/// Players:GetPlayerByUserId(userId)
/// Returns player info or None.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn players_get_player_by_user_id(user_id: i64) -> Option<PlayerRune> {
    with_marketplace(None, |bridge| {
        for (entity_id, (uid, name)) in &bridge.player_info {
            if *uid == user_id {
                let tickets = *bridge.player_tickets.get(entity_id).unwrap_or(&0);
                return Some(PlayerRune {
                    user_id: *uid,
                    name: name.clone(),
                    entity_id: *entity_id,
                    ticket_balance: tickets,
                });
            }
        }
        None
    })
}

/// Players.LocalPlayer — get the local player
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn players_get_local_player() -> Option<PlayerRune> {
    with_marketplace(None, |bridge| {
        bridge.player_info.iter().next().map(|(entity_id, (uid, name))| {
            let tickets = *bridge.player_tickets.get(entity_id).unwrap_or(&0);
            PlayerRune {
                user_id: *uid,
                name: name.clone(),
                entity_id: *entity_id,
                ticket_balance: tickets,
            }
        })
    })
}

// ============================================================================
// RunService API — Environment Queries
// ============================================================================

/// RunService:IsClient() — always returns true in engine (single-process)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn run_service_is_client() -> bool { true }

/// RunService:IsServer() — returns true when running as Forge server
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn run_service_is_server() -> bool { false }

/// RunService:IsStudio() — returns true when running in EustressEngine editor
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn run_service_is_studio() -> bool { true }

/// RunService:IsRunning() — returns true when simulation is playing
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn run_service_is_running() -> bool {
    // TODO: bridge with PlayModeState resource
    false
}

// ============================================================================
// BasePart Property Setters (write to TOML via SPACE_ROOT)
// ============================================================================

/// Set an entity's position by name (writes to .part.toml)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_position(entity_name: &str, x: f64, y: f64, z: f64) {
    update_part_toml(entity_name, |doc| {
        if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
            transform.insert("position".to_string(), toml::Value::Array(vec![
                toml::Value::Float(x), toml::Value::Float(y), toml::Value::Float(z),
            ]));
        }
    });
}

/// Set an entity's rotation by name (Euler angles in degrees)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_rotation(entity_name: &str, rx: f64, ry: f64, rz: f64) {
    update_part_toml(entity_name, |doc| {
        if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
            // Convert degrees to quaternion stored as [x, y, z, w]
            let (sx, cx) = (rx.to_radians() * 0.5).sin_cos();
            let (sy, cy) = (ry.to_radians() * 0.5).sin_cos();
            let (sz, cz) = (rz.to_radians() * 0.5).sin_cos();
            let qx = sx * cy * cz - cx * sy * sz;
            let qy = cx * sy * cz + sx * cy * sz;
            let qz = cx * cy * sz - sx * sy * cz;
            let qw = cx * cy * cz + sx * sy * sz;
            transform.insert("rotation".to_string(), toml::Value::Array(vec![
                toml::Value::Float(qx), toml::Value::Float(qy),
                toml::Value::Float(qz), toml::Value::Float(qw),
            ]));
        }
    });
}

/// Set an entity's scale/size by name
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_size(entity_name: &str, x: f64, y: f64, z: f64) {
    update_part_toml(entity_name, |doc| {
        if let Some(transform) = doc.get_mut("transform").and_then(|t| t.as_table_mut()) {
            transform.insert("scale".to_string(), toml::Value::Array(vec![
                toml::Value::Float(x), toml::Value::Float(y), toml::Value::Float(z),
            ]));
        }
    });
}

/// Set an entity's anchored state
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_anchored(entity_name: &str, anchored: bool) {
    update_part_toml(entity_name, |doc| {
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            props.insert("anchored".to_string(), toml::Value::Boolean(anchored));
        }
    });
}

/// Set an entity's color (r, g, b in 0-1 range)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_color(entity_name: &str, r: f64, g: f64, b: f64) {
    update_part_toml(entity_name, |doc| {
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            props.insert("color".to_string(), toml::Value::Array(vec![
                toml::Value::Float(r), toml::Value::Float(g),
                toml::Value::Float(b), toml::Value::Float(1.0),
            ]));
        }
    });
}

/// Set an entity's material preset
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_material(entity_name: &str, material: &str) {
    update_part_toml(entity_name, |doc| {
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            props.insert("material".to_string(), toml::Value::String(material.to_string()));
        }
    });
}

/// Set an entity's transparency (0.0 = opaque, 1.0 = invisible)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_transparency(entity_name: &str, transparency: f64) {
    update_part_toml(entity_name, |doc| {
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            props.insert("transparency".to_string(), toml::Value::Float(transparency));
        }
    });
}

/// Set an entity's CanCollide property
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_can_collide(entity_name: &str, can_collide: bool) {
    update_part_toml(entity_name, |doc| {
        if let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) {
            props.insert("can_collide".to_string(), toml::Value::Boolean(can_collide));
        }
    });
}

// ============================================================================
// Attribute System (custom key-value properties)
// ============================================================================

// Thread-local attribute storage: entity_name → { key → value }
thread_local! {
    static INSTANCE_ATTRIBUTES: std::cell::RefCell<std::collections::HashMap<String, std::collections::HashMap<String, String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Set a custom attribute on an instance by name.
/// Attributes are string key-value pairs stored in memory (not persisted to TOML).
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn instance_set_attribute(entity_name: &str, key: &str, value: &str) {
    INSTANCE_ATTRIBUTES.with(|attrs| {
        attrs.borrow_mut()
            .entry(entity_name.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
    });
}

/// Get a custom attribute from an instance by name.
/// Returns None if the attribute doesn't exist.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn instance_get_attribute(entity_name: &str, key: &str) -> Option<String> {
    INSTANCE_ATTRIBUTES.with(|attrs| {
        attrs.borrow()
            .get(entity_name)
            .and_then(|m| m.get(key))
            .cloned()
    })
}

// ============================================================================
// Camera API
// ============================================================================

// Thread-local camera state (set by Bevy camera system before script execution)
thread_local! {
    pub static CAMERA_STATE: std::cell::RefCell<CameraState> = std::cell::RefCell::new(CameraState::default());
}

#[derive(Default, Clone)]
pub struct CameraState {
    pub position: [f64; 3],
    pub look_vector: [f64; 3],
    pub right_vector: [f64; 3],
    pub up_vector: [f64; 3],
    pub fov: f64,
    pub viewport_width: f64,
    pub viewport_height: f64,
}

/// Set the camera state before Rune script execution.
pub fn set_camera_state(state: CameraState) {
    CAMERA_STATE.with(|cs| *cs.borrow_mut() = state);
}

/// Camera position as (x, y, z)
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn camera_get_position() -> (f64, f64, f64) {
    CAMERA_STATE.with(|cs| {
        let s = cs.borrow();
        (s.position[0], s.position[1], s.position[2])
    })
}

/// Camera forward direction as (x, y, z) unit vector
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn camera_get_look_vector() -> (f64, f64, f64) {
    CAMERA_STATE.with(|cs| {
        let s = cs.borrow();
        (s.look_vector[0], s.look_vector[1], s.look_vector[2])
    })
}

/// Camera field of view in degrees
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn camera_get_fov() -> f64 {
    CAMERA_STATE.with(|cs| cs.borrow().fov)
}

/// Set camera field of view in degrees
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn camera_set_fov(fov: f64) {
    CAMERA_STATE.with(|cs| cs.borrow_mut().fov = fov);
}

/// Convert a screen point (x, y) to a world-space ray (origin, direction).
/// Returns ((ox, oy, oz), (dx, dy, dz)).
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn camera_screen_point_to_ray(x: f64, y: f64) -> ((f64, f64, f64), (f64, f64, f64)) {
    CAMERA_STATE.with(|cs| {
        let s = cs.borrow();
        let origin = (s.position[0], s.position[1], s.position[2]);
        // Simplified: project screen point through camera frustum
        let ndc_x = (x / s.viewport_width.max(1.0)) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y / s.viewport_height.max(1.0)) * 2.0;
        let fov_rad = s.fov.to_radians();
        let aspect = s.viewport_width / s.viewport_height.max(1.0);
        let dir_x = s.look_vector[0] + s.right_vector[0] * ndc_x * (fov_rad * 0.5).tan() * aspect + s.up_vector[0] * ndc_y * (fov_rad * 0.5).tan();
        let dir_y = s.look_vector[1] + s.right_vector[1] * ndc_x * (fov_rad * 0.5).tan() * aspect + s.up_vector[1] * ndc_y * (fov_rad * 0.5).tan();
        let dir_z = s.look_vector[2] + s.right_vector[2] * ndc_x * (fov_rad * 0.5).tan() * aspect + s.up_vector[2] * ndc_y * (fov_rad * 0.5).tan();
        let len = (dir_x * dir_x + dir_y * dir_y + dir_z * dir_z).sqrt().max(1e-10);
        (origin, (dir_x / len, dir_y / len, dir_z / len))
    })
}

// ============================================================================
// Mouse API
// ============================================================================

// Thread-local mouse state (set by Bevy input system before script execution)
thread_local! {
    pub static MOUSE_STATE: std::cell::RefCell<MouseState> = std::cell::RefCell::new(MouseState::default());
}

#[derive(Default, Clone)]
pub struct MouseState {
    /// World-space hit position (raycast from cursor into scene)
    pub hit_position: [f64; 3],
    /// Name of the entity under the cursor (empty if none)
    pub target_name: String,
    /// Screen-space cursor position
    pub screen_x: f64,
    pub screen_y: f64,
}

pub fn set_mouse_state(state: MouseState) {
    MOUSE_STATE.with(|ms| *ms.borrow_mut() = state);
}

/// Mouse.Hit — world-space position where the cursor ray hits geometry.
/// Returns (x, y, z).
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn mouse_get_hit() -> (f64, f64, f64) {
    MOUSE_STATE.with(|ms| {
        let s = ms.borrow();
        (s.hit_position[0], s.hit_position[1], s.hit_position[2])
    })
}

/// Mouse.Target — name of the entity under the cursor.
/// Returns empty string if the cursor is not over any entity.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn mouse_get_target() -> String {
    MOUSE_STATE.with(|ms| ms.borrow().target_name.clone())
}

// ============================================================================
// Physics Forces (Avian3d bridge)
// ============================================================================

// Thread-local physics command queue (collected by Bevy system after script execution)
thread_local! {
    pub static PHYSICS_COMMANDS: std::cell::RefCell<Vec<PhysicsCommand>> = std::cell::RefCell::new(Vec::new());
}

#[derive(Debug, Clone)]
pub enum PhysicsCommand {
    ApplyImpulse { entity_name: String, x: f64, y: f64, z: f64 },
    ApplyAngularImpulse { entity_name: String, x: f64, y: f64, z: f64 },
    SetVelocity { entity_name: String, x: f64, y: f64, z: f64 },
}

/// Drain all queued physics commands (called by Bevy system after script execution).
pub fn drain_physics_commands() -> Vec<PhysicsCommand> {
    PHYSICS_COMMANDS.with(|cmds| {
        let mut cmds = cmds.borrow_mut();
        std::mem::take(&mut *cmds)
    })
}

/// Apply a linear impulse to an entity (instantaneous force).
/// Units: kg·m/s (mass × velocity change).
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_apply_impulse(entity_name: &str, x: f64, y: f64, z: f64) {
    PHYSICS_COMMANDS.with(|cmds| {
        cmds.borrow_mut().push(PhysicsCommand::ApplyImpulse {
            entity_name: entity_name.to_string(), x, y, z,
        });
    });
}

/// Apply an angular impulse to an entity (instantaneous torque).
/// Units: kg·m²/s (moment of inertia × angular velocity change).
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_apply_angular_impulse(entity_name: &str, x: f64, y: f64, z: f64) {
    PHYSICS_COMMANDS.with(|cmds| {
        cmds.borrow_mut().push(PhysicsCommand::ApplyAngularImpulse {
            entity_name: entity_name.to_string(), x, y, z,
        });
    });
}

// Thread-local physics state snapshot (populated by Bevy system before script execution)
thread_local! {
    pub static PHYSICS_STATE: std::cell::RefCell<std::collections::HashMap<String, PhysicsSnapshot>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[derive(Debug, Clone, Default)]
pub struct PhysicsSnapshot {
    pub mass: f64,
    pub velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
}

/// Populate physics state from Avian3d before Rune script execution.
pub fn set_physics_state(states: std::collections::HashMap<String, PhysicsSnapshot>) {
    PHYSICS_STATE.with(|ps| *ps.borrow_mut() = states);
}

/// Clear physics state after Rune script execution.
pub fn clear_physics_state() {
    PHYSICS_STATE.with(|ps| ps.borrow_mut().clear());
}

/// Get the mass of an entity in kg.
/// Reads from the physics snapshot populated before script execution.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_get_mass(entity_name: &str) -> f64 {
    PHYSICS_STATE.with(|ps| {
        ps.borrow().get(entity_name).map(|s| s.mass).unwrap_or(1.0)
    })
}

/// Get the linear velocity of an entity as (x, y, z) in m/s.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_get_velocity(entity_name: &str) -> (f64, f64, f64) {
    PHYSICS_STATE.with(|ps| {
        ps.borrow().get(entity_name)
            .map(|s| (s.velocity[0], s.velocity[1], s.velocity[2]))
            .unwrap_or((0.0, 0.0, 0.0))
    })
}

/// Set the linear velocity of an entity directly.
/// Units: m/s.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn part_set_velocity(entity_name: &str, x: f64, y: f64, z: f64) {
    PHYSICS_COMMANDS.with(|cmds| {
        cmds.borrow_mut().push(PhysicsCommand::SetVelocity {
            entity_name: entity_name.to_string(), x, y, z,
        });
    });
}

// ============================================================================
// Workspace Properties
// ============================================================================

thread_local! {
    /// Gravity value — defaults to Earth gravity (9.80665 m/s²).
    /// Shared between Rune scripts and the Avian3d physics engine.
    pub static WORKSPACE_GRAVITY: std::cell::RefCell<f64> = std::cell::RefCell::new(9.80665);
}

/// Get the current workspace gravity in m/s².
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn workspace_get_gravity() -> f64 {
    WORKSPACE_GRAVITY.with(|g| *g.borrow())
}

/// Set the workspace gravity in m/s². Affects all physics simulation.
/// Earth = 9.80665, Moon = 1.625, Mars = 3.72076, zero-g = 0.0.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn workspace_set_gravity(gravity: f64) {
    WORKSPACE_GRAVITY.with(|g| *g.borrow_mut() = gravity);
}

// ============================================================================
// Instance Delete
// ============================================================================

/// Delete an instance by removing its TOML file and referenced mesh binary
/// from any service directory. Searches Workspace, SoulService, MaterialService,
/// StarterGui, and all other top-level service directories in the Space.
/// Will not delete scaffolding files (_service.toml, _instance.toml, space.toml).
/// If the TOML references a .glb mesh asset, that binary is also deleted.
/// The engine despawns the entity on the next file-watcher cycle.
/// Returns true if found and deleted, false otherwise.
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn instance_delete(entity_name: &str) -> bool {
    SPACE_ROOT.with(|root| {
        let root = root.borrow();
        let Some(base) = root.as_ref() else { return false };
        let safe = entity_name.replace(' ', "_").replace('/', "_");

        let extensions = [
            ".part.toml", ".glb.toml", ".instance.toml",
            ".mat.toml", ".rune", ".lua", ".luau", ".soul",
        ];

        let mut search_dirs = vec![base.clone()];
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.starts_with('.') {
                        search_dirs.push(path);
                    }
                }
            }
        }

        // Try folder-based deletion first: dir/{name}/_instance.toml
        for dir in &search_dirs {
            let folder = dir.join(&safe);
            if folder.is_dir() && folder.join("_instance.toml").exists() {
                return std::fs::remove_dir_all(&folder).is_ok();
            }
        }

        // Legacy flat-file deletion
        for dir in &search_dirs {
            for ext in &extensions {
                let candidate = dir.join(format!("{}{}", safe, ext));
                if !candidate.exists() { continue; }

                let fname = candidate.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if fname.starts_with('_') || fname == "space.toml" || fname == "simulation.toml" {
                    continue;
                }

                // Before deleting, check if the TOML references a mesh binary
                if fname.ends_with(".glb.toml") || fname.ends_with(".part.toml") {
                    if let Ok(content) = std::fs::read_to_string(&candidate) {
                        if let Ok(val) = toml::from_str::<toml::Value>(&content) {
                            // Check [asset].path or [metadata].asset for referenced .glb
                            let mesh_path = val.get("asset")
                                .and_then(|a| a.get("path"))
                                .and_then(|p| p.as_str())
                                .or_else(|| val.get("metadata")
                                    .and_then(|m| m.get("asset"))
                                    .and_then(|a| a.as_str()));

                            if let Some(mesh_rel) = mesh_path {
                                // Resolve relative to the TOML file's parent directory
                                let mesh_abs = candidate.parent()
                                    .unwrap_or(dir)
                                    .join(mesh_rel);
                                if mesh_abs.exists() && mesh_abs.starts_with(base) {
                                    let _ = std::fs::remove_file(&mesh_abs);
                                }
                            }
                        }
                    }
                }

                return std::fs::remove_file(&candidate).is_ok();
            }
        }
        false
    })
}

/// Helper: find an entity's TOML file by name and apply a mutation.
/// Searches Workspace/ for folder/_instance.toml first, then legacy .part.toml/.glb.toml.
fn update_part_toml(entity_name: &str, mutate: impl FnOnce(&mut toml::Value)) {
    SPACE_ROOT.with(|root| {
        let root = root.borrow();
        let Some(base) = root.as_ref() else { return };
        let workspace = base.join("Workspace");
        let safe = entity_name.replace(' ', "_").replace('/', "_");
        let candidates = [
            workspace.join(&safe).join("_instance.toml"),
            workspace.join(format!("{}.part.toml", safe)),
            workspace.join(format!("{}.glb.toml", safe)),
        ];
        for path in &candidates {
            if !path.exists() { continue; }
            let Ok(content) = std::fs::read_to_string(path) else { continue };
            let Ok(mut doc) = toml::from_str::<toml::Value>(&content) else { continue };
            mutate(&mut doc);
            if let Ok(new_content) = toml::to_string_pretty(&doc) {
                let _ = std::fs::write(path, new_content);
            }
            return;
        }
    });
}

// ============================================================================
// GUI Scripting Bridge — Roblox-compatible UI API
// ============================================================================
// Uses shared GuiCommand / GUI_COMMANDS / GUI_SNAPSHOT from eustress_common::gui
// so both Rune and Luau push to the same command queue.
//
// Re-export for gui_bridge.rs compatibility:
pub use eustress_common::gui::{GuiCommand, push_gui_command, drain_gui_commands, set_gui_snapshot, gui_snapshot_get, clear_gui_snapshot};

// ── Rune API functions ─────────────────────────────────────────────────────

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_text(name: &str, text: &str) {
    push_gui_command(GuiCommand::SetText {
        name: name.to_string(),
        text: text.to_string(),
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_get_text(name: &str) -> String {
    gui_snapshot_get(name)
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_visible(name: &str, visible: bool) {
    push_gui_command(GuiCommand::SetVisible {
        name: name.to_string(),
        visible,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_bg_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetBgColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_text_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetTextColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_border_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetBorderColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_position(name: &str, x: f64, y: f64) {
    push_gui_command(GuiCommand::SetPosition {
        name: name.to_string(),
        x: x as f32, y: y as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_size(name: &str, w: f64, h: f64) {
    push_gui_command(GuiCommand::SetSize {
        name: name.to_string(),
        w: w as f32, h: h as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_font_size(name: &str, size: f64) {
    push_gui_command(GuiCommand::SetFontSize {
        name: name.to_string(),
        size: size as f32,
    });
}

/// Stub module when feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn create_ecs_module() -> Result<(), ()> {
    Ok(())
}
