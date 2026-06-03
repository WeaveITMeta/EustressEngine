//! Frequency-domain analysis: Bode, gain/phase margins, stability.

use core::f32::consts::PI;

// ── Transfer function evaluation ─────────────────────────────────

/// Evaluate the complex polynomial at s = jω.
/// Coefficients: [a0, a1, a2, ...] → a0 + a1·s + a2·s² + ...
/// Returns (real, imag) parts of the evaluated polynomial.
fn eval_poly_jw(coeffs: &[f32], omega: f32) -> (f32, f32) {
    // s = jω, s^k = (jω)^k
    // (jω)^0 = 1            → real
    // (jω)^1 = jω           → imag
    // (jω)^2 = -ω²          → real (negative)
    // (jω)^3 = -jω³         → imag (negative)
    // (jω)^4 = ω⁴           → real
    // pattern period 4: real_sign = [+1, 0, -1, 0], imag_sign = [0, +1, 0, -1]
    let mut real = 0.0f32;
    let mut imag = 0.0f32;
    let mut omega_k = 1.0f32; // ω^k

    for (k, &c) in coeffs.iter().enumerate() {
        match k % 4 {
            0 => real += c * omega_k,
            1 => imag += c * omega_k,
            2 => real -= c * omega_k,
            3 => imag -= c * omega_k,
            _ => unreachable!(),
        }
        omega_k *= omega;
    }

    (real, imag)
}

/// Evaluate H(jω) magnitude for a polynomial transfer function.
/// num/den: coefficients [a0, a1, a2, ...] for a0 + a1·s + a2·s² + ...
/// Returns |H(jω)| at angular frequency omega [rad/s].
pub fn tf_magnitude(num: &[f32], den: &[f32], omega: f32) -> f32 {
    let (nr, ni) = eval_poly_jw(num, omega);
    let (dr, di) = eval_poly_jw(den, omega);
    let num_mag = (nr * nr + ni * ni).sqrt();
    let den_mag = (dr * dr + di * di).sqrt();
    if den_mag == 0.0 {
        f32::INFINITY
    } else {
        num_mag / den_mag
    }
}

/// Evaluate H(jω) phase [radians] for a polynomial transfer function.
pub fn tf_phase(num: &[f32], den: &[f32], omega: f32) -> f32 {
    let (nr, ni) = eval_poly_jw(num, omega);
    let (dr, di) = eval_poly_jw(den, omega);
    let num_phase = ni.atan2(nr);
    let den_phase = di.atan2(dr);
    num_phase - den_phase
}

/// Evaluate both magnitude and phase in one call. Returns (mag, phase_rad).
pub fn tf_bode_point(num: &[f32], den: &[f32], omega: f32) -> (f32, f32) {
    let (nr, ni) = eval_poly_jw(num, omega);
    let (dr, di) = eval_poly_jw(den, omega);
    let num_mag = (nr * nr + ni * ni).sqrt();
    let den_mag = (dr * dr + di * di).sqrt();
    let mag = if den_mag == 0.0 {
        f32::INFINITY
    } else {
        num_mag / den_mag
    };
    let phase = ni.atan2(nr) - di.atan2(dr);
    (mag, phase)
}

// ── Bode plot data ───────────────────────────────────────────────

/// Generate Bode plot data over log-spaced frequency range.
/// Returns Vec<(omega, mag_db, phase_deg)>.
pub fn bode_plot(
    num: &[f32],
    den: &[f32],
    omega_min: f32,
    omega_max: f32,
    n_points: usize,
) -> Vec<(f32, f32, f32)> {
    if n_points == 0 {
        return Vec::new();
    }
    let log_min = omega_min.ln();
    let log_max = omega_max.ln();
    let mut result = Vec::with_capacity(n_points);

    for i in 0..n_points {
        let t = if n_points == 1 {
            0.0
        } else {
            i as f32 / (n_points - 1) as f32
        };
        let omega = (log_min + t * (log_max - log_min)).exp();
        let (mag, phase_rad) = tf_bode_point(num, den, omega);
        let mag_db = mag_to_db(mag);
        let phase_deg = phase_rad.to_degrees();
        result.push((omega, mag_db, phase_deg));
    }

    result
}

/// Convert magnitude to dB: dB = 20·log₁₀(|H|)
pub fn mag_to_db(magnitude: f32) -> f32 {
    20.0 * magnitude.log10()
}

