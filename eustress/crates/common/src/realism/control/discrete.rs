//! Discrete-time (Z-domain) control and digital signal processing.

use core::f32::consts::PI;

// ── Bilinear transform (Tustin's method) ──────────────────────────

/// Convert continuous-time TF H(s) to discrete H(z) using bilinear transform.
/// s = 2/T · (z-1)/(z+1)
/// Returns (b_coeffs, a_coeffs) in Direct Form: H(z) = B(z)/A(z).
pub fn bilinear_transform(
    num_s: &[f32],
    den_s: &[f32],
    sample_period: f32,
) -> (Vec<f32>, Vec<f32>) {
    // Degree of the higher-order polynomial determines the order of H(z).
    let order = den_s.len().max(num_s.len()) - 1;
    let n = order + 1;

    // We expand H(z) by substituting s = (2/T)*(z-1)/(z+1).
    // Multiply numerator and denominator by (z+1)^order.
    // Polynomial coefficients in powers of z^k where index 0 is highest power.

    let two_over_t = 2.0 / sample_period;

    // Helper: raise (a*z + b)^k → coefficients in descending powers of z
    // (a*z + b)^k using binomial expansion.
    fn binomial_power(a: f32, b: f32, k: usize) -> Vec<f32> {
        let mut coeffs = vec![0.0f32; k + 1];
        // coeffs[i] = C(k,i) * a^(k-i) * b^i
        let mut binom = 1u64;
        for i in 0..=k {
            if i > 0 {
                binom = binom * (k - i + 1) as u64 / i as u64;
            }
            let power_a = k - i;
            let power_b = i;
            let a_val = if power_a == 0 { 1.0 } else { a.powi(power_a as i32) };
            let b_val = if power_b == 0 { 1.0 } else { b.powi(power_b as i32) };
            coeffs[i] = binom as f32 * a_val * b_val;
        }
        coeffs
    }

    // Pad num_s and den_s to length n (prepend zeros for lower-degree polys).
    let pad = |poly: &[f32], target_len: usize| -> Vec<f32> {
        let mut v = vec![0.0f32; target_len];
        let offset = target_len - poly.len();
        for (i, &c) in poly.iter().enumerate() {
            v[offset + i] = c;
        }
        v
    };

    let num_padded = pad(num_s, n);
    let den_padded = pad(den_s, n);

    // Build the z-domain numerator and denominator by summing contributions
    // for each coefficient c_k * s^k → c_k * (2/T)^k * (z-1)^k * (z+1)^(order-k)
    let mut b_z = vec![0.0f32; n];
    let mut a_z = vec![0.0f32; n];

    for k in 0..n {
        let power = order - k; // s^power term (num_padded[k] is coefficient of s^power)

        // (z-1)^power
        let zm1 = binomial_power(1.0, -1.0, power);
        // (z+1)^(order-power)
        let zp1 = binomial_power(1.0, 1.0, order - power);

        // Convolve zm1 and zp1 to get the full polynomial of degree `order`
        let mut poly = vec![0.0f32; n];
        for (i, &ci) in zm1.iter().enumerate() {
            for (j, &cj) in zp1.iter().enumerate() {
                poly[i + j] += ci * cj;
            }
        }

        let scale_num = num_padded[k] * two_over_t.powi(power as i32);
        let scale_den = den_padded[k] * two_over_t.powi(power as i32);

        for (i, &p) in poly.iter().enumerate() {
            b_z[i] += scale_num * p;
            a_z[i] += scale_den * p;
        }
    }

    // Normalise so that a_z[0] = 1
    let a0 = a_z[0];
    if a0.abs() > 1e-12 {
        for v in b_z.iter_mut() {
            *v /= a0;
        }
        for v in a_z.iter_mut() {
            *v /= a0;
        }
    }

    (b_z, a_z)
}

/// Frequency pre-warping for bilinear: ω_d = (2/T)·tan(ω_c·T/2)
pub fn prewarp_frequency(omega_c: f32, sample_period: f32) -> f32 {
    (2.0 / sample_period) * (omega_c * sample_period / 2.0).tan()
}

