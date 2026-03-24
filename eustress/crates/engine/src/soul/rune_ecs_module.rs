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
    let mut module = Module::with_crate("eustress")?;
    
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
            if let Some(source) = reg.get(self.entity_id as u64) {
                if !source.archivable {
                    return None;
                }
                let new_id = reg.next_entity_id();
                let new_instance = eustress_common::scripting::InstanceData::new(
                    new_id,
                    &source.class_name,
                    &source.name,
                );
                reg.insert(new_instance);
                Some(InstanceRune { entity_id: new_id as i64 })
            } else {
                None
            }
        })
    }
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
/// 
/// ## Rune: `let info = eustress::tween_info_full(1.0, 1, 1, 0, false, 0.0);`
#[cfg(feature = "realism-scripting")]
#[rune::function]
fn tween_info_full(
    time: f64,
    easing_style: i32,
    easing_direction: i32,
    repeat_count: i32,
    reverses: bool,
    delay_time: f64,
) -> TweenInfoRune {
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
    pub headers: std::collections::HashMap<String, String>,
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
            
            // Collect headers
            let mut response_headers = std::collections::HashMap::new();
            for name in response.headers_names() {
                if let Some(value) = response.header(&name) {
                    response_headers.insert(name, value.to_string());
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
                headers: std::collections::HashMap::new(),
            }
        }
        Err(_) => {
            HttpResponseRune {
                success: false,
                status_code: 0,
                status_message: "Connection failed".to_string(),
                body: String::new(),
                headers: std::collections::HashMap::new(),
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

/// Stub module when feature is disabled
#[cfg(not(feature = "realism-scripting"))]
pub fn create_ecs_module() -> Result<(), ()> {
    Ok(())
}
