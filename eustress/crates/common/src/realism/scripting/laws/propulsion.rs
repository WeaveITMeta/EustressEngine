//! Rune bindings for the propulsion laws.
//!
//! Exposed to scripts under `eustress::realism::propulsion::*`. Each binding is
//! a thin f64 wrapper around an f32 kernel law in
//! `crate::realism::propulsion::{rockets, jets, propellers, electric}`, because
//! Rune works in f64 while the realism kernel is f32.
//!
//! Only all-scalar-parameter, scalar-return laws are bound here; kernel
//! functions taking slices (e.g. `rockets::multistage_delta_v`) are omitted.
//! Names that collide across the source files are disambiguated with a
//! domain prefix (`rocket_thrust`, `rocket_specific_impulse`).

use rune::{ContextError, Module};
use crate::realism::propulsion::{rockets, jets, propellers, electric};

// --- rockets ---------------------------------------------------------------

#[rune::function]
fn tsiolkovsky_delta_v(exhaust_velocity: f64, mass_initial: f64, mass_final: f64) -> f64 {
    rockets::tsiolkovsky_delta_v(exhaust_velocity as f32, mass_initial as f32, mass_final as f32)
        as f64
}

#[rune::function]
fn delta_v_from_isp(isp: f64, mass_initial: f64, mass_final: f64) -> f64 {
    rockets::delta_v_from_isp(isp as f32, mass_initial as f32, mass_final as f32) as f64
}

#[rune::function]
fn rocket_specific_impulse(thrust: f64, mass_flow_rate: f64) -> f64 {
    rockets::specific_impulse(thrust as f32, mass_flow_rate as f32) as f64
}

#[rune::function]
fn exhaust_velocity_from_isp(isp: f64) -> f64 {
    rockets::exhaust_velocity_from_isp(isp as f32) as f64
}

#[rune::function]
fn mass_ratio(delta_v: f64, exhaust_velocity: f64) -> f64 {
    rockets::mass_ratio(delta_v as f32, exhaust_velocity as f32) as f64
}

#[rune::function]
fn propellant_mass_fraction(delta_v: f64, exhaust_velocity: f64) -> f64 {
    rockets::propellant_mass_fraction(delta_v as f32, exhaust_velocity as f32) as f64
}

#[rune::function]
fn rocket_thrust(
    mass_flow_rate: f64,
    exhaust_velocity: f64,
    p_exit: f64,
    p_ambient: f64,
    area_exit: f64,
) -> f64 {
    rockets::thrust(
        mass_flow_rate as f32,
        exhaust_velocity as f32,
        p_exit as f32,
        p_ambient as f32,
        area_exit as f32,
    ) as f64
}

#[rune::function]
fn nozzle_exit_velocity(
    t_chamber: f64,
    gamma: f64,
    r_specific: f64,
    p_exit: f64,
    p_chamber: f64,
) -> f64 {
    rockets::nozzle_exit_velocity(
        t_chamber as f32,
        gamma as f32,
        r_specific as f32,
        p_exit as f32,
        p_chamber as f32,
    ) as f64
}

#[rune::function]
fn area_ratio(mach_exit: f64, gamma: f64) -> f64 {
    rockets::area_ratio(mach_exit as f32, gamma as f32) as f64
}

#[rune::function]
fn characteristic_velocity(t_chamber: f64, gamma: f64, r_specific: f64) -> f64 {
    rockets::characteristic_velocity(t_chamber as f32, gamma as f32, r_specific as f32) as f64
}

#[rune::function]
fn thrust_to_weight(thrust: f64, mass: f64) -> f64 {
    rockets::thrust_to_weight(thrust as f32, mass as f32) as f64
}

// --- jets ------------------------------------------------------------------

#[rune::function]
fn turbojet_thrust(mass_flow: f64, v_exhaust: f64, v_inlet: f64) -> f64 {
    jets::turbojet_thrust(mass_flow as f32, v_exhaust as f32, v_inlet as f32) as f64
}

