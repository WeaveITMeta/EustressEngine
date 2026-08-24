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
    /// TOTAL electrode area of the cell (m²) — every layer of a stack summed,
    /// not one layer's footprint.
    ///
    /// The dendrite model is a current-DENSITY criterion, so this is the
    /// divisor that decides whether a cell reads as safe or as shorting. It
    /// used to be a hardcoded `0.03` in the tick, which is one ~300 cm² layer:
    /// for a 26-layer V-Cell (26 × 284 cm² = 0.7384 m²) that ran 24.6× high and
    /// pinned `dendrite_risk` at 1.0 at every rate, including rates 12× inside
    /// the real limit. A stacked cell MUST set this.
    pub electrode_area_m2: f32,
    /// Fractional cycles accumulated so far.
    ///
    /// `cycle_count` is a `u32`, and the per-tick increment is ~1e-6 cycles, so
    /// accumulating into the integer truncated every increment back to zero and
    /// the counter never moved — which meant capacity fade never ran and cycle
    /// life could not be simulated at all. The fraction accrues here and only
    /// crosses into `cycle_count` when it reaches a whole cycle.
    pub cycle_accum: f32,
    /// Lumped heat capacity of the cell (J/K) = mass × specific heat.
    ///
    /// The tick hardcoded `0.695 kg × 900 J/(kg·K)` = 625.5 J/K. Any cell that
    /// is not 0.695 kg heats at the wrong rate, and the error is linear in the
    /// mass ratio: a 3.55 kg cell integrated 5.1× too fast and reached 2292 °C
    /// on a discharge that physically settles near 40 °C. Temperature feeds the
    /// Nernst term, so a wrong thermal mass corrupts VOLTAGE too, not just the
    /// temperature readout. Leave at 0.0 to keep the legacy 625.5 J/K.
    pub thermal_mass_j_per_k: f32,
    /// Cell-to-ambient thermal resistance (K/W).
    ///
    /// Sets the steady-state rise: ΔT = Q × R. The tick hardcoded 2.0 K/W,
    /// which is a small-pouch number — on a 0.14 m² prismatic can dissipating
    /// 47 W it predicts a 94 K rise where free convection alone gives ~34 K.
    /// Leave at 0.0 to keep the legacy 2.0 K/W.
    pub thermal_resistance_k_per_w: f32,
    /// Standard cell potential of THIS cell's couple (V).
    ///
    /// The tick built its Nernst curve from `constants::na_s::STANDARD_POTENTIAL`,
    /// so every cell in every Space was a sodium-sulfur cell no matter what its
    /// materials said. A lithium-sulfur design measured on that curve reports a
    /// sodium-sulfur voltage, and since energy is volts times amp-hours, the
    /// mismatch lands directly on the headline number rather than announcing
    /// itself. Leave at 0.0 to keep the legacy Na-S 2.23 V.
    pub standard_potential_v: f32,
    /// Entropic coefficient dE/dT of this couple (V/K), for entropic heat.
    /// Leave at 0.0 to keep the legacy Na-S value.
    pub entropy_coefficient_v_per_k: f32,

    // ── Cycle life ────────────────────────────────────────────────────
    // Capacity fade was a blind power law in cycle count: no dependence on how
    // deeply the cell was cycled, how fast it was plated, how hot it ran, or
    // whether it carried a metal reservoir. Every one of those is a first-order
    // lever on a metal-anode cell, so the model could not be used to choose
    // between designs, which is the only thing a life model is for.
    //
    // The replacement tracks LITHIUM INVENTORY. Each cycle consumes a little
    // metal into interphase; retention holds at 1.0 while a reservoir covers
    // that loss and falls once it is spent. That is why an anode-LEAN cell
    // outlives an anode-free one, and the model now shows it instead of
    // asserting it.
    /// Critical plating current density (A/m2). 0.0 falls back to the
    /// Monroe-Newman estimate from the legacy Na/NASICON constants.
    pub j_crit_a_per_m2: f32,
    /// Coulombic efficiency at reference conditions (0-1). Fraction of plated
    /// metal recovered each cycle; the remainder is lost to interphase.
    /// 0.0 keeps a 0.995 default.
    pub coulombic_efficiency_ref: f32,
    /// Excess metal carried as a reservoir, as a fraction of nominal capacity.
    /// 0.0 is anode-free: no reservoir, so the first metal lost is capacity.
    pub li_reservoir_frac: f32,
    /// Stack pressure (MPa). Higher pressure suppresses the voids that form on
    /// stripping. 0.0 keeps a 2.0 MPa default.
    pub stack_pressure_mpa: f32,
    /// Lithium consumed forming interphase per cycle, on a SMOOTH deposit (nm).
    ///
    /// With this set, coulombic loss is derived rather than asserted:
    ///
    ///     1 - CE = R * delta * rho_Li * F / (M_Li * q)
    ///
    /// where R is the roughness of the deposit and q the areal capacity. That
    /// expression is worth more than the constant it replaces, because it says
    /// two things a fitted efficiency cannot. Loss is charged per unit AREA
    /// while charge is stored per area times thickness, so a THICKER electrode
    /// is intrinsically longer-lived. And an efficiency of 0.995 at
    /// 11.5 mAh/cm2 implies 279 nm of interphase per cycle on smooth metal,
    /// which is not physical: it is only physical at roughly a hundred times
    /// the geometric area, so that number was never a material property. It was
    /// a description of dendrites.
    ///
    /// 0.0 falls back to `coulombic_efficiency_ref`.
    pub sei_thickness_nm: f32,
    /// Roughness prefactor: how dendritic the deposit becomes as plating
    /// current rises and stack pressure falls. 0.0 keeps a value calibrated so
    /// that 2 MPa at the plating limit reproduces the measured 0.995.
    pub roughness_k: f32,

    // ── The two failure modes that are not lithium inventory ───────────
    // Until these existed the tick modelled exactly ONE way for a cell to die,
    // so every lifetime figure it produced was the answer to "how long until
    // the metal runs out" rather than "how long until the cell stops working".
    // A cell has more than one way to fail and the earliest one wins.
    /// Calendar fade coefficient: fraction of capacity lost per square root of
    /// equivalent hours at rest.
    ///
    /// Cycle fade is consumed by moving charge; calendar fade is consumed by
    /// sitting still, because the interphase keeps growing on a cell that is
    /// merely parked. A car does a few hundred cycles a year against three and
    /// a half thousand days of parking, so for any ten-year claim this term is
    /// likely to dominate the one above it. 0.0 keeps a default calibrated to
    /// roughly 2 % per year at 25 C and half charge.
    pub calendar_k: f32,
    /// Accumulated Arrhenius- and state-of-charge-weighted hours.
    pub calendar_hours_equiv: f64,
    /// Cathode fatigue coefficient: structural damage per cycle at unit depth.
    ///
    /// Li2S to S is roughly an 80 % volume change, every cycle. The composite
    /// cracks, particles lose contact with the carbon network, and that
    /// capacity is gone whether or not any lithium was consumed. For a
    /// lithium-sulfur cell this is a strong candidate for the mechanism that
    /// actually sets life, and no reservoir protects against it: a metal
    /// reservoir replaces lost lithium and does nothing for a cracked cathode.
    pub crack_k: f32,
    /// Accumulated cathode damage, 0 to 1.
    pub crack_damage: f64,
    /// Running total of metal lost to interphase, as a fraction of nominal.
    ///
    /// f64, and it has to be. This accumulates one small increment per SUBSTEP,
    /// and a long run substeps thousands of times per frame: at 3000x time
    /// compression the per-substep loss is around 1e-11 of nominal. Held in f32,
    /// the epsilon at a quarter is 3e-8, so once the total passed roughly 0.25
    /// every subsequent addition rounded to nothing and the cell simply stopped
    /// ageing. Capacity retention pinned at exactly 0.95 and stayed there for
    /// hundreds of cycles, which reads as a cell that has stabilised rather than
    /// as arithmetic that has run out of mantissa. Worse, the threshold depends
    /// on the timestep, so the same design ages differently at a different clock.
    pub li_inventory_lost: f64,
    /// State of charge at the last direction reversal, and the deepest
    /// excursion reached since it.
    ///
    /// Fade must depend on how deep the CYCLE goes, not on how much charge
    /// moved in one substep. A per-substep depth is a function of the timestep,
    /// so a model built on it changes its answer when the clock changes, which
    /// makes it useless for comparing designs. These two track the excursion so
    /// the depth term means what it says.
    pub soc_turn: f32,
    pub excursion_depth: f32,
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
            // One ~300 cm² electrode. Correct for a single-layer coin/pouch
            // cell; a stack must override it with layers × per-layer area.
            electrode_area_m2: 0.03,
            cycle_accum: 0.0,
            thermal_mass_j_per_k: 0.0,
            thermal_resistance_k_per_w: 0.0,
            standard_potential_v: 0.0,
            entropy_coefficient_v_per_k: 0.0,
            j_crit_a_per_m2: 0.0,
            coulombic_efficiency_ref: 0.0,
            li_reservoir_frac: 0.0,
            stack_pressure_mpa: 0.0,
            li_inventory_lost: 0.0,
            soc_turn: 1.0,
            excursion_depth: 0.0,
            sei_thickness_nm: 0.0,
            roughness_k: 0.0,
            calendar_k: 0.0,
            calendar_hours_equiv: 0.0,
            crack_k: 0.0,
            crack_damage: 0.0,
        }
    }
}

