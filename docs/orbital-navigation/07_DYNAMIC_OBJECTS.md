# 07 - Dynamic Objects

> Tracking satellites, planets, stars, and debris in real-time

## Overview

Dynamic objects are celestial bodies and artificial objects that move relative to the observer. This document covers the tracking systems for satellites (via TLE/SGP4), planets and moons (via ephemerides), stars (via catalogs), and debris/obstacles.

## Object Categories

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        DYNAMIC OBJECT TYPES                             │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  ARTIFICIAL OBJECTS                                             │   │
│  │  ├── Active Satellites (TLE + SGP4)                             │   │
│  │  │   ├── GEO Communications                                     │   │
│  │  │   ├── LEO Constellations (Starlink, etc.)                    │   │
│  │  │   └── Navigation (GPS, Galileo, GLONASS)                     │   │
│  │  ├── Space Stations (TLE + SGP4)                                │   │
│  │  ├── Debris (TLE + SGP4 or Radar)                               │   │
│  │  └── Spacecraft (Direct tracking)                               │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │  NATURAL OBJECTS                                                │   │
│  │  ├── Planets (Ephemerides - pracstro/nyx)                       │   │
│  │  │   ├── Inner: Mercury, Venus, Mars                            │   │
│  │  │   └── Outer: Jupiter, Saturn, Uranus, Neptune                │   │
│  │  ├── Moons (Ephemerides)                                        │   │
│  │  │   ├── Earth's Moon                                           │   │
│  │  │   └── Major moons of other planets                           │   │
│  │  ├── Sun (Ephemerides)                                          │   │
│  │  ├── Asteroids (Ephemerides for major, TLE-like for NEOs)       │   │
│  │  └── Stars (Catalog - fixed direction, parallax negligible)     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Satellite Tracking (SGP4)

### TLE Data Structure

```rust
/// Two-Line Element set for satellite tracking
#[derive(Clone, Debug)]
pub struct TwoLineElement {
    /// Satellite name (line 0)
    pub name: String,
    /// NORAD catalog number
    pub norad_id: u32,
    /// International designator
    pub intl_designator: String,
    /// Epoch (Julian Date)
    pub epoch: f64,
    /// Mean motion derivative (rev/day²)
    pub mean_motion_dot: f64,
    /// Mean motion second derivative (rev/day³)
    pub mean_motion_ddot: f64,
    /// BSTAR drag term
    pub bstar: f64,
    /// Inclination (radians)
    pub inclination: f64,
    /// Right ascension of ascending node (radians)
    pub raan: f64,
    /// Eccentricity
    pub eccentricity: f64,
    /// Argument of perigee (radians)
    pub arg_perigee: f64,
    /// Mean anomaly (radians)
    pub mean_anomaly: f64,
    /// Mean motion (rev/day)
    pub mean_motion: f64,
    /// Revolution number at epoch
    pub rev_number: u32,
}

impl TwoLineElement {
    /// Parse from standard TLE format
    pub fn parse(line0: &str, line1: &str, line2: &str) -> Result<Self, TleParseError> {
        let name = line0.trim().to_string();
        
        // Line 1 parsing
        let norad_id: u32 = line1[2..7].trim().parse()?;
        let intl_designator = line1[9..17].trim().to_string();
        
        let epoch_year: u32 = line1[18..20].trim().parse()?;
        let epoch_day: f64 = line1[20..32].trim().parse()?;
        let full_year = if epoch_year < 57 { 2000 + epoch_year } else { 1900 + epoch_year };
        let epoch = julian_date_from_year_day(full_year, epoch_day);
        
        let mean_motion_dot: f64 = line1[33..43].trim().parse()?;
        let mean_motion_ddot = parse_tle_exponent(&line1[44..52])?;
        let bstar = parse_tle_exponent(&line1[53..61])?;
        
        // Line 2 parsing
        let inclination: f64 = line2[8..16].trim().parse::<f64>()?.to_radians();
        let raan: f64 = line2[17..25].trim().parse::<f64>()?.to_radians();
        let eccentricity: f64 = format!("0.{}", line2[26..33].trim()).parse()?;
        let arg_perigee: f64 = line2[34..42].trim().parse::<f64>()?.to_radians();
        let mean_anomaly: f64 = line2[43..51].trim().parse::<f64>()?.to_radians();
        let mean_motion: f64 = line2[52..63].trim().parse()?;
        let rev_number: u32 = line2[63..68].trim().parse().unwrap_or(0);
        
        Ok(Self {
            name,
            norad_id,
            intl_designator,
            epoch,
            mean_motion_dot,
            mean_motion_ddot,
            bstar,
            inclination,
            raan,
            eccentricity,
            arg_perigee,
            mean_anomaly,
            mean_motion,
            rev_number,
        })
    }
}

fn parse_tle_exponent(s: &str) -> Result<f64, TleParseError> {
    let s = s.trim();
    if s.is_empty() || s == "00000-0" {
        return Ok(0.0);
    }
    
    // Format: ±NNNNN±E where mantissa is 0.NNNNN and E is exponent
    let sign = if s.starts_with('-') { -1.0 } else { 1.0 };
    let s = s.trim_start_matches(['+', '-', ' ']);
    
    if let Some(exp_pos) = s.rfind(['+', '-']) {
        let mantissa: f64 = format!("0.{}", &s[..exp_pos]).parse()?;
        let exponent: i32 = s[exp_pos..].parse()?;
        Ok(sign * mantissa * 10f64.powi(exponent))
    } else {
        Ok(0.0)
    }
}
```

