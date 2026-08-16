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

/// Run condition: the whole pipeline is off unless the host switched it on.
///
/// Deformation is a runtime effect — the engine enables it on Play and clears
/// it on Stop (see `DeformationConfig::enabled`). Gating on plain resource
/// state rather than the engine's `PlayModeState` keeps `eustress-common` free
/// of an engine dependency.
fn deformation_enabled(config: Res<components::DeformationConfig>) -> bool {
    config.enabled
}

impl Plugin for DeformationPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<components::DeformationConfig>()
            .register_type::<components::DeformableMesh>()
            .register_type::<components::VertexDisplacements>()
            .register_type::<components::DeformInitPending>()
            // bevy 0.19: a MessageReader whose type was never registered
            // fails fetch-time validation and the system is SILENTLY skipped
            // every frame ("Message not initialized" warn once at startup).
            // Without these, impact deformation + fracture never ran.
            .add_message::<components::ImpactDeformEvent>()
            .add_message::<components::FractureMeshEvent>()
            // Explicitly ordered: every producer marks `dirty`, and
            // `update_mesh_vertices` is the single consumer that turns those
            // marks into vertex writes. Unchained, Bevy was free to run the
            // consumer before the producers and land each deformation a frame
            // late.
            .add_systems(Update, (
                systems::init_deformable_meshes,
                // Was never registered at all, so `deformation = false` could
                // not tear anything down.
                systems::cleanup_deformable_meshes,
                systems::update_stress_deformation,
                systems::update_thermal_deformation,
                systems::apply_impact_deformation,
                // Was never registered either, so "elastic" deformation never
                // sprang back and was permanent in practice.
                systems::relax_elastic_deformation,
                systems::update_mesh_vertices,
                systems::handle_fracture_mesh,
            ).chain().run_if(deformation_enabled));

        info!("DeformationPlugin initialized - Vertex deformation ready");
    }
}
