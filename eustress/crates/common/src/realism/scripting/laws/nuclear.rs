//! Rune bindings for the nuclear physics laws.
//!
//! Exposed to scripts under `eustress::realism::nuclear::*`. Each binding is a
//! thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::nuclear::{decay, shielding, criticality}`, because Rune
//! works in f64 while the realism kernel is f32.
//!
//! Only all-scalar-parameter, scalar-return functions are bound. The tuple-
//! returning `decay::bateman_two_step` is intentionally not exposed here.

use rune::{ContextError, Module};
use crate::realism::nuclear::{decay, shielding, criticality};

// --- decay -----------------------------------------------------------------

#[rune::function]
fn decay_constant_from_half_life(half_life: f64) -> f64 {
    decay::decay_constant_from_half_life(half_life as f32) as f64
}

#[rune::function]
fn half_life_from_decay_constant(lambda: f64) -> f64 {
    decay::half_life_from_decay_constant(lambda as f32) as f64
}

#[rune::function]
fn remaining_nuclei(n0: f64, lambda: f64, time: f64) -> f64 {
    decay::remaining_nuclei(n0 as f32, lambda as f32, time as f32) as f64
}

#[rune::function]
fn activity(n: f64, lambda: f64) -> f64 {
    decay::activity(n as f32, lambda as f32) as f64
}

#[rune::function]
fn activity_from_mass(mass_grams: f64, molar_mass: f64, lambda: f64) -> f64 {
    decay::activity_from_mass(mass_grams as f32, molar_mass as f32, lambda as f32) as f64
}

#[rune::function]
fn decayed_fraction(lambda: f64, time: f64) -> f64 {
    decay::decayed_fraction(lambda as f32, time as f32) as f64
}

#[rune::function]
fn mean_lifetime(lambda: f64) -> f64 {
    decay::mean_lifetime(lambda as f32) as f64
}

#[rune::function]
fn secular_equilibrium_activity(parent_activity: f64) -> f64 {
    decay::secular_equilibrium_activity(parent_activity as f32) as f64
}

#[rune::function]
fn specific_activity(lambda: f64, molar_mass: f64) -> f64 {
    decay::specific_activity(lambda as f32, molar_mass as f32) as f64
}

#[rune::function]
fn carbon14_age(current_c14_fraction: f64) -> f64 {
    decay::carbon14_age(current_c14_fraction as f32) as f64
}

// --- shielding -------------------------------------------------------------

#[rune::function]
fn attenuation(incident_intensity: f64, linear_attenuation_coeff: f64, thickness: f64) -> f64 {
    shielding::attenuation(
        incident_intensity as f32,
        linear_attenuation_coeff as f32,
        thickness as f32,
    ) as f64
}

#[rune::function]
fn attenuation_with_buildup(incident: f64, mu: f64, thickness: f64, buildup_factor: f64) -> f64 {
    shielding::attenuation_with_buildup(
        incident as f32,
        mu as f32,
        thickness as f32,
        buildup_factor as f32,
    ) as f64
}

#[rune::function]
fn half_value_layer(mu: f64) -> f64 {
    shielding::half_value_layer(mu as f32) as f64
}

#[rune::function]
fn tenth_value_layer(mu: f64) -> f64 {
    shielding::tenth_value_layer(mu as f32) as f64
}

#[rune::function]
fn thickness_for_attenuation(mu: f64, attenuation_factor: f64) -> f64 {
    shielding::thickness_for_attenuation(mu as f32, attenuation_factor as f32) as f64
}

#[rune::function]
fn mass_attenuation_thickness(mass_attenuation_coeff: f64, density: f64, thickness: f64) -> f64 {
    shielding::mass_attenuation_thickness(
        mass_attenuation_coeff as f32,
        density as f32,
        thickness as f32,
    ) as f64
}

#[rune::function]
fn dose_rate_point_source(activity: f64, gamma_constant: f64, distance: f64) -> f64 {
    shielding::dose_rate_point_source(activity as f32, gamma_constant as f32, distance as f32) as f64
}

#[rune::function]
fn dose_equivalent(absorbed_dose_gray: f64, quality_factor: f64) -> f64 {
    shielding::dose_equivalent(absorbed_dose_gray as f32, quality_factor as f32) as f64
}

#[rune::function]
fn inverse_square_dose(dose1: f64, r1: f64, r2: f64) -> f64 {
    shielding::inverse_square_dose(dose1 as f32, r1 as f32, r2 as f32) as f64
}

#[rune::function]
fn shielding_layers_needed(initial_dose: f64, target_dose: f64, hvl: f64) -> f64 {
    shielding::shielding_layers_needed(initial_dose as f32, target_dose as f32, hvl as f32) as f64
}

