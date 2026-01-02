//! # Physics Service
//! 
//! Physics configuration and collision groups.
//! 
//! ## Classes
//! - `PhysicsService`: Global physics settings
//! - `CollisionGroup`: Collision filtering
//! - `PhysicsBody`: Physics body configuration

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// PhysicsService Resource
// ============================================================================

/// PhysicsService - global physics settings
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct PhysicsService {
    /// Gravity vector
    pub gravity: Vec3,
    /// Physics simulation enabled
    pub enabled: bool,
    /// Time scale (1.0 = normal)
    pub time_scale: f32,
    /// Allow sleeping (optimization)
    pub allow_sleep: bool,
    /// Solver iterations
    pub solver_iterations: u32,
    /// Solver sub-steps
    pub solver_substeps: u32,
}

impl Default for PhysicsService {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -196.2, 0.0), // Eustress-style gravity
            enabled: true,
            time_scale: 1.0,
            allow_sleep: true,
            solver_iterations: 4,
            solver_substeps: 1,
        }
    }
}

// ============================================================================
// Collision Groups
// ============================================================================

/// Collision group for filtering
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CollisionGroup {
    /// Group name
    pub name: String,
    /// Group ID (bitmask)
    pub group: u32,
    /// Mask of groups this can collide with
    pub mask: u32,
}

impl Default for CollisionGroup {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            group: 1,
            mask: u32::MAX, // Collide with everything
        }
    }
}

/// Predefined collision groups
pub mod collision_groups {
    pub const DEFAULT: u32 = 1 << 0;
    pub const PLAYER: u32 = 1 << 1;
    pub const NPC: u32 = 1 << 2;
    pub const PROJECTILE: u32 = 1 << 3;
    pub const TRIGGER: u32 = 1 << 4;
    pub const TERRAIN: u32 = 1 << 5;
    pub const VEHICLE: u32 = 1 << 6;
    pub const DEBRIS: u32 = 1 << 7;
}

// ============================================================================
// Physics Body Types
// ============================================================================

/// Physics body type marker
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[reflect(Component)]
pub enum PhysicsBodyType {
    /// Static body (doesn't move)
    #[default]
    Static,
    /// Dynamic body (affected by forces)
    Dynamic,
    /// Kinematic body (moved by code, affects others)
    Kinematic,
}

/// Physics material properties
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PhysicsMaterial {
    /// Friction coefficient (0-1)
    pub friction: f32,
    /// Restitution/bounciness (0-1)
    pub restitution: f32,
    /// Density (affects mass)
    pub density: f32,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            friction: 0.3,
            restitution: 0.0,
            density: 1.0,
        }
    }
}

// ============================================================================
// Constraints
// ============================================================================

/// Constraint types for physics joints
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Constraint {
    /// Constraint type
    pub constraint_type: ConstraintType,
    /// First attached entity
    pub attachment0: Option<Entity>,
    /// Second attached entity
    pub attachment1: Option<Entity>,
    /// Is constraint enabled
    pub enabled: bool,
    /// Is constraint visible in editor
    pub visible: bool,
}

impl Default for Constraint {
    fn default() -> Self {
        Self {
            constraint_type: ConstraintType::Weld,
            attachment0: None,
            attachment1: None,
            enabled: true,
            visible: true,
        }
    }
}

/// Types of physics constraints
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum ConstraintType {
    #[default]
    Weld,
    Hinge,
    Rope,
    Spring,
    Rod,
    Prismatic,
    BallSocket,
    Motor,
}

// ============================================================================
// Forces
// ============================================================================

/// Body velocity component (for custom physics)
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BodyVelocity {
    /// Target velocity
    pub velocity: Vec3,
    /// Max force to apply
    pub max_force: Vec3,
    /// Power (how quickly to reach target)
    pub power: f32,
}

/// Body force component
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BodyForce {
    /// Force vector
    pub force: Vec3,
    /// Relative to part or world
    pub relative_to: ForceRelativeTo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum ForceRelativeTo {
    #[default]
    World,
    Part,
}
