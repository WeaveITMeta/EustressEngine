//! # Physics Script Commands — Shared Bridge
//!
//! Thread-local command queue for physics operations from Rune/Luau scripts.
//! Used by both engine and client physics bridge systems.

use std::cell::RefCell;
use std::collections::HashMap;

/// Command to apply physics forces/velocities from scripts
#[derive(Debug, Clone)]
pub enum PhysicsCommand {
    ApplyImpulse { entity_name: String, x: f64, y: f64, z: f64 },
    ApplyAngularImpulse { entity_name: String, x: f64, y: f64, z: f64 },
    SetVelocity { entity_name: String, x: f64, y: f64, z: f64 },
}

/// Snapshot of an entity's physics state for script access
#[derive(Debug, Clone, Default)]
pub struct PhysicsSnapshot {
    pub mass: f64,
    pub velocity: [f64; 3],
    pub angular_velocity: [f64; 3],
}

thread_local! {
    /// Pending physics commands from scripts
    pub static PHYSICS_COMMANDS: RefCell<Vec<PhysicsCommand>> = RefCell::new(Vec::new());
    /// Physics state snapshot (populated before script execution)
    pub static PHYSICS_STATE: RefCell<HashMap<String, PhysicsSnapshot>> = RefCell::new(HashMap::new());
    /// Workspace gravity (m/s²)
    pub static WORKSPACE_GRAVITY: RefCell<f64> = RefCell::new(9.80665);
}

pub fn push_physics_command(cmd: PhysicsCommand) {
    PHYSICS_COMMANDS.with(|cmds| cmds.borrow_mut().push(cmd));
}

pub fn drain_physics_commands() -> Vec<PhysicsCommand> {
    PHYSICS_COMMANDS.with(|cmds| std::mem::take(&mut *cmds.borrow_mut()))
}

pub fn set_physics_state(states: HashMap<String, PhysicsSnapshot>) {
    PHYSICS_STATE.with(|ps| *ps.borrow_mut() = states);
}

pub fn get_physics_snapshot(name: &str) -> Option<PhysicsSnapshot> {
    PHYSICS_STATE.with(|ps| ps.borrow().get(name).cloned())
}

pub fn set_workspace_gravity(gravity: f64) {
    WORKSPACE_GRAVITY.with(|g| *g.borrow_mut() = gravity);
}

pub fn get_workspace_gravity() -> f64 {
    WORKSPACE_GRAVITY.with(|g| *g.borrow())
}
