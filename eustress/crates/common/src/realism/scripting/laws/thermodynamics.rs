//! Rune bindings for the thermodynamics laws.
//!
//! Exposed to scripts under `eustress::realism::thermodynamics::*`. Each binding
//! is a thin f64 wrapper around the f32 kernel law in
//! `crate::realism::laws::thermodynamics`, because Rune works in f64 while the
//! realism kernel is f32.
//!
//! Only the all-scalar-parameter, scalar-returning laws are bound here. The
//! `WaterPhase` enum, the `water_phase` classifier, and the
//! `ThermodynamicStateData` struct (and its methods) are intentionally not
//! exposed.

use rune::{ContextError, Module};
use crate::realism::laws::thermodynamics as thermo;

// ----------------------------------------------------------------------------
// Ideal gas law
// ----------------------------------------------------------------------------

#[rune::function]
fn ideal_gas_pressure(n: f64, t: f64, v: f64) -> f64 {
    thermo::ideal_gas_pressure(n as f32, t as f32, v as f32) as f64
}

#[rune::function]
fn ideal_gas_volume(n: f64, t: f64, p: f64) -> f64 {
    thermo::ideal_gas_volume(n as f32, t as f32, p as f32) as f64
}

#[rune::function]
fn ideal_gas_temperature(p: f64, v: f64, n: f64) -> f64 {
    thermo::ideal_gas_temperature(p as f32, v as f32, n as f32) as f64
}

#[rune::function]
fn ideal_gas_moles(p: f64, v: f64, t: f64) -> f64 {
    thermo::ideal_gas_moles(p as f32, v as f32, t as f32) as f64
}

// ----------------------------------------------------------------------------
// Van der Waals (real gas)
// ----------------------------------------------------------------------------

#[rune::function]
fn van_der_waals_pressure(n: f64, t: f64, v: f64, a: f64, b: f64) -> f64 {
    thermo::van_der_waals_pressure(n as f32, t as f32, v as f32, a as f32, b as f32) as f64
}

// ----------------------------------------------------------------------------
// First law
// ----------------------------------------------------------------------------

#[rune::function]
fn internal_energy_change(heat_in: f64, work_out: f64) -> f64 {
    thermo::internal_energy_change(heat_in as f32, work_out as f32) as f64
}

#[rune::function]
fn work_isobaric(pressure: f64, delta_volume: f64) -> f64 {
    thermo::work_isobaric(pressure as f32, delta_volume as f32) as f64
}

#[rune::function]
fn work_isothermal(n: f64, t: f64, v1: f64, v2: f64) -> f64 {
    thermo::work_isothermal(n as f32, t as f32, v1 as f32, v2 as f32) as f64
}

#[rune::function]
fn work_adiabatic(p1: f64, v1: f64, p2: f64, v2: f64, gamma: f64) -> f64 {
    thermo::work_adiabatic(p1 as f32, v1 as f32, p2 as f32, v2 as f32, gamma as f32) as f64
}

// ----------------------------------------------------------------------------
// Heat capacity
// ----------------------------------------------------------------------------

#[rune::function]
fn heat_capacity_monatomic_cv(n: f64) -> f64 {
    thermo::heat_capacity_monatomic_cv(n as f32) as f64
}

#[rune::function]
fn heat_capacity_monatomic_cp(n: f64) -> f64 {
    thermo::heat_capacity_monatomic_cp(n as f32) as f64
}

#[rune::function]
fn heat_capacity_diatomic_cv(n: f64) -> f64 {
    thermo::heat_capacity_diatomic_cv(n as f32) as f64
}

#[rune::function]
fn heat_capacity_diatomic_cp(n: f64) -> f64 {
    thermo::heat_capacity_diatomic_cp(n as f32) as f64
}

#[rune::function]
fn heat_required(mass: f64, specific_heat: f64, delta_temp: f64) -> f64 {
    thermo::heat_required(mass as f32, specific_heat as f32, delta_temp as f32) as f64
}

#[rune::function]
fn temperature_change(heat: f64, mass: f64, specific_heat: f64) -> f64 {
    thermo::temperature_change(heat as f32, mass as f32, specific_heat as f32) as f64
}

// ----------------------------------------------------------------------------
// Second law / entropy
// ----------------------------------------------------------------------------

#[rune::function]
fn entropy_change_reversible(heat: f64, temperature: f64) -> f64 {
    thermo::entropy_change_reversible(heat as f32, temperature as f32) as f64
}

