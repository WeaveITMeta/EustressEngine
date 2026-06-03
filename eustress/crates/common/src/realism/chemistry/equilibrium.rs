//! Chemical equilibrium, colligative properties, and phase-equilibrium helpers.
//!
//! All quantities use SI base units unless the doc-comment states otherwise:
//! - concentrations in mol/L (M)
//! - temperatures in Kelvin
//! - pressures in atm (consistent with the R = 0.08206 L·atm/mol/K convention)
//! - enthalpies in J/mol
//! - the universal gas constant R = 8.314 J/(mol·K)

const R: f64 = 8.314; // J/(mol·K)

// ────────────────────────────────────────────────────────────────────────────
// Acid / base equilibria
// ────────────────────────────────────────────────────────────────────────────

/// Return the equilibrium [H+] concentration (mol/L) for a weak acid.
///
/// Solves the exact quadratic:  Ka = x² / (c − x)
/// → x² + Ka·x − Ka·c = 0
/// → x = (−Ka + √(Ka² + 4·Ka·c)) / 2
///
/// Panics (debug) if `ka` or `concentration` are non-positive.
pub fn weak_acid_h_concentration(ka: f64, concentration: f64) -> f64 {
    debug_assert!(ka > 0.0, "Ka must be positive");
    debug_assert!(concentration > 0.0, "concentration must be positive");

    let discriminant = ka * ka + 4.0 * ka * concentration;
    (-ka + discriminant.sqrt()) / 2.0
}

/// Return the pH of a weak-acid solution.
///
/// Uses the exact quadratic (`weak_acid_h_concentration`) rather than the
/// 5 % approximation, so it is accurate even for dilute solutions.
pub fn weak_acid_ph(ka: f64, concentration: f64) -> f64 {
    let h = weak_acid_h_concentration(ka, concentration);
    -h.log10()
}

/// Return the pH of a weak-base solution.
///
/// Solves the analogous quadratic for [OH−]:
/// Kb = x² / (c − x)  →  pOH = −log10(x)  →  pH = 14 − pOH
pub fn weak_base_ph(kb: f64, concentration: f64) -> f64 {
    debug_assert!(kb > 0.0, "Kb must be positive");
    debug_assert!(concentration > 0.0, "concentration must be positive");

    let discriminant = kb * kb + 4.0 * kb * concentration;
    let oh = (-kb + discriminant.sqrt()) / 2.0;
    let poh = -oh.log10();
    14.0 - poh
}

/// Return the pH of a buffer solution (Henderson-Hasselbalch equation).
///
/// `pka`       — pKa of the weak acid
/// `conc_base` — concentration of the conjugate base A− (mol/L)
/// `conc_acid` — concentration of the weak acid HA (mol/L)
///
/// pH = pKa + log10([A−] / [HA])
pub fn buffer_ph(pka: f64, conc_base: f64, conc_acid: f64) -> f64 {
    debug_assert!(conc_base > 0.0, "conjugate-base concentration must be positive");
    debug_assert!(conc_acid > 0.0, "weak-acid concentration must be positive");

    pka + (conc_base / conc_acid).log10()
}

/// Return the buffer capacity β (mol/L per pH unit).
///
/// β = 2.303 · Ka · [H+] / (Ka + [H+])² · C_total
///
/// where `c_total` = [HA] + [A−] and `h_conc` = [H+].
pub fn buffer_capacity(ka: f64, h_conc: f64, c_total: f64) -> f64 {
    debug_assert!(ka > 0.0, "Ka must be positive");
    debug_assert!(h_conc > 0.0, "[H+] must be positive");
    debug_assert!(c_total > 0.0, "total buffer concentration must be positive");

    let denom = (ka + h_conc).powi(2);
    2.303 * ka * h_conc / denom * c_total
}

// ────────────────────────────────────────────────────────────────────────────
// Solubility equilibria
// ────────────────────────────────────────────────────────────────────────────

/// Compute the ion product Q from a slice of (concentration, stoichiometry) pairs.
///
/// Q = ∏ cᵢ^νᵢ
///
/// Useful for checking whether a solution is sub-saturated (Q < Ksp),
/// at equilibrium (Q = Ksp), or supersaturated (Q > Ksp).
pub fn ion_product(ions: &[(f64, f64)]) -> f64 {
    ions.iter().fold(1.0_f64, |acc, &(conc, stoich)| acc * conc.powf(stoich))
}

