//! Physics bridge — drains Rune physics commands and applies them to Avian3d.
//!
//! Everything a Rune script reads or queues here crosses through
//! [`RunePhysicsBridge`], never through a thread-local shared between two
//! systems: `.after()` orders systems but does not pin them to a thread, so a
//! thread-local written by one system is usually invisible to the next.
//! [`crate::soul::rune_play::drive_rune_frame`] installs the snapshot and the
//! live gravity on the VM's own thread and hands back what scripts queued.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use avian3d::prelude::*;
use super::rune_ecs_module::{PhysicsCommand, PhysicsSnapshot};

/// Physics state crossing between Bevy systems and the Rune VM thread.
#[derive(Resource)]
pub struct RunePhysicsBridge {
    /// What `part_get_mass` / `part_get_velocity` read, keyed by entity name.
    /// Empty unless a Rune script names one of them.
    pub state: Arc<HashMap<String, PhysicsSnapshot>>,
    /// Impulses and velocity sets scripts queued, waiting to be applied.
    pub commands: Vec<PhysicsCommand>,
    /// `SetVelocity` commands handed from the force system to the velocity one.
    pub velocity_commands: Vec<PhysicsCommand>,
    /// Live gravity magnitude (m/s²) that `workspace_get_gravity` reads.
    pub gravity: f64,
    /// A gravity a script set this frame, for `Workspace.gravity`.
    pub gravity_write: Option<f64>,
}

impl Default for RunePhysicsBridge {
    fn default() -> Self {
        Self {
            state: Arc::default(),
            commands: Vec::new(),
            velocity_commands: Vec::new(),
            gravity: 9.80665,
            gravity_write: None,
        }
    }
}

/// Bevy system: apply queued Rune physics commands to Avian3d entities.
/// Impulse and angular impulse use the Forces query.
/// SetVelocity runs in a separate system to avoid query conflicts.
pub fn apply_rune_force_commands(
    mut bridge: ResMut<RunePhysicsBridge>,
    mut forces_query: Query<(Forces, &Name)>,
) {
    if bridge.commands.is_empty() { return; }
    let commands = std::mem::take(&mut bridge.commands);

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
    bridge.velocity_commands.extend(velocity_commands);
}

/// Separate system for SetVelocity to avoid query conflicts with Forces.
pub fn apply_rune_velocity_commands(
    mut bridge: ResMut<RunePhysicsBridge>,
    mut velocity_query: Query<(&mut LinearVelocity, &Name), Without<Camera3d>>,
) {
    if bridge.velocity_commands.is_empty() { return; }
    let commands = std::mem::take(&mut bridge.velocity_commands);

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

/// Bevy system: push a Rune-authored gravity change into `Workspace.gravity`,
/// the single source of truth. The canonical
/// `eustress_common::services::workspace::sync_workspace_gravity_to_avian` then
/// converts it into Avian's `Gravity`, so this is not a second writer of the
/// physics resource.
///
/// Only pushes when a script actually CHANGED the value: `drive_rune_frame`
/// seeds the VM with the live gravity and reports a write only when a script
/// left a different value behind, so a scene- or tool-authored gravity is
/// never clobbered by a stale Rune default.
pub fn sync_rune_gravity(
    workspace: Option<ResMut<eustress_common::services::workspace::Workspace>>,
    mut bridge: ResMut<RunePhysicsBridge>,
) {
    let Some(mut ws) = workspace else { return };
    if let Some(gravity) = bridge.gravity_write.take() {
        ws.gravity = Vec3::new(0.0, -(gravity as f32), 0.0);
    }
    let live = ws.gravity.length() as f64;
    if bridge.gravity != live {
        bridge.gravity = live;
    }
}

/// Bevy system: build the physics snapshot Rune's `part_get_mass` /
/// `part_get_velocity` read. `drive_rune_frame` installs it on the VM thread.
///
/// Built only while PLAYING and only while a Rune script names one of those
/// functions: a function no source names cannot be called, and a per-entity
/// map with a String key per entity cost milliseconds a frame in scenes whose
/// scripts never read it (~253 ms/frame on Vehicle Simulator's 301K entities
/// before the Play gate). Entities with no velocity or mass are left out;
/// the getters answer them with the same defaults the map would have held.
pub fn snapshot_physics_state(
    play: Res<State<crate::play_mode::PlayModeState>>,
    scripts: Query<&crate::soul::SoulScriptData>,
    edited: Query<(), Changed<crate::soul::SoulScriptData>>,
    mut reads_physics: Local<Option<bool>>,
    mut bridge: ResMut<RunePhysicsBridge>,
    query: Query<
        (&Name, Option<&LinearVelocity>, Option<&AngularVelocity>, Option<&Mass>),
        Or<(With<LinearVelocity>, With<AngularVelocity>, With<Mass>)>,
    >,
) {
    if *play.get() != crate::play_mode::PlayModeState::Playing {
        return;
    }
    if reads_physics.is_none() || !edited.is_empty() {
        *reads_physics = Some(scripts.iter().any(|s| {
            s.run_context == crate::soul::SoulRunContext::Rune
                && (s.source.contains("part_get_mass") || s.source.contains("part_get_velocity"))
        }));
    }
    if *reads_physics != Some(true) {
        if !bridge.state.is_empty() {
            bridge.state = Arc::default();
        }
        return;
    }
    let mut states = HashMap::with_capacity(bridge.state.len());
    for (name, lin_vel, ang_vel, mass) in &query {
        let snapshot = PhysicsSnapshot {
            mass: mass.map(|m| m.0 as f64).unwrap_or(1.0),
            velocity: lin_vel.map(|v| [v.0.x as f64, v.0.y as f64, v.0.z as f64]).unwrap_or([0.0; 3]),
            angular_velocity: ang_vel.map(|v| [v.0.x as f64, v.0.y as f64, v.0.z as f64]).unwrap_or([0.0; 3]),
        };
        states.insert(name.as_str().to_string(), snapshot);
    }
    bridge.state = Arc::new(states);
}

/// Plugin to register the physics bridge systems.
pub struct RunePhysicsBridgePlugin;

impl Plugin for RunePhysicsBridgePlugin {
    fn build(&self, app: &mut App) {
        // Snapshot before the Rune frame reads it; apply what it queued in
        // the same frame.
        let rune_frame = crate::soul::rune_play::drive_rune_frame;
        app.init_resource::<RunePhysicsBridge>()
            .add_systems(Update, (
                snapshot_physics_state.before(rune_frame),
                apply_rune_force_commands.after(snapshot_physics_state).after(rune_frame),
                apply_rune_velocity_commands.after(apply_rune_force_commands),
                sync_rune_gravity.after(rune_frame),
            ));
    }
}
