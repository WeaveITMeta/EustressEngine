//! Composite materials — rule of mixtures, Halpin-Tsai, Tsai-Hill failure.
//!
//! Pure micromechanics functions for two-phase (fiber + matrix) composites.
//! Volume fractions are dimensionless in [0, 1]; moduli, strengths, and
//! stresses are in pascals (Pa); densities in kilograms per cubic meter
//! (kg/m^3). Specific properties divide by density.
//!
//! No Bevy/ECS dependencies.

// ============================================================================
// Elastic moduli
// ============================================================================

/// Longitudinal (fiber-direction) modulus by the Voigt rule of mixtures:
///   E_1 = E_f * V_f + E_m * (1 - V_f).
///
/// * `e_fiber` — fiber modulus (Pa)
/// * `e_matrix` — matrix modulus (Pa)
/// * `fiber_volume_fraction` — V_f in [0, 1]
#[inline]
pub fn rule_of_mixtures_modulus(e_fiber: f32, e_matrix: f32, fiber_volume_fraction: f32) -> f32 {
    e_fiber * fiber_volume_fraction + e_matrix * (1.0 - fiber_volume_fraction)
}

/// Transverse modulus by the inverse (Reuss) rule of mixtures:
///   1 / E_2 = V_f / E_f + (1 - V_f) / E_m.
///
/// Returns 0.0 if either constituent modulus is non-positive (degenerate).
///
/// * `e_fiber` — fiber modulus (Pa)
/// * `e_matrix` — matrix modulus (Pa)
/// * `vf` — fiber volume fraction in [0, 1]
#[inline]
pub fn inverse_rule_of_mixtures_transverse(e_fiber: f32, e_matrix: f32, vf: f32) -> f32 {
    if e_fiber <= 0.0 || e_matrix <= 0.0 {
        return 0.0;
    }
    let compliance = vf / e_fiber + (1.0 - vf) / e_matrix;
    if compliance <= 0.0 {
        return 0.0;
    }
    1.0 / compliance
}

/// Halpin-Tsai semi-empirical modulus:
///   E = E_m * (1 + xi * eta * V_f) / (1 - eta * V_f),
///   eta = (E_f/E_m - 1) / (E_f/E_m + xi).
///
/// The reinforcing factor `xi` depends on geometry/loading (e.g. xi -> infinity
/// recovers the Voigt bound, xi -> 0 recovers the Reuss bound).
///
/// * `e_fiber` — fiber modulus (Pa)
/// * `e_matrix` — matrix modulus (Pa)
/// * `vf` — fiber volume fraction in [0, 1]
/// * `xi` — Halpin-Tsai reinforcing factor (dimensionless)
#[inline]
pub fn halpin_tsai_modulus(e_fiber: f32, e_matrix: f32, vf: f32, xi: f32) -> f32 {
    if e_matrix <= 0.0 {
        return 0.0;
    }
    let ratio = e_fiber / e_matrix;
    let eta_denom = ratio + xi;
    if eta_denom == 0.0 {
        return e_matrix;
    }
    let eta = (ratio - 1.0) / eta_denom;
    let denom = 1.0 - eta * vf;
    if denom == 0.0 {
        return 0.0;
    }
    e_matrix * (1.0 + xi * eta * vf) / denom
}

// ============================================================================
// Other rule-of-mixtures properties
// ============================================================================

/// Composite density by the rule of mixtures:
///   rho = rho_f * V_f + rho_m * (1 - V_f).
#[inline]
pub fn rule_of_mixtures_density(rho_fiber: f32, rho_matrix: f32, vf: f32) -> f32 {
    rho_fiber * vf + rho_matrix * (1.0 - vf)
}

/// Longitudinal composite strength by the rule of mixtures:
///   sigma = sigma_f * V_f + sigma_m * (1 - V_f).
///
/// (A first-order estimate; real strength is governed by the constituent that
/// fails first at its own strain.)
#[inline]
pub fn rule_of_mixtures_strength(sigma_fiber: f32, sigma_matrix: f32, vf: f32) -> f32 {
    sigma_fiber * vf + sigma_matrix * (1.0 - vf)
}

/// Major Poisson's ratio by the rule of mixtures:
///   nu_12 = nu_f * V_f + nu_m * (1 - V_f).
#[inline]
pub fn composite_poisson_ratio(nu_fiber: f32, nu_matrix: f32, vf: f32) -> f32 {
    nu_fiber * vf + nu_matrix * (1.0 - vf)
}

