//! Rune bindings for the chemistry laws.
//!
//! Exposed to scripts under `eustress::realism::chemistry::*`. Each binding is a
//! thin f64 wrapper around the kernel laws in `crate::realism::laws::kinetics`
//! (chemical kinetics, catalysis) and `crate::realism::chemistry` (combustion,
//! equilibrium), because Rune works in f64 while the kinetics kernel is f32.
//! The combustion and equilibrium kernels are already f64, so those wrappers are
//! pass-through. Only scalar-in / scalar-out laws are bound here.

use rune::{ContextError, Module};
use crate::realism::laws::kinetics::chemical as chem;
use crate::realism::laws::kinetics::catalysis as cat;
use crate::realism::chemistry::combustion as comb;
use crate::realism::chemistry::equilibrium as eq;

// ── Chemical kinetics (crate::realism::laws::kinetics::chemical) ──────────────

#[rune::function]
fn arrhenius_rate(a: f64, e_a: f64, t_kelvin: f64) -> f64 {
    chem::arrhenius_rate(a as f32, e_a as f32, t_kelvin as f32) as f64
}

#[rune::function]
fn activation_energy(k1: f64, t1: f64, k2: f64, t2: f64) -> f64 {
    chem::activation_energy(k1 as f32, t1 as f32, k2 as f32, t2 as f32) as f64
}

#[rune::function]
fn rate_constant_at_t(k1: f64, e_a: f64, t1: f64, t2: f64) -> f64 {
    chem::rate_constant_at_t(k1 as f32, e_a as f32, t1 as f32, t2 as f32) as f64
}

#[rune::function]
fn first_order_concentration(c0: f64, k: f64, t: f64) -> f64 {
    chem::first_order_concentration(c0 as f32, k as f32, t as f32) as f64
}

#[rune::function]
fn second_order_concentration(c0: f64, k: f64, t: f64) -> f64 {
    chem::second_order_concentration(c0 as f32, k as f32, t as f32) as f64
}

#[rune::function]
fn half_life_first_order(k: f64) -> f64 {
    chem::half_life_first_order(k as f32) as f64
}

#[rune::function]
fn half_life_second_order(k: f64, c0: f64) -> f64 {
    chem::half_life_second_order(k as f32, c0 as f32) as f64
}

#[rune::function]
fn equilibrium_constant(delta_g_std: f64, t_kelvin: f64) -> f64 {
    chem::equilibrium_constant(delta_g_std as f32, t_kelvin as f32) as f64
}

#[rune::function]
fn standard_gibbs_from_k(k_eq: f64, t_kelvin: f64) -> f64 {
    chem::standard_gibbs_from_k(k_eq as f32, t_kelvin as f32) as f64
}

#[rune::function]
fn equilibrium_constant_at_t(k1: f64, delta_h: f64, t1: f64, t2: f64) -> f64 {
    chem::equilibrium_constant_at_t(k1 as f32, delta_h as f32, t1 as f32, t2 as f32) as f64
}

#[rune::function]
fn reaction_direction(delta_g_std: f64, k_eq: f64, q: f64, t: f64) -> f64 {
    chem::reaction_direction(delta_g_std as f32, k_eq as f32, q as f32, t as f32) as f64
}

#[rune::function]
fn ph_from_h_concentration(h_conc: f64) -> f64 {
    chem::ph_from_h_concentration(h_conc as f32) as f64
}

#[rune::function]
fn h_concentration_from_ph(ph: f64) -> f64 {
    chem::h_concentration_from_ph(ph as f32) as f64
}

#[rune::function]
fn poh_from_oh_concentration(oh_conc: f64) -> f64 {
    chem::poh_from_oh_concentration(oh_conc as f32) as f64
}

#[rune::function]
fn poh_from_ph(ph: f64) -> f64 {
    chem::poh_from_ph(ph as f32) as f64
}

#[rune::function]
fn henderson_hasselbalch(pka: f64, c_base: f64, c_acid: f64) -> f64 {
    chem::henderson_hasselbalch(pka as f32, c_base as f32, c_acid as f32) as f64
}

#[rune::function]
fn weak_acid_dissociation(ka: f64, c_acid: f64) -> f64 {
    chem::weak_acid_dissociation(ka as f32, c_acid as f32) as f64
}

#[rune::function]
fn pka(ka: f64) -> f64 {
    chem::pka(ka as f32) as f64
}

