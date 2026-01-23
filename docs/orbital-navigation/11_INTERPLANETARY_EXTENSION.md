# 11 - Interplanetary Extension

> Extending the Eustress Orbital Navigation System for Mars missions and beyond

## Overview

This document analyzes the current system's readiness for interplanetary travel (specifically Earth-to-Mars missions) and outlines the required modifications and additions.

## Current System Assessment

The Eustress Orbital Navigation System was designed with extensibility in mind. Many components require only configuration changes for Mars missions, while others need meaningful extensions.

---

## Components Ready with Minor Tuning

These components are **almost ready** and require low-to-medium effort changes:

### 1. Orbital Gravity Resource

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Gravity model | `earth_only()` | `full_system()` or custom n-body | **Low** |
| Central body | Earth-centric | Configurable (Sun, Mars, Earth) | Low |
| SOI transitions | Not implemented | Add sphere-of-influence detection | Medium |

```rust
/// Extended gravity configuration for interplanetary travel
#[derive(Clone, Debug)]
pub enum GravityModel {
    /// Single central body (current default)
    SingleBody { gm: f64, j2: Option<f64> },
    /// Full solar system n-body
    FullSystem,
    /// Custom selection of bodies
    Custom { bodies: Vec<CelestialBody> },
    /// Patched conics with SOI transitions
    PatchedConics { primary: CelestialBody, secondaries: Vec<CelestialBody> },
}

impl GravityModel {
    pub fn earth_only() -> Self {
        Self::SingleBody { gm: GM_EARTH, j2: Some(J2_EARTH) }
    }
    
    pub fn mars_only() -> Self {
        Self::SingleBody { gm: GM_MARS, j2: Some(J2_MARS) }
    }
    
    pub fn full_system() -> Self {
        Self::FullSystem
    }
    
    pub fn earth_mars_transfer() -> Self {
        Self::PatchedConics {
            primary: CelestialBody::Sun,
            secondaries: vec![CelestialBody::Earth, CelestialBody::Mars],
        }
    }
}
```

---

### 2. Ephemeris Engine

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Planet positions | ✓ Via pracstro/nyx | Already supports Mars, Earth, Sun | **Low** |
| Accuracy | Sufficient for LEO | Verify accuracy over 6-9 month spans | Low |
| Mars moons | Not included | Add Phobos/Deimos ephemerides | Low |

```rust
/// Extended ephemeris for interplanetary missions
impl EphemerisEngine {
    /// Get Mars position (heliocentric, ICRS)
    pub fn mars_position(&self, jd: f64) -> DVec3 {
        self.position(CelestialBody::Mars, jd)
    }
    
    /// Get Earth-Mars vector
    pub fn earth_to_mars(&self, jd: f64) -> DVec3 {
        self.mars_position(jd) - self.position(CelestialBody::Earth, jd)
    }
    
    /// Get phase angle (Sun-Earth-Mars angle)
    pub fn phase_angle(&self, jd: f64) -> f64 {
        let sun_earth = -self.position(CelestialBody::Earth, jd);
        let earth_mars = self.earth_to_mars(jd);
        sun_earth.angle_between(earth_mars)
    }
    
    /// Get synodic period remaining until next transfer window
    pub fn next_transfer_window(&self, jd: f64) -> f64 {
        // Earth-Mars synodic period ≈ 780 days
        const SYNODIC_PERIOD: f64 = 779.94;
        // Calculate optimal phase angle (~44° for Hohmann)
        let current_phase = self.phase_angle(jd).to_degrees();
        let optimal_phase = 44.0;
        
        let phase_diff = (optimal_phase - current_phase).rem_euclid(360.0);
        phase_diff / 360.0 * SYNODIC_PERIOD
    }
}
```

---

### 3. Region Registry

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Load radius | 10⁶ m (1,000 km) | Increase to 10⁸–10⁹ m | **Low** |
| Unload radius | 10⁶ m | Scale proportionally | Low |
| Region size | 1,000 km cells | Adaptive based on distance from bodies | Medium |