#[rune::function]
fn thrust_with_fuel(air_flow: f64, fuel_flow: f64, v_exhaust: f64, v_inlet: f64) -> f64 {
    jets::thrust_with_fuel(air_flow as f32, fuel_flow as f32, v_exhaust as f32, v_inlet as f32)
        as f64
}

#[rune::function]
fn specific_thrust(thrust: f64, air_mass_flow: f64) -> f64 {
    jets::specific_thrust(thrust as f32, air_mass_flow as f32) as f64
}

#[rune::function]
fn tsfc(fuel_flow: f64, thrust: f64) -> f64 {
    jets::tsfc(fuel_flow as f32, thrust as f32) as f64
}

#[rune::function]
fn propulsive_efficiency(v_exhaust: f64, v_flight: f64) -> f64 {
    jets::propulsive_efficiency(v_exhaust as f32, v_flight as f32) as f64
}

#[rune::function]
fn thermal_efficiency_jet(
    v_exhaust: f64,
    v_flight: f64,
    fuel_heating_value: f64,
    fuel_air_ratio: f64,
) -> f64 {
    jets::thermal_efficiency_jet(
        v_exhaust as f32,
        v_flight as f32,
        fuel_heating_value as f32,
        fuel_air_ratio as f32,
    ) as f64
}

#[rune::function]
fn overall_efficiency(propulsive: f64, thermal: f64) -> f64 {
    jets::overall_efficiency(propulsive as f32, thermal as f32) as f64
}

#[rune::function]
fn bypass_thrust(
    core_flow: f64,
    fan_flow: f64,
    v_core: f64,
    v_fan: f64,
    v_inlet: f64,
) -> f64 {
    jets::bypass_thrust(
        core_flow as f32,
        fan_flow as f32,
        v_core as f32,
        v_fan as f32,
        v_inlet as f32,
    ) as f64
}

#[rune::function]
fn bypass_ratio(fan_flow: f64, core_flow: f64) -> f64 {
    jets::bypass_ratio(fan_flow as f32, core_flow as f32) as f64
}

// --- propellers ------------------------------------------------------------

#[rune::function]
fn thrust_coefficient(thrust: f64, density: f64, rps: f64, diameter: f64) -> f64 {
    propellers::thrust_coefficient(thrust as f32, density as f32, rps as f32, diameter as f32)
        as f64
}

#[rune::function]
fn power_coefficient(power: f64, density: f64, rps: f64, diameter: f64) -> f64 {
    propellers::power_coefficient(power as f32, density as f32, rps as f32, diameter as f32) as f64
}

#[rune::function]
fn advance_ratio(velocity: f64, rps: f64, diameter: f64) -> f64 {
    propellers::advance_ratio(velocity as f32, rps as f32, diameter as f32) as f64
}

#[rune::function]
fn propeller_efficiency(thrust_coeff: f64, power_coeff: f64, advance_ratio: f64) -> f64 {
    propellers::propeller_efficiency(thrust_coeff as f32, power_coeff as f32, advance_ratio as f32)
        as f64
}

#[rune::function]
fn thrust_from_coefficient(ct: f64, density: f64, rps: f64, diameter: f64) -> f64 {
    propellers::thrust_from_coefficient(ct as f32, density as f32, rps as f32, diameter as f32)
        as f64
}

#[rune::function]
fn power_from_coefficient(cp: f64, density: f64, rps: f64, diameter: f64) -> f64 {
    propellers::power_from_coefficient(cp as f32, density as f32, rps as f32, diameter as f32)
        as f64
}

#[rune::function]
fn disk_actuator_induced_velocity(thrust: f64, density: f64, disk_area: f64) -> f64 {
    propellers::disk_actuator_induced_velocity(thrust as f32, density as f32, disk_area as f32)
        as f64
}

#[rune::function]
fn ideal_hover_power(thrust: f64, density: f64, disk_area: f64) -> f64 {
    propellers::ideal_hover_power(thrust as f32, density as f32, disk_area as f32) as f64
}

#[rune::function]
fn figure_of_merit(ideal_power: f64, actual_power: f64) -> f64 {
    propellers::figure_of_merit(ideal_power as f32, actual_power as f32) as f64
}

