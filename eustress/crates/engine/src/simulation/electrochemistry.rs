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
                // Runs AFTER the Rune frame so a script's `set_sim_value`
                // reaches the cell in the SAME frame it was written, rather
                // than a frame later (or never — see the fixes documented on
                // `apply_sim_values_to_ecs`).
                apply_sim_values_to_ecs.after(crate::soul::rune_play::drive_rune_frame),
                electrochemical_tick.after(apply_sim_values_to_ecs),
                publish_echem_to_sim_values.after(electrochemical_tick),
            ).run_if(in_state(PlayModeState::Playing)),
        )
        .add_systems(OnEnter(PlayModeState::Playing), set_default_discharge);
    }
}

/// Publish ElectrochemicalState fields into the SIM_VALUES thread-local
/// so Rune scripts (battery_hud.rune) and watchpoints can read them.
fn publish_echem_to_sim_values(
    query: Query<(&ElectrochemicalState, Option<&ThermodynamicState>)>,
    mut sim_res: ResMut<crate::simulation::plugin::SimValuesResource>,
) {
    // Use the entity with the highest capacity_ah — that's the cell stack / assembly,
    // not passive components (anode, electrolyte etc.) which have capacity_ah = 0.
    let Some((echem, thermo)) = query.iter()
        .max_by(|(a, _), (b, _)| a.capacity_ah.partial_cmp(&b.capacity_ah).unwrap_or(std::cmp::Ordering::Equal))
    else { return };

    let temp_c = thermo.map(|t| t.temperature - 273.15).unwrap_or(25.0);

    // Build values map
    let values = [
        ("battery.voltage", echem.terminal_voltage as f64),
        ("battery.current", echem.current as f64),
        ("battery.soc", echem.soc as f64),
        ("battery.power", (echem.terminal_voltage * echem.current) as f64),
        ("battery.c_rate", echem.c_rate as f64),
        ("battery.dendrite_risk", echem.dendrite_risk as f64),
        ("battery.capacity_retention", echem.capacity_retention as f64),
        ("battery.cycle_count", echem.cycle_count as f64),
        ("battery.heat_generation", echem.heat_generation as f64),
        ("battery.temperature_c", temp_c as f64),
    ];

    // Write to thread-local (for Rune scripts on same thread)
    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        let mut sv = sv.borrow_mut();
        for (k, v) in &values {
            sv.insert(k.to_string(), *v);
        }
    });

    // Write to Bevy Resource (for recording system on any thread)
    for (k, v) in &values {
        sim_res.0.insert(k.to_string(), *v);
    }
}

/// Set a default 0.5C discharge current on play start so the demo shows
/// voltage/SOC changing immediately. Scripts can override via set_sim_value.
fn set_default_discharge(
    mut query: Query<&mut ElectrochemicalState>,
) {
    // Find and set current only on the cell stack (highest capacity_ah).
    // Passive components (anode slice, electrolyte etc.) have capacity_ah = 0
    // and must stay at zero current — they are not independent cells.
    if let Some(mut echem) = query.iter_mut()
        .filter(|e| e.capacity_ah > 0.0)
        .max_by(|a, b| a.capacity_ah.partial_cmp(&b.capacity_ah).unwrap_or(std::cmp::Ordering::Equal))
    {
        echem.current = echem.capacity_ah * 0.5;
        info!("⚡ Default discharge set: {:.1}A (0.5C) for {:.1}Ah cell",
            echem.current, echem.capacity_ah);
    }
    // Also set the mode in SIM_VALUES so scripts know we're discharging
    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        let mut sv = sv.borrow_mut();
        sv.insert("battery.mode".to_string(), 2.0); // 2 = discharging
    });
}

