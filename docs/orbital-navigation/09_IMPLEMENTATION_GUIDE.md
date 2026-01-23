# 09 - Implementation Guide

> Complete Rust/Bevy implementation details for the Eustress Orbital Navigation System

## Project Structure

```
eustress-orbital/
├── Cargo.toml
├── src/
│   ├── lib.rs                 # Crate root, re-exports
│   ├── prelude.rs             # Common imports
│   ├── plugin.rs              # Main Bevy plugin
│   │
│   ├── coords/                # Coordinate systems
│   │   ├── mod.rs
│   │   ├── geodetic.rs        # WGS84 geodetic
│   │   ├── ecef.rs            # Earth-centered Earth-fixed
│   │   ├── regions.rs         # Relative Euclidean regions
│   │   └── transforms.rs      # Coordinate transformations
│   │
│   ├── orbital/               # Orbital mechanics
│   │   ├── mod.rs
│   │   ├── elements.rs        # Orbital elements
│   │   ├── propagation.rs     # Kepler & SGP4
│   │   ├── perturbations.rs   # J2, third-body
│   │   └── maneuvers.rs       # Delta-V planning
│   │
│   ├── navigation/            # Navigation system
│   │   ├── mod.rs
│   │   ├── arrays.rs          # Nearby objects, threats
│   │   ├── mainframe.rs       # Central navigation state
│   │   ├── alerts.rs          # Alert management
│   │   └── display.rs         # HUD data
│   │
│   ├── objects/               # Dynamic objects
│   │   ├── mod.rs
│   │   ├── satellites.rs      # TLE catalog, SGP4
│   │   ├── celestial.rs       # Planets, stars
│   │   ├── debris.rs          # Space debris
│   │   └── sam3d.rs           # SAM 3D models
│   │
│   ├── telescope/             # Telescope processing
│   │   ├── mod.rs
│   │   ├── lens.rs            # Lens geometry
│   │   ├── calibration.rs     # Astrometric calibration
│   │   └── inference.rs       # SAM 3D inference
│   │
│   └── physics/               # Physics foundations
│       ├── mod.rs
│       ├── constants.rs       # Physical constants
│       ├── gravity.rs         # Gravitational models
│       └── relativity.rs      # Relativistic corrections
│
├── examples/
│   ├── basic_orbit.rs         # Simple orbital demo
│   ├── satellite_tracking.rs  # TLE-based tracking
│   └── navigation_demo.rs     # Full navigation system
│
└── assets/
    ├── tle/                   # TLE data files
    └── catalogs/              # Star catalogs
```

## Cargo.toml

```toml
[package]
name = "eustress-orbital"
version = "0.1.0"
edition = "2021"
description = "Orbital navigation system for Eustress Engine"
license = "MIT OR Apache-2.0"

[features]
default = ["bevy_integration"]
bevy_integration = ["bevy"]
python_bindings = ["pyo3"]
sam3d = ["ort"]

[dependencies]
# Core math
glam = { version = "0.29", features = ["serde"] }
nalgebra = { version = "0.33", features = ["serde-serialize"] }

# Bevy integration (optional)
bevy = { version = "0.15", optional = true, default-features = false, features = [
    "bevy_asset",
    "bevy_render",
    "bevy_pbr",
    "bevy_core_pipeline",
    "bevy_winit",
    "x11",
] }

# Floating origin
big_space = "0.7"

# Coordinate transformations
proj = "0.6"

# Orbital propagation
sgp4 = "0.18"

# Ephemerides (choose one)
pracstro = { version = "0.3", optional = true }
# nyx-space = { version = "2.0", optional = true }

# Spatial indexing
rstar = "0.12"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Error handling
thiserror = "2.0"

# Async runtime (for TLE fetching)
tokio = { version = "1.0", features = ["rt-multi-thread", "macros"], optional = true }

# SAM 3D inference (optional)
ort = { version = "2.0", features = ["load-dynamic"], optional = true }

# Python bindings (optional)
pyo3 = { version = "0.22", features = ["extension-module"], optional = true }

[dev-dependencies]
criterion = "0.5"
approx = "0.5"

[[bench]]
name = "propagation"
harness = false

[[example]]
name = "basic_orbit"
required-features = ["bevy_integration"]

[[example]]
name = "satellite_tracking"
required-features = ["bevy_integration"]
```

## Core Module: lib.rs

