# Eustress Orbital Coordinate Grid

**The definitive breakthrough for Earth One**: a singular, persistent, photoreal digital twin that's geodesically accurate, orbitally realistic, and infinitely extensible with abstract spaces.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Core Types](#core-types)
4. [WGS84/ECEF Coordinates](#wgs84ecef-coordinates)
5. [Relative Euclidean Regions](#relative-euclidean-regions)
6. [Orbital Gravity](#orbital-gravity)
7. [P2P Integration](#p2p-integration)
8. [Scene Format](#scene-format)
9. [Usage Examples](#usage-examples)
10. [Best Practices](#best-practices)

---

## Overview

The Eustress Orbital Coordinate Grid fuses WGS84/ECEF geospatial precision with procedural Euclidean regions, all chunked for P2P streaming and hybrid rendering. It's Cesium-level tiling meets Kerbal Space Program orbital physics, in a Rust-native engine.

### Key Features

- **Global Layer**: WGS84/ECEF backbone with real ellipsoidal Earth (oblate spheroid)
- **Floating Origin**: Each region has its own local Cartesian space (no f32 precision jitter)
- **N-Body Gravity**: Optional Earth + Moon + Sun gravity simulation
- **P2P Streaming**: Regions sync via CRDTs for persistent worlds
- **Abstract Spaces**: Non-Earth dimensions linked to Earth locations

### Why This Matters

| Problem | Solution |
|---------|----------|
| f32 precision breaks at planetary scale | Floating-origin regions with DVec3 global coords |
| Flat-Earth physics assumptions | True spherical gravity from celestial bodies |
| Disconnected Euclidean bubbles | Seamless region transitions with velocity preservation |
| No geospatial compatibility | WGS84/ECEF standard for GIS integration |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    Eustress Orbital Coordinate Grid                          │
├─────────────────────────────────────────────────────────────────────────────┤
│  Global Layer (WGS84/ECEF)                                                  │
│     ├── DVec3 precision (f64) for planetary scale                           │
│     ├── True ellipsoidal Earth (oblate spheroid)                            │
│     └── Orbital mechanics (n-body gravity approximation)                    │
├─────────────────────────────────────────────────────────────────────────────┤
│  Relative Euclidean Regions                                                 │
│     ├── RegionId → local Cartesian space (f32 for Bevy/Avian)              │
│     ├── Floating origin per chunk (no precision jitter)                     │
│     └── Seamless transitions (velocity preservation)                        │
├─────────────────────────────────────────────────────────────────────────────┤
│  Chunking & Streaming                                                       │
│     ├── Cesium-inspired 3D Tiles hierarchy (quadtree/octree)               │
│     ├── Adaptive LOD for GS/mesh hybrid rendering                          │
│     └── P2P CRDT sync for persistent regions                               │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Core Types

### OrbitalCoords Component

The main component for entities in the orbital grid:

```rust
use eustress_common::orbital::*;

#[derive(Component)]
pub struct OrbitalCoords {
    /// High-precision global ECEF position (meters from Earth center)
    pub global_ecef: GlobalPosition,
    
    /// Current region this entity belongs to
    pub region_id: RegionId,
    
    /// Local Euclidean position within the region (f32 for Bevy/Avian)
    pub local_position: Vec3,
    
    /// Local velocity in region space (m/s)
    pub local_velocity: Vec3,
    
    /// Whether this entity should sync its Transform from orbital coords
    pub sync_transform: bool,
}
```

### GlobalPosition

High-precision ECEF coordinates:

```rust
pub struct GlobalPosition {
    pub x: f64,  // Through prime meridian at equator
    pub y: f64,  // Through 90°E at equator
    pub z: f64,  // Through north pole
}

// Create from geodetic (lat/lon/alt)
let sf = GlobalPosition::from_geodetic(37.7749, -122.4194, 10.0);

// Convert back to geodetic
let (lat, lon, alt) = sf.to_geodetic();

// Distance between points
let distance = sf.distance_to(&other);
```

### RegionId

Hierarchical region identifier for chunking:

```rust
pub struct RegionId {
    pub level: u8,   // 0 = whole Earth, 24 = ~1m resolution
    pub face: u8,    // Cube-sphere face (0-5)
    pub x: u32,      // X index at this level
    pub y: u32,      // Y index at this level
    pub z: u32,      // Z index (altitude bands)
}

// Create from geodetic
let region = RegionId::from_geodetic(37.7749, -122.4194);

// Get tile size
let size = region.tile_size_meters(); // ~600m at level 16

// Abstract (non-Earth) region
let abstract_region = RegionId::abstract_region(12345);
```

---

## WGS84/ECEF Coordinates

### Constants

```rust
use eustress_common::orbital::wgs84::*;

// Earth ellipsoid parameters
WGS84_A          // Semi-major axis: 6,378,137 m
WGS84_B          // Semi-minor axis: 6,356,752 m
EARTH_MEAN_RADIUS // Mean radius: 6,371,000 m
EARTH_GM         // Gravitational parameter: 3.986e14 m³/s²
```

### Coordinate Conversions

```rust
// Geodetic → ECEF
let ecef = geodetic_to_ecef(lat_deg, lon_deg, alt_meters);

// ECEF → Geodetic
let (lat, lon, alt) = ecef_to_geodetic(x, y, z);

// Great-circle distance (Haversine)
let distance = haversine_distance(lat1, lon1, lat2, lon2);

// Bearing between points
let bearing = bearing(lat1, lon1, lat2, lon2);

// Destination from start + bearing + distance
let (lat2, lon2) = destination_point(lat, lon, bearing_deg, distance_m);
```

---

## Relative Euclidean Regions

### Region Definition

```rust
pub struct Region {
    pub id: RegionId,
    pub origin: GlobalPosition,      // ECEF center
    pub size: Vec3,                  // Local bounds (meters)
    pub active: bool,                // Currently loaded
    pub custom_gravity: Option<Vec3>, // Override orbital gravity
    pub is_abstract: bool,           // Non-Earth space
    pub parent_offset: Option<Vec3>, // Link to parent region
}

// Create Earth-surface region
let sf_region = Region::from_geodetic(37.7749, -122.4194, 1000.0);

// Create abstract space
let dungeon = Region::abstract_space(hash("dungeon_1"), Vec3::splat(500.0));

// Create linked abstract space (portal from Earth)
let pocket_dim = Region::abstract_child(
    sf_region.id,
    Vec3::new(0.0, -100.0, 0.0), // 100m below surface
    Vec3::splat(200.0),
    hash("pocket_dimension"),
);
```

### RegionRegistry Resource

```rust
// Access the registry
fn my_system(mut registry: ResMut<RegionRegistry>) {
    // Register a new region
    registry.register(Region::from_geodetic(40.7128, -74.0060, 1000.0));
    
    // Activate/deactivate regions
    registry.activate(region_id);
    registry.deactivate(region_id);
    
    // Find region containing a position
    if let Some(id) = registry.find_region(&global_pos) {
        // ...
    }
    
    // Update active regions based on focus
    registry.update_active_regions(&focus_global);
}
```

### Region Transitions

```rust
// Seamless transition between regions
let (new_pos, new_vel) = transition_between_regions(
    local_pos,
    local_vel,
    &from_region,
    &to_region,
);

// Calculate offset between regions
let offset = region_offset(&from_region, &to_region);
```

---

## Orbital Gravity

### OrbitalGravity Resource

```rust
// Earth-only (default, fast)
app.insert_resource(OrbitalGravity::earth_only());

// Earth + Moon (for tidal effects)
app.insert_resource(OrbitalGravity::earth_moon());

// Full solar system
app.insert_resource(OrbitalGravity::full_system());
```

### Gravity Calculation

```rust
fn my_system(gravity: Res<OrbitalGravity>) {
    // Get gravity at a position
    let g = gravity.gravity_at(&global_pos);
    
    // Get "up" direction (opposite of gravity)
    let up = gravity.up_at(&global_pos);
    
    // Use cached gravity (updated each frame)
    let cached_g = gravity.cached();
}
```

### Celestial Bodies

```rust
let earth = CelestialBody::earth();
let moon = CelestialBody::moon();
let sun = CelestialBody::sun();

// Surface gravity
let g = earth.surface_gravity(); // ~9.81 m/s²

// Escape velocity
let v_escape = earth.escape_velocity(); // ~11.2 km/s

// Orbital velocity at altitude
let v_orbit = earth.orbital_velocity(400_000.0); // ISS: ~7.66 km/s
```

### Camera Gravity Alignment

```rust
// Add to camera entity
commands.spawn((
    Camera3d::default(),
    OrbitalCoords::from_geodetic(37.7749, -122.4194, 100.0),
    GravityAligned::default(),      // Smooth alignment
    // or
    GravityAligned::instant(),      // Instant alignment
    // or
    GravityAligned::with_speed(10.0), // Custom speed
));
```

---

## P2P Integration

### Region ↔ ChunkId Mapping

```rust
#[cfg(feature = "p2p")]
use eustress_common::orbital::regions::{region_to_chunk_id, chunk_id_to_region};

// Convert for networking
let chunk_id = region_to_chunk_id(&region_id);

// Convert back
let region_id = chunk_id_to_region(&chunk_id);
```

### Distributed World Plugin

```rust
// Both plugins work together
app.add_plugins(OrbitalPlugin::default())
   .add_plugins(DistributedWorldPlugin);
```

---

## Scene Format

### OrbitalSettings in Scene

```rust
pub struct OrbitalSettings {
    pub enabled: bool,
    pub origin_geodetic: [f64; 3],    // [lat, lon, alt]
    pub region_size: f32,              // Chunk size in meters
    pub nbody_gravity: bool,           // Use n-body simulation
    pub custom_gravity: Option<[f32; 3]>,
    pub is_abstract_space: bool,
    pub parent_region: Option<String>,
    pub parent_offset: Option<[f32; 3]>,
    pub max_detail_level: u8,
    pub camera_gravity_alignment: bool,
}
```

### Preset Configurations

```rust
// Earth surface scene (San Francisco)
let settings = OrbitalSettings::earth_surface(37.7749, -122.4194, 0.0);

// Orbital scene (ISS altitude)
let settings = OrbitalSettings::orbital(0.0, 0.0, 400.0);

// Abstract space (standard gravity)
let settings = OrbitalSettings::abstract_space([0.0, -9.81, 0.0]);

// Abstract space linked to Earth
let settings = OrbitalSettings::abstract_linked(
    "L16F2(12345,67890,0)",
    [0.0, -50.0, 0.0],
    [0.0, -9.81, 0.0],
);
```

---

## Usage Examples

### Spawn Entity at Geodetic Location

```rust
fn spawn_at_location(mut commands: Commands) {
    // San Francisco
    commands.spawn((
        OrbitalCoords::from_geodetic(37.7749, -122.4194, 10.0),
        Transform::default(),
        GlobalTransform::default(),
        Mesh3d::default(),
        // ...
    ));
}
```

### Track Player as Focus

```rust
fn setup_player(mut commands: Commands) {
    commands.spawn((
        OrbitalCoords::from_geodetic(37.7749, -122.4194, 2.0),
        OrbitalFocusMarker, // This entity is the focus
        GravityAligned::default(),
        // Player components...
    ));
}
```

### Create Abstract Dimension

```rust
fn create_dungeon(mut registry: ResMut<RegionRegistry>) {
    // Create abstract region
    let dungeon = Region::abstract_space(
        hash("dark_dungeon"),
        Vec3::new(500.0, 100.0, 500.0),
    );
    
    // Override gravity (lower gravity dungeon)
    let mut dungeon = dungeon;
    dungeon.custom_gravity = Some(Vec3::NEG_Y * 4.0);
    
    registry.register(dungeon);
}
```

### Portal Between Regions

```rust
fn handle_portal(
    mut commands: Commands,
    portals: Query<(&Portal, &OrbitalCoords)>,
    players: Query<(Entity, &OrbitalCoords), With<Player>>,
) {
    for (player_entity, player_coords) in players.iter() {
        for (portal, portal_coords) in portals.iter() {
            if player_coords.local_position.distance(portal_coords.local_position) < 2.0 {
                // Schedule transition to target region
                commands.entity(player_entity).insert(PendingRegionTransition {
                    target_region: portal.target_region,
                    preserve_velocity: true,
                });
            }
        }
    }
}
```

---

## Best Practices

### 1. Use Appropriate Detail Levels

| Use Case | Detail Level | Tile Size |
|----------|--------------|-----------|
| Global view | 8 | ~150 km |
| City view | 12 | ~10 km |
| Neighborhood | 16 | ~600 m |
| Street level | 20 | ~40 m |
| Indoor | 24 | ~2.5 m |

### 2. Manage Region Loading

```rust
// Configure registry for your use case
let mut registry = RegionRegistry::new();
registry.max_active_regions = 9;      // 3x3 grid
registry.load_distance = 5000.0;      // 5km
registry.unload_distance = 10000.0;   // 10km
```

### 3. Handle Precision Carefully

```rust
// ✅ Good: Use local coordinates for physics
let local_pos = coords.local_position;
physics_body.position = local_pos;

// ❌ Bad: Convert global to f32 directly
let bad_pos = Vec3::new(
    coords.global_ecef.x as f32, // Precision loss!
    coords.global_ecef.y as f32,
    coords.global_ecef.z as f32,
);
```

### 4. Optimize Gravity Calculations

```rust
// For most games, Earth-only is sufficient
app.insert_resource(OrbitalGravity::earth_only());

// Only use n-body for space simulations
app.insert_resource(OrbitalGravity {
    use_nbody: true,
    max_influence_distance: 1e9, // Limit calculation range
    ..OrbitalGravity::earth_moon()
});
```

### 5. Abstract Spaces for Interiors

```rust
// Large buildings should be abstract spaces
// This avoids precision issues and allows custom gravity

let building_interior = Region::abstract_child(
    street_region.id,
    Vec3::new(100.0, 0.0, 50.0), // Building entrance
    Vec3::new(200.0, 50.0, 200.0), // Interior size
    hash("empire_state_interior"),
);
```

---

## Related Documentation

- [P2P Distributed Worlds](../networking/README.md)
- [Scene Format](./SCENE_FORMAT.md)
- [Physics Integration](./PHYSICS.md)
- [USD Native Format](./USD_NATIVE_FORMAT.md)
