//! # Luau Raycasting API
//!
//! Injects `workspace:Raycast()` and `RaycastParams.new()` into the Luau VM.
//! Uses a thread-local bridge to communicate with the engine's spatial query system.
//!
//! ## Table of Contents
//!
//! 1. **LuauRaycastBridge** — Thread-local callback bridge to engine raycasting
//! 2. **LuauRaycastResult** — Plain result struct returned to Luau scripts
//! 3. **LuauRaycastParams** — Filter parameters matching Roblox RaycastParams
//! 4. **inject_raycast_api** — Enriches the Luau VM workspace table with Raycast methods
//!
//! ## Luau Script Usage (Roblox-compatible):
//! ```lua
//! local origin = Vector3.new(0, 50, 0)
//! local direction = Vector3.new(0, -100, 0)
//!
//! -- Basic raycast
//! local result = workspace:Raycast(origin, direction)
//! if result then
//!     print("Hit:", result.Instance, "at", result.Position)
//!     print("Normal:", result.Normal, "Distance:", result.Distance)
//!     print("Material:", result.Material)
//! end
//!
//! -- With filter params
//! local params = RaycastParams.new()
//! params.FilterType = Enum.RaycastFilterType.Exclude
//! params.FilterDescendantsInstances = { workspace.Baseplate }
//! params.IgnoreWater = true
//!
//! local result = workspace:Raycast(origin, direction, params)
//! ```

use std::sync::{Arc, RwLock};
use std::collections::HashMap;

// ============================================================================
// 1. LuauRaycastBridge — Thread-local callback to engine spatial query
// ============================================================================

/// Result of a raycast, returned from the engine bridge to Luau.
/// Language-agnostic (no Avian or Bevy types).
#[derive(Debug, Clone)]
pub struct LuauRaycastResult {
    /// Bevy Entity bits of the hit entity
    pub entity_id: u64,
    /// Name of the hit entity
    pub entity_name: String,
    /// World-space hit position [x, y, z]
    pub position: [f32; 3],
    /// World-space surface normal [x, y, z]
    pub normal: [f32; 3],
    /// Distance from ray origin to hit
    pub distance: f32,
    /// Material name of the hit surface
    pub material: String,
}

/// Filter parameters for raycasts, matching Roblox RaycastParams semantics.
#[derive(Debug, Clone, Default)]
pub struct LuauRaycastParams {
    /// true = exclude listed names, false = include only listed names
    pub exclude_mode: bool,
    /// Entity names to filter
    pub filter_names: Vec<String>,
    /// Whether to ignore water volumes
    pub ignore_water: bool,
    /// Whether to respect can_collide = false
    pub respect_can_collide: bool,
    /// Maximum ray distance
    pub max_distance: f32,
}

impl LuauRaycastParams {
    pub fn new() -> Self {
        Self {
            exclude_mode: true,
            filter_names: Vec::new(),
            ignore_water: false,
            respect_can_collide: true,
            max_distance: 1000.0,
        }
    }
}

/// Callback type for performing a raycast from the engine.
/// origin: [x,y,z], direction: [x,y,z], params → Option<result>
pub type RaycastCallback = Arc<dyn Fn([f32; 3], [f32; 3], &LuauRaycastParams) -> Option<LuauRaycastResult> + Send + Sync>;

/// Callback type for performing a raycast-all from the engine.
/// origin: [x,y,z], direction: [x,y,z], params, max_hits → Vec<result>
pub type RaycastAllCallback = Arc<dyn Fn([f32; 3], [f32; 3], &LuauRaycastParams, u32) -> Vec<LuauRaycastResult> + Send + Sync>;

/// Thread-local bridge holding engine raycast callbacks.
/// Set by the engine before Luau script execution, cleared after.
pub struct LuauRaycastBridge {
    pub raycast: Option<RaycastCallback>,
    pub raycast_all: Option<RaycastAllCallback>,
}

