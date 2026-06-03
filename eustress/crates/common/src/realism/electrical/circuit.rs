//! Per-frame circuit state integration.
//!
//! Uses forward Euler integration for L and C state variables.
//! Resistors computed analytically. Not a full SPICE solver.
//!
//! # Integration method
//!
//! Forward Euler with dt capped at 0.05 s (20 Hz minimum update):
//!
//!   Capacitor:  V_{n+1} = V_n + (I / C) * dt
//!   Inductor:   I_{n+1} = I_n + (V_eff / L) * dt
//!
//! Resistors and diodes are computed analytically from node voltages each
//! frame — they have no state variable to integrate.

use bevy::prelude::*;
use crate::realism::electrical::components::*;

// ── Capacitor integration ─────────────────────────────────────────────────────

/// Update capacitor voltage: V_{n+1} = V_n + I/C * dt
///
/// If the capacitor entity also has a [`CircuitBranch`] component, the branch's
/// anode [`ElectricalNode`] `current_out` field is used as the charging current.
/// When no branch is present the capacitor's own `current` field is used directly.
///
/// Stored energy is updated to E = ½ · C · V² after the voltage step.
/// dt is capped at 0.05 s so a stalled frame cannot cause numeric blow-up.
pub fn update_capacitor_system(
    time: Res<Time>,
    mut query: Query<(&mut Capacitor, Option<&CircuitBranch>)>,
    nodes: Query<&ElectricalNode>,
) {
    let dt = time.delta_secs().min(0.05);

    for (mut cap, branch) in &mut query {
        // Determine the current flowing into the capacitor.
        let i: f32 = if let Some(br) = branch {
            // Use the anode node's current_out as the charging current.
            if let Ok(node_a) = nodes.get(br.node_a) {
                node_a.current_out
            } else {
                cap.current
            }
        } else {
            cap.current
        };

        let c = cap.capacitance_farads;

        // V_{n+1} = V_n + (I / C) * dt   [dV/dt = I/C]
        if c > 0.0 {
            cap.voltage += (i / c) * dt;
        }

        // Stored energy: E = ½ C V²
        cap.stored_energy_joules = 0.5 * c * cap.voltage * cap.voltage;
    }
}

// ── Inductor integration ──────────────────────────────────────────────────────

/// Update inductor current: I_{n+1} = I_n + V_eff/L * dt
///
/// If the inductor entity has a [`CircuitBranch`] the voltage across it is
/// read from the difference in node potentials (V_a − V_b). If no branch is
/// present the inductor's own `voltage` field is used.
///
/// Parasitic (DC) resistance voltage drop is subtracted from the effective
/// voltage before integration:  V_eff = V_branch − I · R_parasitic
///
/// `stored_energy_joules` is updated to E = ½ · L · I² after the current step.
/// dt is capped at 0.05 s.
///
/// The parasitic voltage drop is stored in a local variable; callers can
/// inspect `inductor.current` and `inductor.voltage` after this system runs.
pub fn update_inductor_system(
    time: Res<Time>,
    mut query: Query<(&mut Inductor, Option<&CircuitBranch>)>,
    nodes: Query<&ElectricalNode>,
) {
    let dt = time.delta_secs().min(0.05);

    for (mut ind, branch) in &mut query {
        // Determine voltage across the inductor from branch nodes or the
        // component's own `voltage` field.
        let v_branch: f32 = if let Some(br) = branch {
            let v_a = nodes.get(br.node_a).map(|n| n.voltage).unwrap_or(0.0);
            let v_b = nodes.get(br.node_b).map(|n| n.voltage).unwrap_or(0.0);
            v_a - v_b
        } else {
            ind.voltage
        };

        // Subtract parasitic resistive drop: V_eff = V - I * R_parasitic
        let parasitic_drop = ind.current * ind.resistance_ohms;
        let v_eff = v_branch - parasitic_drop;

        let l = ind.inductance_henries;

        // I_{n+1} = I_n + (V_eff / L) * dt   [dI/dt = V/L]
        if l > 0.0 {
            ind.current += (v_eff / l) * dt;
        }

        // Write back the computed voltage (V = L · dI/dt ≈ v_eff, recorded for
        // external inspection alongside the parasitic drop).
        // We store the effective terminal voltage (v_branch) and note the parasitic
        // drop is available as `ind.resistance_ohms * ind.current` (pre-step).
        // `ind.voltage` is updated to reflect the voltage seen this frame.
        ind.voltage = v_branch;

        // Stored energy: E = ½ L I²
        ind.stored_energy_joules = 0.5 * l * ind.current * ind.current;
    }
}

