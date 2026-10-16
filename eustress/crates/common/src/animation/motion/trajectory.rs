//! # Trajectory
//!
//! Where a character is about to go, and which way it will face, in the
//! character's own frame. The matcher compares this path with the paths the
//! database's clips take.
//!
//! The desired velocity and facing go through a critically damped spring. The
//! controller follows the same spring, so the prediction and the body agree:
//! a character that is told to run forward does not jump to the run, it
//! accelerates along the curve the spring draws, and so does the prediction.
//!
//! Planar vectors are `(x, z)` in the character's frame: `+x` is its right and
//! `-z` is forward. A world-space yaw `a` faces `(-sin a, -cos a)`, which is
//! `Quat::from_rotation_y(a) * Vec3::NEG_Z` seen from above.

use bevy::math::Vec2;
use std::f32::consts::{PI, TAU};

/// `y * half_life` for which a critically damped spring that starts at rest has
/// closed half of its distance after `half_life`: `(1 + 1.6783) * e^-1.6783`
/// is 0.5.
const HALF_LIFE_K: f32 = 1.6783;

/// Seconds per step of the prediction.
pub const SIM_STEP: f32 = 1.0 / 30.0;

/// The seconds ahead the path is sampled at, unless a database says otherwise.
pub const DEFAULT_TRAJECTORY_TIMES: [f32; 4] = [0.2, 0.4, 0.6, 1.0];

/// One point of the predicted path, in the character's frame as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrajectoryPoint {
    /// Where the root will be.
    pub position: Vec2,
    /// The unit vector it will face along.
    pub facing: Vec2,
}

/// What the character is doing now, in world space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlanarState {
    /// World `(x, z)`.
    pub position: Vec2,
    pub velocity: Vec2,
    /// The rate of change of `velocity`; zero when unknown.
    pub acceleration: Vec2,
    pub yaw: f32,
    pub yaw_rate: f32,
}

/// What the character is asked to do: where to go and which way to face.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Desired {
    /// World `(x, z)`, metres per second.
    pub velocity: Vec2,
    pub yaw: f32,
}

/// The unit facing vector of a yaw.
pub fn forward(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}

/// A world-space vector in the frame of a character facing `yaw`.
pub fn to_local(world: Vec2, yaw: f32) -> Vec2 {
    let (s, c) = yaw.sin_cos();
    Vec2::new(world.x * c - world.y * s, world.x * s + world.y * c)
}

/// A vector in the frame of a character facing `yaw`, in world space.
pub fn to_world(local: Vec2, yaw: f32) -> Vec2 {
    let (s, c) = yaw.sin_cos();
    Vec2::new(local.x * c + local.y * s, -local.x * s + local.y * c)
}

/// An angle brought into `[-PI, PI)`.
pub fn wrap_angle(angle: f32) -> f32 {
    angle - TAU * ((angle + PI) / TAU).floor()
}

/// The spring rate for a half-life, per second.
pub fn spring_rate(half_life: f32) -> f32 {
    HALF_LIFE_K / half_life.max(1.0e-3)
}

/// One step of a critically damped spring on `x` (rate of change `v`) toward
/// `goal`, for `dt` seconds at rate `y`.
fn spring(x: f32, v: f32, goal: f32, y: f32, dt: f32) -> (f32, f32) {
    let j0 = x - goal;
    let j1 = v + j0 * y;
    let e = (-y * dt).exp();
    (goal + (j0 + j1 * dt) * e, (v - j1 * y * dt) * e)
}

fn spring2(x: Vec2, v: Vec2, goal: Vec2, y: f32, dt: f32) -> (Vec2, Vec2) {
    let (xx, vx) = spring(x.x, v.x, goal.x, y, dt);
    let (xz, vz) = spring(x.y, v.y, goal.y, y, dt);
    (Vec2::new(xx, xz), Vec2::new(vx, vz))
}