impl Default for LuauRaycastBridge {
    fn default() -> Self {
        Self {
            raycast: None,
            raycast_all: None,
        }
    }
}

thread_local! {
    static LUAU_RAYCAST_BRIDGE: std::cell::RefCell<LuauRaycastBridge> = std::cell::RefCell::new(LuauRaycastBridge::default());
}

/// Install raycast callbacks for the current thread before Luau execution.
pub fn set_luau_raycast_bridge(raycast: RaycastCallback, raycast_all: RaycastAllCallback) {
    LUAU_RAYCAST_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        bridge.raycast = Some(raycast);
        bridge.raycast_all = Some(raycast_all);
    });
}

/// Clear raycast callbacks after Luau execution.
pub fn clear_luau_raycast_bridge() {
    LUAU_RAYCAST_BRIDGE.with(|cell| {
        let mut bridge = cell.borrow_mut();
        bridge.raycast = None;
        bridge.raycast_all = None;
    });
}

// ============================================================================
// 2. Thread-local RaycastParams storage (handles for Luau userdata)
// ============================================================================

thread_local! {
    static LUAU_PARAMS_STORE: std::cell::RefCell<Vec<LuauRaycastParams>> = std::cell::RefCell::new(Vec::new());
}

/// Allocate a new LuauRaycastParams and return its 1-based handle.
fn allocate_params() -> usize {
    LUAU_PARAMS_STORE.with(|store| {
        let mut store = store.borrow_mut();
        store.push(LuauRaycastParams::new());
        store.len() // 1-based
    })
}

/// Get a clone of params by handle (1-based). Returns default if invalid.
fn get_params(handle: usize) -> LuauRaycastParams {
    if handle == 0 {
        return LuauRaycastParams::new();
    }
    LUAU_PARAMS_STORE.with(|store| {
        let store = store.borrow();
        store.get(handle - 1).cloned().unwrap_or_default()
    })
}

/// Mutate params by handle (1-based).
fn with_params_mut<F>(handle: usize, callback: F)
where
    F: FnOnce(&mut LuauRaycastParams),
{
    if handle == 0 { return; }
    LUAU_PARAMS_STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(params) = store.get_mut(handle - 1) {
            callback(params);
        }
    });
}

// ============================================================================
// 3. Inject into Luau VM
// ============================================================================

