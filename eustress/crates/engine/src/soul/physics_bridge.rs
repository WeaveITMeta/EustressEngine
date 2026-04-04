//! Physics bridge — drains Rune physics commands and applies them to Avian3d.

use bevy::prelude::*;
use avian3d::prelude::*;
use super::rune_ecs_module::{self, PhysicsCommand};

/// Bevy system: drain queued Rune physics commands and apply to Avian3d entities.
/// Impulse and angular impulse use the Forces query.
/// SetVelocity runs in a separate system to avoid query conflicts.
pub fn apply_rune_force_commands(
    mut forces_query: Query<(Forces, &Name)>,
) {
    let commands = rune_ecs_module::drain_physics_commands();
    if commands.is_empty() { return; }

    // Separate velocity commands to process in the other system
    let mut velocity_commands = Vec::new();

    for cmd in commands {
        match cmd {
            PhysicsCommand::ApplyImpulse { entity_name, x, y, z } => {
                for (mut forces, name) in &mut forces_query {
                    if name.as_str() == entity_name {
                        forces.apply_linear_impulse(Vec3::new(x as f32, y as f32, z as f32));
                        break;
                    }
                }
            }
            PhysicsCommand::ApplyAngularImpulse { entity_name, x, y, z } => {
                for (mut forces, name) in &mut forces_query {
                    if name.as_str() == entity_name {
                        forces.apply_angular_impulse(Vec3::new(x as f32, y as f32, z as f32));
                        break;
                    }
                }
            }
            PhysicsCommand::SetVelocity { .. } => {
                velocity_commands.push(cmd);
            }
        }
    }

    // Store velocity commands for the next system
    PENDING_VELOCITY_COMMANDS.with(|cmds| {
        *cmds.borrow_mut() = velocity_commands;
    });
}

thread_local! {
    static PENDING_VELOCITY_COMMANDS: std::cell::RefCell<Vec<PhysicsCommand>> =
        std::cell::RefCell::new(Vec::new());
}

/// Separate system for SetVelocity to avoid query conflicts with Forces.
pub fn apply_rune_velocity_commands(
    mut velocity_query: Query<(&mut LinearVelocity, &Name), Without<Camera3d>>,
) {
    let commands = PENDING_VELOCITY_COMMANDS.with(|cmds| {
        std::mem::take(&mut *cmds.borrow_mut())
    });
    if commands.is_empty() { return; }

    for cmd in commands {
        if let PhysicsCommand::SetVelocity { entity_name, x, y, z } = cmd {
            for (mut lin_vel, name) in &mut velocity_query {
                if name.as_str() == entity_name {
                    lin_vel.0 = Vec3::new(x as f32, y as f32, z as f32);
                    break;
                }
            }
        }
    }
}

/// Bevy system: sync workspace gravity from Rune thread-local to Avian3d Gravity resource.
pub fn sync_rune_gravity(
    mut gravity: ResMut<Gravity>,
) {
    let rune_gravity = rune_ecs_module::WORKSPACE_GRAVITY.with(|g| *g.borrow());
    let current = gravity.0.y.abs() as f64;
    if (current - rune_gravity).abs() > 0.001 {
        gravity.0 = Vec3::new(0.0, -(rune_gravity as f32), 0.0);
    }
}

/// Bevy system: snapshot Avian3d physics state into Rune thread-locals.
pub fn snapshot_physics_state(
    query: Query<(&Name, Option<&LinearVelocity>, Option<&AngularVelocity>, Option<&Mass>)>,
) {
    let mut states = std::collections::HashMap::new();
    for (name, lin_vel, ang_vel, mass) in &query {
        let snapshot = rune_ecs_module::PhysicsSnapshot {
            mass: mass.map(|m| m.0 as f64).unwrap_or(1.0),
            velocity: lin_vel.map(|v| [v.0.x as f64, v.0.y as f64, v.0.z as f64]).unwrap_or([0.0; 3]),
            angular_velocity: ang_vel.map(|v| [v.0.x as f64, v.0.y as f64, v.0.z as f64]).unwrap_or([0.0; 3]),
        };
        states.insert(name.as_str().to_string(), snapshot);
    }
    rune_ecs_module::set_physics_state(states);
}

/// Plugin to register the physics bridge systems.
pub struct RunePhysicsBridgePlugin;

impl Plugin for RunePhysicsBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            snapshot_physics_state,
            apply_rune_force_commands.after(snapshot_physics_state),
            apply_rune_velocity_commands.after(apply_rune_force_commands),
            sync_rune_gravity,
        ));
    }
}
