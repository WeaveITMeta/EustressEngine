//! # Chemistry — species, reactions, reactors, combustion, equilibrium.

pub mod components;
pub mod combustion;
pub mod equilibrium;
pub mod reactor;

pub mod prelude {
    pub use super::components::{ChemicalSpecies, ChemicalMixture, ChemicalReaction, CstrReactor, BatchReactor};
    pub use super::combustion::*;
    pub use super::equilibrium::*;
}

use bevy::prelude::*;
use tracing::info;
use components::{ChemicalMixture, ChemicalReaction, CstrReactor, BatchReactor};
use reactor::{update_cstr_system, update_batch_system, update_reactor_temperature_system};

pub struct ChemistryPlugin;

impl Plugin for ChemistryPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<ChemicalMixture>()
            .register_type::<ChemicalReaction>()
            .register_type::<CstrReactor>()
            .register_type::<BatchReactor>()
            .add_systems(Update, (
                update_cstr_system,
                update_batch_system,
                update_reactor_temperature_system,
            ).chain());
        info!("ChemistryPlugin ready — reactions, combustion, equilibrium active");
    }
}
