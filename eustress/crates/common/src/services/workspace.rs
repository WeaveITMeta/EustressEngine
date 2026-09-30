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
// Units
// ============================================================================

/// The unit `Workspace.gravity` is authored/stored in.
///
/// The engine is meter-native (see [`crate::units::ENGINE_NATIVE_UNIT`]), so
/// gravity is stored in m/s² and the SI default `-9.80665 m/s²` needs no
/// conversion before it reaches Avian. This constant is the single source of
/// truth the canonical [`sync_workspace_gravity_to_avian`] system reads when
/// converting through [`crate::units`]; change it here and every gravity-sync
/// path converts correctly.
pub const GRAVITY_AUTHORED_UNIT: crate::units::Unit = crate::units::ENGINE_NATIVE_UNIT;

/// The Workspace's default gravity, m/s²: standard gravity, straight down.
pub const DEFAULT_GRAVITY: Vec3 = Vec3::new(0.0, -crate::units::STANDARD_GRAVITY_F32, 0.0);

/// The gravity to simulate with, m/s²: the Workspace's, which Avian's
/// `Gravity` follows, or [`DEFAULT_GRAVITY`] in a host with no Workspace.
///
/// For code that moves things without Avian (realism particles, buoyancy),
/// so they fall and float under the same gravity as the parts around them,
/// including when a script or tool changes it mid-game.
pub fn live_gravity(workspace: Option<&Workspace>) -> Vec3 {
    workspace.map_or(DEFAULT_GRAVITY, |ws| ws.gravity)
}

/// A Workspace service file's `gravity` key, as written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthoredGravity {
    /// A vector in m/s², the key's format: `gravity = [0.0, -9.80665, 0.0]`.
    Vector([f64; 3]),
    /// A stud-era value: a bare number, or a vector of one of
    /// [`LEGACY_STUD_GRAVITIES`]. Its magnitude, in studs/s².
    Legacy(f64),
}

/// The gravities stud-era files wrote, studs/s²: Roblox's 196.2 and the
/// studs-native build's 196.8. Files carry them as bare numbers and as
/// vectors (`[0.0, -196.2, 0.0]`); no gravity written in m/s² is near them.
pub const LEGACY_STUD_GRAVITIES: [f64; 2] = [196.2, 196.8];

impl AuthoredGravity {
    /// Read a vector-valued `gravity`: m/s² as written, unless its magnitude
    /// is one of [`LEGACY_STUD_GRAVITIES`].
    pub fn from_vector(v: [f64; 3]) -> Self {
        let magnitude = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if LEGACY_STUD_GRAVITIES.iter().any(|g| (magnitude - g).abs() < 0.01) {
            AuthoredGravity::Legacy(magnitude)
        } else {
            AuthoredGravity::Vector(v)
        }
    }
}

/// The gravity a Space runs at, m/s², from its Workspace service file.
///
/// A vector is taken as written. A stud-era value ([`AuthoredGravity::Legacy`])
/// is read by where it came from. In a Roblox import it is Roblox's gravity in
/// studs/s², scaled by the stud that import's lengths use
/// (`roblox_import_stud`), so falls match the imported geometry. In any other
/// Space nothing ever read the value, so the Space runs at standard gravity,
/// as it always has. No key, or a value that is not finite, is standard
/// gravity too. Studio and the Player both apply a Space's gravity through
/// this, so the two agree.
pub fn authored_gravity(key: Option<AuthoredGravity>, roblox_import_stud: Option<crate::units::Unit>) -> Vec3 {
    let gravity = match key {
        Some(AuthoredGravity::Vector([x, y, z])) => Vec3::new(x as f32, y as f32, z as f32),
        Some(AuthoredGravity::Legacy(studs)) => match roblox_import_stud {
            Some(stud) => Vec3::new(0.0, -((studs * stud.to_meters()) as f32), 0.0),
            None => DEFAULT_GRAVITY,
        },
        None => DEFAULT_GRAVITY,
    };
    if gravity.is_finite() { gravity } else { DEFAULT_GRAVITY }
}

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
    ///
    /// NOTE: `scene::WorkspaceSettings.gravity` is a DIFFERENT field, authored in
    /// legacy studs (`units::Unit::LegacyStud`, default 196.8). Convert it through
    /// [`crate::units`] before assigning it here: [`sync_workspace_gravity_to_avian`]
    /// converts from meters and would pass a studs value straight through as a
    /// 20x error.
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
            // Physics: exact standard gravity, 9.80665 m/s² (Space Grade Ready)
            gravity: DEFAULT_GRAVITY,
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

