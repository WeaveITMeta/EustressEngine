//! Euler-Bernoulli beam mechanics — bending, shear, deflection, vibration.
//!
//! Pure functions for static beam analysis. All inputs/outputs are SI base
//! units unless otherwise noted:
//!   - lengths in meters (m)
//!   - forces in newtons (N)
//!   - moments in newton-meters (N·m)
//!   - distributed load `w` in newtons per meter (N/m)
//!   - Young's modulus `e` in pascals (Pa)
//!   - second moment of area `i` in meters^4 (m^4)
//!   - stress in pascals (Pa)
//!
//! No Bevy/ECS dependencies — this module is engine-agnostic.

use core::f32::consts::PI;

// ============================================================================
// Stress
// ============================================================================

/// Bending (flexural) stress: sigma = M * y / I.
///
/// * `moment` — bending moment at the section (N·m)
/// * `y` — distance from the neutral axis to the fiber of interest (m)
/// * `i_moment` — second moment of area about the neutral axis (m^4)
#[inline]
pub fn bending_stress(moment: f32, y: f32, i_moment: f32) -> f32 {
    if i_moment == 0.0 {
        return 0.0;
    }
    moment * y / i_moment
}

/// Transverse shear stress: tau = V * Q / (I * b).
///
/// * `shear` — transverse shear force at the section (N)
/// * `q_first_moment` — first moment of area above the point (m^3)
/// * `i_moment` — second moment of area of the full section (m^4)
/// * `width` — section width at the point of interest (m)
#[inline]
pub fn shear_stress(shear: f32, q_first_moment: f32, i_moment: f32, width: f32) -> f32 {
    let denom = i_moment * width;
    if denom == 0.0 {
        return 0.0;
    }
    shear * q_first_moment / denom
}

// ============================================================================
// Deflection
// ============================================================================

/// Deflection of a cantilever with an end point load, at distance `x` from the
/// fixed support: y = P * x^2 * (3L - x) / (6 * E * I).
///
/// * `p` — end load (N)
/// * `length` — beam length L (m)
/// * `e` — Young's modulus (Pa)
/// * `i` — second moment of area (m^4)
/// * `x` — distance from the fixed end (m), 0 <= x <= L
#[inline]
pub fn beam_deflection_cantilever_end_load(p: f32, length: f32, e: f32, i: f32, x: f32) -> f32 {
    let denom = 6.0 * e * i;
    if denom == 0.0 {
        return 0.0;
    }
    p * x * x * (3.0 * length - x) / denom
}

/// Maximum deflection of a cantilever with an end point load (at the free end):
/// y_max = P * L^3 / (3 * E * I).
#[inline]
pub fn beam_deflection_cantilever_max(p: f32, length: f32, e: f32, i: f32) -> f32 {
    let denom = 3.0 * e * i;
    if denom == 0.0 {
        return 0.0;
    }
    p * length * length * length / denom
}

/// Maximum deflection of a simply supported beam with a central point load:
/// y_max = P * L^3 / (48 * E * I).
#[inline]
pub fn beam_deflection_simply_supported_center(p: f32, length: f32, e: f32, i: f32) -> f32 {
    let denom = 48.0 * e * i;
    if denom == 0.0 {
        return 0.0;
    }
    p * length * length * length / denom
}

/// Maximum deflection of a simply supported beam under a uniformly distributed
/// load (UDL): y_max = 5 * w * L^4 / (384 * E * I).
///
/// * `w` — distributed load intensity (N/m)
#[inline]
pub fn beam_deflection_udl_simply_supported(w: f32, length: f32, e: f32, i: f32) -> f32 {
    let denom = 384.0 * e * i;
    if denom == 0.0 {
        return 0.0;
    }
    let l2 = length * length;
    5.0 * w * l2 * l2 / denom
}

// ============================================================================
// Maximum bending moments
// ============================================================================

/// Maximum bending moment of a cantilever with an end point load: M = P * L.
/// Occurs at the fixed support.
#[inline]
pub fn max_moment_cantilever_end(p: f32, length: f32) -> f32 {
    p * length
}

/// Maximum bending moment of a simply supported beam with a central point load:
/// M = P * L / 4. Occurs at midspan.
#[inline]
pub fn max_moment_simply_supported_center(p: f32, length: f32) -> f32 {
    p * length / 4.0
}

/// Maximum bending moment of a simply supported beam under a UDL:
/// M = w * L^2 / 8. Occurs at midspan.
#[inline]
pub fn max_moment_udl(w: f32, length: f32) -> f32 {
    w * length * length / 8.0
}

// ============================================================================
// Section properties
// ============================================================================

/// Section modulus: Z = I / c, where c is the extreme-fiber distance.
///
/// * `i_moment` — second moment of area (m^4)
/// * `c_max` — distance from neutral axis to outermost fiber (m)
#[inline]
pub fn section_modulus(i_moment: f32, c_max: f32) -> f32 {
    if c_max == 0.0 {
        return 0.0;
    }
    i_moment / c_max
}

/// Second moment of area of a solid rectangle about its centroidal axis
/// (bending about the axis parallel to base `b`): I = b * h^3 / 12.
///
/// * `b` — width (m)
/// * `h` — height in the bending direction (m)
#[inline]
pub fn moment_of_area_rectangle(b: f32, h: f32) -> f32 {
    b * h * h * h / 12.0
}

