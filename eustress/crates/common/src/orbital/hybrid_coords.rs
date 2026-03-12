//! # Hybrid Vec3/DVec3 Coordinate System
//!
//! Automatic precision switching for solar system-scale worlds.
//! Uses f32 Vec3 for nearby objects (performance), DVec3 f64 for distant objects (precision).
//!
//! ## Design Philosophy
//!
//! - **Near objects** (<100km): Vec3 f32 for Bevy/Avian physics (fast)
//! - **Far objects** (>100km): DVec3 f64 for orbital mechanics (accurate)
//! - **Automatic switching**: Based on distance from camera/focus
//! - **Zero-copy rendering**: Convert DVec3 → Vec3 only for visible objects
//!
//! ## Integration with Existing Orbital Grid
//!
//! This extends the existing `OrbitalCoords` system with automatic precision management.

use bevy::prelude::*;
use bevy::math::DVec3;

// ============================================================================
// Constants
// ============================================================================

/// Distance threshold for switching to DVec3 (100km)
pub const PRECISION_THRESHOLD: f64 = 100_000.0;

/// Maximum safe f32 distance from origin (~16km for 1mm precision)
pub const F32_SAFE_DISTANCE: f32 = 16_000.0;

/// Astronomical Unit in meters (Earth-Sun distance)
pub const AU: f64 = 149_597_870_700.0;

/// Light-year in meters
pub const LIGHT_YEAR: f64 = 9.4607e15;

// ============================================================================
// HybridPosition - Automatic Vec3/DVec3 Switching
// ============================================================================

/// Position that automatically uses Vec3 or DVec3 based on scale
#[derive(Debug, Clone, Copy, Component, Reflect)]
pub struct HybridPosition {
    /// High-precision absolute position (always f64)
    pub absolute: DVec3,
    
    /// Cached relative position for rendering (f32, relative to focus)
    #[reflect(ignore)]
    pub relative: Vec3,
    
    /// Whether this position is currently using high precision
    pub use_high_precision: bool,
}

impl HybridPosition {
    /// Create from DVec3 (high precision)
    pub fn from_dvec3(pos: DVec3) -> Self {
        Self {
            absolute: pos,
            relative: Vec3::ZERO,
            use_high_precision: true,
        }
    }
    
    /// Create from Vec3 (low precision, assumes near origin)
    pub fn from_vec3(pos: Vec3) -> Self {
        Self {
            absolute: pos.as_dvec3(),
            relative: pos,
            use_high_precision: false,
        }
    }
    
    /// Create from geodetic coordinates (lat, lon, alt)
    pub fn from_geodetic(lat: f64, lon: f64, alt: f64) -> Self {
        use super::wgs84::geodetic_to_ecef;
        let ecef = geodetic_to_ecef(lat, lon, alt);
        Self::from_dvec3(ecef)
    }
    
    /// Distance from origin
    pub fn magnitude(&self) -> f64 {
        self.absolute.length()
    }
    
    /// Distance to another position
    pub fn distance_to(&self, other: &HybridPosition) -> f64 {
        (self.absolute - other.absolute).length()
    }
    
    /// Update relative position from focus point
    pub fn update_relative(&mut self, focus: &HybridPosition) {
        let delta = self.absolute - focus.absolute;
        let distance = delta.length();
        
        // Switch precision mode based on distance
        self.use_high_precision = distance > PRECISION_THRESHOLD;
        
        // Convert to f32 for rendering
        self.relative = delta.as_vec3();
    }
    
    /// Get render position (always Vec3)
    pub fn render_position(&self) -> Vec3 {
        self.relative
    }
    
    /// Add offset in local space
    pub fn add_local(&mut self, offset: Vec3) {
        self.absolute += offset.as_dvec3();
        self.relative += offset;
    }
}

impl Default for HybridPosition {
    fn default() -> Self {
        Self::from_vec3(Vec3::ZERO)
    }
}

// ============================================================================
// HybridVelocity - Velocity with Precision Tracking
// ============================================================================

