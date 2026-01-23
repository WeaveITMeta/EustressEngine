# 10 - API Reference

> Complete API documentation for the Eustress Orbital Navigation System

## Table of Contents

1. [Components](#components)
2. [Resources](#resources)
3. [Systems](#systems)
4. [Functions](#functions)
5. [Types and Enums](#types-and-enums)
6. [Constants](#constants)

---

## Components

### OrbitalCoords

High-precision orbital coordinates with floating-origin support.

```rust
#[derive(Component, Reflect, Clone, Copy, Debug)]
pub struct OrbitalCoords {
    pub global_ecef: DVec3,
    pub region: RegionId,
    pub local_pos: Vec3,
    pub local_vel: Vec3,
    pub sync_transform: bool,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `global_ecef` | `DVec3` | High-precision position in ECEF (meters) |
| `region` | `RegionId` | Current hierarchical region |
| `local_pos` | `Vec3` | Position relative to floating origin |
| `local_vel` | `Vec3` | Velocity in local frame (m/s) |
| `sync_transform` | `bool` | Auto-sync to Bevy Transform |

**Methods:**

```rust
impl OrbitalCoords {
    /// Create from geodetic coordinates
    pub fn from_geodetic(lat_deg: f64, lon_deg: f64, alt_m: f64) -> Self;
    
    /// Create from ECEF position
    pub fn from_ecef(ecef: DVec3) -> Self;
    
    /// Get velocity in ECEF frame
    pub fn velocity_ecef(&self) -> DVec3;
}
```

---

### SpaceshipMarker

Marker component identifying the player's vessel (floating origin).

```rust
#[derive(Component, Reflect, Default)]
pub struct SpaceshipMarker;
```

**Usage:**
```rust
commands.spawn((
    OrbitalCoords::from_geodetic(28.5, -80.6, 400_000.0),
    SpaceshipMarker,
    FloatingOrigin,
));
```

---

### SpaceshipPhysics

Physics state for spacecraft propulsion and dynamics.

```rust
#[derive(Component, Clone, Debug)]
pub struct SpaceshipPhysics {
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub mass: f64,
    pub thrust: Vec3,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `velocity` | `Vec3` | Linear velocity (m/s) |
| `angular_velocity` | `Vec3` | Angular velocity (rad/s) |
| `mass` | `f64` | Total mass (kg) |
| `thrust` | `Vec3` | Current thrust vector (N) |

---

### GravityAligned

Automatically aligns entity orientation to local gravity vector.

```rust
#[derive(Component, Reflect, Clone, Copy, Debug)]
pub struct GravityAligned {
    pub smoothing: f32,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `smoothing` | `f32` | Alignment speed (higher = faster) |

**Methods:**
```rust
impl GravityAligned {
    pub fn smooth(factor: f32) -> Self;
}
```

---

### OrbitalObject

Identifies trackable orbital objects (satellites, planets, debris).

```rust
#[derive(Component)]
pub struct OrbitalObject {
    pub tle: Option<TwoLineElement>,
    pub body: Option<CelestialBody>,
    pub object_type: ObjectType,
    pub sam_model: Option<Handle<Mesh>>,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tle` | `Option<TwoLineElement>` | TLE for SGP4 propagation |
| `body` | `Option<CelestialBody>` | Celestial body for ephemeris |
| `object_type` | `ObjectType` | Classification |
| `sam_model` | `Option<Handle<Mesh>>` | SAM 3D reconstructed mesh |

---

### Sam3dModel

Component for entities with SAM 3D reconstructed models.

```rust
#[derive(Component)]
pub struct Sam3dModel {
    pub cache_id: u64,
    pub quality: ModelQuality,
    pub last_updated: f64,
    pub reference_blend: f32,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `cache_id` | `u64` | Reference to cached model |
| `quality` | `ModelQuality` | Reconstruction quality |
| `last_updated` | `f64` | Timestamp of last observation |
| `reference_blend` | `f32` | Blend with reference model (0-1) |

---

### TargetedSam3d

Marker for currently targeted SAM 3D model.

```rust
#[derive(Component)]
pub struct TargetedSam3d;
```

---

## Resources

### NavigationState

Central navigation state for the vessel.

```rust
#[derive(Resource)]
pub struct NavigationState {
    pub origin_ecef: DVec3,
    pub origin_velocity: DVec3,
    pub origin_region: RegionId,
    pub reference_frame: ReferenceFrame,
    pub last_update: f64,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `origin_ecef` | `DVec3` | Ship position in ECEF |
| `origin_velocity` | `DVec3` | Ship velocity in ECEF |
| `origin_region` | `RegionId` | Current region |
| `reference_frame` | `ReferenceFrame` | Active reference frame |
| `last_update` | `f64` | Last update timestamp |

---

### RegionRegistry

Manages hierarchical spatial regions.

```rust
#[derive(Resource)]
pub struct RegionRegistry {
    regions: HashMap<RegionId, Region>,
    active: Vec<RegionId>,
    max_active: usize,
    load_radius_m: f64,
}
```

**Methods:**

```rust
impl RegionRegistry {
    /// Create new registry
    pub fn new(max_active: usize, load_radius_m: f64) -> Self;
    
    /// Find or create region for ECEF position
    pub fn find_or_create(&mut self, ecef: DVec3, level: u8) -> RegionId;
    
    /// Convert global to local coordinates
    pub fn global_to_local(&self, ecef: DVec3, region: RegionId) -> Vec3;
    
    /// Convert local to global coordinates
    pub fn local_to_global(&self, local: Vec3, region: RegionId) -> DVec3;
    
    /// Update active regions around focus point
    pub fn update_active_regions(&mut self, focus_ecef: DVec3);
    
    /// Set focus point for floating origin
    pub fn set_focus(&mut self, ecef: DVec3);
}
```

---

### NearbyObjectsArray

Sorted array of objects near the vessel.

```rust
#[derive(Resource)]
pub struct NearbyObjectsArray {
    pub objects: Vec<NearbyObject>,
    pub max_range: f64,
    pub max_objects: usize,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `objects` | `Vec<NearbyObject>` | Sorted by distance |
| `max_range` | `f64` | Maximum tracking range (m) |
| `max_objects` | `usize` | Maximum tracked objects |

---

### ThreatQueue

Priority queue of collision threats.

```rust
#[derive(Resource)]
pub struct ThreatQueue {
    pub threats: BinaryHeap<CollisionThreat>,
    pub threat_threshold: f64,
    pub time_horizon: f64,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `threats` | `BinaryHeap<CollisionThreat>` | Sorted by time-to-closest-approach |
| `threat_threshold` | `f64` | Minimum approach distance (m) |
| `time_horizon` | `f64` | Detection horizon (s) |

---

### TargetList

Designated navigation targets.

```rust
#[derive(Resource, Default)]
pub struct TargetList {
    pub targets: Vec<NavigationTarget>,
    pub primary: Option<usize>,
    pub secondary: Vec<usize>,
}
```

**Methods:**
```rust
impl TargetList {
    /// Add a new target
    pub fn add(&mut self, target: NavigationTarget) -> usize;
    
    /// Set primary target by index
    pub fn set_primary(&mut self, index: usize);
    
    /// Get primary target
    pub fn get_primary(&self) -> Option<&NavigationTarget>;
    
    /// Remove target by index
    pub fn remove(&mut self, index: usize);
}
```

---

### AlertManager

Manages navigation alerts and warnings.

```rust
#[derive(Resource)]
pub struct AlertManager {
    pub active_alerts: Vec<NavigationAlert>,
    pub alert_history: Vec<NavigationAlert>,
    pub max_history: usize,
}
```

**Methods:**
```rust
impl AlertManager {
    /// Raise a new alert
    pub fn raise_alert(&mut self, alert: NavigationAlert);
    
    /// Acknowledge an alert
    pub fn acknowledge(&mut self, alert_id: u64);
    
    /// Dismiss an alert
    pub fn dismiss(&mut self, alert_id: u64);
    
    /// Update (auto-dismiss expired alerts)
    pub fn update(&mut self, current_time: f64);
}
```

---

### TleCatalog

Catalog of Two-Line Element sets.

```rust
#[derive(Resource, Default)]
pub struct TleCatalog {
    pub tles: HashMap<u32, TwoLineElement>,
    pub last_update: f64,
    pub source_url: String,
}
```

**Methods:**
```rust
impl TleCatalog {
    /// Load from file
    pub fn load_from_file(path: &str) -> Result<Self>;
    
    /// Parse TLE file content
    pub fn parse_tle_file(content: &str) -> Result<Self>;
    
    /// Get TLEs by orbital regime
    pub fn get_by_regime(&self, regime: OrbitalRegime) -> Vec<&TwoLineElement>;
    
    /// Get TLE by NORAD ID
    pub fn get(&self, norad_id: u32) -> Option<&TwoLineElement>;
}
```

---

### Sgp4Propagator

SGP4 orbital propagation engine.

```rust
#[derive(Resource)]
pub struct Sgp4Propagator { /* ... */ }
```

**Methods:**
```rust
impl Sgp4Propagator {
    /// Create new propagator
    pub fn new() -> Self;
    
    /// Propagate TLE to Julian Date
    pub fn propagate(&self, tle: &TwoLineElement, jd: f64) -> Result<SatelliteState>;
}
```

---

### EphemerisEngine

Planetary ephemeris calculation engine.

```rust
#[derive(Resource)]
pub struct EphemerisEngine { /* ... */ }
```

**Methods:**
```rust
impl EphemerisEngine {
    /// Create new engine
    pub fn new() -> Self;
    
    /// Get body position (barycentric, ICRS)
    pub fn position(&self, body: CelestialBody, jd: f64) -> DVec3;
    
    /// Get body position (geocentric, GCRS)
    pub fn position_geocentric(&self, body: CelestialBody, jd: f64) -> DVec3;
    
    /// Get body position (ECEF)
    pub fn position_ecef(&self, body: CelestialBody, jd: f64) -> DVec3;
}
```

---

### Sam3dModelCache

Cache for SAM 3D reconstructed models.

```rust
#[derive(Resource)]
pub struct Sam3dModelCache { /* ... */ }
```

**Methods:**
```rust
impl Sam3dModelCache {
    /// Create new cache
    pub fn new(max_models: usize, max_age: f64) -> Self;
    
    /// Insert or update model
    pub fn insert(&mut self, model: CachedSam3dModel);
    
    /// Get model by ID
    pub fn get(&mut self, id: u64) -> Option<&CachedSam3dModel>;
    
    /// Query models near position
    pub fn query_nearby(&self, ecef: DVec3, radius: f64) -> Vec<&CachedSam3dModel>;
    
    /// Evict stale models
    pub fn evict_stale(&mut self, current_time: f64);
}
```

---

## Systems

### Coordinate Systems

| System | Set | Description |
|--------|-----|-------------|
| `update_floating_origin` | `CoordinateUpdate` | Updates floating origin when ship moves |
| `recompute_local_positions` | `CoordinateUpdate` | Recalculates local positions relative to ship |
| `sync_transforms` | `CoordinateUpdate` | Syncs Bevy Transforms from OrbitalCoords |

### Navigation Systems

| System | Set | Description |
|--------|-----|-------------|
| `update_nearby_objects` | `NavigationUpdate` | Updates sorted nearby objects array |
| `update_threat_queue` | `NavigationUpdate` | Detects and prioritizes collision threats |
| `generate_collision_alerts` | `NavigationUpdate` | Creates alerts for threats |
| `update_alert_manager` | `NavigationUpdate` | Processes alert lifecycle |

### Physics Systems

| System | Set | Description |
|--------|-----|-------------|
| `update_spaceship_physics` | `PhysicsUpdate` | Integrates ship motion |
| `execute_maneuvers` | `PhysicsUpdate` | Executes scheduled maneuvers |
| `update_orbital_objects` | `CoordinateUpdate` | Propagates all orbital objects |

### Alignment Systems

| System | Set | Description |
|--------|-----|-------------|
| `align_to_gravity` | `PostPhysics` | Aligns entities to local gravity |

---

## Functions

### Coordinate Transformations

```rust
/// Convert geodetic to ECEF
pub fn geodetic_to_ecef(geo: &Geodetic) -> DVec3;

/// Convert ECEF to geodetic
pub fn ecef_to_geodetic(ecef: DVec3) -> Geodetic;

/// Convert GCRS to ECEF
pub fn gcrs_to_ecef(gcrs: DVec3, jd: f64) -> DVec3;

/// Convert ECEF to GCRS
pub fn ecef_to_gcrs(ecef: DVec3, jd: f64) -> DVec3;

/// Convert TEME to ECEF
pub fn teme_to_ecef(pos: DVec3, vel: DVec3, jd: f64) -> (DVec3, DVec3);
```

### Orbital Mechanics

```rust
/// Solve Kepler's equation
pub fn solve_kepler(mean_anomaly: f64, eccentricity: f64, tolerance: f64) -> f64;

/// Convert state vector to orbital elements
pub fn state_to_elements(position: DVec3, velocity: DVec3) -> OrbitalElements;

/// Convert orbital elements to state vector
pub fn elements_to_state(elements: &OrbitalElements, true_anomaly: f64) -> (DVec3, DVec3);

/// Convert eccentric anomaly to true anomaly
pub fn eccentric_to_true_anomaly(e_anom: f64, ecc: f64) -> f64;

/// Convert true anomaly to eccentric anomaly
pub fn true_to_eccentric_anomaly(true_anom: f64, ecc: f64) -> f64;
```

### Geometry

```rust
/// Check line-of-sight between two points
pub fn has_line_of_sight(pos_a: DVec3, pos_b: DVec3) -> bool;

/// Calculate elevation angle from ground to satellite
pub fn elevation_angle(ground_ecef: DVec3, sat_ecef: DVec3) -> f64;

/// Calculate azimuth angle from ground to satellite
pub fn azimuth_angle(ground_geo: &Geodetic, sat_ecef: DVec3) -> f64;

/// Triangulate 3D position from two observations
pub fn triangulate(ray_1: DVec3, pos_1: DVec3, ray_2: DVec3, pos_2: DVec3) -> DVec3;

/// Calculate depth from parallax
pub fn depth_from_parallax(pixel_1: [f64; 2], pixel_2: [f64; 2], 
                           camera: &PinholeCamera, baseline: f64) -> f64;
```

### Time

```rust
/// Calculate Greenwich Mean Sidereal Time
pub fn greenwich_mean_sidereal_time(jd: f64) -> f64;

/// Convert year and day-of-year to Julian Date
pub fn julian_date_from_year_day(year: u32, day: f64) -> f64;
```

### Physics

```rust
/// Calculate Lorentz factor
pub fn lorentz_factor(velocity: f64) -> f64;

/// Calculate gravitational time dilation
pub fn gravitational_time_dilation(r: f64, gm: f64) -> f64;

/// Calculate relativistic frame rate for navigation
/// FPS(v) = FPS_base × √((1 + β)/(1 - β))
pub fn relativistic_frame_rate(velocity: f64, base_fps: f64) -> f64;

/// Adaptive frame rate with smoothness profile
/// FPS(v, σ) = FPS_base × √((1 + β)/(1 - β)) × σ
pub fn adaptive_frame_rate(velocity: f64, base_fps: f64, profile: SmoothnessProfile) -> f64;

/// Calculate minimum safe frame rate considering velocity and reaction distance
pub fn navigation_frame_rate(velocity: f64, base_fps: f64, min_reaction_dist: f64) -> f64;

/// Calculate third-body gravitational acceleration
pub fn third_body_acceleration(sat_pos: DVec3, perturber_pos: DVec3, gm: f64) -> DVec3;

/// RK4 integration step
pub fn rk4_step(t: f64, state: OrbitalState, dt: f64, accel: AccelerationFn) -> OrbitalState;
```

### Fine-Grained Interpolation

```rust
/// Cubic Hermite spline interpolation for smooth position
pub fn hermite_interpolate(p0: DVec3, v0: DVec3, p1: DVec3, v1: DVec3, t: f64) -> DVec3;

/// Relativistic length contraction correction
pub fn length_contraction_correction(position: DVec3, velocity: DVec3) -> DVec3;

/// Light travel time compensation for distant objects
pub fn light_time_correction(observer_pos: DVec3, object_pos: DVec3, object_vel: DVec3) -> DVec3;

/// Sub-frame position prediction with relativistic corrections
pub fn predict_position(position: DVec3, velocity: DVec3, acceleration: DVec3, 
                        dt: f64, observer_velocity: DVec3) -> DVec3;
```

---

## Types and Enums

### Geodetic

```rust
#[derive(Clone, Copy, Debug)]
pub struct Geodetic {
    pub longitude: f64,  // radians
    pub latitude: f64,   // radians
    pub altitude: f64,   // meters
}

impl Geodetic {
    pub fn from_degrees(lon_deg: f64, lat_deg: f64, alt_m: f64) -> Self;
    pub fn to_degrees(&self) -> (f64, f64, f64);
}
```

### RegionId

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionId {
    pub level: u8,
    pub face: u8,
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub is_abstract: bool,
}

impl RegionId {
    pub fn from_ecef(ecef: DVec3, level: u8) -> Self;
    pub fn parent(&self) -> Option<Self>;
    pub fn children(&self) -> [Self; 8];
}
```

### OrbitalElements

```rust
#[derive(Clone, Copy, Debug)]
pub struct OrbitalElements {
    pub semi_major_axis: f64,
    pub eccentricity: f64,
    pub inclination: f64,
    pub raan: f64,
    pub arg_periapsis: f64,
    pub true_anomaly: f64,
    pub mean_anomaly_epoch: f64,
    pub epoch: f64,
}

impl OrbitalElements {
    pub fn period(&self) -> f64;
    pub fn mean_motion(&self) -> f64;
    pub fn specific_energy(&self) -> f64;
    pub fn periapsis(&self) -> f64;
    pub fn apoapsis(&self) -> f64;
}
```

### TwoLineElement

```rust
#[derive(Clone, Debug)]
pub struct TwoLineElement {
    pub name: String,
    pub norad_id: u32,
    pub intl_designator: String,
    pub epoch: f64,
    pub mean_motion_dot: f64,
    pub mean_motion_ddot: f64,
    pub bstar: f64,
    pub inclination: f64,
    pub raan: f64,
    pub eccentricity: f64,
    pub arg_perigee: f64,
    pub mean_anomaly: f64,
    pub mean_motion: f64,
    pub rev_number: u32,
}

impl TwoLineElement {
    pub fn parse(line0: &str, line1: &str, line2: &str) -> Result<Self>;
}
```

### SmoothnessProfile

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SmoothnessProfile {
    Standard,    // σ = 1.0 - Baseline navigation
    Smooth,      // σ = 2.0 - Enhanced interpolation
    UltraSmooth, // σ = 4.0 - Maximum fidelity
    Cinematic,   // σ = 8.0 - Film-quality rendering
}

impl SmoothnessProfile {
    pub fn factor(&self) -> f64;
}
```

### UpdateTiers

```rust
#[derive(Clone, Debug)]
pub struct UpdateTiers {
    pub physics_hz: f64,       // 1000 Hz default - force, collision
    pub navigation_hz: f64,    // 500 Hz default - position, threats
    pub rendering_hz: f64,     // 240 Hz default - visual output
    pub interpolation_hz: f64, // 10000 Hz default - sub-frame motion
    pub prediction_hz: f64,    // 100 Hz default - trajectory extrapolation
}

impl UpdateTiers {
    /// Scale all tiers for relativistic velocity
    pub fn scale_for_velocity(&self, velocity: f64, profile: SmoothnessProfile) -> Self;
}
```

### Enums

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectType {
    Satellite,
    Debris,
    Planet,
    Moon,
    Star,
    Station,
    Vessel,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceFrame {
    ShipLocal,
    ShipOrbital,
    EarthFixed,
    EarthInertial,
    TargetRelative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CelestialBody {
    Sun,
    Mercury,
    Venus,
    Earth,
    Moon,
    Mars,
    Jupiter,
    Saturn,
    Uranus,
    Neptune,
    Pluto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitalRegime {
    LEO,
    MEO,
    GEO,
    HEO,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertType {
    CollisionWarning,
    ProximityAlert,
    TrajectoryDeviation,
    FuelLow,
    SystemMalfunction,
    TargetAcquired,
    TargetLost,
    ManeuverRequired,
    ManeuverComplete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Caution,
    Warning,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelQuality {
    Rough,
    Standard,
    Refined,
    Reference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManeuverType {
    Prograde,
    Retrograde,
    Normal,
    AntiNormal,
    RadialIn,
    RadialOut,
    Custom,
}
```

---

## Constants

### Physical Constants

```rust
pub mod constants {
    /// Gravitational constant (m³/kg/s²)
    pub const G: f64 = 6.67430e-11;
    
    /// Earth's gravitational parameter (m³/s²)
    pub const GM_EARTH: f64 = 3.986004418e14;
    
    /// Earth's equatorial radius (m)
    pub const R_EARTH: f64 = 6_378_137.0;
    
    /// Earth's polar radius (m)
    pub const R_EARTH_POLAR: f64 = 6_356_752.314245;
    
    /// Earth's J2 zonal harmonic
    pub const J2_EARTH: f64 = 1.08263e-3;
    
    /// Sun's gravitational parameter (m³/s²)
    pub const GM_SUN: f64 = 1.32712440018e20;
    
    /// Moon's gravitational parameter (m³/s²)
    pub const GM_MOON: f64 = 4.9048695e12;
    
    /// Speed of light (m/s)
    pub const C: f64 = 299_792_458.0;
    
    /// Astronomical Unit (m)
    pub const AU: f64 = 149_597_870_700.0;
    
    /// GEO altitude (m)
    pub const GEO_ALTITUDE: f64 = 35_786_000.0;
    
    /// Earth's rotation rate (rad/s)
    pub const OMEGA_EARTH: f64 = 7.2921158553e-5;
}
```

### WGS84 Constants

```rust
pub mod wgs84 {
    /// Semi-major axis (m)
    pub const A: f64 = 6_378_137.0;
    
    /// Flattening
    pub const F: f64 = 1.0 / 298.257223563;
    
    /// Semi-minor axis (m)
    pub const B: f64 = A * (1.0 - F);
    
    /// First eccentricity squared
    pub const E2: f64 = 2.0 * F - F * F;
    
    /// Second eccentricity squared
    pub const EP2: f64 = (A * A - B * B) / (B * B);
}
```

---

## Plugin Configuration

### EustressOrbitalPlugin

```rust
pub struct EustressOrbitalPlugin {
    /// Maximum tracking range (meters)
    pub max_tracking_range: f64,
    
    /// Maximum tracked objects
    pub max_tracked_objects: usize,
    
    /// Threat detection time horizon (seconds)
    pub threat_time_horizon: f64,
    
    /// Enable SAM 3D processing
    pub enable_sam3d: bool,
}

impl Default for EustressOrbitalPlugin {
    fn default() -> Self {
        Self {
            max_tracking_range: 100_000_000.0,
            max_tracked_objects: 1000,
            threat_time_horizon: 86400.0,
            enable_sam3d: false,
        }
    }
}
```

**Usage:**
```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(EustressOrbitalPlugin {
        max_tracking_range: 50_000_000.0,
        max_tracked_objects: 500,
        threat_time_horizon: 43200.0,
        enable_sam3d: true,
    })
    .run();
```

---

## System Sets

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum OrbitalUpdateSet {
    /// Coordinate and position updates
    CoordinateUpdate,
    /// Navigation array and threat updates
    NavigationUpdate,
    /// Physics integration (FixedUpdate)
    PhysicsUpdate,
    /// Post-physics adjustments
    PostPhysics,
}
```

**Execution Order:**
```
FixedUpdate:
  PhysicsUpdate

Update:
  CoordinateUpdate → NavigationUpdate → PostPhysics
```

---

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Coordinate transformation failed: {0}")]
    CoordinateError(String),

    #[error("Propagation failed: {0}")]
    PropagationError(String),

    #[error("TLE parse error: {0}")]
    TleParseError(String),

    #[error("SAM 3D error: {0}")]
    Sam3dError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("SGP4 error: {0}")]
    Sgp4Error(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

---

## Feature Flags

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `bevy_integration` | Bevy ECS integration (default) | `bevy` |
| `python_bindings` | PyO3 Python bindings | `pyo3` |
| `sam3d` | SAM 3D inference support | `ort` |

---

## See Also

- [01_OVERVIEW.md](./01_OVERVIEW.md) - System overview
- [09_IMPLEMENTATION_GUIDE.md](./09_IMPLEMENTATION_GUIDE.md) - Implementation details
- [Bevy Documentation](https://bevyengine.org/learn/book/introduction/)
- [big_space Documentation](https://docs.rs/big_space)
