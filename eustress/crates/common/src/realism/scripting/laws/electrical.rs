//! Rune bindings for the electrical / circuit laws.
//!
//! Exposed to scripts under `eustress::realism::electrical::*`. Each binding is
//! a thin f64 wrapper around the f32 kernel law in
//! `crate::realism::laws::electromagnetism::circuits`, because Rune works in
//! f64 while the realism kernel is f32.

use rune::{ContextError, Module};
use crate::realism::laws::electromagnetism::circuits as c;

#[rune::function]
fn ohm_current(voltage: f64, resistance: f64) -> f64 {
    c::ohm_current(voltage as f32, resistance as f32) as f64
}

#[rune::function]
fn ohm_voltage(current: f64, resistance: f64) -> f64 {
    c::ohm_voltage(current as f32, resistance as f32) as f64
}

#[rune::function]
fn ohm_resistance(voltage: f64, current: f64) -> f64 {
    c::ohm_resistance(voltage as f32, current as f32) as f64
}

#[rune::function]
fn rc_time_constant(r: f64, cap: f64) -> f64 {
    c::rc_time_constant(r as f32, cap as f32) as f64
}

#[rune::function]
fn rl_time_constant(r: f64, l: f64) -> f64 {
    c::rl_time_constant(r as f32, l as f32) as f64
}

#[rune::function]
fn rlc_natural_frequency(l: f64, cap: f64) -> f64 {
    c::rlc_natural_frequency(l as f32, cap as f32) as f64
}

#[rune::function]
fn rlc_damping_ratio(r: f64, l: f64, cap: f64) -> f64 {
    c::rlc_damping_ratio(r as f32, l as f32, cap as f32) as f64
}

#[rune::function]
fn rlc_quality_factor(r: f64, l: f64, cap: f64) -> f64 {
    c::rlc_quality_factor(r as f32, l as f32, cap as f32) as f64
}

#[rune::function]
fn rc_charge_voltage(v0: f64, t: f64, tau: f64) -> f64 {
    c::rc_charge_voltage(v0 as f32, t as f32, tau as f32) as f64
}

#[rune::function]
fn rc_discharge_voltage(v0: f64, t: f64, tau: f64) -> f64 {
    c::rc_discharge_voltage(v0 as f32, t as f32, tau as f32) as f64
}

#[rune::function]
fn rl_current_rise(i_final: f64, t: f64, tau: f64) -> f64 {
    c::rl_current_rise(i_final as f32, t as f32, tau as f32) as f64
}

#[rune::function]
fn capacitor_energy(cap: f64, v: f64) -> f64 {
    c::capacitor_energy(cap as f32, v as f32) as f64
}

#[rune::function]
fn inductor_energy(l: f64, i: f64) -> f64 {
    c::inductor_energy(l as f32, i as f32) as f64
}

#[rune::function]
fn capacitive_reactance(omega: f64, cap: f64) -> f64 {
    c::capacitive_reactance(omega as f32, cap as f32) as f64
}

#[rune::function]
fn inductive_reactance(omega: f64, l: f64) -> f64 {
    c::inductive_reactance(omega as f32, l as f32) as f64
}

#[rune::function]
fn rlc_series_impedance(r: f64, omega: f64, l: f64, cap: f64) -> f64 {
    c::rlc_series_impedance(r as f32, omega as f32, l as f32, cap as f32) as f64
}

#[rune::function]
fn rlc_series_phase(r: f64, omega: f64, l: f64, cap: f64) -> f64 {
    c::rlc_series_phase(r as f32, omega as f32, l as f32, cap as f32) as f64
}

#[rune::function]
fn resonant_frequency_hz(l: f64, cap: f64) -> f64 {
    c::resonant_frequency_hz(l as f32, cap as f32) as f64
}

#[rune::function]
fn power_dc(v: f64, i: f64) -> f64 {
    c::power_dc(v as f32, i as f32) as f64
}

#[rune::function]
fn power_resistive(i: f64, r: f64) -> f64 {
    c::power_resistive(i as f32, r as f32) as f64
}

#[rune::function]
fn power_ac_real(v_rms: f64, i_rms: f64, cos_phi: f64) -> f64 {
    c::power_ac_real(v_rms as f32, i_rms as f32, cos_phi as f32) as f64
}

#[rune::function]
fn power_factor(p_real: f64, s_apparent: f64) -> f64 {
    c::power_factor(p_real as f32, s_apparent as f32) as f64
}

#[rune::function]
fn voltage_divider(v_in: f64, r1: f64, r2: f64) -> f64 {
    c::voltage_divider(v_in as f32, r1 as f32, r2 as f32) as f64
}

#[rune::function]
fn current_divider(i_in: f64, r_branch: f64, r_total: f64) -> f64 {
    c::current_divider(i_in as f32, r_branch as f32, r_total as f32) as f64
}

/// Build the `eustress::realism::electrical` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "electrical"])?;
    m.function_meta(ohm_current)?;
    m.function_meta(ohm_voltage)?;
    m.function_meta(ohm_resistance)?;
    m.function_meta(rc_time_constant)?;
    m.function_meta(rl_time_constant)?;
    m.function_meta(rlc_natural_frequency)?;
    m.function_meta(rlc_damping_ratio)?;
    m.function_meta(rlc_quality_factor)?;
    m.function_meta(rc_charge_voltage)?;
    m.function_meta(rc_discharge_voltage)?;
    m.function_meta(rl_current_rise)?;
    m.function_meta(capacitor_energy)?;
    m.function_meta(inductor_energy)?;
    m.function_meta(capacitive_reactance)?;
    m.function_meta(inductive_reactance)?;
    m.function_meta(rlc_series_impedance)?;
    m.function_meta(rlc_series_phase)?;
    m.function_meta(resonant_frequency_hz)?;
    m.function_meta(power_dc)?;
    m.function_meta(power_resistive)?;
    m.function_meta(power_ac_real)?;
    m.function_meta(power_factor)?;
    m.function_meta(voltage_divider)?;
    m.function_meta(current_divider)?;
    Ok(m)
}
