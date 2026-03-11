//! # Electrochemistry Laws
//!
//! Fundamental electrochemical equations for general-purpose simulation.
//! Chemistry-agnostic implementations of core electrochemical principles.
//!
//! ## Table of Contents
//!
//! 1. **Nernst Equation** — Equilibrium potential vs concentration
//! 2. **Butler-Volmer Kinetics** — Charge-transfer current density
//! 3. **Ohmic Losses** — IR drop, ASR, terminal voltage
//! 4. **Ionic Transport** — Arrhenius conductivity, Nernst-Planck, Nernst-Einstein
//! 5. **Heat Generation** — Ohmic, entropic, reaction heat
//! 6. **Cycle Degradation** — Power-law capacity fade
//! 7. **State Functions** — SOC, DOD, C-rate, Ragone
//! 8. **Dendrite Risk** — Sand's time, Monroe-Newman critical current

use crate::realism::constants;

// ============================================================================
// 1. Nernst Equation
// ============================================================================

/// Nernst equation: `E = E° - (RT/nF) × ln(Q)`
///
/// # Arguments
/// * `e_standard`     — Standard cell potential E° (V)
/// * `n`              — Electrons transferred per formula unit
/// * `temperature`    — Temperature (K)
/// * `activity_ratio` — Reaction quotient Q = products / reactants
#[inline]
pub fn nernst_potential(e_standard: f32, n: f32, temperature: f32, activity_ratio: f32) -> f32 {
    if n <= 0.0 || temperature <= 0.0 || activity_ratio <= 0.0 {
        return e_standard;
    }
    let rt_nf = (constants::R_F32 * temperature) / (n * constants::FARADAY_F32);
    e_standard - rt_nf * activity_ratio.ln()
}

/// Thermal voltage: `V_T = RT/F` (V). At 298.15 K ≈ 25.7 mV.
#[inline]
pub fn thermal_voltage(temperature: f32) -> f32 {
    (constants::R_F32 * temperature) / constants::FARADAY_F32
}

// ============================================================================
// 2. Butler-Volmer Kinetics
// ============================================================================

/// Full Butler-Volmer: `j = j₀ × [exp(α_a F η / RT) - exp(-α_c F η / RT)]`
pub fn butler_volmer_current(
    j0: f32, eta: f32, alpha_a: f32, alpha_c: f32, temperature: f32,
) -> f32 {
    if j0 <= 0.0 || temperature <= 0.0 { return 0.0; }
    let f_rt = constants::FARADAY_F32 / (constants::R_F32 * temperature);
    j0 * ((alpha_a * f_rt * eta).exp() - (-alpha_c * f_rt * eta).exp())
}

/// Symmetric Butler-Volmer (α = 0.5): `j = 2j₀ sinh(Fη / 2RT)`
#[inline]
pub fn butler_volmer_symmetric(j0: f32, eta: f32, temperature: f32) -> f32 {
    if j0 <= 0.0 || temperature <= 0.0 { return 0.0; }
    let f_2rt = constants::FARADAY_F32 / (2.0 * constants::R_F32 * temperature);
    2.0 * j0 * (f_2rt * eta).sinh()
}

/// Tafel overpotential (high-η limit): `η = (RT / αF) × ln(j / j₀)`
pub fn tafel_overpotential(j: f32, j0: f32, alpha: f32, temperature: f32) -> f32 {
    if j0 <= 0.0 || j <= 0.0 || temperature <= 0.0 { return 0.0; }
    ((constants::R_F32 * temperature) / (alpha * constants::FARADAY_F32)) * (j / j0).ln()
}

/// Exchange current density: `j₀ = F k₀ c_ox^α c_red^(1-α)`
pub fn exchange_current_density(k0: f32, c_ox: f32, c_red: f32, alpha: f32) -> f32 {
    if k0 <= 0.0 || c_ox <= 0.0 || c_red <= 0.0 { return 0.0; }
    constants::FARADAY_F32 * k0 * c_ox.powf(alpha) * c_red.powf(1.0 - alpha)
}

// ============================================================================
// 3. Ohmic Losses
// ============================================================================

/// Ohmic overpotential: `η_ohm = I × R` (V)
#[inline]
pub fn ohmic_overpotential(current: f32, resistance: f32) -> f32 {
    current * resistance
}

/// Electrolyte area-specific resistance: `ASR = thickness / σ` (Ω·m²)
#[inline]
pub fn electrolyte_asr(thickness: f32, ionic_conductivity: f32) -> f32 {
    if ionic_conductivity <= 0.0 { return f32::INFINITY; }
    thickness / ionic_conductivity
}