#[rune::function]
fn enthalpy_at_temperature(delta_h_ref: f64, delta_cp: f64, t_ref: f64, t: f64) -> f64 {
    chem::enthalpy_at_temperature(delta_h_ref as f32, delta_cp as f32, t_ref as f32, t as f32) as f64
}

// ── Catalytic kinetics (crate::realism::laws::kinetics::catalysis) ────────────

#[rune::function]
fn michaelis_menten(v_max: f64, km: f64, substrate: f64) -> f64 {
    cat::michaelis_menten(v_max as f32, km as f32, substrate as f32) as f64
}

#[rune::function]
fn km_from_lineweaver(x_intercept: f64) -> f64 {
    cat::km_from_lineweaver(x_intercept as f32) as f64
}

#[rune::function]
fn vmax_from_lineweaver(y_intercept: f64) -> f64 {
    cat::vmax_from_lineweaver(y_intercept as f32) as f64
}

#[rune::function]
fn hill_kinetics(v_max: f64, k_half: f64, n: f64, substrate: f64) -> f64 {
    cat::hill_kinetics(v_max as f32, k_half as f32, n as f32, substrate as f32) as f64
}

#[rune::function]
fn hill_coefficient(s_10pct: f64, s_90pct: f64) -> f64 {
    cat::hill_coefficient(s_10pct as f32, s_90pct as f32) as f64
}

#[rune::function]
fn competitive_inhibition_rate(v_max: f64, km: f64, substrate: f64, inhibitor: f64, ki: f64) -> f64 {
    cat::competitive_inhibition_rate(
        v_max as f32,
        km as f32,
        substrate as f32,
        inhibitor as f32,
        ki as f32,
    ) as f64
}

#[rune::function]
fn noncompetitive_inhibition_rate(
    v_max: f64,
    km: f64,
    substrate: f64,
    inhibitor: f64,
    ki: f64,
) -> f64 {
    cat::noncompetitive_inhibition_rate(
        v_max as f32,
        km as f32,
        substrate as f32,
        inhibitor as f32,
        ki as f32,
    ) as f64
}

#[rune::function]
fn uncompetitive_inhibition_rate(
    v_max: f64,
    km: f64,
    substrate: f64,
    inhibitor: f64,
    ki: f64,
) -> f64 {
    cat::uncompetitive_inhibition_rate(
        v_max as f32,
        km as f32,
        substrate as f32,
        inhibitor as f32,
        ki as f32,
    ) as f64
}

#[rune::function]
fn langmuir_coverage(k: f64, p: f64) -> f64 {
    cat::langmuir_coverage(k as f32, p as f32) as f64
}

#[rune::function]
fn langmuir_competitive(ki: f64, pi: f64, sum_kp: f64) -> f64 {
    cat::langmuir_competitive(ki as f32, pi as f32, sum_kp as f32) as f64
}

#[rune::function]
fn bet_coverage(c: f64, p: f64, p_sat: f64) -> f64 {
    cat::bet_coverage(c as f32, p as f32, p_sat as f32) as f64
}

#[rune::function]
fn langmuir_hinshelwood(k: f64, theta_a: f64, theta_b: f64, active_sites: f64) -> f64 {
    cat::langmuir_hinshelwood(k as f32, theta_a as f32, theta_b as f32, active_sites as f32) as f64
}

#[rune::function]
fn eley_rideal(k: f64, theta_a: f64, p_b: f64) -> f64 {
    cat::eley_rideal(k as f32, theta_a as f32, p_b as f32) as f64
}

#[rune::function]
fn surface_activation_energy(k1: f64, t1: f64, k2: f64, t2: f64) -> f64 {
    cat::surface_activation_energy(k1 as f32, t1 as f32, k2 as f32, t2 as f32) as f64
}

#[rune::function]
fn turnover_frequency(rate: f64, active_sites_per_m2: f64, surface_area: f64) -> f64 {
    cat::turnover_frequency(rate as f32, active_sites_per_m2 as f32, surface_area as f32) as f64
}

// ── Combustion (crate::realism::chemistry::combustion) — kernel is f64 ────────

#[rune::function]
fn stoich_afr_hydrocarbon(n: f64, m: f64) -> f64 {
    comb::stoich_afr_hydrocarbon(n, m)
}