/// Second moment of area of a solid circle: I = PI * r^4 / 4.
#[inline]
pub fn moment_of_area_circle(r: f32) -> f32 {
    PI * r * r * r * r / 4.0
}

/// Second moment of area of a hollow circle (annulus):
/// I = PI * (r_outer^4 - r_inner^4) / 4.
#[inline]
pub fn moment_of_area_hollow_circle(r_outer: f32, r_inner: f32) -> f32 {
    let ro4 = r_outer * r_outer * r_outer * r_outer;
    let ri4 = r_inner * r_inner * r_inner * r_inner;
    PI * (ro4 - ri4) / 4.0
}

/// Second moment of area of a symmetric I-beam about its strong (horizontal)
/// centroidal axis.
///
/// The section is treated as a full bounding rectangle (`b` x `h`) with the two
/// rectangular voids beside the web removed:
///   I = (b * h^3) / 12 - ((b - t_web) * h_web^3) / 12,
/// where h_web = h - 2 * t_flange is the clear height between flanges.
///
/// * `b` — overall flange width (m)
/// * `h` — overall section height (m)
/// * `t_web` — web thickness (m)
/// * `t_flange` — single flange thickness (m)
#[inline]
pub fn moment_of_area_i_beam(b: f32, h: f32, t_web: f32, t_flange: f32) -> f32 {
    let h_web = h - 2.0 * t_flange;
    let outer = b * h * h * h / 12.0;
    // Material removed is the region of full height minus the web width,
    // but only over the clear web height (the flanges span the full width).
    let void_width = b - t_web;
    let void = if h_web > 0.0 && void_width > 0.0 {
        void_width * h_web * h_web * h_web / 12.0
    } else {
        0.0
    };
    outer - void
}

// ============================================================================
// Natural frequencies (transverse vibration, Euler-Bernoulli)
// ============================================================================

/// First-mode natural frequency of a cantilever beam (Hz):
/// f = (1.875^2) / (2 * PI * L^2) * sqrt(E * I / rho_lin).
///
/// Here `rho_lin` is the mass per unit length (kg/m), i.e. rho * A.
///
/// * `e` — Young's modulus (Pa)
/// * `i` — second moment of area (m^4)
/// * `rho_lin` — mass per unit length (kg/m)
/// * `length` — beam length L (m)
#[inline]
pub fn natural_frequency_cantilever(e: f32, i: f32, rho_lin: f32, length: f32) -> f32 {
    if rho_lin <= 0.0 || length == 0.0 {
        return 0.0;
    }
    // First eigenvalue of a clamped-free beam: beta_1 * L = 1.875104...
    let beta1 = 1.875_104_1_f32;
    let coeff = (beta1 * beta1) / (2.0 * PI * length * length);
    coeff * (e * i / rho_lin).sqrt()
}

/// First-mode natural frequency of a simply supported beam (Hz):
/// f = (PI / 2) / L^2 * sqrt(E * I / rho_lin).
///
/// `rho_lin` is the mass per unit length (kg/m).
#[inline]
pub fn natural_frequency_simply_supported(e: f32, i: f32, rho_lin: f32, length: f32) -> f32 {
    if rho_lin <= 0.0 || length == 0.0 {
        return 0.0;
    }
    let coeff = (PI / 2.0) / (length * length);
    coeff * (e * i / rho_lin).sqrt()
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
    fn rectangle_second_moment_known_value() {
        // b = 2, h = 3 -> I = 2 * 27 / 12 = 4.5
        let i = moment_of_area_rectangle(2.0, 3.0);
        assert!(approx(i, 4.5, 1e-6), "got {i}");
    }

    #[test]
    fn circle_second_moment_known_value() {
        // r = 2 -> I = PI * 16 / 4 = 4 * PI
        let i = moment_of_area_circle(2.0);
        assert!(approx(i, 4.0 * PI, 1e-6), "got {i}");
    }

    #[test]
    fn cantilever_max_deflection_matches_formula() {
        // P = 1000 N, L = 2 m, E = 200e9 Pa, I = 1e-6 m^4
        // y = P L^3 / (3 E I) = 1000 * 8 / (3 * 200e9 * 1e-6)
        //   = 8000 / 600000 = 0.013333... m
        let y = beam_deflection_cantilever_max(1000.0, 2.0, 200e9, 1e-6);
        assert!(approx(y, 8000.0 / 600_000.0, 1e-5), "got {y}");
    }

    #[test]
    fn bending_stress_and_section_modulus_consistent() {
        // sigma = M y / I, and Z = I / c => sigma_max = M / Z.
        let i = moment_of_area_rectangle(0.1, 0.2); // 0.1*0.008/12
        let c = 0.1; // half of h = 0.2
        let m = 500.0;
        let sigma_direct = bending_stress(m, c, i);
        let z = section_modulus(i, c);
        let sigma_via_z = m / z;
        assert!(approx(sigma_direct, sigma_via_z, 1e-5));
    }

    #[test]
    fn udl_moment_known_value() {
        // w = 100 N/m, L = 4 m -> M = 100 * 16 / 8 = 200 N·m
        let m = max_moment_udl(100.0, 4.0);
        assert!(approx(m, 200.0, 1e-6), "got {m}");
    }
}
