# 04 - Earth-Centric Orbital Grids

> Earth-relative orbital tracking, geostationary points, and orbital shell management

## Overview

Earth-centric orbital grids provide a fixed reference frame for tracking objects in orbit around Earth. Unlike spaceship-centric coordinates (which move with the vessel), Earth-centric grids remain stationary relative to Earth's surface or rotation, making them ideal for:

- Geostationary satellite tracking
- Ground station communication windows
- Orbital debris cataloging
- Launch and reentry corridors

## Orbital Shell Architecture

### Shell Definition

Orbital space is divided into concentric shells based on altitude:

```
┌─────────────────────────────────────────────────────────────────────┐
│                        ORBITAL SHELLS                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  GEO Shell (35,786 km)                                      │   │
│  │  - Geostationary satellites                                 │   │
│  │  - Communication satellites                                 │   │
│  │  - Weather satellites                                       │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  MEO Shell (2,000 - 35,786 km)                              │   │
│  │  - GPS constellation (~20,200 km)                           │   │
│  │  - Galileo, GLONASS                                         │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  LEO Shell (160 - 2,000 km)                                 │   │
│  │  - ISS (~400 km)                                            │   │
│  │  - Starlink (~550 km)                                       │   │
│  │  - Earth observation                                        │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ═══════════════════════════════════════════════════════════════   │
│                    KÁRMÁN LINE (100 km)                             │
│  ═══════════════════════════════════════════════════════════════   │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  EARTH SURFACE                                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Shell Data Structure

```rust
#[derive(Clone, Debug)]
pub struct OrbitalShell {
    pub name: String,
    pub min_altitude_km: f64,
    pub max_altitude_km: f64,
    pub grid_resolution: ShellResolution,
    pub objects: Vec<Entity>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShellResolution {
    /// Longitude divisions (e.g., 360 for 1° resolution)
    pub lon_divisions: u32,
    /// Latitude divisions (e.g., 180 for 1° resolution)
    pub lat_divisions: u32,
    /// Altitude layers within shell
    pub alt_layers: u32,
}

#[derive(Resource)]
pub struct OrbitalShellRegistry {
    pub shells: Vec<OrbitalShell>,
    pub leo: usize,  // Index into shells
    pub meo: usize,
    pub geo: usize,
}

impl Default for OrbitalShellRegistry {
    fn default() -> Self {
        Self {
            shells: vec![
                OrbitalShell {
                    name: "LEO".into(),
                    min_altitude_km: 160.0,
                    max_altitude_km: 2000.0,
                    grid_resolution: ShellResolution {
                        lon_divisions: 360,
                        lat_divisions: 180,
                        alt_layers: 20,
                    },
                    objects: Vec::new(),
                },
                OrbitalShell {
                    name: "MEO".into(),
                    min_altitude_km: 2000.0,
                    max_altitude_km: 35786.0,
                    grid_resolution: ShellResolution {
                        lon_divisions: 180,
                        lat_divisions: 90,
                        alt_layers: 10,
                    },
                    objects: Vec::new(),
                },
                OrbitalShell {
                    name: "GEO".into(),
                    min_altitude_km: 35786.0,
                    max_altitude_km: 36000.0,
                    grid_resolution: ShellResolution {
                        lon_divisions: 720,  // 0.5° for GEO slots
                        lat_divisions: 1,    // Equatorial only
                        alt_layers: 1,
                    },
                    objects: Vec::new(),
                },
            ],
            leo: 0,
            meo: 1,
            geo: 2,
        }
    }
}
```

## Geostationary Grid

### GEO Slot System

Geostationary satellites occupy specific longitude slots along the equator:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoSlot {
    /// Longitude in degrees (-180 to +180)
    pub longitude: f64,
    /// Slot width in degrees (typically 0.1° to 2°)
    pub width: f64,
    /// Assigned operator/satellite
    pub assignment: Option<GeoAssignment>,
}

#[derive(Clone, Debug)]
pub struct GeoAssignment {
    pub satellite_name: String,
    pub norad_id: u32,
    pub operator: String,
}

#[derive(Resource)]
pub struct GeoSlotRegistry {
    /// All defined GEO slots
    pub slots: Vec<GeoSlot>,
    /// Quick lookup by longitude (rounded to 0.1°)
    pub by_longitude: std::collections::HashMap<i32, usize>,
}

impl GeoSlotRegistry {
    pub fn find_slot(&self, longitude: f64) -> Option<&GeoSlot> {
        let key = (longitude * 10.0).round() as i32;
        self.by_longitude.get(&key).map(|&idx| &self.slots[idx])
    }
    
    pub fn nearest_slot(&self, longitude: f64) -> Option<&GeoSlot> {
        self.slots.iter().min_by(|a, b| {
            let diff_a = (a.longitude - longitude).abs();
            let diff_b = (b.longitude - longitude).abs();
            diff_a.partial_cmp(&diff_b).unwrap()
        })
    }
}
```

### GEO Position Calculation

```rust
/// Geostationary altitude in meters
pub const GEO_ALTITUDE_M: f64 = 35_786_000.0;

/// Earth's equatorial radius in meters
pub const EARTH_RADIUS_M: f64 = 6_378_137.0;

/// GEO orbital radius from Earth's center
pub const GEO_RADIUS_M: f64 = EARTH_RADIUS_M + GEO_ALTITUDE_M;

/// Calculate ECEF position for a GEO slot
pub fn geo_slot_to_ecef(longitude_deg: f64) -> DVec3 {
    let lon_rad = longitude_deg.to_radians();
    
    DVec3::new(
        GEO_RADIUS_M * lon_rad.cos(),
        GEO_RADIUS_M * lon_rad.sin(),
        0.0, // Equatorial plane
    )
}

/// Calculate sub-satellite point (ground track) for GEO
pub fn geo_subsatellite_point(longitude_deg: f64) -> Geodetic {
    Geodetic {
        longitude: longitude_deg.to_radians(),
        latitude: 0.0, // Always equatorial
        altitude: 0.0, // On surface
    }
}
```

## Earth-Fixed Grid Cells

### Spherical Grid System

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EarthGridCell {
    /// Longitude index (0 to lon_divisions-1)
    pub lon_idx: u32,
    /// Latitude index (0 to lat_divisions-1)
    pub lat_idx: u32,
    /// Altitude layer index
    pub alt_idx: u32,
    /// Shell this cell belongs to
    pub shell: u8,
}

impl EarthGridCell {
    /// Create from geodetic coordinates
    pub fn from_geodetic(
        geo: &Geodetic,
        resolution: &ShellResolution,
        shell: u8,
        shell_min_alt: f64,
        shell_max_alt: f64,
    ) -> Self {
        let lon_deg = geo.longitude.to_degrees();
        let lat_deg = geo.latitude.to_degrees();
        let alt_km = geo.altitude / 1000.0;
        
        // Normalize longitude to [0, 360)
        let lon_norm = (lon_deg + 180.0).rem_euclid(360.0);
        let lon_idx = ((lon_norm / 360.0) * resolution.lon_divisions as f64) as u32;
        
        // Normalize latitude to [0, 180) (south pole = 0, north pole = 180)
        let lat_norm = lat_deg + 90.0;
        let lat_idx = ((lat_norm / 180.0) * resolution.lat_divisions as f64) as u32;
        
        // Altitude layer
        let alt_norm = (alt_km - shell_min_alt) / (shell_max_alt - shell_min_alt);
        let alt_idx = (alt_norm.clamp(0.0, 0.9999) * resolution.alt_layers as f64) as u32;
        
        Self {
            lon_idx: lon_idx.min(resolution.lon_divisions - 1),
            lat_idx: lat_idx.min(resolution.lat_divisions - 1),
            alt_idx: alt_idx.min(resolution.alt_layers - 1),
            shell,
        }
    }
    
