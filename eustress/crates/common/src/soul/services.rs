//! # Script Services
//!
//! Roblox-inspired service containers for Soul scripts.
//! 
//! ## Service Resolution
//! 
//! The service is determined by the **parent hierarchy** in Explorer, NOT by .md frontmatter.
//! - Script under `Workspace` → `ScriptService::Workspace`
//! - Script under `ServerScriptService` → `ScriptService::ServerScriptService`
//! - Script under `ReplicatedStorage` → `ScriptService::ReplicatedStorage`
//! 
//! This mirrors Roblox Studio behavior where script location determines context.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// Script Service Enum
// ============================================================================

/// Target service for a Soul script
/// Mirrors Roblox's service hierarchy for familiarity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
pub enum ScriptService {
    /// World simulation - spatial queries, collisions, spawns
    /// Access: Server + Client (replicated)
    /// Example: Trampoline bounces, door interactions
    Workspace,
    
    /// Server-only logic - hidden from clients
    /// Access: Server only
    /// Example: Leaderboards, anti-cheat, game state
    ServerScriptService,
    
    /// Shared assets and modules
    /// Access: Server + Client
    /// Example: RemoteFunctions, shared utilities
    ReplicatedStorage,
    
    /// Early client loads - runs before other replication
    /// Access: Client first
    /// Example: Loading screens, UI initialization
    ReplicatedFirst,
    
    /// Environment effects
    /// Access: Server + Client
    /// Example: Day/night cycle, fog, atmosphere
    Lighting,
    
    /// Player authentication and management
    /// Access: Server + Client
    /// Example: Join/leave handling, player data
    Players,
    
    /// Audio management
    /// Access: Server + Client
    /// Example: Background music, ambient sounds
    SoundService,
    
    /// Chat and communication
    /// Access: Server + Client
    /// Example: Chat commands, filters
    Chat,
    
    /// Teams and groups
    /// Access: Server + Client
    /// Example: Team assignment, colors
    Teams,
    
    /// GUI elements
    /// Access: Client only
    /// Example: HUD, menus, overlays
    StarterGui,
    
    /// Player character setup
    /// Access: Server + Client
    /// Example: Character customization, tools
    StarterPlayer,
}

impl ScriptService {
    /// Get service name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptService::Workspace => "Workspace",
            ScriptService::ServerScriptService => "ServerScriptService",
            ScriptService::ReplicatedStorage => "ReplicatedStorage",
            ScriptService::ReplicatedFirst => "ReplicatedFirst",
            ScriptService::Lighting => "Lighting",
            ScriptService::Players => "Players",
            ScriptService::SoundService => "SoundService",
            ScriptService::Chat => "Chat",
            ScriptService::Teams => "Teams",
            ScriptService::StarterGui => "StarterGui",
            ScriptService::StarterPlayer => "StarterPlayer",
        }
    }
    
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "workspace" => Some(ScriptService::Workspace),
            "serverscriptservice" | "server_script_service" => Some(ScriptService::ServerScriptService),
            "replicatedstorage" | "replicated_storage" => Some(ScriptService::ReplicatedStorage),
            "replicatedfirst" | "replicated_first" => Some(ScriptService::ReplicatedFirst),
            "lighting" => Some(ScriptService::Lighting),
            "players" => Some(ScriptService::Players),
            "soundservice" | "sound_service" => Some(ScriptService::SoundService),
            "chat" => Some(ScriptService::Chat),
            "teams" => Some(ScriptService::Teams),
            "startergui" | "starter_gui" => Some(ScriptService::StarterGui),
            "starterplayer" | "starter_player" => Some(ScriptService::StarterPlayer),
            _ => None,
        }
    }
    
    /// Is this service server-only?
    pub fn is_server_only(&self) -> bool {
        matches!(self, ScriptService::ServerScriptService)
    }
    
    /// Is this service client-only?
    pub fn is_client_only(&self) -> bool {
        matches!(self, ScriptService::StarterGui | ScriptService::ReplicatedFirst)
    }
    
    /// Is this service replicated to clients?
    pub fn is_replicated(&self) -> bool {
        matches!(self, 
            ScriptService::Workspace |
            ScriptService::ReplicatedStorage |
            ScriptService::Lighting |
            ScriptService::Players |
            ScriptService::SoundService |
            ScriptService::Chat |
            ScriptService::Teams |
            ScriptService::StarterPlayer
        )
    }
    
    /// Get the Bevy system set for this service
    pub fn system_set_name(&self) -> &'static str {
        match self {
            ScriptService::Workspace => "SoulWorkspace",
            ScriptService::ServerScriptService => "SoulServerScript",
            ScriptService::ReplicatedStorage => "SoulReplicated",
            ScriptService::ReplicatedFirst => "SoulReplicatedFirst",
            ScriptService::Lighting => "SoulLighting",
            ScriptService::Players => "SoulPlayers",
            ScriptService::SoundService => "SoulSound",
            ScriptService::Chat => "SoulChat",
            ScriptService::Teams => "SoulTeams",
            ScriptService::StarterGui => "SoulGui",
            ScriptService::StarterPlayer => "SoulStarterPlayer",
        }
    }
}

impl Default for ScriptService {
    fn default() -> Self {
        ScriptService::Workspace
    }
}

// ============================================================================
// Service Marker Components
// ============================================================================

/// Marker: Entity belongs to Workspace service
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InWorkspace;

/// Marker: Entity belongs to ServerScriptService
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InServerScriptService;

/// Marker: Entity belongs to ReplicatedStorage
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InReplicatedStorage;