/// Velocity that maintains precision for high-speed orbital mechanics
#[derive(Debug, Clone, Copy, Component, Reflect)]
pub struct HybridVelocity {
    /// High-precision velocity (m/s)
    pub absolute: DVec3,
    
    /// Cached local velocity for physics (f32)
    #[reflect(ignore)]
    pub local: Vec3,
}

impl HybridVelocity {
    pub fn from_dvec3(vel: DVec3) -> Self {
        Self {
            absolute: vel,
            local: vel.as_vec3(),
        }
    }
    
    pub fn from_vec3(vel: Vec3) -> Self {
        Self {
            absolute: vel.as_dvec3(),
            local: vel,
        }
    }
    
    /// Orbital velocity at given altitude above Earth
    pub fn orbital_velocity(altitude: f64) -> Self {
        use super::wgs84::{EARTH_MEAN_RADIUS, EARTH_GM};
        let r = EARTH_MEAN_RADIUS + altitude;
        let v = (EARTH_GM / r).sqrt();
        Self::from_dvec3(DVec3::new(v, 0.0, 0.0))
    }
    
    /// Update local velocity (for physics integration)
    pub fn update_local(&mut self) {
        self.local = self.absolute.as_vec3();
    }
    
    /// Magnitude in m/s
    pub fn speed(&self) -> f64 {
        self.absolute.length()
    }
}

impl Default for HybridVelocity {
    fn default() -> Self {
        Self::from_vec3(Vec3::ZERO)
    }
}

// ============================================================================
// Focus Tracking
// ============================================================================

/// Marker component for the camera/player focus point
#[derive(Component, Debug, Default)]
pub struct HybridFocus;

/// Resource tracking the current focus position
#[derive(Resource, Debug)]
pub struct FocusPosition {
    pub position: HybridPosition,
    pub velocity: HybridVelocity,
}

