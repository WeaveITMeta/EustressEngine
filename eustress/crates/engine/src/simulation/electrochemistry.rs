//! # Electrochemical Tick System
//!
//! Advances ElectrochemicalState components each simulation tick using
//! real physics from `eustress_common::realism::laws::electrochemistry`.
//!
//! ## Model
//!
//! Each tick (dt from SimulationClock):
//! 1. Compute OCV via Nernst equation at current SOC
//! 2. Compute charge-transfer overpotential via Butler-Volmer
//! 3. Compute terminal voltage = OCV - IR drop - overpotentials
//! 4. Update SOC via coulomb counting (current × dt / capacity)
//! 5. Compute heat generation (ohmic + reaction + entropic)
//! 6. Update ThermodynamicState temperature from heat
//! 7. Update dendrite risk via Monroe-Newman model
//! 8. Track cycle count and capacity degradation

use bevy::prelude::*;
use eustress_common::realism::laws::electrochemistry as echem;
use eustress_common::realism::constants;
use eustress_common::realism::particles::components::{ElectrochemicalState, ThermodynamicState};
use eustress_common::simulation::SimulationClock;

use crate::play_mode::PlayModeState;

/// Plugin that registers the electrochemical tick system.
pub struct ElectrochemistryPlugin;

impl Plugin for ElectrochemistryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_sim_values_to_ecs,
                electrochemical_tick.after(apply_sim_values_to_ecs),
            ).run_if(in_state(PlayModeState::Playing)),
        )
        .add_systems(OnEnter(PlayModeState::Playing), set_default_discharge);
    }
}

/// Set a default 0.5C discharge current on play start so the demo shows
/// voltage/SOC changing immediately. Scripts can override via set_sim_value.
fn set_default_discharge(
    mut query: Query<&mut ElectrochemicalState>,
) {
    for mut echem in &mut query {
        if echem.capacity_ah > 0.0 {
            // Default: 0.5C discharge (positive current = discharge)
            echem.current = echem.capacity_ah * 0.5;
            info!("⚡ Default discharge set: {:.1}A (0.5C) for {:.1}Ah cell",
                echem.current, echem.capacity_ah);
        }
    }
    // Also set the mode in SIM_VALUES so scripts know we're discharging
    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        let mut sv = sv.borrow_mut();
        sv.insert("battery.mode".to_string(), 2.0); // 2 = discharging
    });
}

/// Read script-set values from SIM_VALUES and apply to ECS components.
///
/// Scripts call set_sim_value("battery.mode", 1.0) etc. to control the
/// simulation. This system reads those values and maps them to ECS fields.
///
/// Modes: 0 = idle, 1 = charging, 2 = discharging
fn apply_sim_values_to_ecs(
    mut query: Query<&mut ElectrochemicalState>,
) {
    let (mode, target_current) = crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        let sv = sv.borrow();
        (
            sv.get("battery.mode").copied().unwrap_or(2.0), // default discharge
            sv.get("battery.target_current").copied(),
        )
    });

    for mut echem in &mut query {
        if echem.capacity_ah <= 0.0 { continue; }

        match mode as i32 {
            0 => {
                // Idle — no current
                echem.current = 0.0;
            }
            1 => {
                // Charging — negative current (convention: positive = discharge)
                let rate = target_current.unwrap_or((echem.capacity_ah * 1.0) as f64);
                echem.current = -(rate as f32);
            }
            2 => {
                // Discharging — positive current
                let rate = target_current.unwrap_or((echem.capacity_ah * 0.5) as f64);
                echem.current = rate as f32;
            }
            _ => {}
        }
    }
}