### SGP4 Propagation

```rust
use sgp4::{Constants, Elements, Prediction};

/// SGP4 propagator wrapper
pub struct Sgp4Propagator {
    constants: Constants,
}

impl Sgp4Propagator {
    pub fn new() -> Self {
        Self {
            constants: Constants::from_elements_afspc_compatibility_mode(&Elements::default())
                .unwrap_or_else(|_| Constants::default()),
        }
    }
    
    /// Propagate TLE to given Julian Date
    pub fn propagate(&self, tle: &TwoLineElement, julian_date: f64) -> Result<SatelliteState, Sgp4Error> {
        // Convert TLE to sgp4 Elements
        let elements = Elements {
            object_name: Some(tle.name.clone()),
            international_designator: Some(tle.intl_designator.clone()),
            norad_id: tle.norad_id,
            classification: sgp4::Classification::Unclassified,
            datetime: sgp4::iau_epoch_to_datetime(tle.epoch)?,
            mean_motion_dot: tle.mean_motion_dot,
            mean_motion_ddot: tle.mean_motion_ddot,
            drag_term: tle.bstar,
            element_set_number: 0,
            inclination: tle.inclination.to_degrees(),
            right_ascension: tle.raan.to_degrees(),
            eccentricity: tle.eccentricity,
            argument_of_perigee: tle.arg_perigee.to_degrees(),
            mean_anomaly: tle.mean_anomaly.to_degrees(),
            mean_motion: tle.mean_motion,
            revolution_number: tle.rev_number,
            ephemeris_type: 0,
        };
        
        let constants = Constants::from_elements(&elements)?;
        
        // Minutes since epoch
        let minutes_since_epoch = (julian_date - tle.epoch) * 1440.0;
        
        let prediction = constants.propagate(minutes_since_epoch)?;
        
        // Convert TEME to ECEF
        let (pos_ecef, vel_ecef) = teme_to_ecef(
            DVec3::new(prediction.position[0], prediction.position[1], prediction.position[2]) * 1000.0, // km to m
            DVec3::new(prediction.velocity[0], prediction.velocity[1], prediction.velocity[2]) * 1000.0,
            julian_date,
        );
        
        Ok(SatelliteState {
            position_ecef: pos_ecef,
            velocity_ecef: vel_ecef,
            julian_date,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SatelliteState {
    pub position_ecef: DVec3,
    pub velocity_ecef: DVec3,
    pub julian_date: f64,
}

/// Convert TEME (True Equator Mean Equinox) to ECEF
fn teme_to_ecef(pos_teme: DVec3, vel_teme: DVec3, jd: f64) -> (DVec3, DVec3) {
    // Greenwich Mean Sidereal Time
    let gmst = greenwich_mean_sidereal_time(jd);
    
    let cos_gmst = gmst.cos();
    let sin_gmst = gmst.sin();
    
    // Rotation matrix TEME -> ECEF (simplified, ignoring polar motion)
    let pos_ecef = DVec3::new(
        cos_gmst * pos_teme.x + sin_gmst * pos_teme.y,
        -sin_gmst * pos_teme.x + cos_gmst * pos_teme.y,
        pos_teme.z,
    );
    
    // Earth rotation rate (rad/s)
    let omega_earth = 7.2921158553e-5;
    
    // Velocity includes Earth rotation
    let vel_ecef = DVec3::new(
        cos_gmst * vel_teme.x + sin_gmst * vel_teme.y + omega_earth * pos_ecef.y,
        -sin_gmst * vel_teme.x + cos_gmst * vel_teme.y - omega_earth * pos_ecef.x,
        vel_teme.z,
    );
    
    (pos_ecef, vel_ecef)
}

fn greenwich_mean_sidereal_time(jd: f64) -> f64 {
    let t = (jd - 2451545.0) / 36525.0;
    
    // GMST in seconds
    let gmst_sec = 67310.54841 
        + (876600.0 * 3600.0 + 8640184.812866) * t 
        + 0.093104 * t * t 
        - 6.2e-6 * t * t * t;
    
    // Convert to radians
    (gmst_sec % 86400.0) / 86400.0 * 2.0 * std::f64::consts::PI
}
```