```rust
/// Interplanetary region configuration
impl RegionRegistry {
    /// Configure for Earth orbit operations
    pub fn earth_orbit_config() -> Self {
        Self::new(25, 1_000_000.0) // 1,000 km load radius
    }
    
    /// Configure for interplanetary cruise
    pub fn interplanetary_config() -> Self {
        Self::new(25, 100_000_000_000.0) // 100 million km load radius
    }
    
    /// Configure for Mars approach
    pub fn mars_approach_config() -> Self {
        Self::new(25, 10_000_000.0) // 10,000 km load radius
    }
    
    /// Adaptive configuration based on nearest body distance
    pub fn adaptive_config(distance_to_nearest_body: f64) -> Self {
        let load_radius = (distance_to_nearest_body * 0.1).clamp(1_000_000.0, 1e11);
        Self::new(25, load_radius)
    }
}
```

---

### 4. Threat Queue / Time Horizon

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Prediction horizon | ~1 day (86,400 s) | Extend to weeks/months | **Medium** |
| Update frequency | High (collision focus) | Lower for cruise phase | Low |
| Threat types | Debris, satellites | Add asteroid/comet catalog | Medium |

```rust
/// Extended threat configuration for interplanetary travel
#[derive(Clone, Debug)]
pub struct InterplanetaryThreatConfig {
    /// Prediction horizon (seconds)
    pub time_horizon: f64,
    /// Minimum approach distance for alert (meters)
    pub threat_threshold: f64,
    /// Mission phase affects parameters
    pub phase: MissionPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionPhase {
    EarthDeparture,      // High threat awareness, short horizon
    InterplanetaryCruise, // Low threat, long horizon
    MarsApproach,        // Medium threat, medium horizon
    MarsOrbit,           // High threat (Phobos/Deimos), short horizon
}

impl InterplanetaryThreatConfig {
    pub fn for_phase(phase: MissionPhase) -> Self {
        match phase {
            MissionPhase::EarthDeparture => Self {
                time_horizon: 86_400.0 * 7.0,  // 1 week
                threat_threshold: 10_000.0,     // 10 km
                phase,
            },
            MissionPhase::InterplanetaryCruise => Self {
                time_horizon: 86_400.0 * 90.0, // 90 days
                threat_threshold: 1_000_000.0,  // 1,000 km
                phase,
            },
            MissionPhase::MarsApproach => Self {
                time_horizon: 86_400.0 * 14.0, // 2 weeks
                threat_threshold: 100_000.0,    // 100 km
                phase,
            },
            MissionPhase::MarsOrbit => Self {
                time_horizon: 86_400.0 * 3.0,  // 3 days
                threat_threshold: 5_000.0,      // 5 km
                phase,
            },
        }
    }
}
```

---

### 5. SGP4 / TLE Propagation

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Propagator | SGP4 (Earth satellites) | **Replace** with n-body or high-fidelity | **High** |
| Use case | LEO/MEO/GEO tracking | Irrelevant for Mars transfers | N/A |
| Alternative | None | Patched conics or full-force models | High |

**Note**: SGP4 is fundamentally Earth-centric and unsuitable for interplanetary trajectories. It should be replaced with:
- Patched conics for quick approximations
- Full n-body integration for high-fidelity propagation

---

### 6. Navigation HUD

| Aspect | Current State | Required Change | Effort |
|--------|---------------|-----------------|--------|
| Elements displayed | Earth-centric orbital | Add heliocentric elements | **Medium** |
| Phase information | None | Phase angle to Mars, synodic period | Medium |
| Transfer data | None | Δv budget, time-to-arrival | Medium |

```rust
/// Extended navigation display for interplanetary travel
#[derive(Clone, Debug, Default)]
pub struct InterplanetaryDisplay {
    // Heliocentric orbital elements
    pub heliocentric_elements: Option<OrbitalElements>,
    
    // Mars-relative data
    pub distance_to_mars: f64,
    pub phase_angle_deg: f64,
    pub time_to_mars_soi: f64,
    
    // Transfer window data
    pub days_to_next_window: f64,
    pub synodic_period_days: f64,
    
    // Δv budget
    pub delta_v_remaining: f64,
    pub delta_v_to_capture: f64,
    
    // Communication
    pub light_time_to_earth: f64,
    pub light_time_to_mars: f64,
    
    // Mission timeline
    pub mission_elapsed_time: f64,
    pub estimated_arrival: f64,
}
```

