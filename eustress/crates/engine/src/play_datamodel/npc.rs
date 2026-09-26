//! Humanoids that are not the local player: `Humanoid:Move`, `:MoveTo`,
//! `WalkSpeed`, `AutoRotate`, `MoveToFinished`.
//!
//! The model's `HumanoidRootPart` (or `PrimaryPart`) must be an unanchored
//! part; it becomes the physics body. Its rotation is locked so the
//! character stays upright (Roblox humanoids never tip over), and each frame
//! its horizontal velocity is set from the move request while gravity keeps
//! the vertical.
//!
//! `Jump` follows Roblox's rule, which is the player's too: under
//! `UseJumpPower` it takes off at `JumpPower`, otherwise at the speed that
//! peaks at `JumpHeight` under the live gravity. An NPC and the player with
//! equal numbers jump alike under any gravity the Space has.

use std::collections::HashMap;

use bevy::prelude::*;

use avian3d::prelude::{Gravity, LinearVelocity, LockedAxes};

use eustress_common::avatar::locomotion::downward_gravity;
use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::avatar::ResolvedMotion;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, HumanoidCommand, InstanceId};
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
    gravity: Option<Res<Gravity>>,
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
            vel.0.y = jump_speed(&g, *humanoid, downward_gravity(gravity.as_deref()));
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

/// Take-off speed, m/s, of `humanoid`'s jump under a downward gravity of
/// `gravity` m/s²: `JumpPower` under `UseJumpPower`, otherwise the speed that
/// peaks at `JumpHeight`, by the player's rule
/// ([`ResolvedMotion::take_off_speed`]). A missing number is the Humanoid
/// class default.
fn jump_speed(g: &DataModel, humanoid: InstanceId, gravity: f32) -> f32 {
    let number = |name: &str, default: f64| {
        g.get_prop(humanoid, name).and_then(|v| v.as_number()).unwrap_or(default) as f32
    };
    let use_jump_power = g.get_prop(humanoid, "UseJumpPower").and_then(|v| v.as_bool()).unwrap_or(false);
    let launch = use_jump_power.then(|| number("JumpPower", 14.0));
    ResolvedMotion::take_off_speed(launch, number("JumpHeight", 2.0), gravity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::units::STANDARD_GRAVITY_F32;

    fn humanoid(g: &mut DataModel) -> InstanceId {
        let ws = g.get_service("Workspace").unwrap();
        let model = g.create_virtual("Model", "Npc", Some(ws));
        g.create_virtual("Humanoid", "Humanoid", Some(model))
    }

    /// Under any gravity, Roblox's 54.9 m/s² included, a jump peaks at its
    /// `JumpHeight`; weightless, a height needs no take-off.
    #[test]
    fn a_jump_peaks_at_its_height_under_any_gravity() {
        let mut g = DataModel::new();
        let h = humanoid(&mut g);
        g.set_prop(h, "JumpHeight", DmValue::Number(2.0)).unwrap();
        for gravity in [1.62_f32, STANDARD_GRAVITY_F32, 54.936] {
            let v = jump_speed(&g, h, gravity);
            let apex = v * v / (2.0 * gravity);
            assert!((apex - 2.0).abs() < 1e-4, "apex {apex} under {gravity}");
        }
        assert_eq!(jump_speed(&g, h, 0.0), 0.0, "weightless, a height needs no take-off");
    }

    /// Under `UseJumpPower`, `JumpPower` is the take-off speed, whatever the
    /// gravity and whatever `JumpHeight` says; turned off, `JumpHeight` rules
    /// again.
    #[test]
    fn under_use_jump_power_the_npc_takes_off_at_jump_power() {
        let mut g = DataModel::new();
        let h = humanoid(&mut g);
        g.set_prop(h, "JumpHeight", DmValue::Number(2.0)).unwrap();
        g.set_prop(h, "JumpPower", DmValue::Number(9.5)).unwrap();
        g.set_prop(h, "UseJumpPower", DmValue::Bool(true)).unwrap();
        for gravity in [0.0_f32, 1.62, STANDARD_GRAVITY_F32, 54.936] {
            assert_eq!(jump_speed(&g, h, gravity), 9.5, "gravity {gravity}");
        }
        g.set_prop(h, "UseJumpPower", DmValue::Bool(false)).unwrap();
        let v = jump_speed(&g, h, STANDARD_GRAVITY_F32);
        assert!((v * v / (2.0 * STANDARD_GRAVITY_F32) - 2.0).abs() < 1e-4, "JumpHeight rules again");
    }

    /// The same rule as the player's jump, so equal numbers take off alike.
    #[test]
    fn an_npc_and_the_player_take_off_alike() {
        let mut g = DataModel::new();
        let h = humanoid(&mut g);
        let mut motion = ResolvedMotion {
            walk_speed: 1.0,
            run_speed: 1.0,
            sprint_multiplier: 1.0,
            jump_apex_m: 0.0,
            jump_speed_mps: None,
        };
        for height in [0.5_f32, 2.0, 7.2] {
            g.set_prop(h, "JumpHeight", DmValue::Number(height as f64)).unwrap();
            motion.jump_apex_m = height;
            for gravity in [1.62_f32, STANDARD_GRAVITY_F32, 54.936] {
                let player = motion.jump_velocity_under(gravity);
                assert!((jump_speed(&g, h, gravity) - player).abs() < 1e-5, "height {height}, gravity {gravity}");
            }
        }
        g.set_prop(h, "JumpPower", DmValue::Number(11.0)).unwrap();
        g.set_prop(h, "UseJumpPower", DmValue::Bool(true)).unwrap();
        motion.jump_speed_mps = Some(11.0);
        assert_eq!(jump_speed(&g, h, STANDARD_GRAVITY_F32), motion.jump_velocity());
    }

    #[test]
    fn a_bad_number_does_not_launch_the_body() {
        let mut g = DataModel::new();
        let h = humanoid(&mut g);
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -3.0] {
            g.set_prop(h, "UseJumpPower", DmValue::Bool(false)).unwrap();
            g.set_prop(h, "JumpHeight", DmValue::Number(bad)).unwrap();
            assert_eq!(jump_speed(&g, h, STANDARD_GRAVITY_F32), 0.0, "JumpHeight {bad}");
            g.set_prop(h, "UseJumpPower", DmValue::Bool(true)).unwrap();
            g.set_prop(h, "JumpPower", DmValue::Number(bad)).unwrap();
            assert_eq!(jump_speed(&g, h, STANDARD_GRAVITY_F32), 0.0, "JumpPower {bad}");
        }
    }
}
