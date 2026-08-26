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
use eustress_common::realism::particles::components::{
    ElectrochemicalState, ThermodynamicState, CATHODE_COMPOSITE_FACTOR,
    CREEP_EXPONENT, DEFAULT_BRIDGE_CHI, DEFAULT_BRIDGE_SCALE,
    DEFAULT_BRIDGE_WEIBULL_M, DEFAULT_CRACK_K, DEFAULT_CREEP_K,
    DEFAULT_CREEP_THRESHOLD_MPA, DEFAULT_RESISTANCE_EA_EV, KB_EV_PER_K,
    LI2S_SPECIFIC_CAPACITY_MAH_PER_G,
};
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
        // Retention with no attribution cannot tell you which lever to pull, so
        // each fade channel is published separately. Whichever of these three is
        // largest is the mechanism that is actually killing the cell, and the
        // others are noise until it is fixed.
        ("battery.fade_lithium", echem.lithium_fade()),
        ("battery.fade_cathode", echem.crack_damage),
        ("battery.fade_calendar", echem.calendar_fade()),
        ("battery.calendar_hours", echem.calendar_hours_equiv),
        ("battery.fade_creep", echem.creep_effective().min(1.0)),
        // Total creep against the part of it that has actually reached the
        // separator. The gap between these two is the reserve void doing its job.
        ("battery.creep_strain_total", echem.creep_strain),
        // Layers lost outright, which is not a fade and is not visible at the
        // terminals. Watch this, not voltage.
        ("battery.fade_short", echem.short_fade()),
        // Whether ANY mechanism in this model currently penalises stack
        // pressure. Creep is the only one, and it is the reason the creep term
        // exists at all: without it the tick reports a better number at every
        // pressure authored, which makes it an advocate for its own biggest
        // lever rather than a test of it.
        //
        // The confinement correction and the accommodation void together can
        // drive creep to zero across an entire run - measured 5.16e-6 of strain
        // over 1,517 h at 5 MPa, against a void that absorbs 0.396 - and at that
        // point pressure enters only through the roughness term's (2/P)^1.5,
        // which improves monotonically. The model is an advocate again and looks
        // exactly the same from the outside.
        //
        // So a pressure sweep must read this. At 0.0 the sweep is unbounded and
        // its optimum is an artefact of wherever the sweep happened to stop.
        ("battery.pressure_ceiling_active",
            if echem.creep_effective() > 0.0 { 1.0 } else { 0.0 }),
        ("battery.shorted_layers", echem.shorted_layers),
        // What the lithium reservoir costs. Published every tick so a cycle-life
        // figure and a specific-energy figure can never again be quoted in one
        // sentence while describing two different cells.
        ("battery.reservoir_mass_g", (echem.reservoir_mass_kg * 1000.0) as f64),
        ("battery.specific_energy_wh_kg", {
            let m = echem.cell_mass_kg + echem.reservoir_mass_kg;
            if m > 0.0 {
                (echem.capacity_ah * echem.terminal_voltage / m) as f64
            } else {
                0.0
            }
        }),
        ("battery.resistance_ohm", echem.resistance_effective as f64),
        ("battery.ambient_c", (echem.ambient_temperature_k - 273.15) as f64),
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
        //
        // Resistance is temperature dependent, and treating it as a constant is
        // what made the low-temperature envelope unsimulatable. A solid
        // electrolyte conducts by thermally activated hopping, so conductivity
        // goes as exp(-Ea/kT) and resistance as its inverse. At -55 C this is a
        // factor of ~150 against the 25 C value, which is the difference
        // between a cell that is slow and a cell the model thinks is fine.
        //
        // The same term does double duty: the raised resistance is also what
        // dissipates I^2 R into the cell, so a cold cell heats itself and the
        // ceiling rises as it does. Both halves fall out of one line.
        let ea = if echem_state.resistance_activation_ev > 0.0 {
            echem_state.resistance_activation_ev
        } else {
            DEFAULT_RESISTANCE_EA_EV
        };
        let r_eff = echem_state.internal_resistance
            * ((ea / KB_EV_PER_K) * (1.0 / temperature - 1.0 / 298.15)).exp();

        echem_state.resistance_effective = r_eff;

        // Ohmic (IR drop)
        let eta_ohmic = echem::ohmic_overpotential(current, r_eff);

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
        // Tafel is the HIGH-FIELD LIMIT of Butler-Volmer, and this tick was
        // applying it at every current including those far below the exchange
        // current. Below j0 the logarithm turns negative and returns an
        // overpotential that ASSISTS the reaction, so `terminal_voltage`
        // subtracted a negative and reported a discharging cell ABOVE its own
        // open-circuit voltage: +26 mV at the V-Cell's design rate, +75 mV at
        // C/10, against an OCV span of 177 mV across the entire state of
        // charge. The artefact was comparable to the whole voltage curve.
        //
        // The exact inverse of symmetric Butler-Volmer has no such branch and
        // needs no guard - it is linear below j0 and becomes Tafel above it.
        let eta_ct = if j0 > 0.0 && current_density.abs() > 1e-6 {
            echem::butler_volmer_overpotential(current_density.abs(), j0, temperature)
        } else {
            0.0
        };

        // Transport. A cell whose deliverable capacity does not fall with rate
        // is not a cell, and this one's did not: `effective_capacity` carried no
        // current term and the diffusion overpotential was passed as a literal
        // zero, so a C/10 and a 2C sweep returned identical amp-hours. The
        // limiting current comes from the separator the cell actually specifies
        // - thickness and ionic conductivity, both already on the state and
        // until now both dead - so rate capability is derived rather than
        // asserted.
        let j_lim = if echem_state.ionic_conductivity > 0.0
            && echem_state.separator_thickness_um > 0.0
        {
            echem::ionic_limiting_current(
                echem_state.ionic_conductivity,
                temperature,
                echem_state.separator_thickness_um * 1e-6,
                1.5,
            )
        } else {
            0.0
        };
        let eta_conc = if j_lim > 0.0 {
            echem::concentration_overpotential(current_density, j_lim, 2.0, temperature)
        } else {
            0.0
        };

        // ── 4. Terminal voltage ──
        let is_discharge = current > 0.0;
        echem_state.terminal_voltage = echem::terminal_voltage(
            ocv, eta_ohmic, eta_ct, eta_conc,
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
        let q_ohmic = echem::ohmic_heat(current, r_eff);
        let q_reaction = echem::reaction_heat(current, eta_ct);
        let entropy_coeff = if echem_state.entropy_coefficient_v_per_k != 0.0 {
            echem_state.entropy_coefficient_v_per_k
        } else {
            constants::na_s::ENTROPY_COEFFICIENT
        };
        // Entropic heat is REVERSIBLE and changes sign with the current: a cell
        // that warms on discharge cools on charge. Taking `.abs()` forced it to
        // heat on both legs, which at the V-Cell's design current is a 157 W
        // error on the charge leg and, at its 0.6 K/W path, a steady-state
        // temperature wrong by tens of kelvin. Temperature feeds the Nernst
        // term, the cycling weight, the calendar clock and the creep rate, so
        // one `.abs()` reached all four fade channels.
        //
        // `reaction_heat` had the mirror-image bug - it went NEGATIVE on charge
        // where polarisation must always dissipate - so the two errors partly
        // cancelled and neither showed up in a temperature trace.
        let q_entropic = echem::entropic_heat(temperature, current, entropy_coeff);
        echem_state.heat_generation = q_ohmic + q_reaction + q_entropic;

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
            let ambient = if echem_state.ambient_temperature_k > 0.0 {
                echem_state.ambient_temperature_k
            } else {
                298.15_f32
            };

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
            // No floor at ambient any more. With the entropic term carrying its
            // real sign a charging cell genuinely absorbs heat and can sit
            // BELOW its surroundings; clamping that away was only safe while
            // the sign bug above guaranteed every term heated. Keep a floor at
            // absolute zero so a pathological authored coefficient cannot take
            // the state negative and poison the Arrhenius exponents.
            thermo_state.temperature = thermo_state.temperature.max(1.0);
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
            // Arrhenius on the parasitic reaction, referenced to 25 °C.
            let f_temp = ((temperature - 298.15) / 20.0).exp().clamp(0.2, 20.0);

            // ── Coulombic loss, derived rather than fitted ──
            //
            // Interphase forms on the real surface of the deposit, so the
            // lithium it consumes is charged PER UNIT AREA, while the charge
            // cycled is area times areal capacity:
            //
            //     1 - CE = R * delta * rho_Li * F / (M_Li * q)
            //
            // Two consequences neither a fitted efficiency nor the separate
            // rate and pressure factors this replaces could express. Loss goes
            // as 1/q, so a THICKER electrode is intrinsically longer-lived,
            // which is the opposite of how areal capacity had been treated
            // everywhere else in this design. And roughness R, not efficiency,
            // is the thing pressure and plating current actually act on: they
            // are inside R here rather than multiplying alongside it, so the
            // model no longer counts the same physics twice.
            let one_minus_ce = if echem_state.sei_thickness_nm > 0.0 {
                const RHO_LI: f32 = 534.0;      // kg/m3
                const M_LI: f32 = 6.941e-3;     // kg/mol
                const FARADAY: f32 = 96485.33;  // C/mol
                // Areal capacity, mAh/cm2, from the cell's own geometry.
                let q_areal = (echem_state.capacity_ah * 0.1
                    / electrode_area.max(1e-6)).max(0.1);
                let k = if echem_state.roughness_k > 0.0 {
                    echem_state.roughness_k
                } else {
                    // Calibrated so 2 MPa at the plating limit reproduces the
                    // 0.995 the branch sweeps were measured against.
                    105.7
                };
                let j_ratio = (plating_density / j_crit.max(1e-6)).clamp(0.0, 2.0);
                let roughness =
                    1.0 + k * j_ratio.powf(1.5) * (2.0 / pressure).powf(1.5);
                let delta_m = echem_state.sei_thickness_nm * 1e-9;
                roughness * delta_m * RHO_LI * FARADAY / (M_LI * q_areal * 3.6e4)
            } else {
                1.0 - ce_ref
            };
            // Calendar ageing is not a fourth independent channel, it is an INPUT
            // to this one. The reduced argyrodite interphase is patchy and more
            // resistive than the bulk it replaces, so it focuses plating current
            // into the low-impedance patches that remain - which is exactly the
            // phenomenon the roughness term describes. A cell that has sat for a
            // year plates worse than a new one, and until this existed the model
            // let the two mechanisms add without either knowing about the other.
            let age_factor = if echem_state.calendar_roughness_beta > 0.0 {
                1.0 + echem_state.calendar_roughness_beta
                    * (echem_state.calendar_hours_equiv as f32 / 8760.0).sqrt()
            } else {
                1.0
            };
            let loss_frac = one_minus_ce * f_depth * f_temp * age_factor;
            // Widen before accumulating, not after: the increment is around
            // 1e-11 of nominal per substep and would vanish into an f32 total.
            echem_state.li_inventory_lost +=
                (loss_frac * (charge_delta_ah.abs() / effective_capacity)) as f64;

            // Cathode fatigue: the second, independent way this cell dies.
            //
            // Li2S <-> S swings the cathode volume by roughly 80 % every cycle.
            // The composite cracks, particles lose contact with the carbon
            // network, and that capacity is gone whether or not any lithium was
            // consumed. The metal reservoir above does nothing about it — a
            // reservoir replaces lost lithium, not a fractured cathode — so
            // this is the mechanism that can end a cell the reservoir sweep
            // says should still be healthy.
            //
            // Coffin-Manson fatigue: damage per cycle goes as strain^m, and the
            // strain here is the excursion depth. Integrating over a full cycle
            // (dQ = 2 * depth * Q) recovers k * depth^2.5 per cycle, so the
            // per-unit-charge form carries half the coefficient.
            let crack_k = if echem_state.crack_k > 0.0 {
                echem_state.crack_k
            } else {
                DEFAULT_CRACK_K
            };
            // Stack pressure holds the composite in compression through a 68 %
            // volume swing and re-closes cracks each cycle, so the crack rate
            // depends on it. The creep block computes a pressure a few dozen
            // lines below and this term ignored the value entirely, which makes
            // the model four single-mechanism models sharing an x-axis rather
            // than one coupled model. A negative exponent makes pressure protect
            // the cathode; a positive one makes it fracture particles directly.
            // The sign is a real open question and is authored, not assumed.
            let crack_p_factor = if echem_state.crack_pressure_exponent != 0.0 {
                (pressure / 2.0).powf(echem_state.crack_pressure_exponent)
            } else {
                1.0
            };
            echem_state.crack_damage += (crack_k
                * crack_p_factor
                * depth.powf(1.5)
                * (charge_delta_ah.abs() / effective_capacity))
                as f64;
        }

        // ── 8b. Calendar fade, and the three-mechanism retention ──
        //
        // Cycle fade is consumed by moving charge. Calendar fade is consumed by
        // sitting still, because the interphase keeps growing on a cell that is
        // merely parked at a state of charge and a temperature. Until this
        // existed a simulated cell left alone aged not at all, so every
        // ten-year figure the engine produced was really a cycle-life figure
        // wearing a calendar label. A passenger car does a few hundred cycles a
        // year against three and a half thousand days parked; for that duty
        // this term is expected to dominate the one above it.
        //
        // Note the scope: this runs on EVERY substep, not only when charge is
        // moving. That is the whole point of it.
        {
            // Interphase growth is diffusion-limited, hence sqrt(t).
            // Accumulating WEIGHTED hours rather than weighting the result is
            // what lets a varying temperature and state of charge integrate
            // correctly instead of being sampled at whatever the last substep
            // happened to be.
            //
            // A fully charged metal anode is the most reactive state the cell
            // ever occupies, which is why storage state of charge is a design
            // variable and not a detail: parking a pack at 40 % rather than
            // 100 % costs nothing in hardware.
            let soc_weight = 0.25 + 1.75 * echem_state.soc.clamp(0.0, 1.0).powi(2);

            // One `exp((T - 298.15) / 20)` used to drive creep, interphase
            // growth AND the parasitic reaction. Sharing a ramp makes the
            // mechanisms' RATIO temperature-invariant by construction, so a cell
            // died of the same thing at -40 C as at 60 C and the optimum stack
            // pressure could never move with temperature. It must: lithium
            // diffuses at about 0.55 eV and the interphase grows at about 0.65,
            // so a hot cell dies of chemistry and a cold one of mechanics. The
            // shared ramp is also 37 kJ/mol, roughly 0.38 eV, which is not
            // either of them.
            //
            // A mechanism with no authored activation energy keeps the legacy
            // ramp exactly, so nothing already measured moves.
            let legacy_ramp = ((temperature - 298.15) / 20.0).exp().clamp(0.2, 20.0);
            let arrhenius_for = |ea: f32| -> f32 {
                if ea <= 0.0 { return legacy_ramp; }
                ((ea / KB_EV_PER_K) * (1.0 / 298.15 - 1.0 / temperature))
                    .exp()
                    .clamp(1.0e-4, 1.0e4)
            };
            let calendar_arrhenius = arrhenius_for(echem_state.calendar_activation_ev);
            let creep_arrhenius = arrhenius_for(echem_state.creep_activation_ev);

            echem_state.calendar_hours_equiv +=
                (dt / 3600.0 * soc_weight * calendar_arrhenius) as f64;

            // ── Lithium creep ──
            //
            // The ceiling on the best lever in the design. Stack pressure is
            // worth 6.7x between 2 and 8 MPa, and until this existed the tick
            // would report a better number at ANY pressure authored, which made
            // it an advocate for its own biggest lever rather than a test of it.
            //
            // Lithium is at 0.66 of its melting point at room temperature, so it
            // creeps under the very pressure that suppresses dendrites. Past the
            // threshold the metal extrudes into the separator instead of
            // densifying, and the cell soft-shorts rather than fading. The
            // exponent is what matters: at 6.6, a factor of 1.25 in pressure is
            // a factor of 4.5 in rate, so the knee is sharp and the safe band
            // has a hard edge rather than a gentle rolloff.
            let creep_threshold = if echem_state.creep_threshold_mpa > 0.0 {
                echem_state.creep_threshold_mpa
            } else {
                DEFAULT_CREEP_THRESHOLD_MPA
            };
            let stack_p = if echem_state.stack_pressure_mpa > 0.0 {
                echem_state.stack_pressure_mpa
            } else {
                2.0
            };
            //
            // Two corrections, both of which the unconfined law got wrong in
            // the same direction.
            //
            // Power-law creep is driven by DEVIATORIC stress. The full stack
            // pressure is the right driver for a billet upset between platens
            // with its sides free; a plated layer is confined between a rigid
            // collector and a rigid ceramic, where only
            // (1 - 2nu)/(1 - nu) = 0.44 of the axial stress is deviatoric. At an
            // exponent of 6.6 that factor alone is 78x on rate.
            //
            // And the deposit is given somewhere to go before it presses on
            // anything: the V-Cell's 28.7 um reserve void against 72.5 um of
            // lithium means the first 0.396 of strain is free. Charging it from
            // the first hour treats a design feature as a defect.
            let p_drive = stack_p * echem_state.deviatoric_fraction();
            if p_drive > creep_threshold {
                let ck = if echem_state.creep_k > 0.0 {
                    echem_state.creep_k
                } else {
                    DEFAULT_CREEP_K
                };
                let over = (p_drive - creep_threshold) / creep_threshold;
                let rate = ck * over.powf(CREEP_EXPONENT) * creep_arrhenius;
                let d_strain = (rate * dt / 3600.0) as f64;
                echem_state.creep_strain += d_strain;

                let void_cap = echem_state.creep_accommodation_frac as f64;
                if echem_state.creep_accommodated < void_cap {
                    echem_state.creep_accommodated =
                        (echem_state.creep_accommodated + d_strain).min(void_cap);
                }
            }

            // ── The fifth channel: bridging ──
            //
            // Extruded metal has to be somewhere, and squeeze flow to a 46 mm
            // free edge is slower than through-thickness flow by roughly
            // (edge / thickness)^2, so most of it goes into the separator. Once
            // a filament spans the film that layer stops contributing voltage.
            //
            // This is charged as layers LOST, not as capacity faded, because
            // that is what it is. It is also the one channel a voltmeter can
            // never see: one bridged layer of 569 moves stack voltage by 0.18 %.
            if echem_state.layer_count > 0
                && echem_state.separator_thickness_um > 0.0
                && echem_state.plated_thickness_um > 0.0
            {
                let chi = if echem_state.bridge_chi > 0.0 {
                    echem_state.bridge_chi
                } else {
                    DEFAULT_BRIDGE_CHI
                };
                let shape = if echem_state.bridge_weibull_m > 0.0 {
                    echem_state.bridge_weibull_m
                } else {
                    DEFAULT_BRIDGE_WEIBULL_M
                };
                let scale = if echem_state.bridge_scale > 0.0 {
                    echem_state.bridge_scale
                } else {
                    DEFAULT_BRIDGE_SCALE
                };
                let extruded_um =
                    echem_state.creep_effective() as f32 * echem_state.plated_thickness_um;
                let penetration =
                    (chi * extruded_um / echem_state.separator_thickness_um).clamp(0.0, 1.0);
                // Weibull over the layer population. Over hundreds of layers the
                // expectation IS the answer, so this stays fractional rather
                // than drawing an integer per layer per substep.
                let bridged_frac =
                    1.0 - (-(penetration / scale).powf(shape)).exp();
                echem_state.shorted_layers =
                    (echem_state.layer_count as f64 * bridged_frac as f64)
                        .max(echem_state.shorted_layers);
            }

            // Four mechanisms, and the earliest one wins. Only lithium loss is
            // buffered by the reservoir; cracking, calendar fade and creep are
            // none of them a lithium-inventory problem, so carrying more metal
            // covers none of them.
            let retention = (1.0 - echem_state.lithium_fade())
                * (1.0 - echem_state.crack_damage)
                * (1.0 - echem_state.calendar_fade())
                * (1.0 - echem_state.creep_effective().min(1.0))
                * (1.0 - echem_state.short_fade());
            echem_state.capacity_retention = (retention as f32).clamp(0.01, 1.0);

            // ── What the reservoir costs ──
            //
            // `li_reservoir_frac` buffers lithium loss and so buys cycles, and
            // it did so for free: nothing in the model charged a gram or a
            // micron for it, so the cycle-life sweep and the mass budget ended
            // up describing two different cells that were quoted in the same
            // sentence. An anode-free cell ships fully discharged with all of
            // its metal held as Li2S, so a reservoir is not spare metal lying
            // about - it is extra cathode, and extra cathode has mass and
            // thickness. This charges for it every tick so the two numbers can
            // never diverge again.
            if echem_state.li_reservoir_frac > 0.0 && echem_state.capacity_ah > 0.0 {
                let reserve_mah = echem_state.li_reservoir_frac
                    * echem_state.capacity_ah
                    * 1000.0;
                let li2s_g = reserve_mah / LI2S_SPECIFIC_CAPACITY_MAH_PER_G;
                echem_state.reservoir_mass_kg =
                    li2s_g * CATHODE_COMPOSITE_FACTOR / 1000.0;
            } else {
                echem_state.reservoir_mass_kg = 0.0;
            }
        }
        } // end substep
    }
}
