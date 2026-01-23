# 02 - Coordinate Systems

> WGS84, ECEF, and Relative Euclidean Regions for precision at any scale

## Overview

The Eustress Orbital Navigation System employs a multi-layered coordinate system hierarchy to handle positions from sub-meter precision to interplanetary distances. This document details each layer and the transformations between them.

## Coordinate System Hierarchy

```
┌─────────────────────────────────────────────────────────────────────┐
│  Level 1: ICRS (International Celestial Reference System)          │
│  - Inertial frame centered at solar system barycenter              │
│  - Used for: Star positions, interplanetary navigation             │
│  - Precision: Microarcseconds                                      │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │ Earth rotation + precession
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Level 2: GCRS (Geocentric Celestial Reference System)             │
│  - Non-rotating frame at Earth's center                            │
│  - Used for: Satellite orbits, lunar operations                    │
│  - Precision: Millimeters                                          │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │ Earth rotation (sidereal time)
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Level 3: ECEF/WGS84 (Earth-Centered, Earth-Fixed)                 │
│  - Rotating with Earth                                             │
│  - Used for: Ground stations, geostationary satellites             │
│  - Precision: Centimeters (GPS-grade)                              │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │ Regional decomposition
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Level 4: Relative Euclidean Regions                               │
│  - Hierarchical spatial chunks                                     │
│  - Used for: Rendering, physics, local navigation                  │
│  - Precision: Sub-millimeter (f32 in local frame)                  │
└─────────────────────────────────────────────────────────────────────┘
```

## WGS84 Geodetic System

The World Geodetic System 1984 (WGS84) is the foundation for Earth-referenced positions.

### Ellipsoid Parameters

| Parameter | Symbol | Value |
|-----------|--------|-------|
| Semi-major axis | a | 6,378,137.0 m |
| Semi-minor axis | b | 6,356,752.314245 m |
| Flattening | f | 1/298.257223563 |
| Eccentricity² | e² | 0.00669437999014 |

### Geodetic Coordinates

```
(λ, φ, h) = (longitude, latitude, height above ellipsoid)

λ ∈ [-180°, +180°]  (East positive)
φ ∈ [-90°, +90°]    (North positive)
h ∈ ℝ               (meters above WGS84 ellipsoid)
```

### Implementation

```rust
use std::f64::consts::PI;

pub const WGS84_A: f64 = 6_378_137.0;           // Semi-major axis (m)
pub const WGS84_F: f64 = 1.0 / 298.257223563;   // Flattening
pub const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F); // Semi-minor axis
pub const WGS84_E2: f64 = 2.0 * WGS84_F - WGS84_F * WGS84_F; // Eccentricity²

#[derive(Clone, Copy, Debug)]
pub struct Geodetic {
    pub longitude: f64,  // radians
    pub latitude: f64,   // radians
    pub altitude: f64,   // meters above ellipsoid
}

impl Geodetic {
    pub fn from_degrees(lon_deg: f64, lat_deg: f64, alt_m: f64) -> Self {
        Self {
            longitude: lon_deg.to_radians(),
            latitude: lat_deg.to_radians(),
            altitude: alt_m,
        }
    }
    
    pub fn to_degrees(&self) -> (f64, f64, f64) {
        (
            self.longitude.to_degrees(),
            self.latitude.to_degrees(),
            self.altitude,
        )
    }
}
```

## ECEF (Earth-Centered, Earth-Fixed)

ECEF provides Cartesian coordinates fixed to Earth's rotation.

### Coordinate Definition

```
Origin: Earth's center of mass
X-axis: Intersection of equator and prime meridian (0° lon)
Y-axis: Intersection of equator and 90°E meridian
Z-axis: North pole (parallel to Earth's rotation axis)
```

### Geodetic → ECEF Transformation

