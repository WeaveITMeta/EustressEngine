//! Combustion: stoichiometric AFR, heating values, adiabatic flame temperature.
//!
//! Pure thermochemistry — no Bevy, no external crates.  All units are SI unless
//! stated otherwise in the doc comment (MJ/kg, kJ/mol, etc.).

// ---------------------------------------------------------------------------
// Stoichiometry
// ---------------------------------------------------------------------------

/// Stoichiometric air-fuel ratio (AFR) for a hydrocarbon C_n H_m.
///
/// Reaction: C_n H_m + (n + m/4) O2 → n CO2 + (m/2) H2O
///
/// Air is assumed 23.2 % O2 by mass.
///
/// # Arguments
/// * `n` — number of carbon atoms per molecule
/// * `m` — number of hydrogen atoms per molecule
///
/// # Returns
/// Stoichiometric AFR (mass of air / mass of fuel), dimensionless.
pub fn stoich_afr_hydrocarbon(n: f64, m: f64) -> f64 {
    // Molar masses (g/mol)
    const MW_C: f64 = 12.011;
    const MW_H: f64 = 1.008;
    const MW_O2: f64 = 31.998;
    const O2_MASS_FRACTION_IN_AIR: f64 = 0.232;

    let o2_moles = n + m / 4.0;
    let mw_fuel = n * MW_C + m * MW_H;
    let mass_o2_per_mass_fuel = (o2_moles * MW_O2) / mw_fuel;
    mass_o2_per_mass_fuel / O2_MASS_FRACTION_IN_AIR
}

/// Equivalence ratio λ (lambda): actual AFR divided by stoichiometric AFR.
///
/// λ < 1 → rich mixture, λ > 1 → lean mixture, λ = 1 → stoichiometric.
pub fn equivalence_ratio(actual_afr: f64, stoich_afr: f64) -> f64 {
    actual_afr / stoich_afr
}

/// Fuel equivalence ratio φ (phi): stoichiometric AFR divided by actual AFR.
///
/// φ = 1/λ.  φ > 1 → rich, φ < 1 → lean.
pub fn fuel_equivalence_ratio(actual_afr: f64, stoich_afr: f64) -> f64 {
    stoich_afr / actual_afr
}

// ---------------------------------------------------------------------------
// Heating values
// ---------------------------------------------------------------------------

/// Lower heating value (LHV) via the Dulong formula.
///
/// LHV = 33.9·C + 121.4·(H − O/8) + 10.5·S  [MJ/kg]
///
/// All mass fractions are dimensionless (kg/kg).
///
/// # Arguments
/// * `c` — mass fraction of carbon
/// * `h` — mass fraction of hydrogen
/// * `o` — mass fraction of oxygen (already in the fuel)
/// * `s` — mass fraction of sulfur
pub fn dulong_lhv(c: f64, h: f64, o: f64, s: f64) -> f64 {
    33.9 * c + 121.4 * (h - o / 8.0) + 10.5 * s
}

/// Convert LHV to HHV (higher heating value) by recovering latent heat of
/// condensation of the water formed during combustion.
///
/// HHV = LHV + 2.442 · (9 · H)  [MJ/kg]
///
/// 2.442 MJ/kg is the latent heat of vaporisation of water at 25 °C.
/// 9 kg water are produced per kg of hydrogen burned (MW_H2O / (2·MW_H) = 18/2).
pub fn lhv_to_hhv(lhv: f64, h_mass_fraction: f64) -> f64 {
    lhv + 2.442 * 9.0 * h_mass_fraction
}

/// Reference LHV constants for common fuels (MJ/kg).
pub mod fuel_lhv {
    /// Methane (natural gas main component)
    pub const METHANE: f64 = 50.05;
    /// Ethane
    pub const ETHANE: f64 = 47.79;
    /// Propane (LPG main component)
    pub const PROPANE: f64 = 46.35;
    /// n-Butane
    pub const BUTANE: f64 = 45.75;
    /// n-Octane (gasoline surrogate)
    pub const OCTANE: f64 = 44.43;
    /// n-Decane (diesel surrogate)
    pub const DECANE: f64 = 44.24;
    /// Hydrogen (H2)
    pub const HYDROGEN: f64 = 120.1;
    /// Ethanol (E100)
    pub const ETHANOL: f64 = 26.83;
    /// Methanol
    pub const METHANOL: f64 = 19.93;
}

// ---------------------------------------------------------------------------
// Adiabatic flame temperature
// ---------------------------------------------------------------------------

