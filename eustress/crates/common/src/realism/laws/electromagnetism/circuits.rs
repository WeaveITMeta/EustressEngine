//! Circuit laws: Ohm, Kirchhoff, R/L/C, AC impedance, power.

// ── DC Ohm's law ─────────────────────────────────────────────────

/// I = V/R
pub fn ohm_current(voltage: f32, resistance: f32) -> f32 {
    voltage / resistance
}

/// V = I·R
pub fn ohm_voltage(current: f32, resistance: f32) -> f32 {
    current * resistance
}

/// R = V/I
pub fn ohm_resistance(voltage: f32, current: f32) -> f32 {
    voltage / current
}

// ── Series / parallel combinations ──────────────────────────────

/// R = ΣRᵢ
pub fn resistors_series(r: &[f32]) -> f32 {
    r.iter().copied().sum()
}

/// 1/R = Σ(1/Rᵢ)
pub fn resistors_parallel(r: &[f32]) -> f32 {
    let inv_sum: f32 = r.iter().map(|&ri| 1.0 / ri).sum();
    1.0 / inv_sum
}

/// 1/C = Σ(1/Cᵢ)
pub fn capacitors_series(c: &[f32]) -> f32 {
    let inv_sum: f32 = c.iter().map(|&ci| 1.0 / ci).sum();
    1.0 / inv_sum
}

/// C = ΣCᵢ
pub fn capacitors_parallel(c: &[f32]) -> f32 {
    c.iter().copied().sum()
}

/// L = ΣLᵢ
pub fn inductors_series(l: &[f32]) -> f32 {
    l.iter().copied().sum()
}

/// 1/L = Σ(1/Lᵢ)
pub fn inductors_parallel(l: &[f32]) -> f32 {
    let inv_sum: f32 = l.iter().map(|&li| 1.0 / li).sum();
    1.0 / inv_sum
}

// ── Time constants & transients ──────────────────────────────────

/// τ = RC
pub fn rc_time_constant(r: f32, c: f32) -> f32 {
    r * c
}

/// τ = L/R
pub fn rl_time_constant(r: f32, l: f32) -> f32 {
    l / r
}

/// ω₀ = 1/√(LC)
pub fn rlc_natural_frequency(l: f32, c: f32) -> f32 {
    1.0 / (l * c).sqrt()
}

/// ζ = (R/2)·√(C/L)
pub fn rlc_damping_ratio(r: f32, l: f32, c: f32) -> f32 {
    (r / 2.0) * (c / l).sqrt()
}

/// Q = (1/R)·√(L/C)
pub fn rlc_quality_factor(r: f32, l: f32, c: f32) -> f32 {
    (1.0 / r) * (l / c).sqrt()
}

/// V(t) = V₀·(1 - e^(-t/τ))  — capacitor charging
pub fn rc_charge_voltage(v0: f32, t: f32, tau: f32) -> f32 {
    v0 * (1.0 - (-t / tau).exp())
}

/// V(t) = V₀·e^(-t/τ)  — capacitor discharging
pub fn rc_discharge_voltage(v0: f32, t: f32, tau: f32) -> f32 {
    v0 * (-t / tau).exp()
}

/// I(t) = I_f·(1 - e^(-t/τ))  — inductor current rise
pub fn rl_current_rise(i_final: f32, t: f32, tau: f32) -> f32 {
    i_final * (1.0 - (-t / tau).exp())
}

// ── Capacitor/Inductor state updates ────────────────────────────

/// Integrate capacitor voltage: dV/dt = I/C. Returns new voltage.
pub fn capacitor_voltage_step(v: f32, i: f32, c: f32, dt: f32) -> f32 {
    v + (i / c) * dt
}

/// Integrate inductor current: dI/dt = V/L. Returns new current.
pub fn inductor_current_step(i: f32, v: f32, l: f32, dt: f32) -> f32 {
    i + (v / l) * dt
}

/// Capacitor stored energy: E = ½CV²
pub fn capacitor_energy(c: f32, v: f32) -> f32 {
    0.5 * c * v * v
}

/// Inductor stored energy: E = ½LI²
pub fn inductor_energy(l: f32, i: f32) -> f32 {
    0.5 * l * i * i
}