/// In-plane shear modulus by the inverse rule of mixtures:
///   1 / G_12 = V_f / G_f + (1 - V_f) / G_m.
///
/// Returns 0.0 for degenerate (non-positive) constituent moduli.
#[inline]
pub fn shear_modulus_composite(g_fiber: f32, g_matrix: f32, vf: f32) -> f32 {
    if g_fiber <= 0.0 || g_matrix <= 0.0 {
        return 0.0;
    }
    let compliance = vf / g_fiber + (1.0 - vf) / g_matrix;
    if compliance <= 0.0 {
        return 0.0;
    }
    1.0 / compliance
}

// ============================================================================
// Failure
// ============================================================================

/// Tsai-Hill failure index for an orthotropic lamina under plane stress:
///   index = (s1/X)^2 - s1*s2/X^2 + (s2/Y)^2 + (tau12/S)^2.
///
/// Failure is predicted when the returned index >= 1.0.
///
/// * `sigma1` — longitudinal stress (Pa)
/// * `sigma2` — transverse stress (Pa)
/// * `tau12` — in-plane shear stress (Pa)
/// * `x_strength` — longitudinal strength X (Pa)
/// * `y_strength` — transverse strength Y (Pa)
/// * `s_shear` — in-plane shear strength S (Pa)
#[inline]
pub fn tsai_hill_index(
    sigma1: f32,
    sigma2: f32,
    tau12: f32,
    x_strength: f32,
    y_strength: f32,
    s_shear: f32,
) -> f32 {
    if x_strength == 0.0 || y_strength == 0.0 || s_shear == 0.0 {
        return f32::INFINITY;
    }
    let x2 = x_strength * x_strength;
    let term1 = (sigma1 / x_strength) * (sigma1 / x_strength);
    let term_cross = sigma1 * sigma2 / x2;
    let term2 = (sigma2 / y_strength) * (sigma2 / y_strength);
    let term_shear = (tau12 / s_shear) * (tau12 / s_shear);
    term1 - term_cross + term2 + term_shear
}

// ============================================================================
// Specific (mass-normalized) properties
// ============================================================================

/// Specific modulus: E / rho (units: Pa / (kg/m^3) = m^2/s^2).
#[inline]
pub fn specific_modulus(e: f32, density: f32) -> f32 {
    if density <= 0.0 {
        return 0.0;
    }
    e / density
}

/// Specific strength: strength / rho (units: Pa / (kg/m^3) = m^2/s^2).
#[inline]
pub fn specific_strength(strength: f32, density: f32) -> f32 {
    if density <= 0.0 {
        return 0.0;
    }
    strength / density
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
    fn rule_of_mixtures_endpoints() {
        // V_f = 1 -> pure fiber; V_f = 0 -> pure matrix.
        assert!(approx(rule_of_mixtures_modulus(230e9, 3.5e9, 1.0), 230e9, 1e-5));
        assert!(approx(rule_of_mixtures_modulus(230e9, 3.5e9, 0.0), 3.5e9, 1e-5));
        // Midpoint is the simple average for V_f = 0.5.
        let mid = rule_of_mixtures_modulus(230e9, 3.5e9, 0.5);
        assert!(approx(mid, 0.5 * (230e9 + 3.5e9), 1e-5));
    }

    #[test]
    fn transverse_modulus_below_longitudinal() {
        // Reuss bound (transverse) must not exceed the Voigt bound (longitudinal).
        let vf = 0.6;
        let e_long = rule_of_mixtures_modulus(230e9, 3.5e9, vf);
        let e_trans = inverse_rule_of_mixtures_transverse(230e9, 3.5e9, vf);
        assert!(e_trans > 0.0);
        assert!(e_trans < e_long, "trans {e_trans} should be < long {e_long}");
    }

    #[test]
    fn tsai_hill_failure_at_pure_longitudinal_limit() {
        // Pure longitudinal stress at the strength X gives index = 1.
        let idx = tsai_hill_index(1500e6, 0.0, 0.0, 1500e6, 40e6, 70e6);
        assert!(approx(idx, 1.0, 1e-5), "got {idx}");
        // Below the limit -> safe (< 1).
        let idx_safe = tsai_hill_index(750e6, 0.0, 0.0, 1500e6, 40e6, 70e6);
        assert!(idx_safe < 1.0);
    }

    #[test]
    fn halpin_tsai_between_bounds() {
        // For a finite positive xi the Halpin-Tsai modulus lies between the
        // Reuss (transverse) and Voigt (longitudinal) bounds.
        let vf = 0.5;
        let voigt = rule_of_mixtures_modulus(230e9, 3.5e9, vf);
        let reuss = inverse_rule_of_mixtures_transverse(230e9, 3.5e9, vf);
        let ht = halpin_tsai_modulus(230e9, 3.5e9, vf, 2.0);
        assert!(ht >= reuss - 1.0 && ht <= voigt + 1.0, "ht={ht} reuss={reuss} voigt={voigt}");
    }
}
