//! Deterministic randomness for the wave amplitudes.
//!
//! Each lattice mode draws its two standard normals from a counter-based
//! hash of (seed, cascade, kx index, kz index): a pure function of the key,
//! never a sequential stream, so the sea a seed describes does not depend on
//! loop order, thread count or which other modes exist. The indices are the
//! signed lattice frequencies (kx = 2 pi fx / L), not array positions, so a
//! mode keeps its amplitude when the resolution changes while its cascade
//! still owns it.

/// SplitMix64 (Steele, Lea and Flood 2014): a bijective mix of 64 bits in
/// which every input bit affects every output bit.
#[inline]
pub fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Domain tag ("OCEAN_H1"), so these draws never coincide with another
/// solver's SplitMix use of the same seed.
const OCEAN_TAG: u64 = 0x4F43_4541_4E5F_4831;

/// 64 random bits for one lane of one lattice mode.
#[inline]
pub fn mode_bits(seed: u64, cascade: u32, fx: i32, fz: i32, lane: u32) -> u64 {
    let mut h = splitmix64(seed ^ OCEAN_TAG);
    h = splitmix64(h ^ u64::from(cascade));
    h = splitmix64(h ^ (fx as i64 as u64));
    h = splitmix64(h ^ (fz as i64 as u64));
    splitmix64(h ^ u64::from(lane))
}

/// Uniform in (0, 1]: 53 random bits, offset by one so the logarithm in
/// Box-Muller stays finite.
#[inline]
fn unit_open(bits: u64) -> f64 {
    ((bits >> 11) + 1) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Two independent standard normals for one lattice mode: both outputs of
/// one Box-Muller transform, computed in f64 and rounded once.
#[inline]
pub fn mode_normals(seed: u64, cascade: u32, fx: i32, fz: i32) -> (f32, f32) {
    let u1 = unit_open(mode_bits(seed, cascade, fx, fz, 0));
    let u2 = unit_open(mode_bits(seed, cascade, fx, fz, 1));
    let r = (-2.0 * u1.ln()).sqrt();
    let (s, c) = (std::f64::consts::TAU * u2).sin_cos();
    ((r * c) as f32, (r * s) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_normals_are_pure_functions_of_the_key() {
        let a = mode_normals(7, 1, -3, 12);
        assert_eq!(a, mode_normals(7, 1, -3, 12));
        for other in [
            mode_normals(8, 1, -3, 12),
            mode_normals(7, 2, -3, 12),
            mode_normals(7, 1, 3, 12),
            mode_normals(7, 1, -3, 13),
            mode_normals(7, 1, 12, -3),
        ] {
            assert_ne!(a, other);
        }
    }

    /// 2 x 160 801 draws: the standard error of the mean and of the
    /// correlation is 1 / sqrt(n) = 0.0025 and of the variance
    /// sqrt(2 / n) = 0.0035, so the bounds below are about four standard
    /// errors.
    #[test]
    fn mode_normals_are_standard_and_independent() {
        let (mut n, mut sr, mut si, mut srr, mut sii, mut sri) = (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0);
        for fz in -200..=200 {
            for fx in -200..=200 {
                let (r, i) = mode_normals(3, 0, fx, fz);
                let (r, i) = (r as f64, i as f64);
                n += 1.0;
                sr += r;
                si += i;
                srr += r * r;
                sii += i * i;
                sri += r * i;
            }
        }
        let (mr, mi) = (sr / n, si / n);
        let (vr, vi) = (srr / n - mr * mr, sii / n - mi * mi);
        let corr = (sri / n - mr * mi) / (vr * vi).sqrt();
        assert!(mr.abs() < 0.01 && mi.abs() < 0.01, "means {mr} {mi}");
        assert!((vr - 1.0).abs() < 0.015 && (vi - 1.0).abs() < 0.015, "variances {vr} {vi}");
        assert!(corr.abs() < 0.01, "correlation {corr}");
    }
}