### TLE Catalog Management

```rust
/// TLE catalog resource
#[derive(Resource)]
pub struct TleCatalog {
    /// All TLEs indexed by NORAD ID
    pub tles: std::collections::HashMap<u32, TwoLineElement>,
    /// Last update timestamp
    pub last_update: f64,
    /// Update source URL
    pub source_url: String,
}

impl TleCatalog {
    /// Load TLEs from file
    pub fn load_from_file(path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        Self::parse_tle_file(&content)
    }
    
    /// Parse TLE file content
    pub fn parse_tle_file(content: &str) -> Result<Self, std::io::Error> {
        let mut tles = std::collections::HashMap::new();
        let lines: Vec<&str> = content.lines().collect();
        
        let mut i = 0;
        while i + 2 < lines.len() {
            let line0 = lines[i];
            let line1 = lines[i + 1];
            let line2 = lines[i + 2];
            
            // Check if this is a valid TLE set
            if line1.starts_with('1') && line2.starts_with('2') {
                if let Ok(tle) = TwoLineElement::parse(line0, line1, line2) {
                    tles.insert(tle.norad_id, tle);
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        
        Ok(Self {
            tles,
            last_update: 0.0,
            source_url: String::new(),
        })
    }
    
    /// Get satellites in a specific orbital regime
    pub fn get_by_regime(&self, regime: OrbitalRegime) -> Vec<&TwoLineElement> {
        self.tles.values().filter(|tle| {
            let period_min = 1440.0 / tle.mean_motion; // minutes
            let altitude_km = estimate_altitude_from_period(period_min);
            
            match regime {
                OrbitalRegime::LEO => altitude_km < 2000.0,
                OrbitalRegime::MEO => altitude_km >= 2000.0 && altitude_km < 35000.0,
                OrbitalRegime::GEO => altitude_km >= 35000.0 && altitude_km < 37000.0,
                OrbitalRegime::HEO => tle.eccentricity > 0.25,
            }
        }).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitalRegime {
    LEO,
    MEO,
    GEO,
    HEO,
}

fn estimate_altitude_from_period(period_min: f64) -> f64 {
    // Kepler's third law: T² ∝ a³
    // a = (T² * GM / 4π²)^(1/3)
    let period_sec = period_min * 60.0;
    let a = (period_sec * period_sec * GM_EARTH / (4.0 * std::f64::consts::PI * std::f64::consts::PI)).powf(1.0/3.0);
    (a - EARTH_RADIUS_M) / 1000.0 // km
}
```

## Planetary Ephemerides

### Ephemeris Engine