impl Default for FocusPosition {
    fn default() -> Self {
        Self {
            position: HybridPosition::default(),
            velocity: HybridVelocity::default(),
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Update focus position from marked entity
pub fn update_focus_position(
    focus_query: Query<(&HybridPosition, Option<&HybridVelocity>), With<HybridFocus>>,
    mut focus_res: ResMut<FocusPosition>,
) {
    if let Ok((pos, vel)) = focus_query.single() {
        focus_res.position = *pos;
        if let Some(v) = vel {
            focus_res.velocity = *v;
        }
    }
}

/// Update all relative positions based on focus
pub fn update_relative_positions(
    focus: Res<FocusPosition>,
    mut query: Query<&mut HybridPosition, Without<HybridFocus>>,
) {
    for mut pos in query.iter_mut() {
        pos.update_relative(&focus.position);
    }
}

/// Sync hybrid positions to Bevy transforms for rendering
pub fn sync_hybrid_to_transform(
    mut query: Query<(&HybridPosition, &mut Transform), Changed<HybridPosition>>,
) {
    for (hybrid_pos, mut transform) in query.iter_mut() {
        transform.translation = hybrid_pos.render_position();
    }
}

/// Integrate velocity into position (high-precision physics)
pub fn integrate_hybrid_motion(
    time: Res<Time>,
    mut query: Query<(&mut HybridPosition, &HybridVelocity)>,
) {
    let dt = time.delta_secs_f64();
    
    for (mut pos, vel) in query.iter_mut() {
        // High-precision integration
        pos.absolute += vel.absolute * dt;
    }
}

// ============================================================================
// Solar System Presets
// ============================================================================

/// Solar system body definitions
#[derive(Debug, Clone)]
pub struct SolarBody {
    pub name: &'static str,
    pub mass: f64,              // kg
    pub radius: f64,            // m
    pub orbit_radius: f64,      // m (from parent)
    pub orbital_period: f64,    // seconds
}

impl SolarBody {
    pub const EARTH: Self = Self {
        name: "Earth",
        mass: 5.972e24,
        radius: 6.371e6,
        orbit_radius: AU,
        orbital_period: 365.25 * 24.0 * 3600.0,
    };
    
    pub const MOON: Self = Self {
        name: "Moon",
        mass: 7.342e22,
        radius: 1.737e6,
        orbit_radius: 384_400_000.0,
        orbital_period: 27.3 * 24.0 * 3600.0,
    };
    
    pub const SUN: Self = Self {
        name: "Sun",
        mass: 1.989e30,
        radius: 6.96e8,
        orbit_radius: 0.0,
        orbital_period: 0.0,
    };
    
    pub const MARS: Self = Self {
        name: "Mars",
        mass: 6.39e23,
        radius: 3.39e6,
        orbit_radius: 1.524 * AU,
        orbital_period: 687.0 * 24.0 * 3600.0,
    };
    
    /// Surface gravity (m/s²)
    pub fn surface_gravity(&self) -> f64 {
        const G: f64 = 6.67430e-11;
        G * self.mass / (self.radius * self.radius)
    }
    
    /// Escape velocity (m/s)
    pub fn escape_velocity(&self) -> f64 {
        const G: f64 = 6.67430e-11;
        (2.0 * G * self.mass / self.radius).sqrt()
    }
    
    /// Orbital velocity at distance r from center
    pub fn orbital_velocity(&self, r: f64) -> f64 {
        const G: f64 = 6.67430e-11;
        (G * self.mass / r).sqrt()
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Check if position requires high precision
pub fn requires_high_precision(pos: &HybridPosition, focus: &HybridPosition) -> bool {
    pos.distance_to(focus) > PRECISION_THRESHOLD
}

/// Convert AU to meters
pub const fn au_to_meters(au: f64) -> f64 {
    au * AU
}

/// Convert meters to AU
pub const fn meters_to_au(meters: f64) -> f64 {
    meters / AU
}

/// Format distance with appropriate units
pub fn format_distance(meters: f64) -> String {
    if meters < 1000.0 {
        format!("{:.1} m", meters)
    } else if meters < 1_000_000.0 {
        format!("{:.2} km", meters / 1000.0)
    } else if meters < AU {
        format!("{:.0} km", meters / 1000.0)
    } else if meters < LIGHT_YEAR {
        format!("{:.3} AU", meters / AU)
    } else {
        format!("{:.2} ly", meters / LIGHT_YEAR)
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Plugin for hybrid coordinate system
pub struct HybridCoordsPlugin;

impl Plugin for HybridCoordsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FocusPosition>()
            .register_type::<HybridPosition>()
            .register_type::<HybridVelocity>()
            .add_systems(Update, (
                update_focus_position,
                update_relative_positions.after(update_focus_position),
                sync_hybrid_to_transform.after(update_relative_positions),
                integrate_hybrid_motion,
            ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_precision_switching() {
        let focus = HybridPosition::from_vec3(Vec3::ZERO);
        
        // Near object (should use f32)
        let mut near = HybridPosition::from_vec3(Vec3::new(1000.0, 0.0, 0.0));
        near.update_relative(&focus);
        assert!(!near.use_high_precision);
        
        // Far object (should use f64)
        let mut far = HybridPosition::from_dvec3(DVec3::new(200_000.0, 0.0, 0.0));
        far.update_relative(&focus);
        assert!(far.use_high_precision);
    }
    
    #[test]
    fn test_solar_bodies() {
        assert_eq!(SolarBody::EARTH.surface_gravity().round(), 10.0);
        assert!(SolarBody::EARTH.escape_velocity() > 11_000.0);
        
        // ISS orbital velocity
        let iss_altitude = 400_000.0;
        let v = SolarBody::EARTH.orbital_velocity(SolarBody::EARTH.radius + iss_altitude);
        assert!((v - 7660.0).abs() < 100.0); // ~7.66 km/s
    }
    
    #[test]
    fn test_distance_formatting() {
        assert_eq!(format_distance(500.0), "500.0 m");
        assert_eq!(format_distance(5_000.0), "5.00 km");
        assert_eq!(format_distance(AU), "1.000 AU");
    }
}
