//! # Foci: who the world is simulated and streamed around
//!
//! A focus is a point the world must be ready for: the viewport camera, a
//! player's character, a vehicle a player drives. Prefetch
//! ([`super::prefetch`]) loads ahead of where foci are going; simulation LOD
//! ([`super::sim_tier`]) keeps full fidelity near them.
//!
//! A focus is marked with [`StreamFocus`]. Its velocity is measured from its
//! own position samples ([`FocusTracker`]), not read from a physics body, so
//! a replica's kinematic avatar, an eased body and a free camera are all
//! measured the same way.

use std::collections::HashMap;

use bevy::prelude::*;

/// Marks an entity the world is streamed and simulated around: a player's
/// character, a driven vehicle, a camera. The play runtime tags every
/// avatar; anything else (a camera rig, a drone a script flies) can carry it
/// too.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct StreamFocus;

/// Speed under which a focus counts as stationary, m/s. Under a slow walk,
/// so jitter and drift never fabricate a heading.
pub const STATIONARY_SPEED: f32 = 0.25;

/// One focus's smoothed motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusMotion {
    pub pos: Vec3,
    /// EMA-smoothed velocity, m/s.
    pub vel: Vec3,
}

/// Smoothed velocity per focus, measured from position samples.
///
/// The same rule the engine's camera residency uses
/// (`engine::space::stream_predict::CameraMotion`), for any number of foci:
/// an exponential moving average over per-tick deltas, and a jump past the
/// teleport threshold drops the history instead of reading as speed.
#[derive(Resource, Debug, Default, Clone)]
pub struct FocusTracker {
    motions: HashMap<u64, FocusMotion>,
    /// Keys seen on the last [`FocusTracker::retain_seen`] pass.
    seen: Vec<u64>,
}

impl FocusTracker {
    /// Fold one position sample for focus `key` into its smoothed velocity
    /// and return the result. `smoothing` is the weight on the new sample,
    /// `0..=1`. A non-positive `dt` keeps the last estimate.
    pub fn sample(&mut self, key: u64, pos: Vec3, dt: f32, smoothing: f32, teleport: f32) -> FocusMotion {
        self.seen.push(key);
        if !pos.is_finite() {
            return self.motions.get(&key).copied().unwrap_or(FocusMotion { pos: Vec3::ZERO, vel: Vec3::ZERO });
        }
        let a = smoothing.clamp(0.0, 1.0);
        let entry = self.motions.entry(key).or_insert(FocusMotion { pos, vel: Vec3::ZERO });
        let delta = pos - entry.pos;
        if delta.length() >= teleport {
            *entry = FocusMotion { pos, vel: Vec3::ZERO };
            return *entry;
        }
        if dt > f32::EPSILON {
            let instant = delta / dt;
            entry.vel = entry.vel * (1.0 - a) + instant * a;
        }
        entry.pos = pos;
        *entry
    }

    /// Forget foci not sampled since the last call, so a despawned avatar's
    /// key never lingers.
    pub fn retain_seen(&mut self) {
        let seen: std::collections::HashSet<u64> = self.seen.drain(..).collect();
        self.motions.retain(|k, _| seen.contains(k));
    }

    /// Number of foci tracked.
    pub fn len(&self) -> usize {
        self.motions.len()
    }

    /// Whether no focus is tracked.
    pub fn is_empty(&self) -> bool {
        self.motions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rage_focus_velocity_converges_on_true_speed() {
        let mut t = FocusTracker::default();
        let mut m = FocusMotion { pos: Vec3::ZERO, vel: Vec3::ZERO };
        for i in 0..200 {
            m = t.sample(7, Vec3::new(4.0 * i as f32, 0.0, 0.0), 0.1, 0.3, 1_000.0);
        }
        assert!((m.vel.x - 40.0).abs() < 0.5, "40 m/s along +X, got {:?}", m.vel);
        assert!(m.vel.z.abs() < 1e-3);
    }

    #[test]
    fn rage_focus_teleport_resets_instead_of_spiking() {
        let mut t = FocusTracker::default();
        t.sample(1, Vec3::ZERO, 0.016, 0.3, 500.0);
        let m = t.sample(1, Vec3::new(20_000.0, 0.0, 0.0), 0.016, 0.3, 500.0);
        assert_eq!(m.vel, Vec3::ZERO, "a teleport is not motion");
    }

    #[test]
    fn rage_focus_forgets_unseen_keys() {
        let mut t = FocusTracker::default();
        t.sample(1, Vec3::ZERO, 0.016, 0.3, 500.0);
        t.sample(2, Vec3::ZERO, 0.016, 0.3, 500.0);
        t.retain_seen();
        assert_eq!(t.len(), 2);
        t.sample(1, Vec3::X, 0.016, 0.3, 500.0);
        t.retain_seen();
        assert_eq!(t.len(), 1, "focus 2 was not sampled this pass");
    }
}
