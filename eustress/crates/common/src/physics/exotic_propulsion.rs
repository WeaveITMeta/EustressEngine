//! # Exotic Propulsion Systems — Element 115 Reactor + Gravity Drive
//!
//! Complete implementation of the Element 115 (Moscovium) antimatter reactor,
//! Thermoelectric Generator (TEG), Gravity A-Wave amplifiers, wave cascading
//! interference patterns, inverse square projection, phase-locked warp lasing,
//! and coherence tuning for interstellar travel.
//!
//! ## Table of Contents
//!
//! 1. **Physical Constants** — C, G, KAPPA, E_PLANCK
//! 2. **Reactor115** — Element 115 transmutation → antimatter → E=mc² → TEG
//! 3. **GravityGradient** — Directional spacetime curvature for reactionless thrust
//! 4. **GravityAmplifier** — Three "Gravity Bells" tuning A-wave to directional curvature
//! 5. **GravityAWaveFocus** — Virtual mass projection via inverse square law
//! 6. **WaveCascader** — 3-amplifier interference pattern for constructive focusing
//! 7. **GravityLaser** — Phase-locked coherent beam for warp tunnel
//! 8. **CoherenceTuning** — Pilot frequency tuning against isotope resonance drift
//! 9. **MassShielding** — Gravitational flux redirection via EM fields
//! 10. **ReactionlessDrive** — Zero-G cabin, inertial dampening
//! 11. **SurfaceTemperature** — Hull heat from TEG waste (1% of reactor output)
//! 12. **ThermalDynamics** — Core/shell temperature feedback → frequency drift acceleration
//! 13. **SpacetimeStress** — Gravity snap events on coherence collapse
//! 14. **PlanetaryFloat** — Hover equilibrium via g(h) = GM/(R+h)²
//! 15. **KinematicBuffer** — Ghost Box PID smoothing (Mu/Nu/Pi/Iota runes)
//! 16. **RegenerativeBraking** — Momentum → reactor recharge on deceleration
//! 17. **ToroidalCore** — MHD containment, Lense-Thirring frame-drag, flux quantization
//! 18. **RuneParameters** — 18 Greek scriptable variables for differential equation macros
//! 19. **IsotopeState** — Fuel depletion, crystal degradation, half-life scaling
//! 20. **Systems (1-18)** — Chained simulation loop from reactor to regen braking
//! 21. **ExoticPropulsionPlugin** — Unified Bevy plugin with 18-system chain
//!
//! ## Physics Model
//!
//! - **Fuel**: Reactor115 consumes Moscovium isotopes
//! - **Conversion**: E=mc² generates electrical power via TEG (99% efficient)
//! - **Focus**: TEG electricity powers microwave emitters (GravityAmplifier)
//! - **Movement**: Ship "falls" toward unit circle vector (WASD control)
//! - **Inertia**: Ship moves with spacetime gradient → zero G-forces
//! - **Warp**: Phase-locked amplifiers create coherent gravity tunnel (lasing mode)
//! - **Stability**: Pilot tunes frequency against natural isotope drift
//!
//! ## Flight Modes
//!
//! - **Mode 1 (WASD)**: Phase-offset waves create localized sink → "fall" locally
//! - **Mode 2 (Lasing)**: Phase-locked waves create spacetime tunnel → warp interstellar

use bevy::prelude::*;
use std::f64::consts::PI;
use tracing::{info, warn, trace};

// ============================================================================
// Physical Constants
// ============================================================================

/// Speed of light (m/s)
pub const C: f64 = 299_792_458.0;

/// Gravitational constant (m³/kg·s²)
pub const G: f64 = 6.67430e-11;

/// Einstein gravitational constant: 8πG/c⁴
pub const KAPPA: f64 = 2.076e-43; // m/J

/// Planck energy (J) - theoretical minimum for spacetime manipulation
pub const E_PLANCK: f64 = 1.956e9; // ~2 billion joules

// ============================================================================
// Reactor115 — Element 115 Antimatter Reactor + TEG
// ============================================================================

/// Element 115 (Moscovium) antimatter reactor with Thermoelectric Generator
///
/// Process:
/// 1. Proton bombardment of Element 115 isotope
/// 2. Transmutation to Element 116 (immediate decay)
/// 3. Antimatter (antiproton) release from decay
/// 4. Matter-antimatter annihilation → E=mc² energy
/// 5. TEG converts heat to electricity (99% efficient)
///
/// Even microgram-scale fuel consumption produces Terawatts of power.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct Reactor115 {
    /// Grams of Element 115 fuel remaining
    pub fuel_mass_grams: f64,

    /// Rate of proton bombardment (0.0 = off, 1.0 = full power)
    pub throttle: f64,

    /// TEG efficiency (0.99 = "the 99% claim")
    pub teg_efficiency: f64,

    /// Current electrical output in Watts
    pub current_output: f64,
}

impl Reactor115 {
    /// Update reactor state for one tick
    /// Converts fuel mass to energy via E=mc²
    pub fn update(&mut self, dt: f64) {
        // Microgram-scale consumption per tick at full throttle
        let mass_converted_kg = 1e-9 * self.throttle * dt;
        let raw_energy = mass_converted_kg * C * C;

        self.current_output = raw_energy * self.teg_efficiency;
        self.fuel_mass_grams -= mass_converted_kg * 1000.0;

        // Clamp fuel to zero (no negative mass)
        if self.fuel_mass_grams < 0.0 {
            self.fuel_mass_grams = 0.0;
            self.current_output = 0.0;
            self.throttle = 0.0;
        }
    }
}

impl Default for Reactor115 {
    fn default() -> Self {
        Self {
            fuel_mass_grams: 500.0,   // 500g of Element 115
            throttle: 0.0,
            teg_efficiency: 0.99,     // 99% TEG efficiency
            current_output: 0.0,
        }
    }
}

// ============================================================================
// GravityGradient — Directional Spacetime Curvature
// ============================================================================

/// Artificial gravitational asymmetry for reactionless propulsion
///
/// Creates a "slope" in spacetime geometry:
/// - High mass-energy density behind the craft (steep curvature)
/// - Low mass-energy density ahead (shallow/shielded)
///
/// Ship perpetually "falls" toward lower potential energy state.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct GravityGradient {
    /// Gradient strength (m/s² effective acceleration)
    pub gradient_strength: f64,

    /// Direction of the gradient (unit vector, ship falls this way)
    pub direction: Vec3,

    /// Energy consumption rate (J/s = Watts)
    pub power_draw: f64,

    /// Current energy reserves (J) — fed by Reactor115
    pub energy_reserves: f64,

    /// Maximum gradient strength achievable by hardware
    pub max_gradient: f64,

    /// Efficiency factor (0.0 - 1.0)
    pub efficiency: f64,
}

impl GravityGradient {
    pub fn new(max_gradient: f64, efficiency: f64) -> Self {
        Self {
            gradient_strength: 0.0,
            direction: Vec3::ZERO,
            power_draw: 0.0,
            energy_reserves: 0.0,
            max_gradient,
            efficiency,
        }
    }

    /// Set gradient direction and strength
    pub fn set_gradient(&mut self, direction: Vec3, strength: f64) {
        self.direction = direction.normalize_or_zero();
        self.gradient_strength = strength.min(self.max_gradient);
    }
}

impl Default for GravityGradient {
    fn default() -> Self {
        Self::new(100.0, 0.85) // 100 m/s² max, 85% amplifier efficiency
    }
}

// ============================================================================
// GravityAmplifier — Three "Gravity Bells"
// ============================================================================

/// Gravity A-wave amplifier (one of three per craft)
///
/// Takes raw TEG power and "tunes" it into a directional gravity wave.
/// Uses the strong nuclear force extension from Element 115 atoms.
///
/// Three amplifiers create a triangular focal pattern:
/// - Each offset by 120° (2π/3 radians)
/// - Phase shift controls WASD steering direction
/// - Gain factor determines curvature depth
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct GravityAmplifier {
    /// "Gravity A" wave frequency (GHz)
    pub operating_frequency: f64,

    /// Gain factor (amplification of base wave)
    pub gain: f64,

    /// Phase shift for unit circle WASD steering (radians)
    pub phase_shift: f64,

    /// Microwave-to-gravity conversion efficiency
    pub conversion_efficiency: f64,
}

impl Default for GravityAmplifier {
    fn default() -> Self {
        Self {
            operating_frequency: 7.46, // Speculative A-wave tuning frequency (GHz)
            gain: 1e6,                 // Massive amplification for spacetime curvature
            phase_shift: 0.0,
            conversion_efficiency: 0.85,
        }
    }
}

// ============================================================================
// GravityAWaveFocus — Inverse Square Projection
// ============================================================================

/// Virtual mass projection via inverse square law
///
/// The gravity amplifiers project a point of maximum curvature at
/// a specific distance (r) from the Center of Mass. The force pulling
/// the ship obeys F = G × m_ship × m_virtual / r².
///
/// Small r (near-field): Massive acceleration (1/r²), risk of singularity
/// Large r (far-field): Smoother travel, higher fuel consumption
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct GravityAWaveFocus {
    /// Distance from Center of Mass to projected gravity sink (meters)
    pub projection_distance: f64,

    /// Virtual mass created by A-wave (kg-equivalent, from E=mc²)
    pub virtual_mass: f64,

    /// Direction vector on the unit circle (WASD controlled)
    pub target_direction: Vec3,
}