```rust
use glam::DVec3;

pub fn geodetic_to_ecef(geo: &Geodetic) -> DVec3 {
    let sin_lat = geo.latitude.sin();
    let cos_lat = geo.latitude.cos();
    let sin_lon = geo.longitude.sin();
    let cos_lon = geo.longitude.cos();
    
    // Radius of curvature in the prime vertical
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
    
    let x = (n + geo.altitude) * cos_lat * cos_lon;
    let y = (n + geo.altitude) * cos_lat * sin_lon;
    let z = (n * (1.0 - WGS84_E2) + geo.altitude) * sin_lat;
    
    DVec3::new(x, y, z)
}
```

### ECEF → Geodetic Transformation (Bowring's Method)

```rust
pub fn ecef_to_geodetic(ecef: DVec3) -> Geodetic {
    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;
    
    let p = (x * x + y * y).sqrt();
    let longitude = y.atan2(x);
    
    // Bowring's iterative method (converges in 2-3 iterations)
    let mut latitude = (z / p).atan(); // Initial approximation
    
    for _ in 0..5 {
        let sin_lat = latitude.sin();
        let cos_lat = latitude.cos();
        let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
        
        latitude = (z + WGS84_E2 * n * sin_lat).atan2(p);
    }
    
    let sin_lat = latitude.sin();
    let cos_lat = latitude.cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
    
    let altitude = if cos_lat.abs() > 1e-10 {
        p / cos_lat - n
    } else {
        z.abs() / sin_lat.abs() - n * (1.0 - WGS84_E2)
    };
    
    Geodetic { longitude, latitude, altitude }
}
```

## ICRS and GCRS

### ICRS (International Celestial Reference System)

The ICRS is the fundamental inertial reference frame for astronomy:

- **Origin**: Solar system barycenter
- **Orientation**: Fixed relative to distant quasars
- **Axes**: Aligned with J2000.0 equator and equinox

### GCRS (Geocentric Celestial Reference System)

GCRS is ICRS translated to Earth's center:

```rust
pub fn icrs_to_gcrs(icrs_pos: DVec3, earth_barycentric: DVec3) -> DVec3 {
    icrs_pos - earth_barycentric
}
```

### GCRS → ECEF Transformation

Requires Earth rotation angle (ERA) based on UT1:

```rust
pub fn gcrs_to_ecef(gcrs: DVec3, julian_date_ut1: f64) -> DVec3 {
    // Earth Rotation Angle (simplified, ignoring precession/nutation)
    let du = julian_date_ut1 - 2451545.0; // Days since J2000.0
    let era = 2.0 * PI * (0.7790572732640 + 1.00273781191135448 * du);
    
    let cos_era = era.cos();
    let sin_era = era.sin();
    
    // Rotation matrix R3(-ERA)
    DVec3::new(
        cos_era * gcrs.x + sin_era * gcrs.y,
        -sin_era * gcrs.x + cos_era * gcrs.y,
        gcrs.z,
    )
}
```

## Relative Euclidean Regions

The core innovation: hierarchical spatial chunks that provide f32 precision within an f64 global framework.

### Region Structure

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegionId {
    pub level: u8,        // Hierarchy level (0 = planetary, 24+ = sub-meter)
    pub face: u8,         // Cube face (0-5) for spherical mapping
    pub x: u32,           // Grid X index
    pub y: u32,           // Grid Y index  
    pub z: u32,           // Grid Z index (for 3D regions)
    pub is_abstract: bool, // True for non-physical spaces (ship interiors)
}

