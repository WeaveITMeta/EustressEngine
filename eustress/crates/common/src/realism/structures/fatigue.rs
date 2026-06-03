//! Fatigue and fracture — S-N curve, Miner's rule, Paris crack growth, stress intensity.
//!
//! Pure functions for high-cycle fatigue life and linear-elastic fracture
//! mechanics. SI units throughout:
//!   - stress / strength / modulus in pascals (Pa)
//!   - crack length in meters (m)
//!   - stress-intensity factor K in pascals * sqrt(meter) (Pa*m^0.5)
//!   - fracture toughness K_IC in pascals * sqrt(meter) (Pa*m^0.5)
//!   - cycle counts are dimensionless
//!
//! Paris-law constant `C` and exponent `m` are expressed in units consistent
//! with `da/dN` in m/cycle and `delta_K` in Pa*m^0.5.
//!
//! No Bevy/ECS dependencies.

use core::f32::consts::PI;

// ============================================================================
// S-N (stress-life) behavior
// ============================================================================

/// Endurance (fatigue) limit estimate for wrought steels:
///   Se = 0.5 * Sut for Sut < 1400 MPa, capped at 700 MPa above that.
///
/// Inputs and output are in pascals. The threshold is 1.4e9 Pa and the cap is
/// 7.0e8 Pa.
///
/// * `ultimate_strength` — ultimate tensile strength Sut (Pa)
#[inline]
pub fn endurance_limit_steel(ultimate_strength: f32) -> f32 {
    if ultimate_strength < 1.4e9 {
        0.5 * ultimate_strength
    } else {
        7.0e8
    }
}

/// Basquin stress-life relation solved for cycles to failure:
///   N = (sigma_a / sigma_f')^(1 / b).
///
/// Note the fatigue-strength exponent `b` is negative for real materials, so a
/// larger alternating stress yields fewer cycles.
///
/// * `sigma_a` — alternating stress amplitude (Pa)
/// * `sigma_f_prime` — fatigue strength coefficient sigma_f' (Pa)
/// * `b_exponent` — fatigue strength exponent b (negative, dimensionless)
#[inline]
pub fn basquin_stress_life(sigma_a: f32, sigma_f_prime: f32, b_exponent: f32) -> f32 {
    if sigma_f_prime <= 0.0 || sigma_a <= 0.0 || b_exponent == 0.0 {
        return f32::INFINITY;
    }
    (sigma_a / sigma_f_prime).powf(1.0 / b_exponent)
}

/// Goodman mean-stress failure index:
///   index = sigma_a / Se + sigma_m / Sut.
///
/// Failure (per the Goodman line) is predicted when the returned index >= 1.0.
///
/// * `sigma_a` — alternating stress amplitude (Pa)
/// * `sigma_m` — mean stress (Pa)
/// * `sut` — ultimate tensile strength (Pa)
/// * `se` — endurance limit (Pa)
#[inline]
pub fn goodman_factor(sigma_a: f32, sigma_m: f32, sut: f32, se: f32) -> f32 {
    let a = if se != 0.0 { sigma_a / se } else { f32::INFINITY };
    let b = if sut != 0.0 { sigma_m / sut } else { f32::INFINITY };
    a + b
}

/// Soderberg mean-stress failure index:
///   index = sigma_a / Se + sigma_m / Sy.
///
/// Failure (per the Soderberg line) is predicted when the index >= 1.0. This is
/// more conservative than Goodman because it uses yield strength.
///
/// * `sigma_a` — alternating stress amplitude (Pa)
/// * `sigma_m` — mean stress (Pa)
/// * `sy` — yield strength (Pa)
/// * `se` — endurance limit (Pa)
#[inline]
pub fn soderberg_factor(sigma_a: f32, sigma_m: f32, sy: f32, se: f32) -> f32 {
    let a = if se != 0.0 { sigma_a / se } else { f32::INFINITY };
    let b = if sy != 0.0 { sigma_m / sy } else { f32::INFINITY };
    a + b
}

/// Palmgren-Miner linear cumulative damage:
///   D = sum_i (n_i / N_i).
///
/// Failure is predicted when D >= 1.0. Entries with a non-positive life
/// `N_i` are skipped (treated as contributing no finite damage); the slices are
/// paired index-for-index up to the shorter length.
///
/// * `cycles` — applied cycle counts n_i at each stress level
/// * `life_cycles` — fatigue life N_i at each corresponding stress level
#[inline]
pub fn miners_rule(cycles: &[f32], life_cycles: &[f32]) -> f32 {
    let n = cycles.len().min(life_cycles.len());
    let mut damage = 0.0_f32;
    for k in 0..n {
        let life = life_cycles[k];
        if life > 0.0 {
            damage += cycles[k] / life;
        }
    }
    damage
}

// ============================================================================
// Linear-elastic fracture mechanics
// ============================================================================

/// Mode-I stress-intensity factor: K = Y * sigma * sqrt(PI * a).
///
/// * `sigma` — remote applied stress (Pa)
/// * `crack_length` — crack length a (m); for an edge crack this is the full
///   length, for a central crack it is the half-length
/// * `geometry_factor` — dimensionless geometry/shape factor Y
#[inline]
pub fn stress_intensity_factor(sigma: f32, crack_length: f32, geometry_factor: f32) -> f32 {
    if crack_length <= 0.0 {
        return 0.0;
    }
    geometry_factor * sigma * (PI * crack_length).sqrt()
}