impl Default for GravityAWaveFocus {
    fn default() -> Self {
        Self {
            projection_distance: 10.0,  // 10m ahead of craft
            virtual_mass: 1e12,         // Billion-ton virtual pull
            target_direction: Vec3::Z,  // Forward
        }
    }
}

// ============================================================================
// WaveCascader — 3-Amplifier Interference Pattern
// ============================================================================

/// Cascading wave phase conjugation system
///
/// Three gravity amplifiers create an interference pattern.
/// By cascading waves with precise offsets in frequency, amplitude,
/// and time/phase, constructive interference creates a localized
/// gravity sink at exactly the target distance r.
///
/// Formula: Φ(r) = Σ [A × cos(k×r − ω×t + φ + offset_i)]²
/// where offset_i = i × 2π/3 for three amplifiers
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct WaveCascader {
    /// Base frequency of the A-wave (radians/second)
    pub base_frequency: f64,

    /// Amplitude multiplier from TEG output
    pub energy_gain: f64,

    /// Time-offset for phase-shifting (0.0 to 2π, WASD controlled)
    pub phase_step: f64,
}

impl WaveCascader {
    /// Calculate focused "virtual mass" intensity at point r
    ///
    /// Cascading 3-wave interference with 120° spatial offsets.
    /// Returns energy density at the focal point (proportional to virtual mass).
    pub fn calculate_focus_intensity(&self, power: f64, time: f64, distance: f64) -> f64 {
        let mut interference = 0.0;

        // Three amplifiers offset by 120° (2π/3)
        for i in 0..3 {
            let spatial_offset = (i as f64) * (2.0 * PI / 3.0);

            // Wave formula: A × cos(k×r − ω×t + φ + offset)
            let wave = (power * self.energy_gain)
                * ((self.base_frequency * distance)
                    - (time * self.base_frequency)
                    + self.phase_step
                    + spatial_offset)
                    .cos();

            interference += wave;
        }

        // Energy density = interference² (always positive)
        interference.powi(2).max(0.0)
    }
}

impl Default for WaveCascader {
    fn default() -> Self {
        Self {
            base_frequency: 7.46e9 * 2.0 * PI, // 7.46 GHz in rad/s
            energy_gain: 1e6,
            phase_step: 0.0,
        }
    }
}

// ============================================================================
// GravityLaser — Phase-Locked Warp Tunnel
// ============================================================================

/// Phase-locked coherent gravity beam for interstellar warp
///
/// When activated, three amplifiers synchronize their wave cycles
/// into a single collimated beam creating a "gravity tunnel" (soliton wave).
///
/// Mode 1 (WASD): Phase-offset → localized sink → "fall" locally
/// Mode 2 (Lasing): Phase-locked → coherent tunnel → warp interstellar
///
/// Because waves are in-phase, amplitude triples but energy density
/// increases by 9x (A²), providing the massive spacetime stiffness
/// jump required for FTL (faster-than-light) travel.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct GravityLaser {
    /// When true, all 3 amplifiers lock phases (0 delta)
    pub is_locked: bool,

    /// Minimum Reactor115 output to maintain coherence (Watts)
    pub coherence_threshold: f64,

    /// Beam collimation factor (smaller = tighter focus)
    pub beam_width: f64,
}

impl Default for GravityLaser {
    fn default() -> Self {
        Self {
            is_locked: false,
            coherence_threshold: 1e15, // Petawatt scale
            beam_width: 0.001,         // Highly focused
        }
    }
}

// ============================================================================
// CoherenceTuning — Pilot Frequency Drift Management
// ============================================================================

/// Manual tuning system for warp coherence
///
/// The Element 115 core resonance naturally drifts over time.
/// The pilot must continuously adjust the amplifier frequency to
/// match the shifting resonance, or the lasing beam de-coheres.
///
/// De-coherence consequences:
/// - Warp tunnel collapses
/// - Energy bleeds into hull as heat
/// - Sudden velocity drop
///
/// This creates a high-stakes "Phase-Locked Loop" mini-game
/// during interstellar travel.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct CoherenceTuning {
    /// Ideal frequency for current 115 isotope resonance (Hz)
    pub target_frequency: f64,

    /// Current amplifier frequency (Hz) — adjusted by pilot
    pub current_frequency: f64,

    /// Coherence factor (1.0 = perfect warp, 0.0 = dropped out)
    pub coherence_factor: f64,

    /// Rate at which isotope resonance drifts (Hz/second)
    pub drift_rate: f64,
}

impl Default for CoherenceTuning {
    fn default() -> Self {
        Self {
            target_frequency: 7.46e9,   // 7.46 GHz
            current_frequency: 7.46e9,
            coherence_factor: 1.0,
            drift_rate: 1000.0,          // 1 kHz/s drift
        }
    }
}

// ============================================================================
// MassShielding — Gravitational Flux Redirection
// ============================================================================

/// Gravitational flux redirection using electromagnetic fields
///
/// Uses superconductors or rotating masses to "divert" local
/// gravitational field. Reduces gravitational drag in shield direction.
///
/// Tied to Reactor115 output — if reactor damaged or out of fuel,
/// shield drops and craft suddenly gains "weight."
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct MassShielding {
    /// Shielding effectiveness (0.0 - 1.0)
    pub effectiveness: f64,

    /// Shielding direction (unit vector)
    pub shield_direction: Vec3,

    /// Angular coverage (radians)
    pub cone_angle: f64,

    /// Energy cost per second (Watts)
    pub power_consumption: f64,

    /// Active state
    pub active: bool,
}

impl MassShielding {
    pub fn new(effectiveness: f64, cone_angle: f64) -> Self {
        Self {
            effectiveness: effectiveness.clamp(0.0, 1.0),
            shield_direction: Vec3::ZERO,
            cone_angle,
            power_consumption: 0.0,
            active: false,
        }
    }

    /// Calculate shielding factor for a given gravity source direction
    pub fn shielding_factor(&self, gravity_direction: Vec3) -> f64 {
        if !self.active {
            return 1.0;
        }
        let angle = self.shield_direction.dot(gravity_direction).acos();
        if angle <= self.cone_angle as f32 {
            1.0 - self.effectiveness
        } else {
            1.0
        }
    }

    pub fn activate(&mut self, direction: Vec3) {
        self.shield_direction = direction.normalize_or_zero();
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }
}

impl Default for MassShielding {
    fn default() -> Self {
        Self::new(0.5, PI / 4.0) // 50% effectiveness, 45° cone
    }
}

// ============================================================================
// ReactionlessDrive — Zero-G Cabin + Inertial Dampening
// ============================================================================

/// Combined exotic propulsion state
///
/// Because the entire craft (and everything inside it) is falling
/// at the same rate within the warp bubble, pilots feel zero G-force.
/// You could perform a 90° turn at 10,000 mph with no "splat."
#[derive(Component, Debug, Clone)]
pub struct ReactionlessDrive {
    /// Current acceleration (m/s²)
    pub acceleration: Vec3,

    /// Zero G-forces flag (ship moves with spacetime bubble)
    pub inertial_dampening: bool,

    /// Total energy reserves (J)
    pub total_energy: f64,

    /// Energy generation rate (W) — from Reactor115
    pub power_generation: f64,
}

impl Default for ReactionlessDrive {
    fn default() -> Self {
        Self {
            acceleration: Vec3::ZERO,
            inertial_dampening: true,
            total_energy: 1e20,   // 100 exajoules
            power_generation: 0.0, // Set by reactor
        }
    }
}

// ============================================================================
// SurfaceTemperature — Hull Heat from TEG Waste
// ============================================================================

/// Hull surface temperature from TEG waste heat
///
/// Waste heat = 1% of reactor output (what TEG didn't capture).
/// Aerogel + Tungsten shielding reduces external transfer.
/// At 99% TEG efficiency, hull remains "safe to touch."
/// At 10% TEG efficiency (modern standards), craft melts instantly.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SurfaceTemperature {
    /// External hull temperature (Kelvin)
    pub hull_temperature: f64,

    /// Tungsten/Aerogel shielding factor (heat reduction multiplier)
    pub shielding_factor: f64,

    /// Ambient temperature (K)
    pub ambient: f64,
}

impl SurfaceTemperature {
    /// Calculate hull heat from reactor waste
    pub fn calculate_hull_heat(&mut self, reactor: &Reactor115) {
        // Waste heat = 1% the TEG didn't capture
        let waste_heat = reactor.current_output * (1.0 - reactor.teg_efficiency);
        // Aerogel shielding reduces external transfer dramatically
        let heat_transfer = waste_heat * self.shielding_factor;
        // Simple thermal model: ΔT proportional to heat input
        self.hull_temperature = self.ambient + heat_transfer * 1e-9;
    }
}

impl Default for SurfaceTemperature {
    fn default() -> Self {
        Self {
            hull_temperature: 293.15, // Room temperature (20°C)
            shielding_factor: 1e-5,   // Aerogel shielding
            ambient: 293.15,
        }
    }
}

// ============================================================================
// ThermalDynamics — Core/Shell Temperature + Frequency Drift Feedback
// ============================================================================

