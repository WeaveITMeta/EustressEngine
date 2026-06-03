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
};

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
fn hydrate_arc_reactor_core_system(
    mut commands: Commands,
    query: Query<(Entity, &Instance), Without<ArcReactorCoreMarker>>,
) {
    for (entity, instance) in &query {
        if instance.class_name != ClassName::ArcReactorCore { continue; }

        commands.entity(entity).insert((
            ArcReactorCoreMarker,
            NuclearKineticsComponent::default(),
            ControlRodBankComponent::default(),
            ThermalHydraulicsComponent::default(),
            PowerConversionComponent::default(),
            VCellBatteryComponent::default(),
            ArcReactorAIController::default(),
        ));

        info!("ArcReactorCore hydrated on entity {:?} ('{}')", entity, instance.name);
    }
}