pub struct Region {
    pub id: RegionId,
    pub origin_ecef: DVec3,    // High-precision global origin
    pub half_extent: f64,      // Region size in meters
    pub custom_gravity: Option<DVec3>,
    pub parent: Option<RegionId>,
    pub parent_offset: Option<DVec3>,
}
```

### Level Scale Mapping

| Level | Half-Extent | Use Case |
|-------|-------------|----------|
| 0 | ~6,400 km | Planetary scale |
| 4 | ~400 km | Continental |
| 8 | ~25 km | Regional |
| 12 | ~1.5 km | City |
| 16 | ~100 m | Building |
| 20 | ~6 m | Room |
| 24 | ~0.4 m | Object |
| 28 | ~2.5 cm | Detail |

### Region Decomposition

```rust
impl RegionId {
    pub fn from_ecef(ecef: DVec3, target_level: u8) -> Self {
        // Map ECEF to cube face
        let (face, u, v, w) = ecef_to_cube_face(ecef);
        
        // Compute grid indices at target level
        let scale = 1u32 << target_level;
        let x = ((u + 1.0) * 0.5 * scale as f64) as u32;
        let y = ((v + 1.0) * 0.5 * scale as f64) as u32;
        let z = ((w + 1.0) * 0.5 * scale as f64) as u32;
        
        Self {
            level: target_level,
            face,
            x: x.min(scale - 1),
            y: y.min(scale - 1),
            z: z.min(scale - 1),
            is_abstract: false,
        }
    }
    
    pub fn parent(&self) -> Option<Self> {
        if self.level == 0 {
            return None;
        }
        Some(Self {
            level: self.level - 1,
            face: self.face,
            x: self.x / 2,
            y: self.y / 2,
            z: self.z / 2,
            is_abstract: self.is_abstract,
        })
    }
    
    pub fn children(&self) -> [Self; 8] {
        let new_level = self.level + 1;
        let base_x = self.x * 2;
        let base_y = self.y * 2;
        let base_z = self.z * 2;
        
        [
            Self { level: new_level, face: self.face, x: base_x,     y: base_y,     z: base_z,     is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x + 1, y: base_y,     z: base_z,     is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x,     y: base_y + 1, z: base_z,     is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x + 1, y: base_y + 1, z: base_z,     is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x,     y: base_y,     z: base_z + 1, is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x + 1, y: base_y,     z: base_z + 1, is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x,     y: base_y + 1, z: base_z + 1, is_abstract: self.is_abstract },
            Self { level: new_level, face: self.face, x: base_x + 1, y: base_y + 1, z: base_z + 1, is_abstract: self.is_abstract },
        ]
    }
}

fn ecef_to_cube_face(ecef: DVec3) -> (u8, f64, f64, f64) {
    let abs_x = ecef.x.abs();
    let abs_y = ecef.y.abs();
    let abs_z = ecef.z.abs();
    
    if abs_x >= abs_y && abs_x >= abs_z {
        if ecef.x > 0.0 {
            (0, ecef.y / abs_x, ecef.z / abs_x, 1.0) // +X face
        } else {
            (1, -ecef.y / abs_x, ecef.z / abs_x, 1.0) // -X face
        }
    } else if abs_y >= abs_x && abs_y >= abs_z {
        if ecef.y > 0.0 {
            (2, -ecef.x / abs_y, ecef.z / abs_y, 1.0) // +Y face
        } else {
            (3, ecef.x / abs_y, ecef.z / abs_y, 1.0) // -Y face
        }
    } else {
        if ecef.z > 0.0 {
            (4, ecef.x / abs_z, ecef.y / abs_z, 1.0) // +Z face (North)
        } else {
            (5, ecef.x / abs_z, -ecef.y / abs_z, 1.0) // -Z face (South)
        }
    }
}
```

### Region Registry

```rust
use bevy::utils::HashMap;

#[derive(Resource)]
pub struct RegionRegistry {
    regions: HashMap<RegionId, Region>,
    active: Vec<RegionId>,
    max_active: usize,
    load_radius_m: f64,
}

impl RegionRegistry {
    pub fn new(max_active: usize, load_radius_m: f64) -> Self {
        Self {
            regions: HashMap::new(),
            active: Vec::with_capacity(max_active),
            max_active,
            load_radius_m,
        }
    }
    
    pub fn find_or_create(&mut self, ecef: DVec3, level: u8) -> RegionId {
        let id = RegionId::from_ecef(ecef, level);
        
        if !self.regions.contains_key(&id) {
            let origin = self.compute_region_origin(&id);
            let half_extent = self.level_to_half_extent(level);
            
            self.regions.insert(id, Region {
                id,
                origin_ecef: origin,
                half_extent,
                custom_gravity: None,
                parent: id.parent(),
                parent_offset: None,
            });
        }
        
        id
    }
    