/// Adiabatic flame temperature — simple constant-Cp approximation.
///
/// ΔT = LHV / ((1 + AFR) · Cp_mix)
///
/// # Arguments
/// * `lhv_mj_per_kg`     — lower heating value of the fuel [MJ/kg]
/// * `afr`               — actual air-fuel ratio used (mass based)
/// * `cp_mix_kj_per_kgk` — constant-pressure specific heat of the
///                          combustion products mixture [kJ/(kg·K)]
/// * `t_initial_k`       — initial (reactants) temperature [K]
///
/// # Returns
/// Adiabatic flame temperature [K].
pub fn adiabatic_flame_temp_simple(
    lhv_mj_per_kg: f64,
    afr: f64,
    cp_mix_kj_per_kgk: f64,
    t_initial_k: f64,
) -> f64 {
    let lhv_kj = lhv_mj_per_kg * 1_000.0; // MJ/kg → kJ/kg
    let delta_t = lhv_kj / ((1.0 + afr) * cp_mix_kj_per_kgk);
    t_initial_k + delta_t
}

/// Adiabatic flame temperature — accurate iterative solution.
///
/// Integrates temperature-dependent Cp(T) = a + b·T polynomials for the
/// product species (CO2, H2O, N2, excess O2) using bisection.
///
/// The enthalpy balance is:
///   LHV_fuel = ∑_products m_i · ∫_{T_ref}^{T_ad} Cp_i(T) dT
///
/// # Arguments
/// * `lhv_mj_per_kg` — lower heating value [MJ/kg fuel]
/// * `n`             — carbon atoms per fuel molecule
/// * `m`             — hydrogen atoms per fuel molecule
/// * `mw_fuel`       — molar mass of fuel [g/mol]
/// * `afr`           — actual air-fuel ratio (mass based); use stoich for φ=1
/// * `t_ref_k`       — reference / initial temperature [K]
///
/// # Returns
/// Adiabatic flame temperature [K], converged within 1 K.
pub fn adiabatic_flame_temp_accurate(
    lhv_mj_per_kg: f64,
    n: f64,
    m: f64,
    mw_fuel: f64,
    afr: f64,
    t_ref_k: f64,
) -> f64 {
    // --- Molar masses [kg/mol] ---
    const MW_CO2: f64 = 44.010e-3;
    const MW_H2O: f64 = 18.015e-3;
    const MW_N2: f64 = 28.014e-3;
    const MW_O2: f64 = 31.998e-3;
    const MW_AIR: f64 = 28.966e-3;

    // Moles of O2 required for stoichiometric combustion (per mole fuel)
    let o2_stoich = n + m / 4.0;

    // Actual air supplied [mol per mol fuel]
    // afr (mass) = (n_air · MW_air) / MW_fuel
    let air_moles_actual = afr * (mw_fuel * 1e-3) / MW_AIR;

    let o2_actual = air_moles_actual * 0.21;
    let n2_actual = air_moles_actual * 0.79;

    // Product moles (per mole fuel)
    let mol_co2 = n;
    let mol_h2o = m / 2.0;
    let mol_n2 = n2_actual;
    // Excess O2 (lean) or 0 (rich / stoich)
    let mol_o2_excess = (o2_actual - o2_stoich).max(0.0);

    // Total product mass per kg fuel [kg products / kg fuel]
    let mw_fuel_kg = mw_fuel * 1e-3; // g/mol → kg/mol
    let m_co2 = mol_co2 * MW_CO2 / mw_fuel_kg;
    let m_h2o = mol_h2o * MW_H2O / mw_fuel_kg;
    let m_n2 = mol_n2 * MW_N2 / mw_fuel_kg;
    let m_o2 = mol_o2_excess * MW_O2 / mw_fuel_kg;

    // --- Linear Cp(T) = a + b·T [kJ/(kg·K)] ---
    // Coefficients from NASA/JANAF curve fits (valid 300–3500 K)
    // CO2: a=0.735, b=8.4e-4
    // H2O: a=1.670, b=3.0e-4
    // N2:  a=1.040, b=1.4e-4
    // O2:  a=0.910, b=1.7e-4
    struct CpCoeff {
        a: f64,
        b: f64,
    }
    let cp_co2 = CpCoeff { a: 0.735, b: 8.4e-4 };
    let cp_h2o = CpCoeff { a: 1.670, b: 3.0e-4 };
    let cp_n2  = CpCoeff { a: 1.040, b: 1.4e-4 };
    let cp_o2  = CpCoeff { a: 0.910, b: 1.7e-4 };

    // ∫_{T_ref}^{T} (a + b·T') dT' = a·(T−T_ref) + b/2·(T²−T_ref²)
    let enthalpy_rise = |t: f64| -> f64 {
        let dt = t - t_ref_k;
        let dt2 = t * t - t_ref_k * t_ref_k;
        let h = |mass: f64, cp: &CpCoeff| mass * (cp.a * dt + cp.b / 2.0 * dt2);
        h(m_co2, &cp_co2) + h(m_h2o, &cp_h2o) + h(m_n2, &cp_n2) + h(m_o2, &cp_o2)
        // units: (kg/kg_fuel) · kJ/(kg·K) · K = kJ/kg_fuel
    };

    let lhv_kj = lhv_mj_per_kg * 1_000.0; // MJ/kg → kJ/kg

    // Bisect for T_ad: enthalpy_rise(T_ad) = lhv_kj
    let mut lo = t_ref_k;
    let mut hi = 6_000.0_f64;

    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if enthalpy_rise(mid) < lhv_kj {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 0.5 {
            break;
        }
    }

    (lo + hi) / 2.0
}