```rust
/// Planetary body identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// Ephemeris calculation engine
pub struct EphemerisEngine {
    // In production, this would use pracstro or nyx-space
    // Simplified analytical approximations here
}

impl EphemerisEngine {
    pub fn new() -> Self {
        Self {}
    }
    
    /// Get position of body at Julian Date (ICRS, meters from solar system barycenter)
    pub fn position(&self, body: CelestialBody, jd: f64) -> DVec3 {
        match body {
            CelestialBody::Sun => self.sun_position(jd),
            CelestialBody::Moon => self.moon_position(jd),
            CelestialBody::Earth => self.earth_position(jd),
            _ => self.planet_position(body, jd),
        }
    }
    
    /// Get position relative to Earth (GCRS, meters)
    pub fn position_geocentric(&self, body: CelestialBody, jd: f64) -> DVec3 {
        let body_pos = self.position(body, jd);
        let earth_pos = self.position(CelestialBody::Earth, jd);
        body_pos - earth_pos
    }
    
    /// Get position in ECEF (for rendering)
    pub fn position_ecef(&self, body: CelestialBody, jd: f64) -> DVec3 {
        let gcrs = self.position_geocentric(body, jd);
        gcrs_to_ecef(gcrs, jd)
    }
    
    fn sun_position(&self, jd: f64) -> DVec3 {
        // Simplified: Sun at origin of heliocentric system
        // For barycentric, would need full planetary perturbations
        DVec3::ZERO
    }
    
    fn earth_position(&self, jd: f64) -> DVec3 {
        // Simplified Earth orbit (circular approximation)
        let t = (jd - 2451545.0) / 365.25; // Years since J2000
        let mean_longitude = (280.46 + 360.0 * t).to_radians();
        
        let au = 149_597_870_700.0; // meters
        DVec3::new(
            au * mean_longitude.cos(),
            au * mean_longitude.sin(),
            0.0,
        )
    }
    
    fn moon_position(&self, jd: f64) -> DVec3 {
        // Simplified Moon position (circular orbit around Earth)
        let t = (jd - 2451545.0); // Days since J2000
        let mean_longitude = (218.32 + 13.176396 * t).to_radians();
        let inclination = 5.145_f64.to_radians();
        
        let lunar_distance = 384_400_000.0; // meters
        
        let earth_pos = self.earth_position(jd);
        
        // Moon position relative to Earth
        let moon_rel = DVec3::new(
            lunar_distance * mean_longitude.cos(),
            lunar_distance * mean_longitude.sin() * inclination.cos(),
            lunar_distance * mean_longitude.sin() * inclination.sin(),
        );
        
        earth_pos + moon_rel
    }
    
    fn planet_position(&self, body: CelestialBody, jd: f64) -> DVec3 {
        // Simplified planetary positions using mean orbital elements
        let (a_au, period_years, lon_j2000) = match body {
            CelestialBody::Mercury => (0.387, 0.241, 252.25),
            CelestialBody::Venus => (0.723, 0.615, 181.98),
            CelestialBody::Mars => (1.524, 1.881, 355.45),
            CelestialBody::Jupiter => (5.203, 11.86, 34.40),
            CelestialBody::Saturn => (9.537, 29.46, 49.94),
            CelestialBody::Uranus => (19.19, 84.01, 313.23),
            CelestialBody::Neptune => (30.07, 164.8, 304.88),
            CelestialBody::Pluto => (39.48, 248.0, 238.93),
            _ => return DVec3::ZERO,
        };
        
        let t = (jd - 2451545.0) / 365.25; // Years since J2000
        let mean_longitude = (lon_j2000 + 360.0 * t / period_years).to_radians();
        
        let au = 149_597_870_700.0;
        let distance = a_au * au;
        
        DVec3::new(
            distance * mean_longitude.cos(),
            distance * mean_longitude.sin(),
            0.0, // Simplified: ignoring inclination
        )
    }
}

fn gcrs_to_ecef(gcrs: DVec3, jd: f64) -> DVec3 {
    let gmst = greenwich_mean_sidereal_time(jd);
    let cos_gmst = gmst.cos();
    let sin_gmst = gmst.sin();
    
    DVec3::new(
        cos_gmst * gcrs.x + sin_gmst * gcrs.y,
        -sin_gmst * gcrs.x + cos_gmst * gcrs.y,
        gcrs.z,
    )
}
```

## Star Tracking

### Star Direction Calculation

