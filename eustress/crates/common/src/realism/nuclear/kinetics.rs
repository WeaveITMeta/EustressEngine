//! # Reactor kinetics
//!
//! The one-delayed-group point kinetics equations and the laws that feed them:
//! reactivity from temperature and from control rods, the prompt jump, and
//! decay heat after shutdown.
//!
//! ```text
//! dn/dt = ((ρ − β) / Λ) · n + λ · C
//! dC/dt = (β / Λ) · n − λ · C
//! ```
//!
//! - `n`: neutron population, normalised so 1.0 is the reference power
//! - `C`: delayed-neutron precursor concentration
//! - `ρ`: reactivity (Δk/k)
//! - `β`: effective delayed-neutron fraction (about 0.0065 for U-235)
//! - `Λ`: prompt-neutron generation time (s)
//! - `λ`: one-group precursor decay constant (1/s; about 0.08 for U-235)
//!
//! The equations are stiff. Below prompt critical the fast eigenvalue is about
//! (ρ − β)/Λ, near −260 1/s for β = 0.0065 and Λ = 25 µs, so forward Euler
//! diverges at any step longer than 2Λ/(β − ρ), about 7.7 ms there.
//! [`step_implicit`] solves the backward-Euler step in closed form and stays
//! stable at any frame length.

/// Coefficient of the Way–Wigner decay-heat correlation.
pub const WAY_WIGNER_COEFFICIENT: f32 = 0.0622;

/// Most pieces [`step_implicit`] splits one step into above prompt critical.
const MAX_PIECES: u32 = 4096;

/// Precursor concentration in equilibrium with population `n`:
/// C = β·n / (Λ·λ). A core started here holds steady at ρ = 0.
pub fn equilibrium_precursors(n: f32, beta: f32, generation_time: f32, decay_constant: f32) -> f32 {
    let denom = generation_time * decay_constant;
    if denom <= 0.0 {
        return 0.0;
    }
    beta * n / denom
}

/// dn/dt = ((ρ − β)/Λ)·n + λ·C.
pub fn population_rate(
    n: f32,
    precursors: f32,
    reactivity: f32,
    beta: f32,
    generation_time: f32,
    decay_constant: f32,
) -> f32 {
    if generation_time <= 0.0 {
        return 0.0;
    }
    (reactivity - beta) / generation_time * n + decay_constant * precursors
}

/// dC/dt = (β/Λ)·n − λ·C.
pub fn precursor_rate(
    n: f32,
    precursors: f32,
    beta: f32,
    generation_time: f32,
    decay_constant: f32,
) -> f32 {
    if generation_time <= 0.0 {
        return 0.0;
    }
    beta / generation_time * n - decay_constant * precursors
}

/// Advance the point kinetics equations by `dt` seconds with backward Euler,
/// solved in closed form. Returns `(n, C)`.
///
/// With a = (ρ − β)/Λ, b = β/Λ and s = 1 + λ·dt:
///
/// ```text
/// n₁ = (n₀ + dt·λ·C₀/s) / (1 − dt·a − dt²·λ·b/s)
/// C₁ = (C₀ + dt·b·n₁) / s
/// ```
///
/// Below prompt critical (ρ < β) one step is stable at any `dt`. Above it the
/// population grows by more than a factor of e per long step, so the step is
/// split into pieces with dt·a ≤ 0.5 each, which keeps both values finite and
/// non-negative. A result too large for f32 saturates at `f32::MAX`.
pub fn step_implicit(
    n: f32,
    precursors: f32,
    reactivity: f32,
    beta: f32,
    generation_time: f32,
    decay_constant: f32,
    dt: f32,
) -> (f32, f32) {
    if dt <= 0.0 || generation_time <= 0.0 {
        return (n, precursors);
    }
    let a = (reactivity - beta) / generation_time;
    let b = beta / generation_time;
    let pieces = if a * dt > 0.5 {
        ((a * dt / 0.5).ceil() as u32).clamp(1, MAX_PIECES)
    } else {
        1
    };
    let h = dt / pieces as f32;
    let s = 1.0 + decay_constant * h;
    let denom = 1.0 - h * a - h * h * decay_constant * b / s;
    if denom <= 0.0 {
        return (f32::MAX, f32::MAX);
    }
    let (mut n1, mut c1) = (n, precursors);
    for _ in 0..pieces {
        n1 = (n1 + h * decay_constant * c1 / s) / denom;
        c1 = (c1 + h * b * n1) / s;
    }
    (n1.clamp(0.0, f32::MAX), c1.clamp(0.0, f32::MAX))
}

/// Reactivity from a linear temperature coefficient: ρ = α·(T − T_ref).
/// A negative α (Doppler broadening in the fuel, moderator expansion) makes a
/// core self-limiting: heating it removes reactivity.
pub fn temperature_feedback(coefficient: f32, temperature: f32, reference_temperature: f32) -> f32 {
    coefficient * (temperature - reference_temperature)
}

/// Reactivity of a control-rod bank with a linear worth curve:
/// ρ = W·(x_ref − x), where x is the fractional insertion (0 withdrawn,
/// 1 fully inserted), x_ref the insertion at which the bank adds no
/// reactivity, and W the bank's total worth (Δk/k).
pub fn rod_worth_linear(total_worth: f32, insertion: f32, reference_insertion: f32) -> f32 {
    total_worth * (reference_insertion - insertion)
}

