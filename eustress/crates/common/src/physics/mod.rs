//! # Physics Module
//!
//! Advanced physics systems for Eustress Engine.
//!
//! ## Modules
//!
//! - `gravity`: N-body gravitational physics with big G and little g calculations
//! - `dynamic_gravity`: Explorer Workspace-integrated dynamic gravity with tiered optimization
//! - `exotic_propulsion`: Element 115 reactor, gravity amplifiers, warp lasing, WASD vector thrust
//!
//! ## Features
//!
//! - Planetary-scale object support (Earth radius: 6.371 million meters)
//! - Inverse square law gravity calculations
//! - Force threshold filtering (>0.001 N)
//! - Spatial partitioning for O(n log n) performance
//! - Hybrid Vec3/DVec3 precision for solar system scale
//! - Dynamic mass/radius editing at runtime
//! - Tiered performance optimization (Heavy/Medium/Light objects)
//! - Force metatable for real-time tracking
//! - Element 115 antimatter reactor with TEG (99% efficient E=mc² conversion)
//! - Gravity A-wave amplifiers with unit circle WASD steering
//! - Inverse square projection (virtual mass at focal point)
//! - Wave cascading interference (3-amplifier constructive focusing)
//! - Phase-locked lasing for FTL warp tunnels
//! - Coherence tuning mini-game (pilot vs isotope drift)
//! - Mass shielding (gravitational flux redirection)
//! - Zero-G cabin (inertial dampening within warp bubble)
//! - Thermal dynamics with core/shell temperature and frequency drift feedback
//! - Spacetime stress accumulation and gravity snap events
//! - Planetary float equilibrium via g(h) = GM/(R+h)² hover lock
//! - Kinematic buffer (Ghost Box) for smooth PID-style flight
//! - Regenerative braking (momentum → reactor recharge)
//! - Toroidal core: MHD containment, Lense-Thirring frame-drag, flux quantization
//! - Greek Rune parameter architecture (18 scriptable differential equation variables)
//! - Isotope state tracking with fuel depletion and crystal degradation

pub mod gravity;
pub mod dynamic_gravity;
pub mod exotic_propulsion;

pub use gravity::{
    GravityPlugin,
    Mass,
    PhysicalRadius,
    GravitationalForce,
    GravitySource,
    GravityAffected,
    GravityConfig,
    GravityStats,
    gravitational_force,
    surface_gravity,
    orbital_velocity,
    escape_velocity,
    gravitational_potential,
    G,
    FORCE_THRESHOLD,
};

pub use dynamic_gravity::{
    DynamicGravityPlugin,
    DynamicMass,
    DynamicRadius,
    DynamicGravityForce,
    DynamicGravityConfig,
    DynamicGravityStats,
    ForceMetatable,
    MassTier,
    HEAVY_MASS_THRESHOLD,
    MEDIUM_MASS_THRESHOLD,
};

pub use exotic_propulsion::{
    ExoticPropulsionPlugin,
    Reactor115,
    GravityGradient,
    GravityAmplifier,
    GravityAWaveFocus,
    WaveCascader,
    GravityLaser,
    CoherenceTuning,
    MassShielding,
    ReactionlessDrive,
    SurfaceTemperature,
    ThermalDynamics,
    SpacetimeStress,
    PlanetaryFloat,
    KinematicBuffer,
    RegenerativeBraking,
    ToroidalCore,
    RuneParameters,
    IsotopeState,
    C,
    KAPPA,
    E_PLANCK,
};
