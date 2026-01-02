//! # GPU Compute for SPH
//!
//! WGPU-based compute shaders for fluid simulation.
//!
//! ## Table of Contents
//!
//! 1. **SphGpuPipeline** - Compute pipeline for SPH
//! 2. **Buffers** - GPU buffer management
//! 3. **Shaders** - WGSL compute shaders
//!
//! ## Architecture
//!
//! Uses Bevy's wgpu integration for GPU-accelerated SPH:
//! - Density estimation compute pass
//! - Pressure/viscosity force compute pass
//! - Position integration compute pass

#[cfg(feature = "realism-gpu")]
pub mod pipeline;
#[cfg(feature = "realism-gpu")]
pub mod buffers;
#[cfg(feature = "realism-gpu")]
pub mod shaders;
#[cfg(feature = "realism-gpu")]
pub mod compute;

pub mod prelude {
    #[cfg(feature = "realism-gpu")]
    pub use super::pipeline::*;
    #[cfg(feature = "realism-gpu")]
    pub use super::buffers::*;
    #[cfg(feature = "realism-gpu")]
    pub use super::GpuSphPlugin;
}

use bevy::prelude::*;

/// GPU SPH compute plugin
#[cfg(feature = "realism-gpu")]
pub struct GpuSphPlugin;

#[cfg(feature = "realism-gpu")]
impl Plugin for GpuSphPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<pipeline::SphGpuConfig>()
            .add_systems(Startup, pipeline::setup_sph_pipeline)
            .add_systems(Update, (
                buffers::sync_particles_to_gpu,
                pipeline::dispatch_sph_compute,
                buffers::sync_particles_from_gpu,
            ).chain());
        
        info!("GpuSphPlugin initialized - WGPU compute ready");
    }
}

/// Placeholder plugin when feature is disabled
#[cfg(not(feature = "realism-gpu"))]
pub struct GpuSphPlugin;

#[cfg(not(feature = "realism-gpu"))]
impl Plugin for GpuSphPlugin {
    fn build(&self, _app: &mut App) {
        info!("GpuSphPlugin disabled - enable 'realism-gpu' feature");
    }
}
