//! # Particle Components
//!
//! ECS components for physical particles with thermodynamic and kinetic properties.
//!
//! ## Table of Contents
//!
//! 1. **Particle** - Base particle component
//! 2. **ThermodynamicState** - Temperature, pressure, entropy
//! 3. **KineticState** - Velocity, momentum, angular motion
//! 4. **Bundles** - Common particle configurations

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use crate::realism::constants;

// ============================================================================
// Particle Types
// ============================================================================

/// Type of particle for simulation behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect, Serialize, Deserialize)]
pub enum ParticleType {
    /// Gas particle (ideal gas behavior)
    #[default]
    Gas,
    /// Liquid particle (SPH fluid)
    Liquid,
    /// Solid particle (rigid body)
    Solid,
    /// Plasma particle (charged gas)
    Plasma,
    /// Dust/debris particle (affected by air resistance)
    Dust,
    /// Smoke particle (buoyant, dissipates)
    Smoke,
    /// Fire particle (emits heat, rises)
    Fire,
}

// ============================================================================
// Core Particle Component
// ============================================================================

/// Base particle component with physical properties
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Particle {
    /// Mass in kilograms
    pub mass: f32,
    /// Radius in meters (for collision/visualization)
    pub radius: f32,
    /// Particle type
    pub particle_type: ParticleType,
    /// Lifetime remaining (seconds, None = infinite)
    pub lifetime: Option<f32>,
    /// Is particle active in simulation
    pub active: bool,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            mass: 1.0,
            radius: 0.1,
            particle_type: ParticleType::Gas,
            lifetime: None,
            active: true,
        }
    }
}

impl Particle {
    /// Create a new particle with given mass and radius
    pub fn new(mass: f32, radius: f32) -> Self {
        Self {
            mass,
            radius,
            ..default()
        }
    }
    
    /// Create a gas particle
    pub fn gas(mass: f32, radius: f32) -> Self {
        Self {
            mass,
            radius,
            particle_type: ParticleType::Gas,
            ..default()
        }
    }
    
    /// Create a liquid particle (for SPH)
    pub fn liquid(mass: f32, radius: f32) -> Self {
        Self {
            mass,
            radius,
            particle_type: ParticleType::Liquid,
            ..default()
        }
    }
    
    /// Create a solid particle
    pub fn solid(mass: f32, radius: f32) -> Self {
        Self {
            mass,
            radius,
            particle_type: ParticleType::Solid,
            ..default()
        }
    }
    
    /// Set lifetime
    pub fn with_lifetime(mut self, seconds: f32) -> Self {
        self.lifetime = Some(seconds);
        self
    }
}

// ============================================================================
// Thermodynamic State
// ============================================================================

/// Thermodynamic state of a particle or system
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ThermodynamicState {
    /// Temperature in Kelvin
    pub temperature: f32,
    /// Pressure in Pascals
    pub pressure: f32,
    /// Volume in cubic meters
    pub volume: f32,
    /// Internal energy in Joules
    pub internal_energy: f32,
    /// Entropy in J/K
    pub entropy: f32,
    /// Enthalpy in Joules
    pub enthalpy: f32,
    /// Amount of substance in moles
    pub moles: f32,
}

impl Default for ThermodynamicState {
    fn default() -> Self {
        Self::standard_conditions(1.0)
    }
}

impl ThermodynamicState {
    /// Create state at standard conditions (25°C, 1 atm)
    pub fn standard_conditions(moles: f32) -> Self {
        let temperature = constants::STANDARD_TEMPERATURE;
        let pressure = constants::STANDARD_PRESSURE;
        let volume = (moles * constants::R_F32 * temperature) / pressure;
        let internal_energy = 1.5 * moles * constants::R_F32 * temperature;
        let enthalpy = internal_energy + pressure * volume;
        
        Self {
            temperature,
            pressure,
            volume,
            internal_energy,
            entropy: 0.0, // Reference point
            enthalpy,
            moles,
        }
    }
    