/// Cell resistance from ASR: `R = ASR / A` (Ω)
#[inline]
pub fn cell_resistance_from_asr(asr: f32, electrode_area: f32) -> f32 {
    if electrode_area <= 0.0 { return f32::INFINITY; }
    asr / electrode_area
}

/// Terminal voltage with all loss mechanisms.
///
/// Discharge: `V = OCV - η_ohm - η_ct - η_diff`
/// Charge:    `V = OCV + η_ohm + η_ct + η_diff`
#[inline]
pub fn terminal_voltage(
    ocv: f32, eta_ohmic: f32, eta_ct: f32, eta_diff: f32, is_discharge: bool,
) -> f32 {
    let loss = eta_ohmic + eta_ct + eta_diff;
    if is_discharge { ocv - loss } else { ocv + loss }
}

/// Round-trip efficiency: `η_rt = V_discharge / V_charge`
#[inline]
pub fn round_trip_efficiency(v_discharge: f32, v_charge: f32) -> f32 {
    if v_charge <= 0.0 { return 0.0; }
    (v_discharge / v_charge).clamp(0.0, 1.0)
}

// ============================================================================
// 4. Ionic Transport
// ============================================================================

/// Arrhenius conductivity: `σ(T) = σ₀ exp(-E_a / RT)`
///
/// # Arguments
/// * `sigma0` — Pre-exponential factor (S/m or S/cm depending on use)
/// * `e_act` — Activation energy (J/mol)
/// * `temperature` — Temperature (K)
#[inline]
pub fn arrhenius_conductivity(sigma0: f32, e_act: f32, temperature: f32) -> f32 {
    if temperature <= 0.0 { return 0.0; }
    sigma0 * (-(e_act / (constants::R_F32 * temperature))).exp()
}

/// Nernst-Einstein diffusivity: `D = σRT / (z²F²c)` (m²/s)
pub fn nernst_einstein_diffusivity(
    conductivity: f32, concentration: f32, z: f32, temperature: f32,
) -> f32 {
    let denom = z * z * constants::FARADAY_F32 * constants::FARADAY_F32 * concentration;
    if denom <= 0.0 || temperature <= 0.0 { return 0.0; }
    (conductivity * constants::R_F32 * temperature) / denom
}

/// Nernst-Planck molar flux (1D): `J = -D(dc/dx) - (zFD/RT) c (dφ/dx)`
pub fn nernst_planck_flux(
    diffusivity: f32, concentration: f32, conc_gradient: f32,
    potential_gradient: f32, z: f32, temperature: f32,
) -> f32 {
    if temperature <= 0.0 { return 0.0; }
    let migr = (z * constants::FARADAY_F32) / (constants::R_F32 * temperature);
    -diffusivity * conc_gradient - migr * diffusivity * concentration * potential_gradient
}

// ============================================================================
// 5. Heat Generation
// ============================================================================

/// Ohmic heat: `Q = I²R` (W)
#[inline]
pub fn ohmic_heat(current: f32, resistance: f32) -> f32 {
    current * current * resistance
}

/// Charge-transfer heat: `Q = I |η_ct|` (W)
#[inline]
pub fn reaction_heat(current: f32, eta_ct: f32) -> f32 {
    current * eta_ct.abs()
}

/// Entropic heat: `Q = -T I (dE/dT)` (W)
///
/// # Arguments
/// * `temperature` — Temperature (K)
/// * `current` — Operating current (A)
/// * `de_dt` — Entropy coefficient dE/dT (V/K), chemistry-specific
#[inline]
pub fn entropic_heat(temperature: f32, current: f32, de_dt: f32) -> f32 {
    -temperature * current * de_dt
}

/// Total cell heat: `Q = Q_ohm + Q_rxn + Q_entropy` (W)
pub fn total_heat_generation(
    current: f32, resistance: f32, eta_ct: f32, temperature: f32, de_dt: f32,
) -> f32 {
    ohmic_heat(current, resistance)
        + reaction_heat(current, eta_ct)
        + entropic_heat(temperature, current, de_dt)
}

/// Steady-state temperature rise: `ΔT = Q × R_thermal` (K)
#[inline]
pub fn steady_state_temp_rise(heat_rate: f32, r_thermal: f32) -> f32 {
    heat_rate * r_thermal
}

