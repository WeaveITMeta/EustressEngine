//! Membrane biophysics — Nernst, Goldman, Hodgkin-Huxley.

/// Ideal gas constant in joules per mole per kelvin.
const R_GAS: f32 = 8.314;

/// Faraday constant in coulombs per mole.
const FARADAY: f32 = 96485.33;

/// Nernst equilibrium potential in volts: E = (R * T) / (z * F) * ln(out / in).
pub fn nernst_potential(
    valence: f32,
    temperature: f32,
    conc_outside: f32,
    conc_inside: f32,
) -> f32 {
    if valence == 0.0 || conc_inside <= 0.0 || conc_outside <= 0.0 {
        return 0.0;
    }
    (R_GAS * temperature) / (valence * FARADAY) * (conc_outside / conc_inside).ln()
}

/// Goldman-Hodgkin-Katz resting potential in volts.
///
/// E = (R * T / F) * ln( (Pk*Kout + Pna*Naout + Pcl*Clin)
///                      / (Pk*Kin  + Pna*Nain  + Pcl*Clout) )
///
/// Chloride concentrations are reversed relative to the cations because Cl-
/// carries negative charge.
#[allow(clippy::too_many_arguments)]
pub fn goldman_potential(
    temperature: f32,
    p_k: f32,
    p_na: f32,
    p_cl: f32,
    k_out: f32,
    k_in: f32,
    na_out: f32,
    na_in: f32,
    cl_out: f32,
    cl_in: f32,
) -> f32 {
    let numerator = p_k * k_out + p_na * na_out + p_cl * cl_in;
    let denominator = p_k * k_in + p_na * na_in + p_cl * cl_out;
    if numerator <= 0.0 || denominator <= 0.0 {
        return 0.0;
    }
    (R_GAS * temperature) / FARADAY * (numerator / denominator).ln()
}

/// Typical neuronal resting membrane potential, -70 mV expressed in volts.
pub fn resting_potential_approx() -> f32 {
    -0.070
}

/// Membrane time constant tau = R * C (seconds).
pub fn membrane_time_constant(resistance: f32, capacitance: f32) -> f32 {
    resistance * capacitance
}

/// Membrane length (space) constant lambda = sqrt(Rm / Ra).
pub fn membrane_space_constant(membrane_resistance: f32, axial_resistance: f32) -> f32 {
    if axial_resistance <= 0.0 {
        return 0.0;
    }
    (membrane_resistance / axial_resistance).sqrt()
}

/// Passive cable voltage decay with distance: V(x) = V0 * exp(-x / lambda).
pub fn cable_voltage_decay(v0: f32, distance: f32, space_constant: f32) -> f32 {
    if space_constant == 0.0 {
        return 0.0;
    }
    v0 * (-distance / space_constant).exp()
}

/// Hodgkin-Huxley sodium current: I_Na = g_na_max * m^3 * h * (V - E_na).
pub fn hh_sodium_current(g_na_max: f32, m: f32, h: f32, v: f32, e_na: f32) -> f32 {
    g_na_max * m * m * m * h * (v - e_na)
}

/// Hodgkin-Huxley potassium current: I_K = g_k_max * n^4 * (V - E_k).
pub fn hh_potassium_current(g_k_max: f32, n: f32, v: f32, e_k: f32) -> f32 {
    let n4 = n * n * n * n;
    g_k_max * n4 * (v - e_k)
}

/// Hodgkin-Huxley leak current: I_leak = g_leak * (V - E_leak).
pub fn hh_leak_current(g_leak: f32, v: f32, e_leak: f32) -> f32 {
    g_leak * (v - e_leak)
}

/// Nernst potential expressed in millivolts (convenience wrapper).
pub fn nernst_potential_mv(
    valence: f32,
    temperature: f32,
    conc_outside: f32,
    conc_inside: f32,
) -> f32 {
    nernst_potential(valence, temperature, conc_outside, conc_inside) * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nernst_potassium_resting() {
        // K+ with [out] = 4 mM, [in] = 140 mM at 310 K is about -0.095 V.
        let e = nernst_potential(1.0, 310.0, 4.0, 140.0);
        assert!((e - (-0.0950)).abs() < 0.002, "expected ~-0.095 V, got {e}");
    }

    #[test]
    fn nernst_millivolt_wrapper_scales_by_1000() {
        let v = nernst_potential(1.0, 310.0, 4.0, 140.0);
        let mv = nernst_potential_mv(1.0, 310.0, 4.0, 140.0);
        assert!((mv - v * 1000.0).abs() < 1e-3, "mv {mv} != v*1000 {}", v * 1000.0);
    }

    #[test]
    fn cable_decay_one_space_constant() {
        // At x == lambda the voltage falls to V0 / e.
        let v = cable_voltage_decay(1.0, 2.0, 2.0);
        let expected = 1.0 / core::f32::consts::E;
        assert!((v - expected).abs() < 1e-5, "expected {expected}, got {v}");
    }

    #[test]
    fn sodium_current_zero_at_reversal() {
        // No driving force when V equals the sodium reversal potential.
        let i = hh_sodium_current(120.0, 0.5, 0.6, 0.050, 0.050);
        assert!(i.abs() < 1e-6, "expected 0 current at reversal, got {i}");
    }
}