---

## Components Requiring Meaningful Extension

These capabilities need to be **added or significantly extended**:

### 1. High-Fidelity Interplanetary Propagator

**Why needed**: Hohmann/Lambert/low-thrust trajectories, n-body with Sun+planets

**Effort**: **High**

```rust
/// Interplanetary trajectory propagator
pub struct InterplanetaryPropagator {
    /// Gravitational parameters for all bodies
    bodies: Vec<(CelestialBody, f64)>,
    /// Ephemeris engine for body positions
    ephemeris: EphemerisEngine,
    /// Integration tolerance
    tolerance: f64,
}

impl InterplanetaryPropagator {
    /// Propagate state using n-body integration
    pub fn propagate(
        &self,
        state: OrbitalState,
        t0: f64,
        t1: f64,
        dt: f64,
    ) -> Vec<(f64, OrbitalState)> {
        let mut trajectory = Vec::new();
        let mut t = t0;
        let mut current = state;
        
        while t < t1 {
            // N-body acceleration from all bodies
            let accel = self.n_body_acceleration(current.position, t);
            
            // RK4 or higher-order integration
            current = rk4_step(t, current, dt, |t, s| {
                self.n_body_acceleration(s.position, t)
            });
            
            t += dt;
            trajectory.push((t, current));
        }
        
        trajectory
    }
    
    /// Calculate n-body gravitational acceleration
    fn n_body_acceleration(&self, position: DVec3, jd: f64) -> DVec3 {
        let mut accel = DVec3::ZERO;
        
        for (body, gm) in &self.bodies {
            let body_pos = self.ephemeris.position(*body, jd);
            let r = body_pos - position;
            let r_mag = r.length();
            
            if r_mag > 1.0 {
                accel += *gm / (r_mag * r_mag * r_mag) * r;
            }
        }
        
        accel
    }
}

/// Lambert solver for transfer orbit calculation
pub fn lambert_solver(
    r1: DVec3,           // Initial position
    r2: DVec3,           // Final position
    tof: f64,            // Time of flight (seconds)
    gm: f64,             // Central body GM
    prograde: bool,      // Prograde or retrograde transfer
) -> Result<(DVec3, DVec3)> {
    // Returns (v1, v2) - velocities at departure and arrival
    // Implementation uses universal variable formulation
    todo!("Lambert solver implementation")
}

/// Hohmann transfer calculator
pub fn hohmann_transfer(
    r1: f64,  // Initial orbit radius
    r2: f64,  // Final orbit radius
    gm: f64,  // Central body GM
) -> HohmannTransfer {
    let a_transfer = (r1 + r2) / 2.0;
    
    let v1_circular = (gm / r1).sqrt();
    let v1_transfer = (gm * (2.0 / r1 - 1.0 / a_transfer)).sqrt();
    let delta_v1 = (v1_transfer - v1_circular).abs();
    
    let v2_transfer = (gm * (2.0 / r2 - 1.0 / a_transfer)).sqrt();
    let v2_circular = (gm / r2).sqrt();
    let delta_v2 = (v2_circular - v2_transfer).abs();
    
    let tof = std::f64::consts::PI * (a_transfer.powi(3) / gm).sqrt();
    
    HohmannTransfer {
        delta_v1,
        delta_v2,
        total_delta_v: delta_v1 + delta_v2,
        time_of_flight: tof,
        transfer_semi_major_axis: a_transfer,
    }
}

#[derive(Clone, Debug)]
pub struct HohmannTransfer {
    pub delta_v1: f64,
    pub delta_v2: f64,
    pub total_delta_v: f64,
    pub time_of_flight: f64,
    pub transfer_semi_major_axis: f64,
}
```

---

### 2. Patched Conics / Sphere-of-Influence Hand-off

**Why needed**: Smooth transition Earth SOI → heliocentric → Mars SOI

**Effort**: **Medium–High**

