use bevy::prelude::*;
use tracing::{info, warn};
use super::components::*;
use super::constants::*;

// ── 1. Reactivity calculation ─────────────────────────────────────────────────

/// Derives current reactivity from rod position + Doppler feedback.
/// Runs before the kinetics integrator so ρ is always fresh.
pub fn update_reactivity_system(
    mut query: Query<(&ControlRodBankComponent, &ThermalHydraulicsComponent, &mut NuclearKineticsComponent), With<ArcReactorCore>>,
) {
    for (rods, thermal, mut kinetics) in &mut query {
        if kinetics.is_scrammed { continue; }
        let rho_rod = rods.rod_reactivity();
        let doppler = TEMP_FEEDBACK_COEFFICIENT * (thermal.core_temp_celsius - TEMP_FEEDBACK_REFERENCE);
        kinetics.reactivity = rho_rod + doppler;
    }
}

// ── 2. Point kinetics integrator ─────────────────────────────────────────────

/// Integrates the one-group point kinetics equations each frame.
///
///   dn/dt = ((ρ − β) / Λ) · n + λ · C
///   dC/dt = (β / Λ) · n − λ · C
pub fn update_nuclear_kinetics_system(
    time: Res<Time>,
    mut query: Query<&mut NuclearKineticsComponent, With<ArcReactorCore>>,
) {
    let dt = time.delta_secs().min(0.05);

    for mut kinetics in &mut query {
        if kinetics.is_scrammed {
            kinetics.shutdown_seconds += dt;
            kinetics.neutron_population = (kinetics.neutron_population * (1.0 - dt * 50.0)).max(0.0);
            continue;
        }

        let n        = kinetics.neutron_population;
        let c        = kinetics.precursor_concentration;
        let rho      = kinetics.reactivity;
        let beta     = kinetics.beta;
        let lambda   = kinetics.mean_generation_time;
        let lambda_d = kinetics.precursor_decay_constant;

        let dn = ((rho - beta) / lambda) * n + lambda_d * c;
        let dc = (beta / lambda) * n - lambda_d * c;

        kinetics.neutron_population = (n + dn * dt).clamp(0.0, 5.0);
        kinetics.precursor_concentration = (c + dc * dt).max(0.0);
    }
}

// ── 3. Thermal-hydraulics ─────────────────────────────────────────────────────

/// Updates core and coolant temperatures given the current neutron population,
/// rod state, and coolant flow.
pub fn update_thermal_hydraulics_system(
    time: Res<Time>,
    mut query: Query<(&NuclearKineticsComponent, &ControlRodBankComponent, &mut ThermalHydraulicsComponent), With<ArcReactorCore>>,
) {
    let dt = time.delta_secs().min(0.05);

    for (kinetics, rods, mut thermal) in &mut query {
        let rod_f    = rods.insertion_fraction();
        let flow_eff = thermal.coolant_flow_pct / 100.0;

        let pth = if kinetics.is_scrammed {
            let t_shutdown = kinetics.shutdown_seconds.max(1.0);
            let decay = DECAY_HEAT_COEFF * thermal.rated_power_watts * t_shutdown.powf(DECAY_HEAT_EXPONENT);
            thermal.decay_heat_watts = decay;
            decay
        } else {
            let p = (1.0 - rod_f * 1.5).max(0.0) * kinetics.neutron_population * thermal.rated_power_watts;
            thermal.decay_heat_watts = 0.0;
            p
        };
        thermal.thermal_power_watts = pth;

        let heat_removed = pth * flow_eff * COOLANT_HEAT_TRANSFER_EFF;
        thermal.heat_removed_watts = heat_removed;

        let delta_t = (pth - heat_removed) * CORE_THERMAL_INERTIA
            - (thermal.core_temp_celsius - AMBIENT_TEMP_CELSIUS) * PASSIVE_HEAT_LOSS_COEFF;

        thermal.core_temp_celsius = (thermal.core_temp_celsius + delta_t * dt * 10.0)
            .clamp(20.0, 2_000.0);
        thermal.coolant_temp_celsius = thermal.core_temp_celsius * COOLANT_TEMP_RATIO * flow_eff;
    }
}

// ── 4. Power conversion ───────────────────────────────────────────────────────

/// Converts thermal power to electrical output through the TE + Stirling stage.
pub fn update_power_conversion_system(
    mut query: Query<(&ThermalHydraulicsComponent, &mut PowerConversionComponent), With<ArcReactorCore>>,
) {
    for (thermal, mut conv) in &mut query {
        let te = conv.te_efficiency;
        let st = conv.stirling_efficiency;
        conv.total_efficiency = te + st * (1.0 - te);
        conv.electrical_output_watts = (thermal.thermal_power_watts * conv.total_efficiency).max(0.0);
    }
}

