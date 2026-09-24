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

/// PhysicsService - global physics settings and simulation fidelity.
///
/// The authored surface for which kinds of physics the engine steps. Every
/// field here is read by something: the tuning fields drive Avian, and each
/// [`PhysicsDomain`](crate::realism::PhysicsDomain) flag gates that domain's
/// systems through a run condition, so switching one off stops the work rather
/// than just recording a preference.
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct PhysicsService {
    /// Gravity vector, in metres per second squared.
    ///
    /// Eustress is metre-native (studs are a display unit only), so this is
    /// -9.80665 on Y. It previously defaulted to -196.2, the stud-scale value,
    /// which was 20x the acceleration the engine actually applied.
    pub gravity: Vec3,
    /// Master switch. False stops every domain below, whatever they are set to.
    pub enabled: bool,
    /// Time scale (1.0 = normal)
    pub time_scale: f32,
    /// Allow sleeping (optimization)
    pub allow_sleep: bool,
    /// Solver iterations
    pub solver_iterations: u32,
    /// Solver sub-steps
    pub solver_substeps: u32,

    // ── Fidelity ────────────────────────────────────────────────────────
    //
    // One flag per domain that actually runs systems each step. Domains whose
    // plugins only register types and expose pure functions (structures,
    // plasma, propulsion, control, thermocycles, numerics) deliberately have
    // NO flag: a switch that gates nothing is worse than no switch, because it
    // reads as a capability the engine does not have.
    /// Rigid bodies, collision detection and the constraint solver (Avian).
    pub kinematics: bool,
    /// Heat conduction and transfer between touching bodies.
    pub thermodynamics: bool,
    /// Reactions, combustion and reactor temperature.
    pub chemistry: bool,
    /// Circuits, motors and power electronics.
    pub electricity: bool,
    /// Impact denting and fracture into separate bodies.
    pub deformation: bool,
    /// Smoothed-particle fluid dynamics.
    pub fluids: bool,
    /// Stress and strain response of materials.
    pub materials: bool,
    /// Thermal and kinetic stepping of particle species.
    pub particles: bool,
    /// The divergence-free SPH particle solver.
    pub particle_simulation: bool,
    /// Decay chains and reactor kinetics.
    pub nuclear: bool,
    /// Property overlays that draw simulation state into the viewport.
    pub visualizers: bool,
    /// Spread domain work across threads with Rayon where supported.
    pub parallel: bool,
}

impl Default for PhysicsService {
    fn default() -> Self {
        Self {
            // Metres per second squared. See the field docs for why this is
            // not the stud-scale 196.2 it used to be.
            gravity: Vec3::new(0.0, -9.80665, 0.0),
            enabled: true,
            time_scale: 1.0,
            allow_sleep: true,
            solver_iterations: 4,
            // Matches the engine's pinned `SubstepCount(6)`. This used to say
            // 1; harmless while nothing read it, but the moment it syncs to
            // Avian a default of 1 would silently change every trajectory.
            solver_substeps: 6,

            // Everything on by default: the engine behaves exactly as it did
            // before these flags existed until someone turns one off.
            kinematics: true,
            thermodynamics: true,
            chemistry: true,
            electricity: true,
            deformation: true,
            fluids: true,
            materials: true,
            particles: true,
            particle_simulation: true,
            nuclear: true,
            visualizers: true,
            parallel: true,
        }
    }
}

impl PhysicsService {
    /// Whether a domain should step this frame.
    ///
    /// Folds in the master switch so callers never have to remember to check
    /// both, which is how a "disabled" simulation ends up half running.
    pub fn domain_enabled(&self, domain: crate::realism::PhysicsDomain) -> bool {
        self.enabled && self.domain_flag(domain)
    }

    /// A domain's own flag, IGNORING the master switch. For reporting and
    /// persistence; use [`domain_enabled`](Self::domain_enabled) to decide
    /// whether anything should run.
    pub fn domain_flag(&self, domain: crate::realism::PhysicsDomain) -> bool {
        use crate::realism::PhysicsDomain as D;
        match domain {
            D::Kinematics => self.kinematics,
            D::Thermodynamics => self.thermodynamics,
            D::Chemistry => self.chemistry,
            D::Electricity => self.electricity,
            D::Deformation => self.deformation,
            D::Fluids => self.fluids,
            D::Materials => self.materials,
            D::Particles => self.particles,
            D::ParticleSimulation => self.particle_simulation,
            D::Nuclear => self.nuclear,
            D::Visualizers => self.visualizers,
        }
    }

    /// Set a domain's own flag. Paired with [`domain_flag`](Self::domain_flag)
    /// so every caller that walks `PhysicsDomain::ALL` reaches every field
    /// through one exhaustive match, rather than keeping its own field list
    /// that a new domain could be forgotten from.
    pub fn set_domain_flag(&mut self, domain: crate::realism::PhysicsDomain, on: bool) {
        use crate::realism::PhysicsDomain as D;
        let field = match domain {
            D::Kinematics => &mut self.kinematics,
            D::Thermodynamics => &mut self.thermodynamics,
            D::Chemistry => &mut self.chemistry,
            D::Electricity => &mut self.electricity,
            D::Deformation => &mut self.deformation,
            D::Fluids => &mut self.fluids,
            D::Materials => &mut self.materials,
            D::Particles => &mut self.particles,
            D::ParticleSimulation => &mut self.particle_simulation,
            D::Nuclear => &mut self.nuclear,
            D::Visualizers => &mut self.visualizers,
        };
        *field = on;
    }
}

/// The value type and allowed range of one PhysicsService setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhysicsSettingKind {
    Bool,
    /// A real number, inclusive range.
    Float { min: f64, max: f64 },
    /// A whole number, inclusive range.
    Int { min: i64, max: i64 },
    /// Three real numbers `[x, y, z]`.
    Vec3,
}

/// Every PhysicsService setting that is NOT a per-domain flag: its property
/// key, type and range, and what it does. The domain flags come from
/// [`crate::realism::PhysicsDomain::ALL`]. Together the two are the complete
/// editable surface, read by the engine to validate edits and by the MCP
/// server to describe its tools, so neither keeps a copy that can drift.
///
/// `solver_iterations` and `allow_sleep` are deliberately absent: nothing
/// applies them to Avian yet, and offering a knob that changes nothing is the
/// defect this surface was built to get rid of.
pub const PHYSICS_GENERAL_SETTINGS: &[(&str, PhysicsSettingKind, &str)] = &[
    (
        "enabled",
        PhysicsSettingKind::Bool,
        "Master switch. Off stops every physics domain, whatever the individual flags say.",
    ),
    (
        "gravity",
        PhysicsSettingKind::Vec3,
        "Gravity in metres per second squared, as [x, y, z]. Earth is [0, -9.80665, 0], the Moon [0, -1.62, 0], Mars [0, -3.71, 0].",
    ),
    (
        "time_scale",
        PhysicsSettingKind::Float { min: 0.0, max: 100.0 },
        "Speed of the rigid-body physics clock. 1 is real time, 0.5 half speed, 2 double. 0 stops the clock without pausing Play.",
    ),
    (
        "solver_substeps",
        PhysicsSettingKind::Int { min: 1, max: 64 },
        "Solver substeps per fixed step. More is stabler for tall stacks and chains of joints, and costs proportionally more. 6 is the pinned default that keeps runs reproducible.",
    ),
    (
        "parallel",
        PhysicsSettingKind::Bool,
        "Spread particle work across CPU threads where supported.",
    ),
];

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
