//! # Soul Types
//!
//! Core type definitions for Soul scripting system.

use serde::{Deserialize, Serialize};

// ============================================================================
// Script Type Classification
// ============================================================================

/// Script type classification
/// Distinguishes Meta Engine Code from Plausible Edits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScriptType {
    /// Meta Engine Code - core, immutable systems
    /// Examples: Physics overrides, server-authoritative validation
    /// Strict ECS patterns, no creative liberties
    Meta,
    
    /// Plausible Edits - flexible, scene-specific behaviors
    /// Examples: "Player bounces whimsically on clouds"
    /// Creative, entity-guarded, can be hot-swapped
    Plausible,
    
    /// Mixed - contains both meta and plausible sections
    #[default]
    Mixed,
}

impl ScriptType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScriptType::Meta => "meta",
            ScriptType::Plausible => "plausible",
            ScriptType::Mixed => "mixed",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "meta" | "engine" | "core" => Some(ScriptType::Meta),
            "plausible" | "creative" | "edit" => Some(ScriptType::Plausible),
            "mixed" | "both" => Some(ScriptType::Mixed),
            _ => None,
        }
    }
    
    /// Emoji marker for this type
    pub fn emoji(&self) -> &'static str {
        match self {
            ScriptType::Meta => "🔵",
            ScriptType::Plausible => "🟢",
            ScriptType::Mixed => "🟡",
        }
    }
}

// ============================================================================
// Build Status
// ============================================================================

/// Build pipeline status
/// Simple status updates without percentages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BuildStatus {
    /// Not yet started
    #[default]
    Pending,
    
    /// Parsing .md file
    Parsing,
    
    /// Generating code with Claude Opus 4.5
    Generating {
        /// Current step description
        step: String,
    },
    
    /// Validating generated code
    Validating,
    
    /// Running Miri for UB detection
    MiriCheck,
    
    /// Compilation successful
    Complete,
    
    /// Compilation failed
    Failed {
        /// Error message
        error: String,
    },
    
    /// Using fallback (Claude 4.5 Thinking mode)
    Fallback {
        /// Reason for fallback
        reason: String,
        /// Current thinking step
        step: u32,
    },
}

impl BuildStatus {
    pub fn as_str(&self) -> &str {
        match self {
            BuildStatus::Pending => "Pending",
            BuildStatus::Parsing => "Parsing",
            BuildStatus::Generating { step } => step,
            BuildStatus::Validating => "Validating",
            BuildStatus::MiriCheck => "Miri Check",
            BuildStatus::Complete => "Complete",
            BuildStatus::Failed { .. } => "Failed",
            BuildStatus::Fallback { .. } => "Fallback",
        }
    }
    
    pub fn is_complete(&self) -> bool {
        matches!(self, BuildStatus::Complete)
    }
    
    pub fn is_failed(&self) -> bool {
        matches!(self, BuildStatus::Failed { .. })
    }
    
    pub fn is_in_progress(&self) -> bool {
        matches!(self, 
            BuildStatus::Parsing | 
            BuildStatus::Generating { .. } | 
            BuildStatus::Validating |
            BuildStatus::MiriCheck |
            BuildStatus::Fallback { .. }
        )
    }
}

// ============================================================================
// Script Frontmatter
// ============================================================================

/// YAML frontmatter from .md script files
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScriptFrontmatter {
    /// Scene name
    pub scene: String,
    
    /// Target service
    pub service: Option<String>,
    
    /// Script type (meta/plausible/mixed)
    #[serde(rename = "type")]
    pub script_type: Option<String>,
    
    /// Default unit for distances
    pub unit: Option<String>,
    
    /// Script version
    pub version: Option<String>,
    
    /// Author
    pub author: Option<String>,
    
    /// Description
    pub description: Option<String>,
    
    /// Dependencies (other scripts)
    pub dependencies: Option<Vec<String>>,
    
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
}

// ============================================================================
// Event Handler
// ============================================================================

/// Event handler definition from script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventHandler {
    /// Event name (e.g., "On Player Land")
    pub name: String,
    
    /// When condition (trigger)
    pub when: Option<String>,
    
    /// If condition (guard)
    pub if_condition: Option<String>,
    
    /// Else branch
    pub else_branch: Option<Vec<String>>,
    
    /// Then actions
    pub then_actions: Vec<String>,
    
    /// Script type for this handler
    pub handler_type: ScriptType,
    
    /// Meta check (server-authoritative validation)
    pub meta_check: Option<String>,
    
    /// Plausible edit description
    pub plausible_edit: Option<String>,
}

// ============================================================================
// Function Definition
// ============================================================================

/// Function definition from script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    /// Function name
    pub name: String,
    
    /// Parameters
    pub params: Vec<String>,
    
    /// Return type (optional)
    pub returns: Option<String>,
    
    /// Function body (prose)
    pub body: Vec<String>,
    
    /// Target service instance
    pub instance: Option<String>,
}

// ============================================================================
// Global Variable
// ============================================================================

/// Global variable definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalVar {
    /// Variable name
    pub name: String,
    
    /// Value (as string, will be parsed)
    pub value: String,
    
    /// Optional type hint
    pub type_hint: Option<String>,
    
    /// Description
    pub description: Option<String>,
}

// ============================================================================
// Query Definition
// ============================================================================

/// Query definition for entity queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryDef {
    /// Query description
    pub description: String,
    
    /// Entity type to query
    pub entity_type: Option<String>,
    
    /// Filter conditions
    pub filters: Vec<String>,
    
    /// Radius for spatial queries
    pub radius: Option<String>,
    
    /// Service scope
    pub service: Option<String>,
}

// ============================================================================
// Action Definition
// ============================================================================

/// Action to perform
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDef {
    /// Action type
    pub action_type: ActionType,
    
    /// Target entity/component
    pub target: Option<String>,
    
    /// Parameters
    pub params: std::collections::HashMap<String, String>,
    
    /// Raw prose description
    pub description: String,
}

/// Types of actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    /// Spawn an entity/prefab
    Spawn,
    /// Destroy an entity
    Destroy,
    /// Modify a component
    Modify,
    /// Play sound
    PlaySound,
    /// Spawn particles
    SpawnParticles,
    /// Apply force/velocity
    ApplyForce,
    /// Teleport entity
    Teleport,
    /// Fire remote event
    FireEvent,
    /// Tween property
    Tween,
    /// Log/warn/error
    Log,
    /// Custom action
    Custom,
}

impl ActionType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "spawn" | "create" | "instantiate" => Some(ActionType::Spawn),
            "destroy" | "delete" | "remove" => Some(ActionType::Destroy),
            "modify" | "set" | "change" | "update" => Some(ActionType::Modify),
            "sound" | "playsound" | "audio" => Some(ActionType::PlaySound),
            "particles" | "vfx" | "effect" => Some(ActionType::SpawnParticles),
            "force" | "velocity" | "impulse" | "push" => Some(ActionType::ApplyForce),
            "teleport" | "move" | "warp" => Some(ActionType::Teleport),
            "event" | "fire" | "signal" | "remote" => Some(ActionType::FireEvent),
            "tween" | "animate" | "lerp" => Some(ActionType::Tween),
            "log" | "print" | "warn" | "error" => Some(ActionType::Log),
            _ => Some(ActionType::Custom),
        }
    }
}