#[rune::function]
fn entropy_change_isothermal(n: f64, v1: f64, v2: f64) -> f64 {
    thermo::entropy_change_isothermal(n as f32, v1 as f32, v2 as f32) as f64
}

// NOTE: entropy_change_general (6 args) omitted — Rune 0.14 binds at most 5 args.

#[rune::function]
fn carnot_efficiency(t_cold: f64, t_hot: f64) -> f64 {
    thermo::carnot_efficiency(t_cold as f32, t_hot as f32) as f64
}

#[rune::function]
fn cop_refrigerator(t_cold: f64, t_hot: f64) -> f64 {
    thermo::cop_refrigerator(t_cold as f32, t_hot as f32) as f64
}

#[rune::function]
fn cop_heat_pump(t_cold: f64, t_hot: f64) -> f64 {
    thermo::cop_heat_pump(t_cold as f32, t_hot as f32) as f64
}

// ----------------------------------------------------------------------------
// Heat transfer
// ----------------------------------------------------------------------------

#[rune::function]
fn heat_conduction_rate(k: f64, area: f64, delta_temp: f64, thickness: f64) -> f64 {
    thermo::heat_conduction_rate(k as f32, area as f32, delta_temp as f32, thickness as f32) as f64
}

#[rune::function]
fn heat_convection_rate(h: f64, area: f64, t_surface: f64, t_fluid: f64) -> f64 {
    thermo::heat_convection_rate(h as f32, area as f32, t_surface as f32, t_fluid as f32) as f64
}

#[rune::function]
fn heat_radiation_rate(emissivity: f64, area: f64, t_surface: f64, t_environment: f64) -> f64 {
    thermo::heat_radiation_rate(emissivity as f32, area as f32, t_surface as f32, t_environment as f32) as f64
}

// ----------------------------------------------------------------------------
// Phase transitions
// ----------------------------------------------------------------------------

#[rune::function]
fn heat_phase_change(mass: f64, latent_heat: f64) -> f64 {
    thermo::heat_phase_change(mass as f32, latent_heat as f32) as f64
}

// ----------------------------------------------------------------------------
// Enthalpy and free energies
// ----------------------------------------------------------------------------

#[rune::function]
fn enthalpy(internal_energy: f64, pressure: f64, volume: f64) -> f64 {
    thermo::enthalpy(internal_energy as f32, pressure as f32, volume as f32) as f64
}

#[rune::function]
fn enthalpy_change_isobaric(heat_at_constant_pressure: f64) -> f64 {
    thermo::enthalpy_change_isobaric(heat_at_constant_pressure as f32) as f64
}

#[rune::function]
fn gibbs_free_energy(enthalpy: f64, temperature: f64, entropy: f64) -> f64 {
    thermo::gibbs_free_energy(enthalpy as f32, temperature as f32, entropy as f32) as f64
}

#[rune::function]
fn helmholtz_free_energy(internal_energy: f64, temperature: f64, entropy: f64) -> f64 {
    thermo::helmholtz_free_energy(internal_energy as f32, temperature as f32, entropy as f32) as f64
}

/// Build the `eustress::realism::thermodynamics` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "thermodynamics"])?;
    m.function_meta(ideal_gas_pressure)?;
    m.function_meta(ideal_gas_volume)?;
    m.function_meta(ideal_gas_temperature)?;
    m.function_meta(ideal_gas_moles)?;
    m.function_meta(van_der_waals_pressure)?;
    m.function_meta(internal_energy_change)?;
    m.function_meta(work_isobaric)?;
    m.function_meta(work_isothermal)?;
    m.function_meta(work_adiabatic)?;
    m.function_meta(heat_capacity_monatomic_cv)?;
    m.function_meta(heat_capacity_monatomic_cp)?;
    m.function_meta(heat_capacity_diatomic_cv)?;
    m.function_meta(heat_capacity_diatomic_cp)?;
    m.function_meta(heat_required)?;
    m.function_meta(temperature_change)?;
    m.function_meta(entropy_change_reversible)?;
    m.function_meta(entropy_change_isothermal)?;
    m.function_meta(carnot_efficiency)?;
    m.function_meta(cop_refrigerator)?;
    m.function_meta(cop_heat_pump)?;
    m.function_meta(heat_conduction_rate)?;
    m.function_meta(heat_convection_rate)?;
    m.function_meta(heat_radiation_rate)?;
    m.function_meta(heat_phase_change)?;
    m.function_meta(enthalpy)?;
    m.function_meta(enthalpy_change_isobaric)?;
    m.function_meta(gibbs_free_energy)?;
    m.function_meta(helmholtz_free_energy)?;
    Ok(m)
}
