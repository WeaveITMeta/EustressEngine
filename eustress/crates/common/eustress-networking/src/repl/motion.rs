//! The motion lane: where the bodies physics moves are, and how a player
//! draws them between samples.
//!
//! The host samples every body that physics moved and every player can see,
//! quantizes it to 40 bytes, and sends as many as fit a datagram. A lost
//! datagram is replaced by the next one. A player renders each body a little
//! in the past (the interpolation delay), between the two samples around
//! that moment, using their velocities as well as their poses.

use std::collections::{HashMap, VecDeque};

use bevy::math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use super::id::NetId;

/// Largest datagram the motion lane sends: under the smallest path MTU
/// QUIC allows, with room for its headers.
pub const MAX_MOTION_DATAGRAM: usize = 1100;
/// Encoded size of one [`BodyState`].
pub const BODY_BYTES: usize = 40;
/// A frame's fixed cost: its enum tag, tick and vector length.
const FRAME_OVERHEAD: usize = 32;
/// Host ticks per second.
pub const TICK_HZ: f64 = 60.0;
/// Ticks a body keeps moving along its last velocity when samples stop.
pub const MAX_EXTRAPOLATE_TICKS: f64 = 15.0;
/// Samples kept per body.
const HISTORY: usize = 32;

const LINEAR_SCALE: f32 = 100.0; // 1 cm/s, to ±327 m/s
const ANGULAR_SCALE: f32 = 100.0; // 10 mrad/s, to ±327 rad/s: a 0.3 m wheel at 350 km/h
const ROT_SCALE: f32 = 32767.0;

/// One body at one tick, as sent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodyState {
    pub id: NetId,
    pub pos: [f32; 3],
    /// Unit quaternion (x, y, z, w) with w ≥ 0, each × 32767.
    pub rot: [i16; 4],
    /// Linear velocity, cm/s.
    pub lin: [i16; 3],
    /// Angular velocity in world axes, hundredths of a radian per second.
    pub ang: [i16; 3],
}

fn to_i16(v: f32, scale: f32) -> i16 {
    (v * scale).round().clamp(-32767.0, 32767.0) as i16
}

impl BodyState {
    pub fn new(id: NetId, position: Vec3, rotation: Quat, linear: Vec3, angular: Vec3) -> Self {
        let q = rotation.normalize();
        let q = if q.w < 0.0 { -q } else { q };
        Self {
            id,
            pos: position.to_array(),
            rot: [to_i16(q.x, ROT_SCALE), to_i16(q.y, ROT_SCALE), to_i16(q.z, ROT_SCALE), to_i16(q.w, ROT_SCALE)],
            lin: linear.to_array().map(|v| to_i16(v, LINEAR_SCALE)),
            ang: angular.to_array().map(|v| to_i16(v, ANGULAR_SCALE)),
        }
    }

    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.pos)
    }

    pub fn rotation(&self) -> Quat {
        let q = Quat::from_xyzw(
            self.rot[0] as f32 / ROT_SCALE,
            self.rot[1] as f32 / ROT_SCALE,
            self.rot[2] as f32 / ROT_SCALE,
            self.rot[3] as f32 / ROT_SCALE,
        );
        if q.length_squared() < 1e-6 {
            Quat::IDENTITY
        } else {
            q.normalize()
        }
    }

    pub fn linear(&self) -> Vec3 {
        Vec3::from_array(self.lin.map(|v| v as f32 / LINEAR_SCALE))
    }

    pub fn angular(&self) -> Vec3 {
        Vec3::from_array(self.ang.map(|v| v as f32 / ANGULAR_SCALE))
    }

    /// Worth sending again: moved or turned past what the wire resolves.
    pub fn differs_from(&self, other: &BodyState) -> bool {
        self.rot != other.rot
            || self.lin != other.lin
            || self.ang != other.ang
            || self.position().distance_squared(other.position()) > 1e-6
    }

    pub fn is_finite(&self) -> bool {
        self.pos.iter().all(|v| v.is_finite())
    }
}

/// Bodies at one host tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MotionFrame {
    pub tick: u64,
    pub bodies: Vec<BodyState>,
}

/// The host tick `secs` seconds into the host's clock. Every tick a host
/// stamps comes from here, so its frames and its Welcome agree.
pub fn tick_at(secs: f64) -> u64 {
    (secs.max(0.0) * TICK_HZ) as u64
}