/// Marker: Entity belongs to ReplicatedFirst
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InReplicatedFirst;

/// Marker: Entity belongs to Lighting service
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InLighting;

/// Marker: Entity belongs to Players service
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InPlayers;

/// Marker: Entity belongs to SoundService
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct InSoundService;

// ============================================================================
// Service Resolution
// ============================================================================

/// Resolve service from parent entity name
/// This is called when a Script is placed under a service container
pub fn resolve_service_from_parent(parent_name: &str) -> ScriptService {
    match parent_name.to_lowercase().as_str() {
        "workspace" => ScriptService::Workspace,
        "serverscriptservice" | "server script service" => ScriptService::ServerScriptService,
        "replicatedstorage" | "replicated storage" => ScriptService::ReplicatedStorage,
        "replicatedfirst" | "replicated first" => ScriptService::ReplicatedFirst,
        "lighting" => ScriptService::Lighting,
        "players" => ScriptService::Players,
        "soundservice" | "sound service" => ScriptService::SoundService,
        "chat" => ScriptService::Chat,
        "teams" => ScriptService::Teams,
        "startergui" | "starter gui" => ScriptService::StarterGui,
        "starterplayer" | "starter player" => ScriptService::StarterPlayer,
        _ => ScriptService::Workspace, // Default to Workspace
    }
}

/// Resolve RunContext from script type and service
/// - LocalScript → always Client
/// - Script in ServerScriptService → Server
/// - Script in ReplicatedStorage → Both
/// - Script in StarterGui → Client
pub fn resolve_run_context(service: ScriptService, is_local_script: bool) -> RunContext {
    if is_local_script {
        return RunContext::Client;
    }
    
    match service {
        ScriptService::ServerScriptService => RunContext::Server,
        ScriptService::StarterGui | ScriptService::ReplicatedFirst => RunContext::Client,
        ScriptService::ReplicatedStorage => RunContext::Both,
        _ => RunContext::Server, // Default to Server for safety
    }
}

// ============================================================================
// Script Component
// ============================================================================

/// Script instance component - attached to entities that have Soul scripts
/// 
/// ## Service Resolution
/// The `service` field is automatically set based on the Script's parent in Explorer.
/// Do NOT set this manually - it's derived from hierarchy.
/// 
/// ## Double-Click Behavior
/// Double-clicking a Script in Explorer opens its `.md` source file for editing.
/// Building the project compiles all .md files to Rust.
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct Script {
    /// Script name (display name in Explorer)
    pub name: String,
    
    /// Source .md file path (relative to scripts/ directory)
    pub source: String,
    
    /// Target service (derived from parent hierarchy, do not set manually)
    #[serde(skip)]
    pub service: ScriptService,
    
    /// Is the script enabled?
    pub enabled: bool,
    
    /// Run context (Server, Client, or Both) - derived from service
    #[serde(skip)]
    pub run_context: RunContext,
    
    /// Last compilation status
    #[serde(skip)]
    pub compiled: bool,
    
    /// Compilation error (if any)
    #[serde(skip)]
    pub error: Option<String>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            name: "Script".to_string(),
            source: String::new(),
            service: ScriptService::Workspace,
            enabled: true,
            run_context: RunContext::Server,
            compiled: false,
            error: None,
        }
    }
}

impl Script {
    /// Create a new script with name and source
    pub fn new(name: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            source: source.to_string(),
            ..Default::default()
        }
    }
    
    /// Update service based on parent name
    pub fn set_parent_service(&mut self, parent_name: &str) {
        self.service = resolve_service_from_parent(parent_name);
        self.run_context = resolve_run_context(self.service, false);
    }
    
    /// Get the full path to the .md source file
    pub fn source_path(&self, scripts_dir: &std::path::Path) -> std::path::PathBuf {
        if self.source.is_empty() {
            scripts_dir.join(format!("{}.md", self.name.to_lowercase().replace(' ', "_")))
        } else {
            scripts_dir.join(&self.source)
        }
    }
    
    /// Mark as compiled successfully
    pub fn mark_compiled(&mut self) {
        self.compiled = true;
        self.error = None;
    }
    
    /// Mark as compilation failed
    pub fn mark_error(&mut self, error: &str) {
        self.compiled = false;
        self.error = Some(error.to_string());
    }
}

/// Script run context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum RunContext {
    /// Server-side only
    #[default]
    Server,
    /// Client-side only
    Client,
    /// Both server and client
    Both,
}

impl RunContext {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunContext::Server => "Server",
            RunContext::Client => "Client",
            RunContext::Both => "Both",
        }
    }
}

// ============================================================================
// LocalScript Component
// ============================================================================

/// LocalScript - client-only script (like Roblox LocalScript)
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct LocalScript {
    /// Script name
    pub name: String,
    
    /// Source .md file path
    pub source: String,
    
    /// Is the script enabled?
    pub enabled: bool,
}

impl Default for LocalScript {
    fn default() -> Self {
        Self {
            name: "LocalScript".to_string(),
            source: String::new(),
            enabled: true,
        }
    }
}

// ============================================================================
// ModuleScript Component
// ============================================================================

/// ModuleScript - reusable module (like Roblox ModuleScript)
#[derive(Component, Debug, Clone, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct ModuleScript {
    /// Module name
    pub name: String,
    
    /// Source .md file path
    pub source: String,
    
    /// Exported functions/values
    pub exports: Vec<String>,
}

impl Default for ModuleScript {
    fn default() -> Self {
        Self {
            name: "ModuleScript".to_string(),
            source: String::new(),
            exports: Vec::new(),
        }
    }
}
