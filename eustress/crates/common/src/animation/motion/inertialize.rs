//! # Inertialization
//!
//! A switch between two clips is hidden by an offset, not a crossfade. When the
//! matcher switches, the pose that was on screen (and the speed it was moving
//! at) is compared with the new clip's pose and speed. The difference is the
//! offset, and it decays to nothing: each frame the new clip's pose is
//! composed with what is left of it, so the screen continues from where it was
//! and settles into the new clip. Two clips are never played and blended.
//!
//! Rotations carry their offset as a scaled axis (axis times angle) and are
//! composed on the left of the clip's rotation, so the offset is expressed in
//! the joint's parent space. Translations carry theirs as a plain vector, for
//! the joints that are asked to translate (the hips of a skeleton). Each
//! offset follows a critically damped spring, so it leaves with the speed the
//! screen had and arrives without overshoot:
//!
//! ```text
//! x(t) = (x0 + (v0 + y * x0) * t) * e^(-y * t)        y = DECAY_K / blend_time
//! ```
//!
//! `DECAY_K` is chosen so that a switch that starts at rest has fallen to about
//! 2 percent of its size when `blend_time` has passed.
//!
//! The module is plain data and arithmetic, with no scene or ECS access: the
//! caller hands it the sampled pose of each frame.

use bevy::math::{Quat, Vec3};

/// `y * blend_time`: the offset of a switch from rest has fallen to about
/// 2 percent at `blend_time`, since `(1 + 5.8) * e^-5.8` is 0.0206.
pub const DECAY_K: f32 = 5.8;

/// Shortest `blend_time` accepted, seconds.
const MIN_BLEND_TIME: f32 = 0.02;

/// An offset smaller than this (radians or metres) counts as gone.
const SETTLED_OFFSET: f32 = 1.0e-4;

/// An offset speed smaller than this (radians or metres per second) counts as
/// gone.
const SETTLED_SPEED: f32 = 1.0e-3;

/// One joint's local transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointXf {
    pub rotation: Quat,
    pub translation: Vec3,
}

impl Default for JointXf {
    fn default() -> Self {
        Self { rotation: Quat::IDENTITY, translation: Vec3::ZERO }
    }
}

/// One joint's local speed: the angular speed as a scaled axis, radians per
/// second, and the linear speed, metres per second.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct JointVel {
    pub angular: Vec3,
    pub linear: Vec3,
}

/// The rotation `q` as a scaled axis, taking the short way round.
fn log_rotation(q: Quat) -> Vec3 {
    let q = if q.w < 0.0 { -q } else { q };
    q.to_scaled_axis()
}

/// The speed that takes `prev` to `next` in `dt` seconds. Zero when `dt` is
/// not positive.
pub fn joint_velocity(prev: &JointXf, next: &JointXf, dt: f32) -> JointVel {
    if dt <= 1.0e-6 {
        return JointVel::default();
    }
    JointVel {
        angular: log_rotation(next.rotation * prev.rotation.inverse()) / dt,
        linear: (next.translation - prev.translation) / dt,
    }
}

/// The speeds that take `prev` to `now` in `dt` seconds, joint by joint.
pub fn pose_velocities(prev: &[JointXf], now: &[JointXf], dt: f32) -> Vec<JointVel> {
    prev.iter().zip(now).map(|(p, n)| joint_velocity(p, n, dt)).collect()
}

/// A critically damped spring that carries an offset to zero.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Decay {
    /// The offset.
    pub x: Vec3,
    /// Its rate of change.
    pub v: Vec3,
}

impl Decay {
    /// Advance by `dt` seconds at decay rate `y` (per second).
    pub fn step(&mut self, y: f32, dt: f32) {
        let j1 = self.v + self.x * y;
        let e = (-y * dt).exp();
        self.x = (self.x + j1 * dt) * e;
        self.v = (self.v - j1 * (y * dt)) * e;
    }

