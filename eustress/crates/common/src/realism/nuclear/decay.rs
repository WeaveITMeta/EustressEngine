//! Radioactive decay — single decay, decay chains (Bateman), activity.
//!
//! Pure-math helpers (no Bevy). All quantities are in SI-adjacent units:
//! nuclei are dimensionless counts, decay constants are per second (unless a
//! per-year half-life is supplied), activity is in Becquerel (decays/second),
//! masses are in grams and molar masses in grams/mole.

use core::f32::consts::LN_2;

/// Avogadro's number — nuclei per mole.
const AVOGADRO: f32 = 6.022_140_76e23;

/// Half-life of carbon-14 in years (used by `carbon14_age`).
const C14_HALF_LIFE_YEARS: f32 = 5730.0;

/// Decay constant λ from a half-life: λ = ln(2) / t_half.
///
/// The unit of the returned constant is the reciprocal of the unit of
/// `half_life` (e.g. per second if `half_life` is in seconds).
pub fn decay_constant_from_half_life(half_life: f32) -> f32 {
    LN_2 / half_life
}

/// Half-life from a decay constant: t_half = ln(2) / λ.
pub fn half_life_from_decay_constant(lambda: f32) -> f32 {
    LN_2 / lambda
}

/// Remaining nuclei after `time`: N(t) = N0 · exp(−λ t).
pub fn remaining_nuclei(n0: f32, lambda: f32, time: f32) -> f32 {
    n0 * (-lambda * time).exp()
}

/// Activity A = λ N, in Becquerel when λ is per second.
pub fn activity(n: f32, lambda: f32) -> f32 {
    lambda * n
}

/// Activity from a sample mass: A = λ · (m / M) · N_A.
///
/// `mass_grams` is the sample mass, `molar_mass` its molar mass in g/mol.
pub fn activity_from_mass(mass_grams: f32, molar_mass: f32, lambda: f32) -> f32 {
    lambda * (mass_grams / molar_mass) * AVOGADRO
}

/// Fraction of the original nuclei that have decayed after `time`:
/// 1 − exp(−λ t).
pub fn decayed_fraction(lambda: f32, time: f32) -> f32 {
    1.0 - (-lambda * time).exp()
}

/// Mean lifetime τ = 1 / λ.
pub fn mean_lifetime(lambda: f32) -> f32 {
    1.0 / lambda
}

/// Two-step Bateman solution for a parent → daughter (→ stable) chain.
///
/// Returns `(N1, N2)` where:
/// - parent  N1 = N1_0 · exp(−λ1 t)
/// - daughter N2 = N1_0 · λ1 / (λ2 − λ1) · (exp(−λ1 t) − exp(−λ2 t))
///
/// When λ1 == λ2 the closed form is singular, so the limiting case
/// N2 = N1_0 · λ1 · t · exp(−λ1 t) is used instead.
pub fn bateman_two_step(n1_0: f32, lambda1: f32, lambda2: f32, time: f32) -> (f32, f32) {
    let n1 = n1_0 * (-lambda1 * time).exp();

    // Guard the (λ2 − λ1) denominator: fall back to the equal-rate limit when
    // the two decay constants are numerically indistinguishable.
    let denom = lambda2 - lambda1;
    let n2 = if denom.abs() <= f32::EPSILON {
        n1_0 * lambda1 * time * (-lambda1 * time).exp()
    } else {
        n1_0 * lambda1 / denom * ((-lambda1 * time).exp() - (-lambda2 * time).exp())
    };

    (n1, n2)
}

/// Daughter activity at secular equilibrium equals the parent activity.
///
/// Trivial pass-through provided for call-site clarity in chain models where
/// the daughter is much shorter-lived than the parent (λ2 ≫ λ1).
pub fn secular_equilibrium_activity(parent_activity: f32) -> f32 {
    parent_activity
}

/// Specific activity per gram of pure nuclide: a = λ · N_A / M.
///
/// `molar_mass` in g/mol; the result is in Becquerel per gram when λ is per
/// second.
pub fn specific_activity(lambda: f32, molar_mass: f32) -> f32 {
    lambda * AVOGADRO / molar_mass
}

/// Radiocarbon age in years from the surviving carbon-14 fraction.
///
/// t = −(1 / λ) · ln(fraction), with λ derived from the carbon-14 half-life of
/// 5730 years. `current_c14_fraction` is N(t)/N0 (1.0 for a living sample).
pub fn carbon14_age(current_c14_fraction: f32) -> f32 {
    let lambda = decay_constant_from_half_life(C14_HALF_LIFE_YEARS);
    -(1.0 / lambda) * current_c14_fraction.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_life_round_trip_and_half_remaining() {
        let half_life = 10.0_f32;
        let lambda = decay_constant_from_half_life(half_life);
        // λ and t_half are inverse transforms of each other.
        assert!((half_life_from_decay_constant(lambda) - half_life).abs() < 1e-3);
        // After exactly one half-life, half the nuclei remain.
        let remaining = remaining_nuclei(1000.0, lambda, half_life);
        assert!((remaining - 500.0).abs() < 0.5);
    }

    #[test]
    fn activity_and_decayed_fraction_consistency() {
        let lambda = decay_constant_from_half_life(5.0);
        // Activity is λ·N.
        assert!((activity(100.0, lambda) - lambda * 100.0).abs() < 1e-6);
        // After one half-life roughly half has decayed.
        let frac = decayed_fraction(lambda, 5.0);
        assert!((frac - 0.5).abs() < 1e-3);
    }

    #[test]
    fn bateman_equal_rate_limit_is_finite() {
        // Equal decay constants must use the finite limiting form, not divide
        // by zero.
        let (n1, n2) = bateman_two_step(1000.0, 0.1, 0.1, 5.0);
        assert!(n1.is_finite() && n2.is_finite());
        let expected_n2 = 1000.0 * 0.1 * 5.0 * (-0.1_f32 * 5.0).exp();
        assert!((n2 - expected_n2).abs() < 1e-2);
        // Daughter population starts at zero and is positive at t > 0.
        assert!(n2 > 0.0);
    }

    #[test]
    fn carbon14_living_sample_is_zero_age() {
        // A fraction of 1.0 means no decay has happened yet.
        assert!(carbon14_age(1.0).abs() < 1e-2);
        // Half the carbon-14 remaining ⇒ one half-life of age.
        assert!((carbon14_age(0.5) - C14_HALF_LIFE_YEARS).abs() < 1.0);
    }
}
