//! # AI Service
//! 
//! NPC behavior, pathfinding, and AI management.
//! 
//! ## Classes
//! - `AIService`: Global AI settings
//! - `NPC`: Non-player character
//! - `Behavior`: AI behavior tree/state
//! - `Pathfinding`: Navigation data

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// AIService Resource
// ============================================================================

/// AIService - global AI settings and management
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct AIService {
    /// Is AI enabled globally
    pub enabled: bool,
    /// Max NPCs to update per frame
    pub max_updates_per_frame: u32,
    /// Default pathfinding agent radius
    pub default_agent_radius: f32,
    /// Default pathfinding agent height
    pub default_agent_height: f32,
    /// Navigation mesh entity
    pub navmesh: Option<Entity>,
}

impl Default for AIService {
    fn default() -> Self {
        Self {
            enabled: true,
            max_updates_per_frame: 50,
            default_agent_radius: 0.5,
            default_agent_height: 2.0,
            navmesh: None,
        }
    }
}

// ============================================================================
// NPC Component
// ============================================================================

/// NPC - non-player character (like Eustress's Humanoid for NPCs)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct NPC {
    /// Display name
    pub display_name: String,
    /// Current health
    pub health: f32,
    /// Max health
    pub max_health: f32,
    /// Walk speed
    pub walk_speed: f32,
    /// Is hostile to players
    pub hostile: bool,
    /// Detection range
    pub detection_range: f32,
    /// Attack range
    pub attack_range: f32,
    /// Attack damage
    pub attack_damage: f32,
    /// Attack cooldown (seconds)
    pub attack_cooldown: f32,
    /// Current target entity
    #[serde(skip)]
    pub target: Option<Entity>,
    /// Current AI state
    pub state: NPCState,
    /// Respawn time (0 = no respawn)
    pub respawn_time: f32,
    /// Spawn position
    pub spawn_position: Vec3,
}

impl Default for NPC {
    fn default() -> Self {
        Self {
            display_name: "NPC".to_string(),
            health: 100.0,
            max_health: 100.0,
            walk_speed: 8.0,
            hostile: false,
            detection_range: 30.0,
            attack_range: 3.0,
            attack_damage: 10.0,
            attack_cooldown: 1.0,
            target: None,
            state: NPCState::Idle,
            respawn_time: 30.0,
            spawn_position: Vec3::ZERO,
        }
    }
}

/// NPC AI states
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum NPCState {
    #[default]
    Idle,
    Patrol,
    Chase,
    Attack,
    Flee,
    Dead,
    Talking,
    Custom(u32),
}

// ============================================================================
// Behavior Component
// ============================================================================

/// Behavior - AI behavior configuration
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Behavior {
    /// Behavior type
    pub behavior_type: BehaviorType,
    /// Patrol waypoints (if patrol)
    #[serde(default)]
    pub waypoints: Vec<Vec3>,
    /// Current waypoint index
    #[serde(skip)]
    pub current_waypoint: usize,
    /// Wait time at each waypoint
    pub waypoint_wait: f32,
    /// Wander radius (if wander)
    pub wander_radius: f32,
    /// Home position
    pub home_position: Vec3,
    /// Max distance from home before returning
    pub leash_distance: f32,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            behavior_type: BehaviorType::Idle,
            waypoints: Vec::new(),
            current_waypoint: 0,
            waypoint_wait: 2.0,
            wander_radius: 10.0,
            home_position: Vec3::ZERO,
            leash_distance: 50.0,
        }
    }
}

/// Behavior types
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum BehaviorType {
    #[default]
    Idle,
    Patrol,
    Wander,
    Guard,
    Follow,
    Flee,
    Scripted,
}

// ============================================================================
// Pathfinding
// ============================================================================

/// PathfindingAgent - navigation agent component
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct PathfindingAgent {
    /// Agent radius
    pub radius: f32,
    /// Agent height
    pub height: f32,
    /// Max slope angle (degrees)
    pub max_slope: f32,
    /// Step height
    pub step_height: f32,
    /// Current path
    pub path: Vec<Vec3>,
    /// Current path index
    pub path_index: usize,
    /// Is path valid
    pub has_path: bool,
}

/// Event to request pathfinding
#[derive(Message, Clone, Debug)]
pub struct PathfindRequest {
    pub entity: Entity,
    pub target: Vec3,
}

/// Event when path is computed
#[derive(Message, Clone, Debug)]
pub struct PathfindResult {
    pub entity: Entity,
    pub path: Vec<Vec3>,
    pub success: bool,
}

// ============================================================================
// Dialogue
// ============================================================================

/// Dialogue component for NPCs
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Dialogue {
    /// Dialogue lines
    pub lines: Vec<DialogueLine>,
    /// Current line index
    #[serde(skip)]
    pub current_line: usize,
    /// Is dialogue active
    #[serde(skip)]
    pub active: bool,
}

impl Default for Dialogue {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            current_line: 0,
            active: false,
        }
    }
}

/// A single dialogue line
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct DialogueLine {
    /// Speaker name
    pub speaker: String,
    /// Text content
    pub text: String,
    /// Duration to display (0 = wait for input)
    pub duration: f32,
    /// Choices (if any)
    pub choices: Vec<DialogueChoice>,
}

/// Dialogue choice
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct DialogueChoice {
    /// Choice text
    pub text: String,
    /// Jump to line index
    pub jump_to: usize,
}
