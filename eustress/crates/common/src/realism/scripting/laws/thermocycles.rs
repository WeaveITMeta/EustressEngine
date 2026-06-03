//! Rune bindings for the thermodynamic-cycle laws.
//!
//! Exposed to scripts under `eustress::realism::thermocycles::*`. Each binding
//! is a thin f64 wrapper around the f32 kernel laws in
//! `crate::realism::thermocycles::{rankine, brayton, otto, refrigeration,
//! heat_exchangers}`, because Rune works in f64 while the realism kernel is f32.
//!
//! Several cycle modules export functions with the same bare name
//! (`thermal_efficiency`, `turbine_work`, `compressor_work`, etc.). Because
//! every `#[rune::function]` in a module must have a unique name, each wrapper
//! is prefixed with its originating cycle (`rankine_*`, `brayton_*`, `otto_*`,
//! `refrigeration_*`, `hx_*`).

use rune::{ContextError, Module};
use crate::realism::thermocycles::{rankine, brayton, otto, refrigeration, heat_exchangers};

// ---------------------------------------------------------------------------
// Rankine steam power cycle
// ---------------------------------------------------------------------------

#[rune::function]
fn rankine_carnot_efficiency(t_hot: f64, t_cold: f64) -> f64 {
    rankine::carnot_efficiency(t_hot as f32, t_cold as f32) as f64
}

#[rune::function]
fn rankine_ideal_efficiency(
    h_turbine_in: f64,
    h_turbine_out: f64,
    h_pump_in: f64,
    h_pump_out: f64,
) -> f64 {
    rankine::rankine_ideal_efficiency(
        h_turbine_in as f32,
        h_turbine_out as f32,
        h_pump_in as f32,
        h_pump_out as f32,
    ) as f64
}

#[rune::function]
fn rankine_pump_work(v_specific: f64, p_high: f64, p_low: f64) -> f64 {
    rankine::pump_work(v_specific as f32, p_high as f32, p_low as f32) as f64
}

#[rune::function]
fn rankine_turbine_work(h_in: f64, h_out: f64) -> f64 {
    rankine::turbine_work(h_in as f32, h_out as f32) as f64
}

#[rune::function]
fn rankine_heat_added(h_boiler_out: f64, h_boiler_in: f64) -> f64 {
    rankine::heat_added(h_boiler_out as f32, h_boiler_in as f32) as f64
}

#[rune::function]
fn rankine_back_work_ratio(w_pump: f64, w_turbine: f64) -> f64 {
    rankine::back_work_ratio(w_pump as f32, w_turbine as f32) as f64
}

#[rune::function]
fn rankine_reheat_efficiency(
    w_turbine1: f64,
    w_turbine2: f64,
    w_pump: f64,
    q_in: f64,
    q_reheat: f64,
) -> f64 {
    rankine::reheat_efficiency(
        w_turbine1 as f32,
        w_turbine2 as f32,
        w_pump as f32,
        q_in as f32,
        q_reheat as f32,
    ) as f64
}

#[rune::function]
fn rankine_isentropic_turbine_efficiency(h_in: f64, h_out_actual: f64, h_out_ideal: f64) -> f64 {
    rankine::isentropic_turbine_efficiency(h_in as f32, h_out_actual as f32, h_out_ideal as f32)
        as f64
}

#[rune::function]
fn rankine_thermal_efficiency(w_net: f64, q_in: f64) -> f64 {
    rankine::thermal_efficiency(w_net as f32, q_in as f32) as f64
}

// ---------------------------------------------------------------------------
// Brayton gas-turbine cycle
// ---------------------------------------------------------------------------

#[rune::function]
fn brayton_efficiency(pressure_ratio: f64, gamma: f64) -> f64 {
    brayton::brayton_efficiency(pressure_ratio as f32, gamma as f32) as f64
}

#[rune::function]
fn brayton_temperature_after_compression(t_inlet: f64, pressure_ratio: f64, gamma: f64) -> f64 {
    brayton::temperature_after_compression(t_inlet as f32, pressure_ratio as f32, gamma as f32)
        as f64
}

#[rune::function]
fn brayton_temperature_after_expansion(t_inlet: f64, pressure_ratio: f64, gamma: f64) -> f64 {
    brayton::temperature_after_expansion(t_inlet as f32, pressure_ratio as f32, gamma as f32) as f64
}

#[rune::function]
fn brayton_compressor_work(cp: f64, t1: f64, t2: f64) -> f64 {
    brayton::compressor_work(cp as f32, t1 as f32, t2 as f32) as f64
}

#[rune::function]
fn brayton_turbine_work(cp: f64, t3: f64, t4: f64) -> f64 {
    brayton::turbine_work(cp as f32, t3 as f32, t4 as f32) as f64
}

