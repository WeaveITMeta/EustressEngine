//! Reactor criticality — four-factor formula, buckling, migration area.
//!
//! Pure-math helpers (no Bevy). Areas (diffusion area L², slowing-down area τ,
//! migration area M²) are in cm²; bucklings B² are in cm⁻²; reactivity is
//! dimensionless (Δk/k); reactor periods are in seconds.

use core::f32::consts::PI;

/// First root of the J0 Bessel function — radial buckling factor for a finite
/// cylinder.
const BESSEL_J0_FIRST_ROOT: f32 = 2.405;

/// Infinite-multiplication factor from the four-factor formula:
/// k∞ = η · ε · p · f.
///
/// `eta` reproduction factor, `epsilon` fast-fission factor, `p_resonance`
/// resonance escape probability, `f_thermal` thermal utilisation.
pub fn four_factor_k_infinity(eta: f32, epsilon: f32, p_resonance: f32, f_thermal: f32) -> f32 {
    eta * epsilon * p_resonance * f_thermal
}

/// Effective multiplication factor from the six-factor formula:
/// k_eff = k∞ · P_FNL · P_TNL.
///
/// `fast_nonleak` is the fast non-leakage probability, `thermal_nonleak` the
/// thermal non-leakage probability.
pub fn six_factor_k_effective(k_infinity: f32, fast_nonleak: f32, thermal_nonleak: f32) -> f32 {
    k_infinity * fast_nonleak * thermal_nonleak
}

/// Reactivity ρ = (k − 1) / k.
pub fn reactivity(k_effective: f32) -> f32 {
    (k_effective - 1.0) / k_effective
}

/// Reactivity expressed in dollars: ρ($) = ρ / β.
pub fn reactivity_dollars(reactivity: f32, beta: f32) -> f32 {
    reactivity / beta
}

/// Migration area M² = L² + τ.
pub fn migration_area(diffusion_area: f32, slowing_down_area: f32) -> f32 {
    diffusion_area + slowing_down_area
}

/// Geometric buckling of a bare sphere: B² = (π / R)².
pub fn geometric_buckling_sphere(radius: f32) -> f32 {
    let term = PI / radius;
    term * term
}

/// Geometric buckling of a bare finite cylinder:
/// B² = (2.405 / R)² + (π / H)².
pub fn geometric_buckling_cylinder(radius: f32, height: f32) -> f32 {
    let radial = BESSEL_J0_FIRST_ROOT / radius;
    let axial = PI / height;
    radial * radial + axial * axial
}

/// Geometric buckling of a bare cube of side `side`: B² = 3 · (π / a)².
pub fn geometric_buckling_cube(side: f32) -> f32 {
    let term = PI / side;
    3.0 * term * term
}

/// Critical radius of a bare sphere for a given material buckling:
/// R = π / sqrt(B²).
pub fn critical_radius_sphere(material_buckling: f32) -> f32 {
    PI / material_buckling.sqrt()
}

/// Thermal non-leakage probability: P_TNL = 1 / (1 + L² B²).
pub fn thermal_nonleakage(diffusion_area: f32, buckling: f32) -> f32 {
    1.0 / (1.0 + diffusion_area * buckling)
}

/// Fast non-leakage probability: P_FNL = exp(−τ B²).
pub fn fast_nonleakage(slowing_down_area: f32, buckling: f32) -> f32 {
    (-slowing_down_area * buckling).exp()
}

/// Stable reactor doubling time from the reactor period:
/// T_double = T_period · ln(2).
pub fn doubling_time(reactor_period: f32) -> f32 {
    reactor_period * core::f32::consts::LN_2
}

/// Simplified stable reactor period from a reactivity insertion (prompt-jump
/// approximation, valid for ρ below the delayed-neutron fraction):
/// T = (β − ρ) / (λ · ρ).
///
/// `reactivity` ρ (Δk/k), `beta` delayed-neutron fraction, `neutron_lifetime`
/// the prompt-neutron lifetime (retained for signature compatibility with the
/// full one-group form), `decay_constant` the effective precursor decay
/// constant λ. Very small ρ is guarded to avoid a divide-by-zero; the
/// resulting near-infinite period reflects a reactor that is essentially
/// critical.
pub fn reactor_period(
    reactivity: f32,
    beta: f32,
    neutron_lifetime: f32,
    decay_constant: f32,
) -> f32 {
    // `neutron_lifetime` is part of the documented signature but does not enter
    // the prompt-jump approximation; bind it so the contract is explicit.
    let _ = neutron_lifetime;

    // Guard tiny reactivity: an effectively critical reactor has an unbounded
    // period. Return a very large finite value rather than dividing by zero.
    if reactivity.abs() <= f32::EPSILON {
        return f32::MAX;
    }
    (beta - reactivity) / (decay_constant * reactivity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_four_factor_is_one_and_critical_has_zero_reactivity() {
        // All four factors at unity give k∞ = 1.
        assert!((four_factor_k_infinity(1.0, 1.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
        // A just-critical reactor (k = 1) has zero reactivity.
        assert!(reactivity(1.0).abs() < 1e-6);
    }

    #[test]
    fn buckling_and_critical_radius_round_trip() {
        let radius = 10.0_f32;
        let b2 = geometric_buckling_sphere(radius);
        // Recovering the radius from its own geometric buckling returns it.
        assert!((critical_radius_sphere(b2) - radius).abs() < 1e-3);
        // Cube buckling has the expected 3×(π/a)² form.
        let side = 8.0_f32;
        let expected = 3.0 * (PI / side) * (PI / side);
        assert!((geometric_buckling_cube(side) - expected).abs() < 1e-4);
    }

    #[test]
    fn nonleakage_bounds_and_period_guard() {
        // Non-leakage probabilities are between 0 and 1 for positive areas.
        let p_tnl = thermal_nonleakage(50.0, 1e-3);
        let p_fnl = fast_nonleakage(40.0, 1e-3);
        assert!(p_tnl > 0.0 && p_tnl < 1.0);
        assert!(p_fnl > 0.0 && p_fnl < 1.0);
        // Near-zero reactivity must not divide by zero.
        assert_eq!(reactor_period(0.0, 0.0065, 2.5e-5, 0.08), f32::MAX);
        // Reactivity below beta yields a positive, finite period.
        let t = reactor_period(0.001, 0.0065, 2.5e-5, 0.08);
        assert!(t.is_finite() && t > 0.0);
    }
}
