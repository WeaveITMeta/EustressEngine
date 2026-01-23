# 03 - Spaceship-Centric Travel

> Floating origin architecture for jitter-free navigation at planetary scales

## Overview

Spaceship-centric travel places the vessel at the center of the coordinate system, with all other objects positioned relative to it. This approach eliminates floating-point precision issues that plague traditional fixed-origin systems at large scales.

## The Floating Origin Problem

### Traditional Approach (Broken)

```
Fixed Origin at Earth's Center:
- Spaceship at 400 km altitude: position = (6,778,137, 0, 0) meters
- Nearby debris at 400.001 km: position = (6,778,138, 0, 0) meters
- Difference: 1 meter

f32 precision at 6.7 million: ~0.5 meters
Result: Objects separated by 1m may render at same position → JITTER
```

### Floating Origin Solution

```
Origin at Spaceship:
- Spaceship position: (0, 0, 0)
- Nearby debris: (1, 0, 0) meters
- Full f32 precision available for local differences

Result: Sub-millimeter precision for nearby objects → NO JITTER
```

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                    SPACESHIP (Floating Origin)                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  OrbitalCoords                                          │   │
│  │  - global_ecef: DVec3 (high precision, rarely used)     │   │
│  │  - region: RegionId (current spatial chunk)             │   │
│  │  - local_pos: Vec3 = (0, 0, 0) always                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                    FloatingOrigin marker                        │
└──────────────────────────────┼──────────────────────────────────┘
                               │
        ┌──────────────────────┼──────────────────────┐
        ▼                      ▼                      ▼
┌───────────────┐    ┌───────────────┐    ┌───────────────┐
│   Satellite   │    │    Planet     │    │    Debris     │
│  local_pos:   │    │  local_pos:   │    │  local_pos:   │
│  relative to  │    │  relative to  │    │  relative to  │
│   spaceship   │    │   spaceship   │    │   spaceship   │
└───────────────┘    └───────────────┘    └───────────────┘
```

### Bevy Integration with big_space

```rust
use bevy::prelude::*;
use big_space::{FloatingOrigin, GridCell, BigSpacePlugin};
use glam::DVec3;

/// Marker component for the player's spaceship
#[derive(Component)]
pub struct SpaceshipMarker;