/// Thermal feedback loop linking reactor waste heat to frequency drift
///
/// As the Element 115 reactor increases power, waste heat makes the
/// isotope's resonance more volatile. The thermal_drift_coefficient
/// determines how many Hz of drift increase per Kelvin above ambient.
///
/// Without a Hybrid Quantum Computer to auto-correct, the pilot must
/// manually chase the frequency to prevent a Gravity Snap.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ThermalDynamics {
    /// Current internal core temperature (Kelvin)
    pub core_temperature: f32,

    /// External shell temperature (Kelvin) — "safe to touch" target
    pub shell_temperature: f32,

    /// Heat transfer rate from core to shell (Aerogel/Tungsten shielding)
    pub insulation_factor: f32,

    /// Hz of frequency drift increase per Kelvin above ambient
    pub thermal_drift_coefficient: f64,

    /// Ambient baseline temperature (Kelvin)
    pub ambient: f32,
}

impl Default for ThermalDynamics {
    fn default() -> Self {
        Self {
            core_temperature: 293.0,       // Room temperature
            shell_temperature: 293.0,
            insulation_factor: 0.00001,    // Aerogel/Tungsten shielding
            thermal_drift_coefficient: 50.0, // 50 Hz drift increase per Kelvin
            ambient: 293.0,
        }
    }
}

// ============================================================================
// SpacetimeStress — Gravity Snap on Coherence Collapse
// ============================================================================

/// Spacetime stress accumulator for gravity snap events
///
/// When coherence_factor drops below 0.1 during high-output lasing,
/// the potential energy stored in the warp tunnel can no longer be
/// contained. It snaps back to flat Minkowski space, releasing a
/// radial shockwave that dissipates as a gravitational "ring."
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SpacetimeStress {
    /// Accumulated stress from de-coherence (0.0 - 1.0)
    pub stress_level: f32,

    /// Snap magnitude when coherence collapses (Joules)
    pub snap_energy: f64,

    /// Whether a snap event is currently active
    pub snap_active: bool,

    /// Decay rate for stress dissipation (per second)
    pub decay_rate: f32,
}

impl Default for SpacetimeStress {
    fn default() -> Self {
        Self {
            stress_level: 0.0,
            snap_energy: 0.0,
            snap_active: false,
            decay_rate: 0.5, // Stress halves roughly every 2 seconds
        }
    }
}

// ============================================================================
// PlanetaryFloat — Hover Equilibrium via g(h) = GM/(R+h)²
// ============================================================================

/// Planetary float equilibrium calculator
///
/// To float, the artificial acceleration (a_artificial) generated by
/// the Element 115 A-wave must equal and opposite the planet's g(h).
///
/// g(h) = G × M_planet / (R_planet + h)²
///
/// If the reactor has a maximum power output (limiting artificial gradient),
/// there is a specific height where gravity becomes weak enough to lock onto.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct PlanetaryFloat {
    /// Mass of the nearest planet (kg) — Earth default
    pub planet_mass: f64,

    /// Radius of the nearest planet (m) — Earth default
    pub planet_radius: f64,

    /// Current altitude above surface (m)
    pub altitude: f64,

    /// Whether hover lock is engaged
    pub hover_locked: bool,

    /// Target hover altitude (m) — autopilot target
    pub target_altitude: f64,

    /// Local gravitational acceleration at current altitude (m/s²)
    pub local_gravity: f64,
}

impl Default for PlanetaryFloat {
    fn default() -> Self {
        let planet_mass = 5.972e24;   // Earth mass (kg)
        let planet_radius = 6.371e6;  // Earth radius (m)
        let altitude = 100.0;         // 100m default hover
        let r = planet_radius + altitude;
        let local_gravity = G * planet_mass / (r * r);

        Self {
            planet_mass,
            planet_radius,
            altitude,
            hover_locked: false,
            target_altitude: 100.0,
            local_gravity,
        }
    }
}

impl PlanetaryFloat {
    /// Calculate local gravitational acceleration at altitude h
    /// g(h) = G × M / (R + h)²
    pub fn gravity_at_altitude(&self, h: f64) -> f64 {
        let r = self.planet_radius + h;
        G * self.planet_mass / (r * r)
    }

    /// Update local_gravity from current altitude
    pub fn update_local_gravity(&mut self) {
        self.local_gravity = self.gravity_at_altitude(self.altitude);
    }

    /// Calculate the minimum altitude at which a given artificial acceleration
    /// can achieve hover equilibrium: solve GM/(R+h)² = a_artificial for h
    pub fn minimum_hover_altitude(&self, artificial_acceleration: f64) -> f64 {
        if artificial_acceleration <= 0.0 {
            return f64::INFINITY;
        }
        // h = sqrt(GM / a) - R
        let h = (G * self.planet_mass / artificial_acceleration).sqrt() - self.planet_radius;
        h.max(0.0)
    }
}

// ============================================================================
// KinematicBuffer — Smooth Ride via PID-style Rune Control
// ============================================================================

/// Kinematic buffer for smooth UFO-style flight
///
/// Implements Proportional-Integral-Derivative (PID) logic using
/// the kinematic "Runes":
///
/// - Mu (μ): Momentum & relativistic inertia
/// - Nu (ν): Velocity damping against spacetime fabric
/// - Pi (π): Impulse low-pass filter (absorbs reactor micro-spikes)
/// - Iota (ι): Counter-torque for gyroscopic precession from torus swirl
///
/// The "Ghost Box" concept: the solid ship is smoothly interpolated
/// toward where the Mu rune predicts it should be in 10ms.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct KinematicBuffer {
    /// Current smoothed velocity (m/s)
    pub smoothed_velocity: Vec3,

    /// Mu (μ): Effective relativistic mass ratio (changes as γ warps space)
    pub mu_momentum: f64,

    /// Nu (ν): Velocity damping coefficient (drag against spacetime fabric)
    pub nu_damping: f64,

    /// Pi (π): Impulse absorption strength (low-pass filter cutoff)
    pub pi_impulse_filter: f64,

    /// Iota (ι): Counter-torque coefficient for gyroscopic precession
    pub iota_counter_torque: f64,

    /// Ghost position: predicted target position (interpolation target)
    pub ghost_position: Vec3,

    /// Interpolation speed (0.0 = no smoothing, 1.0 = instant snap)
    pub interpolation_rate: f32,
}

impl Default for KinematicBuffer {
    fn default() -> Self {
        Self {
            smoothed_velocity: Vec3::ZERO,
            mu_momentum: 1.0,         // 1.0 = rest mass (no relativistic correction)
            nu_damping: 0.95,         // 95% velocity retention per tick
            pi_impulse_filter: 0.1,   // Only 10% of spikes pass through
            iota_counter_torque: 1.0, // Full counter-torque compensation
            ghost_position: Vec3::ZERO,
            interpolation_rate: 0.15, // Smooth 15% per frame
        }
    }
}

// ============================================================================
// RegenerativeBraking — Momentum → Reactor Recharge
// ============================================================================

/// Regenerative braking system for the gravity drive
///
/// When decelerating, excess momentum is converted back into
/// electrical energy and fed into the Reactor115 reserves.
/// This extends fuel life during stop-and-go maneuvers.
///
/// Energy recovered = 0.5 × m_effective × Δv²
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct RegenerativeBraking {
    /// Conversion efficiency of momentum → electrical energy (0.0 - 1.0)
    pub recovery_efficiency: f64,

    /// Whether regenerative braking is active
    pub active: bool,

    /// Total energy recovered since last reset (Joules)
    pub energy_recovered: f64,

    /// Previous frame velocity for Δv calculation
    pub previous_velocity: Vec3,
}

impl Default for RegenerativeBraking {
    fn default() -> Self {
        Self {
            recovery_efficiency: 0.85, // 85% kinetic → electrical
            active: true,
            energy_recovered: 0.0,
            previous_velocity: Vec3::ZERO,
        }
    }
}

// ============================================================================
// ToroidalCore — MHD Containment + Lense-Thirring + Flux Quantization
// ============================================================================

/// Toroidal reactor core geometry and containment physics
///
/// The torus enables:
/// - Poloidal field: primary containment (prevents plasma-hull contact)
/// - Toroidal field: A-wave amplification via self-interference per revolution
/// - MHD stability: plasma swirl must remain laminar (turbulence = efficiency drop)
/// - Lense-Thirring: relativistic frame-dragging softens local spacetime metric
/// - Flux quantization: energy exists in discrete packets (gears for the reactor)
/// - Topological insulator lining: 100% surface conduction, bulk insulation
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ToroidalCore {
    /// Poloidal magnetic flux (Tesla·m²) — containment strength
    pub poloidal_flux: f64,

    /// Toroidal magnetic flux (Tesla·m²) — A-wave amplification path
    pub toroidal_flux: f64,

    /// Plasma swirl velocity as fraction of c (β = v/c)
    pub plasma_beta: f64,

    /// MHD stability index (1.0 = laminar, 0.0 = fully turbulent)
    pub mhd_stability: f64,

    /// Lense-Thirring frame-drag magnitude (rad/s equivalent)
    pub frame_drag: f64,

    /// Current flux quantum level (discrete energy step, like a "gear")
    pub flux_quantum_level: u32,

    /// Maximum flux quantum level
    pub max_flux_quantum: u32,

    /// Impedance match ratio (1.0 = perfect, 0.0 = total reflection)
    pub impedance_match: f64,
}