/// Canonical gravity-sync system: copy `Workspace.gravity` into Avian's
/// `Gravity` resource, converting through [`crate::units`] from the authored
/// unit ([`GRAVITY_AUTHORED_UNIT`]) to engine-native meters.
///
/// This is the only system that writes Avian's `Gravity` during the frame.
/// Studio schedules it from `WorkspacePlugin`; every other gravity author
/// (scene load, the Rune `workspace_set_gravity` binding) writes
/// `Workspace.gravity` and lets this system perform the conversion. One-shot
/// startup inserts of `Gravity` remain, and this system converges them on the
/// first frame that `Workspace.gravity` differs.
///
/// Runs only when an `avian3d::prelude::Gravity` resource exists, so it is
/// harmless in editor/headless builds that never inserted one.
///
/// Gated on common's existing `physics` feature (which pulls `avian3d`); there
/// is intentionally no separate `avian` feature.
#[cfg(feature = "physics")]
pub fn sync_workspace_gravity_to_avian(
    workspace: Option<Res<Workspace>>,
    gravity: Option<ResMut<avian3d::prelude::Gravity>>,
) {
    let (Some(ws), Some(mut gravity)) = (workspace, gravity) else { return };
    // Authored unit → engine-native meters. Identity (zero-cost) while
    // GRAVITY_AUTHORED_UNIT == ENGINE_NATIVE_UNIT, but routes through `units`
    // so a future studs-authored gravity converts automatically.
    let converted = crate::units::accel_to_engine_vec3_f32(
        ws.gravity.to_array(),
        GRAVITY_AUTHORED_UNIT,
    );
    let converted = Vec3::from_array(converted);
    if gravity.0 != converted {
        gravity.0 = converted;
        info!(
            "Avian gravity set to {:?} m/s² (from Workspace.gravity {:?} {})",
            converted, ws.gravity, GRAVITY_AUTHORED_UNIT.symbol()
        );
    }
}

impl Workspace {
    /// Create with custom gravity (in m/s² — engine-native; see
    /// [`GRAVITY_AUTHORED_UNIT`]).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Unit;

    #[test]
    fn a_vector_is_taken_as_written() {
        let moon = authored_gravity(Some(AuthoredGravity::Vector([0.0, -1.62, 0.0])), None);
        assert_eq!(moon, Vec3::new(0.0, -1.62, 0.0));
        // The Space's own gravity wins over the import rule.
        let written = authored_gravity(Some(AuthoredGravity::Vector([0.0, -54.936, 0.0])), Some(Unit::Foot));
        assert_eq!(written, Vec3::new(0.0, -54.936, 0.0));
    }

    /// Every native Space holds the old template's 196.2, which nothing read:
    /// it keeps running at standard gravity.
    #[test]
    fn a_native_spaces_old_number_is_standard_gravity() {
        assert_eq!(authored_gravity(Some(AuthoredGravity::Legacy(196.2)), None), DEFAULT_GRAVITY);
        assert_eq!(authored_gravity(None, None), DEFAULT_GRAVITY);
    }

    /// An older Roblox import holds Roblox's gravity in studs/s², read in the
    /// stud its lengths use: feet for every import made so far.
    #[test]
    fn an_old_roblox_import_falls_like_roblox_at_its_scale() {
        let feet = authored_gravity(Some(AuthoredGravity::Legacy(196.2)), Some(Unit::Foot));
        assert!((feet.y + 196.2 * 0.3048).abs() < 1e-3, "{feet:?}");
        let studs = authored_gravity(Some(AuthoredGravity::Legacy(196.2)), Some(Unit::Stud));
        assert!((studs.y + 196.2 * 0.28).abs() < 1e-3, "{studs:?}");
    }

    /// Sixteen Tucson Spaces and two more hold the stud-era default as a
    /// vector, `[0.0, -196.2, 0.0]`: read as the stud-era value it is, never
    /// as 196.2 m/s².
    #[test]
    fn a_stud_era_vector_is_legacy() {
        let tucson = AuthoredGravity::from_vector([0.0, -196.2, 0.0]);
        assert_eq!(tucson, AuthoredGravity::Legacy(196.2));
        assert_eq!(authored_gravity(Some(tucson), None), DEFAULT_GRAVITY);
        assert_eq!(AuthoredGravity::from_vector([0.0, -196.8, 0.0]), AuthoredGravity::Legacy(196.8));
        let imported = authored_gravity(Some(AuthoredGravity::from_vector([0.0, -196.2, 0.0])), Some(Unit::Foot));
        assert!((imported.y + 196.2 * 0.3048).abs() < 1e-3, "{imported:?}");
        // Metric vectors, Roblox's at 0.28 m studs included, are as written.
        assert_eq!(AuthoredGravity::from_vector([0.0, -54.936, 0.0]), AuthoredGravity::Vector([0.0, -54.936, 0.0]));
        assert_eq!(AuthoredGravity::from_vector([0.0, 0.0, 0.0]), AuthoredGravity::Vector([0.0, 0.0, 0.0]));
    }

    #[test]
    fn a_value_that_is_not_finite_is_standard_gravity() {
        assert_eq!(authored_gravity(Some(AuthoredGravity::Vector([0.0, f64::NAN, 0.0])), None), DEFAULT_GRAVITY);
        assert_eq!(authored_gravity(Some(AuthoredGravity::Legacy(f64::INFINITY)), Some(Unit::Foot)), DEFAULT_GRAVITY);
    }
}