/// Read script-set values and apply them to ECS components.
///
/// Scripts call `set_sim_value("battery.mode", 1.0)` etc. to control the
/// simulation. This system reads those values and maps them to ECS fields.
///
/// Modes: 0 = idle, 1 = charging, 2 = discharging
///
/// # Two fixes live here
///
/// **Source of truth.** This read from the `SIM_VALUES` *thread-local*, which
/// is per-worker-thread: a script running on thread A wrote values this system
/// could not see from thread B. It now reads [`SimValuesResource`], the
/// cross-thread map the Rune driver merges script writes into.
///
/// **Explicit current beats the mode default.** `mode` defaults to `2.0`
/// (discharge at 0.5C) and was re-applied unconditionally every frame, so a
/// script writing `set_sim_value("battery.current", 0.0)` — which is exactly
/// what the V-Cell safety controllers do — had its value stomped on the next
/// frame. A direct `battery.current` write now short-circuits the mode logic
/// for as long as the script keeps asserting it ([`ScriptSimWrites`] carries
/// "written this frame", which an ECS-value comparison cannot express once the
/// control loop reaches steady state).
fn apply_sim_values_to_ecs(
    mut query: Query<&mut ElectrochemicalState>,
    sim_values: Res<crate::simulation::plugin::SimValuesResource>,
    script_writes: Res<crate::simulation::plugin::ScriptSimWrites>,
) {
    // Apply to the cell stack entity only (highest capacity_ah). Passive
    // components (anode slice, electrolyte, …) have capacity_ah = 0 and must
    // stay at zero current — they are not independent cells.
    let Some(mut echem) = query.iter_mut()
        .filter(|e| e.capacity_ah > 0.0)
        .max_by(|a, b| a.capacity_ah.partial_cmp(&b.capacity_ah).unwrap_or(std::cmp::Ordering::Equal))
    else { return };
    let echem = &mut *echem;

    // A cell's DESIGN is not set from here. It lives in the instance file,
    // which the Space's file watcher hot-reloads on change and which git
    // versions alongside the telemetry a run produces. Branching a Space,
    // editing the design, simulating and committing already gives a diffable
    // experiment record where the parameters and their outcome travel together.
    // A second, sim-value path for the same fields would be invisible to that
    // diff and would quietly become the source of truth nobody could review.
    // Only the DRIVE is set here.

    // Explicit current assertion wins outright.
    if let Some(current) = script_writes.0.get("battery.current").copied() {
        echem.current = current as f32;
        return;
    }

    let mode = sim_values.0.get("battery.mode").copied().unwrap_or(2.0); // default discharge
    let target_current = sim_values.0.get("battery.target_current").copied();

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

/// Advance all ElectrochemicalState components by one simulation timestep.
///
/// Uses real Na-S electrochemistry from the laws module:
/// - Nernst OCV, Butler-Volmer kinetics, ohmic losses
/// - Coulomb counting for SOC, heat generation, dendrite risk
fn electrochemical_tick(
    time: Res<Time>,
    clock: Res<SimulationClock>,
    mut query: Query<(
        &Name,
        &mut ElectrochemicalState,
        Option<&mut ThermodynamicState>,
    )>,
) {
    // Gap 8 (2026-08-21 revision) — SUB-STEP the frame instead of clamping it.
    //
    // The previous version computed the frame's compressed simulation time and
    // then clamped it to `clock.dt() * max_ticks_per_frame` — one 0.167 s bite
    // per FRAME at the shipped 1/60 s timestep and 10-tick cap. That is a hard
    // ceiling on throughput, not a stability guard: at 2 fps a run requested at
    // `time_scale = 100` integrated at 0.167 × 2 = 0.33× REALTIME, i.e. 300×
    // slower than asked, and silently. Measured on the V-Cell rig: requested
    // 100×, delivered 0.34×.
    //
    // Explicit Euler needs each STEP bounded, not the frame's total, so the
    // frame's simulation time is now consumed in steps of at most `clock.dt()`.
    // `MAX_SUBSTEPS` keeps a stalled frame from spiralling; when it binds we
    // warn rather than under-integrate in silence, because a quiet 300× shortfall
    // is exactly what made three months of "long" runs meaningless.
    const MAX_SUBSTEPS: u32 = 4096;
    let frame_sim_dt = time.delta_secs_f64() * clock.time_scale;
    if frame_sim_dt <= 0.0 { return; }
    let step = clock.dt().max(1e-6);
    let mut substeps = (frame_sim_dt / step).ceil() as u32;
    if substeps == 0 { substeps = 1; }
    if substeps > MAX_SUBSTEPS {
        warn!(
            "⚠ electrochemical_tick: {} substeps needed for {:.3}s of simulation \
             this frame (time_scale {:.0}); capping at {}. Effective compression \
             is {:.1}x lower than requested — lower time_scale or raise the frame rate.",
            substeps, frame_sim_dt, clock.time_scale, MAX_SUBSTEPS,
            substeps as f64 / MAX_SUBSTEPS as f64
        );
        substeps = MAX_SUBSTEPS;
    }
    let dt = (frame_sim_dt / substeps as f64) as f32;
    if dt <= 0.0 { return; }

    for (_name, mut echem_state, mut thermo) in &mut query {
        // Skip entities with zero capacity (passive components like housing, terminals)
        if echem_state.capacity_ah <= 0.0 {
            continue;
        }
        for _substep in 0..substeps {

        let temperature = thermo.as_ref()
            .map(|t| t.temperature)
            .unwrap_or(298.15); // Default 25°C

        // ── 1. Open-circuit voltage via Nernst equation ──
        //
        // The couple comes from the CELL, not from a constant in the engine.
        // Hardcoding `na_s::STANDARD_POTENTIAL` here made every cell in every
        // Space a sodium-sulfur cell regardless of its materials, so a
        // lithium-sulfur design reported an Na-S voltage. Energy is volts times
        // amp-hours, so that landed straight on the headline instead of
        // announcing itself. 0.0 keeps the legacy Na-S value.
        let e_standard = if echem_state.standard_potential_v > 0.0 {
            echem_state.standard_potential_v
        } else {
            constants::na_s::STANDARD_POTENTIAL
        };
        // Activity ratio approximation: Q ≈ (1 - SOC) / SOC
        let soc = echem_state.soc.clamp(0.001, 0.999);
        let activity_ratio = (1.0 - soc) / soc;
        let ocv = echem::nernst_potential(
            e_standard,
            constants::na_s::ELECTRONS,
            temperature,
            activity_ratio,
        );
        echem_state.voltage = ocv;

        // ── 2. Current and C-rate ──
        let current = echem_state.current; // Positive = discharge, negative = charge
        echem_state.c_rate = echem::c_rate(current.abs(), echem_state.capacity_ah);

        if current.abs() < 1e-6 {
            // No current flowing — terminal = OCV, no heat. `break`, not
            // `continue`: nothing evolves at zero current, so re-running the
            // remaining substeps would burn up to MAX_SUBSTEPS iterations per
            // frame to reach the same state.
            echem_state.terminal_voltage = ocv;
            echem_state.heat_generation = 0.0;
            break;
        }

        // ── 3. Overpotentials ──
        // Ohmic (IR drop)
        let eta_ohmic = echem::ohmic_overpotential(current, echem_state.internal_resistance);

        // Charge-transfer (Butler-Volmer symmetric approximation)
        // Exchange current density ~50 A/m² for Na-S at 25°C
        let j0 = 50.0_f32; // A/m²
        // TOTAL electrode area of the cell, summed over every layer of the
        // stack. This was hardcoded to 0.03 m² — one ~300 cm² layer — which for
        // a 26-layer V-Cell (0.7384 m²) overstated current density by 24.6× and
        // pinned `dendrite_risk` at 1.0 at every rate ever tested, including
        // rates 12× inside the true limit. A saturated alarm is a dead alarm.
        let electrode_area = if echem_state.electrode_area_m2 > 0.0 {
            echem_state.electrode_area_m2
        } else {
            0.03_f32
        };
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
        let entropy_coeff = if echem_state.entropy_coefficient_v_per_k != 0.0 {
            echem_state.entropy_coefficient_v_per_k
        } else {
            constants::na_s::ENTROPY_COEFFICIENT
        };
        let q_entropic = echem::entropic_heat(temperature, current, entropy_coeff);
        echem_state.heat_generation = q_ohmic + q_reaction + q_entropic.abs();

        // ── 7. Thermal coupling ──
        if let Some(ref mut thermo_state) = thermo {
            // Lumped thermal model: m·Cp·dT/dt = Q - (T - T_amb)/R
            //
            // Both parameters used to be hardcoded to one specific 0.695 kg
            // pouch — 625.5 J/K and 2.0 K/W. Every other cell integrated at
            // the wrong rate AND settled at the wrong temperature, and because
            // temperature feeds the Nernst term in step 1, the error reached
            // VOLTAGE and therefore energy. A 3.55 kg cell drawing 878 A ran
            // away to 2292 °C and dragged mean voltage down to 1.87 V, which
            // read as a physics result and was in fact a units bug. Authored
            // values now win; 0.0 keeps the legacy constants so scenes that
            // never set them are unchanged.
            let thermal_mass = if echem_state.thermal_mass_j_per_k > 0.0 {
                echem_state.thermal_mass_j_per_k
            } else {
                0.695 * 900.0 // J/K — legacy 0.695 kg pouch at Cp 900 J/(kg·K)
            };
            let r_thermal = if echem_state.thermal_resistance_k_per_w > 0.0 {
                echem_state.thermal_resistance_k_per_w
            } else {
                2.0_f32 // K/W — legacy AlN pad + housing
            };
            let ambient = 298.15_f32;

            // Integrate BOTH terms against the same dt, and solve the cooling
            // term exponentially rather than explicitly: at a large `dt` (the
            // substep loop can hand this system a whole second of compressed
            // time) an explicit `(T - T_amb)/R · dt / mCp` step overshoots
            // ambient and oscillates once dt exceeds the R·mCp time constant.
            // The closed form is unconditionally stable at any dt.
            let t_steady = ambient + echem_state.heat_generation * r_thermal;
            let tau = (r_thermal * thermal_mass).max(1e-6);
            let decay = (-dt / tau).exp();
            thermo_state.temperature =
                t_steady + (thermo_state.temperature - t_steady) * decay;
            thermo_state.temperature = thermo_state.temperature.max(ambient);
        }

        // ── 8. Dendrite risk (Monroe-Newman model) ──
        //
        // The critical current density is a property of the metal/electrolyte
        // INTERFACE, so a cell that specifies one is believed. The fallback is
        // the Monroe-Newman estimate from the legacy Na/NASICON constants,
        // which is wrong for any other couple: it assumes a 30 GPa oxide and
        // sodium's molar volume, so a lithium cell on a softer sulfide reads a
        // limit it does not have.
        let j_crit = if echem_state.j_crit_a_per_m2 > 0.0 {
            echem_state.j_crit_a_per_m2
        } else {
            let g_electrolyte = 30.0e9_f32; // Pa, oxide ceramic
            let interlayer = 5.0e-9_f32;    // m, ALD Al₂O₃
            let v_molar = 23.7e-6_f32;      // m³/mol, Na
            echem::monroe_newman_critical_current(g_electrolyte, interlayer, v_molar)
        };
        // Plating happens on CHARGE. On discharge the cell strips metal, and
        // the failure mode there is void formation, not dendrite growth, so a
        // discharge current must not be scored against a plating limit.
        let plating_density = if current < 0.0 { current_density.abs() } else { 0.0 };
        echem_state.dendrite_risk = echem::dendrite_risk(
            plating_density,
            j_crit,
        ).clamp(0.0, 1.0);

        // ── 9. Cycle counting ──
        // Detect full cycle: SOC crosses 0.1 (discharge) then 0.9 (charge)
        // Simple heuristic: count when SOC drops below 10% as half-cycle
        // (Real implementation would use rain-flow counting)
        // For now, accumulate partial cycles based on charge throughput
        // Accumulate the FRACTION separately. The old line added a ~1e-6 cycle
        // increment to `cycle_count as f32` and cast straight back to u32, so
        // every increment truncated away and the counter never left zero — which
        // meant `capacity_retention` was permanently 1.0 and capacity fade never
        // ran. Cycle life was not merely inaccurate, it was unsimulatable.
        if effective_capacity > 0.0 {
            let cycle_fraction = charge_delta_ah.abs() / (2.0 * effective_capacity);
            echem_state.cycle_accum += cycle_fraction;
            if echem_state.cycle_accum >= 1.0 {
                let whole = echem_state.cycle_accum.floor();
                echem_state.cycle_count = echem_state.cycle_count.saturating_add(whole as u32);
                echem_state.cycle_accum -= whole;
            }
        }

        // ── 10. Capacity fade by metal-inventory loss ──
        //
        // The old model was `Q(N)/Q₀ = 1 - αN^β` with α and β fixed constants.
        // It could not distinguish a cell cycled 10% deep from one cycled to
        // empty, could not see plating rate, temperature, stack pressure or
        // whether the cell carried a metal reservoir, and therefore could not
        // be used to CHOOSE between designs — which is the only thing a life
        // model is for. Every one of those is a first-order lever on a
        // metal-anode cell.
        //
        // This tracks inventory instead. Each unit of charge plated loses a
        // little metal to interphase; the loss rate rises with depth, with
        // plating current, with temperature, and falls with stack pressure.
        // Capacity holds while a reservoir covers the cumulative loss and only
        // then declines, which is the mechanistic reason an anode-LEAN cell
        // outlives an anode-free one rather than an assertion that it does.
        if effective_capacity > 0.0 && charge_delta_ah != 0.0 {
            let ce_ref = if echem_state.coulombic_efficiency_ref > 0.0 {
                echem_state.coulombic_efficiency_ref
            } else {
                0.995
            };
            let pressure = if echem_state.stack_pressure_mpa > 0.0 {
                echem_state.stack_pressure_mpa
            } else {
                2.0
            };
            // Depth of the CYCLE, tracked across substeps.
            //
            // Using the charge moved in one substep would make fade a function
            // of the timestep: halve the clock and the answer changes, which
            // makes the model useless for comparing designs. The excursion is
            // measured from the last direction reversal instead, so "depth"
            // means depth of discharge and nothing else.
            // Signed against the direction of travel: discharging walks SOC
            // down from the turning point, charging walks it up. A negative
            // swing means the cell reversed, so the turning point moves here
            // and the excursion restarts. No separate direction flag is needed.
            let signed_swing = if current > 0.0 {
                echem_state.soc_turn - echem_state.soc
            } else {
                echem_state.soc - echem_state.soc_turn
            };
            if signed_swing < 0.0 {
                echem_state.soc_turn = echem_state.soc;
                echem_state.excursion_depth = 0.0;
            } else if signed_swing > echem_state.excursion_depth {
                echem_state.excursion_depth = signed_swing;
            }
            let depth = echem_state.excursion_depth.clamp(0.01, 1.0);
            // Loss PER UNIT CHARGE rises with excursion depth, because a
            // thicker deposit is a rougher one and roughness is surface area
            // for the parasitic reaction. Integrated over a cycle this gives
            // loss ∝ D^1.8 and therefore cycles-to-end-of-life ∝ D^-1.8, which
            // is the shape metal-anode cells actually show. The exponent is an
            // assumption; the branch sweep is what tests it.
            let f_depth = depth.powf(0.8);
            // Rougher deposit at higher plating current density.
            let f_rate = (plating_density.max(1e-6) / j_crit.max(1e-6))
                .max(0.05)
                .powf(0.5);
            // Arrhenius on the parasitic reaction, referenced to 25 °C.
            let f_temp = ((temperature - 298.15) / 20.0).exp().clamp(0.2, 20.0);
            // Pressure suppresses the voids that form on stripping.
            let f_press = (2.0_f32 / pressure).powf(0.5);
            let loss_frac = (1.0 - ce_ref) * f_depth * f_rate * f_temp * f_press;
            // Widen before accumulating, not after: the increment is around
            // 1e-11 of nominal per substep and would vanish into an f32 total.
            echem_state.li_inventory_lost +=
                (loss_frac * (charge_delta_ah.abs() / effective_capacity)) as f64;

            let usable = (echem_state.li_inventory_lost
                - echem_state.li_reservoir_frac as f64)
                .max(0.0);
            echem_state.capacity_retention = (1.0 - usable as f32).clamp(0.01, 1.0);
        }
        } // end substep
    }
}