/// Advance all ElectrochemicalState components by one simulation timestep.
///
/// Uses real Na-S electrochemistry from the laws module:
/// - Nernst OCV, Butler-Volmer kinetics, ohmic losses
/// - Coulomb counting for SOC, heat generation, dendrite risk
fn electrochemical_tick(
    clock: Res<SimulationClock>,
    mut query: Query<(
        &Name,
        &mut ElectrochemicalState,
        Option<&mut ThermodynamicState>,
    )>,
) {
    let dt = clock.dt() as f32;
    if dt <= 0.0 { return; }

    for (_name, mut echem_state, thermo) in &mut query {
        // Skip entities with zero capacity (passive components like housing, terminals)
        if echem_state.capacity_ah <= 0.0 {
            continue;
        }

        let temperature = thermo.as_ref()
            .map(|t| t.temperature)
            .unwrap_or(298.15); // Default 25°C

        // ── 1. Open-circuit voltage via Nernst equation ──
        // Activity ratio approximation: Q ≈ (1 - SOC) / SOC for Na-S
        let soc = echem_state.soc.clamp(0.001, 0.999);
        let activity_ratio = (1.0 - soc) / soc;
        let ocv = echem::nernst_potential(
            constants::na_s::STANDARD_POTENTIAL,
            constants::na_s::ELECTRONS,
            temperature,
            activity_ratio,
        );
        echem_state.voltage = ocv;

        // ── 2. Current and C-rate ──
        let current = echem_state.current; // Positive = discharge, negative = charge
        echem_state.c_rate = echem::c_rate(current.abs(), echem_state.capacity_ah);

        if current.abs() < 1e-6 {
            // No current flowing — terminal = OCV, no heat
            echem_state.terminal_voltage = ocv;
            echem_state.heat_generation = 0.0;
            continue;
        }

        // ── 3. Overpotentials ──
        // Ohmic (IR drop)
        let eta_ohmic = echem::ohmic_overpotential(current, echem_state.internal_resistance);

        // Charge-transfer (Butler-Volmer symmetric approximation)
        // Exchange current density ~50 A/m² for Na-S at 25°C
        let j0 = 50.0_f32; // A/m²
        let electrode_area = 0.03_f32; // ~300 cm² = 0.03 m²
        let current_density = current / electrode_area;
        let eta_ct = if j0 > 0.0 && current_density.abs() > 1e-6 {
            echem::tafel_overpotential(current_density.abs(), j0, 0.5, temperature)
        } else {
            0.0
        };

        // ── 4. Terminal voltage ──
        let is_discharge = current > 0.0;
        echem_state.terminal_voltage = echem::terminal_voltage(
            ocv, eta_ohmic, eta_ct, 0.0, // no diffusion overpotential for now
            is_discharge,
        );

        // ── 5. SOC update via coulomb counting ──
        // current > 0 = discharge (SOC decreases), current < 0 = charge (SOC increases)
        let charge_delta_ah = current * dt / 3600.0; // A·s → Ah
        let effective_capacity = echem_state.capacity_ah * echem_state.capacity_retention;
        echem_state.soc = echem::state_of_charge(
            echem_state.soc,
            charge_delta_ah,
            effective_capacity,
        ).clamp(0.0, 1.0);

        // ── 6. Heat generation ──
        let q_ohmic = echem::ohmic_heat(current, echem_state.internal_resistance);
        let q_reaction = echem::reaction_heat(current, eta_ct);
        let q_entropic = echem::entropic_heat(
            temperature,
            current,
            constants::na_s::ENTROPY_COEFFICIENT,
        );
        echem_state.heat_generation = q_ohmic + q_reaction + q_entropic.abs();

        // ── 7. Thermal coupling ──
        if let Some(ref mut thermo_state) = thermo {
            // Simple thermal model: dT = Q·dt / (m·Cp)
            // V-Cell mass ≈ 0.45 kg, effective Cp ≈ 900 J/(kg·K)
            let thermal_mass = 0.45 * 900.0; // J/K
            let dt_temp = echem_state.heat_generation * dt / thermal_mass;
            thermo_state.temperature += dt_temp;

            // Passive cooling toward ambient (25°C = 298.15 K)
            // Thermal resistance ≈ 2.0 K/W for AlN pad + housing
            let ambient = 298.15_f32;
            let r_thermal = 2.0_f32; // K/W
            let cooling_rate = (thermo_state.temperature - ambient) / r_thermal;
            let dt_cooling = cooling_rate * dt / thermal_mass;
            thermo_state.temperature -= dt_cooling;
            thermo_state.temperature = thermo_state.temperature.max(ambient);
        }

        // ── 8. Dendrite risk (Monroe-Newman model) ──
        // Critical current density for Sc-NASICON + Na interface
        // G_electrolyte ≈ 30 GPa for NASICON, interlayer ≈ 5 nm ALD Al₂O₃
        let g_electrolyte = 30.0e9_f32; // Pa (shear modulus of NASICON)
        let interlayer = 5.0e-9_f32;    // m (ALD Al₂O₃)
        let v_molar = 23.7e-6_f32;      // m³/mol (Na molar volume)
        let j_crit = echem::monroe_newman_critical_current(
            g_electrolyte,
            interlayer,
            v_molar,
        );
        echem_state.dendrite_risk = echem::dendrite_risk(
            current_density.abs(),
            j_crit,
        ).clamp(0.0, 1.0);

        // ── 9. Cycle counting ──
        // Detect full cycle: SOC crosses 0.1 (discharge) then 0.9 (charge)
        // Simple heuristic: count when SOC drops below 10% as half-cycle
        // (Real implementation would use rain-flow counting)
        // For now, accumulate partial cycles based on charge throughput
        let cycle_fraction = charge_delta_ah.abs() / (2.0 * effective_capacity);
        let new_cycles = echem_state.cycle_count as f32 + cycle_fraction;
        echem_state.cycle_count = new_cycles as u32;

        // ── 10. Capacity degradation (power-law) ──
        // Q(N)/Q₀ = 1 - α·N^β
        // α ≈ 0.00005, β ≈ 0.5 for NASICON solid-state
        echem_state.capacity_retention = echem::capacity_retention_power_law(
            echem_state.cycle_count,
            0.00005,
            0.5,
        ).clamp(0.01, 1.0);
    }
}