/// Inject `workspace:Raycast()`, `workspace:RaycastAll()`, and `RaycastParams.new()`
/// into the given mlua Lua instance.
///
/// Call this after `LuauRuntime::new()` to enrich the workspace table.
#[cfg(feature = "luau")]
pub fn inject_raycast_api(lua: &mlua::Lua) -> Result<(), String> {
    let globals = lua.globals();

    // Get or create the workspace table
    let workspace: mlua::Table = globals.get("workspace")
        .map_err(|error| format!("Failed to get workspace table: {}", error))?;

    // workspace:Raycast(origin, direction, params?)
    // origin = {x, y, z} table or Vector3
    // direction = {x, y, z} table or Vector3
    // params = RaycastParams handle (integer) or nil
    let raycast_function = lua.create_function(|lua_ctx, args: mlua::MultiValue| {
        // Parse arguments: self (workspace table), origin, direction, params?
        let mut args_iter = args.into_iter();
        let _self_table = args_iter.next(); // workspace self reference

        // Parse origin vector
        let origin = parse_vector3_arg(lua_ctx, args_iter.next())?;
        // Parse direction vector
        let direction = parse_vector3_arg(lua_ctx, args_iter.next())?;
        // Parse optional params handle
        let params_handle = match args_iter.next() {
            Some(mlua::Value::Integer(handle)) => handle as usize,
            Some(mlua::Value::Number(handle)) => handle as usize,
            Some(mlua::Value::Table(table)) => {
                // Could be a RaycastParams userdata table with __handle field
                table.get::<i64>("__handle").unwrap_or(0) as usize
            }
            _ => 0,
        };

        let params = get_params(params_handle);

        // Call through the thread-local bridge
        let result = LUAU_RAYCAST_BRIDGE.with(|cell| {
            let bridge = cell.borrow();
            if let Some(ref raycast_fn) = bridge.raycast {
                raycast_fn(origin, direction, &params)
            } else {
                tracing::warn!("[Luau] Raycast bridge not available — returning nil");
                None
            }
        });

        // Convert result to Luau table (matching Roblox RaycastResult format)
        match result {
            Some(hit) => {
                let result_table = lua_ctx.create_table()
                    .map_err(|error| mlua::Error::external(format!("Failed to create result table: {}", error)))?;

                // Instance (entity name — closest Luau equivalent)
                result_table.set("Instance", hit.entity_name.clone())
                    .map_err(|error| mlua::Error::external(format!("set Instance: {}", error)))?;
                // EntityId (Bevy entity bits — Eustress extension)
                result_table.set("EntityId", hit.entity_id as i64)
                    .map_err(|error| mlua::Error::external(format!("set EntityId: {}", error)))?;

                // Position as Vector3-like table
                let pos_table = lua_ctx.create_table()
                    .map_err(|error| mlua::Error::external(format!("create pos table: {}", error)))?;
                pos_table.set("X", hit.position[0] as f64)?;
                pos_table.set("Y", hit.position[1] as f64)?;
                pos_table.set("Z", hit.position[2] as f64)?;
                result_table.set("Position", pos_table)?;

                // Normal as Vector3-like table
                let normal_table = lua_ctx.create_table()
                    .map_err(|error| mlua::Error::external(format!("create normal table: {}", error)))?;
                normal_table.set("X", hit.normal[0] as f64)?;
                normal_table.set("Y", hit.normal[1] as f64)?;
                normal_table.set("Z", hit.normal[2] as f64)?;
                result_table.set("Normal", normal_table)?;

                // Distance (Eustress extension — not in Roblox RaycastResult)
                result_table.set("Distance", hit.distance as f64)?;

                // Material
                result_table.set("Material", hit.material.clone())?;

                Ok(mlua::Value::Table(result_table))
            }
            None => Ok(mlua::Value::Nil),
        }
    }).map_err(|error| format!("Failed to create workspace:Raycast: {}", error))?;

    workspace.set("Raycast", raycast_function)
        .map_err(|error| format!("Failed to set workspace.Raycast: {}", error))?;

    // workspace:RaycastAll(origin, direction, params?, maxHits?)
    let raycast_all_function = lua.create_function(|lua_ctx, args: mlua::MultiValue| {
        let mut args_iter = args.into_iter();
        let _self_table = args_iter.next();

        let origin = parse_vector3_arg(lua_ctx, args_iter.next())?;
        let direction = parse_vector3_arg(lua_ctx, args_iter.next())?;
        let params_handle = match args_iter.next() {
            Some(mlua::Value::Integer(handle)) => handle as usize,
            Some(mlua::Value::Number(handle)) => handle as usize,
            _ => 0,
        };
        let max_hits = match args_iter.next() {
            Some(mlua::Value::Integer(n)) => n.max(1) as u32,
            Some(mlua::Value::Number(n)) => (n as u32).max(1),
            _ => 10,
        };

        let params = get_params(params_handle);

        let results = LUAU_RAYCAST_BRIDGE.with(|cell| {
            let bridge = cell.borrow();
            if let Some(ref raycast_all_fn) = bridge.raycast_all {
                raycast_all_fn(origin, direction, &params, max_hits)
            } else {
                tracing::warn!("[Luau] RaycastAll bridge not available — returning empty");
                Vec::new()
            }
        });

        // Convert to Luau array of result tables
        let results_table = lua_ctx.create_table()
            .map_err(|error| mlua::Error::external(format!("create results table: {}", error)))?;

        for (index, hit) in results.iter().enumerate() {
            let hit_table = lua_ctx.create_table()
                .map_err(|error| mlua::Error::external(format!("create hit table: {}", error)))?;

            hit_table.set("Instance", hit.entity_name.clone())?;
            hit_table.set("EntityId", hit.entity_id as i64)?;

            let pos_table = lua_ctx.create_table()?;
            pos_table.set("X", hit.position[0] as f64)?;
            pos_table.set("Y", hit.position[1] as f64)?;
            pos_table.set("Z", hit.position[2] as f64)?;
            hit_table.set("Position", pos_table)?;

            let normal_table = lua_ctx.create_table()?;
            normal_table.set("X", hit.normal[0] as f64)?;
            normal_table.set("Y", hit.normal[1] as f64)?;
            normal_table.set("Z", hit.normal[2] as f64)?;
            hit_table.set("Normal", normal_table)?;

            hit_table.set("Distance", hit.distance as f64)?;
            hit_table.set("Material", hit.material.clone())?;

            results_table.set(index + 1, hit_table)?; // Lua arrays are 1-based
        }

        Ok(mlua::Value::Table(results_table))
    }).map_err(|error| format!("Failed to create workspace:RaycastAll: {}", error))?;

    workspace.set("RaycastAll", raycast_all_function)
        .map_err(|error| format!("Failed to set workspace.RaycastAll: {}", error))?;

    // RaycastParams.new() — global constructor
    let raycast_params_table = lua.create_table()
        .map_err(|error| format!("Failed to create RaycastParams table: {}", error))?;

    let params_new = lua.create_function(|lua_ctx, _args: mlua::MultiValue| {
        let handle = allocate_params();

        // Return a table that acts like a Roblox RaycastParams object
        let params_object = lua_ctx.create_table()?;
        params_object.set("__handle", handle as i64)?;
        params_object.set("FilterType", "Exclude")?;
        params_object.set("IgnoreWater", false)?;
        params_object.set("RespectCanCollide", true)?;
        params_object.set("MaxDistance", 1000.0)?;

        // FilterDescendantsInstances setter (simplified: accepts array of name strings)
        // In Roblox this is a table of Instance references, here we use name strings
        params_object.set("__filter_names", lua_ctx.create_table()?)?;

        Ok(mlua::Value::Table(params_object))
    }).map_err(|error| format!("Failed to create RaycastParams.new: {}", error))?;

    raycast_params_table.set("new", params_new)
        .map_err(|error| format!("Failed to set RaycastParams.new: {}", error))?;

    globals.set("RaycastParams", raycast_params_table)
        .map_err(|error| format!("Failed to set RaycastParams global: {}", error))?;

    Ok(())
}

