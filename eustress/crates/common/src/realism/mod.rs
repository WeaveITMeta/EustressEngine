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
pub mod lod;
pub mod particles;
pub mod materials;
pub mod fluids;
pub mod visualizers;
pub mod deformation;
pub mod thermal_conduction;
pub mod nuclear;
// STEM Stack — Phases A-D
pub mod numerics;
pub mod electrical;
pub mod chemistry;
pub mod control;
// STEM Stack — Phases E-L
pub mod structures;
pub mod thermocycles;
pub mod propulsion;
pub mod plasma;
// Particle simulations: the solver and Bevy integration behind the
// `ParticleSimulation` / `ParticleSpecies` classes.
pub mod particle_sim;

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
    pub use super::nuclear::prelude::*;
    pub use super::numerics::prelude::*;
    pub use super::electrical::prelude::*;
    pub use super::chemistry::prelude::*;
    pub use super::control::prelude::*;
    pub use super::structures::prelude::*;
    pub use super::thermocycles::prelude::*;
    pub use super::propulsion::prelude::*;
    pub use super::plasma::prelude::*;
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

/// One schedulable domain of physics, gated by a flag on
/// [`PhysicsService`](crate::services::physics::PhysicsService).
///
/// Every variant corresponds to systems that ACTUALLY run each step. The
/// realism module also ships `structures`, `plasma`, `propulsion`, `control`,
/// `thermocycles` and `numerics`, whose plugins register types and expose pure
/// functions but schedule nothing, so they get no variant here: a toggle that
/// gates no work is indistinguishable, from the outside, from a capability the
/// engine does not have.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PhysicsDomain {
    /// Rigid bodies, collision detection and the constraint solver (Avian).
    Kinematics,
    /// Heat conduction and transfer.
    Thermodynamics,
    /// Reactions, combustion, reactor temperature.
    Chemistry,
    /// Circuits, motors, power electronics.
    Electricity,
    /// Impact denting and fracture into separate bodies.
    Deformation,
    /// Smoothed-particle fluid dynamics.
    Fluids,
    /// Stress and strain response of materials.
    Materials,
    /// Thermal and kinetic stepping of particle species.
    Particles,
    /// The divergence-free SPH particle solver.
    ParticleSimulation,
    /// Decay chains and reactor kinetics.
    Nuclear,
    /// Property overlays that draw simulation state into the viewport.
    Visualizers,
}

impl PhysicsDomain {
    /// Every domain, in declaration order. Used to configure the run
    /// conditions in one pass so a new variant cannot be added without being
    /// gated.
    pub const ALL: [PhysicsDomain; 11] = [
        PhysicsDomain::Kinematics,
        PhysicsDomain::Thermodynamics,
        PhysicsDomain::Chemistry,
        PhysicsDomain::Electricity,
        PhysicsDomain::Deformation,
        PhysicsDomain::Fluids,
        PhysicsDomain::Materials,
        PhysicsDomain::Particles,
        PhysicsDomain::ParticleSimulation,
        PhysicsDomain::Nuclear,
        PhysicsDomain::Visualizers,
    ];

    /// The name shown in the PhysicsService properties panel.
    pub fn label(self) -> &'static str {
        match self {
            PhysicsDomain::Kinematics => "Kinematics",
            PhysicsDomain::Thermodynamics => "Thermodynamics",
            PhysicsDomain::Chemistry => "Chemistry",
            PhysicsDomain::Electricity => "Electricity",
            PhysicsDomain::Deformation => "Deformation",
            PhysicsDomain::Fluids => "Fluids",
            PhysicsDomain::Materials => "Materials",
            PhysicsDomain::Particles => "Particles",
            PhysicsDomain::ParticleSimulation => "ParticleSimulation",
            PhysicsDomain::Nuclear => "Nuclear",
            PhysicsDomain::Visualizers => "Visualizers",
        }
    }

    /// The property key this domain's flag is stored under in the
    /// PhysicsService `_service.toml`, and the field name agents send over
    /// MCP. snake_case, matching every other service property.
    pub fn key(self) -> &'static str {
        match self {
            PhysicsDomain::Kinematics => "kinematics",
            PhysicsDomain::Thermodynamics => "thermodynamics",
            PhysicsDomain::Chemistry => "chemistry",
            PhysicsDomain::Electricity => "electricity",
            PhysicsDomain::Deformation => "deformation",
            PhysicsDomain::Fluids => "fluids",
            PhysicsDomain::Materials => "materials",
            PhysicsDomain::Particles => "particles",
            PhysicsDomain::ParticleSimulation => "particle_simulation",
            PhysicsDomain::Nuclear => "nuclear",
            PhysicsDomain::Visualizers => "visualizers",
        }
    }

    /// What switching this domain off stops. Shown to agents over MCP, so it
    /// names the concrete work rather than the category.
    pub fn description(self) -> &'static str {
        match self {
            PhysicsDomain::Kinematics => {
                "Rigid bodies, collision detection, and the constraint solver. Off freezes every body in place."
            }
            PhysicsDomain::Thermodynamics => {
                "Heat conduction and transfer between touching bodies, and particle heat exchange."
            }
            PhysicsDomain::Chemistry => {
                "Reactions, combustion, batch and continuous reactors, and reactor temperature."
            }
            PhysicsDomain::Electricity => {
                "Circuits, capacitors, inductors, diodes, motors, and power electronics."
            }
            PhysicsDomain::Deformation => {
                "Impact denting and fracture into separate bodies, for parts marked Destructible."
            }
            PhysicsDomain::Fluids => {
                "Smoothed particle fluid dynamics, aerodynamics, and buoyancy."
            }
            PhysicsDomain::Materials => {
                "Stress and strain response of materials, and the fracture checks that read it."
            }
            PhysicsDomain::Particles => "Thermal and kinetic stepping of particle species.",
            PhysicsDomain::ParticleSimulation => {
                "The divergence free SPH solver used by ParticleSimulation objects."
            }
            PhysicsDomain::Nuclear => {
                "Nuclear systems that step every frame. The nuclear laws scripts call run either way."
            }
            PhysicsDomain::Visualizers => {
                "Overlays that draw simulation state such as stress and vector fields into the viewport."
            }
        }
    }

    /// Resolve a domain from a property key, accepting the spellings agents
    /// actually send: the snake_case key (`particle_simulation`), the panel's
    /// display name (`ParticleSimulation`), or either with stray case or
    /// underscores. Comparison ignores case and underscores.
    pub fn from_key(key: &str) -> Option<PhysicsDomain> {
        let squash = |s: &str| -> String {
            s.chars().filter(|c| *c != '_').flat_map(|c| c.to_lowercase()).collect()
        };
        let wanted = squash(key);
        PhysicsDomain::ALL.into_iter().find(|d| squash(d.key()) == wanted)
    }
}

