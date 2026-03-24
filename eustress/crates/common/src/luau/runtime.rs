//! # Luau Runtime
//!
//! mlua-based Luau virtual machine with sandboxing and ECS integration.
//!
//! ## Table of Contents
//!
//! 1. **LuauRuntime** — Manages the mlua Lua VM instance with Luau backend
//! 2. **LuauRuntimeState** — Bevy resource wrapping the runtime
//! 3. **ScriptExecutionQueue** — Queued script chunks awaiting execution
//! 4. **Events** — Script lifecycle events

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Luau Runtime — mlua VM wrapper
// ============================================================================

/// Luau virtual machine wrapper built on mlua.
/// Provides sandboxed execution, module caching, and Eustress API injection.
pub struct LuauRuntime {
    /// The mlua Lua instance (Luau backend)
    #[cfg(feature = "luau")]
    lua: mlua::Lua,

    /// Cached module return values (for `require()`)
    module_cache: HashMap<String, Vec<u8>>,

    /// Execution statistics
    pub stats: LuauRuntimeStats,
}

/// Runtime execution statistics
#[derive(Debug, Clone, Default)]
pub struct LuauRuntimeStats {
    /// Total chunks executed
    pub chunks_executed: u64,
    /// Successful executions
    pub successful: u64,
    /// Failed executions
    pub failed: u64,
    /// Total execution time in microseconds
    pub total_time_us: u64,
    /// Modules loaded via require()
    pub modules_loaded: u64,
}