// ── Digital PID ─────────────────────────────────────────────────

/// PID in discrete time (position form with anti-windup clamping).
/// state: [integral_sum, prev_error, prev_prev_error]
pub fn digital_pid_step(
    kp: f32,
    ki: f32,
    kd: f32,
    setpoint: f32,
    measured: f32,
    state: &mut [f32; 3],
    dt: f32,
    output_min: f32,
    output_max: f32,
) -> f32 {
    let error = setpoint - measured;

    // Proportional term
    let p_term = kp * error;

    // Integral term with anti-windup: only integrate when not saturated
    // or when the integration would reduce saturation
    let integral_candidate = state[0] + ki * error * dt;
    let output_pre_clamp = p_term + integral_candidate + kd * (error - state[1]) / dt;
    let is_saturated_high = output_pre_clamp >= output_max;
    let is_saturated_low = output_pre_clamp <= output_min;
    // Back-calculation anti-windup: freeze integrator if saturated and error pushes further
    let should_integrate = !(is_saturated_high && error > 0.0) && !(is_saturated_low && error < 0.0);
    if should_integrate {
        state[0] = integral_candidate;
    }

    // Derivative term (backward difference)
    let d_term = if dt > 1e-12 {
        kd * (error - state[1]) / dt
    } else {
        0.0
    };

    // Update previous errors
    state[2] = state[1];
    state[1] = error;

    let output = p_term + state[0] + d_term;
    output.clamp(output_min, output_max)
}

// ── IIR filters (Direct Form II) ────────────────────────────────

/// Single IIR filter step in Direct Form II.
/// b: feedforward coefficients, a: feedback coefficients (a[0] normalised to 1).
/// w: delay-line state (length max(b.len(), a.len()) - 1).
/// Returns output.
pub fn iir_df2_step(b: &[f32], a: &[f32], x: f32, w: &mut [f32]) -> f32 {
    let state_len = b.len().max(a.len()).saturating_sub(1);
    debug_assert!(w.len() >= state_len, "w must have at least max(len(b),len(a))-1 elements");

    // Direct Form II transposed structure:
    // v[n] = x[n] - a[1]*w[0] - a[2]*w[1] - ...
    // y[n] = b[0]*v[n] + b[1]*w[0] + b[2]*w[1] + ...
    // then shift: w[k] = w[k-1] for k>0, w[0] = v[n]

    // Compute v[n] (the internal state input)
    let mut v = x;
    for k in 1..a.len() {
        if k - 1 < w.len() {
            v -= a[k] * w[k - 1];
        }
    }

    // Compute output y[n]
    let mut y = b[0] * v;
    for k in 1..b.len() {
        if k - 1 < w.len() {
            y += b[k] * w[k - 1];
        }
    }

    // Shift delay line
    if state_len > 0 {
        for k in (1..state_len).rev() {
            w[k] = w[k - 1];
        }
        w[0] = v;
    }

    y
}

// ── Standard filter designs ──────────────────────────────────────

/// 1st-order low-pass filter coefficients (bilinear, cutoff = fc Hz, sample rate = fs Hz).
/// Returns (b, a) for use with iir_df2_step.
/// H(s) = ωc/(s + ωc), bilinear: K = tan(π·fc/fs)
/// b = [K/(1+K), K/(1+K)], a = [1, (K-1)/(K+1)]
pub fn lpf1_coefficients(fc_hz: f32, fs_hz: f32) -> ([f32; 2], [f32; 2]) {
    let k = (PI * fc_hz / fs_hz).tan();
    let norm = 1.0 + k;
    let b0 = k / norm;
    let b1 = k / norm;
    let a0 = 1.0;
    let a1 = (k - 1.0) / norm;
    ([b0, b1], [a0, a1])
}