/// Return the molar solubility s (mol/L) for a sparingly-soluble salt MₐXᵦ.
///
/// Ksp = (a·s)^a · (b·s)^b = aᵃ·bᵇ · s^(a+b)
/// → s = (Ksp / (aᵃ · bᵇ))^(1/(a+b))
///
/// `stoich_a` = a, `stoich_b` = b (stoichiometric coefficients, typically integers)
pub fn molar_solubility(ksp: f64, stoich_a: f64, stoich_b: f64) -> f64 {
    debug_assert!(ksp > 0.0, "Ksp must be positive");
    debug_assert!(stoich_a > 0.0 && stoich_b > 0.0, "stoichiometries must be positive");

    let prefactor = stoich_a.powf(stoich_a) * stoich_b.powf(stoich_b);
    let exponent = 1.0 / (stoich_a + stoich_b);
    (ksp / prefactor).powf(exponent)
}

/// Return the molar solubility of a 1:1 salt (MX) in the presence of a
/// common-ion `ci_conc` (mol/L) already in solution.
///
/// Exact quadratic for  MX ⇌ M+ + X− :
///   Ksp = (s)(s + ci_conc)
///   s² + ci_conc·s − Ksp = 0
///   s = (−ci_conc + √(ci_conc² + 4·Ksp)) / 2
///
/// Returns 0.0 if Ksp ≤ 0 (salt does not dissolve).
pub fn common_ion_solubility(ksp: f64, ci_conc: f64) -> f64 {
    if ksp <= 0.0 {
        return 0.0;
    }
    debug_assert!(ci_conc >= 0.0, "common-ion concentration must be non-negative");

    let discriminant = ci_conc * ci_conc + 4.0 * ksp;
    (-ci_conc + discriminant.sqrt()) / 2.0
}

// ────────────────────────────────────────────────────────────────────────────
// Phase equilibria and colligative properties
// ────────────────────────────────────────────────────────────────────────────

/// Return the vapour pressure P₂ (atm) at temperature T₂ (K) using the
/// Clausius-Clapeyron equation.
///
/// ln(P₂/P₁) = −ΔH_vap/R · (1/T₂ − 1/T₁)
/// → P₂ = P₁ · exp(−ΔH_vap/R · (1/T₂ − 1/T₁))
///
/// `p1`      — reference vapour pressure (atm)
/// `t1`      — reference temperature (K)
/// `t2`      — target temperature (K)
/// `dh_vap`  — enthalpy of vaporisation (J/mol)
pub fn clausius_clapeyron(p1: f64, t1: f64, t2: f64, dh_vap: f64) -> f64 {
    debug_assert!(t1 > 0.0 && t2 > 0.0, "temperatures must be in Kelvin");
    p1 * (-dh_vap / R * (1.0 / t2 - 1.0 / t1)).exp()
}

/// Return the boiling-point elevation ΔTb (K) of a solution.
///
/// ΔTb = Kb · m · i
///
/// `kb`  — ebullioscopic constant (K·kg/mol)
/// `m`   — molality of the solute (mol/kg)
/// `i`   — van't Hoff factor (1 for non-electrolytes, 2 for NaCl, …)
pub fn boiling_point_elevation(kb: f64, m: f64, i: f64) -> f64 {
    kb * m * i
}

/// Return the freezing-point depression ΔTf (K) of a solution.
///
/// ΔTf = Kf · m · i
///
/// `kf`  — cryoscopic constant (K·kg/mol)
/// `m`   — molality of the solute (mol/kg)
/// `i`   — van't Hoff factor
pub fn freezing_point_depression(kf: f64, m: f64, i: f64) -> f64 {
    kf * m * i
}

/// Return the osmotic pressure π (atm) of a dilute solution.
///
/// π = i · M · R_atm · T
///
/// Uses R = 0.08206 L·atm/(mol·K) so that `m_conc` in mol/L gives π in atm.
///
/// `i`       — van't Hoff factor
/// `m_conc`  — molar concentration (mol/L)
/// `temp`    — temperature (K)
pub fn osmotic_pressure(i: f64, m_conc: f64, temp: f64) -> f64 {
    const R_ATM: f64 = 0.082_06; // L·atm/(mol·K)
    i * m_conc * R_ATM * temp
}