// ── 5. Battery buffer ─────────────────────────────────────────────────────────

/// Updates V-Cell SoC based on power surplus / deficit.
pub fn update_battery_buffer_system(
    time: Res<Time>,
    mut query: Query<(&PowerConversionComponent, &mut VCellBatteryComponent), With<ArcReactorCore>>,
) {
    let dt = time.delta_secs().min(0.05);

    for (conv, mut batt) in &mut query {
        let balance = conv.electrical_output_watts - batt.load_demand_watts;
        batt.power_balance_watts = balance;

        let over  = balance.max(0.0);
        let under = (-balance).max(0.0);

        batt.state_of_charge_pct = (batt.state_of_charge_pct
            + over  * dt * VCELL_CHARGE_RATE
            - under * dt * VCELL_DISCHARGE_RATE)
            .clamp(0.0, 100.0);
    }
}

// ── 6. AI PID controller ──────────────────────────────────────────────────────

/// Runs all three PID loops and writes corrections to ControlRodBankComponent
/// and ThermalHydraulicsComponent when the AI controller is active.
pub fn update_ai_controller_system(
    time: Res<Time>,
    mut query: Query<(
        &mut ArcReactorAIController,
        &mut ControlRodBankComponent,
        &mut ThermalHydraulicsComponent,
        &NuclearKineticsComponent,
        &PowerConversionComponent,
        &VCellBatteryComponent,
    ), With<ArcReactorCore>>,
) {
    let dt = time.delta_secs().min(0.05);

    for (mut ai, mut rods, mut thermal, kinetics, conv, batt) in &mut query {
        if !ai.ai_override_enabled { continue; }

        match ai.mode {
            ReactorControlMode::Standby | ReactorControlMode::EmergencyShutdown => continue,
            _ => {}
        }

        ai.power_pid.setpoint = batt.load_demand_watts;

        let rod_correction = if matches!(ai.mode, ReactorControlMode::PowerFollow) {
            ai.power_pid.update(conv.electrical_output_watts, dt)
        } else {
            0.0
        };

        let reactivity_correction = ai.reactivity_pid.update(kinetics.neutron_population, dt);
        let total_rod_delta = rod_correction + reactivity_correction;

        rods.bank_a_pct = (rods.bank_a_pct + total_rod_delta * 0.5).clamp(0.0, 100.0);
        rods.bank_b_pct = (rods.bank_b_pct + total_rod_delta * 0.5).clamp(0.0, 100.0);

        let flow_correction = ai.thermal_pid.update(thermal.core_temp_celsius, dt);
        thermal.coolant_flow_pct = (thermal.coolant_flow_pct - flow_correction).clamp(10.0, 100.0);

        ai.time_since_scram += dt;
    }
}

// ── 7. Safety monitor ────────────────────────────────────────────────────────

/// Monitors safety limits and sends a `ReactorScramMessage` when a limit is
/// exceeded.  Executing the SCRAM is done by `execute_scram_system`.
pub fn nuclear_safety_monitor_system(
    query: Query<(Entity, &NuclearKineticsComponent, &ThermalHydraulicsComponent, &VCellBatteryComponent), With<ArcReactorCore>>,
    mut scram_msgs: MessageWriter<ReactorScramMessage>,
) {
    for (entity, kinetics, thermal, batt) in &query {
        if kinetics.is_scrammed { continue; }

        if thermal.core_temp_celsius > thermal.scram_temp_celsius {
            scram_msgs.write(ReactorScramMessage {
                entity,
                reason: ScramReason::TemperatureExceeded { celsius: thermal.core_temp_celsius },
            });
        } else if kinetics.neutron_population > 4.5 {
            scram_msgs.write(ReactorScramMessage {
                entity,
                reason: ScramReason::NeutronExcursion { population: kinetics.neutron_population },
            });
        } else if batt.state_of_charge_pct < VCELL_LOW_SOC_THRESHOLD {
            scram_msgs.write(ReactorScramMessage {
                entity,
                reason: ScramReason::BatteryCritical { soc_pct: batt.state_of_charge_pct },
            });
        }
    }
}