/// Bodies one datagram carries.
pub const fn bodies_per_datagram() -> usize {
    (MAX_MOTION_DATAGRAM - FRAME_OVERHEAD) / BODY_BYTES
}

/// `bodies` at `tick`, as frames that each fit one datagram.
pub fn split_motion(tick: u64, bodies: &[BodyState]) -> Vec<MotionFrame> {
    bodies
        .chunks(bodies_per_datagram())
        .map(|c| MotionFrame { tick, bodies: c.to_vec() })
        .collect()
}

/// A player's estimate of the host's clock, from the ticks frames carry.
///
/// Each frame says "the host was at tick T when it sent this". The least
/// delayed frames give the tightest estimate, so the offset follows the
/// largest `T / hz - local time` seen, and gives way slowly (10 ms per second)
/// so a route that got longer, or two clocks that drift, are followed too.
#[derive(Debug, Clone, Default)]
pub struct TickClock {
    offset: Option<f64>,
    last_local: f64,
    /// How far behind the best estimate frames typically arrive, in seconds.
    jitter: f64,
}

impl TickClock {
    pub fn observe(&mut self, tick: u64, local_secs: f64) {
        let sample = tick as f64 / TICK_HZ - local_secs;
        match self.offset {
            None => self.offset = Some(sample),
            Some(offset) => {
                let elapsed = (local_secs - self.last_local).max(0.0);
                let decayed = offset - 0.01 * elapsed;
                let next = decayed.max(sample);
                self.jitter += ((next - sample) - self.jitter) * 0.05;
                self.offset = Some(next);
            }
        }
        self.last_local = local_secs;
    }

    /// The host's tick now, fractional.
    pub fn host_tick(&self, local_secs: f64) -> Option<f64> {
        self.offset.map(|o| (local_secs + o) * TICK_HZ)
    }

    /// How far in the past to draw other bodies, in ticks: two send
    /// intervals, plus twice the jitter, within 3 to 30 ticks.
    pub fn delay_ticks(&self, send_interval_ticks: f64) -> f64 {
        (2.0 * send_interval_ticks + 2.0 * self.jitter * TICK_HZ).clamp(3.0, 30.0)
    }
}

/// Recent samples per body, and the pose to draw at any moment.
#[derive(Debug, Default)]
pub struct Interpolator {
    bodies: HashMap<NetId, VecDeque<(u64, BodyState)>>,
}

impl Interpolator {
    pub fn push(&mut self, frame: &MotionFrame) {
        for b in frame.bodies.iter().filter(|b| b.is_finite()) {
            let history = self.bodies.entry(b.id).or_default();
            // Datagrams can arrive out of order: keep ticks sorted, one each.
            let at = history.partition_point(|(t, _)| *t < frame.tick);
            if history.get(at).is_some_and(|(t, _)| *t == frame.tick) {
                continue;
            }
            history.insert(at, (frame.tick, *b));
            while history.len() > HISTORY {
                history.pop_front();
            }
        }
    }

    /// Stop drawing `id` (it was destroyed, or the host settled it).
    pub fn forget(&mut self, id: NetId) {
        self.bodies.remove(&id);
    }

    pub fn ids(&self) -> impl Iterator<Item = NetId> + '_ {
        self.bodies.keys().copied()
    }

    /// Where to draw `id` at host tick `at` (fractional): between the two
    /// samples around it, or carried on from the newest for a little while
    /// when none is newer, then held. A pose is always finite.
    pub fn sample(&self, id: NetId, at: f64) -> Option<(Vec3, Quat)> {
        if !at.is_finite() {
            return None;
        }
        self.pose_at(id, at).filter(|(pos, rot)| pos.is_finite() && rot.is_finite())
    }

    fn pose_at(&self, id: NetId, at: f64) -> Option<(Vec3, Quat)> {
        let h = self.bodies.get(&id)?;
        let (first_tick, first) = h.front()?;
        if at <= *first_tick as f64 {
            return Some((first.position(), first.rotation()));
        }
        let after = h.partition_point(|(t, _)| (*t as f64) < at);
        if after >= h.len() {
            let (t, last) = h.back()?;
            let ahead = (at - *t as f64).min(MAX_EXTRAPOLATE_TICKS) / TICK_HZ;
            let ahead = ahead as f32;
            let spin = last.angular() * ahead;
            let turn = if spin.length_squared() > 0.0 { Quat::from_scaled_axis(spin) } else { Quat::IDENTITY };
            return Some((last.position() + last.linear() * ahead, (turn * last.rotation()).normalize()));
        }
        let (t0, a) = &h[after - 1];
        let (t1, b) = &h[after];
        let span = (*t1 - *t0) as f64;
        let s = ((at - *t0 as f64) / span) as f32;
        let dt = (span / TICK_HZ) as f32;
        let pos = hermite(a.position(), a.linear() * dt, b.position(), b.linear() * dt, s);
        Some((pos, turn_between(a, b, s, dt)))
    }
}

