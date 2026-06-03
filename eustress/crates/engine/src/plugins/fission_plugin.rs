/// # FissionPlugin — ARC-1 Arc Reactor Core (engine-side hydration)
///
/// Engine-side plugin that hydrates `ArcReactorCore` class entities with
/// their nuclear ECS components.  The physics systems live in `NuclearPlugin`,
/// which is registered by `RealismPlugin` in `main.rs`.

use bevy::prelude::*;
use eustress_common::classes::{ClassName, Instance};
use eustress_common::realism::nuclear::components::{
    ArcReactorCore as ArcReactorCoreMarker,
    NuclearKineticsComponent,
    ControlRodBankComponent,
    ThermalHydraulicsComponent,
    PowerConversionComponent,
    VCellBatteryComponent,
    ArcReactorAIController,
    ReactorControlMode,
    NuclearInit,
};
use eustress_common::realism::nuclear::control_law::DeterministicControlState;

pub struct FissionPlugin;

impl Plugin for FissionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, hydrate_arc_reactor_core_system);
        info!("FissionPlugin active — ARC-1 entity hydration running");
    }
}

// ── Hydration ─────────────────────────────────────────────────────────────────

/// Attaches nuclear ECS components to entities whose `class_name` is
/// `ArcReactorCore` and haven't been hydrated yet (no marker component).
///
/// When a `NuclearInit` carrier component is present (written from the
/// `[nuclear]` TOML section at spawn), its values seed the components instead
/// of the defaults; the carrier is then removed.
fn hydrate_arc_reactor_core_system(
    mut commands: Commands,
    query: Query<(Entity, &Instance, Option<&NuclearInit>), Without<ArcReactorCoreMarker>>,
) {
    for (entity, instance, init) in &query {
        if instance.class_name != ClassName::ArcReactorCore { continue; }

        let mut kinetics = NuclearKineticsComponent::default();
        let mut rods = ControlRodBankComponent::default();
        let mut thermal = ThermalHydraulicsComponent::default();
        let mut conversion = PowerConversionComponent::default();
        let mut battery = VCellBatteryComponent::default();
        let mut ai = ArcReactorAIController::default();

        if let Some(init) = init {
            kinetics.neutron_population = init.neutron_population;
            thermal.core_temp_celsius = init.core_temp_celsius;
            thermal.coolant_flow_pct = init.coolant_flow_pct;
            battery.state_of_charge_pct = init.battery_soc_pct;
            battery.load_demand_watts = init.load_demand_watts;
            rods.bank_a_pct = init.rod_bank_a_pct;
            rods.bank_b_pct = init.rod_bank_b_pct;
            conversion.te_efficiency = init.te_efficiency;
            conversion.stirling_efficiency = init.stirling_efficiency;
            conversion.total_efficiency =
                init.te_efficiency + init.stirling_efficiency * (1.0 - init.te_efficiency);
            ai.mode = if init.ai_regulation_enabled {
                ReactorControlMode::Regulation
            } else {
                ReactorControlMode::Standby
            };
            ai.ai_override_enabled = init.ai_regulation_enabled;
        }

        commands.entity(entity)
            .insert((
                ArcReactorCoreMarker,
                kinetics,
                rods,
                thermal,
                conversion,
                battery,
                ai,
                DeterministicControlState::default(),
            ))
            .remove::<NuclearInit>();

        info!(
            "ArcReactorCore hydrated on entity {:?} ('{}'){}",
            entity, instance.name,
            if init.is_some() { " [from TOML]" } else { " [defaults]" }
        );
    }
}