// ── AC impedance ─────────────────────────────────────────────────

/// Capacitive reactance: X_C = 1/(ωC)
pub fn capacitive_reactance(omega: f32, c: f32) -> f32 {
    1.0 / (omega * c)
}

/// Inductive reactance: X_L = ωL
pub fn inductive_reactance(omega: f32, l: f32) -> f32 {
    omega * l
}

/// RLC series impedance magnitude: |Z| = √(R² + (X_L - X_C)²)
pub fn rlc_series_impedance(r: f32, omega: f32, l: f32, c: f32) -> f32 {
    let x_l = inductive_reactance(omega, l);
    let x_c = capacitive_reactance(omega, c);
    let x_net = x_l - x_c;
    (r * r + x_net * x_net).sqrt()
}

/// RLC series phase angle: φ = atan2(X_L - X_C, R)
pub fn rlc_series_phase(r: f32, omega: f32, l: f32, c: f32) -> f32 {
    let x_l = inductive_reactance(omega, l);
    let x_c = capacitive_reactance(omega, c);
    (x_l - x_c).atan2(r)
}

/// Resonant frequency: ω₀ = 1/√(LC) [rad/s]
pub fn resonant_frequency_rad(l: f32, c: f32) -> f32 {
    1.0 / (l * c).sqrt()
}

/// Resonant frequency in Hz: f₀ = 1/(2π√(LC))
pub fn resonant_frequency_hz(l: f32, c: f32) -> f32 {
    1.0 / (2.0 * core::f32::consts::PI * (l * c).sqrt())
}

// ── Power ────────────────────────────────────────────────────────

/// P = VI
pub fn power_dc(v: f32, i: f32) -> f32 {
    v * i
}

/// P = I²R
pub fn power_resistive(i: f32, r: f32) -> f32 {
    i * i * r
}

/// P = V_rms · I_rms · cos(φ)
pub fn power_ac_real(v_rms: f32, i_rms: f32, cos_phi: f32) -> f32 {
    v_rms * i_rms * cos_phi
}

/// Q = V_rms · I_rms · sin(φ)
pub fn power_ac_reactive(v_rms: f32, i_rms: f32, sin_phi: f32) -> f32 {
    v_rms * i_rms * sin_phi
}

/// S = V_rms · I_rms
pub fn power_ac_apparent(v_rms: f32, i_rms: f32) -> f32 {
    v_rms * i_rms
}

/// pf = P/S
pub fn power_factor(p_real: f32, s_apparent: f32) -> f32 {
    p_real / s_apparent
}

// ── Kirchhoff checks ──────────────────────────────────────────────

/// KCL: returns sum of currents at node (should be 0 for valid circuit).
/// Convention: currents_in are positive, currents_out are negative.
pub fn kcl_check(currents_in: &[f32], currents_out: &[f32]) -> f32 {
    let sum_in: f32 = currents_in.iter().copied().sum();
    let sum_out: f32 = currents_out.iter().copied().sum();
    sum_in - sum_out
}

/// KVL: returns sum of voltages around loop (should be 0 for valid circuit).
/// signs[i] = +1 or -1 indicating polarity of each voltage element.
pub fn kvl_check(voltages: &[f32], signs: &[i8]) -> f32 {
    voltages
        .iter()
        .zip(signs.iter())
        .map(|(&v, &s)| v * (s as f32))
        .sum()
}

// ── Voltage divider / current divider ────────────────────────────

/// V_out = V_in · R2 / (R1 + R2)
pub fn voltage_divider(v_in: f32, r1: f32, r2: f32) -> f32 {
    v_in * r2 / (r1 + r2)
}

