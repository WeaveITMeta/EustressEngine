//! Column buckling — Euler critical load, slenderness, Johnson parabola.
//!
//! Pure functions for axial-compression stability analysis of slender members.
//! SI units throughout:
//!   - lengths in meters (m)
//!   - areas in meters^2 (m^2)
//!   - second moment of area in meters^4 (m^4)
//!   - modulus / stress / strength in pascals (Pa)
//!   - loads in newtons (N)
//!
//! Slenderness ratio `lambda = K * L / r` is dimensionless.
//!
//! No Bevy/ECS dependencies.

use core::f32::consts::PI;

// ============================================================================
// Euler buckling
// ============================================================================

/// Euler critical (buckling) load: P_cr = PI^2 * E * I / (K * L)^2.
///
/// * `e` — Young's modulus (Pa)
/// * `i` — least second moment of area of the cross-section (m^4)
/// * `length` — unbraced column length L (m)
/// * `k_factor` — effective-length factor K (see [`end_condition_k`])
#[inline]
pub fn euler_critical_load(e: f32, i: f32, length: f32, k_factor: f32) -> f32 {
    let kl = k_factor * length;
    let denom = kl * kl;
    if denom == 0.0 {
        return 0.0;
    }
    PI * PI * e * i / denom
}

/// Euler critical stress for a given slenderness ratio:
/// sigma_cr = PI^2 * E / lambda^2.
///
/// * `e` — Young's modulus (Pa)
/// * `slenderness` — slenderness ratio lambda = K * L / r (dimensionless)
#[inline]
pub fn euler_critical_stress(e: f32, slenderness: f32) -> f32 {
    if slenderness == 0.0 {
        return 0.0;
    }
    PI * PI * e / (slenderness * slenderness)
}

// ============================================================================
// Slenderness
// ============================================================================

/// Slenderness ratio: lambda = K * L / r.
///
/// * `k_factor` — effective-length factor K
/// * `length` — column length L (m)
/// * `radius_gyration` — radius of gyration r (m)
#[inline]
pub fn slenderness_ratio(k_factor: f32, length: f32, radius_gyration: f32) -> f32 {
    if radius_gyration == 0.0 {
        return 0.0;
    }
    k_factor * length / radius_gyration
}

/// Radius of gyration of a cross-section: r = sqrt(I / A).
///
/// * `i_moment` — second moment of area (m^4)
/// * `area` — cross-sectional area (m^2)
#[inline]
pub fn radius_of_gyration(i_moment: f32, area: f32) -> f32 {
    if area <= 0.0 {
        return 0.0;
    }
    (i_moment / area).sqrt()
}

/// Critical (transition) slenderness ratio separating the Johnson and Euler
/// regimes: lambda_c = sqrt(2 * PI^2 * E / Sy).
///
/// For lambda > lambda_c the long-column (Euler) curve governs; for
/// lambda <= lambda_c the intermediate-column (Johnson parabola) curve governs.
///
/// * `e` — Young's modulus (Pa)
/// * `yield_strength` — material yield strength Sy (Pa)
#[inline]
pub fn critical_slenderness(e: f32, yield_strength: f32) -> f32 {
    if yield_strength <= 0.0 {
        return 0.0;
    }
    (2.0 * PI * PI * e / yield_strength).sqrt()
}

// ============================================================================
// Johnson parabola (intermediate columns)
// ============================================================================

/// J.B. Johnson parabolic buckling stress (intermediate columns):
/// sigma_cr = Sy - (Sy^2 / (4 * PI^2 * E)) * lambda^2.
///
/// Valid for lambda <= lambda_c; result is clamped at zero.
///
/// * `yield_strength` — Sy (Pa)
/// * `e` — Young's modulus (Pa)
/// * `slenderness` — lambda = K * L / r
#[inline]
pub fn johnson_buckling_stress(yield_strength: f32, e: f32, slenderness: f32) -> f32 {
    if e <= 0.0 {
        return yield_strength.max(0.0);
    }
    let term = (yield_strength * yield_strength) / (4.0 * PI * PI * e) * slenderness * slenderness;
    (yield_strength - term).max(0.0)
}