// --- criticality -----------------------------------------------------------

#[rune::function]
fn four_factor_k_infinity(eta: f64, epsilon: f64, p_resonance: f64, f_thermal: f64) -> f64 {
    criticality::four_factor_k_infinity(
        eta as f32,
        epsilon as f32,
        p_resonance as f32,
        f_thermal as f32,
    ) as f64
}

#[rune::function]
fn six_factor_k_effective(k_infinity: f64, fast_nonleak: f64, thermal_nonleak: f64) -> f64 {
    criticality::six_factor_k_effective(
        k_infinity as f32,
        fast_nonleak as f32,
        thermal_nonleak as f32,
    ) as f64
}

#[rune::function]
fn reactivity(k_effective: f64) -> f64 {
    criticality::reactivity(k_effective as f32) as f64
}

#[rune::function]
fn reactivity_dollars(reactivity: f64, beta: f64) -> f64 {
    criticality::reactivity_dollars(reactivity as f32, beta as f32) as f64
}

#[rune::function]
fn migration_area(diffusion_area: f64, slowing_down_area: f64) -> f64 {
    criticality::migration_area(diffusion_area as f32, slowing_down_area as f32) as f64
}

#[rune::function]
fn geometric_buckling_sphere(radius: f64) -> f64 {
    criticality::geometric_buckling_sphere(radius as f32) as f64
}

#[rune::function]
fn geometric_buckling_cylinder(radius: f64, height: f64) -> f64 {
    criticality::geometric_buckling_cylinder(radius as f32, height as f32) as f64
}

#[rune::function]
fn geometric_buckling_cube(side: f64) -> f64 {
    criticality::geometric_buckling_cube(side as f32) as f64
}

#[rune::function]
fn critical_radius_sphere(material_buckling: f64) -> f64 {
    criticality::critical_radius_sphere(material_buckling as f32) as f64
}

#[rune::function]
fn thermal_nonleakage(diffusion_area: f64, buckling: f64) -> f64 {
    criticality::thermal_nonleakage(diffusion_area as f32, buckling as f32) as f64
}

#[rune::function]
fn fast_nonleakage(slowing_down_area: f64, buckling: f64) -> f64 {
    criticality::fast_nonleakage(slowing_down_area as f32, buckling as f32) as f64
}

#[rune::function]
fn doubling_time(reactor_period: f64) -> f64 {
    criticality::doubling_time(reactor_period as f32) as f64
}

#[rune::function]
fn reactor_period(reactivity: f64, beta: f64, neutron_lifetime: f64, decay_constant: f64) -> f64 {
    criticality::reactor_period(
        reactivity as f32,
        beta as f32,
        neutron_lifetime as f32,
        decay_constant as f32,
    ) as f64
}

/// Build the `eustress::realism::nuclear` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "nuclear"])?;
    // decay
    m.function_meta(decay_constant_from_half_life)?;
    m.function_meta(half_life_from_decay_constant)?;
    m.function_meta(remaining_nuclei)?;
    m.function_meta(activity)?;
    m.function_meta(activity_from_mass)?;
    m.function_meta(decayed_fraction)?;
    m.function_meta(mean_lifetime)?;
    m.function_meta(secular_equilibrium_activity)?;
    m.function_meta(specific_activity)?;
    m.function_meta(carbon14_age)?;
    // shielding
    m.function_meta(attenuation)?;
    m.function_meta(attenuation_with_buildup)?;
    m.function_meta(half_value_layer)?;
    m.function_meta(tenth_value_layer)?;
    m.function_meta(thickness_for_attenuation)?;
    m.function_meta(mass_attenuation_thickness)?;
    m.function_meta(dose_rate_point_source)?;
    m.function_meta(dose_equivalent)?;
    m.function_meta(inverse_square_dose)?;
    m.function_meta(shielding_layers_needed)?;
    // criticality
    m.function_meta(four_factor_k_infinity)?;
    m.function_meta(six_factor_k_effective)?;
    m.function_meta(reactivity)?;
    m.function_meta(reactivity_dollars)?;
    m.function_meta(migration_area)?;
    m.function_meta(geometric_buckling_sphere)?;
    m.function_meta(geometric_buckling_cylinder)?;
    m.function_meta(geometric_buckling_cube)?;
    m.function_meta(critical_radius_sphere)?;
    m.function_meta(thermal_nonleakage)?;
    m.function_meta(fast_nonleakage)?;
    m.function_meta(doubling_time)?;
    m.function_meta(reactor_period)?;
    Ok(m)
}