// ============================================================================
// 6. Cycle Degradation — Power-Law Capacity Fade
// ============================================================================

/// Capacity retention: `Q(N)/Q₀ = 1 - α × N^β`
pub fn capacity_retention_power_law(cycle_count: f32, alpha: f32, beta: f32) -> f32 {
    if cycle_count <= 0.0 { return 1.0; }
    (1.0 - alpha * cycle_count.powf(beta)).clamp(0.0, 1.0)
}

/// Cycles to target retention: `N = ((1 - target) / α)^(1/β)`
pub fn cycles_to_retention(target_retention: f32, alpha: f32, beta: f32) -> f32 {
    if alpha <= 0.0 || beta <= 0.0 { return f32::INFINITY; }
    ((1.0 - target_retention.clamp(0.0, 1.0)) / alpha).powf(1.0 / beta)
}

// ============================================================================
// 7. State Functions — SOC, DOD, C-rate, Energy
// ============================================================================

/// Coulomb-counting SOC: `SOC = SOC₀ - Q_out / Q_nom`
#[inline]
pub fn state_of_charge(soc_initial: f32, charge_out_ah: f32, nominal_capacity: f32) -> f32 {
    if nominal_capacity <= 0.0 { return soc_initial; }
    (soc_initial - charge_out_ah / nominal_capacity).clamp(0.0, 1.0)
}

/// Depth of discharge: `DOD = 1 - SOC`
#[inline]
pub fn depth_of_discharge(soc: f32) -> f32 {
    (1.0 - soc).clamp(0.0, 1.0)
}

/// Instantaneous power: `P = V × I` (W)
#[inline]
pub fn power_output(v_terminal: f32, current: f32) -> f32 {
    v_terminal * current
}

/// Specific power (W/kg)
#[inline]
pub fn specific_power(v_terminal: f32, current: f32, mass_kg: f32) -> f32 {
    if mass_kg <= 0.0 { return 0.0; }
    power_output(v_terminal, current) / mass_kg
}

/// C-rate: `C = I / Q_nom` (h⁻¹)
#[inline]
pub fn c_rate(current_a: f32, capacity_ah: f32) -> f32 {
    if capacity_ah <= 0.0 { return 0.0; }
    current_a / capacity_ah
}

/// Current from C-rate: `I = C × Q_nom` (A)
#[inline]
pub fn current_from_c_rate(c_rate_val: f32, capacity_ah: f32) -> f32 {
    c_rate_val * capacity_ah
}

/// Ragone energy density (Peukert): `E(C) = E_1C / C^(n-1)`
///
/// # Arguments
/// * `energy_1c` — Energy density at 1C rate (Wh/kg or Wh/L)
/// * `c_rate_val` — Current C-rate
/// * `peukert_exp` — Peukert exponent (chemistry-specific, typically 1.0–1.3)
pub fn ragone_energy_density(energy_1c: f32, c_rate_val: f32, peukert_exp: f32) -> f32 {
    if c_rate_val <= 0.0 { return energy_1c; }
    energy_1c / c_rate_val.powf(peukert_exp - 1.0)
}

/// Ionic limiting current density (A/m²) before transport limitation.
///
/// `j_lim = σ V_T / (thickness × τ)` where τ = tortuosity
///
/// # Arguments
/// * `conductivity_s_m` — Ionic conductivity (S/m)
/// * `temperature` — Temperature (K)
/// * `thickness` — Electrolyte thickness (m)
/// * `tortuosity` — Tortuosity factor (≥1.0)
pub fn ionic_limiting_current(
    conductivity_s_m: f32,
    temperature: f32,
    thickness: f32,
    tortuosity: f32,
) -> f32 {
    if thickness <= 0.0 || tortuosity < 1.0 { return 0.0; }
    let v_t = thermal_voltage(temperature);
    (conductivity_s_m * v_t) / (thickness * tortuosity.max(1.0))
}

// ============================================================================
// 8. Dendrite Risk — Sand's Time, Monroe-Newman Critical Current
// ============================================================================

/// Sand's time — time (s) to dendrite penetration under constant current.
///
/// `t = π D (Fc₀)² / j²`
pub fn sands_time(diffusivity: f32, concentration: f32, current_density: f32) -> f32 {
    if diffusivity <= 0.0 || concentration <= 0.0 || current_density <= 0.0 {
        return f32::INFINITY;
    }
    let fc0 = constants::FARADAY_F32 * concentration;
    std::f32::consts::PI * diffusivity * fc0 * fc0 / (current_density * current_density)
}