    /// Create state for ideal gas at given conditions
    pub fn ideal_gas(moles: f32, temperature: f32, volume: f32) -> Self {
        let pressure = (moles * constants::R_F32 * temperature) / volume;
        let internal_energy = 1.5 * moles * constants::R_F32 * temperature;
        let enthalpy = internal_energy + pressure * volume;
        
        Self {
            temperature,
            pressure,
            volume,
            internal_energy,
            entropy: moles * constants::R_F32 * (temperature / 298.15).ln(),
            enthalpy,
            moles,
        }
    }
    
    /// Create state at given temperature and pressure
    pub fn at_conditions(moles: f32, temperature: f32, pressure: f32) -> Self {
        let volume = (moles * constants::R_F32 * temperature) / pressure;
        Self::ideal_gas(moles, temperature, volume)
    }
    
    /// Update pressure from ideal gas law
    pub fn update_pressure(&mut self) {
        if self.volume > 0.0 {
            self.pressure = (self.moles * constants::R_F32 * self.temperature) / self.volume;
        }
    }
    
    /// Update internal energy for monatomic ideal gas
    pub fn update_internal_energy(&mut self) {
        self.internal_energy = 1.5 * self.moles * constants::R_F32 * self.temperature;
    }
    
    /// Update enthalpy
    pub fn update_enthalpy(&mut self) {
        self.enthalpy = self.internal_energy + self.pressure * self.volume;
    }
    
    /// Add heat at constant volume (isochoric)
    pub fn add_heat_isochoric(&mut self, heat: f32) {
        let cv = 1.5 * self.moles * constants::R_F32;
        if cv > 0.0 {
            self.temperature += heat / cv;
            self.update_internal_energy();
            self.update_pressure();
            self.update_enthalpy();
            if self.temperature > 0.0 {
                self.entropy += heat / self.temperature;
            }
        }
    }
    
    /// Add heat at constant pressure (isobaric)
    pub fn add_heat_isobaric(&mut self, heat: f32) {
        let cp = 2.5 * self.moles * constants::R_F32;
        if cp > 0.0 {
            self.temperature += heat / cp;
            self.volume = (self.moles * constants::R_F32 * self.temperature) / self.pressure;
            self.update_internal_energy();
            self.update_enthalpy();
            if self.temperature > 0.0 {
                self.entropy += heat / self.temperature;
            }
        }
    }
    
    /// Get density (kg/m³) assuming molar mass of air (~29 g/mol)
    pub fn density(&self, molar_mass: f32) -> f32 {
        if self.volume > 0.0 {
            (self.moles * molar_mass) / self.volume
        } else {
            0.0
        }
    }
}

// ============================================================================
// Kinetic State
// ============================================================================

/// Kinetic state of a particle (velocity, momentum, angular motion)
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct KineticState {
    /// Linear velocity in m/s
    pub velocity: Vec3,
    /// Linear momentum in kg·m/s (cached, updated from mass*velocity)
    pub momentum: Vec3,
    /// Angular velocity in rad/s
    pub angular_velocity: Vec3,
    /// Angular momentum in kg·m²/s
    pub angular_momentum: Vec3,
    /// Accumulated force this frame (N)
    pub accumulated_force: Vec3,
    /// Accumulated torque this frame (N·m)
    pub accumulated_torque: Vec3,
}

impl KineticState {
    /// Create with initial velocity
    pub fn with_velocity(velocity: Vec3) -> Self {
        Self {
            velocity,
            ..default()
        }
    }
    
    /// Create with initial velocity and angular velocity
    pub fn with_motion(velocity: Vec3, angular_velocity: Vec3) -> Self {
        Self {
            velocity,
            angular_velocity,
            ..default()
        }
    }
    
    /// Update momentum from mass and velocity
    pub fn update_momentum(&mut self, mass: f32) {
        self.momentum = mass * self.velocity;
    }
    
