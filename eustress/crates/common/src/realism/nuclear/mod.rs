/// # Nuclear Fission Simulation — ARC-1 Arc Reactor Core
///
/// Implements a one-group point kinetics model, thermal-hydraulics balance,
/// power conversion, V-Cell battery buffer, and a three-loop PID controller
/// for autonomous stability management.
///
/// ## Physics chain (each frame, in order)
///
/// ```text
/// ControlRodBankComponent ──► update_reactivity
///                                    │
///                                    ▼
/// NuclearKineticsComponent ◄── update_nuclear_kinetics
///                                    │
///                                    ▼
/// ThermalHydraulicsComponent ◄── update_thermal_hydraulics
///                                    │
///                                    ▼
/// PowerConversionComponent ◄── update_power_conversion
///                                    │
///                                    ▼
/// VCellBatteryComponent ◄── update_battery_buffer
///                                    │
///                                    ▼
/// ArcReactorAIController ◄── update_ai_controller   ──► rod + flow corrections
///                                    │
///                                    ▼
/// (limits check)         ◄── nuclear_safety_monitor ──► ReactorScramMessage
///                                                              │
///                                                              ▼
///                                    ◄── execute_scram  (rods insert, coolant max)
///                                    │
///                                    ▼
/// WatchPointRegistry     ◄── publish_nuclear_watchpoints  (Rune/dashboard reads)
/// ```

pub mod constants;
pub mod components;
pub mod systems;
pub mod control_law;
// Phase J extensions — pure-function libraries
pub mod decay;
pub mod shielding;
pub mod criticality;

pub mod prelude {
    pub use super::components::*;
    pub use super::control_law::{
        FeedforwardCoefficients, DeterministicControlState,
        feedforward_rod_pct, feedforward_flow_pct, compute_control_output,
    };
    pub use super::constants;
    // Phase J: namespaced (not globbed) to avoid name collisions with kinetics
    pub use super::{decay, shielding, criticality};
}

use bevy::prelude::*;
use tracing::info;
use components::*;
use systems::*;
use control_law::{FeedforwardCoefficients, DeterministicControlState, deterministic_control_law_system};
use crate::simulation::{SimulationClock, WatchPointRegistry};

/// Bevy plugin that registers all ARC-1 nuclear simulation ECS types and systems.
pub struct NuclearPlugin;

impl Plugin for NuclearPlugin {
    fn build(&self, app: &mut App) {
        // Init required resources if not already present
        app.init_resource::<SimulationClock>();
        app.init_resource::<WatchPointRegistry>();

        // Deterministic control law resource (falls back to analytical defaults
        // until docs/arc1/feedforward_coefficients.toml is written by the Workshop)
        app.init_resource::<FeedforwardCoefficients>();

        // Register types for Reflect / serialization
        app
            .register_type::<ArcReactorCore>()
            .register_type::<NuclearInit>()
            .register_type::<NuclearKineticsComponent>()
            .register_type::<ControlRodBankComponent>()
            .register_type::<ThermalHydraulicsComponent>()
            .register_type::<PowerConversionComponent>()
            .register_type::<VCellBatteryComponent>()
            .register_type::<ArcReactorAIController>()
            .register_type::<PidState>()
            .register_type::<ReactorControlMode>()
            .register_type::<FeedforwardCoefficients>()
            .register_type::<DeterministicControlState>();

        // Register the SCRAM message (Bevy 0.18 Messages API)
        app.add_message::<ReactorScramMessage>();

        // Physics chain — split into two sets to stay within tuple-size limits.
        // Set A: compute new state values (no ordering dependencies on Set B)
        app.add_systems(Update, (
            update_reactivity_system,
            update_nuclear_kinetics_system,
            update_thermal_hydraulics_system,
            update_power_conversion_system,
            update_battery_buffer_system,
        ).chain());

        // Set B: control + safety + telemetry (runs after Set A each frame)
        app.add_systems(Update, (
            update_ai_controller_system,
            deterministic_control_law_system,  // runs alongside PID; only one is active per mode
            nuclear_safety_monitor_system,
            execute_scram_system,
            publish_nuclear_watchpoints_system,
        ).chain().after(update_battery_buffer_system));

        info!("NuclearPlugin initialised — ARC-1 fission simulation ready");
    }
}
