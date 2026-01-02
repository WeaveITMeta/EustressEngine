//! # Materials Science
//!
//! Stress, strain, fracture mechanics, and material properties.
//!
//! ## Table of Contents
//!
//! 1. **Properties** - MaterialProperties component
//! 2. **Stress-Strain** - Hooke's law, stress tensors
//! 3. **Fracture** - Fracture mechanics, crack propagation
//! 4. **Deformation** - Elastic/plastic deformation

pub mod properties;
pub mod stress_strain;
pub mod fracture;
pub mod deformation;

pub mod prelude {
    pub use super::properties::*;
    pub use super::stress_strain::*;
    pub use super::fracture::*;
    pub use super::deformation::*;
    pub use super::MaterialsPlugin;
}

use bevy::prelude::*;

/// Materials science plugin
pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<properties::MaterialProperties>()
            .register_type::<stress_strain::StressTensor>()
            .register_type::<stress_strain::StrainTensor>()
            .register_type::<fracture::FractureState>()
            .register_type::<deformation::DeformationState>()
            .add_systems(Update, (
                stress_strain::update_stress_strain,
                fracture::check_fracture_conditions,
                deformation::apply_deformation,
            ).chain());
        
        info!("MaterialsPlugin initialized");
    }
}