    /// Update angular momentum from moment of inertia
    pub fn update_angular_momentum(&mut self, moment_of_inertia: f32) {
        self.angular_momentum = moment_of_inertia * self.angular_velocity;
    }
    
    /// Apply force (accumulates for this frame)
    pub fn apply_force(&mut self, force: Vec3) {
        self.accumulated_force += force;
    }
    
    /// Apply torque (accumulates for this frame)
    pub fn apply_torque(&mut self, torque: Vec3) {
        self.accumulated_torque += torque;
    }
    
    /// Apply impulse (immediate velocity change)
    pub fn apply_impulse(&mut self, impulse: Vec3, mass: f32) {
        if mass > 0.0 {
            self.velocity += impulse / mass;
        }
    }
    
    /// Clear accumulated forces (call after integration)
    pub fn clear_forces(&mut self) {
        self.accumulated_force = Vec3::ZERO;
        self.accumulated_torque = Vec3::ZERO;
    }
    
    /// Get kinetic energy
    pub fn kinetic_energy(&self, mass: f32) -> f32 {
        0.5 * mass * self.velocity.length_squared()
    }
    
    /// Get rotational kinetic energy
    pub fn rotational_kinetic_energy(&self, moment_of_inertia: f32) -> f32 {
        0.5 * moment_of_inertia * self.angular_velocity.length_squared()
    }
    
    /// Get speed (magnitude of velocity)
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }
}

// ============================================================================
// Additional Components
// ============================================================================

/// Fluid-specific properties for SPH particles
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct FluidProperties {
    /// Rest density in kg/m³
    pub rest_density: f32,
    /// Current density in kg/m³
    pub density: f32,
    /// Dynamic viscosity in Pa·s
    pub viscosity: f32,
    /// Surface tension coefficient in N/m
    pub surface_tension: f32,
    /// Smoothing length for SPH kernel
    pub smoothing_length: f32,
    /// Phase (liquid, gas, etc.)
    pub phase: FluidPhase,
}

impl Default for FluidProperties {
    fn default() -> Self {
        Self {
            rest_density: constants::WATER_DENSITY,
            density: constants::WATER_DENSITY,
            viscosity: constants::WATER_VISCOSITY,
            surface_tension: constants::WATER_SURFACE_TENSION,
            smoothing_length: 0.1,
            phase: FluidPhase::Liquid,
        }
    }
}

/// Fluid phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect, Serialize, Deserialize)]
pub enum FluidPhase {
    Solid,
    #[default]
    Liquid,
    Gas,
    Supercritical,
}

/// Heat transfer properties
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct HeatTransferProperties {
    /// Thermal conductivity in W/(m·K)
    pub thermal_conductivity: f32,
    /// Specific heat capacity in J/(kg·K)
    pub specific_heat: f32,
    /// Emissivity (0-1)
    pub emissivity: f32,
    /// Convective heat transfer coefficient in W/(m²·K)
    pub convection_coefficient: f32,
}

impl Default for HeatTransferProperties {
    fn default() -> Self {
        Self {
            thermal_conductivity: constants::WATER_THERMAL_CONDUCTIVITY,
            specific_heat: constants::WATER_SPECIFIC_HEAT,
            emissivity: 0.95,
            convection_coefficient: 10.0,
        }
    }
}

// ============================================================================
// Bundles
// ============================================================================