/// Return the dissolved-gas concentration (mol/L) from Henry's law.
///
/// c = kH · P
///
/// `k_h` — Henry's law constant (mol/L/atm)
/// `p`   — partial pressure of the gas (atm)
pub fn henrys_law_concentration(k_h: f64, p: f64) -> f64 {
    k_h * p
}

/// Return the partial vapour pressure of component i above an ideal solution
/// (Raoult's law).
///
/// Pᵢ = xᵢ · P*ᵢ
///
/// `mole_fraction` — mole fraction of component i
/// `pure_vp`       — vapour pressure of the pure component (atm)
pub fn raoult_vapour_pressure(mole_fraction: f64, pure_vp: f64) -> f64 {
    mole_fraction * pure_vp
}

/// Return an approximate dew-point temperature (°C) from relative humidity.
///
/// Uses the simple Magnus-type approximation:
/// Td ≈ T − (100 − RH) / 5
///
/// `temp_c` — dry-bulb temperature (°C)
/// `rh`     — relative humidity (%, 0–100)
pub fn dew_point_approx(temp_c: f64, rh: f64) -> f64 {
    temp_c - (100.0 - rh) / 5.0
}

// ────────────────────────────────────────────────────────────────────────────
// Le Chatelier / conversion helpers
// ────────────────────────────────────────────────────────────────────────────

/// Return the equilibrium conversion α for a single-step reaction
/// A ⇌ B starting from pure A.
///
/// If the initial amount of A = 1, then at equilibrium:
///   [B]/[A] = K  →  α/(1−α) = K  →  α = K / (1 + K)
pub fn equilibrium_conversion_single(k: f64) -> f64 {
    debug_assert!(k >= 0.0, "equilibrium constant K must be non-negative");
    k / (1.0 + k)
}

/// Return the new equilibrium constant K₂ at temperature T₂ (K) given K₁ at
/// T₁ (K), using the van't Hoff equation.
///
/// ln(K₂/K₁) = −ΔH°/R · (1/T₂ − 1/T₁)
/// → K₂ = K₁ · exp(−ΔH°/R · (1/T₂ − 1/T₁))
///
/// `dh_rxn` — standard enthalpy of reaction (J/mol); negative = exothermic
pub fn lechatelier_temperature(k1: f64, t1: f64, t2: f64, dh_rxn: f64) -> f64 {
    debug_assert!(k1 > 0.0, "K1 must be positive");
    debug_assert!(t1 > 0.0 && t2 > 0.0, "temperatures must be in Kelvin");

    k1 * (-dh_rxn / R * (1.0 / t2 - 1.0 / t1)).exp()
}

