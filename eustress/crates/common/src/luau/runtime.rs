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
    #[cfg(feature = "luau")]
    fn inject_eustress_globals(lua: &mlua::Lua) -> Result<(), String> {
        let globals = lua.globals();

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
