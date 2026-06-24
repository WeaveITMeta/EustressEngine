//! Deterministic, dependency-free coherent noise for terrain generation.
//!
//! Every function here is a pure function of `(seed, x, z)` — no global
//! state, no RNG handle, no per-call allocation — so identical inputs yield
//! byte-identical outputs on every machine and every run. That determinism
//! is the contract the whole generator rests on: reproducible worlds, and
//! (crucially) **seam-free** regions, because the base field is one global
//! function sampled per region rather than a per-tile seed.
//!
//! The primitive is classic Perlin gradient noise with quintic fade;
//! [`fbm`] stacks octaves for rolling terrain and [`ridged`] biases toward
//! sharp crest lines for mountain belts. Gradients are derived from a
//! SplitMix64 lattice hash, so there is no permutation table to seed or
//! get out of sync.

/// SplitMix64-style integer hash of a 2D lattice point under a seed.
#[inline]
fn hash2(seed: u64, ix: i64, iz: i64) -> u64 {
    let mut h = seed
        ^ (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iz as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    h
}

/// Unit gradient vector at lattice point `(ix, iz)` — angle derived
/// deterministically from the hash, so the field is continuous and
/// repeatable.
#[inline]
fn gradient(seed: u64, ix: i64, iz: i64) -> (f64, f64) {
    // Top 53 bits → [0,1) → angle in [0, 2π).
    let h = hash2(seed, ix, iz);
    let unit = (h >> 11) as f64 / (1u64 << 53) as f64;
    let a = unit * std::f64::consts::TAU;
    (a.cos(), a.sin())
}

/// Quintic smootherstep (Perlin's improved fade) — C2-continuous, so the
/// noise has no second-derivative creases at lattice boundaries.
#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// 2D Perlin gradient noise. Output is in `[-1, 1]` (the ±√2⁄2 theoretical
/// range is scaled up to ±1).
pub fn perlin2(seed: u64, x: f64, z: f64) -> f64 {
    let x0 = x.floor();
    let z0 = z.floor();
    let xi = x0 as i64;
    let zi = z0 as i64;
    let fx = x - x0;
    let fz = z - z0;

    // Gradient · displacement at each of the four cell corners.
    let dot = |gx: i64, gz: i64, dx: f64, dz: f64| {
        let (gxv, gzv) = gradient(seed, gx, gz);
        gxv * dx + gzv * dz
    };
    let n00 = dot(xi, zi, fx, fz);
    let n10 = dot(xi + 1, zi, fx - 1.0, fz);
    let n01 = dot(xi, zi + 1, fx, fz - 1.0);
    let n11 = dot(xi + 1, zi + 1, fx - 1.0, fz - 1.0);

    let u = fade(fx);
    let v = fade(fz);
    let nx0 = lerp(n00, n10, u);
    let nx1 = lerp(n01, n11, u);
    (lerp(nx0, nx1, v) * std::f64::consts::SQRT_2).clamp(-1.0, 1.0)
}

/// Fractal Brownian motion: sum `octaves` of Perlin at geometrically
/// increasing frequency and decreasing amplitude. Amplitude-normalised, so
/// the result stays in `[-1, 1]`. `lacunarity` ≈ 2.0 and `gain` ≈ 0.5 give
/// natural rolling terrain.
pub fn fbm(seed: u64, x: f64, z: f64, octaves: u32, lacunarity: f64, gain: f64) -> f64 {
    let mut freq = 1.0;
    let mut amp = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..octaves.max(1) {
        // Decorrelate octaves with a per-octave seed offset so they don't
        // share lattice gradients (which would print a visible grid).
        let s = seed.wrapping_add((o as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        sum += amp * perlin2(s, x * freq, z * freq);
        norm += amp;
        freq *= lacunarity;
        amp *= gain;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

/// Ridged multifractal: `(1 - |perlin|)²` summed over octaves. Biased
/// toward sharp ridge lines — the classic mountain-range generator. Output
/// in `[0, 1]` (1 = ridge crest).
pub fn ridged(seed: u64, x: f64, z: f64, octaves: u32, lacunarity: f64, gain: f64) -> f64 {
    let mut freq = 1.0;
    let mut amp = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..octaves.max(1) {
        let s = seed.wrapping_add((o as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
        let r = 1.0 - perlin2(s, x * freq, z * freq).abs();
        sum += amp * r * r;
        norm += amp;
        freq *= lacunarity;
        amp *= gain;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perlin_is_deterministic_and_bounded() {
        for i in 0..2000 {
            let x = i as f64 * 0.137;
            let z = i as f64 * 0.091;
            let a = perlin2(42, x, z);
            let b = perlin2(42, x, z);
            assert_eq!(a.to_bits(), b.to_bits(), "perlin not bit-identical");
            assert!((-1.0..=1.0).contains(&a), "perlin {a} out of [-1,1]");
        }
    }

    #[test]
    fn perlin_is_zero_on_lattice_points() {
        // Perlin noise is exactly 0 at integer lattice points (displacement
        // is zero so every corner dot vanishes) — a good correctness anchor.
        for ix in -5..5 {
            for iz in -5..5 {
                let n = perlin2(7, ix as f64, iz as f64);
                assert!(n.abs() < 1e-12, "lattice point ({ix},{iz}) = {n}, expected 0");
            }
        }
    }

    #[test]
    fn fbm_bounded_and_seed_sensitive() {
        let a = fbm(1, 3.5, 7.25, 6, 2.0, 0.5);
        let b = fbm(2, 3.5, 7.25, 6, 2.0, 0.5);
        assert!((-1.0..=1.0).contains(&a));
        assert_ne!(a.to_bits(), b.to_bits(), "different seeds should diverge");
    }

    #[test]
    fn ridged_in_unit_range() {
        for i in 0..1000 {
            let r = ridged(99, i as f64 * 0.31, i as f64 * 0.17, 5, 2.0, 0.5);
            assert!((0.0..=1.0).contains(&r), "ridged {r} out of [0,1]");
        }
    }
}
