/// # Deterministic Reactor Control Law
///
/// Replaces the three-loop PID controller once enough simulation data has been
/// collected and analysed (Phase 1→2→3 Workshop pipeline).
///
/// Architecture:
///
/// ```text
///  load_demand_W ──► feedforward_rod_pct()   ──► ControlRodBankComponent
///  T_core        ──► feedforward_flow_pct()  ──► ThermalHydraulicsComponent
///  n_measured    ──► proportional_trim()     ──► fine rod correction
///  safety_check  ──► envelope clamp          ──► hard limits
/// ```
///
/// Population from the Workshop analysis populates `FeedforwardCoefficients`.
/// Until that data exists the struct ships with the physics-derived analytical
/// first-guess so the system is usable from day one.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::components::*;
use super::constants::*;

// ── Coefficients ──────────────────────────────────────────────────────────────

/// Fitted coefficients from Phase 2 Workshop analysis.
///
/// Populated by `docs/arc1/feedforward_coefficients.toml` via the
/// `load_feedforward_coefficients_system`.  Falls back to analytical
/// first-guess until the file is written.
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct FeedforwardCoefficients {
    // Rod law:  rod_pct = a_rod · load_W + b_rod
    pub a_rod: f32,
    pub b_rod: f32,

    // Coolant law: flow_pct = c_flow · p_thermal_W + d_flow
    pub c_flow: f32,
    pub d_flow: f32,

    // Proportional trim on neutron population (replaces integral in PID)
    pub kp_neutron_trim: f32,

    // Safety envelope hard limits (from Phase 2-C)
    pub rod_min_safe_pct:  f32,
    pub rod_max_safe_pct:  f32,
    pub flow_min_safe_pct: f32,

    // Whether these coefficients come from data (true) or the analytical guess
    pub data_validated: bool,
}

impl Default for FeedforwardCoefficients {
    fn default() -> Self {
        // Analytical first-guess derived from the ARC-1 physics:
        //
        // At steady state:   P_th ≈ (1 − rod_f · 1.5) · n · P_rated
        // With n≈1.0:        rod_f = (1 − P_th / P_rated) / 1.5
        // rod_pct = rod_f · 200 / 2 = rod_f · 100
        //
        // For P_out = load_W:  P_th = load_W / η_total
        // With η_total ≈ 0.361: P_th = load_W / 0.361
        // rod_f = (1 − load_W / (0.361 · P_rated)) / 1.5
        //       = (1 − load_W / 1155) / 1.5
        //
        // rod_pct = (1 − load_W/1155) / 1.5 · 100
        //         = 66.7 − 0.0578 · load_W
        //
        // Coolant:  at nominal, 70% flow handles P_th ≈ 1163W.
        // Linear fit: flow_pct = 60 + P_th · (10 / 1163) ≈ 60 + 0.0086 · P_th
        Self {
            a_rod:           -0.0578,
            b_rod:           66.7,
            c_flow:          0.0086,
            d_flow:          60.0,
            kp_neutron_trim: 5.0,
            rod_min_safe_pct:  20.0,
            rod_max_safe_pct:  95.0,
            flow_min_safe_pct: 15.0,
            data_validated:    false,
        }
    }
}

// ── Control state ─────────────────────────────────────────────────────────────

/// Per-reactor state for the deterministic control law.
/// Fully STATELESS — no integral, no history.  Identical inputs always
/// produce identical outputs, making it provably verifiable.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize, Default)]
#[reflect(Component)]
pub struct DeterministicControlState {
    /// Last computed feedforward rod insertion [%].
    pub last_rod_ff_pct: f32,
    /// Last computed feedforward coolant flow [%].
    pub last_flow_ff_pct: f32,
    /// Last proportional trim applied to rods.
    pub last_trim_pct: f32,
}

// ── Core computation ──────────────────────────────────────────────────────────

/// Compute the feedforward rod insertion for a given load demand.
///
/// This is the steady-state operating point — no transient dynamics.
/// The proportional trim (below) handles small deviations from steady state.
#[inline]
pub fn feedforward_rod_pct(load_demand_w: f32, coeffs: &FeedforwardCoefficients) -> f32 {
    let raw = coeffs.a_rod * load_demand_w + coeffs.b_rod;
    raw.clamp(coeffs.rod_min_safe_pct, coeffs.rod_max_safe_pct)
}

/// Compute the feedforward coolant flow for a given thermal power.
#[inline]
pub fn feedforward_flow_pct(thermal_power_w: f32, coeffs: &FeedforwardCoefficients) -> f32 {
    let raw = coeffs.c_flow * thermal_power_w + coeffs.d_flow;
    raw.clamp(coeffs.flow_min_safe_pct, 100.0)
}

/// Proportional trim: small correction on rod insertion to drive n → 1.0.
/// Deliberately no integral (that's what makes it "deterministic" — no windup).
#[inline]
pub fn proportional_neutron_trim(
    n_measured: f32,
    n_setpoint: f32,
    kp: f32,
) -> f32 {
    let error = n_setpoint - n_measured;
    // Positive error (n < setpoint) → need more power → withdraw rods (negative Δrod)
    (-kp * error).clamp(-15.0, 15.0)
}

// ── Safety envelope ───────────────────────────────────────────────────────────