/// Return the reaction quotient Q after a pressure change for an ideal-gas
/// equilibrium, expressed in the same Kp units.
///
/// For ideal gases Kp is pressure-invariant, but the *reaction quotient* Q
/// at the new pressure tells the caller which direction the system shifts:
///
/// Q = K · (P₁/P₂)^Δn
///
/// where Δn = Σν_products − Σν_reactants (moles of gas).
///
/// - Q < K  → equilibrium shifts right (toward products)
/// - Q > K  → equilibrium shifts left (toward reactants)
/// - Q = K  → Δn = 0, pressure change has no effect
///
/// `k`       — equilibrium constant Kp at current conditions
/// `p1`      — original total pressure
/// `p2`      — new total pressure
/// `delta_n` — change in moles of gas (products − reactants)
pub fn lechatelier_pressure_shift(k: f64, p1: f64, p2: f64, delta_n: f64) -> f64 {
    debug_assert!(p1 > 0.0 && p2 > 0.0, "pressures must be positive");
    k * (p1 / p2).powf(delta_n)
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-4;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // ── Acid / base ──────────────────────────────────────────────────────────

    #[test]
    fn test_weak_acid_h_concentration_acetic_acid() {
        // Acetic acid: Ka = 1.8e-5, c = 0.1 M
        // Exact quadratic gives [H+] ≈ 1.342e-3.
        let h = weak_acid_h_concentration(1.8e-5, 0.1);
        assert!(approx_eq(h, 1.342e-3, 1e-5), "acetic acid [H+] = {h}");
    }

    #[test]
    fn test_weak_acid_ph_acetic_acid() {
        // pH ≈ 2.872
        let ph = weak_acid_ph(1.8e-5, 0.1);
        assert!(approx_eq(ph, 2.872, 0.01), "acetic acid pH = {ph}");
    }

    #[test]
    fn test_weak_base_ph_ammonia() {
        // Ammonia: Kb = 1.8e-5, c = 0.1 M  → pOH ≈ 2.872 → pH ≈ 11.128
        let ph = weak_base_ph(1.8e-5, 0.1);
        assert!(approx_eq(ph, 11.128, 0.01), "ammonia pH = {ph}");
    }

    #[test]
    fn test_buffer_ph_acetate_buffer() {
        // pKa(acetic) = 4.745, [A-] = 0.1, [HA] = 0.1 → pH = pKa
        let ph = buffer_ph(4.745, 0.1, 0.1);
        assert!(approx_eq(ph, 4.745, 1e-9), "equal-concentration buffer pH = {ph}");
    }

    #[test]
    fn test_buffer_ph_ratio_2_to_1() {
        // [A-]/[HA] = 2 → pH = pKa + log10(2) ≈ pKa + 0.301
        let pka = 4.745_f64;
        let ph = buffer_ph(pka, 0.2, 0.1);
        let expected = pka + 2_f64.log10();
        assert!(approx_eq(ph, expected, 1e-9), "2:1 buffer pH = {ph}");
    }

    #[test]
    fn test_buffer_capacity_peak_at_half_neutralisation() {
        // β is maximised when [H+] = Ka (pH = pKa).
        // β_peak = 2.303 · Ka · Ka / (2·Ka)² · 1.0 = 2.303 / 4 ≈ 0.5757
        let ka = 1.8e-5_f64;
        let beta = buffer_capacity(ka, ka, 1.0);
        let expected = 2.303 / 4.0;
        assert!(approx_eq(beta, expected, 1e-4), "buffer capacity at peak = {beta}");
    }

    // ── Solubility ───────────────────────────────────────────────────────────

    #[test]
    fn test_ion_product_calcium_phosphate() {
        // Q = (3e-5)^3 · (2e-5)^2
        let q = ion_product(&[(3e-5, 3.0), (2e-5, 2.0)]);
        let expected = (3e-5_f64).powi(3) * (2e-5_f64).powi(2);
        assert!(approx_eq(q, expected, 1e-30), "ion product = {q}");
    }

    #[test]
    fn test_molar_solubility_agcl() {
        // AgCl: Ksp = 1.8e-10, a=1, b=1 → s = sqrt(Ksp) ≈ 1.342e-5
        let s = molar_solubility(1.8e-10, 1.0, 1.0);
        assert!(approx_eq(s, 1.342e-5, 1e-7), "AgCl solubility = {s}");
    }

    #[test]
    fn test_molar_solubility_pbcl2() {
        // PbCl2: Ksp = 1.6e-5, a=1, b=2
        // Ksp = (s)(2s)² = 4s³ → s = (Ksp/4)^(1/3)
        let s = molar_solubility(1.6e-5, 1.0, 2.0);
        let expected = (1.6e-5_f64 / 4.0_f64).cbrt();
        assert!(approx_eq(s, expected, 1e-6), "PbCl2 solubility = {s}");
    }

    #[test]
    fn test_common_ion_solubility_no_common_ion_matches_sqrt() {
        // With ci_conc = 0, answer must equal sqrt(Ksp).
        let ksp = 1.8e-10_f64;
        let s = common_ion_solubility(ksp, 0.0);
        assert!(approx_eq(s, ksp.sqrt(), 1e-12), "no-common-ion case = {s}");
    }

    #[test]
    fn test_common_ion_solubility_suppressed() {
        // AgCl in 0.10 M NaCl: s ≈ 1.8e-9 (strongly suppressed)
        let s = common_ion_solubility(1.8e-10, 0.1);
        assert!(s < 1.8e-8, "solubility should be suppressed, got {s}");
        assert!(approx_eq(s, 1.8e-9, 1e-10), "common-ion solubility = {s}");
    }

    // ── Phase equilibria / colligative ────────────────────────────────────────

    #[test]
    fn test_clausius_clapeyron_water() {
        // Water: P(100°C) = 1 atm, ΔH_vap = 40700 J/mol
        // Predict P at 120°C (393.15 K) ≈ 1.96 atm
        let p2 = clausius_clapeyron(1.0, 373.15, 393.15, 40_700.0);
        assert!(approx_eq(p2, 1.96, 0.05), "water VP at 120°C ≈ {p2} atm");
    }

    #[test]
    fn test_boiling_point_elevation_nacl() {
        // Kb(water) = 0.512, m = 1.0, i = 2 → ΔTb = 1.024 K
        let dtb = boiling_point_elevation(0.512, 1.0, 2.0);
        assert!(approx_eq(dtb, 1.024, EPS), "NaCl boiling elevation = {dtb}");
    }

    #[test]
    fn test_freezing_point_depression_glucose() {
        // Kf(water) = 1.86, m = 0.5, i = 1 → ΔTf = 0.93 K
        let dtf = freezing_point_depression(1.86, 0.5, 1.0);
        assert!(approx_eq(dtf, 0.93, EPS), "glucose FPD = {dtf}");
    }

    #[test]
    fn test_osmotic_pressure_physiological_saline() {
        // 0.154 M NaCl, i=2, T=310 K → π ≈ 7.84 atm
        let pi = osmotic_pressure(2.0, 0.154, 310.0);
        assert!(approx_eq(pi, 7.84, 0.05), "saline osmotic pressure ≈ {pi} atm");
    }

    #[test]
    fn test_henrys_law_co2() {
        // kH(CO2) ≈ 3.4e-2 mol/L/atm, P = 1 atm → c = 3.4e-2
        let c = henrys_law_concentration(3.4e-2, 1.0);
        assert!(approx_eq(c, 3.4e-2, 1e-6), "CO2 solubility = {c}");
    }

    #[test]
    fn test_raoult_vapour_pressure() {
        // x_benzene = 0.4, P* = 0.746 atm → P = 0.2984 atm
        let p = raoult_vapour_pressure(0.4, 0.746);
        assert!(approx_eq(p, 0.2984, EPS), "Raoult VP = {p}");
    }

    #[test]
    fn test_dew_point_approx() {
        // T = 30°C, RH = 50% → Td = 20°C
        let td = dew_point_approx(30.0, 50.0);
        assert!(approx_eq(td, 20.0, EPS), "dew point = {td} C");
    }

    // ── Le Chatelier ─────────────────────────────────────────────────────────

    #[test]
    fn test_equilibrium_conversion_single_large_k() {
        let alpha = equilibrium_conversion_single(1000.0);
        assert!(alpha > 0.999, "large K gives alpha near 1, got {alpha}");
    }

    #[test]
    fn test_equilibrium_conversion_single_k_equals_1() {
        let alpha = equilibrium_conversion_single(1.0);
        assert!(approx_eq(alpha, 0.5, EPS), "K=1 gives alpha=0.5, got {alpha}");
    }

    #[test]
    fn test_lechatelier_temperature_endothermic() {
        // ΔH > 0, T increases → K increases
        let k2 = lechatelier_temperature(1.0, 300.0, 400.0, 50_000.0);
        assert!(k2 > 1.0, "endothermic: K increases with T, got K2={k2}");
    }

    #[test]
    fn test_lechatelier_temperature_exothermic() {
        // ΔH < 0, T increases → K decreases
        let k2 = lechatelier_temperature(1.0, 300.0, 400.0, -50_000.0);
        assert!(k2 < 1.0, "exothermic: K decreases with T, got K2={k2}");
    }

    #[test]
    fn test_lechatelier_pressure_shift_delta_n_zero() {
        // Δn = 0 → Q = K regardless of pressure
        let q = lechatelier_pressure_shift(2.5, 1.0, 3.0, 0.0);
        assert!(approx_eq(q, 2.5, EPS), "Dn=0: Q should equal K, got {q}");
    }

    #[test]
    fn test_lechatelier_pressure_shift_math() {
        // K=10, P1=1, P2=2, Δn=-2 → Q = 10 · (0.5)^(-2) = 40
        let q = lechatelier_pressure_shift(10.0, 1.0, 2.0, -2.0);
        let expected = 10.0 * (0.5_f64).powf(-2.0);
        assert!(approx_eq(q, expected, EPS), "pressure shift Q = {q}");
    }

    #[test]
    fn test_van_t_hoff_self_consistency() {
        // Heat then cool back to original T → K unchanged
        let k1 = 5.0_f64;
        let k2 = lechatelier_temperature(k1, 300.0, 500.0, 30_000.0);
        let k1_back = lechatelier_temperature(k2, 500.0, 300.0, 30_000.0);
        assert!(approx_eq(k1_back, k1, 1e-10), "round-trip K = {k1_back}");
    }
}
