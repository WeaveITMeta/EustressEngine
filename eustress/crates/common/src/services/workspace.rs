//! # Workspace Service
//! 
//! Runtime state for the scene hierarchy (like Eustress's Workspace service).
//! Contains world-level physics, bounds, and anti-exploit configuration.
//! 
//! For class definitions (Instance, Part, Model, etc.), see `crate::classes`.
//! 
//! # Network Integration
//! 
//! The networking layer reads from this service for:
//! - Physics validation (`max_entity_speed`, `teleport_threshold`)
//! - World bounds (`world_bounds` for AOI culling)
//! - Gravity application (`gravity`)

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// World Scale Constants (Unit Conversions Only)
// ============================================================================

/// Legacy constant - Eustress uses meters natively (1 unit = 1 meter)
pub const STUD_TO_METERS: f32 = 1.0;

/// Legacy constant - Eustress uses meters natively
pub const METERS_TO_STUDS: f32 = 1.0;

// ============================================================================
// Workspace Resource
// ============================================================================

/// Workspace - runtime state for the scene (like Eustress's Workspace service)
/// 
/// # Serialization
/// This resource is serialized with scenes, allowing per-scene physics tuning.
/// 
/// # Example
/// ```rust,ignore
/// // In scene RON file:
/// workspace: (
///     gravity: (0.0, -35.0, 0.0),
///     max_entity_speed: 100.0,
///     world_bounds: (min: (-50000, -1000, -50000), max: (50000, 10000, 50000)),
/// )
/// ```
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct Workspace {
    // === Physics Properties ===
    
    /// Gravity vector in m/s² (default: -9.80665 Y, exact SI standard gravity)
    /// Networking: Applied to all dynamic RigidBodies
    /// Note: This is the base gravity at sea level (Y=0). Use altitude_gravity() for altitude-adjusted values.
    pub gravity: Vec3,
    
    /// Maximum allowed entity speed in m/s (anti-exploit)
    /// Networking: Server rejects velocities exceeding this
    pub max_entity_speed: f32,
    
    /// Maximum position delta per tick before flagging as teleport (meters)
    /// Networking: Triggers validation on large movements
    pub teleport_threshold: f32,
    
    /// Maximum acceleration in m/s² (anti-exploit)
    /// Networking: Server validates acceleration doesn't exceed this
    pub max_acceleration: f32,
    
    // === World Bounds ===
    
    /// World bounding box (min corner) in meters
    /// Networking: Used for AOI spatial hashing
    pub world_bounds_min: Vec3,
    
    /// World bounding box (max corner) in meters
    /// Networking: Used for AOI spatial hashing
    pub world_bounds_max: Vec3,
    
    /// Fall height before respawn (meters, negative Y)
    pub fall_height: f32,
    
    // === Streaming ===
    
    /// Enable streaming for large worlds
    pub streaming_enabled: bool,
    
    /// Streaming target radius (meters)
    pub streaming_target_radius: f32,
    
    /// Streaming min radius (meters)
    pub streaming_min_radius: f32,
    
    // === Runtime State (not serialized) ===
    
    /// Current camera entity
    #[serde(skip)]
    pub current_camera: Option<Entity>,
    
    /// Terrain entity (if any)
    #[serde(skip)]
    pub terrain: Option<Entity>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            // Physics: Exact SI standard gravity 9.80665 m/s² (Space Grade Ready)
            gravity: Vec3::new(0.0, -9.80665, 0.0),
            max_entity_speed: 100.0,        // m/s (very fast)
            teleport_threshold: 50.0,       // m/tick (flags large jumps)
            max_acceleration: 50.0,         // m/s²
            
            // World bounds: ±10 km
            world_bounds_min: Vec3::new(-10_000.0, -1_000.0, -10_000.0),
            world_bounds_max: Vec3::new(10_000.0, 5_000.0, 10_000.0),
            fall_height: -500.0,
            
            // Streaming
            streaming_enabled: false,
            streaming_target_radius: 1024.0,
            streaming_min_radius: 64.0,
            
            // Runtime
            current_camera: None,
            terrain: None,
        }
    }
}

