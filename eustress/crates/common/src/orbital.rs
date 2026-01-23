//! # Orbital Module
//!
//! Orbital mechanics and coordinate systems for planetary-scale scenes.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// WGS84 constants
pub const WGS84_A: f64 = 6378137.0; // Semi-major axis (meters)
pub const WGS84_B: f64 = 6356752.314245; // Semi-minor axis (meters)
pub const EARTH_MEAN_RADIUS: f64 = 6371000.0; // Mean radius (meters)
pub const EARTH_GM: f64 = 3.986004418e14; // Gravitational parameter (m³/s²)

/// Plugin for orbital mechanics
pub struct OrbitalPlugin;

impl Plugin for OrbitalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RegionRegistry>();
    }
}

/// Orbital coordinates (latitude, longitude, altitude)
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct OrbitalCoords {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
}

/// Global position in ECEF coordinates
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct GlobalPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl GlobalPosition {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    
    pub fn from_geodetic(lat: f64, lon: f64, alt: f64) -> Self {
        geodetic_to_ecef(lat, lon, alt)
    }
}

/// Region identifier for hierarchical spatial indexing
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RegionId {
    /// Level of detail (0 = planet, higher = more detail)
    pub level: u8,
    /// Face of cube-sphere projection (0-5)
    pub face: u8,
    /// X tile coordinate
    pub x: u32,
    /// Y tile coordinate
    pub y: u32,
    /// Z tile coordinate (for 3D regions)
    pub z: u32,
}

impl RegionId {
    /// Create from geodetic coordinates
    pub fn from_geodetic(lat: f64, lon: f64) -> Self {
        // Simple tile calculation based on lat/lon
        let level = 10u8; // Default detail level
        let face = ((lon + 180.0) / 60.0) as u8 % 6;
        let x = ((lon + 180.0) * 1000.0) as u32;
        let y = ((lat + 90.0) * 1000.0) as u32;
        Self { level, face, x, y, z: 0 }
    }
    
    /// Create abstract region ID (non-Earth)
    pub fn abstract_region(id: u64) -> Self {
        Self {
            level: 255,
            face: 255,
            x: (id >> 32) as u32,
            y: id as u32,
            z: 0,
        }
    }
}

/// Region definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub name: String,
    pub center: OrbitalCoords,
    pub radius: f64,
}

impl Region {
    /// Create from geodetic coordinates
    pub fn from_geodetic(lat: f64, lon: f64, size: f64) -> Self {
        Self {
            id: RegionId::from_geodetic(lat, lon),
            name: format!("Region_{:.2}_{:.2}", lat, lon),
            center: OrbitalCoords { latitude: lat, longitude: lon, altitude: 0.0 },
            radius: size / 2.0,
        }
    }
    
    /// Create abstract space region (non-Earth)
    pub fn abstract_space(id: u64, size: Vec3) -> Self {
        Self {
            id: RegionId::abstract_region(id),
            name: format!("Abstract_{}", id),
            center: OrbitalCoords::default(),
            radius: size.max_element() as f64 / 2.0,
        }
    }
}

/// Registry of regions
#[derive(Resource, Default)]
pub struct RegionRegistry {
    pub regions: std::collections::HashMap<RegionId, Region>,
}

/// Orbital gravity component
#[derive(Component, Clone, Debug, Default)]
pub struct OrbitalGravity {
    pub strength: f32,
    pub direction: Vec3,
}

/// Gravity-aligned component marker
#[derive(Component, Clone, Debug, Default)]
pub struct GravityAligned;

/// Celestial body component
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct CelestialBody {
    pub name: String,
    pub mass: f64,
    pub gm: f64,
    pub radius: f64,
    pub position: GlobalPosition,
    pub active: bool,
}

/// Orbital focus marker
#[derive(Component, Clone, Debug, Default)]
pub struct OrbitalFocusMarker;

/// Orbital focus resource
#[derive(Resource, Clone, Debug, Default)]
pub struct OrbitalFocus {
    pub entity: Option<Entity>,
    pub coords: OrbitalCoords,
}

/// Convert geodetic coordinates to ECEF
pub fn geodetic_to_ecef(lat: f64, lon: f64, alt: f64) -> GlobalPosition {
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();
    
    let e2 = 1.0 - (WGS84_B * WGS84_B) / (WGS84_A * WGS84_A);
    let n = WGS84_A / (1.0 - e2 * lat_rad.sin().powi(2)).sqrt();
    
    let x = (n + alt) * lat_rad.cos() * lon_rad.cos();
    let y = (n + alt) * lat_rad.cos() * lon_rad.sin();
    let z = (n * (1.0 - e2) + alt) * lat_rad.sin();
    
    GlobalPosition { x, y, z }
}

/// Convert ECEF to geodetic coordinates
pub fn ecef_to_geodetic(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let e2 = 1.0 - (WGS84_B * WGS84_B) / (WGS84_A * WGS84_A);
    let p = (x * x + y * y).sqrt();
    let lon = y.atan2(x);
    
    // Iterative solution for latitude
    let mut lat = z.atan2(p * (1.0 - e2));
    for _ in 0..10 {
        let n = WGS84_A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        lat = z.atan2(p - e2 * n * lat.cos());
    }
    
    let n = WGS84_A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    let alt = p / lat.cos() - n;
    
    (lat.to_degrees(), lon.to_degrees(), alt)
}

/// Calculate haversine distance between two points
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let a = (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    
    EARTH_MEAN_RADIUS * c
}