```rust
/// Sphere of Influence radii (meters)
pub mod soi {
    pub const EARTH: f64 = 924_000_000.0;      // ~924,000 km
    pub const MARS: f64 = 577_000_000.0;       // ~577,000 km
    pub const MOON: f64 = 66_000_000.0;        // ~66,000 km
    pub const JUPITER: f64 = 48_200_000_000.0; // ~48.2 million km
}

/// Current sphere of influence
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SphereOfInfluence {
    Earth,
    Moon,
    Mars,
    Heliocentric,
    Jupiter,
    // ... other bodies
}

/// Patched conics propagator with SOI transitions
pub struct PatchedConicsPropagator {
    ephemeris: EphemerisEngine,
}

impl PatchedConicsPropagator {
    /// Determine current SOI based on position
    pub fn current_soi(&self, position_heliocentric: DVec3, jd: f64) -> SphereOfInfluence {
        // Check distance to each body
        let earth_pos = self.ephemeris.position(CelestialBody::Earth, jd);
        let mars_pos = self.ephemeris.position(CelestialBody::Mars, jd);
        
        let dist_earth = (position_heliocentric - earth_pos).length();
        let dist_mars = (position_heliocentric - mars_pos).length();
        
        if dist_earth < soi::EARTH {
            SphereOfInfluence::Earth
        } else if dist_mars < soi::MARS {
            SphereOfInfluence::Mars
        } else {
            SphereOfInfluence::Heliocentric
        }
    }
    
    /// Propagate with automatic SOI transitions
    pub fn propagate_with_transitions(
        &self,
        initial_state: OrbitalState,
        initial_soi: SphereOfInfluence,
        t0: f64,
        t1: f64,
    ) -> Vec<SoiTransition> {
        let mut transitions = Vec::new();
        let mut current_soi = initial_soi;
        let mut state = initial_state;
        let mut t = t0;
        
        while t < t1 {
            // Propagate in current SOI
            let (new_state, new_t) = self.propagate_in_soi(state, current_soi, t, t1);
            
            // Check for SOI transition
            let new_soi = self.current_soi(new_state.position, new_t);
            
            if new_soi != current_soi {
                transitions.push(SoiTransition {
                    time: new_t,
                    from: current_soi,
                    to: new_soi,
                    position: new_state.position,
                    velocity: new_state.velocity,
                });
                
                // Transform state to new reference frame
                state = self.transform_to_soi(new_state, current_soi, new_soi, new_t);
                current_soi = new_soi;
            } else {
                state = new_state;
            }
            
            t = new_t;
        }
        
        transitions
    }
}

#[derive(Clone, Debug)]
pub struct SoiTransition {
    pub time: f64,
    pub from: SphereOfInfluence,
    pub to: SphereOfInfluence,
    pub position: DVec3,
    pub velocity: DVec3,
}
```

---

### 3. Deep-Space Communication Delay Modeling

**Why needed**: 4–24 minute one-way light time affects commanding

**Effort**: **Medium**

```rust
/// Communication delay modeling
pub struct CommDelayModel {
    ephemeris: EphemerisEngine,
}

impl CommDelayModel {
    /// Calculate one-way light time to Earth (seconds)
    pub fn light_time_to_earth(&self, spacecraft_pos: DVec3, jd: f64) -> f64 {
        let earth_pos = self.ephemeris.position(CelestialBody::Earth, jd);
        let distance = (spacecraft_pos - earth_pos).length();
        distance / C
    }
    
    /// Calculate round-trip communication delay (seconds)
    pub fn round_trip_delay(&self, spacecraft_pos: DVec3, jd: f64) -> f64 {
        2.0 * self.light_time_to_earth(spacecraft_pos, jd)
    }
    
    /// Check if real-time control is possible (< 3 second delay)
    pub fn is_realtime_possible(&self, spacecraft_pos: DVec3, jd: f64) -> bool {
        self.round_trip_delay(spacecraft_pos, jd) < 3.0
    }
    
    /// Get communication window status
    pub fn comm_status(&self, spacecraft_pos: DVec3, jd: f64) -> CommStatus {
        let delay = self.light_time_to_earth(spacecraft_pos, jd);
        
        CommStatus {
            one_way_delay_seconds: delay,
            one_way_delay_minutes: delay / 60.0,
            round_trip_seconds: delay * 2.0,
            realtime_possible: delay < 1.5,
            autonomous_required: delay > 60.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommStatus {
    pub one_way_delay_seconds: f64,
    pub one_way_delay_minutes: f64,
    pub round_trip_seconds: f64,
    pub realtime_possible: bool,
    pub autonomous_required: bool,
}

/// Earth-Mars communication delay ranges
pub mod comm_delays {
    /// Minimum Earth-Mars distance (~54.6 million km)
    pub const MIN_DISTANCE: f64 = 54_600_000_000.0;
    /// Maximum Earth-Mars distance (~401 million km)
    pub const MAX_DISTANCE: f64 = 401_000_000_000.0;
    /// Minimum one-way light time (~3.03 minutes)
    pub const MIN_LIGHT_TIME: f64 = 182.0;
    /// Maximum one-way light time (~22.3 minutes)
    pub const MAX_LIGHT_TIME: f64 = 1338.0;
}
```