/// I_branch = I_in · R_total / R_branch  (current-divider rule for parallel branches)
/// where R_total is the equivalent parallel resistance of all branches combined.
pub fn current_divider(i_in: f32, r_branch: f32, r_total: f32) -> f32 {
    i_in * r_total / r_branch
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_ohm_law() {
        assert!(approx_eq(ohm_current(12.0, 4.0), 3.0));
        assert!(approx_eq(ohm_voltage(3.0, 4.0), 12.0));
        assert!(approx_eq(ohm_resistance(12.0, 3.0), 4.0));
    }

    #[test]
    fn test_resistors_series() {
        assert!(approx_eq(resistors_series(&[1.0, 2.0, 3.0]), 6.0));
    }

    #[test]
    fn test_resistors_parallel() {
        // Two equal 4Ω resistors → 2Ω
        assert!(approx_eq(resistors_parallel(&[4.0, 4.0]), 2.0));
    }

    #[test]
    fn test_capacitors_series() {
        // Two equal 4F caps in series → 2F
        assert!(approx_eq(capacitors_series(&[4.0, 4.0]), 2.0));
    }

    #[test]
    fn test_capacitors_parallel() {
        assert!(approx_eq(capacitors_parallel(&[1.0, 2.0, 3.0]), 6.0));
    }

    #[test]
    fn test_rc_time_constant() {
        assert!(approx_eq(rc_time_constant(1000.0, 0.001), 1.0));
    }

    #[test]
    fn test_rl_time_constant() {
        assert!(approx_eq(rl_time_constant(2.0, 4.0), 2.0));
    }

    #[test]
    fn test_rc_charge_discharge() {
        let tau = 1.0;
        // At t=0, charging starts at 0
        assert!(approx_eq(rc_charge_voltage(10.0, 0.0, tau), 0.0));
        // At t=0, discharge starts at V0
        assert!(approx_eq(rc_discharge_voltage(10.0, 0.0, tau), 10.0));
        // Charge + discharge at same t should sum to V0 (complementary)
        let t = 0.5;
        let v0 = 10.0;
        let charge = rc_charge_voltage(v0, t, tau);
        let discharge = rc_discharge_voltage(v0, t, tau);
        assert!((charge + discharge - v0).abs() < EPSILON);
    }

    #[test]
    fn test_capacitor_energy() {
        // E = 0.5 * 2 * 3^2 = 9
        assert!(approx_eq(capacitor_energy(2.0, 3.0), 9.0));
    }

    #[test]
    fn test_inductor_energy() {
        // E = 0.5 * 4 * 2^2 = 8
        assert!(approx_eq(inductor_energy(4.0, 2.0), 8.0));
    }

    #[test]
    fn test_ac_impedance() {
        // Pure resistive: X_L = X_C → |Z| = R
        let omega = 1.0;
        let l = 1.0;
        let c = 1.0; // X_L = X_C = 1 at omega=1
        let r = 5.0;
        assert!(approx_eq(rlc_series_impedance(r, omega, l, c), r));
        assert!(approx_eq(rlc_series_phase(r, omega, l, c), 0.0));
    }

    #[test]
    fn test_resonant_frequency() {
        let l = 1.0;
        let c = 1.0;
        // ω₀ = 1, f₀ = 1/(2π)
        assert!(approx_eq(resonant_frequency_rad(l, c), 1.0));
        assert!(
            (resonant_frequency_hz(l, c) - 1.0 / (2.0 * core::f32::consts::PI)).abs() < EPSILON
        );
    }

    #[test]
    fn test_power() {
        assert!(approx_eq(power_dc(12.0, 2.0), 24.0));
        assert!(approx_eq(power_resistive(3.0, 4.0), 36.0));
        assert!(approx_eq(power_ac_apparent(10.0, 2.0), 20.0));
        assert!(approx_eq(power_factor(16.0, 20.0), 0.8));
    }

    #[test]
    fn test_kcl_check() {
        // 3A in, 1A + 2A out → sum = 0
        assert!(approx_eq(kcl_check(&[3.0], &[1.0, 2.0]), 0.0));
    }

    #[test]
    fn test_kvl_check() {
        // 12V source - 4V - 8V = 0
        assert!(approx_eq(
            kvl_check(&[12.0, 4.0, 8.0], &[1, -1, -1]),
            0.0
        ));
    }

    #[test]
    fn test_voltage_divider() {
        // Equal resistors → half voltage
        assert!(approx_eq(voltage_divider(10.0, 5.0, 5.0), 5.0));
        // R2 = 0 → 0V
        assert!(approx_eq(voltage_divider(10.0, 5.0, 0.0), 0.0));
    }
}
