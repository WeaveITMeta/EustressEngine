//! # Built-in `ExtraSectionClaim` impls
//!
//! First-party claimants that ship with `eustress-common` and get
//! registered by the engine at startup. These prove the
//! extra-section extensibility path on real sections — third-party
//! plugins follow the same trait to attach their own ECS components
//! without touching the loader.
//!
//! ## Why keep one in common instead of engine?
//!
//! The claimant + the component it attaches are both common types
//! (`ThermodynamicState` lives in `realism::particles::prelude`).
//! Registering the claim from a shared crate means headless client
//! binaries, test harnesses, and the studio engine all observe the
//! same behaviour without each having to redeclare it.
//!
//! ## Relation to the typed `InstanceDefinition.thermodynamic` field
//!
//! The engine's `InstanceDefinition` struct still consumes
//! `[thermodynamic]` into a typed `TomlThermodynamicState` field — the
//! Properties panel, V-Cell simulation, and save round-trip all
//! depend on that typed path. The claimant below therefore only
//! fires when `[thermodynamic]` lands in
//! [`super::PendingExtraSections`] *without* being typed-consumed —
//! e.g. a plugin that builds its own spawner and skips
//! `InstanceDefinition`, or a test harness that hands the dispatcher
//! an extras map directly. That's exactly the plugin-extensibility
//! contract in action: plugins get the same entrypoint the engine
//! uses for unknown sections.

use super::{ClaimResult, ExtraSectionClaim};

/// First-party claimant: parses a `[thermodynamic]` section body
/// into a [`ThermodynamicState`](crate::realism::particles::prelude::ThermodynamicState)
/// and inserts it as an ECS component.
///
/// Accepts any subset of the seven canonical fields
/// (`temperature`, `pressure`, `volume`, `internal_energy`,
/// `entropy`, `enthalpy`, `moles`) — missing ones fall back to
/// standard conditions. Key lookup is case-tolerant via
/// [`super::get_section_insensitive`] so legacy PascalCase files work
/// unchanged.
///
/// Rejects non-table bodies with
/// [`ClaimResult::Invalid`] so author-time typos surface as warnings
/// instead of silently skipping the section.
pub struct ThermodynamicClaim;

impl ExtraSectionClaim for ThermodynamicClaim {
    fn section_names(&self) -> &'static [&'static str] {
        &["thermodynamic"]
    }

    fn claim(
        &self,
        _section_name: &str,
        section_value: &toml::Value,
        entity: bevy::ecs::entity::Entity,
        commands: &mut bevy::ecs::system::Commands<'_, '_>,
    ) -> ClaimResult {
        if !section_value.is_table() {
            return ClaimResult::Invalid(
                "[thermodynamic] is not a table".to_string(),
            );
        }

        // Start from standard conditions (25°C, 1 atm, 1 mol) so a
        // half-specified section still yields a physically sane state
        // instead of zero temperature / zero moles.
        let mut state =
            crate::realism::particles::prelude::ThermodynamicState::standard_conditions(1.0);

        let read_f32 = |key: &str| -> Option<f32> {
            super::get_section_insensitive(section_value, key).and_then(|v| {
                v.as_float()
                    .map(|f| f as f32)
                    .or_else(|| v.as_integer().map(|i| i as f32))
            })
        };

        if let Some(v) = read_f32("temperature") { state.temperature = v; }
        if let Some(v) = read_f32("pressure") { state.pressure = v; }
        if let Some(v) = read_f32("volume") { state.volume = v; }
        if let Some(v) = read_f32("internal_energy") { state.internal_energy = v; }
        if let Some(v) = read_f32("entropy") { state.entropy = v; }
        if let Some(v) = read_f32("enthalpy") { state.enthalpy = v; }
        if let Some(v) = read_f32("moles") { state.moles = v; }

        commands.entity(entity).insert(state);
        ClaimResult::Claimed
    }
}