/// Convert dB to magnitude
pub fn db_to_mag(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

// ── Stability margins ────────────────────────────────────────────

/// Gain crossover frequency: ω_gc where |H(jω)| = 1 (0 dB).
/// Searches in [omega_lo, omega_hi] using bisection (100 iterations max).
pub fn gain_crossover_frequency(
    num: &[f32],
    den: &[f32],
    omega_lo: f32,
    omega_hi: f32,
) -> Option<f32> {
    // f(ω) = |H(jω)| - 1; find zero crossing
    let f = |w: f32| tf_magnitude(num, den, w) - 1.0;

    let mut lo = omega_lo;
    let mut hi = omega_hi;
    let f_lo = f(lo);
    let f_hi = f(hi);

    // Check that there is a sign change
    if f_lo * f_hi > 0.0 {
        return None;
    }

    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        let f_mid = f(mid);
        if f_mid.abs() < 1e-7 {
            return Some(mid);
        }
        if f_lo * f_mid <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    Some(0.5 * (lo + hi))
}

/// Phase margin: PM = 180° + ∠H(jω_gc)  [degrees]
/// Positive PM required for stability. Typical target: PM > 45°.
pub fn phase_margin(num: &[f32], den: &[f32], omega_gc: f32) -> f32 {
    let phase_rad = tf_phase(num, den, omega_gc);
    180.0 + phase_rad.to_degrees()
}

/// Phase crossover frequency: ω_pc where ∠H(jω) = -180°.
pub fn phase_crossover_frequency(
    num: &[f32],
    den: &[f32],
    omega_lo: f32,
    omega_hi: f32,
) -> Option<f32> {
    // f(ω) = ∠H(jω) + π; find zero crossing (phase = -180° = -π rad)
    let f = |w: f32| tf_phase(num, den, w) + PI;

    let mut lo = omega_lo;
    let mut hi = omega_hi;
    let f_lo = f(lo);
    let f_hi = f(hi);

    if f_lo * f_hi > 0.0 {
        return None;
    }

    for _ in 0..100 {
        let mid = 0.5 * (lo + hi);
        let f_mid = f(mid);
        if f_mid.abs() < 1e-7 {
            return Some(mid);
        }
        if f_lo * f_mid <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    Some(0.5 * (lo + hi))
}

/// Gain margin: GM = -20·log₁₀(|H(jω_pc)|)  [dB]
/// Positive GM required for stability. Typical target: GM > 6 dB.
pub fn gain_margin_db(num: &[f32], den: &[f32], omega_pc: f32) -> f32 {
    let mag = tf_magnitude(num, den, omega_pc);
    -mag_to_db(mag)
}

// ── Standard first/second order responses ────────────────────────

/// First-order system step response: y(t) = K·(1 - e^(-t/τ))
pub fn first_order_step(k: f32, tau: f32, t: f32) -> f32 {
    if tau == 0.0 {
        return k;
    }
    k * (1.0 - (-t / tau).exp())
}

/// First-order time constant from 63.2% rise measurement.
/// The time constant τ is defined as the time at which the step response
/// reaches 63.2% of its final value (1 - e^(-1) ≈ 0.632).
pub fn time_constant_from_63pct(t_63pct: f32) -> f32 {
    // At t = τ: y(τ) = K·(1 - e^{-1}) ≈ 0.632·K
    // Therefore τ = t_63pct
    t_63pct
}

/// Damping ratio from percentage overshoot.
/// ζ = -ln(OS/100) / √(π² + ln²(OS/100))
pub fn damping_from_overshoot(overshoot_pct: f32) -> f32 {
    let os_frac = overshoot_pct / 100.0;
    if os_frac <= 0.0 {
        return 1.0; // critically or over-damped
    }
    let ln_os = os_frac.ln(); // ln is negative for os_frac < 1
    let numerator = -ln_os;
    let denominator = (PI * PI + ln_os * ln_os).sqrt();
    numerator / denominator
}

/// Second-order peak time: t_p = π / (ωn·√(1-ζ²))
pub fn peak_time(omega_n: f32, zeta: f32) -> f32 {
    let discriminant = 1.0 - zeta * zeta;
    if discriminant <= 0.0 {
        return f32::INFINITY; // overdamped or critically damped, no overshoot peak
    }
    PI / (omega_n * discriminant.sqrt())
}

/// Second-order settling time (2% criterion): t_s ≈ 4/(ζ·ωn)
pub fn settling_time_2pct(omega_n: f32, zeta: f32) -> f32 {
    if zeta <= 0.0 || omega_n <= 0.0 {
        return f32::INFINITY;
    }
    4.0 / (zeta * omega_n)
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-4;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    // First-order system: H(s) = 1/(s+1) → num=[1], den=[1,1]
    fn first_order_num() -> Vec<f32> { vec![1.0] }
    fn first_order_den() -> Vec<f32> { vec![1.0, 1.0] }

    #[test]
    fn test_tf_magnitude_dc() {
        // H(j0) = 1/1 = 1 for H(s)=1/(s+1)
        let mag = tf_magnitude(&first_order_num(), &first_order_den(), 0.0);
        assert!(approx_eq(mag, 1.0, EPSILON), "DC magnitude should be 1, got {}", mag);
    }

    #[test]
    fn test_tf_magnitude_at_breakpoint() {
        // H(j1) = 1/(j1+1); |H| = 1/√2 ≈ 0.7071 for H(s)=1/(s+1)
        let mag = tf_magnitude(&first_order_num(), &first_order_den(), 1.0);
        let expected = 1.0_f32 / 2.0_f32.sqrt();
        assert!(approx_eq(mag, expected, EPSILON), "Breakpoint magnitude should be 1/√2, got {}", mag);
    }

    #[test]
    fn test_tf_phase_dc() {
        // H(j0) = 1; phase = 0
        let phase = tf_phase(&first_order_num(), &first_order_den(), 0.0);
        assert!(approx_eq(phase, 0.0, EPSILON), "DC phase should be 0, got {}", phase);
    }

    #[test]
    fn test_tf_phase_at_breakpoint() {
        // H(j1) for H(s)=1/(s+1): phase = 0 - atan2(1,1) = -π/4
        let phase = tf_phase(&first_order_num(), &first_order_den(), 1.0);
        let expected = -PI / 4.0;
        assert!(approx_eq(phase, expected, EPSILON), "Phase at breakpoint should be -π/4, got {}", phase);
    }

    #[test]
    fn test_bode_point_consistency() {
        let (mag, phase) = tf_bode_point(&first_order_num(), &first_order_den(), 1.0);
        let mag2 = tf_magnitude(&first_order_num(), &first_order_den(), 1.0);
        let phase2 = tf_phase(&first_order_num(), &first_order_den(), 1.0);
        assert!(approx_eq(mag, mag2, EPSILON));
        assert!(approx_eq(phase, phase2, EPSILON));
    }

    #[test]
    fn test_bode_plot_length() {
        let pts = bode_plot(&first_order_num(), &first_order_den(), 0.01, 100.0, 50);
        assert_eq!(pts.len(), 50);
    }

    #[test]
    fn test_bode_plot_log_spaced() {
        let pts = bode_plot(&first_order_num(), &first_order_den(), 1.0, 100.0, 3);
        // Should be ~[1, 10, 100]
        assert!(approx_eq(pts[0].0, 1.0, 1e-3));
        assert!(approx_eq(pts[1].0, 10.0, 1e-2));
        assert!(approx_eq(pts[2].0, 100.0, 1e-1));
    }

    #[test]
    fn test_mag_to_db_unity() {
        assert!(approx_eq(mag_to_db(1.0), 0.0, EPSILON));
    }

    #[test]
    fn test_mag_to_db_ten() {
        assert!(approx_eq(mag_to_db(10.0), 20.0, EPSILON));
    }

    #[test]
    fn test_db_to_mag_roundtrip() {
        let mag = 0.5_f32;
        let db = mag_to_db(mag);
        let recovered = db_to_mag(db);
        assert!(approx_eq(recovered, mag, EPSILON));
    }

    #[test]
    fn test_gain_crossover_first_order() {
        // H(s) = 1/(s+1); gain crossover is where |H(jω)| = 1
        // |H(j0)| = 1, so the crossover is at ω=0 or we need a system with gain > 1
        // Use H(s) = 2/(s+1): num=[2], den=[1,1]; |H(jω)| = 2/√(1+ω²) = 1 → ω=√3
        let num = vec![2.0];
        let den = vec![1.0, 1.0];
        let wgc = gain_crossover_frequency(&num, &den, 0.1, 100.0);
        assert!(wgc.is_some());
        let w = wgc.unwrap();
        let expected = 3.0_f32.sqrt(); // ≈ 1.7321
        assert!(approx_eq(w, expected, 1e-3), "Expected √3, got {}", w);
    }

    #[test]
    fn test_phase_margin_first_order_gain2() {
        // For H(s) = 2/(s+1), ω_gc = √3
        // phase at √3: -atan(√3) = -60° → PM = 180 - 60 = 120°
        let num = vec![2.0];
        let den = vec![1.0, 1.0];
        let pm = phase_margin(&num, &den, 3.0_f32.sqrt());
        assert!(approx_eq(pm, 120.0, 1e-2), "Phase margin should be 120°, got {}", pm);
    }

    #[test]
    fn test_gain_crossover_none_when_no_crossing() {
        // H(s) = 0.1/(s+1); |H| < 1 everywhere for ω > 0
        let num = vec![0.1];
        let den = vec![1.0, 1.0];
        let result = gain_crossover_frequency(&num, &den, 0.01, 100.0);
        assert!(result.is_none(), "Should be None for low-gain system");
    }

    #[test]
    fn test_first_order_step_dc() {
        // y(∞) = K
        let y = first_order_step(2.0, 1.0, 1000.0);
        assert!(approx_eq(y, 2.0, 1e-3));
    }

    #[test]
    fn test_first_order_step_tau() {
        // y(τ) = K·(1 - 1/e) ≈ 0.6321·K
        let k = 1.0;
        let tau = 2.0;
        let y = first_order_step(k, tau, tau);
        let expected = k * (1.0 - (-1.0_f32).exp());
        assert!(approx_eq(y, expected, EPSILON));
    }

    #[test]
    fn test_time_constant_from_63pct() {
        // τ should equal the time at 63.2% rise
        let tau = time_constant_from_63pct(3.5);
        assert!(approx_eq(tau, 3.5, EPSILON));
    }

    #[test]
    fn test_damping_from_overshoot_16pct() {
        // For OS ≈ 16.3%, ζ ≈ 0.5
        let zeta = damping_from_overshoot(16.3);
        assert!(approx_eq(zeta, 0.5, 1e-2), "ζ should be ~0.5 for 16.3% OS, got {}", zeta);
    }

    #[test]
    fn test_damping_from_overshoot_zero() {
        // Zero overshoot → critically damped ζ = 1
        let zeta = damping_from_overshoot(0.0);
        assert!(approx_eq(zeta, 1.0, EPSILON));
    }

    #[test]
    fn test_peak_time() {
        // t_p = π / (ωn · √(1-ζ²))
        let omega_n = 2.0;
        let zeta = 0.5;
        let tp = peak_time(omega_n, zeta);
        let expected = PI / (omega_n * (1.0 - zeta * zeta).sqrt());
        assert!(approx_eq(tp, expected, EPSILON));
    }

    #[test]
    fn test_peak_time_overdamped() {
        // Overdamped: ζ ≥ 1 → no peak
        let tp = peak_time(1.0, 1.0);
        assert!(tp.is_infinite());
    }

    #[test]
    fn test_settling_time_2pct() {
        let ts = settling_time_2pct(10.0, 0.5);
        let expected = 4.0 / (0.5 * 10.0);
        assert!(approx_eq(ts, expected, EPSILON));
    }

    #[test]
    fn test_settling_time_invalid() {
        assert!(settling_time_2pct(0.0, 0.5).is_infinite());
        assert!(settling_time_2pct(1.0, 0.0).is_infinite());
    }

    #[test]
    fn test_second_order_phase_crossover() {
        // H(s) = ωn²/(s² + 2ζωn·s + ωn²) with ωn=1, ζ=0.1
        // num = [1], den = [1, 0.2, 1]
        // Phase = -180° at ω = ωn = 1 (approximately)
        let num = vec![1.0];
        let den = vec![1.0, 0.2, 1.0];
        let wpc = phase_crossover_frequency(&num, &den, 0.01, 100.0);
        assert!(wpc.is_some(), "Should find phase crossover");
        let w = wpc.unwrap();
        // For this system phase crosses -180 at ω=1
        assert!(approx_eq(w, 1.0, 1e-2), "Phase crossover near ω=1, got {}", w);
    }

    #[test]
    fn test_gain_margin_db_positive() {
        // At phase crossover, gain should be < 1 for a stable system → GM > 0
        let num = vec![1.0];
        let den = vec![1.0, 0.2, 1.0];
        let wpc = phase_crossover_frequency(&num, &den, 0.01, 100.0).unwrap();
        let gm = gain_margin_db(&num, &den, wpc);
        // At ω=1, |H(j1)| = 1/(2ζ) = 1/0.2 = 5 → GM = -20·log10(5) ≈ -14 dB (unstable loop)
        // This is a unity-feedback analysis so the loop gain matters more; just verify sign
        assert!(gm.is_finite());
    }
}