/// Fraction of a rod bank's total worth inserted at fractional insertion x,
/// for a bank in a sine-shaped axial flux (the integral rod-worth S-curve):
/// f(x) = x − sin(2πx)/(2π). Travel near either end is worth little; travel
/// through the middle of the core is worth the most.
pub fn rod_worth_s_curve(insertion: f32) -> f32 {
    let x = insertion.clamp(0.0, 1.0);
    x - (std::f32::consts::TAU * x).sin() / std::f32::consts::TAU
}

/// Ratio n₁/n₀ right after a reactivity step, before the precursors respond
/// (the prompt jump): n₁/n₀ = β / (β − ρ). Valid below prompt critical;
/// infinite at or above ρ = β.
pub fn prompt_jump_ratio(reactivity: f32, beta: f32) -> f32 {
    let margin = beta - reactivity;
    if margin <= 0.0 {
        return f32::INFINITY;
    }
    beta / margin
}

/// Decay heat as a fraction of the power before shutdown, by the Way–Wigner
/// correlation: P/P₀ = 0.0622·(t^−0.2 − (t + T)^−0.2), where t is the time
/// since shutdown and T the time the core ran before it, both in seconds.
/// A rough fit, fair from about 10 s to 100 days after shutdown; times under
/// 1 s count as 1 s. For a core that ran for years, pass a large T (1e9 s).
pub fn decay_heat_fraction(time_since_shutdown: f32, operating_time: f32) -> f32 {
    let t = time_since_shutdown.max(1.0);
    let t_op = operating_time.max(0.0);
    (WAY_WIGNER_COEFFICIENT * (t.powf(-0.2) - (t + t_op).powf(-0.2))).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BETA: f32 = 0.0065;
    const GEN: f32 = 2.5e-5;
    const LAMBDA: f32 = 0.08;

    fn run(n: f32, c: f32, rho: f32, dt: f32, steps: u32) -> (f32, f32) {
        let (mut n, mut c) = (n, c);
        for _ in 0..steps {
            (n, c) = step_implicit(n, c, rho, BETA, GEN, LAMBDA, dt);
        }
        (n, c)
    }

    #[test]
    fn equilibrium_holds_at_zero_reactivity() {
        let c0 = equilibrium_precursors(1.0, BETA, GEN, LAMBDA);
        assert!(population_rate(1.0, c0, 0.0, BETA, GEN, LAMBDA).abs() < 1e-3);
        assert!(precursor_rate(1.0, c0, BETA, GEN, LAMBDA).abs() < 1e-3);
        let (n, _) = run(1.0, c0, 0.0, 1.0 / 60.0, 600);
        assert!((n - 1.0).abs() < 1e-3, "n drifted to {n}");
    }

    #[test]
    fn stable_at_steps_where_forward_euler_diverges() {
        // Forward Euler diverges above about 7.7 ms here; 100 ms stays put.
        let c0 = equilibrium_precursors(1.0, BETA, GEN, LAMBDA);
        let (n, c) = run(1.0, c0, 0.0, 0.1, 100);
        assert!((n - 1.0).abs() < 1e-3, "n = {n}");
        assert!((c - c0).abs() / c0 < 1e-3, "c = {c}");
    }

    #[test]
    fn negative_reactivity_drops_then_decays() {
        let rho = -0.005;
        let c0 = equilibrium_precursors(1.0, BETA, GEN, LAMBDA);
        let (n, _) = run(1.0, c0, rho, 1.0 / 60.0, 600);
        assert!(n > 0.0);
        assert!(n < prompt_jump_ratio(rho, BETA), "n = {n}");
    }

    #[test]
    fn positive_reactivity_grows_on_the_stable_period() {
        // ρ = 0.001: prompt jump to about 1.18, then the stable period
        // (β − ρ)/(λρ) ≈ 69 s, so about 1.36 after 10 s.
        let c0 = equilibrium_precursors(1.0, BETA, GEN, LAMBDA);
        let (n, _) = run(1.0, c0, 0.001, 1.0 / 60.0, 600);
        assert!(n > 1.25 && n < 1.5, "n = {n}");
    }

    #[test]
    fn prompt_supercritical_stays_finite() {
        let c0 = equilibrium_precursors(1.0, BETA, GEN, LAMBDA);
        // a·dt = 0.7 per step: each step splits in two.
        let (n, c) = run(1.0, c0, 0.01, 0.005, 20);
        assert!(n.is_finite() && c.is_finite());
        assert!(n > 1.0);
    }

    #[test]
    fn prompt_jump_ratio_matches_closed_form() {
        assert!((prompt_jump_ratio(-BETA, BETA) - 0.5).abs() < 1e-6);
        assert!(prompt_jump_ratio(BETA, BETA).is_infinite());
    }

    #[test]
    fn rod_worth_curves() {
        assert!(rod_worth_linear(0.008, 0.5, 0.5).abs() < 1e-9);
        assert!((rod_worth_linear(0.008, 1.0, 0.5) + 0.004).abs() < 1e-7);
        assert!(rod_worth_s_curve(0.0).abs() < 1e-6);
        assert!((rod_worth_s_curve(1.0) - 1.0).abs() < 1e-6);
        assert!((rod_worth_s_curve(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn temperature_feedback_is_linear() {
        assert!((temperature_feedback(-3.0e-6, 800.0, 700.0) + 3.0e-4).abs() < 1e-9);
    }

    #[test]
    fn decay_heat_falls_with_time() {
        let early = decay_heat_fraction(1.0, 1.0e9);
        assert!((early - 0.0612).abs() < 1e-3, "early = {early}");
        let hour = decay_heat_fraction(3600.0, 1.0e9);
        assert!(hour < early && hour > 0.0);
        assert_eq!(decay_heat_fraction(10.0, 0.0), 0.0);
    }
}