    pub fn global_to_local(&self, ecef: DVec3, region: RegionId) -> Vec3 {
        let region_data = &self.regions[&region];
        let offset = ecef - region_data.origin_ecef;
        offset.as_vec3() // Safe: offset is small within region
    }
    
    pub fn local_to_global(&self, local: Vec3, region: RegionId) -> DVec3 {
        let region_data = &self.regions[&region];
        region_data.origin_ecef + local.as_dvec3()
    }
    
    fn compute_region_origin(&self, id: &RegionId) -> DVec3 {
        // Compute center of region in ECEF
        let scale = 1u32 << id.level;
        let half_extent = self.level_to_half_extent(id.level);
        
        // Convert grid indices back to cube coordinates
        let u = (id.x as f64 + 0.5) / scale as f64 * 2.0 - 1.0;
        let v = (id.y as f64 + 0.5) / scale as f64 * 2.0 - 1.0;
        
        // Map cube face to ECEF direction and scale
        cube_face_to_ecef(id.face, u, v, WGS84_A + half_extent)
    }
    
    fn level_to_half_extent(&self, level: u8) -> f64 {
        WGS84_A / (1u64 << level) as f64
    }
}

fn cube_face_to_ecef(face: u8, u: f64, v: f64, radius: f64) -> DVec3 {
    let dir = match face {
        0 => DVec3::new(1.0, u, v),   // +X
        1 => DVec3::new(-1.0, -u, v), // -X
        2 => DVec3::new(-u, 1.0, v),  // +Y
        3 => DVec3::new(u, -1.0, v),  // -Y
        4 => DVec3::new(u, v, 1.0),   // +Z
        5 => DVec3::new(u, -v, -1.0), // -Z
        _ => DVec3::ZERO,
    };
    dir.normalize() * radius
}
```

## Coordinate Transformations Summary

### Complete Pipeline

```rust
pub struct CoordinateTransformer {
    registry: RegionRegistry,
}

impl CoordinateTransformer {
    /// Full transformation: Geodetic → Local Region
    pub fn geodetic_to_local(&mut self, geo: Geodetic, level: u8) -> (RegionId, Vec3) {
        let ecef = geodetic_to_ecef(&geo);
        let region = self.registry.find_or_create(ecef, level);
        let local = self.registry.global_to_local(ecef, region);
        (region, local)
    }
    
    /// Full transformation: Local Region → Geodetic
    pub fn local_to_geodetic(&self, local: Vec3, region: RegionId) -> Geodetic {
        let ecef = self.registry.local_to_global(local, region);
        ecef_to_geodetic(ecef)
    }
    
    /// Distance between two points (handles cross-region)
    pub fn distance(&self, 
        local_a: Vec3, region_a: RegionId,
        local_b: Vec3, region_b: RegionId
    ) -> f64 {
        let ecef_a = self.registry.local_to_global(local_a, region_a);
        let ecef_b = self.registry.local_to_global(local_b, region_b);
        ecef_a.distance(ecef_b)
    }
}
```

## Precision Analysis

### Floating-Point Limits

| Type | Mantissa Bits | Precision at Earth Surface |
|------|---------------|---------------------------|
| f32 | 23 | ~0.5 m (at 6,400 km) |
| f64 | 52 | ~1 nm (at 6,400 km) |

### Region System Precision

With level-20 regions (~6m half-extent):
- Local coordinates: f32 with ~0.001 mm precision
- Global reconstruction: f64 with sub-mm precision
- **No jitter** at any scale

## Next Steps

- [03_SPACESHIP_CENTRIC_TRAVEL.md](./03_SPACESHIP_CENTRIC_TRAVEL.md) - Floating origin implementation
- [04_EARTH_CENTRIC_ORBITAL_GRIDS.md](./04_EARTH_CENTRIC_ORBITAL_GRIDS.md) - Earth-relative tracking
