//! Bevy update systems for CSTR and Batch reactor simulation.
//!
//! Systems are chained so that concentration changes propagate to heat
//! before the temperature system reads them in the same frame.

use bevy::prelude::*;
use super::components::{ChemicalMixture, ChemicalReaction, CstrReactor, BatchReactor};

// ── Shared helpers ────────────────────────────────────────────────────────────

const RHO_WATER:      f32 = 1_000.0;  // kg/m³  (approximate for aqueous solutions)
const CP_WATER:       f32 = 4_000.0;  // J/(kg·K) (approximate)

/// Instantaneous heat released by a reaction [W].
///   Q = −ΔH_rxn [J/mol] · rate [mol/(L·s)] · volume [L]
///   Note: volume [m³] × 1000 = volume [L]
#[inline]
pub fn net_heat_of_reaction(reaction: &ChemicalReaction, rate_mol_per_l_s: f32, volume_m3: f32) -> f32 {
    -reaction.delta_h_rxn * rate_mol_per_l_s * volume_m3 * 1_000.0
}

// ── CSTR system ────────────────────────────────────────────────────────────────

/// Update CSTR composition each frame using forward Euler integration.
///
/// Species balance:
///   dCᵢ/dt = D·(Cᵢ_feed − Cᵢ)  ±  νᵢ · r_net
/// where D = Q_feed / V  (dilution rate [s⁻¹]).
///
/// Heat is written to `mixture.heat_generated_w` for the temperature system.
pub fn update_cstr_system(
    time: Res<Time>,
    mut query: Query<(&mut ChemicalMixture, &mut CstrReactor, Option<&ChemicalReaction>)>,
) {
    let dt = time.delta_secs().min(0.05_f32);

    for (mut mixture, mut reactor, maybe_rxn) in &mut query {
        let d = reactor.dilution_rate();       // s⁻¹
        let v = mixture.volume_m3;

        // Compute net reaction rate (zero when no reaction component attached)
        let r_net = match &maybe_rxn {
            Some(rxn) => rxn.net_rate(&mixture),
            None      => 0.0,
        };

        // Euler integration for each species
        let species_names: Vec<String> = mixture.species.iter().map(|s| s.name.clone()).collect();
        for name in &species_names {
            let c_current = mixture.concentration(name).unwrap_or(0.0);
            let c_feed    = reactor.feed_concentrations.get(name).copied().unwrap_or(0.0);

            // Dilution contribution
            let mut dc_dt = d * (c_feed - c_current);

            // Reaction contribution
            if let Some(rxn) = &maybe_rxn {
                // Reactants are consumed
                if let Some(pos) = rxn.reactant_names.iter().position(|n| n == name) {
                    dc_dt -= rxn.reactant_stoich[pos] * r_net;
                }
                // Products are formed
                if let Some(pos) = rxn.product_names.iter().position(|n| n == name) {
                    dc_dt += rxn.product_stoich[pos] * r_net;
                }
            }

            let c_new = (c_current + dc_dt * dt).max(0.0);
            mixture.set_concentration(name, c_new);
        }

        // Also accept feed species that may not yet be in the mixture
        let feed_names: Vec<String> = reactor.feed_concentrations.keys().cloned().collect();
        for name in feed_names {
            if mixture.concentration(&name).is_none() {
                let c_feed = reactor.feed_concentrations[&name];
                let dc_dt = d * c_feed;
                let c_new = (dc_dt * dt).max(0.0);
                mixture.set_concentration(&name, c_new);
            }
        }

        // Update conversion (first reactant as key species)
        if let Some(rxn) = &maybe_rxn {
            if let Some(key_name) = rxn.reactant_names.first() {
                let c_feed = reactor.feed_concentrations.get(key_name).copied().unwrap_or(1.0);
                if c_feed > 0.0 {
                    let c_now = mixture.concentration(key_name).unwrap_or(0.0);
                    reactor.conversion = ((c_feed - c_now) / c_feed).clamp(0.0, 1.0);
                }
            }
        }

        // Heat generated this frame
        mixture.heat_generated_w = match &maybe_rxn {
            Some(rxn) => net_heat_of_reaction(rxn, r_net, v),
            None      => 0.0,
        };
    }
}

// ── Batch reactor system ──────────────────────────────────────────────────────

/// Update Batch reactor composition (no feed flow, closed vessel).
///
///   dCᵢ/dt = νᵢ · r_net(T, C)
pub fn update_batch_system(
    time: Res<Time>,
    mut query: Query<(&mut ChemicalMixture, &mut BatchReactor, Option<&ChemicalReaction>)>,
) {
    let dt = time.delta_secs().min(0.05_f32);

    for (mut mixture, mut reactor, maybe_rxn) in &mut query {
        reactor.time_elapsed_s += dt;
        let v = mixture.volume_m3;

        let r_net = match &maybe_rxn {
            Some(rxn) => rxn.net_rate(&mixture),
            None      => 0.0,
        };

        // Integrate species
        let species_names: Vec<String> = mixture.species.iter().map(|s| s.name.clone()).collect();
        for name in &species_names {
            let c_current = mixture.concentration(name).unwrap_or(0.0);
            let mut dc_dt = 0.0_f32;

            if let Some(rxn) = &maybe_rxn {
                if let Some(pos) = rxn.reactant_names.iter().position(|n| n == name) {
                    dc_dt -= rxn.reactant_stoich[pos] * r_net;
                }
                if let Some(pos) = rxn.product_names.iter().position(|n| n == name) {
                    dc_dt += rxn.product_stoich[pos] * r_net;
                }
            }

            mixture.set_concentration(name, (c_current + dc_dt * dt).max(0.0));
        }

        // Achieved conversion for key reactant
        if let Some(rxn) = &maybe_rxn {
            if let Some(key_name) = rxn.reactant_names.first() {
                let c0    = mixture.concentration(key_name).unwrap_or(1.0).max(1e-12);
                let c_now = mixture.concentration(key_name).unwrap_or(c0);
                let alpha = ((c0 - c_now) / c0).clamp(0.0, 1.0);
                reactor.achieved_conversion = alpha;
            }
        }

        mixture.heat_generated_w = match &maybe_rxn {
            Some(rxn) => net_heat_of_reaction(rxn, r_net, v),
            None      => 0.0,
        };
    }
}

// ── Temperature system ────────────────────────────────────────────────────────

/// Update mixture temperature from reaction heat and jacket cooling.
///
///   ρ·Cp·V · dT/dt = Q_rxn − Q_removal
///   → dT/dt = (Q_rxn − Q_removal) / (ρ·Cp·V)
pub fn update_reactor_temperature_system(
    time: Res<Time>,
    mut query: Query<(
        &mut ChemicalMixture,
        Option<&CstrReactor>,
        Option<&BatchReactor>,
    )>,
) {
    let dt = time.delta_secs().min(0.05_f32);

    for (mut mixture, maybe_cstr, maybe_batch) in &mut query {
        let q_rxn     = mixture.heat_generated_w;
        let q_removal = maybe_cstr.map(|r| r.heat_removal_w)
            .or_else(|| maybe_batch.map(|r| r.heat_removal_w))
            .unwrap_or(0.0);
        let v = mixture.volume_m3;

        // Lumped thermal mass: ρ_water × Cp_water × V
        let thermal_mass = RHO_WATER * CP_WATER * v;  // J/K

        let dt_rxn = if thermal_mass > 0.0 {
            (q_rxn - q_removal) / thermal_mass
        } else {
            0.0
        };

        mixture.temperature_k = (mixture.temperature_k + dt_rxn * dt).clamp(0.0, 5_000.0);
    }
}