    /// Whether nothing visible is left of the offset.
    pub fn is_settled(&self) -> bool {
        self.x.length() < SETTLED_OFFSET && self.v.length() < SETTLED_SPEED
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct JointOffset {
    rotation: Decay,
    translation: Decay,
}

/// The offsets of one skeleton.
#[derive(Debug, Clone)]
pub struct Inertializer {
    blend_time: f32,
    offsets: Vec<JointOffset>,
    translated: Vec<bool>,
    /// The pose and speed that were on screen last frame.
    last: Vec<JointXf>,
    last_vel: Vec<JointVel>,
    primed: bool,
}

impl Inertializer {
    /// An inertializer for `joints` joints. Only rotations are offset until a
    /// joint is added with [`Inertializer::translate_joint`].
    pub fn new(joints: usize, blend_time: f32) -> Self {
        Self {
            blend_time: blend_time.max(MIN_BLEND_TIME),
            offsets: vec![JointOffset::default(); joints],
            translated: vec![false; joints],
            last: vec![JointXf::default(); joints],
            last_vel: vec![JointVel::default(); joints],
            primed: false,
        }
    }

    /// Offset this joint's translation as well as its rotation (the hips).
    pub fn translate_joint(&mut self, joint: usize) -> &mut Self {
        if let Some(t) = self.translated.get_mut(joint) {
            *t = true;
        }
        self
    }

    pub fn set_blend_time(&mut self, blend_time: f32) {
        self.blend_time = blend_time.max(MIN_BLEND_TIME);
    }

    pub fn blend_time(&self) -> f32 {
        self.blend_time
    }

    pub fn joint_count(&self) -> usize {
        self.offsets.len()
    }

    /// Whether no joint has a visible offset left.
    pub fn is_settled(&self) -> bool {
        self.offsets.iter().all(|o| o.rotation.is_settled() && o.translation.is_settled())
    }

    fn rate(&self) -> f32 {
        DECAY_K / self.blend_time
    }

    /// Start a transition to a new clip. `source` and `source_vel` are the new
    /// clip's pose and speed at the moment of the switch. The offset is what
    /// takes the new clip's pose to the one that was on screen, with the speed
    /// that was on screen, so a switch in the middle of an unfinished
    /// transition starts from the displayed pose, which already holds the old
    /// offset. Does nothing before the first [`Inertializer::apply`].
    pub fn switch(&mut self, source: &[JointXf], source_vel: &[JointVel]) {
        if !self.primed {
            return;
        }
        let joints = self.offsets.len().min(source.len());
        for i in 0..joints {
            let vel = source_vel.get(i).copied().unwrap_or_default();
            let o = &mut self.offsets[i];
            o.rotation.x = log_rotation(self.last[i].rotation * source[i].rotation.inverse());
            o.rotation.v = self.last_vel[i].angular - vel.angular;
            if self.translated[i] {
                o.translation.x = self.last[i].translation - source[i].translation;
                o.translation.v = self.last_vel[i].linear - vel.linear;
            } else {
                o.translation = Decay::default();
            }
        }
    }

    /// Advance the offsets by `dt` seconds and compose them onto `pose`, the
    /// new clip's pose this frame. `pose` becomes what is displayed, and is
    /// remembered, with its speed, for the next switch.
    pub fn apply(&mut self, dt: f32, pose: &mut [JointXf]) {
        let y = self.rate();
        let joints = self.offsets.len().min(pose.len());
        for i in 0..joints {
            let o = &mut self.offsets[i];
            if dt > 0.0 {
                o.rotation.step(y, dt);
                if self.translated[i] {
                    o.translation.step(y, dt);
                }
            }
            pose[i].rotation = (Quat::from_scaled_axis(o.rotation.x) * pose[i].rotation).normalize();
            if self.translated[i] {
                pose[i].translation += o.translation.x;
            }
            self.last_vel[i] = if self.primed { joint_velocity(&self.last[i], &pose[i], dt) } else { JointVel::default() };
            self.last[i] = pose[i];
        }
        self.primed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn at(rotation: Quat, translation: Vec3) -> JointXf {
        JointXf { rotation, translation }
    }

    fn angle(a: Quat, b: Quat) -> f32 {
        log_rotation(a * b.inverse()).length()
    }

    const DT: f32 = 1.0 / 60.0;

    /// A switch starts on the pose that was on screen and ends on the new clip.
    #[test]
    fn a_switch_starts_on_the_displayed_pose_and_settles_into_the_new_clip() {
        let mut inertia = Inertializer::new(1, 0.2);
        let shown = at(Quat::from_rotation_z(PI / 2.0), Vec3::ZERO);
        let mut pose = [shown];
        inertia.apply(DT, &mut pose);

        let source = [at(Quat::IDENTITY, Vec3::ZERO)];
        inertia.switch(&source, &[JointVel::default()]);
        let mut pose = source;
        inertia.apply(DT, &mut pose);
        assert!(angle(pose[0].rotation, shown.rotation) < 0.2, "the first frame is near the old pose");

        // Past 1.5 times the blend time almost nothing of the offset is left.
        for _ in 0..(0.3 / DT) as usize {
            let mut pose = source;
            inertia.apply(DT, &mut pose);
        }
        let mut pose = source;
        inertia.apply(DT, &mut pose);
        assert!(angle(pose[0].rotation, Quat::IDENTITY) < 0.02, "settled into the new clip");
    }

    /// The displayed speed carries across the switch: the first frame moves at
    /// about the speed the screen had, not by the whole gap to the new clip.
    #[test]
    fn the_speed_on_screen_is_kept_across_a_switch() {
        let mut inertia = Inertializer::new(1, 0.2);
        inertia.translate_joint(0);
        // Moving along x at 1 m/s.
        let dt = 1.0 / 240.0;
        let mut before = 0.0;
        for k in 0..4 {
            let mut shown = [at(Quat::IDENTITY, Vec3::X * (k as f32 * dt))];
            inertia.apply(dt, &mut shown);
            before = shown[0].translation.x;
        }

        // The new clip is still at x = 0.5.
        let source = [at(Quat::IDENTITY, Vec3::new(0.5, 0.0, 0.0))];
        inertia.switch(&source, &[JointVel::default()]);
        let mut first = source;
        inertia.apply(dt, &mut first);
        let speed = (first[0].translation.x - before) / dt;
        assert!(speed > 0.0 && speed < 2.5, "about the 1 m/s the screen had, not a jump of 117 m/s, got {speed}");
    }

    /// A rotation offset never takes the long way round.
    #[test]
    fn a_rotation_offset_takes_the_short_way() {
        let mut inertia = Inertializer::new(1, 0.2);
        let shown = at(Quat::from_rotation_y(179.0_f32.to_radians()), Vec3::ZERO);
        let mut pose = [shown];
        inertia.apply(DT, &mut pose);
        let source = [at(Quat::from_rotation_y(-179.0_f32.to_radians()), Vec3::ZERO)];
        inertia.switch(&source, &[JointVel::default()]);
        let mut pose = source;
        inertia.apply(DT, &mut pose);
        assert!(angle(pose[0].rotation, shown.rotation) < 5.0_f32.to_radians(), "2 degrees apart, not 358");
    }

    /// A joint not asked to translate keeps the clip's translation.
    #[test]
    fn only_asked_joints_offset_their_translation() {
        let mut inertia = Inertializer::new(2, 0.2);
        inertia.translate_joint(0);
        let mut pose = [at(Quat::IDENTITY, Vec3::Y), at(Quat::IDENTITY, Vec3::Y)];
        inertia.apply(DT, &mut pose);
        let source = [at(Quat::IDENTITY, Vec3::ZERO), at(Quat::IDENTITY, Vec3::ZERO)];
        inertia.switch(&source, &[JointVel::default(); 2]);
        let mut pose = source;
        inertia.apply(DT, &mut pose);
        assert!(pose[0].translation.y > 0.5, "the hips keep the old height at first");
        assert_eq!(pose[1].translation, Vec3::ZERO, "another joint takes the clip's own");
    }

    #[test]
    fn a_settled_inertializer_changes_nothing() {
        let mut inertia = Inertializer::new(1, 0.2);
        let original = at(Quat::from_rotation_x(0.4), Vec3::new(0.1, 0.2, 0.3));
        let mut pose = [original];
        inertia.apply(DT, &mut pose);
        assert!(inertia.is_settled());
        let mut again = [original];
        inertia.apply(DT, &mut again);
        assert!(angle(again[0].rotation, original.rotation) < 1.0e-5);
        assert_eq!(again[0].translation, original.translation);
    }

    #[test]
    fn a_switch_before_the_first_pose_is_ignored() {
        let mut inertia = Inertializer::new(1, 0.2);
        inertia.switch(&[at(Quat::from_rotation_z(1.0), Vec3::ZERO)], &[JointVel::default()]);
        assert!(inertia.is_settled());
    }

    #[test]
    fn the_decay_halves_and_stays_put_at_zero() {
        let mut decay = Decay { x: Vec3::X, v: Vec3::ZERO };
        let y = DECAY_K / 0.2;
        decay.step(y, 0.2);
        assert!(decay.x.x < 0.03 && decay.x.x > 0.0, "about 2 percent at the blend time, got {}", decay.x.x);
        let mut still = Decay::default();
        still.step(y, 0.1);
        assert_eq!(still, Decay::default());
    }
}