```rust
/// Calculate star direction from catalog data
pub fn star_direction_icrs(star: &CatalogStar, jd: f64) -> DVec3 {
    // Apply proper motion
    let years_since_j2000 = (jd - 2451545.0) / 365.25;
    
    let ra = star.ra + (star.pm_ra as f64 / 3600000.0).to_radians() * years_since_j2000;
    let dec = star.dec + (star.pm_dec as f64 / 3600000.0).to_radians() * years_since_j2000;
    
    // Convert to unit vector (ICRS)
    DVec3::new(
        dec.cos() * ra.cos(),
        dec.cos() * ra.sin(),
        dec.sin(),
    )
}

/// Convert star direction to ECEF (for rendering at "infinity")
pub fn star_direction_ecef(star: &CatalogStar, jd: f64) -> DVec3 {
    let icrs = star_direction_icrs(star, jd);
    
    // Apply Earth rotation (ICRS -> GCRS -> ECEF)
    // For stars, ICRS ≈ GCRS (parallax negligible)
    gcrs_to_ecef(icrs, jd)
}

/// Place star at render distance for skybox
pub fn star_render_position(
    star: &CatalogStar,
    jd: f64,
    observer_ecef: DVec3,
    render_distance: f64,
) -> DVec3 {
    let direction = star_direction_ecef(star, jd);
    observer_ecef + direction * render_distance
}
```

## Debris and Obstacle Tracking

### Debris Object

```rust
/// Space debris or obstacle
#[derive(Clone, Debug)]
pub struct DebrisObject {
    /// Unique identifier
    pub id: u64,
    /// TLE if available
    pub tle: Option<TwoLineElement>,
    /// Last known position (if no TLE)
    pub last_position: Option<DVec3>,
    /// Last known velocity (if no TLE)
    pub last_velocity: Option<DVec3>,
    /// Observation timestamp
    pub observation_time: f64,
    /// Estimated size (meters)
    pub size_estimate: f64,
    /// Radar cross-section (m²)
    pub rcs: Option<f64>,
    /// Source of tracking data
    pub source: TrackingSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackingSource {
    /// Space-Track TLE catalog
    SpaceTrack,
    /// Ground radar observation
    Radar,
    /// Optical telescope observation
    Optical,
    /// Ship's own sensors
    ShipSensor,
    /// Predicted from collision/breakup
    Predicted,
}

impl DebrisObject {
    /// Propagate to given time
    pub fn propagate(&self, jd: f64, propagator: &Sgp4Propagator) -> Option<SatelliteState> {
        if let Some(tle) = &self.tle {
            propagator.propagate(tle, jd).ok()
        } else if let (Some(pos), Some(vel)) = (self.last_position, self.last_velocity) {
            // Linear propagation for short-term
            let dt = (jd - self.observation_time) * 86400.0; // seconds
            Some(SatelliteState {
                position_ecef: pos + vel * dt,
                velocity_ecef: vel,
                julian_date: jd,
            })
        } else {
            None
        }
    }
}

/// Debris catalog resource
#[derive(Resource)]
pub struct DebrisCatalog {
    pub objects: std::collections::HashMap<u64, DebrisObject>,
    pub spatial_index: rstar::RTree<DebrisSpatialEntry>,
}

struct DebrisSpatialEntry {
    id: u64,
    position: [f64; 3],
}

impl rstar::RTreeObject for DebrisSpatialEntry {
    type Envelope = rstar::AABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_point(self.position)
    }
}

impl DebrisCatalog {
    /// Query debris near a position
    pub fn query_nearby(&self, ecef: DVec3, radius: f64) -> Vec<&DebrisObject> {
        let min = [ecef.x - radius, ecef.y - radius, ecef.z - radius];
        let max = [ecef.x + radius, ecef.y + radius, ecef.z + radius];
        let envelope = rstar::AABB::from_corners(min, max);
        
        self.spatial_index
            .locate_in_envelope(&envelope)
            .filter_map(|entry| self.objects.get(&entry.id))
            .collect()
    }
}
```

## Bevy Integration

### Orbital Object Component

```rust
/// Component for any orbital object
#[derive(Component)]
pub struct OrbitalObject {
    /// TLE for artificial satellites
    pub tle: Option<TwoLineElement>,
    /// Celestial body for natural objects
    pub body: Option<CelestialBody>,
    /// Object classification
    pub object_type: ObjectType,
    /// SAM 3D model if observed
    pub sam_model: Option<Handle<Mesh>>,
}

/// System to update all orbital objects
fn update_orbital_objects(
    time: Res<Time>,
    propagator: Res<Sgp4Propagator>,
    ephemeris: Res<EphemerisEngine>,
    mut objects: Query<(&mut OrbitalCoords, &OrbitalObject)>,
) {
    // Current Julian Date (simplified - would use proper time system)
    let jd = 2451545.0 + time.elapsed_secs_f64() / 86400.0;
    
    for (mut coords, obj) in &mut objects {
        match (&obj.tle, &obj.body) {
            (Some(tle), _) => {
                // SGP4 propagation for satellites
                if let Ok(state) = propagator.propagate(tle, jd) {
                    coords.global_ecef = state.position_ecef;
                    // Velocity stored separately if needed
                }
            }
            (_, Some(body)) => {
                // Ephemeris for celestial bodies
                coords.global_ecef = ephemeris.position_ecef(*body, jd);
            }
            _ => {
                // No propagation source - object is static or manually updated
            }
        }
    }
}
```