/// High-precision orbital coordinates
#[derive(Component, Clone, Copy, Debug)]
pub struct OrbitalCoords {
    /// Global position in ECEF (f64 precision)
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

impl OrbitalCoords {
    /// Create from geodetic coordinates (lat/lon/alt)
    pub fn from_geodetic(lat_deg: f64, lon_deg: f64, alt_m: f64) -> Self {
        let geo = Geodetic::from_degrees(lon_deg, lat_deg, alt_m);
        let ecef = geodetic_to_ecef(&geo);
        
        Self {
            global_ecef: ecef,
            region: RegionId::from_ecef(ecef, 16), // ~100m regions
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
}
```

## Floating Origin System

### Plugin Setup

```rust
pub struct SpaceshipCentricPlugin;

impl Plugin for SpaceshipCentricPlugin {
    fn build(&self, app: &mut App) {
        app
            // big_space provides floating origin infrastructure
            .add_plugins(BigSpacePlugin::<i128>::default())
            // Resources
            .insert_resource(RegionRegistry::new(25, 1_000_000.0))
            .insert_resource(NavigationState::default())
            // Systems
            .add_systems(Update, (
                update_floating_origin,
                recompute_local_positions,
                sync_transforms,
            ).chain());
    }
}
```

### Origin Update System

```rust
/// Updates the floating origin when the spaceship moves
fn update_floating_origin(
    spaceship: Query<&OrbitalCoords, (With<SpaceshipMarker>, Changed<OrbitalCoords>)>,
    mut registry: ResMut<RegionRegistry>,
    mut nav_state: ResMut<NavigationState>,
) {
    let Ok(ship_coords) = spaceship.get_single() else { return };
    
    // Update registry's focus point
    registry.set_focus(ship_coords.global_ecef);
    
    // Load/unload regions around new position
    registry.update_active_regions(ship_coords.global_ecef);
    
    // Update navigation state
    nav_state.origin_ecef = ship_coords.global_ecef;
    nav_state.origin_region = ship_coords.region;
}

/// Recomputes local positions for all objects relative to spaceship
fn recompute_local_positions(
    spaceship: Query<&OrbitalCoords, With<SpaceshipMarker>>,
    mut objects: Query<&mut OrbitalCoords, Without<SpaceshipMarker>>,
    registry: Res<RegionRegistry>,
) {
    let Ok(ship) = spaceship.get_single() else { return };
    
    for mut coords in &mut objects {
        // Compute offset from spaceship in global coordinates
        let offset = coords.global_ecef - ship.global_ecef;
        
        // Convert to local f32 (safe: offset is small relative to ship)
        coords.local_pos = offset.as_vec3();
        
        // Update region if needed
        let new_region = registry.find_region_for_offset(offset, ship.region);
        if new_region != coords.region {
            coords.region = new_region;
        }
    }
}

/// Syncs Bevy Transforms from OrbitalCoords
fn sync_transforms(
    mut query: Query<(&OrbitalCoords, &mut Transform), Changed<OrbitalCoords>>,
) {
    for (coords, mut transform) in &mut query {
        if coords.sync_transform {
            transform.translation = coords.local_pos;
        }
    }
}
```

## Spaceship Movement

### Velocity-Based Movement

```rust
#[derive(Component)]
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

fn update_spaceship_physics(
    time: Res<Time>,
    mut spaceship: Query<(
        &mut OrbitalCoords,
        &mut SpaceshipPhysics,
        &mut Transform,
    ), With<SpaceshipMarker>>,
    registry: Res<RegionRegistry>,
) {
    let dt = time.delta_secs_f64();
    
    for (mut coords, mut physics, mut transform) in &mut spaceship {
        // Apply thrust (simplified, ignoring rotation for now)
        let acceleration = physics.thrust.as_dvec3() / physics.mass;
        physics.velocity += (acceleration * dt).as_vec3();
        
        // Update global position
        let velocity_global = physics.velocity.as_dvec3();
        coords.global_ecef += velocity_global * dt;
        
        // Spaceship local_pos is always zero (it's the origin)
        coords.local_pos = Vec3::ZERO;
        
        // Update region if we've moved far enough
        let new_region = RegionId::from_ecef(coords.global_ecef, 16);
        if new_region != coords.region {
            // Region transition - may need to notify other systems
            coords.region = new_region;
        }
        
        // Apply rotation
        let rotation_delta = Quat::from_euler(
            EulerRot::XYZ,
            physics.angular_velocity.x * dt as f32,
            physics.angular_velocity.y * dt as f32,
            physics.angular_velocity.z * dt as f32,
        );
        transform.rotation *= rotation_delta;
    }
}
```

### Orbital Maneuvers

```rust
#[derive(Clone, Debug)]
pub struct OrbitalManeuver {
    /// Time to execute (seconds from now)
    pub t_execute: f64,
    /// Delta-V in local frame (m/s)
    pub delta_v: DVec3,
    /// Maneuver type for UI/logging
    pub maneuver_type: ManeuverType,
}

#[derive(Clone, Debug)]
pub enum ManeuverType {
    Prograde,
    Retrograde,
    Normal,
    AntiNormal,
    RadialIn,
    RadialOut,
    Custom,
}

#[derive(Resource, Default)]
pub struct ManeuverQueue {
    pub maneuvers: Vec<OrbitalManeuver>,
}

fn execute_maneuvers(
    time: Res<Time>,
    mut queue: ResMut<ManeuverQueue>,
    mut spaceship: Query<&mut SpaceshipPhysics, With<SpaceshipMarker>>,
) {
    let current_time = time.elapsed_secs_f64();
    
    // Find and execute due maneuvers
    queue.maneuvers.retain(|maneuver| {
        if maneuver.t_execute <= current_time {
            if let Ok(mut physics) = spaceship.get_single_mut() {
                physics.velocity += maneuver.delta_v.as_vec3();
            }
            false // Remove executed maneuver
        } else {
            true // Keep future maneuvers
        }
    });
}
```

## Reference Frame Switching

### Frame Types

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceFrame {
    /// Ship-centric: ship at origin, +X forward, +Y left, +Z up
    ShipLocal,
    /// Ship-centric but aligned to orbital prograde
    ShipOrbital,
    /// Earth-centric: ECEF coordinates
    EarthFixed,
    /// Inertial: non-rotating Earth-centered
    EarthInertial,
    /// Target-relative: centered on selected target
    TargetRelative,
}

#[derive(Resource)]
pub struct ActiveReferenceFrame {
    pub frame: ReferenceFrame,
    pub target_entity: Option<Entity>,
}
```

### Frame Transformation

```rust
fn transform_to_frame(
    position: DVec3,      // Global ECEF
    velocity: DVec3,      // Global ECEF velocity
    frame: ReferenceFrame,
    ship_ecef: DVec3,
    ship_velocity: DVec3,
    ship_rotation: Quat,
    target_ecef: Option<DVec3>,
    julian_date: f64,
) -> (Vec3, Vec3) {
    match frame {
        ReferenceFrame::ShipLocal => {
            let rel_pos = (position - ship_ecef).as_vec3();
            let rel_vel = (velocity - ship_velocity).as_vec3();
            
            // Rotate into ship's local frame
            let inv_rot = ship_rotation.inverse();
            (inv_rot * rel_pos, inv_rot * rel_vel)
        }
        
        ReferenceFrame::ShipOrbital => {
            let rel_pos = (position - ship_ecef).as_vec3();
            let rel_vel = (velocity - ship_velocity).as_vec3();
            
            // Compute orbital frame (prograde, normal, radial)
            let prograde = ship_velocity.normalize().as_vec3();
            let radial = ship_ecef.normalize().as_vec3();
            let normal = prograde.cross(radial).normalize();
            let radial = normal.cross(prograde).normalize();
            
            let orbital_rot = Mat3::from_cols(prograde, normal, radial);
            (orbital_rot.transpose() * rel_pos, orbital_rot.transpose() * rel_vel)
        }
        
        ReferenceFrame::EarthFixed => {
            // Direct ECEF, offset from ship for rendering
            let rel_pos = (position - ship_ecef).as_vec3();
            let rel_vel = velocity.as_vec3();
            (rel_pos, rel_vel)
        }
        
        ReferenceFrame::EarthInertial => {
            // Rotate ECEF to inertial (undo Earth rotation)
            let ecef_pos = position;
            let inertial_pos = ecef_to_gcrs(ecef_pos, julian_date);
            let rel_pos = (inertial_pos - ecef_to_gcrs(ship_ecef, julian_date)).as_vec3();
            
            // Velocity transformation includes Earth rotation
            let omega_earth = DVec3::new(0.0, 0.0, 7.2921159e-5); // rad/s
            let inertial_vel = velocity + omega_earth.cross(position);
            let ship_inertial_vel = ship_velocity + omega_earth.cross(ship_ecef);
            let rel_vel = (inertial_vel - ship_inertial_vel).as_vec3();
            
            (rel_pos, rel_vel)
        }
        
        ReferenceFrame::TargetRelative => {
            let target = target_ecef.unwrap_or(ship_ecef);
            let rel_pos = (position - target).as_vec3();
            let rel_vel = velocity.as_vec3(); // Simplified
            (rel_pos, rel_vel)
        }
    }
}
```

## Gravity Alignment

### Gravity-Aligned Orientation

```rust
#[derive(Component)]
pub struct GravityAligned {
    /// Smoothing factor (higher = faster alignment)
    pub smoothing: f32,
    /// Override gravity direction (None = compute from position)
    pub custom_gravity: Option<Vec3>,
}

impl GravityAligned {
    pub fn smooth(factor: f32) -> Self {
        Self {
            smoothing: factor,
            custom_gravity: None,
        }
    }
}

fn align_to_gravity(
    time: Res<Time>,
    mut query: Query<(&OrbitalCoords, &GravityAligned, &mut Transform)>,
) {
    let dt = time.delta_secs();
    
    for (coords, gravity_aligned, mut transform) in &mut query {
        // Compute gravity direction (toward Earth center)
        let gravity_dir = gravity_aligned.custom_gravity.unwrap_or_else(|| {
            -coords.global_ecef.normalize().as_vec3()
        });
        
        // Current up vector
        let current_up = transform.rotation * Vec3::Z;
        
        // Target up vector (opposite to gravity)
        let target_up = -gravity_dir;
        
        // Compute rotation to align
        if current_up.dot(target_up) < 0.9999 {
            let rotation_axis = current_up.cross(target_up).normalize_or_zero();
            let rotation_angle = current_up.angle_between(target_up);
            
            // Smooth interpolation
            let smooth_angle = rotation_angle * (gravity_aligned.smoothing * dt).min(1.0);
            let delta_rot = Quat::from_axis_angle(rotation_axis, smooth_angle);
            
            transform.rotation = delta_rot * transform.rotation;
        }
    }
}
```

## Spawning the Spaceship

### Complete Spawn Example

```rust
fn spawn_spaceship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // ISS-like orbit: 51.6° inclination, 400 km altitude
    let lat = 28.5;  // Launch latitude (Kennedy Space Center)
    let lon = -80.6;
    let alt = 400_000.0; // 400 km in meters
    
    commands.spawn((
        // Rendering
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(10.0, 4.0, 2.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.8, 0.9),
            metallic: 0.9,
            ..default()
        })),
        Transform::default(),
        
        // Orbital navigation
        OrbitalCoords::from_geodetic(lat, lon, alt),
        SpaceshipPhysics {
            velocity: Vec3::new(7660.0, 0.0, 0.0), // ~7.66 km/s orbital velocity
            angular_velocity: Vec3::ZERO,
            mass: 420_000.0, // ISS mass in kg
            thrust: Vec3::ZERO,
        },
        
        // Floating origin marker
        FloatingOrigin,
        SpaceshipMarker,
        
        // Gravity alignment
        GravityAligned::smooth(2.0),
        
        // Camera will be child of spaceship
        Name::new("Spaceship"),
    )).with_children(|parent| {
        // Cockpit camera
        parent.spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::new(0.0, 0.0, -10.0), Vec3::Y),
            Name::new("CockpitCamera"),
        ));
    });
}
```

## Performance Considerations

### Update Frequency

| System | Frequency | Reason |
|--------|-----------|--------|
| Floating origin update | On ship movement | Only when needed |
| Local position recompute | Every frame | Smooth rendering |
| Region transitions | On threshold cross | Avoid thrashing |
| Physics integration | Fixed timestep | Determinism |

### Optimization Strategies

1. **Spatial Indexing**: Use `rstar` R-tree for nearby object queries
2. **LOD Regions**: Coarser regions for distant objects
3. **Culling**: Skip updates for objects outside view frustum
4. **Batching**: Group region transitions to minimize recalculations

```rust
#[derive(Resource)]
pub struct SpatialIndex {
    tree: rstar::RTree<SpatialEntry>,
}

struct SpatialEntry {
    entity: Entity,
    position: [f64; 3],
}

impl rstar::RTreeObject for SpatialEntry {
    type Envelope = rstar::AABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_point(self.position)
    }
}

fn query_nearby(
    index: &SpatialIndex,
    center: DVec3,
    radius: f64,
) -> Vec<Entity> {
    let min = [center.x - radius, center.y - radius, center.z - radius];
    let max = [center.x + radius, center.y + radius, center.z + radius];
    let envelope = rstar::AABB::from_corners(min, max);
    
    index.tree
        .locate_in_envelope(&envelope)
        .map(|entry| entry.entity)
        .collect()
}
```

## Next Steps

- [04_EARTH_CENTRIC_ORBITAL_GRIDS.md](./04_EARTH_CENTRIC_ORBITAL_GRIDS.md) - Earth-relative tracking
- [05_NAVIGATION_SYSTEM.md](./05_NAVIGATION_SYSTEM.md) - Navigation arrays and mainframe