impl Workspace {
    /// Create with custom gravity (in studs/s²)
    pub fn with_gravity(mut self, gravity: Vec3) -> Self {
        self.gravity = gravity;
        self
    }
    
    /// Create with custom speed limits
    pub fn with_speed_limits(mut self, max_speed: f32, max_accel: f32) -> Self {
        self.max_entity_speed = max_speed;
        self.max_acceleration = max_accel;
        self
    }
    
    /// Create with custom world bounds
    pub fn with_bounds(mut self, min: Vec3, max: Vec3) -> Self {
        self.world_bounds_min = min;
        self.world_bounds_max = max;
        self
    }
    
    /// Check if a position is within world bounds
    pub fn is_in_bounds(&self, position: Vec3) -> bool {
        position.x >= self.world_bounds_min.x && position.x <= self.world_bounds_max.x
            && position.y >= self.world_bounds_min.y && position.y <= self.world_bounds_max.y
            && position.z >= self.world_bounds_min.z && position.z <= self.world_bounds_max.z
    }
    
    /// Check if a velocity is within allowed limits
    pub fn is_valid_velocity(&self, velocity: Vec3) -> bool {
        velocity.length() <= self.max_entity_speed
    }
    
    /// Check if a position delta is a potential teleport
    pub fn is_teleport(&self, delta: Vec3) -> bool {
        delta.length() > self.teleport_threshold
    }
    
    /// Get gravity in meters/s² (for physics engines using SI units)
    pub fn gravity_meters(&self) -> Vec3 {
        self.gravity * STUD_TO_METERS
    }
    
    /// Get world extent (half-size) for spatial hashing
    pub fn world_extent(&self) -> f32 {
        let size = self.world_bounds_max - self.world_bounds_min;
        size.x.max(size.y).max(size.z) / 2.0
    }
    
    /// Calculate altitude-adjusted gravity using real orbital mechanics.
    /// 
    /// Uses Newton's law of universal gravitation: g(h) = G × M / (R + h)²
    /// where:
    /// - G = 6.67430×10⁻¹¹ m³/(kg·s²) (gravitational constant)
    /// - M = 5.972×10²⁴ kg (Earth mass)
    /// - R = 6.371×10⁶ m (Earth radius)
    /// - h = altitude in meters (Y position, where Y=0 is sea level)
    /// 
    /// This makes Eustress Engine "Space Grade Ready" with physically accurate
    /// gravity that decreases with altitude.
    /// 
    /// # Arguments
    /// * `altitude` - Height above sea level in meters (Y coordinate)
    /// 
    /// # Returns
    /// Gravity magnitude in m/s² at the given altitude
    pub fn altitude_gravity(&self, altitude: f32) -> f32 {
        // Physical constants (SI units)
        const G: f64 = 6.67430e-11;           // Gravitational constant m³/(kg·s²)
        const EARTH_MASS: f64 = 5.972e24;     // Earth mass in kg
        const EARTH_RADIUS: f64 = 6.371e6;    // Earth radius in meters
        
        // g = G × M / (R + h)²
        let r = EARTH_RADIUS + (altitude.max(0.0) as f64);
        let g = G * EARTH_MASS / (r * r);
        
        g as f32
    }
    
    /// Get gravity vector adjusted for altitude (Y position).
    /// 
    /// Returns a gravity vector pointing downward (-Y) with magnitude
    /// calculated using real orbital mechanics.
    /// 
    /// # Arguments
    /// * `y_position` - Y coordinate in world space (0 = sea level)
    pub fn gravity_at_altitude(&self, y_position: f32) -> Vec3 {
        let g_magnitude = self.altitude_gravity(y_position);
        Vec3::new(0.0, -g_magnitude, 0.0)
    }
}

// ============================================================================
// Special Container Markers
// ============================================================================

/// ServerStorage marker - server-only objects
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ServerStorage;

/// ReplicatedStorage marker - shared objects
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct ReplicatedStorage;

/// StarterPack marker - default player tools
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct StarterPack;

/// StarterGui marker - default player UI
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct StarterGui;

/// StarterPlayer marker - player defaults
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct StarterPlayer;
