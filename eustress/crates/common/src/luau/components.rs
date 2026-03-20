//! # Luau ECS Components
//!
//! Bevy components for Luau script types and networking primitives.
//!
//! ## Table of Contents
//!
//! 1. **LuauScript** — Server-side script (equivalent to Roblox `Script`)
//! 2. **LuauLocalScript** — Client-side script (equivalent to Roblox `LocalScript`)
//! 3. **LuauModuleScript** — Reusable module (equivalent to Roblox `ModuleScript`)
//! 4. **RemoteEvent** — Client↔Server one-way event channel
//! 5. **RemoteFunction** — Client↔Server request-response channel
//! 6. **BindableEvent** — In-process one-way event (same context)
//! 7. **BindableFunction** — In-process request-response (same context)

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Script Components
// ============================================================================

/// Server-side Luau script — runs on the server context.
/// Equivalent to Roblox `Script` with `RunContext = Server`.
///
/// Scripts placed under `ServerScriptService` run server-only.
/// Scripts under `Workspace` or `ReplicatedStorage` may replicate.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct LuauScript {
    /// Display name in Explorer
    pub name: String,

    /// Luau source code (inline or loaded from .luau file)
    pub source: String,

    /// Path to external .luau source file (empty if inline)
    pub source_path: String,

    /// Is the script currently enabled?
    pub enabled: bool,

    /// Run context override (Server, Client, or Legacy — derived from parent)
    pub run_context: LuauRunContext,

    /// Has the script been loaded into the VM?
    #[serde(skip)]
    pub loaded: bool,

    /// Last execution error (if any)
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for LuauScript {
    fn default() -> Self {
        Self {
            name: "Script".to_string(),
            source: String::new(),
            source_path: String::new(),
            enabled: true,
            run_context: LuauRunContext::Legacy,
            loaded: false,
            error: None,
        }
    }
}

impl LuauScript {
    /// Create a new server script with name and source
    pub fn new(name: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            ..Default::default()
        }
    }

    /// Create from a .luau file path
    pub fn from_file(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            source_path: path.to_string(),
            ..Default::default()
        }
    }
}

/// Client-side Luau script — runs exclusively on the client.
/// Equivalent to Roblox `LocalScript`.
///
/// Always runs in client context regardless of parent service.
/// Cannot access server-only APIs (DataStoreService, etc.).
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct LuauLocalScript {
    /// Display name in Explorer
    pub name: String,

    /// Luau source code
    pub source: String,

    /// Path to external .luau source file
    pub source_path: String,

    /// Is the script currently enabled?
    pub enabled: bool,

    /// Has the script been loaded into the VM?
    #[serde(skip)]
    pub loaded: bool,

    /// Last execution error (if any)
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for LuauLocalScript {
    fn default() -> Self {
        Self {
            name: "LocalScript".to_string(),
            source: String::new(),
            source_path: String::new(),
            enabled: true,
            loaded: false,
            error: None,
        }
    }
}

impl LuauLocalScript {
    /// Create a new local script with name and source
    pub fn new(name: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            ..Default::default()
        }
    }
}

/// Reusable Luau module — returns a table when `require()`d.
/// Equivalent to Roblox `ModuleScript`.
///
/// Shared between server and client contexts.
/// Must return exactly one value from its top-level chunk.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct LuauModuleScript {
    /// Display name in Explorer
    pub name: String,

    /// Luau source code
    pub source: String,

    /// Path to external .luau source file
    pub source_path: String,

    /// Has the module been loaded and cached?
    #[serde(skip)]
    pub loaded: bool,

    /// Last execution error (if any)
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for LuauModuleScript {
    fn default() -> Self {
        Self {
            name: "ModuleScript".to_string(),
            source: String::new(),
            source_path: String::new(),
            loaded: false,
            error: None,
        }
    }
}

impl LuauModuleScript {
    /// Create a new module script with name and source
    pub fn new(name: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Run Context
// ============================================================================

/// Determines where a LuauScript executes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum LuauRunContext {
    /// Server-side only
    Server,
    /// Client-side only
    Client,
    /// Legacy behavior — derived from parent service hierarchy
    #[default]
    Legacy,
}

impl LuauRunContext {
    /// String representation for display
    pub fn as_str(&self) -> &'static str {
        match self {
            LuauRunContext::Server => "Server",
            LuauRunContext::Client => "Client",
            LuauRunContext::Legacy => "Legacy",
        }
    }
}

// ============================================================================
// Networking Primitives — Remote (Client↔Server)
// ============================================================================

/// One-way event channel between client and server.
/// Equivalent to Roblox `RemoteEvent`.
///
/// - Server fires → all connected clients receive
/// - Client fires → server receives (with player identity)
///
/// Used for: damage notifications, chat messages, UI triggers, etc.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct RemoteEvent {
    /// Event name (used for lookup by scripts via `game.ReplicatedStorage.EventName`)
    pub name: String,

    /// Is this event currently active?
    pub enabled: bool,

    /// Fire count (diagnostic)
    #[serde(skip)]
    pub fire_count: u64,
}

impl Default for RemoteEvent {
    fn default() -> Self {
        Self {
            name: "RemoteEvent".to_string(),
            enabled: true,
            fire_count: 0,
        }
    }
}

impl RemoteEvent {
    /// Create a named remote event
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
}

/// Request-response channel between client and server.
/// Equivalent to Roblox `RemoteFunction`.
///
/// - Client invokes → server processes → returns result to client
/// - Server invokes → client processes → returns result to server
///
/// Used for: data fetching, validation, server-authoritative checks.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct RemoteFunction {
    /// Function name
    pub name: String,

    /// Is this function currently active?
    pub enabled: bool,

    /// Invocation count (diagnostic)
    #[serde(skip)]
    pub invoke_count: u64,
}

impl Default for RemoteFunction {
    fn default() -> Self {
        Self {
            name: "RemoteFunction".to_string(),
            enabled: true,
            invoke_count: 0,
        }
    }
}

impl RemoteFunction {
    /// Create a named remote function
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Networking Primitives — Bindable (In-Process)
// ============================================================================

/// In-process one-way event within the same execution context.
/// Equivalent to Roblox `BindableEvent`.
///
/// Used for decoupled communication between scripts on the same side
/// (server↔server or client↔client). Does NOT cross the network boundary.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct BindableEvent {
    /// Event name
    pub name: String,

    /// Is this event currently active?
    pub enabled: bool,

    /// Fire count (diagnostic)
    #[serde(skip)]
    pub fire_count: u64,
}

impl Default for BindableEvent {
    fn default() -> Self {
        Self {
            name: "BindableEvent".to_string(),
            enabled: true,
            fire_count: 0,
        }
    }
}

impl BindableEvent {
    /// Create a named bindable event
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
}

/// In-process request-response within the same execution context.
/// Equivalent to Roblox `BindableFunction`.
///
/// Synchronous call between scripts on the same side.
/// Does NOT cross the network boundary.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct BindableFunction {
    /// Function name
    pub name: String,

    /// Is this function currently active?
    pub enabled: bool,

    /// Invocation count (diagnostic)
    #[serde(skip)]
    pub invoke_count: u64,
}

impl Default for BindableFunction {
    fn default() -> Self {
        Self {
            name: "BindableFunction".to_string(),
            enabled: true,
            invoke_count: 0,
        }
    }
}

impl BindableFunction {
    /// Create a named bindable function
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }
}
