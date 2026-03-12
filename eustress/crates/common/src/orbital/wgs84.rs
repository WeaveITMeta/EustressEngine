//! # WGS84/ECEF Coordinate Conversions
//!
//! World Geodetic System 1984 (WGS84) coordinate transformations.
//! Converts between geodetic (lat/lon/alt) and ECEF (Earth-Centered Earth-Fixed) coordinates.

use bevy::math::DVec3;

// WGS84 ellipsoid parameters
pub const WGS84_A: f64 = 6378137.0;           // Semi-major axis (meters)
pub const WGS84_B: f64 = 6356752.314245;      // Semi-minor axis (meters)
pub const WGS84_F: f64 = 1.0 / 298.257223563; // Flattening
pub const WGS84_E2: f64 = 0.00669437999014;   // First eccentricity squared

// Earth constants
pub const EARTH_MEAN_RADIUS: f64 = 6371000.0;
pub const EARTH_GM: f64 = 3.986004418e14; // Gravitational parameter (m³/s²)

/// Convert geodetic coordinates to ECEF
///
/// # Arguments
/// * `lat` - Latitude in degrees (-90 to 90)
/// * `lon` - Longitude in degrees (-180 to 180)
/// * `alt` - Altitude above WGS84 ellipsoid in meters
///
/// # Returns
/// ECEF coordinates as DVec3 (x, y, z) in meters
pub fn geodetic_to_ecef(lat: f64, lon: f64, alt: f64) -> DVec3 {
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();
    
    // Radius of curvature in prime vertical
    let n = WGS84_A / (1.0 - WGS84_E2 * lat_rad.sin().powi(2)).sqrt();
    
    let x = (n + alt) * lat_rad.cos() * lon_rad.cos();
    let y = (n + alt) * lat_rad.cos() * lon_rad.sin();
    let z = (n * (1.0 - WGS84_E2) + alt) * lat_rad.sin();
    
    DVec3::new(x, y, z)
}

/// Convert ECEF coordinates to geodetic
///
/// Uses Bowring's method for iterative conversion.
///
/// # Arguments
/// * `ecef` - ECEF coordinates (x, y, z) in meters
///
/// # Returns
/// Tuple of (latitude, longitude, altitude) in degrees and meters
pub fn ecef_to_geodetic(ecef: DVec3) -> (f64, f64, f64) {
    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;
    
    let p = (x * x + y * y).sqrt();
    let lon = y.atan2(x);
    
    // Initial latitude estimate
    let mut lat = (z / p).atan();
    
    // Iterative refinement (Bowring's method)
    for _ in 0..5 {
        let n = WGS84_A / (1.0 - WGS84_E2 * lat.sin().powi(2)).sqrt();
        let alt = p / lat.cos() - n;
        lat = (z / p / (1.0 - WGS84_E2 * n / (n + alt))).atan();
    }
    
    let n = WGS84_A / (1.0 - WGS84_E2 * lat.sin().powi(2)).sqrt();
    let alt = p / lat.cos() - n;
    
    (lat.to_degrees(), lon.to_degrees(), alt)
}

/// Calculate great-circle distance between two points (Haversine formula)
///
/// # Arguments
/// * `lat1`, `lon1` - First point in degrees
/// * `lat2`, `lon2` - Second point in degrees
///
/// # Returns
/// Distance in meters
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let a = (dlat / 2.0).sin().powi(2) +
            lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    
    EARTH_MEAN_RADIUS * c
}

/// Calculate bearing from point 1 to point 2
///
/// # Returns
/// Bearing in degrees (0-360, where 0 is north)
pub fn bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlon = (lon2 - lon1).to_radians();
    
    let y = dlon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() -
            lat1_rad.sin() * lat2_rad.cos() * dlon.cos();
    
    let bearing_rad = y.atan2(x);
    (bearing_rad.to_degrees() + 360.0) % 360.0
}

/// Calculate destination point given start, bearing, and distance
///
/// # Arguments
/// * `lat`, `lon` - Start point in degrees
/// * `bearing` - Bearing in degrees
/// * `distance` - Distance in meters
///
/// # Returns
/// Tuple of (latitude, longitude) in degrees
pub fn destination_point(lat: f64, lon: f64, bearing: f64, distance: f64) -> (f64, f64) {
    let lat_rad = lat.to_radians();
    let lon_rad = lon.to_radians();
    let bearing_rad = bearing.to_radians();
    let angular_distance = distance / EARTH_MEAN_RADIUS;
    
    let lat2 = (lat_rad.sin() * angular_distance.cos() +
                lat_rad.cos() * angular_distance.sin() * bearing_rad.cos()).asin();
    
    let lon2 = lon_rad + (bearing_rad.sin() * angular_distance.sin() * lat_rad.cos())
                        .atan2(angular_distance.cos() - lat_rad.sin() * lat2.sin());
    
    (lat2.to_degrees(), lon2.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_geodetic_ecef_roundtrip() {
        let lat = 37.7749;
        let lon = -122.4194;
        let alt = 100.0;
        
        let ecef = geodetic_to_ecef(lat, lon, alt);
        let (lat2, lon2, alt2) = ecef_to_geodetic(ecef);
        
        assert!((lat - lat2).abs() < 1e-6);
        assert!((lon - lon2).abs() < 1e-6);
        assert!((alt - alt2).abs() < 0.1);
    }
    
    #[test]
    fn test_haversine() {
        // San Francisco to New York
        let sf_lat = 37.7749;
        let sf_lon = -122.4194;
        let ny_lat = 40.7128;
        let ny_lon = -74.0060;
        
        let distance = haversine_distance(sf_lat, sf_lon, ny_lat, ny_lon);
        
        // Should be approximately 4,130 km
        assert!((distance - 4_130_000.0).abs() < 50_000.0);
    }
    
    #[test]
    fn test_bearing() {
        let bearing_deg = bearing(51.5074, -0.1278, 48.8566, 2.3522); // London to Paris
        
        // Should be roughly southeast (around 150 degrees)
        assert!(bearing_deg > 100.0 && bearing_deg < 200.0);
    }
}