/// Column critical stress selecting the appropriate curve automatically.
///
/// Returns the Euler critical stress when lambda is beyond the critical
/// slenderness, otherwise the Johnson parabola stress.
///
/// * `e` — Young's modulus (Pa)
/// * `yield_strength` — Sy (Pa)
/// * `slenderness` — lambda = K * L / r
#[inline]
pub fn column_strength(e: f32, yield_strength: f32, slenderness: f32) -> f32 {
    let lambda_c = critical_slenderness(e, yield_strength);
    if slenderness > lambda_c {
        euler_critical_stress(e, slenderness)
    } else {
        johnson_buckling_stress(yield_strength, e, slenderness)
    }
}

// ============================================================================
// Safety factor and end conditions
// ============================================================================

/// Buckling safety factor: n = P_critical / P_applied.
///
/// Returns `f32::INFINITY` for a non-positive applied load.
#[inline]
pub fn buckling_safety_factor(p_critical: f32, p_applied: f32) -> f32 {
    if p_applied <= 0.0 {
        return f32::INFINITY;
    }
    p_critical / p_applied
}

/// Effective-length factor K for common idealized end conditions.
///
/// Recognized (case-sensitive) keys:
///   - "pinned-pinned" => 1.0
///   - "fixed-free"    => 2.0
///   - "fixed-fixed"   => 0.5
///   - "fixed-pinned"  => 0.699
///
/// Any unrecognized condition returns the conservative default of 1.0.
#[inline]
pub fn end_condition_k(condition: &str) -> f32 {
    match condition {
        "pinned-pinned" => 1.0,
        "fixed-free" => 2.0,
        "fixed-fixed" => 0.5,
        "fixed-pinned" => 0.699,
        _ => 1.0,
    }
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
    fn euler_load_pinned_known_value() {
        // E = 200e9, I = 1e-6, L = 3, K = 1 (pinned-pinned)
        // P_cr = PI^2 * 200e9 * 1e-6 / 9
        let expected = PI * PI * 200e9 * 1e-6 / 9.0;
        let p = euler_critical_load(200e9, 1e-6, 3.0, end_condition_k("pinned-pinned"));
        assert!(approx(p, expected, 1e-5), "got {p}, expected {expected}");
    }

    #[test]
    fn radius_of_gyration_and_slenderness() {
        // I = 1e-6, A = 1e-3 -> r = sqrt(1e-3) ~ 0.031623
        let r = radius_of_gyration(1e-6, 1e-3);
        assert!(approx(r, (1e-3_f32).sqrt(), 1e-6));
        // lambda = K L / r with K = 1, L = 3
        let lambda = slenderness_ratio(1.0, 3.0, r);
        assert!(approx(lambda, 3.0 / r, 1e-5));
    }

    #[test]
    fn column_strength_picks_correct_branch() {
        let e = 200e9;
        let sy = 250e6;
        let lambda_c = critical_slenderness(e, sy);

        // Just below the transition -> Johnson value (>= 0, < Sy at lambda>0).
        let short = column_strength(e, sy, lambda_c * 0.5);
        let johnson = johnson_buckling_stress(sy, e, lambda_c * 0.5);
        assert!(approx(short, johnson, 1e-5));
        assert!(short <= sy);

        // Well above the transition -> Euler value.
        let long = column_strength(e, sy, lambda_c * 2.0);
        let euler = euler_critical_stress(e, lambda_c * 2.0);
        assert!(approx(long, euler, 1e-5));

        // At the transition the two curves should be (nearly) equal.
        let j_at = johnson_buckling_stress(sy, e, lambda_c);
        let eu_at = euler_critical_stress(e, lambda_c);
        assert!(approx(j_at, eu_at, 1e-3), "j={j_at}, eu={eu_at}");
    }
}