/// Monroe-Newman critical current density for solid electrolytes (A/m²).
///
/// `j_crit = 2 G_e δ / (F V_m)` — above this, dendrites are thermodynamically favored.
///
/// # Arguments
/// * `shear_modulus` — Electrolyte shear modulus (Pa)
/// * `interlayer_thickness` — Protective interlayer thickness (m)
/// * `molar_volume` — Metal molar volume (m³/mol)
pub fn monroe_newman_critical_current(
    shear_modulus: f32,
    interlayer_thickness: f32,
    molar_volume: f32,
) -> f32 {
    if molar_volume <= 0.0 { return 0.0; }
    (2.0 * shear_modulus * interlayer_thickness) / (constants::FARADAY_F32 * molar_volume)
}

/// Dendrite risk factor: operating current density / critical current density.
///
/// Returns 0.0 = safe, ≥1.0 = dendrite risk exceeded.
#[inline]
pub fn dendrite_risk(current_density: f32, critical_current: f32) -> f32 {
    if critical_current <= 0.0 { return 1.0; }
    (current_density / critical_current).max(0.0)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-3;

    #[test]
    fn nernst_standard_conditions() {
        let e = nernst_potential(1.5, 2.0, 298.15, 1.0);
        assert!((e - 1.5).abs() < EPSILON);
    }

    #[test]
    fn nernst_activity_shift() {
        let e_std = nernst_potential(1.0, 1.0, 298.15, 1.0);
        let e_high = nernst_potential(1.0, 1.0, 298.15, 10.0);
        assert!(e_high < e_std, "Higher Q should lower potential");
    }

    #[test]
    fn thermal_voltage_25c() {
        let vt = thermal_voltage(298.15);
        assert!((vt - 0.02569).abs() < 1e-4);
    }

    #[test]
    fn arrhenius_increases_with_temp() {
        let s25 = arrhenius_conductivity(1000.0, 20000.0, 298.15);
        let s80 = arrhenius_conductivity(1000.0, 20000.0, 353.15);
        assert!(s80 > s25, "Conductivity must increase with T");
    }

    #[test]
    fn retention_zero_cycles() {
        assert!((capacity_retention_power_law(0.0, 2e-5, 0.8) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn retention_degrades_with_cycles() {
        let ret_1k = capacity_retention_power_law(1000.0, 1e-4, 0.8);
        let ret_5k = capacity_retention_power_law(5000.0, 1e-4, 0.8);
        assert!(ret_5k < ret_1k, "More cycles should degrade capacity");
    }

    #[test]
    fn soc_coulomb_counting() {
        let soc = state_of_charge(1.0, 50.0, 100.0);
        assert!((soc - 0.50).abs() < 0.01);
    }

    #[test]
    fn butler_volmer_zero_eta() {
        let j = butler_volmer_current(100.0, 0.0, 0.5, 0.5, 298.15);
        assert!(j.abs() < EPSILON);
    }

    #[test]
    fn butler_volmer_symmetric_positive_eta() {
        let j = butler_volmer_symmetric(10.0, 0.1, 298.15);
        assert!(j > 0.0, "Positive eta should give positive current");
    }

    #[test]
    fn dendrite_risk_ratio() {
        let risk = dendrite_risk(50.0, 100.0);
        assert!((risk - 0.5).abs() < EPSILON);
    }

    #[test]
    fn dendrite_risk_exceeds_critical() {
        let risk = dendrite_risk(150.0, 100.0);
        assert!(risk >= 1.0, "Should exceed critical");
    }

    #[test]
    fn round_trip_eff() {
        let eff = round_trip_efficiency(1.95, 2.40);
        assert!((eff - 0.8125).abs() < 0.01);
    }

    #[test]
    fn c_rate_calculation() {
        let c = c_rate(100.0, 100.0);
        assert!((c - 1.0).abs() < EPSILON);
    }

    #[test]
    fn ohmic_heat_calculation() {
        let q = ohmic_heat(10.0, 0.01);
        assert!((q - 1.0).abs() < EPSILON);
    }

    #[test]
    fn terminal_voltage_discharge() {
        let v = terminal_voltage(3.7, 0.1, 0.05, 0.02, true);
        assert!((v - 3.53).abs() < 0.01);
    }

    #[test]
    fn terminal_voltage_charge() {
        let v = terminal_voltage(3.7, 0.1, 0.05, 0.02, false);
        assert!((v - 3.87).abs() < 0.01);
    }
}