// ---------------------------------------------------------------------------
// Emissions
// ---------------------------------------------------------------------------

/// CO2 emission factor: kg of CO2 produced per kg of fuel burned.
///
/// Stoichiometric: CO2_factor = (n · MW_CO2) / MW_fuel
///
/// # Arguments
/// * `n`        — carbon atoms per molecule
/// * `mw_fuel`  — molar mass of the fuel [g/mol]
pub fn co2_emission_factor(n: f64, mw_fuel: f64) -> f64 {
    const MW_CO2: f64 = 44.010; // g/mol
    (n * MW_CO2) / mw_fuel
}

/// H2O emission factor: kg of water produced per kg of fuel burned.
///
/// H2O_factor = ((m/2) · MW_H2O) / MW_fuel
///
/// # Arguments
/// * `m`        — hydrogen atoms per molecule
/// * `mw_fuel`  — molar mass of the fuel [g/mol]
pub fn h2o_emission_factor(m: f64, mw_fuel: f64) -> f64 {
    const MW_H2O: f64 = 18.015; // g/mol
    ((m / 2.0) * MW_H2O) / mw_fuel
}

/// Wobbe Index — interchangeability measure for fuel gases.
///
/// WI = LHV_volumetric / √(relative_density)
///
/// # Arguments
/// * `lhv_volumetric`   — lower heating value per unit volume [MJ/m³ or
///                         any consistent unit]
/// * `relative_density` — fuel density relative to air (both at same T, P)
pub fn wobbe_index(lhv_volumetric: f64, relative_density: f64) -> f64 {
    lhv_volumetric / relative_density.sqrt()
}

// ---------------------------------------------------------------------------
// Octane / Cetane
// ---------------------------------------------------------------------------

/// Approximate Research Octane Number (RON) from Motor Octane Number (MON).
///
/// Empirical relation: RON ≈ MON + 10
pub fn ron_from_mon(mon: f64) -> f64 {
    mon + 10.0
}