/// The rotation `s` of the way from `a` to `b`, `dt` seconds apart, turning
/// the way their angular velocities say: `a` carried forward and `b` carried
/// back to the same moment, then blended. A plain slerp takes the shorter way
/// round, so a body turning more than half a turn between samples, like a
/// wheel at speed, would be drawn spinning backwards.
fn turn_between(a: &BodyState, b: &BodyState, s: f32, dt: f32) -> Quat {
    let turn = |spin: Vec3| if spin.length_squared() > 0.0 { Quat::from_scaled_axis(spin) } else { Quat::IDENTITY };
    let from_a = turn(a.angular() * (s * dt)) * a.rotation();
    let from_b = turn(b.angular() * (-(1.0 - s) * dt)) * b.rotation();
    from_a.slerp(from_b, s).normalize()
}

/// Cubic Hermite between `p0` and `p1` with tangents `m0`, `m1` (velocity
/// times the span), at `s` in 0..1.
fn hermite(p0: Vec3, m0: Vec3, p1: Vec3, m1: Vec3, s: f32) -> Vec3 {
    let s2 = s * s;
    let s3 = s2 * s;
    p0 * (2.0 * s3 - 3.0 * s2 + 1.0) + m0 * (s3 - 2.0 * s2 + s) + p1 * (-2.0 * s3 + 3.0 * s2) + m1 * (s3 - s2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_survives_quantization() {
        let q = Quat::from_euler(bevy::math::EulerRot::YXZ, 1.0, -0.4, 2.5);
        let b = BodyState::new(NetId(9), Vec3::new(1234.5, -2.0, 88.25), q, Vec3::new(30.0, -9.81, 0.5), Vec3::new(0.0, 3.0, -1.0));
        assert_eq!(b.position(), Vec3::new(1234.5, -2.0, 88.25));
        assert!(b.rotation().angle_between(q) < 1e-3);
        assert!((b.linear() - Vec3::new(30.0, -9.81, 0.5)).length() < 0.01);
        assert!((b.angular() - Vec3::new(0.0, 3.0, -1.0)).length() < 0.001);
        // q and -q are one rotation; the wire keeps one of them.
        assert_eq!(BodyState::new(NetId(9), Vec3::ZERO, -q, Vec3::ZERO, Vec3::ZERO).rot, b.rot);
        // Out-of-range speeds clamp instead of wrapping.
        let fast = BodyState::new(NetId(9), Vec3::ZERO, Quat::IDENTITY, Vec3::splat(1.0e6), Vec3::ZERO);
        assert!(fast.linear().x > 300.0);
    }

    #[test]
    fn a_datagram_of_bodies_fits() {
        let bodies: Vec<BodyState> = (0..bodies_per_datagram() as u64)
            .map(|i| BodyState::new(NetId(u64::MAX - i), Vec3::splat(1.0e5), Quat::IDENTITY, Vec3::splat(-300.0), Vec3::splat(30.0)))
            .collect();
        let frame = MotionFrame { tick: u64::MAX, bodies };
        let bytes = crate::wire::encode_datagram(&crate::wire::Datagram::Motion(frame));
        assert!(bytes.len() <= MAX_MOTION_DATAGRAM, "{} bytes", bytes.len());
        let many: Vec<BodyState> = (0..100).map(|i| BodyState::new(NetId(i), Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO, Vec3::ZERO)).collect();
        let parts = split_motion(5, &many);
        assert_eq!(parts.iter().map(|f| f.bodies.len()).sum::<usize>(), 100);
        assert!(parts.iter().all(|f| f.bodies.len() <= bodies_per_datagram()));
    }

    #[test]
    fn interpolation_follows_the_path_and_extrapolation_stops() {
        let mut it = Interpolator::default();
        let v = Vec3::new(6.0, 0.0, 0.0); // 6 m/s
        for tick in [0u64, 10, 20] {
            let x = 6.0 * tick as f32 / 60.0;
            it.push(&MotionFrame { tick, bodies: vec![BodyState::new(NetId(1), Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, v, Vec3::ZERO)] });
        }
        // Out of order and duplicate samples change nothing.
        it.push(&MotionFrame { tick: 10, bodies: vec![BodyState::new(NetId(1), Vec3::splat(99.0), Quat::IDENTITY, v, Vec3::ZERO)] });
        let (p, _) = it.sample(NetId(1), 15.0).unwrap();
        assert!((p.x - 1.5).abs() < 1e-3, "constant velocity stays on the line: {p}");
        // Carried on for at most 15 ticks past the newest sample, then held.
        let (p, _) = it.sample(NetId(1), 25.0).unwrap();
        assert!((p.x - 2.5).abs() < 1e-3);
        let (far, _) = it.sample(NetId(1), 1000.0).unwrap();
        assert!((far.x - (2.0 + 6.0 * MAX_EXTRAPOLATE_TICKS as f32 / 60.0)).abs() < 1e-3);
        assert!(it.sample(NetId(2), 5.0).is_none());
    }

    #[test]
    fn every_pose_drawn_is_finite() {
        let body = |pos: Vec3, rot: Quat| BodyState::new(NetId(1), pos, rot, Vec3::ZERO, Vec3::ZERO);
        let mut it = Interpolator::default();
        // A non-finite position never enters.
        it.push(&MotionFrame { tick: 0, bodies: vec![body(Vec3::NAN, Quat::IDENTITY)] });
        assert!(it.sample(NetId(1), 0.0).is_none());
        // A degenerate rotation decodes to identity.
        for (tick, rot) in [(0u64, Quat::from_xyzw(0.0, 0.0, 0.0, 0.0)), (1, Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0))] {
            it.push(&MotionFrame { tick, bodies: vec![body(Vec3::ONE, rot)] });
            assert_eq!(it.sample(NetId(1), tick as f64), Some((Vec3::ONE, Quat::IDENTITY)));
        }
        // A tick that is not a number draws nothing.
        assert!(it.sample(NetId(1), f64::NAN).is_none());
        assert!(it.sample(NetId(1), f64::INFINITY).is_none());
        // Even at the edge of f32, nothing non-finite is drawn.
        it.push(&MotionFrame { tick: 2, bodies: vec![body(Vec3::splat(f32::MAX), Quat::IDENTITY)] });
        it.push(&MotionFrame { tick: 3, bodies: vec![body(Vec3::splat(f32::MAX), Quat::IDENTITY)] });
        for step in 0..=40 {
            if let Some((p, r)) = it.sample(NetId(1), 1.0 + step as f64 * 0.1) {
                assert!(p.is_finite() && r.is_finite(), "step {step}: {p} {r}");
            }
        }
    }

    #[test]
    fn a_fast_wheel_turns_forward_between_samples() {
        // 150 rad/s, sampled every 2 ticks: 5 rad between samples, past half
        // a turn, where the shorter way round is backwards.
        let spin = 150.0_f32;
        let mut it = Interpolator::default();
        for tick in [0u64, 2, 4] {
            let rot = Quat::from_rotation_x(spin * tick as f32 / TICK_HZ as f32);
            it.push(&MotionFrame { tick, bodies: vec![BodyState::new(NetId(1), Vec3::ZERO, rot, Vec3::ZERO, Vec3::X * spin)] });
        }
        for at in [0.5f64, 1.0, 1.5, 2.5, 3.0] {
            let want = Quat::from_rotation_x(spin * at as f32 / TICK_HZ as f32);
            let (_, got) = it.sample(NetId(1), at).unwrap();
            assert!(got.angle_between(want) < 0.02, "at tick {at}: {} rad off", got.angle_between(want));
        }
    }

    #[test]
    fn the_clock_tracks_the_least_delayed_frame() {
        let mut c = TickClock::default();
        // The host is 100 s ahead of this player's clock; frames take 30 ms,
        // some 80 ms.
        for i in 0..200u64 {
            let local = i as f64 / 30.0;
            let host_tick = ((local + 100.0) * TICK_HZ) as u64;
            let delay = if i % 3 == 0 { 0.08 } else { 0.03 };
            c.observe(host_tick, local + delay);
        }
        let now = 200.0 / 30.0;
        let est = c.host_tick(now).unwrap() / TICK_HZ - now;
        assert!((est - (100.0 - 0.03)).abs() < 0.02, "estimated offset {est}");
        assert!(c.delay_ticks(2.0) >= 4.0);
    }
}