/// Predict the path at each of `times` (seconds ahead, ascending) into `out`,
/// in the frame the character is in now.
pub fn predict_trajectory(
    state: &PlanarState,
    desired: &Desired,
    half_life: f32,
    times: &[f32],
    out: &mut Vec<TrajectoryPoint>,
) {
    out.clear();
    let y = spring_rate(half_life);
    let mut position = state.position;
    let mut velocity = state.velocity;
    let mut acceleration = state.acceleration;
    // The yaw as a distance from the goal, which the spring closes.
    let mut yaw_gap = wrap_angle(state.yaw - desired.yaw);
    let mut yaw_rate = state.yaw_rate;
    let mut elapsed = 0.0_f32;
    for &time in times {
        while elapsed + 1.0e-6 < time {
            let dt = SIM_STEP.min(time - elapsed);
            let before = velocity;
            (velocity, acceleration) = spring2(velocity, acceleration, desired.velocity, y, dt);
            position += (before + velocity) * (0.5 * dt);
            (yaw_gap, yaw_rate) = spring(yaw_gap, yaw_rate, 0.0, y, dt);
            elapsed += dt;
        }
        out.push(TrajectoryPoint {
            position: to_local(position - state.position, state.yaw),
            facing: forward(desired.yaw + yaw_gap - state.yaw),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn predict(state: PlanarState, desired: Desired, half_life: f32) -> Vec<TrajectoryPoint> {
        let mut out = Vec::new();
        predict_trajectory(&state, &desired, half_life, &DEFAULT_TRAJECTORY_TIMES, &mut out);
        out
    }

    /// The frame conversions are inverses, and forward is -z.
    #[test]
    fn the_character_frame_has_forward_as_minus_z() {
        for yaw in [0.0_f32, 0.7, -2.0, 3.0] {
            let v = Vec2::new(0.3, -1.2);
            let back = to_world(to_local(v, yaw), yaw);
            assert!((back - v).length() < 1.0e-5);
            // The world direction the character faces is its local forward.
            let local = to_local(forward(yaw), yaw);
            assert!((local - Vec2::new(0.0, -1.0)).length() < 1.0e-5, "yaw {yaw}: {local:?}");
        }
    }

    /// Told to go forward from rest, the path runs straight ahead and lags the
    /// spring.
    #[test]
    fn a_character_sent_forward_accelerates_along_its_own_forward() {
        // Facing world +x, going +x at 4 m/s.
        let state = PlanarState { yaw: -std::f32::consts::FRAC_PI_2, ..Default::default() };
        let desired = Desired { velocity: Vec2::new(4.0, 0.0), yaw: state.yaw };
        let path = predict(state, desired, 0.15);
        for point in &path {
            assert!(point.position.x.abs() < 1.0e-3, "straight: {point:?}");
            assert!(point.position.y < 0.0, "ahead is -z: {point:?}");
            assert!((point.facing - Vec2::new(0.0, -1.0)).length() < 1.0e-3);
        }
        for pair in path.windows(2) {
            assert!(pair[1].position.y < pair[0].position.y, "farther each time");
        }
        // 4 m/s for a second would be 4 m; the spring's lag costs about 0.7 m.
        let far = -path[3].position.y;
        assert!(far > 3.0 && far < 3.6, "got {far}");
    }

    /// Already at speed, the path is a straight line at that speed.
    #[test]
    fn a_character_at_speed_keeps_going() {
        let state = PlanarState { velocity: Vec2::new(0.0, -1.45), ..Default::default() };
        let desired = Desired { velocity: Vec2::new(0.0, -1.45), yaw: 0.0 };
        let path = predict(state, desired, 0.15);
        for (point, time) in path.iter().zip(DEFAULT_TRAJECTORY_TIMES) {
            assert!((point.position.y + 1.45 * time).abs() < 1.0e-3, "{point:?} at {time}");
        }
    }

    /// A turn of 180 degrees ends facing backward, and takes the short way.
    #[test]
    fn a_turn_faces_the_goal_by_the_end() {
        let state = PlanarState::default();
        let desired = Desired { velocity: Vec2::ZERO, yaw: PI };
        let path = predict(state, desired, 0.1);
        let last = path[3].facing;
        assert!(last.dot(Vec2::new(0.0, 1.0)) > 0.95, "faces backward by 1 s, got {last:?}");
        assert!(path[0].facing.dot(Vec2::new(0.0, -1.0)) < 0.9, "already turning at 0.2 s");
    }

    /// Across the wrap of the angle, the short way is taken.
    #[test]
    fn a_turn_across_the_wrap_does_not_spin_the_long_way() {
        let state = PlanarState { yaw: 3.0, ..Default::default() };
        let desired = Desired { velocity: Vec2::ZERO, yaw: -3.0 };
        let path = predict(state, desired, 0.15);
        // 6 radians apart on the number line, 0.28 radians apart on the circle.
        assert!(path[0].facing.x.abs() < 0.5, "{:?}", path[0]);
        assert!(wrap_angle(3.0 - (-3.0)).abs() < 0.3);
    }

    #[test]
    fn wrapping_an_angle_stays_in_range() {
        for a in [-10.0_f32, -3.2, 0.0, 3.2, 10.0] {
            let w = wrap_angle(a);
            assert!((-PI..PI).contains(&w), "{a} -> {w}");
            assert!((w - a).rem_euclid(TAU) < 1.0e-4 || (TAU - (w - a).rem_euclid(TAU)) < 1.0e-4);
        }
    }

    #[test]
    fn no_times_gives_an_empty_path() {
        let mut out = vec![TrajectoryPoint::default()];
        predict_trajectory(&PlanarState::default(), &Desired::default(), 0.15, &[], &mut out);
        assert!(out.is_empty());
    }
}