/// Knock intensity index based on compression ratio and specific heat ratio.
///
/// KI = r^(γ−1) − 1
///
/// Higher KI → greater tendency to auto-ignite (knock).
///
/// # Arguments
/// * `compression_ratio` — geometric compression ratio (dimensionless)
/// * `gamma`             — ratio of specific heats Cp/Cv (≈1.35 for air-fuel mix)
pub fn knock_intensity(compression_ratio: f64, gamma: f64) -> f64 {
    compression_ratio.powf(gamma - 1.0) - 1.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        if b == 0.0 {
            return a.abs() < tol;
        }
        ((a - b) / b).abs() < tol
    }

    // --- Stoichiometry ---

    #[test]
    fn test_methane_afr() {
        // CH4: n=1, m=4 → theoretical AFR ≈ 17.19
        let afr = stoich_afr_hydrocarbon(1.0, 4.0);
        assert!(approx_eq(afr, 17.19, 0.05), "CH4 AFR = {afr:.2}, expected ~17.19");
    }

    #[test]
    fn test_octane_afr() {
        // C8H18: n=8, m=18 → theoretical AFR ≈ 15.12
        let afr = stoich_afr_hydrocarbon(8.0, 18.0);
        assert!(approx_eq(afr, 15.12, 0.05), "C8H18 AFR = {afr:.2}, expected ~15.12");
    }

    #[test]
    fn test_equivalence_ratio_stoich() {
        let afr_s = stoich_afr_hydrocarbon(1.0, 4.0);
        let lambda = equivalence_ratio(afr_s, afr_s);
        assert!((lambda - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_fuel_equivalence_ratio_rich() {
        let afr_s = stoich_afr_hydrocarbon(8.0, 18.0);
        // Rich: supply half the stoich air
        let phi = fuel_equivalence_ratio(afr_s / 2.0, afr_s);
        assert!(approx_eq(phi, 2.0, 1e-6));
    }

    // --- Heating values ---

    #[test]
    fn test_dulong_lhv_coal() {
        // Typical bituminous coal: C=0.75, H=0.05, O=0.08, S=0.01
        let lhv = dulong_lhv(0.75, 0.05, 0.08, 0.01);
        // Expected ≈ 31–33 MJ/kg
        assert!(lhv > 28.0 && lhv < 36.0, "LHV = {lhv:.2} MJ/kg, out of range");
    }

    #[test]
    fn test_lhv_to_hhv_methane() {
        // CH4: H mass fraction = 4·1.008 / 16.043 ≈ 0.2513
        let h = 4.0 * 1.008 / 16.043;
        let hhv = lhv_to_hhv(fuel_lhv::METHANE, h);
        // CH4 HHV ≈ 55.5 MJ/kg
        assert!(approx_eq(hhv, 55.5, 0.05), "CH4 HHV = {hhv:.2} MJ/kg, expected ~55.5");
    }

    #[test]
    fn test_fuel_lhv_ordering() {
        // Hydrogen has highest LHV; methanol lowest in the table
        assert!(fuel_lhv::HYDROGEN > fuel_lhv::METHANE);
        assert!(fuel_lhv::METHANOL < fuel_lhv::ETHANOL);
    }

    // --- Adiabatic flame temperature ---

    #[test]
    fn test_aft_simple_methane() {
        // CH4 at stoich, Cp_mix ≈ 1.3 kJ/(kg·K), T_initial = 298 K
        let afr = stoich_afr_hydrocarbon(1.0, 4.0);
        let t_ad = adiabatic_flame_temp_simple(fuel_lhv::METHANE, afr, 1.3, 298.0);
        // Expect 2000–2600 K
        assert!(t_ad > 2_000.0 && t_ad < 2_700.0, "T_ad simple = {t_ad:.0} K");
    }

    #[test]
    fn test_aft_accurate_methane_stoich() {
        // CH4: n=1, m=4, MW=16.043, stoich AFR≈17.19
        let afr = stoich_afr_hydrocarbon(1.0, 4.0);
        let t_ad = adiabatic_flame_temp_accurate(
            fuel_lhv::METHANE,
            1.0,
            4.0,
            16.043,
            afr,
            298.0,
        );
        // Adiabatic flame temp of CH4/air ≈ 2230 K; allow wider band for simple Cp model
        assert!(t_ad > 1_800.0 && t_ad < 2_700.0, "T_ad accurate = {t_ad:.0} K");
    }

    #[test]
    fn test_aft_accurate_octane_stoich() {
        // C8H18: n=8, m=18, MW=114.23
        let afr = stoich_afr_hydrocarbon(8.0, 18.0);
        let t_ad = adiabatic_flame_temp_accurate(
            fuel_lhv::OCTANE,
            8.0,
            18.0,
            114.23,
            afr,
            298.0,
        );
        assert!(t_ad > 1_800.0 && t_ad < 2_800.0, "T_ad octane = {t_ad:.0} K");
    }

    // --- Emissions ---

    #[test]
    fn test_co2_factor_methane() {
        // CH4: n=1, MW=16.043 → factor = 44.010/16.043 ≈ 2.744
        let f = co2_emission_factor(1.0, 16.043);
        assert!(approx_eq(f, 2.744, 0.05), "CO2 factor CH4 = {f:.3}, expected ~2.744");
    }

    #[test]
    fn test_co2_factor_octane() {
        // C8H18: 8·44.010/114.23 ≈ 3.084
        let f = co2_emission_factor(8.0, 114.23);
        assert!(approx_eq(f, 3.084, 0.10), "CO2 factor C8H18 = {f:.3}");
    }

    #[test]
    fn test_h2o_factor_methane() {
        // CH4: m=4, (4/2)·18.015/16.043 ≈ 2.246
        let f = h2o_emission_factor(4.0, 16.043);
        assert!(approx_eq(f, 2.246, 0.05), "H2O factor CH4 = {f:.3}");
    }

    #[test]
    fn test_wobbe_index() {
        // Natural gas: LHV_vol ≈ 36 MJ/m³, relative density ≈ 0.60
        let wi = wobbe_index(36.0, 0.60);
        // WI ≈ 46.5
        assert!(approx_eq(wi, 46.48, 0.05), "Wobbe index = {wi:.2}");
    }

    // --- Octane / Cetane ---

    #[test]
    fn test_ron_from_mon() {
        assert!((ron_from_mon(85.0) - 95.0).abs() < 1e-10);
    }

    #[test]
    fn test_knock_intensity_typical() {
        // CR=10, γ=1.35 → KI = 10^0.35 − 1 ≈ 1.239
        let ki = knock_intensity(10.0, 1.35);
        let expected = 10.0_f64.powf(0.35) - 1.0;
        assert!((ki - expected).abs() < 1e-10);
        assert!(ki > 1.0 && ki < 2.0, "KI = {ki:.3}");
    }
}