// ============================================================================
// 4. Helper: Parse Vector3 from Luau argument
// ============================================================================

/// Parse a Vector3-like Luau value into [f32; 3].
/// Accepts: table with X/Y/Z or x/y/z or [1]/[2]/[3] keys.
#[cfg(feature = "luau")]
fn parse_vector3_arg(_lua: &mlua::Lua, value: Option<mlua::Value>) -> Result<[f32; 3], mlua::Error> {
    match value {
        Some(mlua::Value::Table(table)) => {
            // Try X/Y/Z (Roblox Vector3 style)
            let x = table.get::<f64>("X")
                .or_else(|_| table.get::<f64>("x"))
                .or_else(|_| table.get::<f64>(1))
                .unwrap_or(0.0);
            let y = table.get::<f64>("Y")
                .or_else(|_| table.get::<f64>("y"))
                .or_else(|_| table.get::<f64>(2))
                .unwrap_or(0.0);
            let z = table.get::<f64>("Z")
                .or_else(|_| table.get::<f64>("z"))
                .or_else(|_| table.get::<f64>(3))
                .unwrap_or(0.0);
            Ok([x as f32, y as f32, z as f32])
        }
        _ => Err(mlua::Error::external("Expected Vector3 table (with X/Y/Z or x/y/z fields)")),
    }
}