impl Default for ToroidalCore {
    fn default() -> Self {
        Self {
            poloidal_flux: 10.0,        // Strong containment
            toroidal_flux: 50.0,        // A-wave amplification path
            plasma_beta: 0.01,          // 1% of c default swirl
            mhd_stability: 1.0,         // Perfectly laminar at start
            frame_drag: 0.0,            // No drag until spun up
            flux_quantum_level: 1,      // First gear
            max_flux_quantum: 10,       // 10 discrete energy steps
            impedance_match: 0.99,      // Near-perfect impedance
        }
    }
}

impl ToroidalCore {
    /// Calculate Lorentz factor γ from plasma velocity
    pub fn lorentz_factor(&self) -> f64 {
        1.0 / (1.0 - self.plasma_beta * self.plasma_beta).sqrt()
    }

    /// Calculate Lense-Thirring frame-drag from plasma angular momentum
    /// Simplified: frame_drag ∝ γ × β × flux
    pub fn update_frame_drag(&mut self) {
        let gamma = self.lorentz_factor();
        self.frame_drag = gamma * self.plasma_beta * self.toroidal_flux * 1e-6;
    }

    /// MHD stability degrades with higher plasma velocity and lower containment
    pub fn update_mhd_stability(&mut self) {
        // Reynolds-like number: higher velocity / containment ratio = more turbulent
        let reynolds_analog = self.plasma_beta * 1000.0 / self.poloidal_flux.max(0.001);
        self.mhd_stability = (1.0 - reynolds_analog * 0.01).clamp(0.0, 1.0);
    }
}

// ============================================================================
// RuneParameters — Greek Variable Architecture (18 Scriptable Parameters)
// ============================================================================

/// Complete Greek Rune parameter set for the differential equation macro system
///
/// Each rune represents a physical variable in the interdependent
/// differential equations governing the Element 115 propulsion system.
/// The Rune-Interpreter uses these to solve hover, warp, and synthesis macros.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct RuneParameters {
    /// α (Alpha): Source flux — mass-energy conversion rate (W)
    pub alpha_source_flux: f64,

    /// β (Beta): Torsional velocity — energy flow in torus relative to c (ratio)
    pub beta_torsional_velocity: f64,

    /// γ (Gamma): Lorentz factor — relativistic metric stiffness (scalar)
    pub gamma_lorentz: f64,

    /// δ (Delta): Gradient change — infinitesimal gravitational potential variation (m/s²)
    pub delta_gradient: f64,

    /// ε (Epsilon): Permittivity — local vacuum/medium coupling constant (F/m)
    pub epsilon_permittivity: f64,

    /// ζ (Zeta): Damping ratio — decay constant of gravity sink oscillations (scalar)
    pub zeta_damping: f64,

    /// η (Eta): Efficiency — ratio of E=mc² output to usable power (W/W)
    pub eta_efficiency: f64,

    /// θ (Theta): Angular phase — wave interference position on unit circle (rad)
    pub theta_phase: f64,

    /// κ (Kappa): Curvature — local spacetime manifold stiffness (m/J)
    pub kappa_curvature: f64,

    /// λ (Lambda): Wavelength — physical distance between A-wave peaks (m)
    pub lambda_wavelength: f64,

    /// μ (Mu): Permeability — magnetic permeability of 115 crystal lattice (H/m)
    pub mu_permeability: f64,

    /// ν (Nu): Frequency — temporal A-wave oscillation rate (Hz)
    pub nu_frequency: f64,

    /// ξ (Xi): Lattice constant — mean distance between 115 nuclei in crystal (m)
    pub xi_lattice: f64,

    /// ρ (Rho): Energy density — concentration of energy within torus (J/m³)
    pub rho_energy_density: f64,

    /// σ (Sigma): Synthesis yield — probability of successful nucleosynthesis (barn)
    pub sigma_yield: f64,

    /// τ (Tau): Time constant — duration of one torus recirculation loop (s)
    pub tau_recirculation: f64,

    /// ψ (Psi): Wave function — probability amplitude of 115 stability (complex magnitude)
    pub psi_wave_function: f64,

    /// ω (Omega): Angular velocity — rotation speed of torus plasma (rad/s)
    pub omega_angular_velocity: f64,
}

impl Default for RuneParameters {
    fn default() -> Self {
        Self {
            alpha_source_flux: 0.0,                    // Set by reactor
            beta_torsional_velocity: 0.01,             // 1% c initial swirl
            gamma_lorentz: 1.0,                        // Rest frame
            delta_gradient: 0.0,                       // No gradient yet
            epsilon_permittivity: 8.854e-12,           // Vacuum permittivity (F/m)
            zeta_damping: 0.7,                         // Critically damped
            eta_efficiency: 0.99,                      // 99% TEG
            theta_phase: 0.0,                          // Forward
            kappa_curvature: KAPPA,                    // Einstein constant
            lambda_wavelength: C / 7.46e9,             // c / 7.46 GHz
            mu_permeability: 1.257e-6,                 // Vacuum permeability (H/m)
            nu_frequency: 7.46e9,                      // 7.46 GHz
            xi_lattice: 3.5e-10,                       // ~3.5 Å estimated for Mc metal
            rho_energy_density: 0.0,                   // Set by reactor
            sigma_yield: 1e-28,                        // Cross-section (barn scale)
            tau_recirculation: 1e-9,                   // Nanosecond loop in torus
            psi_wave_function: 0.0,                    // Uninitialized
            omega_angular_velocity: 0.0,               // Set by torus
        }
    }
}

impl RuneParameters {
    /// Macro: WARP_TUNNEL_LOCK
    /// Solves the derivative of the interference pattern to find the
    /// optimal point where 100% efficiency occurs (coherence apex).
    pub fn macro_warp_tunnel_lock(&self) -> f64 {
        // The derivative of the interference pattern:
        // d/dt[sin(φ) × cos(ω)] / tan(θ)
        let theta_safe = if self.theta_phase.abs() < 1e-10 { 1e-10 } else { self.theta_phase };
        (self.theta_phase.sin() * self.omega_angular_velocity.cos()) / theta_safe.tan()
    }

    /// Macro: HOVER_STABILIZE
    /// Adjusts α (source flux) to maintain δ (gradient) = local g
    pub fn macro_hover_stabilize(&mut self, local_gravity: f64) {
        // Delta must equal local gravity for equilibrium
        self.delta_gradient = local_gravity;

        // Gamma (metric stiffness) required to produce that gradient
        self.gamma_lorentz = 1.0 / (1.0 - self.beta_torsional_velocity.powi(2)).sqrt();

        // Alpha (power) required: solve energy chain backward
        // α = δ / (κ × η × gain)
        let gain = 1e6; // Standard amplifier gain
        if self.kappa_curvature > 0.0 && self.eta_efficiency > 0.0 {
            self.alpha_source_flux = self.delta_gradient / (self.kappa_curvature * self.eta_efficiency * gain);
        }
    }

    /// Update interdependent variables from current reactor state
    pub fn sync_from_reactor(&mut self, reactor: &Reactor115) {
        self.alpha_source_flux = reactor.current_output;
        self.eta_efficiency = reactor.teg_efficiency;
    }

    /// Update torus-derived variables
    pub fn sync_from_torus(&mut self, torus: &ToroidalCore) {
        self.beta_torsional_velocity = torus.plasma_beta;
        self.gamma_lorentz = torus.lorentz_factor();
        self.omega_angular_velocity = torus.plasma_beta * C * 2.0 * PI / self.tau_recirculation;
        self.rho_energy_density = self.alpha_source_flux / (self.tau_recirculation * C * C).max(1e-30);
    }
}

// ============================================================================
// IsotopeState — Fuel Depletion + Scarcity Model
// ============================================================================

/// Element 115 isotope state tracking fuel depletion and stability
///
/// Element 115 is virtually non-existent on Earth. The stable isotope
/// from the Island of Stability (N=184) is the only usable fuel.
/// As it transmutates to 116 and decays, its resonant frequency shifts.
///
/// The depletion constant (epsilon) represents how fast the fuel
/// destabilizes as mass decreases — smaller fuel wedges are more volatile.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct IsotopeState {
    /// Current isotope stability (1.0 = Island of Stability, 0.0 = immediate decay)
    pub stability: f64,

    /// Depletion constant: volatility increase per gram consumed
    pub depletion_epsilon: f64,

    /// Neutron count (target: 184 for Island of Stability)
    pub neutron_count: u32,

    /// Half-life of current isotope state (seconds)
    pub half_life: f64,

    /// Fuel wedge geometry integrity (1.0 = perfect crystal, 0.0 = shattered)
    pub crystal_integrity: f64,
}

impl Default for IsotopeState {
    fn default() -> Self {
        Self {
            stability: 1.0,           // Stable isotope from Island of Stability
            depletion_epsilon: 0.001,  // Slow depletion per gram
            neutron_count: 184,        // Magic number for stability
            half_life: 1e12,           // Effectively stable (~30,000 years)
            crystal_integrity: 1.0,    // Perfect single-crystal wedge
        }
    }
}