// --- electric --------------------------------------------------------------

#[rune::function]
fn ion_exhaust_velocity(accel_voltage: f64, charge_to_mass: f64) -> f64 {
    electric::ion_exhaust_velocity(accel_voltage as f32, charge_to_mass as f32) as f64
}

#[rune::function]
fn ion_thrust(mass_flow: f64, exhaust_velocity: f64) -> f64 {
    electric::ion_thrust(mass_flow as f32, exhaust_velocity as f32) as f64
}

#[rune::function]
fn thrust_from_beam_current(
    beam_current: f64,
    accel_voltage: f64,
    ion_mass: f64,
    charge: f64,
) -> f64 {
    electric::thrust_from_beam_current(
        beam_current as f32,
        accel_voltage as f32,
        ion_mass as f32,
        charge as f32,
    ) as f64
}

#[rune::function]
fn thrust_power(thrust: f64, exhaust_velocity: f64) -> f64 {
    electric::thrust_power(thrust as f32, exhaust_velocity as f32) as f64
}

#[rune::function]
fn thrust_to_power_ratio(thrust: f64, input_power: f64) -> f64 {
    electric::thrust_to_power_ratio(thrust as f32, input_power as f32) as f64
}

#[rune::function]
fn total_efficiency_electric(thrust: f64, exhaust_velocity: f64, input_power: f64) -> f64 {
    electric::total_efficiency_electric(thrust as f32, exhaust_velocity as f32, input_power as f32)
        as f64
}

#[rune::function]
fn specific_impulse_electric(exhaust_velocity: f64) -> f64 {
    electric::specific_impulse_electric(exhaust_velocity as f32) as f64
}

#[rune::function]
fn propellant_utilization(beam_current: f64, mass_flow: f64, charge: f64, ion_mass: f64) -> f64 {
    electric::propellant_utilization(
        beam_current as f32,
        mass_flow as f32,
        charge as f32,
        ion_mass as f32,
    ) as f64
}

/// Build the `eustress::realism::propulsion` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "propulsion"])?;
    // rockets
    m.function_meta(tsiolkovsky_delta_v)?;
    m.function_meta(delta_v_from_isp)?;
    m.function_meta(rocket_specific_impulse)?;
    m.function_meta(exhaust_velocity_from_isp)?;
    m.function_meta(mass_ratio)?;
    m.function_meta(propellant_mass_fraction)?;
    m.function_meta(rocket_thrust)?;
    m.function_meta(nozzle_exit_velocity)?;
    m.function_meta(area_ratio)?;
    m.function_meta(characteristic_velocity)?;
    m.function_meta(thrust_to_weight)?;
    // jets
    m.function_meta(turbojet_thrust)?;
    m.function_meta(thrust_with_fuel)?;
    m.function_meta(specific_thrust)?;
    m.function_meta(tsfc)?;
    m.function_meta(propulsive_efficiency)?;
    m.function_meta(thermal_efficiency_jet)?;
    m.function_meta(overall_efficiency)?;
    m.function_meta(bypass_thrust)?;
    m.function_meta(bypass_ratio)?;
    // propellers
    m.function_meta(thrust_coefficient)?;
    m.function_meta(power_coefficient)?;
    m.function_meta(advance_ratio)?;
    m.function_meta(propeller_efficiency)?;
    m.function_meta(thrust_from_coefficient)?;
    m.function_meta(power_from_coefficient)?;
    m.function_meta(disk_actuator_induced_velocity)?;
    m.function_meta(ideal_hover_power)?;
    m.function_meta(figure_of_merit)?;
    // electric
    m.function_meta(ion_exhaust_velocity)?;
    m.function_meta(ion_thrust)?;
    m.function_meta(thrust_from_beam_current)?;
    m.function_meta(thrust_power)?;
    m.function_meta(thrust_to_power_ratio)?;
    m.function_meta(total_efficiency_electric)?;
    m.function_meta(specific_impulse_electric)?;
    m.function_meta(propellant_utilization)?;
    Ok(m)
}