/// Paris-law crack-growth rate: da/dN = C * (delta_K)^m.
///
/// * `c_const` — Paris coefficient C
/// * `m_exp` — Paris exponent m (dimensionless, typically 2-4)
/// * `delta_k` — stress-intensity range delta_K (Pa*m^0.5)
#[inline]
pub fn paris_crack_growth_rate(c_const: f32, m_exp: f32, delta_k: f32) -> f32 {
    if delta_k <= 0.0 {
        return 0.0;
    }
    c_const * delta_k.powf(m_exp)
}

/// Cycles to failure by integrating the Paris law from an initial to a critical
/// crack length, assuming a constant stress range and constant geometry factor.
///
/// With delta_K = Y * delta_sigma * sqrt(PI * a), the closed-form integral for
/// m != 2 is:
///   N = [ a_c^(1 - m/2) - a_i^(1 - m/2) ]
///       / [ C * (Y * delta_sigma * sqrt(PI))^m * (1 - m/2) ].
///
/// The m == 2 singular case integrates to a logarithm:
///   N = ln(a_c / a_i) / [ C * (Y * delta_sigma * sqrt(PI))^2 ].
///
/// * `c_const` — Paris coefficient C
/// * `m_exp` — Paris exponent m
/// * `delta_sigma` — stress range (Pa)
/// * `geometry_factor` — geometry factor Y
/// * `a_initial` — initial crack length (m)
/// * `a_critical` — critical crack length at failure (m)
#[inline]
pub fn cycles_to_failure_paris(
    c_const: f32,
    m_exp: f32,
    delta_sigma: f32,
    geometry_factor: f32,
    a_initial: f32,
    a_critical: f32,
) -> f32 {
    if a_initial <= 0.0 || a_critical <= a_initial {
        return 0.0;
    }
    // base = C * (Y * delta_sigma * sqrt(PI))^m   (the a-independent factor)
    let k_per_sqrt_a = geometry_factor * delta_sigma * PI.sqrt();
    if k_per_sqrt_a <= 0.0 || c_const <= 0.0 {
        return f32::INFINITY;
    }
    let base = c_const * k_per_sqrt_a.powf(m_exp);
    if base == 0.0 {
        return f32::INFINITY;
    }

    // Special case m == 2: integrand ~ 1/a -> natural log.
    if (m_exp - 2.0).abs() < 1e-6 {
        return (a_critical / a_initial).ln() / base;
    }

    let power = 1.0 - m_exp / 2.0;
    let numerator = a_critical.powf(power) - a_initial.powf(power);
    numerator / (base * power)
}

/// Fast-fracture criterion: true when the applied stress intensity reaches or
/// exceeds the material fracture toughness.
///
/// * `k` — applied stress-intensity factor (Pa*m^0.5)
/// * `k_ic` — plane-strain fracture toughness K_IC (Pa*m^0.5)
#[inline]
pub fn fracture_occurs(k: f32, k_ic: f32) -> bool {
    k >= k_ic
}

/// Critical crack length at which fast fracture occurs for a given stress:
///   a_c = (K_IC / (Y * sigma))^2 / PI.
///
/// * `k_ic` — fracture toughness (Pa*m^0.5)
/// * `sigma` — applied stress (Pa)
/// * `geometry_factor` — geometry factor Y
#[inline]
pub fn critical_crack_length(k_ic: f32, sigma: f32, geometry_factor: f32) -> f32 {
    let denom = geometry_factor * sigma;
    if denom == 0.0 {
        return f32::INFINITY;
    }
    let ratio = k_ic / denom;
    ratio * ratio / PI
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn endurance_limit_branches() {
        // Below threshold: 0.5 * Sut.
        let se = endurance_limit_steel(800e6);
        assert!(approx(se, 400e6, 1e-6), "got {se}");
        // At/above threshold: capped at 700 MPa.
        let se_high = endurance_limit_steel(2.0e9);
        assert!(approx(se_high, 7.0e8, 1e-6), "got {se_high}");
    }

    #[test]
    fn miners_rule_reaches_failure_at_one() {
        // 5000/10000 + 25000/50000 = 0.5 + 0.5 = 1.0
        let d = miners_rule(&[5000.0, 25000.0], &[10000.0, 50000.0]);
        assert!(approx(d, 1.0, 1e-6), "got {d}");
        // A zero-life entry is skipped, not divided by zero.
        let d2 = miners_rule(&[100.0, 100.0], &[0.0, 200.0]);
        assert!(approx(d2, 0.5, 1e-6), "got {d2}");
    }

    #[test]
    fn stress_intensity_and_critical_length_inverse() {
        // K = Y sigma sqrt(PI a); a_c = (K_IC/(Y sigma))^2 / PI.
        // If sigma is chosen so K == K_IC at a, then critical_crack_length
        // should return that same a.
        let y = 1.12;
        let sigma = 200e6;
        let a = 0.005;
        let k = stress_intensity_factor(sigma, a, y);
        // Treat that K as the toughness; recover a.
        let a_back = critical_crack_length(k, sigma, y);
        assert!(approx(a_back, a, 1e-4), "got {a_back}, expected {a}");
    }

    #[test]
    fn paris_growth_rate_monotonic_in_delta_k() {
        let low = paris_crack_growth_rate(1e-11, 3.0, 10.0e6);
        let high = paris_crack_growth_rate(1e-11, 3.0, 20.0e6);
        assert!(high > low);
        // Doubling delta_K with m = 3 multiplies the rate by 8.
        assert!(approx(high / low, 8.0, 1e-4), "ratio {}", high / low);
    }

    #[test]
    fn fracture_occurs_threshold() {
        assert!(fracture_occurs(50.0, 40.0));
        assert!(fracture_occurs(40.0, 40.0));
        assert!(!fracture_occurs(39.9, 40.0));
    }
}