### Spawning Dynamic Objects

```rust
/// Spawn satellites from TLE catalog
fn spawn_satellites(
    mut commands: Commands,
    catalog: Res<TleCatalog>,
    propagator: Res<Sgp4Propagator>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let jd = 2451545.0 + time.elapsed_secs_f64() / 86400.0;
    
    for tle in catalog.tles.values() {
        let Ok(state) = propagator.propagate(tle, jd) else { continue };
        
        commands.spawn((
            Mesh3d(meshes.add(Mesh::from(Sphere::new(10.0)))), // Placeholder mesh
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.8, 0.2),
                emissive: LinearRgba::rgb(0.5, 0.5, 0.1),
                ..default()
            })),
            Transform::default(),
            OrbitalCoords::from_ecef(state.position_ecef),
            OrbitalObject {
                tle: Some(tle.clone()),
                body: None,
                object_type: ObjectType::Satellite,
                sam_model: None,
            },
            Name::new(tle.name.clone()),
        ));
    }
}

/// Spawn celestial bodies
fn spawn_celestial_bodies(
    mut commands: Commands,
    ephemeris: Res<EphemerisEngine>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let jd = 2451545.0 + time.elapsed_secs_f64() / 86400.0;
    
    let bodies = [
        (CelestialBody::Sun, "Sun", 696_340_000.0, Color::srgb(1.0, 0.9, 0.5)),
        (CelestialBody::Moon, "Moon", 1_737_400.0, Color::srgb(0.8, 0.8, 0.8)),
        (CelestialBody::Mars, "Mars", 3_389_500.0, Color::srgb(0.8, 0.4, 0.2)),
        (CelestialBody::Jupiter, "Jupiter", 69_911_000.0, Color::srgb(0.9, 0.8, 0.6)),
        (CelestialBody::Saturn, "Saturn", 58_232_000.0, Color::srgb(0.9, 0.85, 0.6)),
    ];
    
    for (body, name, radius, color) in bodies {
        let pos = ephemeris.position_ecef(body, jd);
        
        // Scale radius for rendering (actual sizes would be invisible at orbital scales)
        let render_radius = (radius / 1000.0).max(1000.0); // At least 1km for visibility
        
        commands.spawn((
            Mesh3d(meshes.add(Mesh::from(Sphere::new(render_radius as f32)))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                emissive: if body == CelestialBody::Sun {
                    LinearRgba::rgb(10.0, 9.0, 5.0)
                } else {
                    LinearRgba::BLACK
                },
                ..default()
            })),
            Transform::default(),
            OrbitalCoords::from_ecef(pos),
            OrbitalObject {
                tle: None,
                body: Some(body),
                object_type: ObjectType::Planet,
                sam_model: None,
            },
            Name::new(name),
        ));
    }
}
```

## Plugin

```rust
pub struct DynamicObjectsPlugin;

impl Plugin for DynamicObjectsPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(Sgp4Propagator::new())
            .insert_resource(EphemerisEngine::new())
            .insert_resource(TleCatalog {
                tles: std::collections::HashMap::new(),
                last_update: 0.0,
                source_url: String::new(),
            })
            .insert_resource(DebrisCatalog {
                objects: std::collections::HashMap::new(),
                spatial_index: rstar::RTree::new(),
            })
            .add_systems(Startup, (spawn_satellites, spawn_celestial_bodies))
            .add_systems(Update, update_orbital_objects);
    }
}
```

## Next Steps

- [08_PHYSICS_FOUNDATIONS.md](./08_PHYSICS_FOUNDATIONS.md) - Orbital mechanics theory
- [09_IMPLEMENTATION_GUIDE.md](./09_IMPLEMENTATION_GUIDE.md) - Full implementation details