#[rune::function]
fn equivalence_ratio(actual_afr: f64, stoich_afr: f64) -> f64 {
    comb::equivalence_ratio(actual_afr, stoich_afr)
}

#[rune::function]
fn fuel_equivalence_ratio(actual_afr: f64, stoich_afr: f64) -> f64 {
    comb::fuel_equivalence_ratio(actual_afr, stoich_afr)
}

#[rune::function]
fn dulong_lhv(c: f64, h: f64, o: f64, s: f64) -> f64 {
    comb::dulong_lhv(c, h, o, s)
}

#[rune::function]
fn lhv_to_hhv(lhv: f64, h_mass_fraction: f64) -> f64 {
    comb::lhv_to_hhv(lhv, h_mass_fraction)
}

#[rune::function]
fn adiabatic_flame_temp_simple(
    lhv_mj_per_kg: f64,
    afr: f64,
    cp_mix_kj_per_kgk: f64,
    t_initial_k: f64,
) -> f64 {
    comb::adiabatic_flame_temp_simple(lhv_mj_per_kg, afr, cp_mix_kj_per_kgk, t_initial_k)
}

// NOTE: adiabatic_flame_temp_accurate (6 args) is omitted from the Rune surface
// because Rune 0.14 binds functions of at most 5 arguments. Use
// adiabatic_flame_temp_simple from scripts, or call the kernel from Rust.

#[rune::function]
fn co2_emission_factor(n: f64, mw_fuel: f64) -> f64 {
    comb::co2_emission_factor(n, mw_fuel)
}

#[rune::function]
fn h2o_emission_factor(m: f64, mw_fuel: f64) -> f64 {
    comb::h2o_emission_factor(m, mw_fuel)
}

#[rune::function]
fn wobbe_index(lhv_volumetric: f64, relative_density: f64) -> f64 {
    comb::wobbe_index(lhv_volumetric, relative_density)
}

#[rune::function]
fn ron_from_mon(mon: f64) -> f64 {
    comb::ron_from_mon(mon)
}

#[rune::function]
fn knock_intensity(compression_ratio: f64, gamma: f64) -> f64 {
    comb::knock_intensity(compression_ratio, gamma)
}

// ── Equilibrium / colligative (crate::realism::chemistry::equilibrium) — f64 ──

#[rune::function]
fn weak_acid_h_concentration(ka: f64, concentration: f64) -> f64 {
    eq::weak_acid_h_concentration(ka, concentration)
}

#[rune::function]
fn weak_acid_ph(ka: f64, concentration: f64) -> f64 {
    eq::weak_acid_ph(ka, concentration)
}

#[rune::function]
fn weak_base_ph(kb: f64, concentration: f64) -> f64 {
    eq::weak_base_ph(kb, concentration)
}

#[rune::function]
fn buffer_ph(pka: f64, conc_base: f64, conc_acid: f64) -> f64 {
    eq::buffer_ph(pka, conc_base, conc_acid)
}

#[rune::function]
fn buffer_capacity(ka: f64, h_conc: f64, c_total: f64) -> f64 {
    eq::buffer_capacity(ka, h_conc, c_total)
}

#[rune::function]
fn molar_solubility(ksp: f64, stoich_a: f64, stoich_b: f64) -> f64 {
    eq::molar_solubility(ksp, stoich_a, stoich_b)
}

#[rune::function]
fn common_ion_solubility(ksp: f64, ci_conc: f64) -> f64 {
    eq::common_ion_solubility(ksp, ci_conc)
}

#[rune::function]
fn clausius_clapeyron(p1: f64, t1: f64, t2: f64, dh_vap: f64) -> f64 {
    eq::clausius_clapeyron(p1, t1, t2, dh_vap)
}

#[rune::function]
fn boiling_point_elevation(kb: f64, m: f64, i: f64) -> f64 {
    eq::boiling_point_elevation(kb, m, i)
}

#[rune::function]
fn freezing_point_depression(kf: f64, m: f64, i: f64) -> f64 {
    eq::freezing_point_depression(kf, m, i)
}

#[rune::function]
fn osmotic_pressure(i: f64, m_conc: f64, temp: f64) -> f64 {
    eq::osmotic_pressure(i, m_conc, temp)
}

#[rune::function]
fn henrys_law_concentration(k_h: f64, p: f64) -> f64 {
    eq::henrys_law_concentration(k_h, p)
}