---

### 4. Solar Radiation Pressure & Drag

**Why needed**: Becomes non-negligible over 6–9 month transfer

**Effort**: **Medium**

```rust
/// Solar radiation pressure model
pub struct SolarRadiationPressure {
    /// Spacecraft cross-sectional area (m²)
    pub area: f64,
    /// Spacecraft mass (kg)
    pub mass: f64,
    /// Reflectivity coefficient (1.0 = absorbing, 2.0 = reflecting)
    pub cr: f64,
}

impl SolarRadiationPressure {
    /// Solar constant at 1 AU (W/m²)
    const SOLAR_CONSTANT: f64 = 1361.0;
    
    /// Calculate SRP acceleration
    pub fn acceleration(&self, position_heliocentric: DVec3) -> DVec3 {
        let r = position_heliocentric.length();
        let r_au = r / AU;
        
        // Solar flux at current distance (inverse square)
        let flux = Self::SOLAR_CONSTANT / (r_au * r_au);
        
        // Radiation pressure (N/m²)
        let pressure = flux / C;
        
        // Acceleration magnitude (m/s²)
        let accel_mag = self.cr * pressure * self.area / self.mass;
        
        // Direction: away from Sun
        let direction = position_heliocentric.normalize();
        
        direction * accel_mag
    }
}

/// Atmospheric drag (for Mars approach/aerobraking)
pub struct AtmosphericDrag {
    /// Drag coefficient
    pub cd: f64,
    /// Cross-sectional area (m²)
    pub area: f64,
    /// Spacecraft mass (kg)
    pub mass: f64,
}

impl AtmosphericDrag {
    /// Mars atmospheric density model (exponential)
    pub fn mars_density(altitude: f64) -> f64 {
        // Surface density ~0.020 kg/m³
        // Scale height ~11.1 km
        const RHO_0: f64 = 0.020;
        const H: f64 = 11_100.0;
        
        RHO_0 * (-altitude / H).exp()
    }
    
    /// Calculate drag acceleration at Mars
    pub fn acceleration_mars(&self, altitude: f64, velocity: DVec3) -> DVec3 {
        let rho = Self::mars_density(altitude);
        let v_mag = velocity.length();
        
        // Drag force: F = 0.5 * ρ * v² * Cd * A
        let drag_mag = 0.5 * rho * v_mag * v_mag * self.cd * self.area / self.mass;
        
        // Direction: opposite to velocity
        -velocity.normalize() * drag_mag
    }
}
```

---

### 5. Transfer Window Calculator

**Why needed**: Show next Earth–Mars opportunities, Δv budgets

**Effort**: **Medium**

