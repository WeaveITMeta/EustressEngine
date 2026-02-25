//! # Electrochemistry Laws
//!
//! Fundamental electrochemical equations for battery simulation.
//! Designed for V-Cell (solid-state Na-S) but general-purpose.
//!
//! ## Table of Contents
//!
//! 1. **Nernst Equation** — Equilibrium potential vs concentration
//! 2. **Butler-Volmer Kinetics** — Charge-transfer current density
//! 3. **Ohmic Losses** — IR drop, ASR, terminal voltage
//! 4. **Ionic Transport** — Arrhenius conductivity, Nernst-Planck, Nernst-Einstein
//! 5. **Heat Generation** — Ohmic, entropic, reaction heat
//! 6. **Na-S Specific** — OCV curve, sulfur utilization, volume expansion
//! 7. **Cycle Degradation** — Power-law capacity fade
//! 8. **State Functions** — SOC, DOD, C-rate, Ragone
//! 9. **NASICON Conductivity** — Sc-doped ASR and limiting current
//! 10. **Dendrite Risk** — Sand's time, Monroe-Newman critical current

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
#[inline]
pub fn arrhenius_conductivity(sigma0: f32, e_act: f32, temperature: f32) -> f32 {
    if temperature <= 0.0 { return 0.0; }
    sigma0 * (-(e_act / (constants::R_F32 * temperature))).exp()
}