/// Run condition for a domain: is it switched on right now?
///
/// Reads the service every frame rather than caching, so a toggle in the
/// properties panel takes effect on the next step with no restart.
pub fn domain_active(
    domain: PhysicsDomain,
) -> impl Fn(Option<Res<crate::services::physics::PhysicsService>>) -> bool + Clone {
    move |physics: Option<Res<crate::services::physics::PhysicsService>>| {
        // No service resource (a headless host that never added the physics
        // plugin) means nothing is switched off, so everything runs.
        physics.map_or(true, |p| p.domain_enabled(domain))
    }
}

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
                thermal_conduction::ThermalConductionPlugin,
                // STEM Stack — Phases A-D
                numerics::NumericsPlugin,
                electrical::ElectricalPlugin,
                chemistry::ChemistryPlugin,
                control::ControlPlugin,
            ));

        // STEM Stack — Phases E-L (separate add_plugins call to stay under
        // Bevy's 15-element plugin-tuple arity limit)
        app.add_plugins((
            structures::StructuresPlugin,
            thermocycles::ThermoCyclesPlugin,
            propulsion::PropulsionPlugin,
            plasma::PlasmaPlugin,
            particle_sim::ParticleSimulationPlugin,
        ));
        
        #[cfg(feature = "realism-symbolic")]
        app.add_plugins(symbolic::SymbolicPlugin);
        
        #[cfg(feature = "realism-scripting")]
        app.add_plugins(scripting::ScriptingPlugin);
        
        #[cfg(feature = "realism-gpu")]
        app.add_plugins(gpu::GpuSphPlugin);
        
        #[cfg(feature = "realism-quantum")]
        app.add_plugins(quantum::QuantumPlugin);

        // Gate every domain in ONE place. Each plugin above tags its systems
        // with its `PhysicsDomain`; the run condition lives here so adding a
        // domain cannot silently leave it ungated, and so a reader can see the
        // whole fidelity surface without opening eleven files.
        // Both schedules: most domains step in `Update`, but the DFSPH particle
        // solver steps in `FixedUpdate`. Configuring a set in a schedule that
        // has no systems in it is harmless, and covering both here means a
        // domain is gated wherever it happens to run.
        for domain in PhysicsDomain::ALL {
            app.configure_sets(Update, domain.run_if(domain_active(domain)));
            app.configure_sets(FixedUpdate, domain.run_if(domain_active(domain)));
        }

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
    // Which domains RUN is not configured here. It is decided by the fidelity
    // flags on `PhysicsService`, which the properties panel edits and which
    // gate each domain's systems through `PhysicsDomain` run conditions. This
    // struct used to carry its own `*_enabled` flags as well; two of them were
    // read nowhere, and the rest were an unexposed second switch for domains
    // that PhysicsService also controls.
    /// Simulation time scale (1.0 = real-time)
    pub time_scale: f32,
    /// Maximum particles for SPH simulation
    pub max_fluid_particles: u32,
    /// Spatial hash cell size for neighbor queries
    pub spatial_cell_size: f32,
}

impl Default for RealismConfig {
    fn default() -> Self {
        Self {
            time_scale: 1.0,
            max_fluid_particles: 100_000,
            spatial_cell_size: 1.0,
        }
    }
}