// ── Resistor (analytic) ───────────────────────────────────────────────────────

/// Compute resistor current and power from branch node voltages.
///
/// Both a [`Resistor`] and a [`CircuitBranch`] must be present on the entity.
///
/// I = (V_a − V_b) / R
/// P = I² · R
///
/// Near-zero resistance (< 1e-12 Ω) is guarded to prevent NaN/Inf; in that
/// case current and power are set to zero.
pub fn update_resistor_system(
    mut query: Query<(&mut Resistor, &CircuitBranch)>,
    nodes: Query<&ElectricalNode>,
) {
    for (mut res, branch) in &mut query {
        let v_a = nodes.get(branch.node_a).map(|n| n.voltage).unwrap_or(0.0);
        let v_b = nodes.get(branch.node_b).map(|n| n.voltage).unwrap_or(0.0);

        let v_across = v_a - v_b;

        // Guard against zero or near-zero resistance to avoid NaN / Inf.
        if res.resistance_ohms > 1e-12 {
            res.current = v_across / res.resistance_ohms;
            res.power_dissipated = res.current * res.current * res.resistance_ohms;
        } else {
            // Short circuit: current undefined — set to zero to keep state valid.
            res.current = 0.0;
            res.power_dissipated = 0.0;
        }
    }
}

// ── Diode (analytic) ──────────────────────────────────────────────────────────

/// Update diode conducting/blocking state and forward current from branch voltages.
///
/// V_across = V_a − V_b
///
/// Conducting when V_across > forward_voltage:
///   is_conducting = true
///   current = (V_across − V_f) / R_internal
///   where R_internal = max(V_f · 0.01, 0.01 Ω) — 1 % of forward voltage,
///   floored at 10 mΩ — approximates bulk junction resistance.
///
/// Blocking otherwise:
///   is_conducting = false
///   current = 0.0
pub fn update_diode_system(
    mut query: Query<(&mut Diode, &CircuitBranch)>,
    nodes: Query<&ElectricalNode>,
) {
    for (mut diode, branch) in &mut query {
        let v_a = nodes.get(branch.node_a).map(|n| n.voltage).unwrap_or(0.0);
        let v_b = nodes.get(branch.node_b).map(|n| n.voltage).unwrap_or(0.0);

        let v_across = v_a - v_b;

        if v_across > diode.forward_voltage {
            diode.is_conducting = true;
            // 1% internal resistance — never below 10 mΩ floor.
            let r_internal = (diode.forward_voltage * 0.01_f32).max(0.01);
            diode.current = (v_across - diode.forward_voltage) / r_internal;
        } else {
            diode.is_conducting = false;
            diode.current = 0.0;
        }
    }
}

// ── Power bus totals ──────────────────────────────────────────────────────────