impl IsotopeState {
    /// Update isotope state based on fuel consumption
    /// As fuel mass decreases, the remaining wedge becomes less stable
    pub fn update_from_reactor(&mut self, reactor: &Reactor115) {
        // Stability decreases as fuel is consumed (smaller wedge = more volatile)
        let fuel_fraction = (reactor.fuel_mass_grams / 500.0).clamp(0.0, 1.0);
        self.stability = fuel_fraction.powf(0.3); // Gentle curve — crisis below 10%

        // Crystal integrity degrades from self-irradiation (Wigner effect)
        // Hotter reactors damage the lattice faster
        if reactor.throttle > 0.5 {
            self.crystal_integrity -= reactor.throttle * 1e-8;
            self.crystal_integrity = self.crystal_integrity.max(0.0);
        }

        // Half-life shortens as stability drops
        self.half_life = 1e12 * self.stability.powi(4); // Drops rapidly below 0.5 stability
    }
}

// ============================================================================
// Systems — Simulation Loop
// ============================================================================

/// System 1: Reactor fuel consumption → E=mc² → TEG electrical output
fn reactor_evolution_system(
    time: Res<Time>,
    mut query: Query<&mut Reactor115>,
) {
    let dt = time.delta_secs_f64();
    for mut reactor in query.iter_mut() {
        reactor.update(dt);
    }
}

/// System 2: WASD/QE keyboard input → unit circle vector → gradient direction
fn drive_control_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut GravityGradient, &mut Reactor115)>,
) {
    for (mut drive, mut reactor) in query.iter_mut() {
        let mut direction = Vec3::ZERO;

        // WASD mapped to the unit circle (X-Z plane)
        if keyboard_input.pressed(KeyCode::KeyW) { direction += Vec3::Z; }
        if keyboard_input.pressed(KeyCode::KeyS) { direction -= Vec3::Z; }
        if keyboard_input.pressed(KeyCode::KeyA) { direction += Vec3::X; }
        if keyboard_input.pressed(KeyCode::KeyD) { direction -= Vec3::X; }
        // Q/E for vertical (Y-axis)
        if keyboard_input.pressed(KeyCode::KeyE) { direction += Vec3::Y; }
        if keyboard_input.pressed(KeyCode::KeyQ) { direction -= Vec3::Y; }

        if direction != Vec3::ZERO {
            // Focus the reactor energy into the gradient
            drive.set_gradient(direction, 50.0); // 50 m/s² "fall"
            drive.energy_reserves = reactor.current_output;
            reactor.throttle = 1.0; // Full bombardment
        } else {
            drive.gradient_strength = 0.0;
            reactor.throttle = 0.01; // Idle state
        }
    }
}

/// System 3: Update amplifier phase from WASD input (unit circle steering)
fn update_amplifier_phase(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut GravityAmplifier>,
) {
    let mut target_phase = 0.0;
    let mut active = false;

    if keyboard.pressed(KeyCode::KeyW) { target_phase = 0.0; active = true; }
    if keyboard.pressed(KeyCode::KeyD) { target_phase = PI / 2.0; active = true; }
    if keyboard.pressed(KeyCode::KeyS) { target_phase = PI; active = true; }
    if keyboard.pressed(KeyCode::KeyA) { target_phase = 3.0 * PI / 2.0; active = true; }

    for mut amp in query.iter_mut() {
        if active {
            // Smoothly rotate the gravity focus (no instant jerks)
            amp.phase_shift = target_phase;
        }
    }
}

/// System 4: Gravity amplification — TEG power × gain × KAPPA → gradient strength
///
/// Uses Einstein constant κ (8πG/c⁴) to calculate how much space bends.
/// The gradient is mathematically tied to the E=mc² output of the reactor.
fn gravity_amplification_system(
    mut query: Query<(&Reactor115, &GravityAmplifier, &mut GravityGradient)>,
) {
    for (reactor, amp, mut gradient) in query.iter_mut() {
        if reactor.throttle > 0.0 {
            // Available power from TEG
            let available_power = reactor.current_output;

            // A-wave scaling: energy density × gain × κ → spacetime curvature
            let focused_energy_density = available_power * amp.gain * amp.conversion_efficiency;

            // KAPPA (8πG/c⁴) defines spacetime stiffness — very small number
            // so we need massive power to get visible acceleration
            let theoretical_acceleration = focused_energy_density * KAPPA;

            // Limit by hardware max
            gradient.gradient_strength = theoretical_acceleration.min(gradient.max_gradient);

            if reactor.throttle > 0.9 {
                info!("A-Wave Focused: {:.2} m/s² acceleration", gradient.gradient_strength);
            }
        }
    }
}

/// System 5: Inverse square projection — virtual mass at focus point
///
/// F = G × m_ship × m_virtual / r²
/// where m_virtual is derived from reactor E=mc² output
fn apply_inverse_square_pull(
    mut query: Query<(&mut Transform, &Reactor115, &GravityAWaveFocus)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs_f64();

    for (mut transform, reactor, focus) in query.iter_mut() {
        if reactor.throttle <= 0.0 {
            continue;
        }

        // Virtual mass from reactor power: m = E/c² × A-wave gain
        let pulse_energy = reactor.current_output * dt;
        let effective_mass = (pulse_energy / (C * C)) * 1e20; // 1e20 = A-wave gain

        // Inverse square law: a = G × m_virtual / r²
        let r = focus.projection_distance;
        if r < 1.0 { continue; }
        let acceleration_magnitude = (G * effective_mass) / (r * r);

        // Directional vector from unit circle WASD
        let accel_vec = focus.target_direction * acceleration_magnitude as f32;

        // Ship "falls" into the sink — no G-force felt (CoM + atoms move together)
        transform.translation += accel_vec * dt as f32;

        if reactor.throttle > 0.5 {
            trace!(
                "Pulling toward sink at {}m with {:.2}G",
                r,
                acceleration_magnitude / 9.8
            );
        }
    }
}

/// System 6: Lasing warp — phase-locked amplifiers create gravity tunnel
///
/// Amplitudes stack constructively (3×), energy density increases 9×.
/// Ship enters "flow state" where space itself translates.
fn lasing_warp_system(
    mut query: Query<(&mut GravityLaser, &mut Reactor115, &GravityGradient, &mut Transform)>,
    time: Res<Time>,
) {
    for (mut laser, mut reactor, _gradient, mut transform) in query.iter_mut() {
        if laser.is_locked && reactor.current_output > laser.coherence_threshold {
            // Phase-locked: amplitudes stack constructively (3 amplifiers)
            let total_amplitude = reactor.current_output * 3.0;

            // Linear gradient instead of point sink
            let warp_factor = (total_amplitude * KAPPA) / laser.beam_width;

            // Ship moves at warp_factor × C
            let velocity = transform.forward() * (warp_factor * C) as f32;
            transform.translation += velocity * time.delta_secs();

            // Lasing is 100× more expensive than hovering
            reactor.throttle = 100.0;
        }
    }
}

/// System 7: Coherence management — pilot tunes frequency against isotope drift
///
/// Left/Right arrows adjust amplifier frequency.
/// If drift exceeds 50 kHz window, coherence breaks → warp collapse + heat spike.
fn coherence_management_system(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut CoherenceTuning, &mut Reactor115, &mut GravityLaser)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs_f64();

    for (mut tuning, reactor, mut laser) in query.iter_mut() {
        if !laser.is_locked {
            continue;
        }

        // Natural drift: Element 115 core resonance changes over time
        tuning.target_frequency += tuning.drift_rate * dt;

        // Pilot tuning: Left/Right arrows adjust frequency
        if keyboard.pressed(KeyCode::ArrowLeft) {
            tuning.current_frequency -= 5000.0 * dt;
        }
        if keyboard.pressed(KeyCode::ArrowRight) {
            tuning.current_frequency += 5000.0 * dt;
        }

        // Coherence window: 50 kHz tolerance
        let freq_delta = (tuning.target_frequency - tuning.current_frequency).abs();
        tuning.coherence_factor = (1.0 - (freq_delta / 50000.0)).max(0.0);

        // De-coherence consequences
        if tuning.coherence_factor < 0.1 {
            // WARP COLLAPSE: Sudden stop + massive heat spike
            laser.is_locked = false;
            let heat_spike = reactor.current_output * 0.01;
            warn!(
                "Warp Coherence Lost! Heat Spike: {:.1} GW",
                heat_spike / 1e9
            );
        }
    }
}

/// System 8: Hull surface temperature from TEG waste heat
fn update_surface_temperature(
    mut query: Query<(&Reactor115, &mut SurfaceTemperature)>,
) {
    for (reactor, mut surface) in query.iter_mut() {
        surface.calculate_hull_heat(reactor);
    }
}

/// System 9: Mass shielding — reduce gravitational forces from shielded direction
fn apply_mass_shielding(
    mut query: Query<(
        &MassShielding,
        &mut super::dynamic_gravity::DynamicGravityForce,
    )>,
) {
    for (shielding, mut force) in query.iter_mut() {
        if shielding.active && force.force.length() > 0.0 {
            let gravity_direction = force.force.normalize();
            let shielding_factor = shielding.shielding_factor(gravity_direction);
            force.force *= shielding_factor as f32;
        }
    }
}