```rust
//! Eustress Orbital Navigation System
//!
//! A comprehensive framework for spaceship-centric travel, Earth-centric orbital grids,
//! and real-time celestial navigation.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod coords;
pub mod navigation;
pub mod objects;
pub mod orbital;
pub mod physics;
pub mod telescope;

#[cfg(feature = "bevy_integration")]
pub mod plugin;

pub mod prelude;

pub use prelude::*;

/// Result type for orbital operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for orbital operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Coordinate transformation error
    #[error("Coordinate transformation failed: {0}")]
    CoordinateError(String),

    /// Orbital propagation error
    #[error("Propagation failed: {0}")]
    PropagationError(String),

    /// TLE parsing error
    #[error("TLE parse error: {0}")]
    TleParseError(String),

    /// SAM 3D inference error
    #[error("SAM 3D error: {0}")]
    Sam3dError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// SGP4 error
    #[error("SGP4 error: {0}")]
    Sgp4Error(String),
}
```

## Prelude Module

```rust
//! Common imports for eustress-orbital

pub use crate::coords::{
    ecef_to_geodetic, geodetic_to_ecef, Geodetic, RegionId, RegionRegistry,
};

pub use crate::navigation::{
    AlertManager, AlertSeverity, AlertType, CollisionThreat, NavigationAlert,
    NavigationDisplay, NavigationState, NearbyObject, NearbyObjectsArray,
    TargetList, ThreatLevel, ThreatQueue,
};

pub use crate::objects::{
    CelestialBody, DebrisCatalog, DebrisObject, ObjectType, OrbitalObject,
    TleCatalog, TwoLineElement,
};

pub use crate::orbital::{
    elements_to_state, solve_kepler, state_to_elements, OrbitalElements,
    OrbitalManeuver, ManeuverType,
};

pub use crate::physics::constants::*;

#[cfg(feature = "bevy_integration")]
pub use crate::plugin::EustressOrbitalPlugin;

#[cfg(feature = "bevy_integration")]
pub use bevy::prelude::*;

#[cfg(feature = "bevy_integration")]
pub use big_space::FloatingOrigin;

pub use glam::DVec3;
```

## Main Plugin

