//! Rune bindings for the structural-engineering laws.
//!
//! Exposed to scripts under `eustress::realism::structures::*`. Each binding is
//! a thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::structures` (beams, columns, fatigue, composites), because
//! Rune works in f64 while the realism kernel is f32.
//!
//! Only stateless scalar-in / scalar-out laws are exposed. Functions taking
//! slices (e.g. `miners_rule`) or string keys (e.g. `end_condition_k`) are not
//! bound here.

use rune::{ContextError, Module};
use crate::realism::structures::{beams, columns, fatigue, composites};

// ============================================================================
// Beams — stress, deflection, moments, section properties, vibration
// ============================================================================

#[rune::function]
fn bending_stress(moment: f64, y: f64, i_moment: f64) -> f64 {
    beams::bending_stress(moment as f32, y as f32, i_moment as f32) as f64
}

#[rune::function]
fn shear_stress(shear: f64, q_first_moment: f64, i_moment: f64, width: f64) -> f64 {
    beams::shear_stress(shear as f32, q_first_moment as f32, i_moment as f32, width as f32) as f64
}

#[rune::function]
fn beam_deflection_cantilever_end_load(p: f64, length: f64, e: f64, i: f64, x: f64) -> f64 {
    beams::beam_deflection_cantilever_end_load(p as f32, length as f32, e as f32, i as f32, x as f32)
        as f64
}

#[rune::function]
fn beam_deflection_cantilever_max(p: f64, length: f64, e: f64, i: f64) -> f64 {
    beams::beam_deflection_cantilever_max(p as f32, length as f32, e as f32, i as f32) as f64
}

#[rune::function]
fn beam_deflection_simply_supported_center(p: f64, length: f64, e: f64, i: f64) -> f64 {
    beams::beam_deflection_simply_supported_center(p as f32, length as f32, e as f32, i as f32)
        as f64
}

#[rune::function]
fn beam_deflection_udl_simply_supported(w: f64, length: f64, e: f64, i: f64) -> f64 {
    beams::beam_deflection_udl_simply_supported(w as f32, length as f32, e as f32, i as f32) as f64
}

#[rune::function]
fn max_moment_cantilever_end(p: f64, length: f64) -> f64 {
    beams::max_moment_cantilever_end(p as f32, length as f32) as f64
}

#[rune::function]
fn max_moment_simply_supported_center(p: f64, length: f64) -> f64 {
    beams::max_moment_simply_supported_center(p as f32, length as f32) as f64
}

#[rune::function]
fn max_moment_udl(w: f64, length: f64) -> f64 {
    beams::max_moment_udl(w as f32, length as f32) as f64
}

#[rune::function]
fn section_modulus(i_moment: f64, c_max: f64) -> f64 {
    beams::section_modulus(i_moment as f32, c_max as f32) as f64
}

#[rune::function]
fn moment_of_area_rectangle(b: f64, h: f64) -> f64 {
    beams::moment_of_area_rectangle(b as f32, h as f32) as f64
}

#[rune::function]
fn moment_of_area_circle(r: f64) -> f64 {
    beams::moment_of_area_circle(r as f32) as f64
}

#[rune::function]
fn moment_of_area_hollow_circle(r_outer: f64, r_inner: f64) -> f64 {
    beams::moment_of_area_hollow_circle(r_outer as f32, r_inner as f32) as f64
}

#[rune::function]
fn moment_of_area_i_beam(b: f64, h: f64, t_web: f64, t_flange: f64) -> f64 {
    beams::moment_of_area_i_beam(b as f32, h as f32, t_web as f32, t_flange as f32) as f64
}

#[rune::function]
fn natural_frequency_cantilever(e: f64, i: f64, rho_lin: f64, length: f64) -> f64 {
    beams::natural_frequency_cantilever(e as f32, i as f32, rho_lin as f32, length as f32) as f64
}

#[rune::function]
fn natural_frequency_simply_supported(e: f64, i: f64, rho_lin: f64, length: f64) -> f64 {
    beams::natural_frequency_simply_supported(e as f32, i as f32, rho_lin as f32, length as f32)
        as f64
}

// ============================================================================
// Columns — Euler buckling, slenderness, Johnson parabola
// ============================================================================

