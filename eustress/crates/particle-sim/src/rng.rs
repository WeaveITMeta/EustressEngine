//! Deterministic randomness for the solver.
//!
//! Sequential draws (placement, emission) use [`SimRng`]; per-particle draws
//! inside parallel loops use [`counter_uniform`] / [`counter_normal`], a
//! counter-based hash of (seed, step, particle, lane). Both depend only on
//! their inputs, so a run is bit-for-bit reproducible regardless of thread
//! count or scheduling.

use bevy_math::Vec3;

#[inline]
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[inline]
fn to_unit_f32(x: u64) -> f32 {
    // 24 high bits -> [0, 1).
    (x >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
}

/// Small sequential generator (SplitMix64).
#[derive(Clone, Debug)]
pub struct SimRng {
    state: u64,
}

impl SimRng {
    pub fn new(seed: u64, stream: u64) -> Self {
        Self { state: splitmix64(seed ^ splitmix64(stream.wrapping_add(0xA5A5_5A5A))) }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        splitmix64(self.state)
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn uniform(&mut self) -> f32 {
        to_unit_f32(self.next_u64())
    }

    /// Standard normal (Box-Muller).
    #[inline]
    pub fn normal(&mut self) -> f32 {
        let u1 = self.uniform().max(1e-7);
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }

    pub fn normal3(&mut self) -> Vec3 {
        Vec3::new(self.normal(), self.normal(), self.normal())
    }
}

/// Uniform in [0, 1) from a pure function of its inputs.
#[inline]
pub fn counter_uniform(seed: u64, step: u64, index: u64, lane: u64) -> f32 {
    let h = splitmix64(seed ^ splitmix64(step ^ splitmix64(index.wrapping_mul(4).wrapping_add(lane))));
    to_unit_f32(h)
}

/// Standard normal from a pure function of its inputs.
#[inline]
pub fn counter_normal(seed: u64, step: u64, index: u64, lane: u64) -> f32 {
    let u1 = counter_uniform(seed, step, index, lane * 2 + 11).max(1e-7);
    let u2 = counter_uniform(seed, step, index, lane * 2 + 12);
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_is_reproducible() {
        let mut a = SimRng::new(7, 3);
        let mut b = SimRng::new(7, 3);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = SimRng::new(7, 4);
        assert_ne!(SimRng::new(7, 3).next_u64(), c.next_u64());
    }

    #[test]
    fn normal_has_unit_variance() {
        let mut r = SimRng::new(1, 1);
        let n = 200_000;
        let (mut s, mut s2) = (0.0f64, 0.0f64);
        for _ in 0..n {
            let x = r.normal() as f64;
            s += x;
            s2 += x * x;
        }
        let mean = s / n as f64;
        let var = s2 / n as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.01, "var {var}");
    }

    #[test]
    fn counter_normal_has_unit_variance() {
        let n = 200_000u64;
        let (mut s, mut s2) = (0.0f64, 0.0f64);
        for i in 0..n {
            let x = counter_normal(42, 9, i, 1) as f64;
            s += x;
            s2 += x * x;
        }
        let mean = s / n as f64;
        let var = s2 / n as f64 - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        assert!((var - 1.0).abs() < 0.01, "var {var}");
    }
}
