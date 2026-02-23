//! # Coordinate Transforms
//!
//! Transforms geographic coordinates (WGS84 lat/lon) to Bevy local world space.
//!
//! ## Pipeline
//! ```text
//! Geographic (WGS84)  →  Projected (UTM)  →  Local (Bevy meters, Y-up)
//!   lat/lon degrees       easting/northing      x/z meters from origin
//! ```
//!
//! ## Table of Contents
//! 1. GeoOrigin — Project coordinate origin
//! 2. Equirectangular fallback (no PROJ dependency)
//! 3. PROJ-based accurate transforms (feature-gated)
//! 4. geo_to_world — Main entry point

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. GeoOrigin — Project coordinate origin
// ============================================================================

/// The geographic origin point for a geospatial project.
/// All Bevy world coordinates are meters relative to this point.
#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
pub struct GeoOrigin {
    /// Origin latitude (WGS84 degrees)
    pub lat: f64,
    /// Origin longitude (WGS84 degrees)
    pub lon: f64,
    /// Target CRS string (e.g., "EPSG:32644")
    pub target_crs: String,
    /// Pre-computed origin easting in target CRS (set after first projection)
    #[serde(skip)]
    pub origin_easting: f64,
    /// Pre-computed origin northing in target CRS (set after first projection)
    #[serde(skip)]
    pub origin_northing: f64,
    /// Whether the origin has been projected (lazy init)
    #[serde(skip)]
    pub initialized: bool,
}

impl GeoOrigin {
    /// Create a new origin from config values
    pub fn new(lat: f64, lon: f64, target_crs: &str) -> Self {
        let mut origin = Self {
            lat,
            lon,
            target_crs: target_crs.to_string(),
            origin_easting: 0.0,
            origin_northing: 0.0,
            initialized: false,
        };
        origin.initialize();
        origin
    }

    /// Initialize the origin projection (compute easting/northing of origin point)
    pub fn initialize(&mut self) {
        if self.initialized {
            return;
        }

        #[cfg(feature = "proj-transforms")]
        {
            if let Ok((easting, northing)) = proj_transform(self.lat, self.lon, &self.target_crs) {
                self.origin_easting = easting;
                self.origin_northing = northing;
                self.initialized = true;
                tracing::info!(
                    "GeoOrigin initialized via PROJ: ({}, {}) → ({:.1}, {:.1}) in {}",
                    self.lat, self.lon, easting, northing, self.target_crs
                );
                return;
            }
        }

        // Fallback: equirectangular (origin is 0,0 by definition)
        self.origin_easting = 0.0;
        self.origin_northing = 0.0;
        self.initialized = true;
        tracing::info!(
            "GeoOrigin initialized via equirectangular fallback: ({}, {})",
            self.lat, self.lon
        );
    }
}

impl From<&crate::config::GeoConfig> for GeoOrigin {
    fn from(config: &crate::config::GeoConfig) -> Self {
        Self::new(
            config.project.origin_lat,
            config.project.origin_lon,
            &config.project.target_crs,
        )
    }
}

// ============================================================================
// 2. Equirectangular fallback (no PROJ dependency)
// ============================================================================

/// Earth radius in meters (WGS84 mean)
const EARTH_RADIUS: f64 = 6_371_000.0;

/// Approximate geographic → local meters using equirectangular projection.
/// Accurate within ~0.5% for areas < 500 km from origin at mid-latitudes.
pub fn equirectangular_to_local(lat: f64, lon: f64, origin: &GeoOrigin) -> (f64, f64) {
    let lat_rad = lat.to_radians();
    let origin_lat_rad = origin.lat.to_radians();

    // X = east-west distance (scaled by cos of latitude)
    let x = (lon - origin.lon).to_radians() * EARTH_RADIUS * origin_lat_rad.cos();
    // Z = north-south distance
    let z = (lat - origin.lat).to_radians() * EARTH_RADIUS;

    let _ = lat_rad; // Suppress unused warning; kept for clarity
    (x, z)
}

// ============================================================================
// 3. PROJ-based accurate transforms (feature-gated)
// ============================================================================

/// Transform WGS84 lat/lon to projected easting/northing using PROJ.
/// Returns (easting, northing) in the target CRS.
#[cfg(feature = "proj-transforms")]
pub fn proj_transform(lat: f64, lon: f64, target_crs: &str) -> Result<(f64, f64), ProjError> {
    use proj::Proj;

    let transformer = Proj::new_known_crs("EPSG:4326", target_crs, None)
        .map_err(|e| ProjError::Init(format!("{}", e)))?;

    // PROJ expects (lon, lat) order for EPSG:4326
    let result = transformer
        .convert((lon, lat))
        .map_err(|e| ProjError::Transform(format!("{}", e)))?;

    Ok(result)
}

/// Errors from PROJ transforms
#[cfg(feature = "proj-transforms")]
#[derive(Debug)]
pub enum ProjError {
    /// Failed to initialize PROJ transformer
    Init(String),
    /// Failed to transform coordinates
    Transform(String),
}

#[cfg(feature = "proj-transforms")]
impl std::fmt::Display for ProjError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjError::Init(e) => write!(f, "PROJ init error: {}", e),
            ProjError::Transform(e) => write!(f, "PROJ transform error: {}", e),
        }
    }
}

// ============================================================================
// 4. geo_to_world — Main entry point
// ============================================================================

/// Transform geographic coordinates (WGS84 lat/lon) to Bevy world position.
///
/// Uses PROJ if available (feature `proj-transforms`), otherwise falls back
/// to equirectangular approximation.
///
/// Axis mapping: GIS (X=East, Y=North) → Bevy (X=East, Y=Up, Z=South)
pub fn geo_to_world(lat: f64, lon: f64, elevation: f32, origin: &GeoOrigin) -> Vec3 {
    #[cfg(feature = "proj-transforms")]
    {
        if let Ok((easting, northing)) = proj_transform(lat, lon, &origin.target_crs) {
            let x = (easting - origin.origin_easting) as f32;
            // Negate: GIS North (positive northing) → Bevy -Z
            let z = -((northing - origin.origin_northing) as f32);
            return Vec3::new(x, elevation, z);
        }
    }

    // Fallback: equirectangular
    let (x, north) = equirectangular_to_local(lat, lon, origin);
    Vec3::new(x as f32, elevation, -(north as f32))
}

/// Transform a GeoJSON [lon, lat] or [lon, lat, alt] coordinate to Bevy world position
pub fn geojson_coord_to_world(coord: &[f64], origin: &GeoOrigin) -> Vec3 {
    let lon = coord[0];
    let lat = coord[1];
    let alt = if coord.len() > 2 { coord[2] as f32 } else { 0.0 };
    geo_to_world(lat, lon, alt, origin)
}
