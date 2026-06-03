//! # Electrical — circuit simulation, power electronics, motors.
//!
//! Attach Resistor, Capacitor, Inductor, VoltageSource, DcMotor etc. components
//! to any entity to participate in circuit simulation.

pub mod components;
pub mod circuit;
pub mod power;

pub mod prelude {
    pub use super::components::*;
    pub use super::power::{DcMotor, AcMotor, BuckConverter, BoostConverter};
}

use bevy::prelude::*;
use tracing::info;
use components::*;
use power::{DcMotor, AcMotor, BuckConverter, BoostConverter};
use circuit::{
    update_capacitor_system, update_inductor_system,
    update_resistor_system, update_diode_system, update_power_bus_system,
};
use power::{
    update_dc_motor_system, update_ac_motor_system,
    update_buck_converter_system, update_boost_converter_system,
};

pub struct ElectricalPlugin;

impl Plugin for ElectricalPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<ElectricalNode>()
            .register_type::<Resistor>()
            .register_type::<Capacitor>()
            .register_type::<Inductor>()
            .register_type::<VoltageSource>()
            .register_type::<CurrentSource>()
            .register_type::<Diode>()
            .register_type::<CircuitBranch>()
            .register_type::<PowerBus>()
            .register_type::<DcMotor>()
            .register_type::<AcMotor>()
            .register_type::<BuckConverter>()
            .register_type::<BoostConverter>()
            .add_systems(Update, (
                update_capacitor_system,
                update_inductor_system,
                update_resistor_system,
                update_diode_system,
                update_dc_motor_system,
                update_ac_motor_system,
                update_buck_converter_system,
                update_boost_converter_system,
                update_power_bus_system,
            ));
        info!("ElectricalPlugin ready — circuits, motors, power electronics active");
    }
}