#[rune::function]
fn euler_critical_load(e: f64, i: f64, length: f64, k_factor: f64) -> f64 {
    columns::euler_critical_load(e as f32, i as f32, length as f32, k_factor as f32) as f64
}

#[rune::function]
fn euler_critical_stress(e: f64, slenderness: f64) -> f64 {
    columns::euler_critical_stress(e as f32, slenderness as f32) as f64
}

#[rune::function]
fn slenderness_ratio(k_factor: f64, length: f64, radius_gyration: f64) -> f64 {
    columns::slenderness_ratio(k_factor as f32, length as f32, radius_gyration as f32) as f64
}

#[rune::function]
fn radius_of_gyration(i_moment: f64, area: f64) -> f64 {
    columns::radius_of_gyration(i_moment as f32, area as f32) as f64
}

#[rune::function]
fn critical_slenderness(e: f64, yield_strength: f64) -> f64 {
    columns::critical_slenderness(e as f32, yield_strength as f32) as f64
}

#[rune::function]
fn johnson_buckling_stress(yield_strength: f64, e: f64, slenderness: f64) -> f64 {
    columns::johnson_buckling_stress(yield_strength as f32, e as f32, slenderness as f32) as f64
}

#[rune::function]
fn column_strength(e: f64, yield_strength: f64, slenderness: f64) -> f64 {
    columns::column_strength(e as f32, yield_strength as f32, slenderness as f32) as f64
}

#[rune::function]
fn buckling_safety_factor(p_critical: f64, p_applied: f64) -> f64 {
    columns::buckling_safety_factor(p_critical as f32, p_applied as f32) as f64
}

// ============================================================================
// Fatigue and fracture — S-N, mean stress, fracture mechanics
// ============================================================================

#[rune::function]
fn endurance_limit_steel(ultimate_strength: f64) -> f64 {
    fatigue::endurance_limit_steel(ultimate_strength as f32) as f64
}

#[rune::function]
fn basquin_stress_life(sigma_a: f64, sigma_f_prime: f64, b_exponent: f64) -> f64 {
    fatigue::basquin_stress_life(sigma_a as f32, sigma_f_prime as f32, b_exponent as f32) as f64
}

#[rune::function]
fn goodman_factor(sigma_a: f64, sigma_m: f64, sut: f64, se: f64) -> f64 {
    fatigue::goodman_factor(sigma_a as f32, sigma_m as f32, sut as f32, se as f32) as f64
}

#[rune::function]
fn soderberg_factor(sigma_a: f64, sigma_m: f64, sy: f64, se: f64) -> f64 {
    fatigue::soderberg_factor(sigma_a as f32, sigma_m as f32, sy as f32, se as f32) as f64
}

#[rune::function]
fn stress_intensity_factor(sigma: f64, crack_length: f64, geometry_factor: f64) -> f64 {
    fatigue::stress_intensity_factor(sigma as f32, crack_length as f32, geometry_factor as f32)
        as f64
}

#[rune::function]
fn paris_crack_growth_rate(c_const: f64, m_exp: f64, delta_k: f64) -> f64 {
    fatigue::paris_crack_growth_rate(c_const as f32, m_exp as f32, delta_k as f32) as f64
}

// NOTE: cycles_to_failure_paris (6 args) omitted — Rune 0.14 binds at most 5 args.

#[rune::function]
fn fracture_occurs(k: f64, k_ic: f64) -> bool {
    fatigue::fracture_occurs(k as f32, k_ic as f32)
}

#[rune::function]
fn critical_crack_length(k_ic: f64, sigma: f64, geometry_factor: f64) -> f64 {
    fatigue::critical_crack_length(k_ic as f32, sigma as f32, geometry_factor as f32) as f64
}

// ============================================================================
// Composites — rule of mixtures, Halpin-Tsai, Tsai-Hill, specific properties
// ============================================================================

#[rune::function]
fn rule_of_mixtures_modulus(e_fiber: f64, e_matrix: f64, fiber_volume_fraction: f64) -> f64 {
    composites::rule_of_mixtures_modulus(e_fiber as f32, e_matrix as f32, fiber_volume_fraction as f32)
        as f64
}