```rust
//! Bevy plugin for orbital navigation

use bevy::prelude::*;
use big_space::BigSpacePlugin;

use crate::coords::RegionRegistry;
use crate::navigation::*;
use crate::objects::*;
use crate::orbital::Sgp4Propagator;

/// Main plugin for the Eustress Orbital Navigation System
pub struct EustressOrbitalPlugin {
    /// Maximum tracking range in meters
    pub max_tracking_range: f64,
    /// Maximum objects to track
    pub max_tracked_objects: usize,
    /// Threat detection time horizon in seconds
    pub threat_time_horizon: f64,
    /// Enable SAM 3D processing
    pub enable_sam3d: bool,
}

impl Default for EustressOrbitalPlugin {
    fn default() -> Self {
        Self {
            max_tracking_range: 100_000_000.0, // 100,000 km
            max_tracked_objects: 1000,
            threat_time_horizon: 86400.0, // 24 hours
            enable_sam3d: false,
        }
    }
}

impl Plugin for EustressOrbitalPlugin {
    fn build(&self, app: &mut App) {
        // Add big_space for floating origin
        app.add_plugins(BigSpacePlugin::<i128>::default());

        // Core resources
        app.insert_resource(RegionRegistry::new(25, 1_000_000.0))
            .insert_resource(NavigationState::default())
            .insert_resource(NearbyObjectsArray {
                objects: Vec::new(),
                max_range: self.max_tracking_range,
                max_objects: self.max_tracked_objects,
            })
            .insert_resource(ThreatQueue {
                threats: std::collections::BinaryHeap::new(),
                threat_threshold: 10_000.0,
                time_horizon: self.threat_time_horizon,
            })
            .insert_resource(TargetList::default())
            .insert_resource(AlertManager {
                active_alerts: Vec::new(),
                alert_history: Vec::new(),
                max_history: 100,
            })
            .insert_resource(NavigationDisplay::default());

        // Object tracking resources
        app.insert_resource(Sgp4Propagator::new())
            .insert_resource(EphemerisEngine::new())
            .insert_resource(TleCatalog::default())
            .insert_resource(DebrisCatalog::default());

        // Register components
        app.register_type::<OrbitalCoords>()
            .register_type::<SpaceshipMarker>()
            .register_type::<GravityAligned>();

        // Core systems
        app.add_systems(
            Update,
            (
                update_floating_origin,
                update_orbital_objects,
                recompute_local_positions,
                sync_transforms,
            )
                .chain()
                .in_set(OrbitalUpdateSet::CoordinateUpdate),
        );

        // Navigation systems
        app.add_systems(
            Update,
            (
                update_nearby_objects,
                update_threat_queue,
                generate_collision_alerts,
                update_alert_manager,
            )
                .chain()
                .in_set(OrbitalUpdateSet::NavigationUpdate)
                .after(OrbitalUpdateSet::CoordinateUpdate),
        );

        // Physics systems
        app.add_systems(
            FixedUpdate,
            (update_spaceship_physics, execute_maneuvers)
                .chain()
                .in_set(OrbitalUpdateSet::PhysicsUpdate),
        );

        // Gravity alignment (optional, runs after physics)
        app.add_systems(
            Update,
            align_to_gravity
                .in_set(OrbitalUpdateSet::PostPhysics)
                .after(OrbitalUpdateSet::PhysicsUpdate),
        );

        // Configure system sets
        app.configure_sets(
            Update,
            (
                OrbitalUpdateSet::CoordinateUpdate,
                OrbitalUpdateSet::NavigationUpdate,
                OrbitalUpdateSet::PostPhysics,
            )
                .chain(),
        );

        app.configure_sets(
            FixedUpdate,
            OrbitalUpdateSet::PhysicsUpdate,
        );
    }
}

/// System sets for orbital navigation
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum OrbitalUpdateSet {
    /// Coordinate and position updates
    CoordinateUpdate,
    /// Navigation array and threat updates
    NavigationUpdate,
    /// Physics integration
    PhysicsUpdate,
    /// Post-physics adjustments
    PostPhysics,
}

/// Marker component for the player's spaceship
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct SpaceshipMarker;

/// High-precision orbital coordinates
#[derive(Component, Reflect, Clone, Copy, Debug)]
#[reflect(Component)]
pub struct OrbitalCoords {
    /// Global position in ECEF (f64 precision)
    #[reflect(ignore)]
    pub global_ecef: DVec3,
    /// Current region for local calculations
    pub region: RegionId,
    /// Local position within region (f32, relative to floating origin)
    pub local_pos: Vec3,
    /// Local velocity (m/s)
    pub local_vel: Vec3,
    /// Whether to sync Transform from OrbitalCoords
    pub sync_transform: bool,
}

impl Default for OrbitalCoords {
    fn default() -> Self {
        Self {
            global_ecef: DVec3::new(R_EARTH + 400_000.0, 0.0, 0.0),
            region: RegionId::default(),
            local_pos: Vec3::ZERO,
            local_vel: Vec3::ZERO,
            sync_transform: true,
        }
    }
}

impl OrbitalCoords {
    /// Create from geodetic coordinates (lat/lon/alt)
    pub fn from_geodetic(lat_deg: f64, lon_deg: f64, alt_m: f64) -> Self {
        let geo = Geodetic::from_degrees(lon_deg, lat_deg, alt_m);
        let ecef = geodetic_to_ecef(&geo);

        Self {
            global_ecef: ecef,
            region: RegionId::from_ecef(ecef, 16),
            local_pos: Vec3::ZERO,
            local_vel: Vec3::ZERO,
            sync_transform: true,
        }
    }

    /// Create from ECEF coordinates
    pub fn from_ecef(ecef: DVec3) -> Self {
        Self {
            global_ecef: ecef,
            region: RegionId::from_ecef(ecef, 16),
            local_pos: Vec3::ZERO,
            local_vel: Vec3::ZERO,
            sync_transform: true,
        }
    }

    /// Get velocity in ECEF frame
    pub fn velocity_ecef(&self) -> DVec3 {
        self.local_vel.as_dvec3()
    }
}

/// Gravity alignment component
#[derive(Component, Reflect, Clone, Copy, Debug)]
#[reflect(Component)]
pub struct GravityAligned {
    /// Smoothing factor (higher = faster alignment)
    pub smoothing: f32,
}

impl Default for GravityAligned {
    fn default() -> Self {
        Self { smoothing: 2.0 }
    }
}

impl GravityAligned {
    /// Create with custom smoothing factor
    pub fn smooth(factor: f32) -> Self {
        Self { smoothing: factor }
    }
}

/// Spaceship physics component
#[derive(Component, Clone, Debug)]
pub struct SpaceshipPhysics {
    /// Velocity in local frame (m/s)
    pub velocity: Vec3,
    /// Angular velocity (rad/s)
    pub angular_velocity: Vec3,
    /// Mass (kg)
    pub mass: f64,
    /// Thrust vector in local frame (N)
    pub thrust: Vec3,
}

impl Default for SpaceshipPhysics {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            mass: 10_000.0,
            thrust: Vec3::ZERO,
        }
    }
}
```