/// 1st-order high-pass filter coefficients.
/// H(s) = s/(s + ωc), bilinear: K = tan(π·fc/fs)
/// b = [1/(1+K), -1/(1+K)], a = [1, (K-1)/(1+K)]
pub fn hpf1_coefficients(fc_hz: f32, fs_hz: f32) -> ([f32; 2], [f32; 2]) {
    let k = (PI * fc_hz / fs_hz).tan();
    let norm = 1.0 + k;
    let b0 = 1.0 / norm;
    let b1 = -1.0 / norm;
    let a0 = 1.0;
    let a1 = (k - 1.0) / norm;
    ([b0, b1], [a0, a1])
}

/// 2nd-order Butterworth low-pass (biquad) coefficients.
/// Q = 1/√2, K = tan(π·fc/fs)
/// norm = 1 + √2·K + K²
/// b = [K²/norm, 2K²/norm, K²/norm]
/// a = [1, 2(K²-1)/norm, (1 - √2·K + K²)/norm]
pub fn lpf2_butterworth(fc_hz: f32, fs_hz: f32) -> ([f32; 3], [f32; 3]) {
    let k = (PI * fc_hz / fs_hz).tan();
    let k2 = k * k;
    let sqrt2 = core::f32::consts::SQRT_2;
    let norm = 1.0 + sqrt2 * k + k2;

    let b0 = k2 / norm;
    let b1 = 2.0 * k2 / norm;
    let b2 = k2 / norm;

    let a0 = 1.0;
    let a1 = 2.0 * (k2 - 1.0) / norm;
    let a2 = (1.0 - sqrt2 * k + k2) / norm;

    ([b0, b1, b2], [a0, a1, a2])
}

/// 2nd-order notch filter (for eliminating specific frequency).
/// H(z) = (1 - 2·cos(ω₀)·z⁻¹ + z⁻²) / (1 - 2·r·cos(ω₀)·z⁻¹ + r²·z⁻²)
/// where r = 1 - π·f_notch/(Q·fs) and ω₀ = 2π·f_notch/fs
pub fn notch_filter(f_notch_hz: f32, q_factor: f32, fs_hz: f32) -> ([f32; 3], [f32; 3]) {
    let omega0 = 2.0 * PI * f_notch_hz / fs_hz;
    let cos_w0 = omega0.cos();
    // Bandwidth-based pole radius: r = 1 - (π * BW / fs), BW = f_notch/Q
    let r = 1.0 - PI * (f_notch_hz / q_factor) / fs_hz;
    let r = r.clamp(0.0, 0.9999); // keep stable

    // Numerator: zeros on the unit circle at ±ω₀
    let b0 = 1.0;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0;

    // Denominator: poles inside unit circle
    let a0 = 1.0;
    let a1 = -2.0 * r * cos_w0;
    let a2 = r * r;

    ([b0, b1, b2], [a0, a1, a2])
}

// ── Z-transform utilities ────────────────────────────────────────

/// Evaluate H(z) magnitude at discrete frequency omega (0 to π for fs/2).
/// z = e^(jω), H(e^(jω)) = B(e^(jω)) / A(e^(jω))
pub fn hz_magnitude(b: &[f32], a: &[f32], omega: f32) -> f32 {
    // Evaluate polynomial P(z) = sum_k p[k] * z^(-(k)) at z = e^(jω)
    // = sum_k p[k] * e^(-j·ω·k) = sum_k p[k] * (cos(-ω·k) + j·sin(-ω·k))
    let eval = |coeffs: &[f32]| -> (f32, f32) {
        let mut re = 0.0f32;
        let mut im = 0.0f32;
        for (k, &c) in coeffs.iter().enumerate() {
            let angle = -(omega * k as f32);
            re += c * angle.cos();
            im += c * angle.sin();
        }
        (re, im)
    };

    let (b_re, b_im) = eval(b);
    let (a_re, a_im) = eval(a);

    let b_mag = (b_re * b_re + b_im * b_im).sqrt();
    let a_mag = (a_re * a_re + a_im * a_im).sqrt();

    if a_mag < 1e-12 {
        f32::INFINITY
    } else {
        b_mag / a_mag
    }
}

