//! # Mesh Deformation System
//!
//! Vertex-level deformation from stress, temperature, and impacts.
//!
//! ## Table of Contents
//!
//! 1. **DeformableMesh** - Component linking mesh to deformation state
//! 2. **VertexDeformation** - Per-vertex displacement data
//! 3. **Systems** - Update vertex positions from physics
//! 4. **GPU Compute** - Shader-based vertex updates
//!
//! ## Architecture
//!
//! When `BasePart.deformation = true`:
//! - Mesh vertices are displaced based on stress tensor
//! - Temperature gradients cause thermal expansion/contraction
//! - Impact forces create permanent plastic deformation
//! - Fracture propagation splits mesh geometry

pub mod components;
pub mod systems;
pub mod vertex;
pub mod fracture_mesh;
pub mod gpu_deform;

pub mod prelude {
    pub use super::components::*;
    pub use super::systems::*;
    pub use super::vertex::*;
    pub use super::fracture_mesh::*;
    pub use super::DeformationPlugin;
}

use bevy::prelude::*;
use tracing::info;

/// Mesh deformation plugin
pub struct DeformationPlugin;

impl Plugin for DeformationPlugin {
    fn build(&self, app: &mut App) {
        // Event registration DISABLED for Bevy 0.19
        // TODO: Fix event registration for Bevy 0.19
        app
            .register_type::<components::DeformableMesh>()
            .register_type::<components::DeformationState>()
            .add_systems(Update, (
                systems::init_deformable_meshes,
                systems::update_stress_deformation,
                systems::update_thermal_deformation,
                systems::apply_impact_deformation,
                systems::update_mesh_vertices,
                systems::handle_fracture_mesh,
            ));
        
        info!("DeformationPlugin initialized - Vertex deformation ready");
    }
}