#[rune::function]
fn inverse_rule_of_mixtures_transverse(e_fiber: f64, e_matrix: f64, vf: f64) -> f64 {
    composites::inverse_rule_of_mixtures_transverse(e_fiber as f32, e_matrix as f32, vf as f32) as f64
}

#[rune::function]
fn halpin_tsai_modulus(e_fiber: f64, e_matrix: f64, vf: f64, xi: f64) -> f64 {
    composites::halpin_tsai_modulus(e_fiber as f32, e_matrix as f32, vf as f32, xi as f32) as f64
}

#[rune::function]
fn rule_of_mixtures_density(rho_fiber: f64, rho_matrix: f64, vf: f64) -> f64 {
    composites::rule_of_mixtures_density(rho_fiber as f32, rho_matrix as f32, vf as f32) as f64
}

#[rune::function]
fn rule_of_mixtures_strength(sigma_fiber: f64, sigma_matrix: f64, vf: f64) -> f64 {
    composites::rule_of_mixtures_strength(sigma_fiber as f32, sigma_matrix as f32, vf as f32) as f64
}

#[rune::function]
fn composite_poisson_ratio(nu_fiber: f64, nu_matrix: f64, vf: f64) -> f64 {
    composites::composite_poisson_ratio(nu_fiber as f32, nu_matrix as f32, vf as f32) as f64
}

#[rune::function]
fn shear_modulus_composite(g_fiber: f64, g_matrix: f64, vf: f64) -> f64 {
    composites::shear_modulus_composite(g_fiber as f32, g_matrix as f32, vf as f32) as f64
}

// NOTE: tsai_hill_index (6 args) omitted — Rune 0.14 binds at most 5 args.

#[rune::function]
fn specific_modulus(e: f64, density: f64) -> f64 {
    composites::specific_modulus(e as f32, density as f32) as f64
}

#[rune::function]
fn specific_strength(strength: f64, density: f64) -> f64 {
    composites::specific_strength(strength as f32, density as f32) as f64
}

/// Build the `eustress::realism::structures` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "structures"])?;
    // Beams
    m.function_meta(bending_stress)?;
    m.function_meta(shear_stress)?;
    m.function_meta(beam_deflection_cantilever_end_load)?;
    m.function_meta(beam_deflection_cantilever_max)?;
    m.function_meta(beam_deflection_simply_supported_center)?;
    m.function_meta(beam_deflection_udl_simply_supported)?;
    m.function_meta(max_moment_cantilever_end)?;
    m.function_meta(max_moment_simply_supported_center)?;
    m.function_meta(max_moment_udl)?;
    m.function_meta(section_modulus)?;
    m.function_meta(moment_of_area_rectangle)?;
    m.function_meta(moment_of_area_circle)?;
    m.function_meta(moment_of_area_hollow_circle)?;
    m.function_meta(moment_of_area_i_beam)?;
    m.function_meta(natural_frequency_cantilever)?;
    m.function_meta(natural_frequency_simply_supported)?;
    // Columns
    m.function_meta(euler_critical_load)?;
    m.function_meta(euler_critical_stress)?;
    m.function_meta(slenderness_ratio)?;
    m.function_meta(radius_of_gyration)?;
    m.function_meta(critical_slenderness)?;
    m.function_meta(johnson_buckling_stress)?;
    m.function_meta(column_strength)?;
    m.function_meta(buckling_safety_factor)?;
    // Fatigue and fracture
    m.function_meta(endurance_limit_steel)?;
    m.function_meta(basquin_stress_life)?;
    m.function_meta(goodman_factor)?;
    m.function_meta(soderberg_factor)?;
    m.function_meta(stress_intensity_factor)?;
    m.function_meta(paris_crack_growth_rate)?;
    m.function_meta(fracture_occurs)?;
    m.function_meta(critical_crack_length)?;
    // Composites
    m.function_meta(rule_of_mixtures_modulus)?;
    m.function_meta(inverse_rule_of_mixtures_transverse)?;
    m.function_meta(halpin_tsai_modulus)?;
    m.function_meta(rule_of_mixtures_density)?;
    m.function_meta(rule_of_mixtures_strength)?;
    m.function_meta(composite_poisson_ratio)?;
    m.function_meta(shear_modulus_composite)?;
    m.function_meta(specific_modulus)?;
    m.function_meta(specific_strength)?;
    Ok(m)
}