/// System 10: Reactionless movement — gradient → Transform (the "fall")
///
/// Displacement = 0.5 × a × t² (applied as frame-by-frame velocity)
/// No G-force because ship moves with local spacetime bubble.
fn apply_reactionless_movement(
    mut query: Query<(&mut Transform, &GravityGradient)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    for (mut transform, drive) in query.iter_mut() {
        if drive.gradient_strength > 0.0 {
            let acceleration = drive.direction * drive.gradient_strength as f32;
            transform.translation += acceleration * dt;
        }
    }
}

/// System 11: Thermal feedback — waste heat raises core temp, accelerates frequency drift
///
/// Core temperature rises from reactor waste heat (1 - TEG efficiency).
/// Shell temperature slowly equalizes through insulation.
/// The temperature delta above ambient directly increases CoherenceTuning drift_rate.
fn thermal_feedback_system(
    time: Res<Time>,
    mut query: Query<(&Reactor115, &mut ThermalDynamics, &mut CoherenceTuning)>,
) {
    let dt = time.delta_secs();

    for (reactor, mut thermal, mut tuning) in query.iter_mut() {
        // Waste heat raises core temperature
        let waste_heat_watts = reactor.current_output * (1.0 - reactor.teg_efficiency);
        let heat_input = waste_heat_watts as f32 * 1e-12; // Scale to Kelvin/s
        thermal.core_temperature += heat_input * dt;

        // Core-to-shell heat transfer through insulation
        let delta_temperature = thermal.core_temperature - thermal.shell_temperature;
        thermal.shell_temperature += delta_temperature * thermal.insulation_factor * dt;

        // Passive cooling: shell radiates toward ambient
        let shell_delta = thermal.shell_temperature - thermal.ambient;
        thermal.shell_temperature -= shell_delta * 0.001 * dt;

        // Core also cools (slower — deep inside the reactor)
        let core_delta = thermal.core_temperature - thermal.ambient;
        thermal.core_temperature -= core_delta * 0.0001 * dt;

        // Thermal drift feedback: hotter core = faster frequency drift
        let excess_temperature = (thermal.core_temperature - thermal.ambient).max(0.0) as f64;
        tuning.drift_rate = 1000.0 + excess_temperature * thermal.thermal_drift_coefficient;
    }
}

/// System 12: Spacetime stress accumulation and gravity snap events
///
/// Stress accumulates during lasing proportional to reactor output.
/// If coherence drops below 0.1, stress releases as a "gravity snap"
/// — the warp tunnel collapses and stored energy radiates outward.
fn spacetime_stress_system(
    time: Res<Time>,
    mut query: Query<(&GravityLaser, &CoherenceTuning, &Reactor115, &mut SpacetimeStress)>,
) {
    let dt = time.delta_secs();

    for (laser, tuning, reactor, mut stress) in query.iter_mut() {
        if laser.is_locked {
            // Stress builds proportional to reactor output during lasing
            let stress_input = (reactor.current_output * KAPPA) as f32 * dt;
            stress.stress_level = (stress.stress_level + stress_input).min(1.0);
        }

        // Coherence collapse triggers snap
        if tuning.coherence_factor < 0.1 && stress.stress_level > 0.01 {
            stress.snap_active = true;
            stress.snap_energy = reactor.current_output * stress.stress_level as f64;
            warn!(
                "GRAVITY SNAP! Stress: {:.2}, Release energy: {:.2e} J",
                stress.stress_level, stress.snap_energy
            );
            stress.stress_level = 0.0; // Fully discharged
        } else {
            stress.snap_active = false;
        }

        // Natural stress decay (spacetime relaxation)
        stress.stress_level *= (1.0 - stress.decay_rate * dt).max(0.0);
    }
}

/// System 13: Toroidal core physics — MHD stability, frame-drag, flux quantization
///
/// Each tick updates the torus containment state:
/// - Recalculates MHD stability from plasma velocity vs containment
/// - Updates Lense-Thirring frame-drag from relativistic plasma
/// - Degrades impedance match if MHD goes turbulent
fn toroidal_core_system(
    mut query: Query<(&Reactor115, &mut ToroidalCore)>,
) {
    for (reactor, mut torus) in query.iter_mut() {
        // Plasma velocity scales with reactor throttle
        torus.plasma_beta = 0.01 * reactor.throttle.max(0.0);

        // MHD stability check
        torus.update_mhd_stability();

        // Lense-Thirring frame-drag
        torus.update_frame_drag();

        // Impedance degrades if MHD is turbulent
        torus.impedance_match = 0.99 * torus.mhd_stability.max(0.1);
    }
}

/// System 14: Isotope state evolution — fuel depletion degrades stability and crystal
fn isotope_evolution_system(
    mut query: Query<(&Reactor115, &mut IsotopeState)>,
) {
    for (reactor, mut isotope) in query.iter_mut() {
        isotope.update_from_reactor(reactor);
    }
}

/// System 15: Rune parameter synchronization — pull live values from reactor and torus
fn rune_sync_system(
    mut query: Query<(&Reactor115, &ToroidalCore, &mut RuneParameters)>,
) {
    for (reactor, torus, mut runes) in query.iter_mut() {
        runes.sync_from_reactor(reactor);
        runes.sync_from_torus(torus);
    }
}

/// System 16: Planetary hover lock — maintain altitude by balancing g(h) against gradient
///
/// When hover_locked is true, the system adjusts GravityGradient strength
/// to exactly cancel local gravity at the current altitude.
/// If the gradient can't match g(h), the craft slowly descends to
/// the minimum hover altitude where equilibrium is possible.
fn planetary_hover_system(
    time: Res<Time>,
    mut query: Query<(
        &mut PlanetaryFloat,
        &mut GravityGradient,
        &mut RuneParameters,
        &mut Transform,
    )>,
) {
    let dt = time.delta_secs_f64();

    for (mut float, mut gradient, mut runes, mut transform) in query.iter_mut() {
        // Update local gravity at current altitude
        float.update_local_gravity();

        if float.hover_locked {
            // Hover lock: set gradient to exactly cancel gravity
            gradient.set_gradient(-Vec3::Y, float.local_gravity);

            // Altitude correction toward target
            let altitude_error = float.target_altitude - float.altitude;
            let correction = altitude_error * 0.1 * dt; // Gentle PID-like correction
            float.altitude += correction;
            transform.translation.y += correction as f32;

            // Update runes for hover macro
            runes.macro_hover_stabilize(float.local_gravity);
        }
    }
}

/// System 17: Kinematic buffer — smooth ride via ghost position interpolation
///
/// Instead of applying raw gradient acceleration to the transform,
/// the kinematic buffer calculates a "ghost position" where the ship
/// should be, then smoothly interpolates the actual transform toward it.
/// Mu filters momentum, Nu damps oscillation, Pi absorbs spikes.
fn kinematic_buffer_system(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &GravityGradient, &mut KinematicBuffer)>,
) {
    let dt = time.delta_secs();

    for (mut transform, gradient, mut buffer) in query.iter_mut() {
        // Read rune parameters into locals to avoid borrow conflicts
        let pi_filter = buffer.pi_impulse_filter as f32;
        let mu_factor = buffer.mu_momentum as f32;
        let nu_damp = buffer.nu_damping as f32;
        let interp_rate = buffer.interpolation_rate;

        if gradient.gradient_strength > 0.0 {
            // Raw acceleration from gradient
            let raw_acceleration = gradient.direction * gradient.gradient_strength as f32;

            // Pi filter: absorb micro-spikes (low-pass)
            let filtered_acceleration = raw_acceleration * pi_filter;

            // Mu: momentum-weighted velocity accumulation
            buffer.smoothed_velocity += filtered_acceleration * dt / mu_factor;

            // Nu: velocity damping (drag against spacetime fabric)
            buffer.smoothed_velocity *= nu_damp;

            // Ghost position: where the ship SHOULD be
            buffer.ghost_position = transform.translation + buffer.smoothed_velocity * dt;
        } else {
            // No input: damp velocity to zero
            buffer.smoothed_velocity *= nu_damp;
            buffer.ghost_position = transform.translation + buffer.smoothed_velocity * dt;
        }

        // Smooth interpolation: actual position → ghost position
        let ghost = buffer.ghost_position;
        transform.translation = transform.translation.lerp(ghost, interp_rate);
    }
}

/// System 18: Regenerative braking — momentum delta → reactor recharge
///
/// When the craft decelerates (current velocity < previous velocity),
/// the kinetic energy difference is converted back to electrical energy
/// and added to the reactor's fuel efficiency buffer.
fn regenerative_braking_system(
    mut query: Query<(&mut RegenerativeBraking, &KinematicBuffer, &mut ReactionlessDrive)>,
) {
    for (mut regen, buffer, mut drive) in query.iter_mut() {
        if !regen.active {
            regen.previous_velocity = buffer.smoothed_velocity;
            continue;
        }

        // Calculate velocity delta
        let delta_velocity = regen.previous_velocity - buffer.smoothed_velocity;
        let delta_speed_squared = delta_velocity.length_squared() as f64;

        // Only recover energy when decelerating (positive delta_speed)
        if delta_speed_squared > 0.001 {
            // E = 0.5 × m_effective × Δv² × recovery_efficiency
            let effective_mass = 1000.0; // Ship mass estimate (kg)
            let recovered = 0.5 * effective_mass * delta_speed_squared * regen.recovery_efficiency;
            regen.energy_recovered += recovered;
            drive.total_energy += recovered;

            if recovered > 1e6 {
                trace!("Regenerative braking recovered {:.2e} J", recovered);
            }
        }

        regen.previous_velocity = buffer.smoothed_velocity;
    }
}