#[rune::function]
fn raoult_vapour_pressure(mole_fraction: f64, pure_vp: f64) -> f64 {
    eq::raoult_vapour_pressure(mole_fraction, pure_vp)
}

#[rune::function]
fn dew_point_approx(temp_c: f64, rh: f64) -> f64 {
    eq::dew_point_approx(temp_c, rh)
}

#[rune::function]
fn equilibrium_conversion_single(k: f64) -> f64 {
    eq::equilibrium_conversion_single(k)
}

#[rune::function]
fn lechatelier_temperature(k1: f64, t1: f64, t2: f64, dh_rxn: f64) -> f64 {
    eq::lechatelier_temperature(k1, t1, t2, dh_rxn)
}

#[rune::function]
fn lechatelier_pressure_shift(k: f64, p1: f64, p2: f64, delta_n: f64) -> f64 {
    eq::lechatelier_pressure_shift(k, p1, p2, delta_n)
}

/// Build the `eustress::realism::chemistry` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "chemistry"])?;

    // Chemical kinetics
    m.function_meta(arrhenius_rate)?;
    m.function_meta(activation_energy)?;
    m.function_meta(rate_constant_at_t)?;
    m.function_meta(first_order_concentration)?;
    m.function_meta(second_order_concentration)?;
    m.function_meta(half_life_first_order)?;
    m.function_meta(half_life_second_order)?;
    m.function_meta(equilibrium_constant)?;
    m.function_meta(standard_gibbs_from_k)?;
    m.function_meta(equilibrium_constant_at_t)?;
    m.function_meta(reaction_direction)?;
    m.function_meta(ph_from_h_concentration)?;
    m.function_meta(h_concentration_from_ph)?;
    m.function_meta(poh_from_oh_concentration)?;
    m.function_meta(poh_from_ph)?;
    m.function_meta(henderson_hasselbalch)?;
    m.function_meta(weak_acid_dissociation)?;
    m.function_meta(pka)?;
    m.function_meta(enthalpy_at_temperature)?;

    // Catalytic kinetics
    m.function_meta(michaelis_menten)?;
    m.function_meta(km_from_lineweaver)?;
    m.function_meta(vmax_from_lineweaver)?;
    m.function_meta(hill_kinetics)?;
    m.function_meta(hill_coefficient)?;
    m.function_meta(competitive_inhibition_rate)?;
    m.function_meta(noncompetitive_inhibition_rate)?;
    m.function_meta(uncompetitive_inhibition_rate)?;
    m.function_meta(langmuir_coverage)?;
    m.function_meta(langmuir_competitive)?;
    m.function_meta(bet_coverage)?;
    m.function_meta(langmuir_hinshelwood)?;
    m.function_meta(eley_rideal)?;
    m.function_meta(surface_activation_energy)?;
    m.function_meta(turnover_frequency)?;

    // Combustion
    m.function_meta(stoich_afr_hydrocarbon)?;
    m.function_meta(equivalence_ratio)?;
    m.function_meta(fuel_equivalence_ratio)?;
    m.function_meta(dulong_lhv)?;
    m.function_meta(lhv_to_hhv)?;
    m.function_meta(adiabatic_flame_temp_simple)?;
    m.function_meta(co2_emission_factor)?;
    m.function_meta(h2o_emission_factor)?;
    m.function_meta(wobbe_index)?;
    m.function_meta(ron_from_mon)?;
    m.function_meta(knock_intensity)?;

    // Equilibrium / colligative
    m.function_meta(weak_acid_h_concentration)?;
    m.function_meta(weak_acid_ph)?;
    m.function_meta(weak_base_ph)?;
    m.function_meta(buffer_ph)?;
    m.function_meta(buffer_capacity)?;
    m.function_meta(molar_solubility)?;
    m.function_meta(common_ion_solubility)?;
    m.function_meta(clausius_clapeyron)?;
    m.function_meta(boiling_point_elevation)?;
    m.function_meta(freezing_point_depression)?;
    m.function_meta(osmotic_pressure)?;
    m.function_meta(henrys_law_concentration)?;
    m.function_meta(raoult_vapour_pressure)?;
    m.function_meta(dew_point_approx)?;
    m.function_meta(equilibrium_conversion_single)?;
    m.function_meta(lechatelier_temperature)?;
    m.function_meta(lechatelier_pressure_shift)?;

    Ok(m)
}