## Example: Basic Orbit

```rust
//! Basic orbital demonstration
//!
//! Run with: cargo run --example basic_orbit

use bevy::prelude::*;
use eustress_orbital::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EustressOrbitalPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(Update, print_orbital_info)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Spawn spaceship at ISS-like orbit
    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(10.0, 4.0, 2.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.8, 0.9),
            metallic: 0.9,
            ..default()
        })),
        Transform::default(),
        OrbitalCoords::from_geodetic(28.5, -80.6, 400_000.0),
        SpaceshipPhysics {
            velocity: Vec3::new(7660.0, 0.0, 0.0),
            ..default()
        },
        FloatingOrigin,
        SpaceshipMarker,
        GravityAligned::smooth(2.0),
        Name::new("Spaceship"),
    ));

    // Spawn Earth (simplified)
    commands.spawn((
        Mesh3d(meshes.add(Mesh::from(Sphere::new(6_378_137.0 / 1000.0)))), // Scaled
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.4, 0.8),
            ..default()
        })),
        Transform::default(),
        OrbitalCoords::from_ecef(DVec3::ZERO),
        Name::new("Earth"),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 50.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_xyz(1.0, 1.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn print_orbital_info(
    time: Res<Time>,
    query: Query<(&OrbitalCoords, &Name), With<SpaceshipMarker>>,
) {
    // Print every 5 seconds
    if (time.elapsed_secs() % 5.0) > 0.1 {
        return;
    }

    for (coords, name) in &query {
        let geo = ecef_to_geodetic(coords.global_ecef);
        let altitude_km = geo.altitude / 1000.0;
        let velocity = coords.local_vel.length();

        println!(
            "{}: Alt={:.1}km, Vel={:.1}m/s, Lat={:.2}°, Lon={:.2}°",
            name,
            altitude_km,
            velocity,
            geo.latitude.to_degrees(),
            geo.longitude.to_degrees(),
        );
    }
}
```

## Example: Satellite Tracking

```rust
//! Satellite tracking demonstration
//!
//! Run with: cargo run --example satellite_tracking

use bevy::prelude::*;
use eustress_orbital::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EustressOrbitalPlugin::default())
        .add_systems(Startup, (setup_scene, load_tle_catalog))
        .add_systems(Update, (spawn_satellites, update_satellite_display))
        .run();
}

fn setup_scene(mut commands: Commands) {
    // Spaceship
    commands.spawn((
        Transform::default(),
        OrbitalCoords::from_geodetic(0.0, 0.0, 400_000.0),
        SpaceshipPhysics::default(),
        FloatingOrigin,
        SpaceshipMarker,
        Name::new("Observer"),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 1000.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn load_tle_catalog(mut catalog: ResMut<TleCatalog>) {
    // Sample TLE for ISS
    let iss_tle = r#"ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  10270-3 0  9025
2 25544  51.6400 208.9163 0006703 276.4853  83.5505 15.49815571 20000"#;

    if let Ok(parsed) = TleCatalog::parse_tle_file(iss_tle) {
        *catalog = parsed;
        println!("Loaded {} TLEs", catalog.tles.len());
    }
}

fn spawn_satellites(
    mut commands: Commands,
    catalog: Res<TleCatalog>,
    propagator: Res<Sgp4Propagator>,
    time: Res<Time>,
    existing: Query<Entity, With<OrbitalObject>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Only spawn once
    if !existing.is_empty() {
        return;
    }

    let jd = 2451545.0 + time.elapsed_secs_f64() / 86400.0;

    for tle in catalog.tles.values() {
        let Ok(state) = propagator.propagate(tle, jd) else {
            continue;
        };

        commands.spawn((
            Mesh3d(meshes.add(Mesh::from(Sphere::new(50.0)))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.8, 0.2),
                emissive: LinearRgba::rgb(0.5, 0.4, 0.1),
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

        println!("Spawned satellite: {}", tle.name);
    }
}

fn update_satellite_display(
    nearby: Res<NearbyObjectsArray>,
    time: Res<Time>,
) {
    if (time.elapsed_secs() % 2.0) > 0.1 {
        return;
    }

    println!("\n=== Nearby Objects ===");
    for obj in nearby.objects.iter().take(5) {
        println!(
            "  {} - {:.1}km, Threat: {:?}",
            obj.name.as_deref().unwrap_or("Unknown"),
            obj.distance / 1000.0,
            obj.threat_level,
        );
    }
}
```

## Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_geodetic_ecef_roundtrip() {
        let original = Geodetic::from_degrees(-122.4194, 37.7749, 100.0);
        let ecef = geodetic_to_ecef(&original);
        let recovered = ecef_to_geodetic(ecef);

        assert_relative_eq!(original.longitude, recovered.longitude, epsilon = 1e-10);
        assert_relative_eq!(original.latitude, recovered.latitude, epsilon = 1e-10);
        assert_relative_eq!(original.altitude, recovered.altitude, epsilon = 1e-6);
    }

    #[test]
    fn test_orbital_elements_roundtrip() {
        let elements = OrbitalElements {
            semi_major_axis: 7_000_000.0,
            eccentricity: 0.001,
            inclination: 0.9,
            raan: 1.5,
            arg_periapsis: 2.0,
            true_anomaly: 0.5,
            mean_anomaly_epoch: 0.0,
            epoch: 0.0,
        };

        let (pos, vel) = elements_to_state(&elements, elements.true_anomaly);
        let recovered = state_to_elements(pos, vel);

        assert_relative_eq!(elements.semi_major_axis, recovered.semi_major_axis, epsilon = 1.0);
        assert_relative_eq!(elements.eccentricity, recovered.eccentricity, epsilon = 1e-6);
        assert_relative_eq!(elements.inclination, recovered.inclination, epsilon = 1e-6);
    }

    #[test]
    fn test_kepler_equation() {
        let mean_anomaly = 1.0;
        let eccentricity = 0.5;

        let eccentric = solve_kepler(mean_anomaly, eccentricity, 1e-12);

        // Verify: M = E - e*sin(E)
        let computed_mean = eccentric - eccentricity * eccentric.sin();
        assert_relative_eq!(mean_anomaly, computed_mean, epsilon = 1e-10);
    }

    #[test]
    fn test_orbital_period() {
        // ISS-like orbit
        let elements = OrbitalElements {
            semi_major_axis: 6_778_000.0, // ~400km altitude
            eccentricity: 0.0,
            ..Default::default()
        };

        let period_min = elements.period() / 60.0;

        // ISS period is ~92 minutes
        assert!(period_min > 90.0 && period_min < 95.0);
    }
}
```

## Benchmarks

```rust
//! Benchmarks for orbital propagation
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use eustress_orbital::prelude::*;

fn bench_kepler_solve(c: &mut Criterion) {
    c.bench_function("solve_kepler", |b| {
        b.iter(|| {
            solve_kepler(black_box(2.5), black_box(0.7), 1e-12)
        })
    });
}

fn bench_coordinate_transform(c: &mut Criterion) {
    let geo = Geodetic::from_degrees(-122.4194, 37.7749, 100.0);

    c.bench_function("geodetic_to_ecef", |b| {
        b.iter(|| geodetic_to_ecef(black_box(&geo)))
    });

    let ecef = geodetic_to_ecef(&geo);

    c.bench_function("ecef_to_geodetic", |b| {
        b.iter(|| ecef_to_geodetic(black_box(ecef)))
    });
}

fn bench_elements_conversion(c: &mut Criterion) {
    let elements = OrbitalElements {
        semi_major_axis: 7_000_000.0,
        eccentricity: 0.1,
        inclination: 0.9,
        raan: 1.5,
        arg_periapsis: 2.0,
        true_anomaly: 0.5,
        mean_anomaly_epoch: 0.0,
        epoch: 0.0,
    };

    c.bench_function("elements_to_state", |b| {
        b.iter(|| elements_to_state(black_box(&elements), black_box(0.5)))
    });

    let (pos, vel) = elements_to_state(&elements, 0.5);

    c.bench_function("state_to_elements", |b| {
        b.iter(|| state_to_elements(black_box(pos), black_box(vel)))
    });
}

criterion_group!(
    benches,
    bench_kepler_solve,
    bench_coordinate_transform,
    bench_elements_conversion,
);
criterion_main!(benches);
```

## Build and Run

```powershell
# Build the library
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo bench

# Run basic example
cargo run --example basic_orbit --release

# Run with all features
cargo run --example satellite_tracking --release --features "sam3d"

# Build documentation
cargo doc --open
```

## Next Steps

- [10_API_REFERENCE.md](./10_API_REFERENCE.md) - Complete API documentation