    /// Get cell center in geodetic coordinates
    pub fn center_geodetic(
        &self,
        resolution: &ShellResolution,
        shell_min_alt: f64,
        shell_max_alt: f64,
    ) -> Geodetic {
        let lon_deg = (self.lon_idx as f64 + 0.5) / resolution.lon_divisions as f64 * 360.0 - 180.0;
        let lat_deg = (self.lat_idx as f64 + 0.5) / resolution.lat_divisions as f64 * 180.0 - 90.0;
        let alt_km = shell_min_alt + (self.alt_idx as f64 + 0.5) / resolution.alt_layers as f64 
            * (shell_max_alt - shell_min_alt);
        
        Geodetic {
            longitude: lon_deg.to_radians(),
            latitude: lat_deg.to_radians(),
            altitude: alt_km * 1000.0,
        }
    }
    
    /// Get all 26 neighboring cells (3D Moore neighborhood)
    pub fn neighbors(&self, resolution: &ShellResolution) -> Vec<EarthGridCell> {
        let mut neighbors = Vec::with_capacity(26);
        
        for dlon in [-1i32, 0, 1] {
            for dlat in [-1i32, 0, 1] {
                for dalt in [-1i32, 0, 1] {
                    if dlon == 0 && dlat == 0 && dalt == 0 {
                        continue;
                    }
                    
                    // Wrap longitude
                    let new_lon = (self.lon_idx as i32 + dlon)
                        .rem_euclid(resolution.lon_divisions as i32) as u32;
                    
                    // Clamp latitude (no wrap at poles)
                    let new_lat = (self.lat_idx as i32 + dlat)
                        .clamp(0, resolution.lat_divisions as i32 - 1) as u32;
                    
                    // Clamp altitude
                    let new_alt = (self.alt_idx as i32 + dalt)
                        .clamp(0, resolution.alt_layers as i32 - 1) as u32;
                    
                    neighbors.push(EarthGridCell {
                        lon_idx: new_lon,
                        lat_idx: new_lat,
                        alt_idx: new_alt,
                        shell: self.shell,
                    });
                }
            }
        }
        
        neighbors
    }
}
```

## Ground Track Projection

### Orbital Ground Track

```rust
#[derive(Clone, Debug)]
pub struct GroundTrack {
    /// Points along the ground track (geodetic, altitude = 0)
    pub points: Vec<Geodetic>,
    /// Corresponding times (seconds from epoch)
    pub times: Vec<f64>,
    /// Orbital period in seconds
    pub period: f64,
}

impl GroundTrack {
    /// Generate ground track from orbital elements
    pub fn from_orbital_elements(
        elements: &OrbitalElements,
        start_time: f64,
        duration: f64,
        step: f64,
    ) -> Self {
        let mut points = Vec::new();
        let mut times = Vec::new();
        
        let mut t = start_time;
        while t < start_time + duration {
            // Propagate to time t
            let (pos_ecef, _vel) = propagate_kepler(elements, t);
            
            // Convert to geodetic
            let geo = ecef_to_geodetic(pos_ecef);
            
            // Sub-satellite point (altitude = 0)
            points.push(Geodetic {
                longitude: geo.longitude,
                latitude: geo.latitude,
                altitude: 0.0,
            });
            times.push(t);
            
            t += step;
        }
        
        Self {
            points,
            times,
            period: elements.period(),
        }
    }
}

/// Simplified Keplerian propagation
fn propagate_kepler(elements: &OrbitalElements, time: f64) -> (DVec3, DVec3) {
    // Mean motion (rad/s)
    let n = (GM_EARTH / elements.semi_major_axis.powi(3)).sqrt();
    
    // Mean anomaly at time
    let m = elements.mean_anomaly_epoch + n * (time - elements.epoch);
    
    // Solve Kepler's equation for eccentric anomaly (Newton-Raphson)
    let mut e_anom = m;
    for _ in 0..10 {
        e_anom = e_anom - (e_anom - elements.eccentricity * e_anom.sin() - m) 
            / (1.0 - elements.eccentricity * e_anom.cos());
    }
    
    // True anomaly
    let true_anom = 2.0 * ((1.0 + elements.eccentricity).sqrt() * (e_anom / 2.0).tan())
        .atan2((1.0 - elements.eccentricity).sqrt());
    
    // Distance from focus
    let r = elements.semi_major_axis * (1.0 - elements.eccentricity * e_anom.cos());
    
    // Position in orbital plane
    let x_orb = r * true_anom.cos();
    let y_orb = r * true_anom.sin();
    
    // Rotate to ECEF (simplified, ignoring Earth rotation for now)
    let pos = rotate_orbital_to_ecef(
        x_orb, y_orb,
        elements.inclination,
        elements.raan,
        elements.arg_periapsis,
    );
    
    // Velocity (simplified)
    let vel = DVec3::ZERO; // Full implementation would compute this
    
    (pos, vel)
}
```

## Visibility and Coverage

### Line-of-Sight Calculation

```rust
/// Check if two points have line-of-sight (no Earth obstruction)
pub fn has_line_of_sight(pos_a: DVec3, pos_b: DVec3) -> bool {
    // Parametric line: P(t) = A + t*(B-A), t ∈ [0,1]
    // Check if line intersects Earth ellipsoid
    
    let d = pos_b - pos_a;
    let a_sq = WGS84_A * WGS84_A;
    let b_sq = WGS84_B * WGS84_B;
    
    // Quadratic coefficients for ellipsoid intersection
    let a_coef = d.x * d.x / a_sq + d.y * d.y / a_sq + d.z * d.z / b_sq;
    let b_coef = 2.0 * (pos_a.x * d.x / a_sq + pos_a.y * d.y / a_sq + pos_a.z * d.z / b_sq);
    let c_coef = pos_a.x * pos_a.x / a_sq + pos_a.y * pos_a.y / a_sq 
        + pos_a.z * pos_a.z / b_sq - 1.0;
    
    let discriminant = b_coef * b_coef - 4.0 * a_coef * c_coef;
    
    if discriminant < 0.0 {
        // No intersection with ellipsoid
        return true;
    }
    
    let sqrt_disc = discriminant.sqrt();
    let t1 = (-b_coef - sqrt_disc) / (2.0 * a_coef);
    let t2 = (-b_coef + sqrt_disc) / (2.0 * a_coef);
    
    // Check if intersection is between the two points
    !(t1 > 0.0 && t1 < 1.0) && !(t2 > 0.0 && t2 < 1.0)
}

/// Calculate elevation angle from ground station to satellite
pub fn elevation_angle(ground_ecef: DVec3, sat_ecef: DVec3) -> f64 {
    let to_sat = sat_ecef - ground_ecef;
    let up = ground_ecef.normalize(); // Local vertical
    
    let horizontal = to_sat - up * to_sat.dot(up);
    let elevation = to_sat.dot(up).atan2(horizontal.length());
    
    elevation.to_degrees()
}

/// Calculate azimuth from ground station to satellite
pub fn azimuth_angle(ground_geo: &Geodetic, sat_ecef: DVec3) -> f64 {
    let ground_ecef = geodetic_to_ecef(ground_geo);
    let to_sat = sat_ecef - ground_ecef;
    
    // Local East-North-Up frame
    let up = ground_ecef.normalize();
    let east = DVec3::new(-ground_geo.longitude.sin(), ground_geo.longitude.cos(), 0.0);
    let north = up.cross(east);
    
    let east_component = to_sat.dot(east);
    let north_component = to_sat.dot(north);
    
    east_component.atan2(north_component).to_degrees()
}
```

### Coverage Footprint

```rust
#[derive(Clone, Debug)]
pub struct CoverageFootprint {
    /// Boundary points (geodetic, altitude = 0)
    pub boundary: Vec<Geodetic>,
    /// Center point (sub-satellite)
    pub center: Geodetic,
    /// Maximum coverage radius on surface (km)
    pub radius_km: f64,
}

impl CoverageFootprint {
    /// Calculate coverage footprint for a satellite
    pub fn from_satellite(
        sat_ecef: DVec3,
        min_elevation_deg: f64,
        num_points: usize,
    ) -> Self {
        let sat_geo = ecef_to_geodetic(sat_ecef);
        let altitude_km = sat_geo.altitude / 1000.0;
        
        // Earth central angle for minimum elevation
        // Using spherical Earth approximation
        let earth_radius_km = EARTH_RADIUS_M / 1000.0;
        let min_elev_rad = min_elevation_deg.to_radians();
        
        // Horizon angle from satellite
        let rho = (earth_radius_km / (earth_radius_km + altitude_km)).asin();
        
        // Earth central angle to edge of coverage
        let lambda = (std::f64::consts::FRAC_PI_2 - min_elev_rad - rho).max(0.0);
        
        // Coverage radius on surface
        let radius_km = lambda * earth_radius_km;
        
        // Generate boundary points
        let mut boundary = Vec::with_capacity(num_points);
        for i in 0..num_points {
            let azimuth = 2.0 * std::f64::consts::PI * i as f64 / num_points as f64;
            
            // Great circle navigation from sub-satellite point
            let (lat, lon) = great_circle_destination(
                sat_geo.latitude,
                sat_geo.longitude,
                azimuth,
                lambda,
            );
            
            boundary.push(Geodetic {
                longitude: lon,
                latitude: lat,
                altitude: 0.0,
            });
        }
        
        Self {
            boundary,
            center: Geodetic {
                longitude: sat_geo.longitude,
                latitude: sat_geo.latitude,
                altitude: 0.0,
            },
            radius_km,
        }
    }
}

/// Great circle destination calculation
fn great_circle_destination(
    lat1: f64, lon1: f64,  // radians
    bearing: f64,          // radians from north
    angular_distance: f64, // radians
) -> (f64, f64) {
    let lat2 = (lat1.sin() * angular_distance.cos() 
        + lat1.cos() * angular_distance.sin() * bearing.cos()).asin();
    
    let lon2 = lon1 + (bearing.sin() * angular_distance.sin() * lat1.cos())
        .atan2(angular_distance.cos() - lat1.sin() * lat2.sin());
    
    (lat2, lon2)
}
```

## Grid Visualization

### Bevy Rendering System

```rust
#[derive(Component)]
pub struct OrbitalGridVisual {
    pub shell: u8,
    pub visible: bool,
    pub color: Color,
}

fn spawn_orbital_grid_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shells: Res<OrbitalShellRegistry>,
) {
    for (idx, shell) in shells.shells.iter().enumerate() {
        // Create shell sphere mesh
        let inner_radius = (EARTH_RADIUS_M + shell.min_altitude_km * 1000.0) as f32;
        let outer_radius = (EARTH_RADIUS_M + shell.max_altitude_km * 1000.0) as f32;
        
        // Wireframe grid lines
        let grid_mesh = create_orbital_grid_mesh(
            inner_radius,
            outer_radius,
            shell.grid_resolution.lon_divisions,
            shell.grid_resolution.lat_divisions,
        );
        
        commands.spawn((
            Mesh3d(meshes.add(grid_mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.2, 0.5, 1.0, 0.1),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            OrbitalGridVisual {
                shell: idx as u8,
                visible: true,
                color: Color::srgba(0.2, 0.5, 1.0, 0.1),
            },
            Name::new(format!("{} Grid", shell.name)),
        ));
    }
}

fn create_orbital_grid_mesh(
    inner_radius: f32,
    outer_radius: f32,
    lon_divisions: u32,
    lat_divisions: u32,
) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    
    let mid_radius = (inner_radius + outer_radius) / 2.0;
    
    // Longitude lines
    for i in 0..lon_divisions {
        let lon = 2.0 * std::f32::consts::PI * i as f32 / lon_divisions as f32;
        
        for j in 0..=lat_divisions {
            let lat = std::f32::consts::PI * j as f32 / lat_divisions as f32 - std::f32::consts::FRAC_PI_2;
            
            let x = mid_radius * lat.cos() * lon.cos();
            let y = mid_radius * lat.cos() * lon.sin();
            let z = mid_radius * lat.sin();
            
            positions.push([x, y, z]);
        }
    }
    
    // Latitude lines
    for j in 0..=lat_divisions {
        let lat = std::f32::consts::PI * j as f32 / lat_divisions as f32 - std::f32::consts::FRAC_PI_2;
        
        for i in 0..=lon_divisions {
            let lon = 2.0 * std::f32::consts::PI * i as f32 / lon_divisions as f32;
            
            let x = mid_radius * lat.cos() * lon.cos();
            let y = mid_radius * lat.cos() * lon.sin();
            let z = mid_radius * lat.sin();
            
            positions.push([x, y, z]);
        }
    }
    
    // Generate line indices
    let verts_per_lon_line = (lat_divisions + 1) as u32;
    for i in 0..lon_divisions {
        for j in 0..lat_divisions {
            let base = i * verts_per_lon_line + j;
            indices.push(base);
            indices.push(base + 1);
        }
    }
    
    let lon_offset = lon_divisions * verts_per_lon_line;
    let verts_per_lat_line = (lon_divisions + 1) as u32;
    for j in 0..=lat_divisions {
        for i in 0..lon_divisions {
            let base = lon_offset + j * verts_per_lat_line + i;
            indices.push(base);
            indices.push(base + 1);
        }
    }
    
    Mesh::new(bevy::render::mesh::PrimitiveTopology::LineList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}
```

## Integration with Spaceship-Centric System

### Dual-Frame Queries

```rust
/// Query objects in Earth-centric grid, return in ship-centric coordinates
pub fn query_earth_grid_relative_to_ship(
    grid_cell: EarthGridCell,
    ship_ecef: DVec3,
    shells: &OrbitalShellRegistry,
    objects: &Query<(Entity, &OrbitalCoords)>,
) -> Vec<(Entity, Vec3)> {
    let shell = &shells.shells[grid_cell.shell as usize];
    
    objects
        .iter()
        .filter(|(_, coords)| {
            let obj_cell = EarthGridCell::from_geodetic(
                &ecef_to_geodetic(coords.global_ecef),
                &shell.grid_resolution,
                grid_cell.shell,
                shell.min_altitude_km,
                shell.max_altitude_km,
            );
            obj_cell == grid_cell
        })
        .map(|(entity, coords)| {
            let relative = (coords.global_ecef - ship_ecef).as_vec3();
            (entity, relative)
        })
        .collect()
}
```

## Next Steps

- [05_NAVIGATION_SYSTEM.md](./05_NAVIGATION_SYSTEM.md) - Navigation arrays and mainframe
- [07_DYNAMIC_OBJECTS.md](./07_DYNAMIC_OBJECTS.md) - Satellite and debris tracking
