//! Rune bindings for the plasma physics laws.
//!
//! Exposed to scripts under `eustress::realism::plasma::*`. Each binding is a
//! thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::plasma::{debye, mhd, fusion}`, because Rune works in f64
//! while the realism kernel is f32. Functions whose kernel returns `bool`
//! (`is_plasma`, `frozen_in_condition`, `lawson_criterion_met`,
//! `ignition_condition`) keep their `bool` return type.

use rune::{ContextError, Module};
use crate::realism::plasma::{debye, mhd, fusion};

// --- debye -----------------------------------------------------------------

#[rune::function]
fn debye_length(electron_density: f64, electron_temperature_k: f64) -> f64 {
    debye::debye_length(electron_density as f32, electron_temperature_k as f32) as f64
}

#[rune::function]
fn plasma_frequency(electron_density: f64) -> f64 {
    debye::plasma_frequency(electron_density as f32) as f64
}

#[rune::function]
fn plasma_frequency_hz(electron_density: f64) -> f64 {
    debye::plasma_frequency_hz(electron_density as f32) as f64
}

#[rune::function]
fn larmor_radius(mass: f64, perpendicular_velocity: f64, charge: f64, magnetic_field: f64) -> f64 {
    debye::larmor_radius(
        mass as f32,
        perpendicular_velocity as f32,
        charge as f32,
        magnetic_field as f32,
    ) as f64
}

#[rune::function]
fn cyclotron_frequency(charge: f64, magnetic_field: f64, mass: f64) -> f64 {
    debye::cyclotron_frequency(charge as f32, magnetic_field as f32, mass as f32) as f64
}

#[rune::function]
fn plasma_parameter(electron_density: f64, electron_temperature_k: f64) -> f64 {
    debye::plasma_parameter(electron_density as f32, electron_temperature_k as f32) as f64
}

#[rune::function]
fn coulomb_logarithm(electron_density: f64, electron_temperature_k: f64) -> f64 {
    debye::coulomb_logarithm(electron_density as f32, electron_temperature_k as f32) as f64
}

#[rune::function]
fn thermal_velocity(temperature_k: f64, mass: f64) -> f64 {
    debye::thermal_velocity(temperature_k as f32, mass as f32) as f64
}

#[rune::function]
fn is_plasma(debye_length: f64, system_size: f64, plasma_parameter: f64) -> bool {
    debye::is_plasma(debye_length as f32, system_size as f32, plasma_parameter as f32)
}

// --- mhd -------------------------------------------------------------------

#[rune::function]
fn alfven_speed(magnetic_field: f64, density: f64) -> f64 {
    mhd::alfven_speed(magnetic_field as f32, density as f32) as f64
}

#[rune::function]
fn magnetic_pressure(magnetic_field: f64) -> f64 {
    mhd::magnetic_pressure(magnetic_field as f32) as f64
}

#[rune::function]
fn plasma_beta(thermal_pressure: f64, magnetic_field: f64) -> f64 {
    mhd::plasma_beta(thermal_pressure as f32, magnetic_field as f32) as f64
}

#[rune::function]
fn magnetic_reynolds_number(velocity: f64, length: f64, magnetic_diffusivity: f64) -> f64 {
    mhd::magnetic_reynolds_number(velocity as f32, length as f32, magnetic_diffusivity as f32) as f64
}

#[rune::function]
fn magnetic_diffusivity(resistivity: f64) -> f64 {
    mhd::magnetic_diffusivity(resistivity as f32) as f64
}

#[rune::function]
fn magnetic_tension(magnetic_field: f64, radius_of_curvature: f64) -> f64 {
    mhd::magnetic_tension(magnetic_field as f32, radius_of_curvature as f32) as f64
}

#[rune::function]
fn magnetosonic_speed(sound_speed: f64, alfven_speed: f64) -> f64 {
    mhd::magnetosonic_speed(sound_speed as f32, alfven_speed as f32) as f64
}

#[rune::function]
fn magnetic_energy_density(magnetic_field: f64) -> f64 {
    mhd::magnetic_energy_density(magnetic_field as f32) as f64
}

#[rune::function]
fn frozen_in_condition(magnetic_reynolds: f64) -> bool {
    mhd::frozen_in_condition(magnetic_reynolds as f32)
}

// --- fusion ----------------------------------------------------------------

#[rune::function]
fn lawson_triple_product(density: f64, temperature_kev: f64, confinement_time: f64) -> f64 {
    fusion::lawson_triple_product(density as f32, temperature_kev as f32, confinement_time as f32)
        as f64
}

#[rune::function]
fn lawson_criterion_met(triple_product: f64) -> bool {
    fusion::lawson_criterion_met(triple_product as f32)
}

#[rune::function]
fn dt_reactivity(temperature_kev: f64) -> f64 {
    fusion::dt_reactivity(temperature_kev as f32) as f64
}

#[rune::function]
fn fusion_power_density(
    n_deuterium: f64,
    n_tritium: f64,
    reactivity: f64,
    energy_per_reaction_joules: f64,
) -> f64 {
    fusion::fusion_power_density(
        n_deuterium as f32,
        n_tritium as f32,
        reactivity as f32,
        energy_per_reaction_joules as f32,
    ) as f64
}

#[rune::function]
fn dt_energy_per_reaction() -> f64 {
    fusion::dt_energy_per_reaction() as f64
}

#[rune::function]
fn fusion_gain_q(fusion_power: f64, heating_power: f64) -> f64 {
    fusion::fusion_gain_q(fusion_power as f32, heating_power as f32) as f64
}

#[rune::function]
fn ignition_condition(q: f64) -> bool {
    fusion::ignition_condition(q as f32)
}

#[rune::function]
fn ideal_ignition_temperature_kev() -> f64 {
    fusion::ideal_ignition_temperature_kev() as f64
}

#[rune::function]
fn coulomb_barrier_energy(z1: f64, z2: f64, separation: f64) -> f64 {
    fusion::coulomb_barrier_energy(z1 as f32, z2 as f32, separation as f32) as f64
}

/// Build the `eustress::realism::plasma` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "plasma"])?;
    // debye
    m.function_meta(debye_length)?;
    m.function_meta(plasma_frequency)?;
    m.function_meta(plasma_frequency_hz)?;
    m.function_meta(larmor_radius)?;
    m.function_meta(cyclotron_frequency)?;
    m.function_meta(plasma_parameter)?;
    m.function_meta(coulomb_logarithm)?;
    m.function_meta(thermal_velocity)?;
    m.function_meta(is_plasma)?;
    // mhd
    m.function_meta(alfven_speed)?;
    m.function_meta(magnetic_pressure)?;
    m.function_meta(plasma_beta)?;
    m.function_meta(magnetic_reynolds_number)?;
    m.function_meta(magnetic_diffusivity)?;
    m.function_meta(magnetic_tension)?;
    m.function_meta(magnetosonic_speed)?;
    m.function_meta(magnetic_energy_density)?;
    m.function_meta(frozen_in_condition)?;
    // fusion
    m.function_meta(lawson_triple_product)?;
    m.function_meta(lawson_criterion_met)?;
    m.function_meta(dt_reactivity)?;
    m.function_meta(fusion_power_density)?;
    m.function_meta(dt_energy_per_reaction)?;
    m.function_meta(fusion_gain_q)?;
    m.function_meta(ignition_condition)?;
    m.function_meta(ideal_ignition_temperature_kev)?;
    m.function_meta(coulomb_barrier_energy)?;
    Ok(m)
}