```rust
/// Transfer window calculator
pub struct TransferWindowCalculator {
    ephemeris: EphemerisEngine,
}

impl TransferWindowCalculator {
    /// Find next optimal transfer window
    pub fn next_window(&self, current_jd: f64) -> TransferWindow {
        // Search for optimal phase angle (~44° for Hohmann)
        let mut jd = current_jd;
        let mut best_window = None;
        let mut best_delta_v = f64::INFINITY;
        
        // Search over next 3 years (covers full synodic period)
        while jd < current_jd + 365.25 * 3.0 {
            let phase = self.phase_angle(jd);
            
            // Check if near optimal (40-50°)
            if phase > 40.0_f64.to_radians() && phase < 50.0_f64.to_radians() {
                let delta_v = self.estimate_delta_v(jd);
                
                if delta_v < best_delta_v {
                    best_delta_v = delta_v;
                    best_window = Some(TransferWindow {
                        departure_jd: jd,
                        phase_angle: phase,
                        estimated_delta_v: delta_v,
                        transfer_time_days: self.estimate_transfer_time(jd),
                        arrival_jd: jd + self.estimate_transfer_time(jd),
                    });
                }
            }
            
            jd += 1.0; // Daily resolution
        }
        
        best_window.unwrap_or_default()
    }
    
    /// Calculate phase angle (Sun-Earth-Mars)
    fn phase_angle(&self, jd: f64) -> f64 {
        let earth = self.ephemeris.position(CelestialBody::Earth, jd);
        let mars = self.ephemeris.position(CelestialBody::Mars, jd);
        
        let sun_earth = -earth;
        let earth_mars = mars - earth;
        
        sun_earth.angle_between(earth_mars)
    }
    
    /// Estimate Δv for transfer
    fn estimate_delta_v(&self, jd: f64) -> f64 {
        // Simplified Hohmann estimate
        let earth_r = self.ephemeris.position(CelestialBody::Earth, jd).length();
        let mars_r = self.ephemeris.position(CelestialBody::Mars, jd).length();
        
        let transfer = hohmann_transfer(earth_r, mars_r, GM_SUN);
        transfer.total_delta_v
    }
    
    /// Estimate transfer time
    fn estimate_transfer_time(&self, jd: f64) -> f64 {
        let earth_r = self.ephemeris.position(CelestialBody::Earth, jd).length();
        let mars_r = self.ephemeris.position(CelestialBody::Mars, jd).length();
        
        let transfer = hohmann_transfer(earth_r, mars_r, GM_SUN);
        transfer.time_of_flight / 86400.0 // Convert to days
    }
}

#[derive(Clone, Debug, Default)]
pub struct TransferWindow {
    pub departure_jd: f64,
    pub phase_angle: f64,
    pub estimated_delta_v: f64,
    pub transfer_time_days: f64,
    pub arrival_jd: f64,
}
```

---

### 6. Fuel / Mass Budget Tracking

**Why needed**: Continuous Δv remaining, Isp, propellant margins

**Effort**: **Medium**

```rust
/// Propulsion and mass budget tracking
#[derive(Clone, Debug)]
pub struct PropulsionBudget {
    /// Dry mass (kg)
    pub dry_mass: f64,
    /// Current propellant mass (kg)
    pub propellant_mass: f64,
    /// Specific impulse (seconds)
    pub isp: f64,
    /// Thrust (Newtons)
    pub thrust: f64,
}

impl PropulsionBudget {
    /// Standard gravity (m/s²)
    const G0: f64 = 9.80665;
    
    /// Calculate remaining Δv using Tsiolkovsky equation
    pub fn delta_v_remaining(&self) -> f64 {
        let m0 = self.dry_mass + self.propellant_mass;
        let mf = self.dry_mass;
        
        self.isp * Self::G0 * (m0 / mf).ln()
    }
    
    /// Calculate propellant needed for given Δv
    pub fn propellant_for_delta_v(&self, delta_v: f64) -> f64 {
        let m0 = self.dry_mass + self.propellant_mass;
        let mass_ratio = (delta_v / (self.isp * Self::G0)).exp();
        let mf = m0 / mass_ratio;
        
        m0 - mf
    }
    
    /// Execute burn, consuming propellant
    pub fn execute_burn(&mut self, delta_v: f64) -> Result<(), &'static str> {
        let propellant_needed = self.propellant_for_delta_v(delta_v);
        
        if propellant_needed > self.propellant_mass {
            return Err("Insufficient propellant");
        }
        
        self.propellant_mass -= propellant_needed;
        Ok(())
    }
    
    /// Get current total mass
    pub fn total_mass(&self) -> f64 {
        self.dry_mass + self.propellant_mass
    }
    
    /// Get propellant margin percentage
    pub fn propellant_margin(&self, required_delta_v: f64) -> f64 {
        let available = self.delta_v_remaining();
        (available - required_delta_v) / required_delta_v * 100.0
    }
}

/// Mission Δv budget breakdown
#[derive(Clone, Debug)]
pub struct MissionDeltaVBudget {
    pub earth_departure: f64,
    pub mid_course_corrections: f64,
    pub mars_orbit_insertion: f64,
    pub landing_or_aerobrake: f64,
    pub margin: f64,
    pub total: f64,
}

impl MissionDeltaVBudget {
    /// Typical Earth-Mars mission budget
    pub fn typical_mars_mission() -> Self {
        Self {
            earth_departure: 3600.0,      // ~3.6 km/s
            mid_course_corrections: 50.0,  // ~50 m/s
            mars_orbit_insertion: 900.0,   // ~0.9 km/s (with aerobraking)
            landing_or_aerobrake: 0.0,     // Aerobraking assumed
            margin: 200.0,                 // ~200 m/s margin
            total: 4750.0,                 // ~4.75 km/s total
        }
    }
}
```