/// Result of a safety check — what the controller must do.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlAction {
    /// Computed values are safe — use them.
    Normal { rod_pct: f32, flow_pct: f32 },
    /// One or both values were clamped to a safe boundary.
    Clamped { rod_pct: f32, flow_pct: f32, reason: &'static str },
    /// Imminent danger — override everything, insert rods fully.
    EmergencyInsert,
}

/// Run the full control law for one frame:
///   1. feedforward (load → rod + flow)
///   2. proportional trim (n error → rod correction)
///   3. safety envelope clamp
pub fn compute_control_output(
    load_demand_w: f32,
    thermal_power_w: f32,
    n_measured: f32,
    n_setpoint: f32,
    t_core: f32,
    coeffs: &FeedforwardCoefficients,
) -> ControlAction {
    // Emergency override: temperature within 100°C of SCRAM limit
    if t_core > SCRAM_TEMP_CELSIUS - 100.0 {
        return ControlAction::EmergencyInsert;
    }
    // Emergency override: neutron excursion
    if n_measured > 3.0 {
        return ControlAction::EmergencyInsert;
    }

    let rod_ff   = feedforward_rod_pct(load_demand_w, coeffs);
    let flow_ff  = feedforward_flow_pct(thermal_power_w, coeffs);
    let rod_trim = proportional_neutron_trim(n_measured, n_setpoint, coeffs.kp_neutron_trim);

    let rod_pct  = (rod_ff + rod_trim).clamp(coeffs.rod_min_safe_pct, coeffs.rod_max_safe_pct);
    let flow_pct = flow_ff.clamp(coeffs.flow_min_safe_pct, 100.0);

    let clamped = (rod_pct - (rod_ff + rod_trim)).abs() > 0.1
                || (flow_pct - flow_ff).abs() > 0.1;

    if clamped {
        ControlAction::Clamped { rod_pct, flow_pct, reason: "safety envelope clamp" }
    } else {
        ControlAction::Normal { rod_pct, flow_pct }
    }
}

// ── Bevy system ───────────────────────────────────────────────────────────────

/// Update system: runs the deterministic control law when the reactor is in
/// `ReactorControlMode::DeterministicLaw` mode.
pub fn deterministic_control_law_system(
    coeffs: Res<FeedforwardCoefficients>,
    mut query: Query<(
        &mut ControlRodBankComponent,
        &mut ThermalHydraulicsComponent,
        &mut DeterministicControlState,
        &ArcReactorAIController,
        &NuclearKineticsComponent,
        &VCellBatteryComponent,
    ), With<ArcReactorCore>>,
) {
    for (mut rods, mut thermal, mut det_state, ai, kin, batt) in &mut query {
        if ai.mode != ReactorControlMode::DeterministicLaw { continue; }
        if kin.is_scrammed { continue; }

        let action = compute_control_output(
            batt.load_demand_watts,
            thermal.thermal_power_watts,
            kin.neutron_population,
            ai.neutron_setpoint,
            thermal.core_temp_celsius,
            &coeffs,
        );

        match action {
            ControlAction::Normal { rod_pct, flow_pct } => {
                det_state.last_rod_ff_pct  = rod_pct;
                det_state.last_flow_ff_pct = flow_pct;
                det_state.last_trim_pct    = rod_pct - feedforward_rod_pct(batt.load_demand_watts, &coeffs);
                rods.bank_a_pct   = rod_pct;
                rods.bank_b_pct   = rod_pct;
                thermal.coolant_flow_pct = flow_pct;
            }
            ControlAction::Clamped { rod_pct, flow_pct, .. } => {
                det_state.last_rod_ff_pct  = rod_pct;
                det_state.last_flow_ff_pct = flow_pct;
                rods.bank_a_pct   = rod_pct;
                rods.bank_b_pct   = rod_pct;
                thermal.coolant_flow_pct = flow_pct;
            }
            ControlAction::EmergencyInsert => {
                rods.bank_a_pct   = 100.0;
                rods.bank_b_pct   = 100.0;
                thermal.coolant_flow_pct = 100.0;
            }
        }
    }
}

// ── Coefficient loader ────────────────────────────────────────────────────────

/// TOML schema for docs/arc1/feedforward_coefficients.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedforwardCoefficientsFile {
    pub a_rod:              f32,
    pub b_rod:              f32,
    pub c_flow:             f32,
    pub d_flow:             f32,
    pub kp_neutron_trim:    f32,
    pub rod_min_safe_pct:   f32,
    pub rod_max_safe_pct:   f32,
    pub flow_min_safe_pct:  f32,
    pub data_validated:     bool,
}

impl From<FeedforwardCoefficientsFile> for FeedforwardCoefficients {
    fn from(f: FeedforwardCoefficientsFile) -> Self {
        Self {
            a_rod:              f.a_rod,
            b_rod:              f.b_rod,
            c_flow:             f.c_flow,
            d_flow:             f.d_flow,
            kp_neutron_trim:    f.kp_neutron_trim,
            rod_min_safe_pct:   f.rod_min_safe_pct,
            rod_max_safe_pct:   f.rod_max_safe_pct,
            flow_min_safe_pct:  f.flow_min_safe_pct,
            data_validated:     f.data_validated,
        }
    }
}

// ── ReactorControlMode extension ─────────────────────────────────────────────

// Add DeterministicLaw to ReactorControlMode in components.rs.
// This extension is handled at the components level; see comments there.
// The system above gates on ReactorControlMode::DeterministicLaw (value = 3).