/// Default calendar fade coefficient: about 2 % of capacity per year at 25 °C
/// and half charge, i.e. `0.02 / sqrt(8760 h)`.
pub const DEFAULT_CALENDAR_K: f32 = 2.14e-4;

/// Default cathode fatigue coefficient: about 20 % structural loss over 1000
/// full-depth cycles, where an unreinforced sulfur composite sits.
pub const DEFAULT_CRACK_K: f32 = 1.75e-4;

impl ElectrochemicalState {
    /// Capacity fraction lost to lithium inventory, net of the reservoir.
    ///
    /// One of three independent fade channels, and the only one the metal
    /// reservoir buffers — which is exactly why the other two are tracked
    /// apart from it rather than folded into a single retention number. A
    /// retention figure with no attribution cannot tell you which lever to
    /// pull next.
    pub fn lithium_fade(&self) -> f64 {
        (self.li_inventory_lost - self.li_reservoir_frac as f64).max(0.0)
    }

    /// Capacity fraction lost to calendar ageing, from accumulated weighted hours.
    pub fn calendar_fade(&self) -> f64 {
        let k = if self.calendar_k > 0.0 {
            self.calendar_k
        } else {
            DEFAULT_CALENDAR_K
        };
        k as f64 * self.calendar_hours_equiv.sqrt()
    }

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
            // 26 layers × 284 cm² (PATENT.md 6.2 / 12.1 rev 1.2).
            electrode_area_m2: 0.7384,
            cycle_accum: 0.0,
            thermal_mass_j_per_k: 0.0,
            thermal_resistance_k_per_w: 0.0,
            standard_potential_v: 0.0,
            entropy_coefficient_v_per_k: 0.0,
            j_crit_a_per_m2: 0.0,
            coulombic_efficiency_ref: 0.0,
            li_reservoir_frac: 0.0,
            stack_pressure_mpa: 0.0,
            li_inventory_lost: 0.0,
            soc_turn: 1.0,
            excursion_depth: 0.0,
            sei_thickness_nm: 0.0,
            roughness_k: 0.0,
            calendar_k: 0.0,
            calendar_hours_equiv: 0.0,
            crack_k: 0.0,
            crack_damage: 0.0,
        }
    }
}