/// Group delay: -dφ/dω [samples]
/// Approximated numerically via central difference of the phase response.
pub fn group_delay(b: &[f32], a: &[f32], omega: f32) -> f32 {
    let delta = 1e-4_f32;
    let omega_lo = (omega - delta).max(0.0);
    let omega_hi = (omega + delta).min(PI);

    let phase_at = |w: f32| -> f32 {
        let eval = |coeffs: &[f32]| -> (f32, f32) {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            for (k, &c) in coeffs.iter().enumerate() {
                let angle = -(w * k as f32);
                re += c * angle.cos();
                im += c * angle.sin();
            }
            (re, im)
        };
        let (b_re, b_im) = eval(b);
        let (a_re, a_im) = eval(a);
        // Phase of H = phase(B) - phase(A)
        b_im.atan2(b_re) - a_im.atan2(a_re)
    };

    let phi_hi = phase_at(omega_hi);
    let phi_lo = phase_at(omega_lo);

    // Unwrap phase difference to handle wrapping
    let mut dphi = phi_hi - phi_lo;
    // Wrap to [-π, π]
    while dphi > PI {
        dphi -= 2.0 * PI;
    }
    while dphi < -PI {
        dphi += 2.0 * PI;
    }

    let dw = omega_hi - omega_lo;
    if dw < 1e-12 {
        0.0
    } else {
        -dphi / dw
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 1000.0;

    #[test]
    fn test_prewarp_low_frequency() {
        // For very low frequencies, ω_d ≈ ω_c (small-angle approximation)
        let omega_c = 0.1_f32; // rad/s, much less than 2/T
        let t = 0.001_f32;
        let omega_d = prewarp_frequency(omega_c, t);
        // Should be close to omega_c for small omega_c*T/2
        assert!((omega_d - omega_c).abs() < omega_c * 0.01, "prewarp: {omega_d} vs {omega_c}");
    }

    #[test]
    fn test_lpf1_dc_gain_unity() {
        let (b, a) = lpf1_coefficients(100.0, FS);
        // At DC (ω=0): H(1) = sum(b)/sum(a) should be 1
        let sum_b: f32 = b.iter().sum();
        let sum_a: f32 = a.iter().sum();
        let gain = sum_b / sum_a;
        assert!((gain - 1.0).abs() < 1e-5, "LPF1 DC gain: {gain}");
    }

    #[test]
    fn test_hpf1_dc_gain_zero() {
        let (b, a) = hpf1_coefficients(100.0, FS);
        let sum_b: f32 = b.iter().sum();
        let sum_a: f32 = a.iter().sum();
        let gain = if sum_a.abs() > 1e-12 { sum_b / sum_a } else { 0.0 };
        assert!(gain.abs() < 1e-5, "HPF1 DC gain should be ~0: {gain}");
    }

    #[test]
    fn test_lpf1_nyquist_attenuation() {
        let (b, a) = lpf1_coefficients(100.0, FS);
        // Nyquist ω = π: H(-1) = b[0]-b[1] / (a[0]-a[1])
        let b_nyq = b[0] - b[1];
        let a_nyq = a[0] - a[1];
        let gain = (b_nyq / a_nyq).abs();
        assert!(gain < 0.5, "LPF1 should attenuate at Nyquist: {gain}");
    }

    #[test]
    fn test_lpf2_butterworth_dc_gain_unity() {
        let (b, a) = lpf2_butterworth(100.0, FS);
        let sum_b: f32 = b.iter().sum();
        let sum_a: f32 = a.iter().sum();
        let gain = sum_b / sum_a;
        assert!((gain - 1.0).abs() < 1e-5, "LPF2 Butterworth DC gain: {gain}");
    }

    #[test]
    fn test_lpf2_butterworth_3db_at_cutoff() {
        let fc = 100.0_f32;
        let (b, a) = lpf2_butterworth(fc, FS);
        let omega_c = 2.0 * PI * fc / FS;
        let mag = hz_magnitude(&b, &a, omega_c);
        // -3 dB = 1/√2 ≈ 0.7071
        let expected = 1.0_f32 / 2.0_f32.sqrt();
        assert!((mag - expected).abs() < 0.01, "LPF2 3dB point: mag={mag}, expected≈{expected}");
    }

    #[test]
    fn test_notch_attenuation_at_notch_freq() {
        let f_notch = 60.0_f32;
        let q = 10.0_f32;
        let (b, a) = notch_filter(f_notch, q, FS);
        let omega = 2.0 * PI * f_notch / FS;
        let mag = hz_magnitude(&b, &a, omega);
        assert!(mag < 0.05, "Notch should deeply attenuate at notch freq: mag={mag}");
    }

    #[test]
    fn test_notch_passes_dc() {
        let (b, a) = notch_filter(60.0, 10.0, FS);
        let mag = hz_magnitude(&b, &a, 0.0);
        assert!((mag - 1.0).abs() < 0.05, "Notch DC gain should be ~1: {mag}");
    }

    #[test]
    fn test_iir_df2_step_settles() {
        let (b, a) = lpf1_coefficients(10.0, FS);
        let mut w = [0.0f32; 1];
        let mut y = 0.0_f32;
        // Apply unit step for many samples — should settle to 1.0
        for _ in 0..2000 {
            y = iir_df2_step(&b, &a, 1.0, &mut w);
        }
        assert!((y - 1.0).abs() < 1e-3, "LPF should settle to 1 for unit step: {y}");
    }

    #[test]
    fn test_digital_pid_converges_to_setpoint() {
        let kp = 1.0_f32;
        let ki = 0.5_f32;
        let kd = 0.01_f32;
        let dt = 0.01_f32;
        let setpoint = 5.0_f32;
        let mut state = [0.0f32; 3];
        let mut plant_state = 0.0_f32;

        // Simple first-order plant: x[k+1] = x[k] + u[k] * dt * 0.5
        for _ in 0..2000 {
            let u = digital_pid_step(kp, ki, kd, setpoint, plant_state, &mut state, dt, -100.0, 100.0);
            plant_state += u * dt * 0.5;
        }
        assert!((plant_state - setpoint).abs() < 0.1, "PID should converge: {plant_state}");
    }

    #[test]
    fn test_digital_pid_output_clamped() {
        let mut state = [0.0f32; 3];
        let u = digital_pid_step(1000.0, 0.0, 0.0, 100.0, 0.0, &mut state, 0.01, -10.0, 10.0);
        assert!(u <= 10.0 && u >= -10.0, "PID output must be clamped: {u}");
    }

    #[test]
    fn test_hz_magnitude_dc_lpf() {
        let (b, a) = lpf1_coefficients(100.0, FS);
        let mag = hz_magnitude(&b, &a, 0.0);
        assert!((mag - 1.0).abs() < 1e-5, "LPF DC magnitude should be 1: {mag}");
    }

    #[test]
    fn test_group_delay_nonnegative_lpf() {
        let (b, a) = lpf1_coefficients(100.0, FS);
        let gd = group_delay(&b, &a, 0.1);
        // Group delay of a causal filter should be non-negative
        assert!(gd >= -0.1, "Group delay should be non-negative: {gd}");
    }

    #[test]
    fn test_bilinear_transform_first_order_lpf() {
        // H(s) = ωc/(s + ωc), ωc = 2π*100 rad/s
        let omega_c = 2.0 * PI * 100.0_f32;
        let t = 1.0 / FS;
        let num_s = [omega_c];
        let den_s = [1.0_f32, omega_c];
        let (b, a) = bilinear_transform(&num_s, &den_s, t);
        // DC gain should be ~1
        let sum_b: f32 = b.iter().sum();
        let sum_a: f32 = a.iter().sum();
        let gain = sum_b / sum_a;
        assert!((gain - 1.0).abs() < 1e-4, "Bilinear LPF DC gain: {gain}");
    }
}
