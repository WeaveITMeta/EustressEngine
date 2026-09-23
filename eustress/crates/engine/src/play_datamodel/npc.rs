//! Humanoids that are not the local player: `Humanoid:Move`, `:MoveTo`,
//! `WalkSpeed`, `AutoRotate`, `MoveToFinished`.
//!
//! The model's `HumanoidRootPart` (or `PrimaryPart`) must be an unanchored
//! part; it becomes the physics body. Its rotation is locked so the
//! character stays upright (Roblox humanoids never tip over), and each frame
//! its horizontal velocity is set from the move request while gravity keeps
//! the vertical.

use std::collections::HashMap;

use bevy::prelude::*;

use avian3d::prelude::{LinearVelocity, LockedAxes};

use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::datamodel::{DmEvent, DmValue, HumanoidCommand, InstanceId};
use eustress_common::scripting::Vector3;

use super::PlayDataModel;

/// How long `MoveTo` walks before giving up, like Roblox.
const MOVE_TO_TIMEOUT_S: f64 = 8.0;
/// Horizontal distance that counts as arrived.
const ARRIVE_DISTANCE: f32 = 0.6;

#[derive(Debug, Clone, Default)]
struct NpcState {
    direction: Vec3,
    target: Option<Vec3>,
    target_started: f64,
    jump: bool,
    locked: bool,
}

#[derive(Resource, Default)]
pub struct NpcControllers {
    states: HashMap<InstanceId, NpcState>,
}

impl NpcControllers {
    pub fn clear(&mut self) {
        self.states.clear();
    }
}

pub fn drive_npc_humanoids(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    mut ctl: ResMut<NpcControllers>,
    mut bodies: Query<(&mut LinearVelocity, &mut Transform), Without<AvatarBody>>,
) {
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    let now = g.frame.time;
    for cmd in std::mem::take(&mut g.humanoid_commands) {
        match cmd {
            HumanoidCommand::Move { humanoid, direction } => {
                let s = ctl.states.entry(humanoid).or_default();
                let d = Vec3::new(direction.x as f32, 0.0, direction.z as f32);
                s.direction = if d.length_squared() > 1e-8 { d.normalize() * d.length().min(1.0) } else { Vec3::ZERO };
                s.target = None;
            }
            HumanoidCommand::MoveTo { humanoid, target } => {
                let s = ctl.states.entry(humanoid).or_default();
                s.target = Some(target.to_vec3());
                s.target_started = now;
            }
            HumanoidCommand::Jump { humanoid } => {
                ctl.states.entry(humanoid).or_default().jump = true;
            }
        }
    }
    if ctl.states.is_empty() {
        return;
    }

    let mut finished: Vec<(InstanceId, bool)> = Vec::new();
    let mut dead: Vec<InstanceId> = Vec::new();
    for (humanoid, state) in ctl.states.iter_mut() {
        if !g.exists(*humanoid) {
            dead.push(*humanoid);
            continue;
        }
        let Some(model) = g.parent(*humanoid) else { continue };
        let root = match g.get_prop(model, "PrimaryPart") {
            Some(DmValue::Instance(r)) if g.exists(r) => Some(r),
            _ => g.find_first_child(model, "HumanoidRootPart", false),
        };
        let Some(root) = root else { continue };
        let Some(entity) = g.entity_of(root).map(Entity::from_bits) else { continue };
        let health = g.get_prop(*humanoid, "Health").and_then(|v| v.as_number()).unwrap_or(100.0);
        let speed = if health > 0.0 {
            g.get_prop(*humanoid, "WalkSpeed").and_then(|v| v.as_number()).unwrap_or(4.5) as f32
        } else {
            0.0
        };
        let auto_rotate = g.get_prop(*humanoid, "AutoRotate").and_then(|v| v.as_bool()).unwrap_or(true);
        let Ok((mut vel, mut tf)) = bodies.get_mut(entity) else { continue };

        if !state.locked {
            commands.entity(entity).insert(LockedAxes::ROTATION_LOCKED);
            state.locked = true;
        }

        let mut dir = state.direction;
        if let Some(target) = state.target {
            let mut delta = target - tf.translation;
            delta.y = 0.0;
            if delta.length() <= ARRIVE_DISTANCE {
                state.target = None;
                dir = Vec3::ZERO;
                finished.push((*humanoid, true));
            } else if now - state.target_started > MOVE_TO_TIMEOUT_S {
                state.target = None;
                dir = Vec3::ZERO;
                finished.push((*humanoid, false));
            } else {
                dir = delta.normalize();
            }
        }

        vel.0.x = dir.x * speed;
        vel.0.z = dir.z * speed;
        if state.jump {
            let height = g.get_prop(*humanoid, "JumpHeight").and_then(|v| v.as_number()).unwrap_or(2.0) as f32;
            vel.0.y = (2.0 * 9.81 * height.max(0.0)).sqrt();
            state.jump = false;
        }
        if auto_rotate && dir.length_squared() > 1e-6 {
            let yaw = (-dir.x).atan2(-dir.z);
            let current = tf.rotation.to_euler(EulerRot::YXZ).0;
            let diff = (yaw - current + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            let alpha = (12.0 * g.frame.dt as f32).min(1.0);
            tf.rotation = Quat::from_rotation_y(current + diff * alpha);
        }
        g.set_prop_from_engine(*humanoid, "MoveDirection", DmValue::Vector3(Vector3::new(dir.x as f64, 0.0, dir.z as f64)));
    }
    for h in dead {
        ctl.states.remove(&h);
    }
    for (h, reached) in finished {
        g.push_event(DmEvent::MoveToFinished { humanoid: h, reached });
    }
}