/// Executes the SCRAM: fully inserts rods, ramps coolant to maximum,
/// disables AI controller PIDs, and transitions kinetics to shutdown.
pub fn execute_scram_system(
    mut scram_msgs: MessageReader<ReactorScramMessage>,
    mut query: Query<(
        &mut NuclearKineticsComponent,
        &mut ControlRodBankComponent,
        &mut ThermalHydraulicsComponent,
        &mut ArcReactorAIController,
    ), With<ArcReactorCore>>,
) {
    for msg in scram_msgs.read() {
        let Ok((mut kinetics, mut rods, mut thermal, mut ai)) = query.get_mut(msg.entity) else { continue };
        if kinetics.is_scrammed { continue; }

        kinetics.is_scrammed = true;
        kinetics.shutdown_seconds = 0.0;
        rods.bank_a_pct = 100.0;
        rods.bank_b_pct = 100.0;
        thermal.coolant_flow_pct = 100.0;
        ai.mode = ReactorControlMode::EmergencyShutdown;
        ai.reactivity_pid.reset();
        ai.thermal_pid.reset();
        ai.power_pid.reset();
        ai.time_since_scram = 0.0;

        let label = match &msg.reason {
            ScramReason::TemperatureExceeded { celsius } =>
                format!("SCRAM: core temp {celsius:.0}°C exceeded {SCRAM_TEMP_CELSIUS:.0}°C limit"),
            ScramReason::NeutronExcursion { population } =>
                format!("SCRAM: neutron excursion n={population:.2}"),
            ScramReason::BatteryCritical { soc_pct } =>
                format!("SCRAM: battery critical SoC={soc_pct:.1}%"),
            ScramReason::ManualScram =>
                "SCRAM: operator manual emergency shutdown".to_string(),
        };
        warn!("{label}");
    }
}

// ── 8. Watchpoint publisher ───────────────────────────────────────────────────

/// Writes reactor telemetry to named watchpoints so Rune scripts and the
/// studio simulator dashboard can read them via `get_sim_value`.
pub fn publish_nuclear_watchpoints_system(
    query: Query<(
        &NuclearKineticsComponent,
        &ThermalHydraulicsComponent,
        &ControlRodBankComponent,
        &PowerConversionComponent,
        &VCellBatteryComponent,
        &ArcReactorAIController,
    ), With<ArcReactorCore>>,
    mut registry: ResMut<crate::simulation::watchpoint::WatchPointRegistry>,
    clock: Res<crate::simulation::clock::SimulationClock>,
) {
    macro_rules! record {
        ($name:expr, $val:expr, $unit:expr) => {{
            let name: &str = $name;
            if registry.get(name).is_none() {
                registry.register(
                    crate::simulation::watchpoint::WatchPoint::new(name, name, $unit)
                );
            }
            registry.record(name, $val as f64, clock.simulation_time_s, clock.tick_count);
        }};
    }

    for (kin, therm, rods, conv, batt, ai) in &query {
        record!("arc1.neutron_population",    kin.neutron_population,          "n");
        record!("arc1.reactivity",            kin.reactivity,                  "Δk/k");
        record!("arc1.is_scrammed",           kin.is_scrammed as u8 as f32,    "bool");
        record!("arc1.shutdown_seconds",      kin.shutdown_seconds,            "s");

        record!("arc1.core_temp_celsius",     therm.core_temp_celsius,         "°C");
        record!("arc1.coolant_temp_celsius",  therm.coolant_temp_celsius,      "°C");
        record!("arc1.thermal_power_watts",   therm.thermal_power_watts,       "W");
        record!("arc1.decay_heat_watts",      therm.decay_heat_watts,          "W");
        record!("arc1.coolant_flow_pct",      therm.coolant_flow_pct,          "%");

        record!("arc1.rod_bank_a_pct",        rods.bank_a_pct,                 "%");
        record!("arc1.rod_bank_b_pct",        rods.bank_b_pct,                 "%");
        record!("arc1.rod_insertion_frac",    rods.insertion_fraction(),       "frac");

        record!("arc1.te_efficiency",         conv.te_efficiency,              "frac");
        record!("arc1.stirling_efficiency",   conv.stirling_efficiency,        "frac");
        record!("arc1.total_efficiency",      conv.total_efficiency,           "frac");
        record!("arc1.electrical_output_w",   conv.electrical_output_watts,    "W");

        record!("arc1.battery_soc_pct",       batt.state_of_charge_pct,       "%");
        record!("arc1.load_demand_watts",     batt.load_demand_watts,          "W");
        record!("arc1.power_balance_watts",   batt.power_balance_watts,        "W");

        record!("arc1.neutron_setpoint",      ai.neutron_setpoint,             "n");
        record!("arc1.temp_setpoint_celsius", ai.temp_setpoint_celsius,        "°C");
        record!("arc1.power_setpoint_watts",  ai.power_setpoint_watts,         "W");
    }
}