/// Complete thermodynamic particle bundle
#[derive(Bundle, Clone)]
pub struct ThermodynamicParticleBundle {
    pub particle: Particle,
    pub thermo: ThermodynamicState,
    pub kinetic: KineticState,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

impl Default for ThermodynamicParticleBundle {
    fn default() -> Self {
        Self {
            particle: Particle::default(),
            thermo: ThermodynamicState::default(),
            kinetic: KineticState::default(),
            transform: Transform::default(),
            global_transform: GlobalTransform::default(),
        }
    }
}

impl ThermodynamicParticleBundle {
    /// Create gas particle at position
    pub fn gas(position: Vec3, mass: f32, temperature: f32) -> Self {
        Self {
            particle: Particle::gas(mass, 0.05),
            thermo: ThermodynamicState::at_conditions(mass / 0.029, temperature, constants::STANDARD_PRESSURE),
            kinetic: KineticState::default(),
            transform: Transform::from_translation(position),
            global_transform: GlobalTransform::default(),
        }
    }
}

/// Fluid particle bundle for SPH simulation
#[derive(Bundle, Clone)]
pub struct FluidParticleBundle {
    pub particle: Particle,
    pub fluid: FluidProperties,
    pub kinetic: KineticState,
    pub thermo: ThermodynamicState,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

impl Default for FluidParticleBundle {
    fn default() -> Self {
        Self {
            particle: Particle::liquid(0.001, 0.02),
            fluid: FluidProperties::default(),
            kinetic: KineticState::default(),
            thermo: ThermodynamicState::standard_conditions(0.001 / 0.018),
            transform: Transform::default(),
            global_transform: GlobalTransform::default(),
        }
    }
}

impl FluidParticleBundle {
    /// Create water particle at position
    pub fn water(position: Vec3) -> Self {
        Self {
            transform: Transform::from_translation(position),
            ..default()
        }
    }
    
    /// Create water particle with velocity
    pub fn water_with_velocity(position: Vec3, velocity: Vec3) -> Self {
        Self {
            kinetic: KineticState::with_velocity(velocity),
            transform: Transform::from_translation(position),
            ..default()
        }
    }
}

// ============================================================================
// Electrochemical State Component
// ============================================================================

/// Electrochemical state for battery/fuel-cell simulation entities.
/// Holds runtime state that evolves during simulation. Static electrode
/// properties live in `MaterialProperties::custom_properties`.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ElectrochemicalState {
    /// Open-circuit voltage at current SOC (V)
    pub voltage: f32,
    /// Terminal voltage under load (V)
    pub terminal_voltage: f32,
    /// Nominal capacity (Ah)
    pub capacity_ah: f32,
    /// State of charge (0.0–1.0)
    pub soc: f32,
    /// Operating current (A, positive = discharge)
    pub current: f32,
    /// Internal resistance (Ω)
    pub internal_resistance: f32,
    /// Ionic conductivity of electrolyte (S/m)
    pub ionic_conductivity: f32,
    /// Cycle count
    pub cycle_count: u32,
    /// C-rate (h⁻¹)
    pub c_rate: f32,
    /// Capacity retention fraction (0.0–1.0)
    pub capacity_retention: f32,
    /// Total heat generation (W)
    pub heat_generation: f32,
    /// Dendrite risk factor (0.0 = safe, ≥1.0 = risk)
    pub dendrite_risk: f32,
}

impl Default for ElectrochemicalState {
    fn default() -> Self {
        Self {
            voltage: 2.23,
            terminal_voltage: 2.23,
            capacity_ah: 202.5,
            soc: 1.0,
            current: 0.0,
            internal_resistance: 0.001,
            ionic_conductivity: 0.01,
            cycle_count: 0,
            c_rate: 0.0,
            capacity_retention: 1.0,
            heat_generation: 0.0,
            dendrite_risk: 0.0,
        }
    }
}

impl ElectrochemicalState {
    /// V-Cell Na-S defaults (202.5 Ah, 2.23 V standard)
    pub fn vcell_na_s() -> Self {
        Self {
            voltage: crate::realism::constants::na_s::STANDARD_POTENTIAL,
            terminal_voltage: crate::realism::constants::na_s::STANDARD_POTENTIAL,
            capacity_ah: 202.5,
            soc: 1.0,
            current: 0.0,
            internal_resistance: 0.001,
            ionic_conductivity: crate::realism::constants::sc_nasicon::IONIC_CONDUCTIVITY_TARGET * 100.0,
            cycle_count: 0,
            c_rate: 0.0,
            capacity_retention: 1.0,
            heat_generation: 0.0,
            dendrite_risk: 0.0,
        }
    }
}