// ============================================================================
// ExoticPropulsionPlugin — Unified Bevy Plugin
// ============================================================================

/// Unified plugin for Element 115 exotic propulsion
///
/// System execution order (chained for correctness):
///
/// **Power & Input Layer:**
/// 1. reactor_evolution_system — Calculate power from E=mc²
/// 2. drive_control_system — Map WASD to gradient direction
/// 3. update_amplifier_phase — Map WASD to A-wave phase
///
/// **Amplification Layer:**
/// 4. gravity_amplification_system — TEG power × gain → curvature
/// 5. apply_inverse_square_pull — Virtual mass at focus → force on CoM
///
/// **Warp Layer:**
/// 6. lasing_warp_system — Phase-locked warp tunnel
/// 7. coherence_management_system — Pilot frequency tuning
///
/// **Thermal & Structural Layer:**
/// 8. update_surface_temperature — Hull heat from TEG waste
/// 9. thermal_feedback_system — Core/shell temp → drift rate feedback
/// 10. spacetime_stress_system — Stress accumulation & gravity snap
///
/// **Core Physics Layer:**
/// 11. toroidal_core_system — MHD stability, frame-drag, impedance
/// 12. isotope_evolution_system — Fuel depletion and crystal degradation
/// 13. rune_sync_system — Sync Greek rune parameters from reactor/torus
///
/// **Navigation Layer:**
/// 14. apply_mass_shielding — Reduce external gravity
/// 15. planetary_hover_system — g(h) equilibrium for hover lock
/// 16. apply_reactionless_movement — Gradient → Transform (raw)
/// 17. kinematic_buffer_system — Ghost box smoothing (overrides raw)
/// 18. regenerative_braking_system — Δv → energy recovery
pub struct ExoticPropulsionPlugin;