#[rune::function]
fn brayton_net_specific_work(turbine_work: f64, compressor_work: f64) -> f64 {
    brayton::net_specific_work(turbine_work as f32, compressor_work as f32) as f64
}

#[rune::function]
fn brayton_optimal_pressure_ratio(t_max: f64, t_min: f64, gamma: f64) -> f64 {
    brayton::optimal_pressure_ratio(t_max as f32, t_min as f32, gamma as f32) as f64
}

#[rune::function]
fn brayton_isentropic_compressor_efficiency(t1: f64, t2_ideal: f64, t2_actual: f64) -> f64 {
    brayton::isentropic_compressor_efficiency(t1 as f32, t2_ideal as f32, t2_actual as f32) as f64
}

// ---------------------------------------------------------------------------
// Otto and Diesel internal-combustion cycles
// ---------------------------------------------------------------------------

#[rune::function]
fn otto_efficiency(compression_ratio: f64, gamma: f64) -> f64 {
    otto::otto_efficiency(compression_ratio as f32, gamma as f32) as f64
}

#[rune::function]
fn otto_diesel_efficiency(compression_ratio: f64, cutoff_ratio: f64, gamma: f64) -> f64 {
    otto::diesel_efficiency(compression_ratio as f32, cutoff_ratio as f32, gamma as f32) as f64
}

#[rune::function]
fn otto_mean_effective_pressure(work_net: f64, displacement_volume: f64) -> f64 {
    otto::mean_effective_pressure(work_net as f32, displacement_volume as f32) as f64
}

#[rune::function]
fn otto_engine_power(bmep: f64, displacement: f64, rpm: f64, strokes_per_cycle: f64) -> f64 {
    otto::engine_power(
        bmep as f32,
        displacement as f32,
        rpm as f32,
        strokes_per_cycle as f32,
    ) as f64
}

#[rune::function]
fn otto_compression_ratio_from_volumes(v_bdc: f64, v_tdc: f64) -> f64 {
    otto::compression_ratio_from_volumes(v_bdc as f32, v_tdc as f32) as f64
}

#[rune::function]
fn otto_peak_temperature(
    t_intake: f64,
    compression_ratio: f64,
    gamma: f64,
    heat_added: f64,
    cv: f64,
) -> f64 {
    otto::otto_peak_temperature(
        t_intake as f32,
        compression_ratio as f32,
        gamma as f32,
        heat_added as f32,
        cv as f32,
    ) as f64
}

#[rune::function]
fn otto_volumetric_efficiency(actual_air_mass: f64, theoretical_air_mass: f64) -> f64 {
    otto::volumetric_efficiency(actual_air_mass as f32, theoretical_air_mass as f32) as f64
}

// ---------------------------------------------------------------------------
// Vapor-compression refrigeration and heat pumps
// ---------------------------------------------------------------------------

#[rune::function]
fn refrigeration_cop_refrigerator(q_cold: f64, w_compressor: f64) -> f64 {
    refrigeration::cop_refrigerator(q_cold as f32, w_compressor as f32) as f64
}

#[rune::function]
fn refrigeration_cop_heat_pump(q_hot: f64, w_compressor: f64) -> f64 {
    refrigeration::cop_heat_pump(q_hot as f32, w_compressor as f32) as f64
}

#[rune::function]
fn refrigeration_carnot_cop_refrigerator(t_cold: f64, t_hot: f64) -> f64 {
    refrigeration::carnot_cop_refrigerator(t_cold as f32, t_hot as f32) as f64
}

#[rune::function]
fn refrigeration_carnot_cop_heat_pump(t_cold: f64, t_hot: f64) -> f64 {
    refrigeration::carnot_cop_heat_pump(t_cold as f32, t_hot as f32) as f64
}

#[rune::function]
fn refrigeration_effect(h_evap_out: f64, h_evap_in: f64) -> f64 {
    refrigeration::refrigeration_effect(h_evap_out as f32, h_evap_in as f32) as f64
}

#[rune::function]
fn refrigeration_compressor_work_vc(h_comp_out: f64, h_comp_in: f64) -> f64 {
    refrigeration::compressor_work_vc(h_comp_out as f32, h_comp_in as f32) as f64
}

#[rune::function]
fn refrigeration_mass_flow_rate_refrigerant(cooling_capacity: f64, refrigeration_effect: f64) -> f64 {
    refrigeration::mass_flow_rate_refrigerant(cooling_capacity as f32, refrigeration_effect as f32)
        as f64
}

#[rune::function]
fn refrigeration_cop_relation(cop_refrigerator: f64) -> f64 {
    refrigeration::cop_relation(cop_refrigerator as f32) as f64
}

// ---------------------------------------------------------------------------
// Heat exchangers — LMTD and NTU-effectiveness
// ---------------------------------------------------------------------------