---

### 7. Autonomous Course Correction Logic

**Why needed**: Mid-course maneuvers based on optical navigation

**Effort**: **High**

```rust
/// Autonomous navigation and course correction
pub struct AutonomousNavigation {
    /// Target body
    target: CelestialBody,
    /// Planned trajectory
    planned_trajectory: Vec<(f64, OrbitalState)>,
    /// Correction threshold (meters)
    correction_threshold: f64,
}

impl AutonomousNavigation {
    /// Check if course correction is needed
    pub fn needs_correction(&self, current_state: &OrbitalState, current_jd: f64) -> bool {
        let planned = self.interpolate_planned(current_jd);
        let deviation = (current_state.position - planned.position).length();
        
        deviation > self.correction_threshold
    }
    
    /// Calculate correction maneuver
    pub fn calculate_correction(
        &self,
        current_state: &OrbitalState,
        current_jd: f64,
    ) -> Option<CorrectionManeuver> {
        if !self.needs_correction(current_state, current_jd) {
            return None;
        }
        
        let planned = self.interpolate_planned(current_jd);
        
        // Simple velocity correction (more sophisticated: Lambert targeting)
        let delta_v = planned.velocity - current_state.velocity;
        
        if delta_v.length() < 0.1 {
            return None; // Too small to bother
        }
        
        Some(CorrectionManeuver {
            time: current_jd,
            delta_v,
            delta_v_magnitude: delta_v.length(),
            reason: CorrectionReason::TrajectoryDeviation,
        })
    }
    
    fn interpolate_planned(&self, jd: f64) -> OrbitalState {
        // Linear interpolation between trajectory points
        // (In practice, use Hermite or higher-order)
        todo!()
    }
}

#[derive(Clone, Debug)]
pub struct CorrectionManeuver {
    pub time: f64,
    pub delta_v: DVec3,
    pub delta_v_magnitude: f64,
    pub reason: CorrectionReason,
}

#[derive(Clone, Copy, Debug)]
pub enum CorrectionReason {
    TrajectoryDeviation,
    TargetingRefinement,
    HazardAvoidance,
    WindowOptimization,
}
```

---

## Summary: Mars Mission Readiness

### Ready with Minor Tuning (Low Effort)

| Component | Change Required |
|-----------|-----------------|
| Gravity model | Switch to `full_system()` |
| Ephemeris | Already supports Mars (verify accuracy) |
| Region Registry | Increase radii to 10⁸–10⁹ m |

### Ready with Medium Effort

| Component | Change Required |
|-----------|-----------------|
| Threat Queue | Extend horizon to weeks/months |
| Navigation HUD | Add heliocentric elements, phase angle |
| Transfer windows | Add calculator |
| Fuel budget | Add tracking |
| Comm delay | Add modeling |
| SRP/drag | Add perturbation models |

### Requires Significant Development (High Effort)

| Component | Change Required |
|-----------|-----------------|
| Propagator | Replace SGP4 with n-body/patched conics |
| SOI transitions | Implement smooth hand-offs |
| Autonomous nav | Mid-course correction logic |
| Mars approach | High-detail Mars model + atmosphere |

---

## Next Steps

1. **Phase 1**: Low-effort tuning (gravity, ephemeris, regions)
2. **Phase 2**: Medium-effort extensions (HUD, comm, fuel)
3. **Phase 3**: High-effort development (propagator, SOI, autonomy)

See [08_PHYSICS_FOUNDATIONS.md](./08_PHYSICS_FOUNDATIONS.md) for orbital mechanics background.