/// Sc-NASICON conductivity at temperature (S/cm).
///
/// σ₀ = 1500 S/cm, E_a = 21,224 J/mol → target 10⁻² S/cm at 298.15 K
#[inline]
pub fn sc_nasicon_conductivity(temperature: f32) -> f32 {
    arrhenius_conductivity(
        constants::sc_nasicon::ARRHENIUS_PREFACTOR,
        constants::sc_nasicon::ACTIVATION_ENERGY_J_MOL,
        temperature,
    )
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

/// Entropic heat: `Q = -T I (dE/dT)` (W). Na-S: dE/dT ≈ -1.5e-4 V/K
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
// 6. Na-S Specific — OCV, Sulfur Utilization, Volume Expansion
// ============================================================================

/// Na-S OCV vs SOC — piecewise linear two-plateau model.
///
/// - Upper plateau (SOC 0.60–0.90): ~2.10–2.35 V — S₈ → Na₂S₄
/// - Lower plateau (SOC 0.05–0.25): ~1.50–1.85 V — Na₂S₄ → Na₂S
pub fn na_s_ocv(soc: f32) -> f32 {
    let s = soc.clamp(0.0, 1.0);
    if s >= 0.90      { 2.35 + (s - 0.90) * (2.80 - 2.35) / 0.10 }
    else if s >= 0.60 { 2.10 + (s - 0.60) * (2.35 - 2.10) / 0.30 }
    else if s >= 0.25 { 1.85 + (s - 0.25) * (2.10 - 1.85) / 0.35 }
    else if s >= 0.05 { 1.50 + (s - 0.05) * (1.85 - 1.50) / 0.20 }
    else              { 1.20 + s * (1.50 - 1.20) / 0.05 }
}

/// Temperature-corrected Na-S OCV: `OCV(T) = OCV(25°C) + (T - 298.15) × dE/dT`
#[inline]
pub fn na_s_ocv_temp_corrected(soc: f32, temperature: f32) -> f32 {
    na_s_ocv(soc) + (temperature - 298.15) * constants::na_s::ENTROPY_COEFFICIENT
}

/// Sulfur utilization: `u = Q_delivered / (m_S × 1672 mAh/g)`
#[inline]
pub fn sulfur_utilization(capacity_delivered_mah: f32, sulfur_mass_g: f32) -> f32 {
    if sulfur_mass_g <= 0.0 { return 0.0; }
    (capacity_delivered_mah / (sulfur_mass_g * constants::na_s::SULFUR_CAPACITY_MAH_G))
        .clamp(0.0, 1.0)
}

/// V-Cell gravimetric energy density (Wh/kg)
pub fn na_s_energy_density(mass_active_g: f32, mass_total_g: f32, utilization: f32) -> f32 {
    if mass_total_g <= 0.0 { return 0.0; }
    constants::na_s::THEORETICAL_ENERGY_DENSITY * (mass_active_g / mass_total_g) * utilization
}

/// Sulfur volume expansion at DOD: linear 0% (charged) → 80% (discharged)
#[inline]
pub fn sulfur_volume_expansion(soc: f32) -> f32 {
    (1.0 - soc).clamp(0.0, 1.0) * constants::na_s::SULFUR_VOLUME_EXPANSION
}

// ============================================================================
// 7. Cycle Degradation — Power-Law Capacity Fade
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

/// V-Cell degradation parameters `(α, β)` by C-rate.
///
/// | C-rate | Cycles to 80% |
/// |--------|---------------|
/// | 0.5C   | ~10,000       |
/// | 1C     | ~8,000        |
/// | 2C     | ~5,000        |
/// | 4C+    | ~3,000        |
pub fn vcell_degradation_params(c_rate: f32) -> (f32, f32) {
    if c_rate <= 0.5 { (2.0e-5, 0.80) }
    else if c_rate <= 1.0 { (3.5e-5, 0.82) }
    else if c_rate <= 2.0 { (6.0e-5, 0.85) }
    else { (1.2e-4, 0.88) }
}

/// V-Cell capacity at given cycle count and C-rate (convenience wrapper).
pub fn vcell_capacity_at_cycle(initial_capacity: f32, cycle_count: f32, c_rate: f32) -> f32 {
    let (alpha, beta) = vcell_degradation_params(c_rate);
    initial_capacity * capacity_retention_power_law(cycle_count, alpha, beta)
}

// ============================================================================
// 8. State Functions — SOC, DOD, C-rate, Energy
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
/// Peukert exponent ≈ 1.15 for solid-state Na-S
pub fn ragone_energy_density(energy_1c: f32, c_rate_val: f32, peukert_exp: f32) -> f32 {
    if c_rate_val <= 0.0 { return energy_1c; }
    energy_1c / c_rate_val.powf(peukert_exp - 1.0)
}

// ============================================================================
// 9. NASICON Conductivity — ASR and Limiting Current
// ============================================================================

/// Sc-NASICON ASR at temperature — conductivity in S/cm converted to SI for ASR (Ω·m²).
pub fn sc_nasicon_asr(thickness_m: f32, temperature: f32) -> f32 {
    let sigma_s_m = sc_nasicon_conductivity(temperature) * 100.0; // S/cm → S/m
    electrolyte_asr(thickness_m, sigma_s_m)
}

/// V-Cell electrolyte resistance (Ω) at temperature for a given electrode area.
///
/// Assumes 30 μm Sc-NASICON membrane.
pub fn vcell_electrolyte_resistance(temperature: f32, electrode_area: f32) -> f32 {
    let asr = sc_nasicon_asr(30.0e-6, temperature);
    cell_resistance_from_asr(asr, electrode_area)
}

/// Ionic limiting current density (A/m²) before transport limitation.
///
/// `j_lim = σ V_T / (thickness × τ)` where τ = tortuosity
pub fn nasicon_limiting_current(temperature: f32, tortuosity: f32) -> f32 {
    let sigma_s_m = sc_nasicon_conductivity(temperature) * 100.0;
    let v_t = thermal_voltage(temperature);
    (sigma_s_m * v_t) / (30.0e-6 * tortuosity.max(1.0))
}

// ============================================================================
// 10. Dendrite Risk — Sand's Time, Monroe-Newman Critical Current
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
/// `j_crit = 2 G_e δ / (F V_m,Na)` — above this, dendrites are thermodynamically favored.
///
/// * `shear_modulus_e` — electrolyte shear modulus (Pa), Sc-NASICON ≈ 32 GPa
/// * `interlayer_thickness` — ALD Al₂O₃ interlayer (m), V-Cell ≈ 5 nm
pub fn monroe_newman_critical_current(shear_modulus_e: f32, interlayer_thickness: f32) -> f32 {
    let vm_na = 23.78e-6; // m³/mol — Na molar volume
    (2.0 * shear_modulus_e * interlayer_thickness) / (constants::FARADAY_F32 * vm_na)
}

/// V-Cell dendrite risk factor: operating j / j_critical.
///
/// Returns 0.0 = safe, ≥1.0 = dendrite risk exceeded.
pub fn vcell_dendrite_risk(current_density: f32, _temperature: f32) -> f32 {
    let j_crit = monroe_newman_critical_current(32.0e9, 5.0e-9);
    if j_crit <= 0.0 { return 1.0; }
    (current_density / j_crit).max(0.0)
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
        // At Q=1.0, E = E°
        let e = nernst_potential(2.23, 2.0, 298.15, 1.0);
        assert!((e - 2.23).abs() < EPSILON);
    }

    #[test]
    fn thermal_voltage_25c() {
        let vt = thermal_voltage(298.15);
        assert!((vt - 0.02569).abs() < 1e-4);
    }

    #[test]
    fn nasicon_conductivity_increases_with_temp() {
        let s25 = sc_nasicon_conductivity(298.15);
        let s80 = sc_nasicon_conductivity(353.15);
        assert!(s80 > s25, "σ must increase with T: {s25} vs {s80}");
        assert!(s25 > 1e-4, "σ at 25°C must exceed 1e-4 S/cm, got {s25}");
    }

    #[test]
    fn ocv_full_range() {
        assert!((na_s_ocv(1.0) - 2.80).abs() < 0.02);
        assert!((na_s_ocv(0.0) - 1.20).abs() < 0.02);
    }

    #[test]
    fn ocv_monotone() {
        let pts = [0.0, 0.05, 0.1, 0.25, 0.4, 0.6, 0.75, 0.9, 1.0];
        for w in pts.windows(2) {
            assert!(na_s_ocv(w[0]) < na_s_ocv(w[1]),
                "OCV not monotone: SOC {}->{}", w[0], w[1]);
        }
    }

    #[test]
    fn retention_zero_cycles() {
        assert!((capacity_retention_power_law(0.0, 2e-5, 0.8) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn vcell_10k_cycles_half_c() {
        let (a, b) = vcell_degradation_params(0.5);
        let ret = capacity_retention_power_law(10_000.0, a, b);
        assert!((ret - 0.80).abs() < 0.03, "Expected ~80%, got {ret:.3}");
    }

    #[test]
    fn sulfur_util_95pct() {
        let u = sulfur_utilization(1_588.4, 1.0);
        assert!((u - 0.95).abs() < 0.01, "Expected 0.95, got {u}");
    }

    #[test]
    fn soc_coulomb_counting() {
        let soc = state_of_charge(1.0, 101.25, 202.5);
        assert!((soc - 0.50).abs() < 0.01);
    }

    #[test]
    fn butler_volmer_zero_eta() {
        // At η=0, j must be 0
        let j = butler_volmer_current(100.0, 0.0, 0.5, 0.5, 298.15);
        assert!(j.abs() < EPSILON);
    }

    #[test]
    fn dendrite_risk_low_current() {
        // At 0.1 mA/cm² = 1.0 A/m², risk should be very low
        let risk = vcell_dendrite_risk(1.0, 298.15);
        assert!(risk < 0.01, "Low current should give negligible risk: {risk}");
    }

    #[test]
    fn round_trip_eff() {
        let eff = round_trip_efficiency(1.95, 2.40);
        assert!((eff - 0.8125).abs() < 0.01);
    }
}