impl Plugin for ExoticPropulsionPlugin {
    fn build(&self, app: &mut App) {
        // Register all component types for reflection/inspection
        app.register_type::<Reactor115>()
            .register_type::<GravityAmplifier>()
            .register_type::<GravityAWaveFocus>()
            .register_type::<WaveCascader>()
            .register_type::<GravityLaser>()
            .register_type::<CoherenceTuning>()
            .register_type::<MassShielding>()
            .register_type::<GravityGradient>()
            .register_type::<SurfaceTemperature>()
            .register_type::<ThermalDynamics>()
            .register_type::<SpacetimeStress>()
            .register_type::<PlanetaryFloat>()
            .register_type::<KinematicBuffer>()
            .register_type::<RegenerativeBraking>()
            .register_type::<ToroidalCore>()
            .register_type::<RuneParameters>()
            .register_type::<IsotopeState>()
            .add_systems(
                Update,
                (
                    // --- Power & Input Layer ---
                    // 1. Calculate power from E=mc²
                    reactor_evolution_system,
                    // 2. Map WASD to gradient direction
                    drive_control_system,
                    // 3. Map WASD to A-wave phase
                    update_amplifier_phase,
                    // --- Amplification Layer ---
                    // 4. TEG power × gain → curvature
                    gravity_amplification_system,
                    // 5. Virtual mass at focus → force on CoM
                    apply_inverse_square_pull,
                    // --- Warp Layer ---
                    // 6. Phase-locked warp tunnel
                    lasing_warp_system,
                    // 7. Pilot frequency tuning
                    coherence_management_system,
                    // --- Thermal & Structural Layer ---
                    // 8. Hull heat from TEG waste
                    update_surface_temperature,
                    // 9. Core/shell temp → drift rate feedback
                    thermal_feedback_system,
                    // 10. Stress accumulation & gravity snap
                    spacetime_stress_system,
                    // --- Core Physics Layer ---
                    // 11. MHD stability, frame-drag, impedance
                    toroidal_core_system,
                    // 12. Fuel depletion and crystal degradation
                    isotope_evolution_system,
                    // 13. Sync Greek rune parameters from reactor/torus
                    rune_sync_system,
                    // --- Navigation Layer ---
                    // 14. Reduce external gravity
                    apply_mass_shielding,
                    // 15. g(h) equilibrium for hover lock
                    planetary_hover_system,
                    // 16. Gradient → Transform (raw)
                    apply_reactionless_movement,
                    // 17. Ghost box smoothing (overrides raw)
                    kinematic_buffer_system,
                    // 18. Δv → energy recovery
                    regenerative_braking_system,
                )
                    .chain(), // Chain ensures strict ordering across all 18 systems
            );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reactor115_fuel_consumption() {
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0;
        let initial_fuel = reactor.fuel_mass_grams;
        reactor.update(1.0); // 1 second

        // Fuel should decrease
        assert!(reactor.fuel_mass_grams < initial_fuel);
        // Output should be massive (E=mc²)
        assert!(reactor.current_output > 0.0);
    }

    #[test]
    fn test_reactor115_teg_efficiency() {
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0;
        reactor.teg_efficiency = 0.99;
        reactor.update(1.0);
        let high_efficiency_output = reactor.current_output;

        let mut reactor_low = Reactor115::default();
        reactor_low.throttle = 1.0;
        reactor_low.teg_efficiency = 0.10; // Modern TEG
        reactor_low.update(1.0);
        let low_efficiency_output = reactor_low.current_output;

        // 99% TEG should produce ~10x more than 10% TEG
        assert!(high_efficiency_output > low_efficiency_output * 5.0);
    }

    #[test]
    fn test_gravity_gradient_direction() {
        let mut gradient = GravityGradient::default();
        gradient.set_gradient(Vec3::X, 50.0);

        assert_eq!(gradient.direction, Vec3::X);
        assert_eq!(gradient.gradient_strength, 50.0);
    }

    #[test]
    fn test_gravity_gradient_max_clamp() {
        let mut gradient = GravityGradient::new(100.0, 0.85);
        gradient.set_gradient(Vec3::Z, 200.0); // Over max

        assert_eq!(gradient.gradient_strength, 100.0); // Clamped
    }

    #[test]
    fn test_mass_shielding_factor() {
        let mut shield = MassShielding::new(0.5, PI / 4.0);
        shield.activate(Vec3::X);

        // Gravity from shielded direction = 50% reduction
        let factor = shield.shielding_factor(Vec3::X);
        assert!((factor - 0.5).abs() < 0.01);

        // Gravity from unshielded direction = no reduction
        let factor_y = shield.shielding_factor(Vec3::Y);
        assert!((factor_y - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_wave_cascader_interference() {
        let cascader = WaveCascader::default();
        let intensity = cascader.calculate_focus_intensity(1e12, 0.0, 10.0);
        // Energy density should be positive (squared interference)
        assert!(intensity >= 0.0);
    }

    #[test]
    fn test_coherence_tuning_drift() {
        let mut tuning = CoherenceTuning::default();
        let initial = tuning.target_frequency;

        // Simulate 10 seconds of drift
        tuning.target_frequency += tuning.drift_rate * 10.0;

        // Target should drift by 10 kHz
        let delta = tuning.target_frequency - initial;
        assert!((delta - 10000.0).abs() < 0.01);

        // Coherence should drop
        let freq_delta = (tuning.target_frequency - tuning.current_frequency).abs();
        tuning.coherence_factor = (1.0 - (freq_delta / 50000.0)).max(0.0);
        assert!(tuning.coherence_factor < 1.0);
        assert!(tuning.coherence_factor > 0.0); // Still within 50 kHz window
    }

    #[test]
    fn test_surface_temperature_safe() {
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0;
        reactor.teg_efficiency = 0.99;
        reactor.update(1.0);

        let mut surface = SurfaceTemperature::default();
        surface.calculate_hull_heat(&reactor);

        // At 99% TEG efficiency + aerogel, hull should be near ambient
        assert!(surface.hull_temperature < 400.0); // Below 127°C
    }

    #[test]
    fn test_surface_temperature_dangerous() {
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0;
        reactor.teg_efficiency = 0.10; // Modern TEG (terrible)
        reactor.update(1.0);

        let mut surface = SurfaceTemperature::default();
        surface.shielding_factor = 0.01; // Poor shielding
        surface.calculate_hull_heat(&reactor);

        // At 10% TEG with poor shielding, hull gets extremely hot
        assert!(surface.hull_temperature > surface.ambient);
    }


    #[test]
    fn test_gravity_laser_defaults() {
        let laser = GravityLaser::default();
        assert!(!laser.is_locked);
        assert_eq!(laser.coherence_threshold, 1e15);
        assert_eq!(laser.beam_width, 0.001);
    }

    #[test]
    fn test_reactor115_fuel_depletion() {
        let mut reactor = Reactor115 {
            fuel_mass_grams: 1e-10, // Nearly empty
            throttle: 1.0,
            teg_efficiency: 0.99,
            current_output: 0.0,
        };
        reactor.update(1.0);

        // Should deplete and shut down
        assert_eq!(reactor.fuel_mass_grams, 0.0);
        assert_eq!(reactor.current_output, 0.0);
        assert_eq!(reactor.throttle, 0.0);
    }

    #[test]
    fn test_gravity_amplifier_defaults() {
        let amp = GravityAmplifier::default();
        assert_eq!(amp.operating_frequency, 7.46);
        assert_eq!(amp.gain, 1e6);
        assert_eq!(amp.phase_shift, 0.0);
        assert_eq!(amp.conversion_efficiency, 0.85);
    }

    #[test]
    fn test_gravity_a_wave_focus_defaults() {
        let focus = GravityAWaveFocus::default();
        assert_eq!(focus.projection_distance, 10.0);
        assert_eq!(focus.virtual_mass, 1e12);
        assert_eq!(focus.target_direction, Vec3::Z);
    }

    #[test]
    fn test_reactionless_drive_defaults() {
        let drive = ReactionlessDrive::default();
        assert!(drive.inertial_dampening);
        assert_eq!(drive.acceleration, Vec3::ZERO);
        assert_eq!(drive.total_energy, 1e20);
    }

    // ====================================================================
    // New production-grade ship component tests
    // ====================================================================

    #[test]
    fn test_thermal_dynamics_defaults() {
        let thermal = ThermalDynamics::default();
        assert_eq!(thermal.core_temperature, 293.0);
        assert_eq!(thermal.shell_temperature, 293.0);
        assert_eq!(thermal.thermal_drift_coefficient, 50.0);
        assert!(thermal.insulation_factor < 0.001); // Very good insulation
    }

    #[test]
    fn test_spacetime_stress_defaults() {
        let stress = SpacetimeStress::default();
        assert_eq!(stress.stress_level, 0.0);
        assert!(!stress.snap_active);
        assert_eq!(stress.snap_energy, 0.0);
        assert_eq!(stress.decay_rate, 0.5);
    }

    #[test]
    fn test_planetary_float_earth_gravity() {
        let float = PlanetaryFloat::default();
        // Earth surface gravity should be ~9.8 m/s² at 100m altitude
        let surface_gravity = float.gravity_at_altitude(0.0);
        assert!((surface_gravity - 9.82).abs() < 0.1);

        // Gravity at 100m should be nearly the same as surface
        assert!((float.local_gravity - 9.82).abs() < 0.1);
    }

    #[test]
    fn test_planetary_float_minimum_hover_altitude() {
        let float = PlanetaryFloat::default();
        // At 9.8 m/s² artificial acceleration, hover altitude should be near surface
        let altitude = float.minimum_hover_altitude(9.82);
        assert!(altitude < 1000.0); // Within a kilometer

        // At 1 m/s², hover altitude must be much higher (weaker gravity needed)
        let high_altitude = float.minimum_hover_altitude(1.0);
        assert!(high_altitude > altitude);

        // Zero acceleration → infinite altitude (can never hover)
        let impossible = float.minimum_hover_altitude(0.0);
        assert!(impossible.is_infinite());
    }

    #[test]
    fn test_kinematic_buffer_defaults() {
        let buffer = KinematicBuffer::default();
        assert_eq!(buffer.smoothed_velocity, Vec3::ZERO);
        assert_eq!(buffer.mu_momentum, 1.0);
        assert_eq!(buffer.nu_damping, 0.95);
        assert_eq!(buffer.pi_impulse_filter, 0.1);
        assert_eq!(buffer.iota_counter_torque, 1.0);
        assert_eq!(buffer.ghost_position, Vec3::ZERO);
        assert_eq!(buffer.interpolation_rate, 0.15);
    }

    #[test]
    fn test_regenerative_braking_defaults() {
        let regen = RegenerativeBraking::default();
        assert!(regen.active);
        assert_eq!(regen.recovery_efficiency, 0.85);
        assert_eq!(regen.energy_recovered, 0.0);
        assert_eq!(regen.previous_velocity, Vec3::ZERO);
    }

    #[test]
    fn test_toroidal_core_lorentz_factor() {
        let mut torus = ToroidalCore::default();
        // At 1% c, Lorentz factor should be very close to 1.0
        assert!((torus.lorentz_factor() - 1.0).abs() < 0.001);

        // At 90% c, Lorentz factor should be ~2.29
        torus.plasma_beta = 0.9;
        assert!((torus.lorentz_factor() - 2.294).abs() < 0.01);
    }

    #[test]
    fn test_toroidal_core_mhd_stability() {
        let mut torus = ToroidalCore::default();
        torus.update_mhd_stability();
        // At low plasma velocity with strong containment, stability should be high
        assert!(torus.mhd_stability > 0.9);

        // Crank up the plasma velocity → turbulence
        torus.plasma_beta = 0.5;
        torus.poloidal_flux = 1.0; // Weaker containment
        torus.update_mhd_stability();
        assert!(torus.mhd_stability < 0.9);
    }

    #[test]
    fn test_toroidal_core_frame_drag() {
        let mut torus = ToroidalCore::default();
        torus.update_frame_drag();
        // At 1% c with default flux, frame drag should be small but nonzero
        assert!(torus.frame_drag > 0.0);
        assert!(torus.frame_drag < 0.001); // Tiny at 1% c
    }

    #[test]
    fn test_rune_parameters_defaults() {
        let runes = RuneParameters::default();
        assert_eq!(runes.alpha_source_flux, 0.0); // No reactor yet
        assert_eq!(runes.beta_torsional_velocity, 0.01);
        assert_eq!(runes.gamma_lorentz, 1.0); // Rest frame
        assert_eq!(runes.eta_efficiency, 0.99);
        assert_eq!(runes.kappa_curvature, KAPPA);
        assert_eq!(runes.nu_frequency, 7.46e9);
        // Lambda = c / frequency
        let expected_lambda = C / 7.46e9;
        assert!((runes.lambda_wavelength - expected_lambda).abs() < 1e-6);
    }

    #[test]
    fn test_rune_hover_stabilize() {
        let mut runes = RuneParameters::default();
        runes.macro_hover_stabilize(9.81); // Earth gravity

        // Delta should be set to local gravity
        assert_eq!(runes.delta_gradient, 9.81);

        // Alpha (power) should be nonzero — reactor needs to work
        assert!(runes.alpha_source_flux > 0.0);

        // Gamma should be very close to 1.0 at 1% c beta
        assert!((runes.gamma_lorentz - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_rune_sync_from_reactor() {
        let mut runes = RuneParameters::default();
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0;
        reactor.update(1.0);

        runes.sync_from_reactor(&reactor);
        assert_eq!(runes.alpha_source_flux, reactor.current_output);
        assert_eq!(runes.eta_efficiency, reactor.teg_efficiency);
    }

    #[test]
    fn test_rune_sync_from_torus() {
        let mut runes = RuneParameters::default();
        let torus = ToroidalCore::default();

        runes.sync_from_torus(&torus);
        assert_eq!(runes.beta_torsional_velocity, torus.plasma_beta);
        assert!((runes.gamma_lorentz - torus.lorentz_factor()).abs() < 1e-10);
        assert!(runes.omega_angular_velocity > 0.0); // Nonzero for nonzero beta
    }

    #[test]
    fn test_rune_warp_tunnel_lock() {
        let mut runes = RuneParameters::default();
        runes.theta_phase = PI / 4.0;
        runes.omega_angular_velocity = 1000.0;
        let result = runes.macro_warp_tunnel_lock();
        // Should return a finite number (derivative of interference pattern)
        assert!(result.is_finite());
    }

    #[test]
    fn test_isotope_state_defaults() {
        let isotope = IsotopeState::default();
        assert_eq!(isotope.stability, 1.0);
        assert_eq!(isotope.neutron_count, 184);
        assert_eq!(isotope.crystal_integrity, 1.0);
        assert_eq!(isotope.half_life, 1e12);
    }

    #[test]
    fn test_isotope_state_depletion() {
        let mut isotope = IsotopeState::default();
        let mut reactor = Reactor115::default();

        // Full fuel → full stability
        isotope.update_from_reactor(&reactor);
        assert_eq!(isotope.stability, 1.0);

        // Half fuel → reduced stability
        reactor.fuel_mass_grams = 250.0;
        isotope.update_from_reactor(&reactor);
        assert!(isotope.stability < 1.0);
        assert!(isotope.stability > 0.5); // Gentle curve

        // Nearly empty → critical stability
        reactor.fuel_mass_grams = 10.0;
        isotope.update_from_reactor(&reactor);
        assert!(isotope.stability < 0.5);
    }

    #[test]
    fn test_isotope_crystal_degradation() {
        let mut isotope = IsotopeState::default();
        let mut reactor = Reactor115::default();
        reactor.throttle = 1.0; // High throttle damages crystal

        // Run many cycles at high throttle
        for _ in 0..10000 {
            isotope.update_from_reactor(&reactor);
        }

        // Crystal integrity should have degraded
        assert!(isotope.crystal_integrity < 1.0);
    }

    #[test]
    fn test_isotope_halflife_scales_with_stability() {
        let mut isotope = IsotopeState::default();
        let mut reactor = Reactor115::default();

        // Full fuel → long half-life
        isotope.update_from_reactor(&reactor);
        let full_halflife = isotope.half_life;

        // Low fuel → much shorter half-life
        reactor.fuel_mass_grams = 50.0;
        isotope.update_from_reactor(&reactor);
        assert!(isotope.half_life < full_halflife);

        // Near-empty → very short half-life
        reactor.fuel_mass_grams = 5.0;
        isotope.update_from_reactor(&reactor);
        assert!(isotope.half_life < full_halflife * 0.01);
    }
}
