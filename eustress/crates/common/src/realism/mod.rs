//! # Realism Physics System
//!
//! Physically accurate simulations grounded in fundamental laws of physics.
//!
//! ## Table of Contents
//!
//! 1. **Constants** - Physical constants (R, k_B, G, etc.)
//! 2. **Units** - SI unit system with conversions
//! 3. **Laws** - Thermodynamics, mechanics, conservation
//! 4. **Particles** - High-performance particle ECS
//! 5. **Symbolic** - Symbolica integration for real-time solving
//! 6. **Scripting** - Rune API for dynamic physics
//! 7. **Materials** - Stress, strain, fracture mechanics
//! 8. **Fluids** - SPH, Navier-Stokes, aerodynamics
//! 9. **Visualizers** - Real-time property display
//! 10. **GPU** - WGPU compute shaders for SPH
//! 11. **Quantum** - Bose-Einstein, Fermi-Dirac statistics
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         REALISM PHYSICS SYSTEM                          │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │                                                                         │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
//! │  │  Constants  │  │    Units    │  │    Laws     │  │  Particles  │   │
//! │  │  R, G, k_B  │  │  SI + Conv  │  │ Thermo/Mech │  │  ECS Comps  │   │
//! │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘   │
//! │         │                │                │                │          │
//! │         └────────────────┴────────────────┴────────────────┘          │
//! │                                   │                                    │
//! │                    ┌──────────────┴──────────────┐                    │
//! │                    ▼                             ▼                    │
//! │         ┌─────────────────────┐      ┌─────────────────────┐         │
//! │         │     Symbolica       │      │       Rune          │         │
//! │         │  Symbolic Solving   │      │  Dynamic Scripting  │         │
//! │         └──────────┬──────────┘      └──────────┬──────────┘         │
//! │                    │                            │                     │
//! │                    └────────────┬───────────────┘                     │
//! │                                 ▼                                     │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
//! │  │  Materials  │  │   Fluids    │  │ Visualizers │  │   Avian3D   │  │
//! │  │ Stress/Frac │  │  SPH/Aero   │  │  Overlays   │  │  Integration│  │
//! │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │
//! │                                                                       │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

pub mod constants;
pub mod units;
pub mod laws;
pub mod particles;
pub mod materials;
pub mod fluids;
pub mod visualizers;
pub mod deformation;

#[cfg(feature = "realism-gpu")]
pub mod gpu;

#[cfg(feature = "realism-quantum")]
pub mod quantum;

#[cfg(feature = "realism-symbolic")]
pub mod symbolic;

#[cfg(feature = "realism-scripting")]
pub mod scripting;

use bevy::prelude::*;
use tracing::info;

pub mod prelude {
    pub use super::constants;
    pub use super::units::*;
    pub use super::laws::prelude::*;
    pub use super::particles::prelude::*;
    pub use super::materials::prelude::*;
    pub use super::fluids::prelude::*;
    pub use super::visualizers::prelude::*;
    pub use super::deformation::prelude::*;
    pub use super::{RealismPlugin, RealismConfig};
    
    #[cfg(feature = "realism-symbolic")]
    pub use super::symbolic::prelude::*;
    
    #[cfg(feature = "realism-scripting")]
    pub use super::scripting::prelude::*;
    
    #[cfg(feature = "realism-gpu")]
    pub use super::gpu::prelude::*;
    
    #[cfg(feature = "realism-quantum")]
    pub use super::quantum::prelude::*;
}

// ============================================================================
// Realism Plugin
// ============================================================================

/// Main plugin for the Realism Physics System
pub struct RealismPlugin;

impl Plugin for RealismPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<RealismConfig>()
            .add_plugins((
                particles::ParticlePlugin,
                materials::MaterialsPlugin,
                fluids::FluidsPlugin,
                visualizers::VisualizersPlugin,
                deformation::DeformationPlugin,
            ));
        
        #[cfg(feature = "realism-symbolic")]
        app.add_plugins(symbolic::SymbolicPlugin);
        
        #[cfg(feature = "realism-scripting")]
        app.add_plugins(scripting::ScriptingPlugin);
        
        #[cfg(feature = "realism-gpu")]
        app.add_plugins(gpu::GpuSphPlugin);
        
        #[cfg(feature = "realism-quantum")]
        app.add_plugins(quantum::QuantumPlugin);
        
        info!("RealismPlugin initialized - Physics simulation ready");
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Global configuration for the realism system
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct RealismConfig {
    /// Enable thermodynamic simulation
    pub thermodynamics_enabled: bool,
    /// Enable materials stress/strain simulation
    pub materials_enabled: bool,
    /// Enable fluid dynamics simulation
    pub fluids_enabled: bool,
    /// Enable property visualizers
    pub visualizers_enabled: bool,
    /// Simulation time scale (1.0 = real-time)
    pub time_scale: f32,
    /// Maximum particles for SPH simulation
    pub max_fluid_particles: u32,
    /// Spatial hash cell size for neighbor queries
    pub spatial_cell_size: f32,
    /// Enable parallel processing via Rayon
    pub parallel_enabled: bool,
}

impl Default for RealismConfig {
    fn default() -> Self {
        Self {
            thermodynamics_enabled: true,
            materials_enabled: true,
            fluids_enabled: true,
            visualizers_enabled: true,
            time_scale: 1.0,
            max_fluid_particles: 100_000,
            spatial_cell_size: 1.0,
            parallel_enabled: true,
        }
    }
}