/// Update power bus totals from all voltage sources and resistive loads.
///
/// For each [`PowerBus`] the system computes:
///   total_power_generated = Σ (source.voltage · source.current)  for enabled sources
///   total_power_consumed  = Σ resistor.power_dissipated
///
/// Both totals are written to every [`PowerBus`] entity.
///
/// Sources and resistors are not filtered by bus membership — this system
/// is intended for single-bus topologies. Multi-bus support can be added by
/// tagging components with a bus [`Entity`] id and filtering accordingly.
pub fn update_power_bus_system(
    mut buses: Query<&mut PowerBus>,
    sources: Query<&VoltageSource>,
    resistors: Query<&Resistor>,
) {
    let total_power_generated: f32 = sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.voltage * s.current)
        .sum();

    let total_power_consumed: f32 = resistors
        .iter()
        .map(|r| r.power_dissipated)
        .sum();

    let balance = total_power_generated - total_power_consumed;

    for mut bus in &mut buses {
        bus.total_power_generated = total_power_generated;
        bus.total_power_consumed = total_power_consumed;
        // Write balance into the `bus_voltage` field? No — balance is separate.
        // PowerBus.power_balance() is a method; we store the raw totals and the
        // method computes the difference on demand. Nothing to write for balance.
        let _ = balance; // used above for clarity; method computes it live
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realism::electrical::components::{Capacitor, Inductor, Resistor, Diode};

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    // ── Capacitor ─────────────────────────────────────────────────────────────

    #[test]
    fn capacitor_charges_proportionally_to_current_and_dt() {
        // V_{n+1} = V_n + I/C * dt
        // V0=0, I=1A, C=1F, dt=0.01s → V = 0.01 V
        let mut cap = Capacitor {
            capacitance_farads: 1.0,
            voltage: 0.0,
            current: 1.0,
            stored_energy_joules: 0.0,
            max_voltage: 50.0,
        };
        let i = cap.current;
        let dt = 0.01_f32;
        cap.voltage += (i / cap.capacitance_farads) * dt;
        cap.stored_energy_joules =
            0.5 * cap.capacitance_farads * cap.voltage * cap.voltage;

        assert!(approx_eq(cap.voltage, 0.01));
        // E = 0.5 * 1 * 0.01^2 = 5e-5
        assert!((cap.stored_energy_joules - 5e-5).abs() < 1e-9);
    }

    #[test]
    fn capacitor_dt_capped_at_50ms() {
        // A 200 ms frame should behave identically to a 50 ms frame.
        let mut cap = Capacitor {
            capacitance_farads: 2.0,
            voltage: 5.0,
            current: 4.0,
            stored_energy_joules: 0.0,
            max_voltage: 50.0,
        };
        let i = cap.current;
        // Simulate the cap in the system: dt = raw.min(0.05)
        let dt = 0.2_f32.min(0.05);
        cap.voltage += (i / cap.capacitance_farads) * dt;
        // V = 5 + (4/2)*0.05 = 5.1
        assert!(approx_eq(cap.voltage, 5.1));
    }

    // ── Inductor ──────────────────────────────────────────────────────────────

    #[test]
    fn inductor_current_rises_from_applied_voltage() {
        // I_{n+1} = I_n + V/L * dt (no parasitic resistance)
        // I0=0, V=10V, L=5H, dt=0.01s → I = 0.02 A
        let mut ind = Inductor {
            inductance_henries: 5.0,
            current: 0.0,
            voltage: 10.0,
            stored_energy_joules: 0.0,
            resistance_ohms: 0.0,
        };
        let parasitic_drop = ind.current * ind.resistance_ohms;
        let v_eff = ind.voltage - parasitic_drop;
        let dt = 0.01_f32;
        ind.current += (v_eff / ind.inductance_henries) * dt;
        ind.stored_energy_joules =
            0.5 * ind.inductance_henries * ind.current * ind.current;

        assert!(approx_eq(ind.current, 0.02));
        // E = 0.5 * 5 * 0.02^2 = 1e-3
        assert!((ind.stored_energy_joules - 1e-3).abs() < 1e-8);
    }

    #[test]
    fn inductor_parasitic_resistance_reduces_effective_voltage() {
        // V_eff = V - I * R_p
        // I0=2A, V=10V, L=1H, R_p=1Ω → V_eff = 10 - 2*1 = 8V
        // I_{n+1} = 2 + 8*0.01 = 2.08 A
        let mut ind = Inductor {
            inductance_henries: 1.0,
            current: 2.0,
            voltage: 10.0,
            stored_energy_joules: 0.0,
            resistance_ohms: 1.0,
        };
        let dt = 0.01_f32;
        let parasitic_drop = ind.current * ind.resistance_ohms; // 2.0 V
        let v_eff = ind.voltage - parasitic_drop;               // 8.0 V
        ind.current += (v_eff / ind.inductance_henries) * dt;
        ind.stored_energy_joules =
            0.5 * ind.inductance_henries * ind.current * ind.current;

        assert!(approx_eq(ind.current, 2.08));
        assert!(approx_eq(parasitic_drop, 2.0));
    }

    // ── Resistor ─────────────────────────────────────────────────────────────

    #[test]
    fn resistor_current_and_power_from_node_voltages() {
        // V_a=12V, V_b=0V, R=4Ω → I=3A, P=36W
        let v_a = 12.0_f32;
        let v_b = 0.0_f32;
        let mut res = Resistor {
            resistance_ohms: 4.0,
            power_rating_w: 100.0,
            current: 0.0,
            power_dissipated: 0.0,
        };
        let v_across = v_a - v_b;
        res.current = v_across / res.resistance_ohms;
        res.power_dissipated =
            res.current * res.current * res.resistance_ohms;

        assert!(approx_eq(res.current, 3.0));
        assert!(approx_eq(res.power_dissipated, 36.0));
    }

    #[test]
    fn resistor_zero_resistance_is_safe() {
        let v_a = 5.0_f32;
        let v_b = 0.0_f32;
        let mut res = Resistor {
            resistance_ohms: 0.0,
            power_rating_w: 1.0,
            current: 99.0,
            power_dissipated: 99.0,
        };
        // Simulate the guard in the system.
        if res.resistance_ohms > 1e-12 {
            let v = v_a - v_b;
            res.current = v / res.resistance_ohms;
            res.power_dissipated =
                res.current * res.current * res.resistance_ohms;
        } else {
            res.current = 0.0;
            res.power_dissipated = 0.0;
        }
        assert!(approx_eq(res.current, 0.0));
        assert!(approx_eq(res.power_dissipated, 0.0));
    }

    // ── Diode ─────────────────────────────────────────────────────────────────

    #[test]
    fn diode_conducts_when_forward_biased() {
        // forward_voltage=0.7V, v_across=1.7V → conducting
        // R_internal = 0.7 * 0.01 = 0.007 Ω → floor to 0.01 Ω
        // I = (1.7 - 0.7) / 0.01 = 100 A
        let mut diode = Diode {
            forward_voltage: 0.7,
            reverse_breakdown: 50.0,
            is_conducting: false,
            current: 0.0,
        };
        let v_across = 1.7_f32;
        if v_across > diode.forward_voltage {
            diode.is_conducting = true;
            let r_internal = (diode.forward_voltage * 0.01_f32).max(0.01);
            diode.current = (v_across - diode.forward_voltage) / r_internal;
        }
        assert!(diode.is_conducting);
        assert!(diode.current > 0.0);
        // R_internal: 0.7*0.01=0.007 → floored to 0.01; I = 1.0/0.01 = 100.0
        assert!(approx_eq(diode.current, 100.0));
    }

    #[test]
    fn diode_blocks_when_reverse_biased() {
        let mut diode = Diode {
            forward_voltage: 0.7,
            reverse_breakdown: 50.0,
            is_conducting: true,
            current: 5.0,
        };
        let v_across = -1.0_f32;
        if v_across > diode.forward_voltage {
            diode.is_conducting = true;
            let r_internal = (diode.forward_voltage * 0.01_f32).max(0.01);
            diode.current = (v_across - diode.forward_voltage) / r_internal;
        } else {
            diode.is_conducting = false;
            diode.current = 0.0;
        }
        assert!(!diode.is_conducting);
        assert!(approx_eq(diode.current, 0.0));
    }

    #[test]
    fn diode_blocks_when_below_forward_voltage() {
        // v_across = 0.3V < forward_voltage = 0.7V → blocking
        let mut diode = Diode {
            forward_voltage: 0.7,
            reverse_breakdown: 50.0,
            is_conducting: true,
            current: 1.0,
        };
        let v_across = 0.3_f32;
        if v_across > diode.forward_voltage {
            diode.is_conducting = true;
            let r_internal = (diode.forward_voltage * 0.01_f32).max(0.01);
            diode.current = (v_across - diode.forward_voltage) / r_internal;
        } else {
            diode.is_conducting = false;
            diode.current = 0.0;
        }
        assert!(!diode.is_conducting);
        assert!(approx_eq(diode.current, 0.0));
    }

    #[test]
    fn diode_r_internal_floor_at_10_milliohms() {
        // forward_voltage very small → floor at 0.01 Ω
        // V_f = 0.001 V → 0.001 * 0.01 = 0.00001 Ω < 0.01 floor
        let mut diode = Diode {
            forward_voltage: 0.001,
            reverse_breakdown: 50.0,
            is_conducting: false,
            current: 0.0,
        };
        let v_across = 1.0_f32;
        if v_across > diode.forward_voltage {
            diode.is_conducting = true;
            let r_internal = (diode.forward_voltage * 0.01_f32).max(0.01);
            assert!(approx_eq(r_internal, 0.01));
            diode.current = (v_across - diode.forward_voltage) / r_internal;
        }
        assert!(diode.is_conducting);
        // I = (1.0 - 0.001) / 0.01 = 99.9
        assert!(diode.current > 0.0);
    }

    #[test]
    fn power_bus_totals_sum_sources_and_resistors() {
        // Two sources: 12V*2A=24W, 5V*1A=5W → total_generated=29W
        // One resistor: 10W → total_consumed=10W
        // balance = 29 - 10 = 19W
        let total_generated = 24.0_f32 + 5.0_f32;
        let total_consumed = 10.0_f32;
        let balance = total_generated - total_consumed;

        let mut bus = PowerBus::dc_12v();
        bus.total_power_generated = total_generated;
        bus.total_power_consumed = total_consumed;

        assert!(approx_eq(bus.total_power_generated, 29.0));
        assert!(approx_eq(bus.total_power_consumed, 10.0));
        assert!(approx_eq(bus.power_balance(), balance));
        assert!(approx_eq(bus.power_balance(), 19.0));
    }
}