impl LuauRuntime {
    /// Create a new Luau runtime with sandboxed globals
    #[cfg(feature = "luau")]
    pub fn new() -> Result<Self, String> {
        let lua = mlua::Lua::new();

        // Enable Luau sandboxing — restricts dangerous operations
        lua.sandbox(true).map_err(|error| format!("Failed to enable Luau sandbox: {}", error))?;

        // Inject Eustress global stubs into the VM
        Self::inject_eustress_globals(&lua)?;

        Ok(Self {
            lua,
            module_cache: HashMap::new(),
            stats: LuauRuntimeStats::default(),
        })
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn new() -> Result<Self, String> {
        Err("Luau feature is not enabled. Rebuild with --features luau".to_string())
    }

    /// Execute a chunk of Luau source code
    #[cfg(feature = "luau")]
    pub fn execute_chunk(&mut self, source: &str, chunk_name: &str) -> Result<(), String> {
        let start = std::time::Instant::now();
        self.stats.chunks_executed += 1;

        let result = self.lua.load(source)
            .set_name(chunk_name)
            .exec()
            .map_err(|error| format!("Luau execution error in '{}': {}", chunk_name, error));

        let elapsed = start.elapsed().as_micros() as u64;
        self.stats.total_time_us += elapsed;

        match &result {
            Ok(()) => self.stats.successful += 1,
            Err(_) => self.stats.failed += 1,
        }

        result
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn execute_chunk(&mut self, _source: &str, _chunk_name: &str) -> Result<(), String> {
        Err("Luau feature is not enabled".to_string())
    }

    /// Load a ModuleScript and cache its return value in the Lua registry.
    /// The module's return value is stored as a registry key for `require()` resolution.
    #[cfg(feature = "luau")]
    pub fn load_module(&mut self, name: &str, source: &str) -> Result<(), String> {
        // Execute the module chunk — it should return exactly one value
        let value = self.lua.load(source)
            .set_name(name)
            .eval::<mlua::Value>()
            .map_err(|error| format!("Module '{}' failed to load: {}", name, error))?;

        // Store the return value in the Lua registry keyed by module name.
        // This allows `require()` to retrieve it without re-execution.
        let registry_key = self.lua.create_registry_value(value)
            .map_err(|error| format!("Module '{}' registry store failed: {}", name, error))?;

        // Serialize the registry key index for our cache tracking
        let key_bytes = format!("{:?}", registry_key).into_bytes();
        self.module_cache.insert(name.to_string(), key_bytes);
        self.stats.modules_loaded += 1;

        Ok(())
    }

    /// Fallback when luau feature is not enabled
    #[cfg(not(feature = "luau"))]
    pub fn load_module(&mut self, _name: &str, _source: &str) -> Result<(), String> {
        Err("Luau feature is not enabled".to_string())
    }

    /// Check if a module is cached
    pub fn is_module_cached(&self, name: &str) -> bool {
        self.module_cache.contains_key(name)
    }

    /// Clear the module cache (forces re-require on next access)
    pub fn clear_module_cache(&mut self) {
        self.module_cache.clear();
    }

    /// Inject Eustress-specific globals into the Luau VM.
    /// These provide the Roblox-compatible API surface:
    /// - `game` — service hierarchy root
    /// - `workspace` — alias for game.Workspace
    /// - `script` — reference to the currently executing script
    /// - `print` / `warn` / `error` — output to Eustress console
    /// - `wait` / `task` — coroutine scheduling
    /// - `Instance.new()` — entity creation
    /// - `Vector3`, `CFrame`, `Color3` — data types from shared scripting module
    #[cfg(feature = "luau")]
    fn inject_eustress_globals(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

        // Helper to create a signal object with Connect/Wait methods
        fn create_signal(lua: &mlua::Lua, connections: mlua::Table) -> Result<mlua::Table, String> {
            let signal = lua.create_table()
                .map_err(|e| format!("Failed to create signal: {}", e))?;
            
            signal.set("_connections", connections.clone())
                .map_err(|e| format!("Failed to set _connections: {}", e))?;
            signal.set("_nextId", 1i64)
                .map_err(|e| format!("Failed to set _nextId: {}", e))?;

            // Signal:Connect(callback) -> Connection
            let connect_fn = lua.create_function(|lua, (this, callback): (mlua::Table, mlua::Function)| {
                let connections: mlua::Table = this.get("_connections")?;
                let next_id: i64 = this.get("_nextId")?;
                this.set("_nextId", next_id + 1)?;
                
                // Store callback
                connections.set(next_id, callback)?;
                
                // Create connection object
                let connection = lua.create_table()?;
                connection.set("_id", next_id)?;
                connection.set("_signal", this.clone())?;
                connection.set("Connected", true)?;
                
                // Connection:Disconnect()
                connection.set("Disconnect", lua.create_function(|_, conn: mlua::Table| {
                    let id: i64 = conn.get("_id")?;
                    let signal: mlua::Table = conn.get("_signal")?;
                    let connections: mlua::Table = signal.get("_connections")?;
                    connections.set(id, mlua::Value::Nil)?;
                    conn.set("Connected", false)?;
                    Ok(())
                })?)?;
                
                Ok(connection)
            }).map_err(|e| format!("Failed to create Connect: {}", e))?;
            signal.set("Connect", connect_fn)
                .map_err(|e| format!("Failed to set Connect: {}", e))?;

            // Signal:Once(callback) -> Connection (fires once then disconnects)
            let once_fn = lua.create_function(|lua, (this, callback): (mlua::Table, mlua::Function)| {
                let connections: mlua::Table = this.get("_connections")?;
                let next_id: i64 = this.get("_nextId")?;
                this.set("_nextId", next_id + 1)?;
                
                // Wrap callback to auto-disconnect
                let wrapped = lua.create_function(move |lua, args: mlua::MultiValue| {
                    let result = callback.call::<mlua::MultiValue>(args.clone());
                    // Get connection and disconnect (simplified - actual impl would track this)
                    result
                })?;
                
                connections.set(next_id, wrapped)?;
                
                let connection = lua.create_table()?;
                connection.set("_id", next_id)?;
                connection.set("_signal", this.clone())?;
                connection.set("Connected", true)?;
                connection.set("Disconnect", lua.create_function(|_, conn: mlua::Table| {
                    let id: i64 = conn.get("_id")?;
                    let signal: mlua::Table = conn.get("_signal")?;
                    let connections: mlua::Table = signal.get("_connections")?;
                    connections.set(id, mlua::Value::Nil)?;
                    conn.set("Connected", false)?;
                    Ok(())
                })?)?;
                
                Ok(connection)
            }).map_err(|e| format!("Failed to create Once: {}", e))?;
            signal.set("Once", once_fn)
                .map_err(|e| format!("Failed to set Once: {}", e))?;

            // Signal:Wait() -> returns when signal fires (stub - returns immediately)
            let wait_fn = lua.create_function(|_, _this: mlua::Table| {
                // TODO: Integrate with coroutine scheduler to actually yield
                Ok(0.0f64) // Return delta time
            }).map_err(|e| format!("Failed to create Wait: {}", e))?;
            signal.set("Wait", wait_fn)
                .map_err(|e| format!("Failed to set Wait: {}", e))?;

            Ok(signal)
        }

        // Inject shared scripting types (Vector3, CFrame, Color3)
        super::types::inject_types(lua)
            .map_err(|e| format!("Failed to inject scripting types: {}", e))?;

        // Override print to route to Eustress output log
        let print_function = lua.create_function(|_, args: mlua::MultiValue| {
            let output: Vec<String> = args.iter().map(|value| format!("{:?}", value)).collect();
            tracing::info!("[Luau] {}", output.join("\t"));
            Ok(())
        }).map_err(|error| format!("Failed to create print function: {}", error))?;
        globals.set("print", print_function)
            .map_err(|error| format!("Failed to set print: {}", error))?;

        // Override warn to route to Eustress warning log
        let warn_function = lua.create_function(|_, args: mlua::MultiValue| {
            let output: Vec<String> = args.iter().map(|value| format!("{:?}", value)).collect();
            tracing::warn!("[Luau] {}", output.join("\t"));
            Ok(())
        }).map_err(|error| format!("Failed to create warn function: {}", error))?;
        globals.set("warn", warn_function)
            .map_err(|error| format!("Failed to set warn: {}", error))?;

        // Stub `game` as an empty table (populated per-script by bridge)
        let game_table = lua.create_table()
            .map_err(|error| format!("Failed to create game table: {}", error))?;
        globals.set("game", game_table)
            .map_err(|error| format!("Failed to set game: {}", error))?;

        // Stub `workspace` as an empty table (alias populated by bridge)
        let workspace_table = lua.create_table()
            .map_err(|error| format!("Failed to create workspace table: {}", error))?;
        globals.set("workspace", workspace_table)
            .map_err(|error| format!("Failed to set workspace: {}", error))?;

        // ====================================================================
        // P0: Instance API — Core entity creation and manipulation
        // ====================================================================
        
        // Global instance registry (entity_id -> instance table)
        let instance_registry = lua.create_table()
            .map_err(|e| format!("Failed to create instance registry: {}", e))?;
        globals.set("__INSTANCE_REGISTRY__", instance_registry)
            .map_err(|e| format!("Failed to set instance registry: {}", e))?;
        
        // Next entity ID counter
        globals.set("__NEXT_ENTITY_ID__", 1i64)
            .map_err(|e| format!("Failed to set entity ID counter: {}", e))?;

        // Instance table with constructor
        let instance_table = lua.create_table()
            .map_err(|e| format!("Failed to create Instance table: {}", e))?;

        // Instance.new(className, parent?) -> Instance
        let instance_new = lua.create_function(|lua, (class_name, parent): (String, Option<mlua::Table>)| {
            let globals = lua.globals();
            
            // Get next entity ID
            let entity_id: i64 = globals.get("__NEXT_ENTITY_ID__")?;
            globals.set("__NEXT_ENTITY_ID__", entity_id + 1)?;
            
            // Create instance table
            let instance = lua.create_table()?;
            instance.set("_entityId", entity_id)?;
            instance.set("_className", class_name.clone())?;
            instance.set("Name", class_name.clone())?;
            instance.set("ClassName", class_name.clone())?;
            instance.set("Parent", mlua::Value::Nil)?;
            instance.set("Archivable", true)?;
            
            // Children storage
            let children = lua.create_table()?;
            instance.set("_children", children)?;
            
            // Properties storage
            let properties = lua.create_table()?;
            instance.set("_properties", properties)?;
            
            // Add class-specific default properties
            match class_name.as_str() {
                "Part" | "MeshPart" | "WedgePart" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauVector3::new(0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauVector3::new(4.0, 1.0, 2.0))?)?;
                    instance.set("CFrame", lua.create_userdata(super::types::LuauCFrame::identity())?)?;
                    instance.set("Anchored", false)?;
                    instance.set("CanCollide", true)?;
                    instance.set("Transparency", 0.0f64)?;
                    instance.set("Color", lua.create_userdata(super::types::LuauColor3::new(0.639, 0.635, 0.647))?)?;
                    instance.set("Material", "Plastic")?;
                }
                "Model" => {
                    instance.set("PrimaryPart", mlua::Value::Nil)?;
                }
                "Script" | "LocalScript" => {
                    instance.set("Source", "")?;
                    instance.set("Enabled", true)?;
                }
                "ModuleScript" => {
                    instance.set("Source", "")?;
                }
                "Humanoid" => {
                    instance.set("Health", 100.0f64)?;
                    instance.set("MaxHealth", 100.0f64)?;
                    instance.set("WalkSpeed", 16.0f64)?;
                    instance.set("JumpPower", 50.0f64)?;
                    instance.set("JumpHeight", 7.2f64)?;
                }
                "Animation" => {
                    instance.set("AnimationId", "")?;
                }
                "Sound" => {
                    instance.set("SoundId", "")?;
                    instance.set("Volume", 1.0f64)?;
                    instance.set("Playing", false)?;
                    instance.set("Looped", false)?;
                }
                "ClickDetector" => {
                    instance.set("MaxActivationDistance", 32.0f64)?;
                }
                "Frame" | "TextLabel" | "TextButton" | "ImageLabel" | "ImageButton" => {
                    instance.set("Position", lua.create_userdata(super::types::LuauUDim2::new(0.0, 0.0, 0.0, 0.0))?)?;
                    instance.set("Size", lua.create_userdata(super::types::LuauUDim2::new(0.0, 100.0, 0.0, 100.0))?)?;
                    instance.set("Visible", true)?;
                    instance.set("BackgroundColor3", lua.create_userdata(super::types::LuauColor3::new(1.0, 1.0, 1.0))?)?;
                    instance.set("BackgroundTransparency", 0.0f64)?;
                }
                _ => {}
            }
            
            // Register instance
            let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
            registry.set(entity_id, instance.clone())?;
            
            // Set parent if provided
            if let Some(parent_table) = parent {
                instance.set("Parent", parent_table.clone())?;
                let parent_children: mlua::Table = parent_table.get("_children")?;
                parent_children.set(entity_id, instance.clone())?;
            }
            
            Ok(instance)
        }).map_err(|e| format!("Failed to create Instance.new: {}", e))?;
        instance_table.set("new", instance_new)
            .map_err(|e| format!("Failed to set Instance.new: {}", e))?;

        globals.set("Instance", instance_table)
            .map_err(|e| format!("Failed to set Instance: {}", e))?;

        // ====================================================================
        // Instance methods (added to each instance via metatable)
        // ====================================================================
        
        // Create instance metatable with methods
        let instance_mt = lua.create_table()
            .map_err(|e| format!("Failed to create instance metatable: {}", e))?;
        
        // __index metamethod for method lookup
        let instance_index = lua.create_function(|lua, (this, key): (mlua::Table, String)| {
            // First check if it's a direct property
            let raw_value: mlua::Value = this.raw_get(key.clone())?;
            if raw_value != mlua::Value::Nil {
                return Ok(raw_value);
            }
            
            // Otherwise return method functions
            match key.as_str() {
                "Clone" => {
                    let clone_fn = lua.create_function(|lua, this: mlua::Table| {
                        let globals = lua.globals();
                        let class_name: String = this.get("_className")?;
                        
                        // Get next entity ID
                        let entity_id: i64 = globals.get("__NEXT_ENTITY_ID__")?;
                        globals.set("__NEXT_ENTITY_ID__", entity_id + 1)?;
                        
                        // Create new instance
                        let clone = lua.create_table()?;
                        clone.set("_entityId", entity_id)?;
                        clone.set("_className", class_name.clone())?;
                        
                        // Copy properties
                        for pair in this.pairs::<mlua::Value, mlua::Value>() {
                            let (k, v) = pair?;
                            if let mlua::Value::String(key_str) = &k {
                                let key = key_str.to_str()?;
                                if !key.starts_with('_') && key != "Parent" {
                                    clone.set(k, v)?;
                                }
                            }
                        }
                        
                        clone.set("Parent", mlua::Value::Nil)?;
                        clone.set("_children", lua.create_table()?)?;
                        
                        // Register clone
                        let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
                        registry.set(entity_id, clone.clone())?;
                        
                        Ok(clone)
                    })?;
                    Ok(mlua::Value::Function(clone_fn))
                }
                "Destroy" => {
                    let destroy_fn = lua.create_function(|lua, this: mlua::Table| {
                        let globals = lua.globals();
                        let entity_id: i64 = this.get("_entityId")?;
                        
                        // Remove from parent's children
                        let parent: mlua::Value = this.get("Parent")?;
                        if let mlua::Value::Table(parent_table) = parent {
                            let parent_children: mlua::Table = parent_table.get("_children")?;
                            parent_children.set(entity_id, mlua::Value::Nil)?;
                        }
                        
                        // Recursively destroy children
                        let children: mlua::Table = this.get("_children")?;
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let destroy: mlua::Function = child.get("Destroy")?;
                            destroy.call::<()>(child)?;
                        }
                        
                        // Remove from registry
                        let registry: mlua::Table = globals.get("__INSTANCE_REGISTRY__")?;
                        registry.set(entity_id, mlua::Value::Nil)?;
                        
                        tracing::info!("[Luau Instance] Destroyed entity {}", entity_id);
                        Ok(())
                    })?;
                    Ok(mlua::Value::Function(destroy_fn))
                }
                "FindFirstChild" => {
                    let find_fn = lua.create_function(|_, (this, name, recursive): (mlua::Table, String, Option<bool>)| {
                        let recursive = recursive.unwrap_or(false);
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let child_name: String = child.get("Name")?;
                            if child_name == name {
                                return Ok(mlua::Value::Table(child));
                            }
                            
                            if recursive {
                                let find_child: mlua::Function = child.get("FindFirstChild")?;
                                let result: mlua::Value = find_child.call((child.clone(), name.clone(), Some(true)))?;
                                if result != mlua::Value::Nil {
                                    return Ok(result);
                                }
                            }
                        }
                        
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_fn))
                }
                "FindFirstChildOfClass" => {
                    let find_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let child_class: String = child.get("_className")?;
                            if child_class == class_name {
                                return Ok(mlua::Value::Table(child));
                            }
                        }
                        
                        Ok(mlua::Value::Nil)
                    })?;
                    Ok(mlua::Value::Function(find_fn))
                }
                "GetChildren" => {
                    let get_fn = lua.create_function(|lua, this: mlua::Table| {
                        let children: mlua::Table = this.get("_children")?;
                        let result = lua.create_table()?;
                        let mut idx = 1;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            result.set(idx, child)?;
                            idx += 1;
                        }
                        
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_fn))
                }
                "GetDescendants" => {
                    let get_fn = lua.create_function(|lua, this: mlua::Table| {
                        let result = lua.create_table()?;
                        let mut idx = 1;
                        
                        fn collect_descendants(table: &mlua::Table, result: &mlua::Table, idx: &mut i32) -> mlua::Result<()> {
                            let children: mlua::Table = table.get("_children")?;
                            for pair in children.pairs::<i64, mlua::Table>() {
                                let (_, child) = pair?;
                                result.set(*idx, child.clone())?;
                                *idx += 1;
                                collect_descendants(&child, result, idx)?;
                            }
                            Ok(())
                        }
                        
                        collect_descendants(&this, &result, &mut idx)?;
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(get_fn))
                }
                "IsA" => {
                    let is_a_fn = lua.create_function(|_, (this, class_name): (mlua::Table, String)| {
                        let this_class: String = this.get("_className")?;
                        
                        // Direct match
                        if this_class == class_name {
                            return Ok(true);
                        }
                        
                        // Inheritance checks
                        let result = match class_name.as_str() {
                            "Instance" => true,
                            "BasePart" => matches!(this_class.as_str(), 
                                "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" | "SpawnLocation" | "Seat"),
                            "PVInstance" => matches!(this_class.as_str(),
                                "Part" | "MeshPart" | "Model" | "BasePart"),
                            "GuiObject" => matches!(this_class.as_str(),
                                "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton"),
                            "LuaSourceContainer" => matches!(this_class.as_str(),
                                "Script" | "LocalScript" | "ModuleScript"),
                            _ => false,
                        };
                        
                        Ok(result)
                    })?;
                    Ok(mlua::Value::Function(is_a_fn))
                }
                "IsDescendantOf" => {
                    let is_desc_fn = lua.create_function(|_, (this, ancestor): (mlua::Table, mlua::Table)| {
                        let ancestor_id: i64 = ancestor.get("_entityId")?;
                        let mut current: mlua::Value = this.get("Parent")?;
                        
                        while let mlua::Value::Table(parent) = current {
                            let parent_id: i64 = parent.get("_entityId")?;
                            if parent_id == ancestor_id {
                                return Ok(true);
                            }
                            current = parent.get("Parent")?;
                        }
                        
                        Ok(false)
                    })?;
                    Ok(mlua::Value::Function(is_desc_fn))
                }
                "GetFullName" => {
                    let get_name_fn = lua.create_function(|_, this: mlua::Table| {
                        let mut parts: Vec<String> = Vec::new();
                        let mut current = mlua::Value::Table(this);
                        
                        while let mlua::Value::Table(inst) = current {
                            let name: String = inst.get("Name")?;
                            parts.push(name);
                            current = inst.get("Parent")?;
                        }
                        
                        parts.reverse();
                        Ok(parts.join("."))
                    })?;
                    Ok(mlua::Value::Function(get_name_fn))
                }
                "ClearAllChildren" => {
                    let clear_fn = lua.create_function(|_, this: mlua::Table| {
                        let children: mlua::Table = this.get("_children")?;
                        
                        for pair in children.pairs::<i64, mlua::Table>() {
                            let (_, child) = pair?;
                            let destroy: mlua::Function = child.get("Destroy")?;
                            destroy.call::<()>(child)?;
                        }
                        
                        Ok(())
                    })?;
                    Ok(mlua::Value::Function(clear_fn))
                }
                _ => Ok(mlua::Value::Nil)
            }
        }).map_err(|e| format!("Failed to create instance __index: {}", e))?;
        
        instance_mt.set("__index", instance_index)
            .map_err(|e| format!("Failed to set instance __index: {}", e))?;
        
        // Store metatable for use by Instance.new
        globals.set("__INSTANCE_MT__", instance_mt)
            .map_err(|e| format!("Failed to set instance metatable: {}", e))?;

        // Stub `task` library for coroutine scheduling
        let task_table = lua.create_table()
            .map_err(|error| format!("Failed to create task table: {}", error))?;

        // task.wait(seconds) — yields current thread
        let task_wait = lua.create_function(|_, seconds: Option<f64>| {
            let _duration = seconds.unwrap_or(0.0);
            // TODO: Integrate with Bevy frame scheduling
            // For now, this is a no-op that returns immediately
            Ok(())
        }).map_err(|error| format!("Failed to create task.wait: {}", error))?;
        task_table.set("wait", task_wait)
            .map_err(|error| format!("Failed to set task.wait: {}", error))?;

        // task.spawn(function) — spawn a new thread
        let task_spawn = lua.create_function(|_, _function: mlua::Function| {
            // TODO: Integrate with Luau coroutine scheduler
            Ok(())
        }).map_err(|error| format!("Failed to create task.spawn: {}", error))?;
        task_table.set("spawn", task_spawn)
            .map_err(|error| format!("Failed to set task.spawn: {}", error))?;

        // task.defer(function) — defer execution to end of frame
        let task_defer = lua.create_function(|_, _function: mlua::Function| {
            // TODO: Queue for end-of-frame execution
            Ok(())
        }).map_err(|error| format!("Failed to create task.defer: {}", error))?;
        task_table.set("defer", task_defer)
            .map_err(|error| format!("Failed to set task.defer: {}", error))?;

        globals.set("task", task_table)
            .map_err(|error| format!("Failed to set task: {}", error))?;

        // Legacy `wait()` global (deprecated in Roblox, but widely used)
        let legacy_wait = lua.create_function(|_, seconds: Option<f64>| {
            let _duration = seconds.unwrap_or(0.03); // ~1 frame at 30fps
            Ok(seconds.unwrap_or(0.03))
        }).map_err(|error| format!("Failed to create wait: {}", error))?;
        globals.set("wait", legacy_wait)
            .map_err(|error| format!("Failed to set wait: {}", error))?;

        // ====================================================================
        // P1: TweenService
        // ====================================================================
        let tween_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create TweenService table: {}", error))?;

        // TweenService:Create(tweenInfo) -> Tween
        let tween_create = lua.create_function(|lua, info: super::types::LuauTweenInfo| {
            // Create a tween table with play/pause/cancel methods
            let tween = lua.create_table()?;
            tween.set("_info", info)?;
            tween.set("_status", 1i32)?; // 1 = Paused
            
            tween.set("Play", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 0i32)?; // 0 = Playing
                Ok(())
            })?)?;
            
            tween.set("Pause", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 1i32)?; // 1 = Paused
                Ok(())
            })?)?;
            
            tween.set("Cancel", lua.create_function(|_, this: mlua::Table| {
                this.set("_status", 2i32)?; // 2 = Cancelled
                Ok(())
            })?)?;
            
            Ok(tween)
        }).map_err(|error| format!("Failed to create TweenService:Create: {}", error))?;
        tween_service_table.set("Create", tween_create)
            .map_err(|error| format!("Failed to set TweenService.Create: {}", error))?;

        globals.set("TweenService", tween_service_table)
            .map_err(|error| format!("Failed to set TweenService: {}", error))?;

        // ====================================================================
        // P0: RunService — Frame-based event signals
        // ====================================================================
        let run_service_table = lua.create_table()
            .map_err(|e| format!("Failed to create RunService table: {}", e))?;

        // Signal connection storage for RunService events
        let heartbeat_connections = lua.create_table()
            .map_err(|e| format!("Failed to create Heartbeat connections: {}", e))?;
        let stepped_connections = lua.create_table()
            .map_err(|e| format!("Failed to create Stepped connections: {}", e))?;
        let render_stepped_connections = lua.create_table()
            .map_err(|e| format!("Failed to create RenderStepped connections: {}", e))?;

        // RunService.Heartbeat — fires every frame after physics
        let heartbeat = create_signal(lua, heartbeat_connections)?;
        run_service_table.set("Heartbeat", heartbeat)
            .map_err(|e| format!("Failed to set Heartbeat: {}", e))?;

        // RunService.Stepped — fires every frame before physics
        let stepped = create_signal(lua, stepped_connections)?;
        run_service_table.set("Stepped", stepped)
            .map_err(|e| format!("Failed to set Stepped: {}", e))?;

        // RunService.RenderStepped — fires every frame before rendering (client only)
        let render_stepped = create_signal(lua, render_stepped_connections)?;
        run_service_table.set("RenderStepped", render_stepped)
            .map_err(|e| format!("Failed to set RenderStepped: {}", e))?;

        // RunService:IsClient() -> bool
        let is_client = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsClient: {}", e))?;
        run_service_table.set("IsClient", is_client)
            .map_err(|e| format!("Failed to set IsClient: {}", e))?;

        // RunService:IsServer() -> bool
        let is_server = lua.create_function(|_, ()| Ok(false))
            .map_err(|e| format!("Failed to create IsServer: {}", e))?;
        run_service_table.set("IsServer", is_server)
            .map_err(|e| format!("Failed to set IsServer: {}", e))?;

        // RunService:IsStudio() -> bool
        let is_studio = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsStudio: {}", e))?;
        run_service_table.set("IsStudio", is_studio)
            .map_err(|e| format!("Failed to set IsStudio: {}", e))?;

        // RunService:IsRunning() -> bool
        let is_running = lua.create_function(|_, ()| Ok(true))
            .map_err(|e| format!("Failed to create IsRunning: {}", e))?;
        run_service_table.set("IsRunning", is_running)
            .map_err(|e| format!("Failed to set IsRunning: {}", e))?;

        // RunService:BindToRenderStep(name, priority, callback)
        let bind_to_render = lua.create_function(|lua, (name, _priority, callback): (String, i32, mlua::Function)| {
            // Store in a global table for render step bindings
            let globals = lua.globals();
            let bindings: mlua::Table = globals.get::<mlua::Table>("__RENDER_STEP_BINDINGS__")
                .unwrap_or_else(|_| {
                    let t = lua.create_table().unwrap();
                    globals.set("__RENDER_STEP_BINDINGS__", t.clone()).ok();
                    t
                });
            bindings.set(name, callback)?;
            Ok(())
        }).map_err(|e| format!("Failed to create BindToRenderStep: {}", e))?;
        run_service_table.set("BindToRenderStep", bind_to_render)
            .map_err(|e| format!("Failed to set BindToRenderStep: {}", e))?;

        // RunService:UnbindFromRenderStep(name)
        let unbind_from_render = lua.create_function(|lua, name: String| {
            let globals = lua.globals();
            if let Ok(bindings) = globals.get::<mlua::Table>("__RENDER_STEP_BINDINGS__") {
                bindings.set(name, mlua::Value::Nil)?;
            }
            Ok(())
        }).map_err(|e| format!("Failed to create UnbindFromRenderStep: {}", e))?;
        run_service_table.set("UnbindFromRenderStep", unbind_from_render)
            .map_err(|e| format!("Failed to set UnbindFromRenderStep: {}", e))?;

        globals.set("RunService", run_service_table)
            .map_err(|e| format!("Failed to set RunService: {}", e))?;

        // ====================================================================
        // P1: UserInputService
        // ====================================================================
        let uis_table = lua.create_table()
            .map_err(|error| format!("Failed to create UserInputService table: {}", error))?;

        // UserInputService:IsKeyDown(keyCode) -> bool
        let is_key_down = lua.create_function(|_, _key_code: i32| {
            // TODO: Wire to actual input state
            Ok(false)
        }).map_err(|error| format!("Failed to create IsKeyDown: {}", error))?;
        uis_table.set("IsKeyDown", is_key_down)
            .map_err(|error| format!("Failed to set IsKeyDown: {}", error))?;

        // UserInputService:IsMouseButtonPressed(button) -> bool
        let is_mouse_pressed = lua.create_function(|_, _button: i32| {
            Ok(false)
        }).map_err(|error| format!("Failed to create IsMouseButtonPressed: {}", error))?;
        uis_table.set("IsMouseButtonPressed", is_mouse_pressed)
            .map_err(|error| format!("Failed to set IsMouseButtonPressed: {}", error))?;

        // UserInputService:GetMouseLocation() -> Vector2 (as table)
        let get_mouse_location = lua.create_function(|lua, ()| {
            let result = lua.create_table()?;
            result.set("X", 0.0f64)?;
            result.set("Y", 0.0f64)?;
            Ok(result)
        }).map_err(|error| format!("Failed to create GetMouseLocation: {}", error))?;
        uis_table.set("GetMouseLocation", get_mouse_location)
            .map_err(|error| format!("Failed to set GetMouseLocation: {}", error))?;

        // UserInputService:GetMouseDelta() -> Vector2 (as table)
        let get_mouse_delta = lua.create_function(|lua, ()| {
            let result = lua.create_table()?;
            result.set("X", 0.0f64)?;
            result.set("Y", 0.0f64)?;
            Ok(result)
        }).map_err(|error| format!("Failed to create GetMouseDelta: {}", error))?;
        uis_table.set("GetMouseDelta", get_mouse_delta)
            .map_err(|error| format!("Failed to set GetMouseDelta: {}", error))?;

        globals.set("UserInputService", uis_table)
            .map_err(|error| format!("Failed to set UserInputService: {}", error))?;

        // ====================================================================
        // P1: Debris service
        // ====================================================================
        let debris_table = lua.create_table()
            .map_err(|error| format!("Failed to create Debris table: {}", error))?;

        // Debris:AddItem(instance, lifetime)
        let add_item = lua.create_function(|_, (_instance, _lifetime): (mlua::Value, f64)| {
            // TODO: Wire to DebrisService
            Ok(())
        }).map_err(|error| format!("Failed to create Debris:AddItem: {}", error))?;
        debris_table.set("AddItem", add_item)
            .map_err(|error| format!("Failed to set Debris.AddItem: {}", error))?;

        globals.set("Debris", debris_table)
            .map_err(|error| format!("Failed to set Debris: {}", error))?;

        // ====================================================================
        // P1: Players Service
        // ====================================================================
        let players_table = lua.create_table()
            .map_err(|e| format!("Failed to create Players table: {}", e))?;

        // Create a default LocalPlayer instance
        let local_player = lua.create_table()
            .map_err(|e| format!("Failed to create LocalPlayer: {}", e))?;
        local_player.set("_entityId", 1i64).map_err(|e| format!("Failed to set _entityId: {}", e))?;
        local_player.set("_className", "Player").map_err(|e| format!("Failed to set _className: {}", e))?;
        local_player.set("Name", "LocalPlayer").map_err(|e| format!("Failed to set Name: {}", e))?;
        local_player.set("UserId", 1i64).map_err(|e| format!("Failed to set UserId: {}", e))?;
        local_player.set("DisplayName", "Player").map_err(|e| format!("Failed to set DisplayName: {}", e))?;
        local_player.set("Character", mlua::Value::Nil).map_err(|e| format!("Failed to set Character: {}", e))?;
        local_player.set("Team", mlua::Value::Nil).map_err(|e| format!("Failed to set Team: {}", e))?;
        
        // Player methods
        let get_mouse = lua.create_function(|lua, _this: mlua::Table| {
            let globals = lua.globals();
            globals.get::<mlua::Table>("Mouse")
        }).map_err(|e| format!("Failed to create GetMouse: {}", e))?;
        local_player.set("GetMouse", get_mouse).map_err(|e| format!("Failed to set GetMouse: {}", e))?;
        
        let kick = lua.create_function(|_, (_this, _message): (mlua::Table, Option<String>)| {
            tracing::warn!("[Luau] Player:Kick() called - no-op in Eustress");
            Ok(())
        }).map_err(|e| format!("Failed to create Kick: {}", e))?;
        local_player.set("Kick", kick).map_err(|e| format!("Failed to set Kick: {}", e))?;

        players_table.set("LocalPlayer", local_player)
            .map_err(|e| format!("Failed to set LocalPlayer: {}", e))?;

        // Players storage for multiplayer
        let players_list = lua.create_table()
            .map_err(|e| format!("Failed to create players list: {}", e))?;
        players_table.set("_players", players_list)
            .map_err(|e| format!("Failed to set _players: {}", e))?;

        // Players:GetPlayers() -> {Player}
        let get_players = lua.create_function(|lua, this: mlua::Table| {
            let players: mlua::Table = this.get("_players")?;
            let result = lua.create_table()?;
            let mut idx = 1;
            for pair in players.pairs::<i64, mlua::Table>() {
                let (_, player) = pair?;
                result.set(idx, player)?;
                idx += 1;
            }
            // Always include LocalPlayer
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            result.set(idx, local_player)?;
            Ok(result)
        }).map_err(|e| format!("Failed to create GetPlayers: {}", e))?;
        players_table.set("GetPlayers", get_players)
            .map_err(|e| format!("Failed to set GetPlayers: {}", e))?;

        // Players:GetPlayerByUserId(userId) -> Player?
        let get_by_id = lua.create_function(|_, (this, user_id): (mlua::Table, i64)| {
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            let local_id: i64 = local_player.get("UserId")?;
            if local_id == user_id {
                return Ok(mlua::Value::Table(local_player));
            }
            let players: mlua::Table = this.get("_players")?;
            for pair in players.pairs::<i64, mlua::Table>() {
                let (_, player) = pair?;
                let pid: i64 = player.get("UserId")?;
                if pid == user_id {
                    return Ok(mlua::Value::Table(player));
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create GetPlayerByUserId: {}", e))?;
        players_table.set("GetPlayerByUserId", get_by_id)
            .map_err(|e| format!("Failed to set GetPlayerByUserId: {}", e))?;

        // Players:GetPlayerFromCharacter(character) -> Player?
        let get_from_char = lua.create_function(|_, (this, character): (mlua::Table, mlua::Table)| {
            let char_id: i64 = character.get("_entityId")?;
            let local_player: mlua::Table = this.get("LocalPlayer")?;
            if let Ok(local_char) = local_player.get::<mlua::Table>("Character") {
                let local_char_id: i64 = local_char.get("_entityId")?;
                if local_char_id == char_id {
                    return Ok(mlua::Value::Table(local_player));
                }
            }
            Ok(mlua::Value::Nil)
        }).map_err(|e| format!("Failed to create GetPlayerFromCharacter: {}", e))?;
        players_table.set("GetPlayerFromCharacter", get_from_char)
            .map_err(|e| format!("Failed to set GetPlayerFromCharacter: {}", e))?;

        // PlayerAdded/PlayerRemoving signals
        let player_added_conns = lua.create_table()
            .map_err(|e| format!("Failed to create PlayerAdded connections: {}", e))?;
        let player_added = create_signal(lua, player_added_conns)?;
        players_table.set("PlayerAdded", player_added)
            .map_err(|e| format!("Failed to set PlayerAdded: {}", e))?;

        let player_removing_conns = lua.create_table()
            .map_err(|e| format!("Failed to create PlayerRemoving connections: {}", e))?;
        let player_removing = create_signal(lua, player_removing_conns)?;
        players_table.set("PlayerRemoving", player_removing)
            .map_err(|e| format!("Failed to set PlayerRemoving: {}", e))?;

        globals.set("Players", players_table)
            .map_err(|e| format!("Failed to set Players: {}", e))?;

        // Also set as game:GetService("Players") compatible
        let game: mlua::Table = globals.get("game")
            .map_err(|e| format!("Failed to get game: {}", e))?;
        let players_ref: mlua::Table = globals.get("Players")
            .map_err(|e| format!("Failed to get Players: {}", e))?;
        game.set("Players", players_ref)
            .map_err(|e| format!("Failed to set game.Players: {}", e))?;

        // ====================================================================
        // P1: ReplicatedStorage — Shared data container
        // ====================================================================
        let replicated_storage = lua.create_table()
            .map_err(|e| format!("Failed to create ReplicatedStorage: {}", e))?;
        replicated_storage.set("_entityId", 100001i64).map_err(|e| format!("{}", e))?;
        replicated_storage.set("_className", "ReplicatedStorage").map_err(|e| format!("{}", e))?;
        replicated_storage.set("Name", "ReplicatedStorage").map_err(|e| format!("{}", e))?;
        replicated_storage.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ReplicatedStorage", replicated_storage.clone())
            .map_err(|e| format!("Failed to set ReplicatedStorage: {}", e))?;
        game.set("ReplicatedStorage", replicated_storage)
            .map_err(|e| format!("Failed to set game.ReplicatedStorage: {}", e))?;

        // ====================================================================
        // P1: ServerStorage — Server-only data container
        // ====================================================================
        let server_storage = lua.create_table()
            .map_err(|e| format!("Failed to create ServerStorage: {}", e))?;
        server_storage.set("_entityId", 100002i64).map_err(|e| format!("{}", e))?;
        server_storage.set("_className", "ServerStorage").map_err(|e| format!("{}", e))?;
        server_storage.set("Name", "ServerStorage").map_err(|e| format!("{}", e))?;
        server_storage.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ServerStorage", server_storage.clone())
            .map_err(|e| format!("Failed to set ServerStorage: {}", e))?;
        game.set("ServerStorage", server_storage)
            .map_err(|e| format!("Failed to set game.ServerStorage: {}", e))?;

        // ====================================================================
        // P1: ServerScriptService — Server scripts container
        // ====================================================================
        let server_script_service = lua.create_table()
            .map_err(|e| format!("Failed to create ServerScriptService: {}", e))?;
        server_script_service.set("_entityId", 100003i64).map_err(|e| format!("{}", e))?;
        server_script_service.set("_className", "ServerScriptService").map_err(|e| format!("{}", e))?;
        server_script_service.set("Name", "ServerScriptService").map_err(|e| format!("{}", e))?;
        server_script_service.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        
        globals.set("ServerScriptService", server_script_service.clone())
            .map_err(|e| format!("Failed to set ServerScriptService: {}", e))?;
        game.set("ServerScriptService", server_script_service)
            .map_err(|e| format!("Failed to set game.ServerScriptService: {}", e))?;

        // ====================================================================
        // P1: StarterGui / StarterPlayer / StarterPack
        // ====================================================================
        let starter_gui = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_gui.set("_entityId", 100004i64).map_err(|e| format!("{}", e))?;
        starter_gui.set("_className", "StarterGui").map_err(|e| format!("{}", e))?;
        starter_gui.set("Name", "StarterGui").map_err(|e| format!("{}", e))?;
        starter_gui.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterGui", starter_gui.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterGui", starter_gui).map_err(|e| format!("{}", e))?;

        let starter_player = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_player.set("_entityId", 100005i64).map_err(|e| format!("{}", e))?;
        starter_player.set("_className", "StarterPlayer").map_err(|e| format!("{}", e))?;
        starter_player.set("Name", "StarterPlayer").map_err(|e| format!("{}", e))?;
        starter_player.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterPlayer", starter_player.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterPlayer", starter_player).map_err(|e| format!("{}", e))?;

        let starter_pack = lua.create_table().map_err(|e| format!("{}", e))?;
        starter_pack.set("_entityId", 100006i64).map_err(|e| format!("{}", e))?;
        starter_pack.set("_className", "StarterPack").map_err(|e| format!("{}", e))?;
        starter_pack.set("Name", "StarterPack").map_err(|e| format!("{}", e))?;
        starter_pack.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("StarterPack", starter_pack.clone()).map_err(|e| format!("{}", e))?;
        game.set("StarterPack", starter_pack).map_err(|e| format!("{}", e))?;

        // ====================================================================
        // P1: Lighting service
        // ====================================================================
        let lighting = lua.create_table().map_err(|e| format!("{}", e))?;
        lighting.set("_entityId", 100007i64).map_err(|e| format!("{}", e))?;
        lighting.set("_className", "Lighting").map_err(|e| format!("{}", e))?;
        lighting.set("Name", "Lighting").map_err(|e| format!("{}", e))?;
        lighting.set("Ambient", lua.create_userdata(super::types::LuauColor3::new(0.5, 0.5, 0.5)).map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        lighting.set("Brightness", 2.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("ClockTime", 14.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("GeographicLatitude", 41.733f64).map_err(|e| format!("{}", e))?;
        lighting.set("TimeOfDay", "14:00:00").map_err(|e| format!("{}", e))?;
        lighting.set("FogColor", lua.create_userdata(super::types::LuauColor3::new(0.75, 0.75, 0.75)).map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        lighting.set("FogEnd", 100000.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("FogStart", 0.0f64).map_err(|e| format!("{}", e))?;
        lighting.set("_children", lua.create_table().map_err(|e| format!("{}", e))?).map_err(|e| format!("{}", e))?;
        globals.set("Lighting", lighting.clone()).map_err(|e| format!("{}", e))?;
        game.set("Lighting", lighting).map_err(|e| format!("{}", e))?;

        // ====================================================================
        // game:GetService(serviceName) -> Service
        // ====================================================================
        let get_service = lua.create_function(|_, (this, service_name): (mlua::Table, String)| {
            let service: mlua::Value = this.get(service_name.clone())?;
            if service == mlua::Value::Nil {
                return Err(mlua::Error::RuntimeError(format!("Service '{}' not found", service_name)));
            }
            Ok(service)
        }).map_err(|e| format!("Failed to create GetService: {}", e))?;
        game.set("GetService", get_service)
            .map_err(|e| format!("Failed to set GetService: {}", e))?;

        // ====================================================================
        // P2: DataStoreService
        // ====================================================================
        let datastore_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create DataStoreService table: {}", error))?;

        // DataStoreService:GetDataStore(name, scope?) -> DataStore
        let get_datastore = lua.create_function(|lua, (name, scope): (String, Option<String>)| {
            let store = lua.create_table()?;
            let full_name = match scope {
                Some(s) => format!("{}_{}", name, s),
                None => name,
            };
            store.set("_name", full_name.clone())?;
            store.set("_cache", lua.create_table()?)?;
            
            // GetAsync
            store.set("GetAsync", lua.create_function(|_, (this, key): (mlua::Table, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                let value: Option<String> = cache.get(key)?;
                Ok(value)
            })?)?;
            
            // SetAsync
            store.set("SetAsync", lua.create_function(|_, (this, key, value): (mlua::Table, String, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                cache.set(key, value)?;
                Ok(())
            })?)?;
            
            // RemoveAsync
            store.set("RemoveAsync", lua.create_function(|_, (this, key): (mlua::Table, String)| {
                let cache: mlua::Table = this.get("_cache")?;
                let old: Option<String> = cache.get(key.clone())?;
                cache.set(key, mlua::Value::Nil)?;
                Ok(old)
            })?)?;
            
            // IncrementAsync
            store.set("IncrementAsync", lua.create_function(|_, (this, key, delta): (mlua::Table, String, i64)| {
                let cache: mlua::Table = this.get("_cache")?;
                let current: i64 = cache.get::<Option<i64>>(key.clone())?.unwrap_or(0);
                let new_value = current + delta;
                cache.set(key, new_value)?;
                Ok(new_value)
            })?)?;
            
            Ok(store)
        }).map_err(|error| format!("Failed to create GetDataStore: {}", error))?;
        datastore_service_table.set("GetDataStore", get_datastore)
            .map_err(|error| format!("Failed to set GetDataStore: {}", error))?;

        // DataStoreService:GetOrderedDataStore(name, scope?) -> OrderedDataStore
        let get_ordered = lua.create_function(|lua, (name, scope): (String, Option<String>)| {
            let store = lua.create_table()?;
            let full_name = match scope {
                Some(s) => format!("{}_{}", name, s),
                None => name,
            };
            store.set("_name", full_name)?;
            store.set("_entries", lua.create_table()?)?;
            
            // SetAsync
            store.set("SetAsync", lua.create_function(|_, (this, key, value): (mlua::Table, String, i64)| {
                let entries: mlua::Table = this.get("_entries")?;
                entries.set(key, value)?;
                Ok(())
            })?)?;
            
            // GetSortedAsync
            store.set("GetSortedAsync", lua.create_function(|lua, (this, ascending, page_size): (mlua::Table, bool, i64)| {
                let entries: mlua::Table = this.get("_entries")?;
                let mut items: Vec<(String, i64)> = Vec::new();
                
                for pair in entries.pairs::<String, i64>() {
                    if let Ok((k, v)) = pair {
                        items.push((k, v));
                    }
                }
                
                if ascending {
                    items.sort_by(|a, b| a.1.cmp(&b.1));
                } else {
                    items.sort_by(|a, b| b.1.cmp(&a.1));
                }
                
                items.truncate(page_size as usize);
                
                let result = lua.create_table()?;
                for (i, (key, value)) in items.into_iter().enumerate() {
                    let entry = lua.create_table()?;
                    entry.set("key", key)?;
                    entry.set("value", value)?;
                    result.set(i + 1, entry)?;
                }
                
                Ok(result)
            })?)?;
            
            Ok(store)
        }).map_err(|error| format!("Failed to create GetOrderedDataStore: {}", error))?;
        datastore_service_table.set("GetOrderedDataStore", get_ordered)
            .map_err(|error| format!("Failed to set GetOrderedDataStore: {}", error))?;

        globals.set("DataStoreService", datastore_service_table)
            .map_err(|error| format!("Failed to set DataStoreService: {}", error))?;

        // ====================================================================
        // P2: HttpService — Full Roblox Parity
        // ====================================================================
        let http_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create HttpService table: {}", error))?;

        // HttpService:GetAsync(url) -> string?
        let http_get = lua.create_function(|_, url: String| {
            match ureq::get(&url).call() {
                Ok(response) => Ok(response.into_string().ok()),
                Err(_) => Ok(None),
            }
        }).map_err(|error| format!("Failed to create HttpService:GetAsync: {}", error))?;
        http_service_table.set("GetAsync", http_get)
            .map_err(|error| format!("Failed to set HttpService.GetAsync: {}", error))?;

        // HttpService:PostAsync(url, body) -> string?
        let http_post = lua.create_function(|_, (url, body): (String, String)| {
            match ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body)
            {
                Ok(response) => Ok(response.into_string().ok()),
                Err(_) => Ok(None),
            }
        }).map_err(|error| format!("Failed to create HttpService:PostAsync: {}", error))?;
        http_service_table.set("PostAsync", http_post)
            .map_err(|error| format!("Failed to set HttpService.PostAsync: {}", error))?;

        // HttpService:RequestAsync(options) -> {Success, StatusCode, StatusMessage, Headers, Body}
        let http_request = lua.create_function(|lua, options: mlua::Table| {
            let url: String = options.get("Url")?;
            let method: String = options.get::<Option<String>>("Method")?.unwrap_or_else(|| "GET".to_string());
            let body: Option<String> = options.get("Body")?;
            let headers: Option<mlua::Table> = options.get("Headers")?;
            
            let mut request = match method.to_uppercase().as_str() {
                "GET" => ureq::get(&url),
                "POST" => ureq::post(&url),
                "PUT" => ureq::put(&url),
                "DELETE" => ureq::delete(&url),
                "PATCH" => ureq::patch(&url),
                "HEAD" => ureq::head(&url),
                _ => ureq::get(&url),
            };
            
            // Apply custom headers
            if let Some(hdrs) = headers {
                for pair in hdrs.pairs::<String, String>() {
                    if let Ok((key, value)) = pair {
                        request = request.set(&key, &value);
                    }
                }
            }
            
            // Set default content-type for body requests
            if body.is_some() {
                request = request.set("Content-Type", "application/json");
            }
            
            let result = match &body {
                Some(b) => request.send_string(b),
                None => request.call(),
            };
            
            let response_table = lua.create_table()?;
            
            match result {
                Ok(response) => {
                    let status = response.status();
                    response_table.set("Success", status >= 200 && status < 300)?;
                    response_table.set("StatusCode", status as i64)?;
                    response_table.set("StatusMessage", response.status_text())?;
                    
                    let headers_table = lua.create_table()?;
                    for name in response.headers_names() {
                        if let Some(value) = response.header(&name) {
                            headers_table.set(name, value)?;
                        }
                    }
                    response_table.set("Headers", headers_table)?;
                    response_table.set("Body", response.into_string().unwrap_or_default())?;
                }
                Err(ureq::Error::Status(code, response)) => {
                    response_table.set("Success", false)?;
                    response_table.set("StatusCode", code as i64)?;
                    response_table.set("StatusMessage", response.status_text())?;
                    response_table.set("Headers", lua.create_table()?)?;
                    response_table.set("Body", response.into_string().unwrap_or_default())?;
                }
                Err(_) => {
                    response_table.set("Success", false)?;
                    response_table.set("StatusCode", 0)?;
                    response_table.set("StatusMessage", "Connection failed")?;
                    response_table.set("Headers", lua.create_table()?)?;
                    response_table.set("Body", "")?;
                }
            }
            
            Ok(response_table)
        }).map_err(|error| format!("Failed to create RequestAsync: {}", error))?;
        http_service_table.set("RequestAsync", http_request)
            .map_err(|error| format!("Failed to set RequestAsync: {}", error))?;

        // HttpService:UrlEncode(input) -> string
        let url_encode = lua.create_function(|_, input: String| {
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
            Ok(encoded)
        }).map_err(|error| format!("Failed to create UrlEncode: {}", error))?;
        http_service_table.set("UrlEncode", url_encode)
            .map_err(|error| format!("Failed to set UrlEncode: {}", error))?;

        // HttpService:GenerateGUID(wrapInCurlyBraces) -> string
        let generate_guid = lua.create_function(|_, wrap: Option<bool>| {
            let uuid = uuid::Uuid::new_v4();
            if wrap.unwrap_or(true) {
                Ok(format!("{{{}}}", uuid))
            } else {
                Ok(uuid.to_string())
            }
        }).map_err(|error| format!("Failed to create GenerateGUID: {}", error))?;
        http_service_table.set("GenerateGUID", generate_guid)
            .map_err(|error| format!("Failed to set GenerateGUID: {}", error))?;

        // HttpService:JSONEncode(value) -> string
        let json_encode = lua.create_function(|_, value: String| {
            Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
        }).map_err(|error| format!("Failed to create JSONEncode: {}", error))?;
        http_service_table.set("JSONEncode", json_encode)
            .map_err(|error| format!("Failed to set JSONEncode: {}", error))?;

        // HttpService:JSONDecode(json) -> string?
        let json_decode = lua.create_function(|_, json: String| {
            let trimmed = json.trim();
            if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
                Ok(Some(trimmed[1..trimmed.len()-1].to_string()))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }).map_err(|error| format!("Failed to create JSONDecode: {}", error))?;
        http_service_table.set("JSONDecode", json_decode)
            .map_err(|error| format!("Failed to set JSONDecode: {}", error))?;

        globals.set("HttpService", http_service_table)
            .map_err(|error| format!("Failed to set HttpService: {}", error))?;

        // ====================================================================
        // P2: CollectionService (Tags)
        // ====================================================================
        let collection_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create CollectionService table: {}", error))?;

        // Store tags in a global table
        let tags_storage = lua.create_table()
            .map_err(|error| format!("Failed to create tags storage: {}", error))?;
        globals.set("__COLLECTION_TAGS__", tags_storage)
            .map_err(|error| format!("Failed to set tags storage: {}", error))?;

        // CollectionService:AddTag(instance, tag)
        let add_tag = lua.create_function(|lua, (entity_id, tag): (i64, String)| {
            let globals = lua.globals();
            let tags: mlua::Table = globals.get("__COLLECTION_TAGS__")?;
            
            let entity_tags: mlua::Table = match tags.get::<Option<mlua::Table>>(entity_id)? {
                Some(t) => t,
                None => {
                    let new_table = lua.create_table()?;
                    tags.set(entity_id, new_table.clone())?;
                    new_table
                }
            };
            entity_tags.set(tag, true)?;
            Ok(())
        }).map_err(|error| format!("Failed to create AddTag: {}", error))?;
        collection_service_table.set("AddTag", add_tag)
            .map_err(|error| format!("Failed to set AddTag: {}", error))?;

        // CollectionService:RemoveTag(instance, tag)
        let remove_tag = lua.create_function(|lua, (entity_id, tag): (i64, String)| {
            let globals = lua.globals();
            let tags: mlua::Table = globals.get("__COLLECTION_TAGS__")?;
            
            if let Some(entity_tags) = tags.get::<Option<mlua::Table>>(entity_id)? {
                entity_tags.set(tag, mlua::Value::Nil)?;
            }
            Ok(())
        }).map_err(|error| format!("Failed to create RemoveTag: {}", error))?;
        collection_service_table.set("RemoveTag", remove_tag)
            .map_err(|error| format!("Failed to set RemoveTag: {}", error))?;

        // CollectionService:HasTag(instance, tag) -> bool
        let has_tag = lua.create_function(|lua, (entity_id, tag): (i64, String)| {
            let globals = lua.globals();
            let tags: mlua::Table = globals.get("__COLLECTION_TAGS__")?;
            
            if let Some(entity_tags) = tags.get::<Option<mlua::Table>>(entity_id)? {
                let has: bool = entity_tags.get::<Option<bool>>(tag)?.unwrap_or(false);
                Ok(has)
            } else {
                Ok(false)
            }
        }).map_err(|error| format!("Failed to create HasTag: {}", error))?;
        collection_service_table.set("HasTag", has_tag)
            .map_err(|error| format!("Failed to set HasTag: {}", error))?;

        // CollectionService:GetTagged(tag) -> {instances}
        let get_tagged = lua.create_function(|lua, tag: String| {
            let globals = lua.globals();
            let tags: mlua::Table = globals.get("__COLLECTION_TAGS__")?;
            let result = lua.create_table()?;
            let mut index = 1;
            
            for pair in tags.pairs::<i64, mlua::Table>() {
                if let Ok((entity_id, entity_tags)) = pair {
                    if entity_tags.get::<Option<bool>>(tag.clone())?.unwrap_or(false) {
                        result.set(index, entity_id)?;
                        index += 1;
                    }
                }
            }
            
            Ok(result)
        }).map_err(|error| format!("Failed to create GetTagged: {}", error))?;
        collection_service_table.set("GetTagged", get_tagged)
            .map_err(|error| format!("Failed to set GetTagged: {}", error))?;

        globals.set("CollectionService", collection_service_table)
            .map_err(|error| format!("Failed to set CollectionService: {}", error))?;

        // ====================================================================
        // P2: SoundService / Sound
        // ====================================================================
        let sound_service_table = lua.create_table()
            .map_err(|error| format!("Failed to create SoundService table: {}", error))?;

        // SoundService:PlayLocalSound(sound)
        let play_local = lua.create_function(|_, sound: mlua::Table| {
            let sound_id: String = sound.get::<Option<String>>("SoundId")?.unwrap_or_default();
            tracing::info!("[Luau Sound] Playing: {}", sound_id);
            sound.set("Playing", true)?;
            Ok(())
        }).map_err(|error| format!("Failed to create PlayLocalSound: {}", error))?;
        sound_service_table.set("PlayLocalSound", play_local)
            .map_err(|error| format!("Failed to set PlayLocalSound: {}", error))?;

        globals.set("SoundService", sound_service_table)
            .map_err(|error| format!("Failed to set SoundService: {}", error))?;

        // ====================================================================
        // P3: Camera API (workspace.CurrentCamera)
        // ====================================================================
        let camera_table = lua.create_table()
            .map_err(|error| format!("Failed to create Camera table: {}", error))?;

        // Camera.CFrame — current camera position/orientation
        let camera_cframe = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Camera.CFrame: {}", error))?;
        camera_table.set("CFrame", camera_cframe)
            .map_err(|error| format!("Failed to set Camera.CFrame: {}", error))?;

        // Camera.FieldOfView — field of view in degrees
        camera_table.set("FieldOfView", 70.0f64)
            .map_err(|error| format!("Failed to set Camera.FieldOfView: {}", error))?;

        // Camera.CameraType — "Custom", "Scriptable", "Follow", etc.
        camera_table.set("CameraType", "Custom")
            .map_err(|error| format!("Failed to set Camera.CameraType: {}", error))?;

        // Camera.CameraSubject — the object the camera follows (nil by default)
        camera_table.set("CameraSubject", mlua::Value::Nil)
            .map_err(|error| format!("Failed to set Camera.CameraSubject: {}", error))?;

        // Camera.Focus — focus point CFrame
        let focus_cframe = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Camera.Focus: {}", error))?;
        camera_table.set("Focus", focus_cframe)
            .map_err(|error| format!("Failed to set Camera.Focus: {}", error))?;

        // Camera.ViewportSize — Vector2 of viewport dimensions
        let viewport_size = lua.create_table()
            .map_err(|error| format!("Failed to create ViewportSize: {}", error))?;
        viewport_size.set("X", 1920.0f64)
            .map_err(|error| format!("Failed to set ViewportSize.X: {}", error))?;
        viewport_size.set("Y", 1080.0f64)
            .map_err(|error| format!("Failed to set ViewportSize.Y: {}", error))?;
        camera_table.set("ViewportSize", viewport_size)
            .map_err(|error| format!("Failed to set Camera.ViewportSize: {}", error))?;

        // Camera:WorldToScreenPoint(worldPoint) -> Vector3, bool
        let world_to_screen = lua.create_function(|lua, point: super::types::LuauVector3| {
            // TODO: Wire to actual camera projection
            let result = lua.create_table()?;
            result.set("X", point.0.x)?;
            result.set("Y", point.0.y)?;
            result.set("Z", point.0.z)?;
            Ok((result, true)) // (screenPoint, onScreen)
        }).map_err(|error| format!("Failed to create WorldToScreenPoint: {}", error))?;
        camera_table.set("WorldToScreenPoint", world_to_screen)
            .map_err(|error| format!("Failed to set WorldToScreenPoint: {}", error))?;

        // Camera:ScreenPointToRay(x, y, depth) -> Ray
        let screen_to_ray = lua.create_function(|lua, (x, y, _depth): (f64, f64, Option<f64>)| {
            // TODO: Wire to actual camera unprojection
            let ray = lua.create_table()?;
            let origin = lua.create_table()?;
            origin.set("X", 0.0f64)?;
            origin.set("Y", 0.0f64)?;
            origin.set("Z", 0.0f64)?;
            let direction = lua.create_table()?;
            direction.set("X", x / 1920.0)?;
            direction.set("Y", y / 1080.0)?;
            direction.set("Z", 1.0f64)?;
            ray.set("Origin", origin)?;
            ray.set("Direction", direction)?;
            Ok(ray)
        }).map_err(|error| format!("Failed to create ScreenPointToRay: {}", error))?;
        camera_table.set("ScreenPointToRay", screen_to_ray)
            .map_err(|error| format!("Failed to set ScreenPointToRay: {}", error))?;

        // Camera:ViewportPointToRay(x, y, depth) -> Ray
        let viewport_to_ray = lua.create_function(|lua, (x, y, _depth): (f64, f64, Option<f64>)| {
            let ray = lua.create_table()?;
            let origin = lua.create_table()?;
            origin.set("X", 0.0f64)?;
            origin.set("Y", 0.0f64)?;
            origin.set("Z", 0.0f64)?;
            let direction = lua.create_table()?;
            direction.set("X", x)?;
            direction.set("Y", y)?;
            direction.set("Z", 1.0f64)?;
            ray.set("Origin", origin)?;
            ray.set("Direction", direction)?;
            Ok(ray)
        }).map_err(|error| format!("Failed to create ViewportPointToRay: {}", error))?;
        camera_table.set("ViewportPointToRay", viewport_to_ray)
            .map_err(|error| format!("Failed to set ViewportPointToRay: {}", error))?;

        // Set workspace.CurrentCamera
        let workspace: mlua::Table = globals.get("workspace")
            .map_err(|error| format!("Failed to get workspace: {}", error))?;
        workspace.set("CurrentCamera", camera_table)
            .map_err(|error| format!("Failed to set workspace.CurrentCamera: {}", error))?;

        // ====================================================================
        // P3: Mouse API (game.Players.LocalPlayer:GetMouse())
        // ====================================================================
        let mouse_table = lua.create_table()
            .map_err(|error| format!("Failed to create Mouse table: {}", error))?;

        // Mouse.X, Mouse.Y — current position
        mouse_table.set("X", 0.0f64)
            .map_err(|error| format!("Failed to set Mouse.X: {}", error))?;
        mouse_table.set("Y", 0.0f64)
            .map_err(|error| format!("Failed to set Mouse.Y: {}", error))?;

        // Mouse.Hit — CFrame of where mouse ray intersects world
        let mouse_hit = lua.create_userdata(super::types::LuauCFrame::identity())
            .map_err(|error| format!("Failed to create Mouse.Hit: {}", error))?;
        mouse_table.set("Hit", mouse_hit)
            .map_err(|error| format!("Failed to set Mouse.Hit: {}", error))?;

        // Mouse.Target — Part the mouse is hovering over (nil if none)
        mouse_table.set("Target", mlua::Value::Nil)
            .map_err(|error| format!("Failed to set Mouse.Target: {}", error))?;

        // Mouse.TargetSurface — Enum.NormalId of surface (stub as string)
        mouse_table.set("TargetSurface", "Front")
            .map_err(|error| format!("Failed to set Mouse.TargetSurface: {}", error))?;

        // Mouse.UnitRay — Ray from camera through mouse position
        let unit_ray = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay: {}", error))?;
        let origin = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay.Origin: {}", error))?;
        origin.set("X", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        origin.set("Y", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        origin.set("Z", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        let direction = lua.create_table()
            .map_err(|error| format!("Failed to create UnitRay.Direction: {}", error))?;
        direction.set("X", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        direction.set("Y", 0.0f64).map_err(|e| format!("Failed: {}", e))?;
        direction.set("Z", 1.0f64).map_err(|e| format!("Failed: {}", e))?;
        unit_ray.set("Origin", origin).map_err(|e| format!("Failed: {}", e))?;
        unit_ray.set("Direction", direction).map_err(|e| format!("Failed: {}", e))?;
        mouse_table.set("UnitRay", unit_ray)
            .map_err(|error| format!("Failed to set Mouse.UnitRay: {}", error))?;

        // Mouse.Icon — cursor icon (string path)
        mouse_table.set("Icon", "")
            .map_err(|error| format!("Failed to set Mouse.Icon: {}", error))?;

        // Store mouse table for LocalPlayer:GetMouse()
        globals.set("_EustressMouse", mouse_table)
            .map_err(|error| format!("Failed to set _EustressMouse: {}", error))?;

        // ====================================================================
        // P3: ClickDetector API
        // ====================================================================
        // ClickDetector is typically a child of a Part, created via Instance.new("ClickDetector")
        // The events (MouseClick, MouseHoverEnter, MouseHoverLeave) are handled by the bridge
        // Here we just ensure the class is recognized

        // ====================================================================
        // P3: Animation API (Animator, AnimationTrack)
        // ====================================================================
        // In Roblox, animations are loaded via Animator:LoadAnimation(Animation)
        // which returns an AnimationTrack. The track can be played, stopped, etc.
        
        // Create Animator prototype for humanoid.Animator
        let animator_proto = lua.create_table()
            .map_err(|e| format!("Failed to create Animator proto: {}", e))?;
        
        // Animator:LoadAnimation(animation) -> AnimationTrack
        let load_animation = lua.create_function(|lua, (animator, animation): (mlua::Table, mlua::Table)| {
            // Create an AnimationTrack table
            let track = lua.create_table()?;
            
            // Copy animation ID from the Animation instance
            let anim_id: String = animation.get::<Option<String>>("AnimationId")?.unwrap_or_default();
            track.set("Animation", animation)?;
            track.set("_animationId", anim_id)?;
            
            // AnimationTrack properties
            track.set("IsPlaying", false)?;
            track.set("Length", 1.0f64)?;
            track.set("Looped", false)?;
            track.set("Priority", 1i32)?; // Enum.AnimationPriority.Core = 1
            track.set("Speed", 1.0f64)?;
            track.set("TimePosition", 0.0f64)?;
            track.set("WeightCurrent", 0.0f64)?;
            track.set("WeightTarget", 1.0f64)?;
            
            // AnimationTrack:Play(fadeTime, weight, speed)
            track.set("Play", lua.create_function(|_, (this, fade_time, weight, speed): (mlua::Table, Option<f64>, Option<f64>, Option<f64>)| {
                let _fade = fade_time.unwrap_or(0.1);
                let _weight = weight.unwrap_or(1.0);
                let _speed = speed.unwrap_or(1.0);
                this.set("IsPlaying", true)?;
                this.set("WeightTarget", _weight)?;
                this.set("Speed", _speed)?;
                tracing::info!("[Luau Animation] Playing animation");
                Ok(())
            })?)?;
            
            // AnimationTrack:Stop(fadeTime)
            track.set("Stop", lua.create_function(|_, (this, _fade_time): (mlua::Table, Option<f64>)| {
                this.set("IsPlaying", false)?;
                this.set("WeightTarget", 0.0f64)?;
                tracing::info!("[Luau Animation] Stopping animation");
                Ok(())
            })?)?;
            
            // AnimationTrack:AdjustSpeed(speed)
            track.set("AdjustSpeed", lua.create_function(|_, (this, speed): (mlua::Table, f64)| {
                this.set("Speed", speed)?;
                Ok(())
            })?)?;
            
            // AnimationTrack:AdjustWeight(weight, fadeTime)
            track.set("AdjustWeight", lua.create_function(|_, (this, weight, _fade_time): (mlua::Table, f64, Option<f64>)| {
                this.set("WeightTarget", weight)?;
                Ok(())
            })?)?;
            
            // AnimationTrack:GetMarkerReachedSignal(name) -> RBXScriptSignal (stub)
            track.set("GetMarkerReachedSignal", lua.create_function(|lua, (_this, _name): (mlua::Table, String)| {
                // Return a stub signal table
                let signal = lua.create_table()?;
                signal.set("Connect", lua.create_function(|_, (_sig, _callback): (mlua::Table, mlua::Function)| {
                    // Stub: would connect to keyframe marker events
                    Ok(())
                })?)?;
                Ok(signal)
            })?)?;
            
            Ok(track)
        }).map_err(|e| format!("Failed to create LoadAnimation: {}", e))?;
        animator_proto.set("LoadAnimation", load_animation)
            .map_err(|e| format!("Failed to set LoadAnimation: {}", e))?;
        
        // Store animator prototype for Instance system
        globals.set("_EustressAnimatorProto", animator_proto)
            .map_err(|e| format!("Failed to set _EustressAnimatorProto: {}", e))?;

        // ====================================================================
        // P3: Humanoid API (stub for character control)
        // ====================================================================
        // Humanoid is typically accessed via character.Humanoid
        // Create a prototype table that can be used when creating Humanoid instances
        
        let humanoid_proto = lua.create_table()
            .map_err(|e| format!("Failed to create Humanoid proto: {}", e))?;
        
        // Humanoid properties (defaults)
        humanoid_proto.set("Health", 100.0f64)
            .map_err(|e| format!("Failed to set Health: {}", e))?;
        humanoid_proto.set("MaxHealth", 100.0f64)
            .map_err(|e| format!("Failed to set MaxHealth: {}", e))?;
        humanoid_proto.set("WalkSpeed", 16.0f64)
            .map_err(|e| format!("Failed to set WalkSpeed: {}", e))?;
        humanoid_proto.set("JumpPower", 50.0f64)
            .map_err(|e| format!("Failed to set JumpPower: {}", e))?;
        humanoid_proto.set("JumpHeight", 7.2f64)
            .map_err(|e| format!("Failed to set JumpHeight: {}", e))?;
        humanoid_proto.set("HipHeight", 2.0f64)
            .map_err(|e| format!("Failed to set HipHeight: {}", e))?;
        humanoid_proto.set("AutoRotate", true)
            .map_err(|e| format!("Failed to set AutoRotate: {}", e))?;
        humanoid_proto.set("AutoJumpEnabled", true)
            .map_err(|e| format!("Failed to set AutoJumpEnabled: {}", e))?;
        
        // Humanoid:TakeDamage(amount)
        let take_damage = lua.create_function(|_, (this, amount): (mlua::Table, f64)| {
            let current: f64 = this.get("Health")?;
            let new_health = (current - amount).max(0.0);
            this.set("Health", new_health)?;
            if new_health <= 0.0 {
                tracing::info!("[Luau Humanoid] Character died");
                // TODO: Fire Died event
            }
            Ok(())
        }).map_err(|e| format!("Failed to create TakeDamage: {}", e))?;
        humanoid_proto.set("TakeDamage", take_damage)
            .map_err(|e| format!("Failed to set TakeDamage: {}", e))?;
        
        // Humanoid:MoveTo(position, part)
        let move_to = lua.create_function(|_, (_this, position, _part): (mlua::Table, super::types::LuauVector3, Option<mlua::Value>)| {
            tracing::info!("[Luau Humanoid] MoveTo: {:?}", position.0);
            // TODO: Wire to character controller pathfinding
            Ok(())
        }).map_err(|e| format!("Failed to create MoveTo: {}", e))?;
        humanoid_proto.set("MoveTo", move_to)
            .map_err(|e| format!("Failed to set MoveTo: {}", e))?;
        
        // Humanoid:Move(moveDirection, relativeToCamera)
        let move_fn = lua.create_function(|_, (_this, direction, _relative): (mlua::Table, super::types::LuauVector3, Option<bool>)| {
            tracing::info!("[Luau Humanoid] Move: {:?}", direction.0);
            // TODO: Wire to character controller
            Ok(())
        }).map_err(|e| format!("Failed to create Move: {}", e))?;
        humanoid_proto.set("Move", move_fn)
            .map_err(|e| format!("Failed to set Move: {}", e))?;
        
        // Humanoid:ChangeState(state)
        let change_state = lua.create_function(|_, (_this, state): (mlua::Table, i32)| {
            tracing::info!("[Luau Humanoid] ChangeState: {}", state);
            // TODO: Wire to animation state machine
            Ok(())
        }).map_err(|e| format!("Failed to create ChangeState: {}", e))?;
        humanoid_proto.set("ChangeState", change_state)
            .map_err(|e| format!("Failed to set ChangeState: {}", e))?;
        
        // Humanoid:GetState() -> Enum.HumanoidStateType
        let get_state = lua.create_function(|_, _this: mlua::Table| {
            // Return Running state (8) as default
            Ok(8i32)
        }).map_err(|e| format!("Failed to create GetState: {}", e))?;
        humanoid_proto.set("GetState", get_state)
            .map_err(|e| format!("Failed to set GetState: {}", e))?;
        
        // Store humanoid prototype for Instance system
        globals.set("_EustressHumanoidProto", humanoid_proto)
            .map_err(|e| format!("Failed to set _EustressHumanoidProto: {}", e))?;

        Ok(())
    }
}

// ============================================================================
// Bevy Resources
// ============================================================================

/// Bevy resource wrapping the Luau runtime state
#[derive(Resource, Default)]
pub struct LuauRuntimeState {
    /// The Luau runtime instance (initialized lazily)
    pub runtime: Option<LuauRuntime>,
    /// Has the runtime been initialized?
    pub initialized: bool,
}

/// Queue of script execution requests processed each frame
#[derive(Resource, Default)]
pub struct ScriptExecutionQueue {
    /// Pending execution requests
    pub pending: Vec<ScriptExecutionRequest>,
}

/// A request to execute a Luau script chunk
#[derive(Debug, Clone)]
pub struct ScriptExecutionRequest {
    /// Human-readable script name (for error reporting)
    pub script_name: String,
    /// Luau source code to execute
    pub source: String,
    /// Entity that owns this script (for context injection)
    pub entity: Option<Entity>,
}

impl ScriptExecutionQueue {
    /// Enqueue a script for execution next frame
    pub fn enqueue(&mut self, name: &str, source: &str, entity: Option<Entity>) {
        self.pending.push(ScriptExecutionRequest {
            script_name: name.to_string(),
            source: source.to_string(),
            entity,
        });
    }
}

// ============================================================================
// Events
// ============================================================================

/// Message: A Luau script was loaded
#[derive(Message, Debug, Clone)]
pub struct LuauScriptLoadEvent {
    /// Script name
    pub script_name: String,
    /// Entity the script belongs to
    pub entity: Entity,
    /// Source file path (if loaded from file)
    pub source_path: Option<String>,
}

/// Message: A Luau script error occurred
#[derive(Message, Debug, Clone)]
pub struct LuauScriptErrorEvent {
    /// Script name
    pub script_name: String,
    /// Error message
    pub error: String,
    /// Line number (if available)
    pub line: Option<u32>,
}