#[rune::function]
fn hx_lmtd(delta_t1: f64, delta_t2: f64) -> f64 {
    heat_exchangers::lmtd(delta_t1 as f32, delta_t2 as f32) as f64
}

#[rune::function]
fn hx_heat_transfer_lmtd(u: f64, area: f64, lmtd: f64) -> f64 {
    heat_exchangers::heat_transfer_lmtd(u as f32, area as f32, lmtd as f32) as f64
}

#[rune::function]
fn hx_required_area(q: f64, u: f64, lmtd: f64) -> f64 {
    heat_exchangers::required_area(q as f32, u as f32, lmtd as f32) as f64
}

#[rune::function]
fn hx_ntu(ua: f64, c_min: f64) -> f64 {
    heat_exchangers::ntu(ua as f32, c_min as f32) as f64
}

#[rune::function]
fn hx_capacity_rate(mass_flow: f64, cp: f64) -> f64 {
    heat_exchangers::capacity_rate(mass_flow as f32, cp as f32) as f64
}

#[rune::function]
fn hx_effectiveness_parallel_flow(ntu: f64, c_ratio: f64) -> f64 {
    heat_exchangers::effectiveness_parallel_flow(ntu as f32, c_ratio as f32) as f64
}

#[rune::function]
fn hx_effectiveness_counter_flow(ntu: f64, c_ratio: f64) -> f64 {
    heat_exchangers::effectiveness_counter_flow(ntu as f32, c_ratio as f32) as f64
}

#[rune::function]
fn hx_effectiveness_to_heat(effectiveness: f64, c_min: f64, t_hot_in: f64, t_cold_in: f64) -> f64 {
    heat_exchangers::effectiveness_to_heat(
        effectiveness as f32,
        c_min as f32,
        t_hot_in as f32,
        t_cold_in as f32,
    ) as f64
}

#[rune::function]
fn hx_max_possible_heat(c_min: f64, t_hot_in: f64, t_cold_in: f64) -> f64 {
    heat_exchangers::max_possible_heat(c_min as f32, t_hot_in as f32, t_cold_in as f32) as f64
}

/// Build the `eustress::realism::thermocycles` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "thermocycles"])?;

    // Rankine
    m.function_meta(rankine_carnot_efficiency)?;
    m.function_meta(rankine_ideal_efficiency)?;
    m.function_meta(rankine_pump_work)?;
    m.function_meta(rankine_turbine_work)?;
    m.function_meta(rankine_heat_added)?;
    m.function_meta(rankine_back_work_ratio)?;
    m.function_meta(rankine_reheat_efficiency)?;
    m.function_meta(rankine_isentropic_turbine_efficiency)?;
    m.function_meta(rankine_thermal_efficiency)?;

    // Brayton
    m.function_meta(brayton_efficiency)?;
    m.function_meta(brayton_temperature_after_compression)?;
    m.function_meta(brayton_temperature_after_expansion)?;
    m.function_meta(brayton_compressor_work)?;
    m.function_meta(brayton_turbine_work)?;
    m.function_meta(brayton_net_specific_work)?;
    m.function_meta(brayton_optimal_pressure_ratio)?;
    m.function_meta(brayton_isentropic_compressor_efficiency)?;

    // Otto / Diesel
    m.function_meta(otto_efficiency)?;
    m.function_meta(otto_diesel_efficiency)?;
    m.function_meta(otto_mean_effective_pressure)?;
    m.function_meta(otto_engine_power)?;
    m.function_meta(otto_compression_ratio_from_volumes)?;
    m.function_meta(otto_peak_temperature)?;
    m.function_meta(otto_volumetric_efficiency)?;

    // Refrigeration / heat pumps
    m.function_meta(refrigeration_cop_refrigerator)?;
    m.function_meta(refrigeration_cop_heat_pump)?;
    m.function_meta(refrigeration_carnot_cop_refrigerator)?;
    m.function_meta(refrigeration_carnot_cop_heat_pump)?;
    m.function_meta(refrigeration_effect)?;
    m.function_meta(refrigeration_compressor_work_vc)?;
    m.function_meta(refrigeration_mass_flow_rate_refrigerant)?;
    m.function_meta(refrigeration_cop_relation)?;

    // Heat exchangers
    m.function_meta(hx_lmtd)?;
    m.function_meta(hx_heat_transfer_lmtd)?;
    m.function_meta(hx_required_area)?;
    m.function_meta(hx_ntu)?;
    m.function_meta(hx_capacity_rate)?;
    m.function_meta(hx_effectiveness_parallel_flow)?;
    m.function_meta(hx_effectiveness_counter_flow)?;
    m.function_meta(hx_effectiveness_to_heat)?;
    m.function_meta(hx_max_possible_heat)?;

    Ok(m)
}
