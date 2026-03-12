//! # N-Body Gravitational Physics System
//!
//! Supports planetary-scale objects with proper gravitational calculations.
//! Implements both big G (universal gravitation) and little g (surface gravity).
//!
//! ## Features
//!
//! - Inverse square law gravity between all massive objects
//! - Force threshold filtering (>0.001 N)
//! - Support for planetary-scale meshes (Earth radius: 6.371 million meters)
//! - Spatial partitioning for efficient O(n log n) calculations
//! - Hybrid Vec3/DVec3 precision for solar system scale
//!
//! ## Physics Constants
//!
//! - **Big G**: 6.67430e-11 m³/(kg·s²) - Universal gravitational constant
//! - **Little g**: 9.81 m/s² - Earth surface gravity (derived from G, M, R)

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Physical Constants
// ============================================================================

/// Universal gravitational constant (m³/(kg·s²))
pub const G: f64 = 6.67430e-11;

/// Minimum force magnitude to apply (Newtons)
/// Forces below this threshold are ignored for performance
pub const FORCE_THRESHOLD: f64 = 0.001;

/// Speed of light (m/s) - for relativistic effects (future)
pub const C: f64 = 299_792_458.0;

// ============================================================================
// Components
// ============================================================================

/// Mass component for gravitational objects
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct Mass {
    /// Mass in kilograms
    pub kg: f64,
}

impl Mass {
    pub fn new(kg: f64) -> Self {
        Self { kg }
    }
    
    /// Earth mass (5.972e24 kg)
    pub fn earth() -> Self {
        Self { kg: 5.972e24 }
    }
    
    /// Moon mass (7.342e22 kg)
    pub fn moon() -> Self {
        Self { kg: 7.342e22 }
    }
    
    /// Sun mass (1.989e30 kg)
    pub fn sun() -> Self {
        Self { kg: 1.989e30 }
    }
    
    /// Mars mass (6.39e23 kg)
    pub fn mars() -> Self {
        Self { kg: 6.39e23 }
    }
    
    /// Human mass (~70 kg)
    pub fn human() -> Self {
        Self { kg: 70.0 }
    }
    
    /// Spacecraft mass (~1000 kg)
    pub fn spacecraft() -> Self {
        Self { kg: 1000.0 }
    }
}

/// Physical radius for gravitational calculations
#[derive(Component, Debug, Clone, Copy, Reflect)]
#[reflect(Component)]
pub struct PhysicalRadius {
    /// Radius in meters
    pub meters: f64,
}

impl PhysicalRadius {
    pub fn new(meters: f64) -> Self {
        Self { meters }
    }
    
    /// Earth radius (6.371e6 m)
    pub fn earth() -> Self {
        Self { meters: 6.371e6 }
    }
    
    /// Moon radius (1.737e6 m)
    pub fn moon() -> Self {
        Self { meters: 1.737e6 }
    }
    
    /// Sun radius (6.96e8 m)
    pub fn sun() -> Self {
        Self { meters: 6.96e8 }
    }
    
    /// Mars radius (3.39e6 m)
    pub fn mars() -> Self {
        Self { meters: 3.39e6 }
    }
    
    /// Calculate surface gravity (little g)
    pub fn surface_gravity(&self, mass: &Mass) -> f64 {
        G * mass.kg / (self.meters * self.meters)
    }
    
    /// Calculate escape velocity
    pub fn escape_velocity(&self, mass: &Mass) -> f64 {
        (2.0 * G * mass.kg / self.meters).sqrt()
    }
}

/// Gravitational force accumulator
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct GravitationalForce {
    /// Accumulated force in Newtons (Vec3 for direction)
    pub force: Vec3,
    
    /// Number of bodies contributing to this force
    pub source_count: usize,
}

impl GravitationalForce {
    pub fn reset(&mut self) {
        self.force = Vec3::ZERO;
        self.source_count = 0;
    }
    
    pub fn add_force(&mut self, force: Vec3) {
        self.force += force;
        self.source_count += 1;
    }
    
    /// Get acceleration (F = ma, so a = F/m)
    pub fn acceleration(&self, mass: &Mass) -> Vec3 {
        if mass.kg > 0.0 {
            self.force / mass.kg as f32
        } else {
            Vec3::ZERO
        }
    }
}

/// Marker for objects that generate gravity
#[derive(Component, Debug, Default)]
pub struct GravitySource;

/// Marker for objects affected by gravity
#[derive(Component, Debug, Default)]
pub struct GravityAffected;

/// Configuration for gravity calculations
#[derive(Resource, Debug, Clone)]
pub struct GravityConfig {
    /// Enable N-body gravity calculations
    pub enabled: bool,
    
    /// Force threshold (N) - forces below this are ignored
    pub force_threshold: f64,
    
    /// Maximum influence distance (m) - beyond this, gravity is ignored
    pub max_distance: f64,
    
    /// Use spatial partitioning for optimization
    pub use_spatial_partition: bool,
    
    /// Grid cell size for spatial partitioning (meters)
    pub partition_cell_size: f64,
    
    /// Enable debug visualization
    pub debug_draw: bool,
}

impl Default for GravityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            force_threshold: FORCE_THRESHOLD,
            max_distance: 1e12, // 1 million km
            use_spatial_partition: true,
            partition_cell_size: 1e8, // 100,000 km cells
            debug_draw: false,
        }
    }
}

impl GravityConfig {
    /// Earth surface gravity only (fast)
    pub fn earth_surface() -> Self {
        Self {
            max_distance: 1e7, // 10,000 km
            partition_cell_size: 1e6, // 1,000 km cells
            ..Default::default()
        }
    }
    
    /// Earth-Moon system
    pub fn earth_moon() -> Self {
        Self {
            max_distance: 5e8, // 500,000 km
            partition_cell_size: 1e7, // 10,000 km cells
            ..Default::default()
        }
    }
    
    /// Full solar system
    pub fn solar_system() -> Self {
        Self {
            max_distance: 1e12, // 1 million km
            partition_cell_size: 1e9, // 1 million km cells
            ..Default::default()
        }
    }
}

// ============================================================================
// Spatial Partitioning
// ============================================================================

/// Spatial grid cell identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellId {
    x: i64,
    y: i64,
    z: i64,
}

impl CellId {
    fn from_position(pos: Vec3, cell_size: f64) -> Self {
        Self {
            x: (pos.x as f64 / cell_size).floor() as i64,
            y: (pos.y as f64 / cell_size).floor() as i64,
            z: (pos.z as f64 / cell_size).floor() as i64,
        }
    }
    
    /// Get neighboring cells (3x3x3 = 27 cells including self)
    fn neighbors(&self) -> Vec<CellId> {
        let mut neighbors = Vec::with_capacity(27);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    neighbors.push(CellId {
                        x: self.x + dx,
                        y: self.y + dy,
                        z: self.z + dz,
                    });
                }
            }
        }
        neighbors
    }
}

/// Spatial partition for efficient gravity calculations
#[derive(Resource, Default)]
struct SpatialPartition {
    /// Map of cell ID to entities in that cell
    cells: HashMap<CellId, Vec<Entity>>,
    
    /// Map of entity to its current cell
    entity_cells: HashMap<Entity, CellId>,
}

impl SpatialPartition {
    fn clear(&mut self) {
        self.cells.clear();
        self.entity_cells.clear();
    }
    
    fn insert(&mut self, entity: Entity, cell: CellId) {
        self.cells.entry(cell).or_default().push(entity);
        self.entity_cells.insert(entity, cell);
    }
    
    fn get_nearby(&self, cell: CellId) -> Vec<Entity> {
        let mut nearby = Vec::new();
        for neighbor in cell.neighbors() {
            if let Some(entities) = self.cells.get(&neighbor) {
                nearby.extend(entities.iter().copied());
            }
        }
        nearby
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Build spatial partition for gravity sources
fn build_spatial_partition(
    config: Res<GravityConfig>,
    mut partition: ResMut<SpatialPartition>,
    sources: Query<(Entity, &Transform), With<GravitySource>>,
) {
    if !config.use_spatial_partition {
        return;
    }
    
    partition.clear();
    
    for (entity, transform) in sources.iter() {
        let cell = CellId::from_position(transform.translation, config.partition_cell_size);
        partition.insert(entity, cell);
    }
}

/// Calculate gravitational forces between all bodies
fn calculate_gravity_forces(
    config: Res<GravityConfig>,
    partition: Res<SpatialPartition>,
    sources: Query<(Entity, &Transform, &Mass, Option<&PhysicalRadius>), With<GravitySource>>,
    mut affected: Query<(Entity, &Transform, &Mass, &mut GravitationalForce), With<GravityAffected>>,
) {
    if !config.enabled {
        return;
    }
    
    // Reset all forces
    for (_, _, _, mut force) in affected.iter_mut() {
        force.reset();
    }
    
    // Calculate forces
    for (affected_entity, affected_transform, affected_mass, mut force) in affected.iter_mut() {
        let affected_pos = affected_transform.translation;
        
        // Get nearby sources (or all if not using spatial partition)
        let nearby_sources: Vec<_> = if config.use_spatial_partition {
            let cell = CellId::from_position(affected_pos, config.partition_cell_size);
            partition.get_nearby(cell)
        } else {
            sources.iter().map(|(e, _, _, _)| e).collect()
        };
        
        for source_entity in nearby_sources {
            // Skip self-interaction
            if source_entity == affected_entity {
                continue;
            }
            
            if let Ok((_, source_transform, source_mass, source_radius)) = sources.get(source_entity) {
                let source_pos = source_transform.translation;
                
                // Calculate distance vector
                let r_vec = source_pos - affected_pos;
                let r_mag = r_vec.length() as f64;
                
                // Skip if beyond max distance
                if r_mag > config.max_distance {
                    continue;
                }
                
                // Prevent division by zero and inside-body calculations
                let min_distance = if let Some(radius) = source_radius {
                    radius.meters
                } else {
                    1.0 // 1 meter minimum
                };
                
                if r_mag < min_distance {
                    continue;
                }
                
                // Calculate gravitational force magnitude: F = G * m1 * m2 / r²
                let force_magnitude = G * affected_mass.kg * source_mass.kg / (r_mag * r_mag);
                
                // Skip if below threshold
                if force_magnitude < config.force_threshold {
                    continue;
                }
                
                // Calculate force vector (direction toward source)
                let force_vec = r_vec.normalize() * force_magnitude as f32;
                
                force.add_force(force_vec);
            }
        }
    }
}

/// Apply gravitational forces to velocities
fn apply_gravity_to_velocity(
    time: Res<Time>,
    mut query: Query<(&Mass, &GravitationalForce, &mut crate::orbital::hybrid_coords::HybridVelocity)>,
) {
    let dt = time.delta_secs_f64();
    
    for (mass, grav_force, mut velocity) in query.iter_mut() {
        // F = ma, so a = F/m
        let acceleration = grav_force.force.as_dvec3() / mass.kg;
        
        // v = v0 + a*dt
        velocity.absolute += acceleration * dt;
        velocity.update_local();
    }
}

/// Debug visualization of gravity forces
fn debug_draw_gravity(
    config: Res<GravityConfig>,
    query: Query<(&Transform, &GravitationalForce), With<GravityAffected>>,
    mut gizmos: Gizmos,
) {
    if !config.debug_draw {
        return;
    }
    
    for (transform, force) in query.iter() {
        if force.force.length() > 0.0 {
            // Draw force vector (scaled for visibility)
            let scale = 1e-6; // Adjust based on force magnitudes
            let end = transform.translation + force.force * scale;
            
            gizmos.line(
                transform.translation,
                end,
                Color::srgb(1.0, 0.5, 0.0),
            );
            
            // Draw sphere at object
            gizmos.sphere(
                Isometry3d::from_translation(transform.translation),
                10.0,
                Color::srgb(0.0, 1.0, 0.0),
            );
        }
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Calculate gravitational force between two objects
pub fn gravitational_force(
    m1: f64,
    m2: f64,
    r_vec: Vec3,
) -> Vec3 {
    let r_mag = r_vec.length() as f64;
    
    if r_mag < 1.0 {
        return Vec3::ZERO;
    }
    
    let force_magnitude = G * m1 * m2 / (r_mag * r_mag);
    r_vec.normalize() * force_magnitude as f32
}

/// Calculate surface gravity (little g) from mass and radius
pub fn surface_gravity(mass_kg: f64, radius_m: f64) -> f64 {
    G * mass_kg / (radius_m * radius_m)
}

/// Calculate orbital velocity at distance r from center
pub fn orbital_velocity(mass_kg: f64, radius_m: f64) -> f64 {
    (G * mass_kg / radius_m).sqrt()
}

/// Calculate escape velocity from surface
pub fn escape_velocity(mass_kg: f64, radius_m: f64) -> f64 {
    (2.0 * G * mass_kg / radius_m).sqrt()
}

/// Calculate gravitational potential energy
pub fn gravitational_potential(m1: f64, m2: f64, r: f64) -> f64 {
    -G * m1 * m2 / r
}

// ============================================================================
// Statistics
// ============================================================================

/// Statistics for gravity calculations
#[derive(Resource, Default, Debug)]
pub struct GravityStats {
    pub total_sources: usize,
    pub total_affected: usize,
    pub total_forces_calculated: usize,
    pub forces_above_threshold: usize,
    pub average_force_magnitude: f64,
    pub max_force_magnitude: f64,
}

fn update_gravity_stats(
    mut stats: ResMut<GravityStats>,
    sources: Query<(), With<GravitySource>>,
    affected: Query<&GravitationalForce, With<GravityAffected>>,
) {
    stats.total_sources = sources.iter().count();
    stats.total_affected = affected.iter().count();
    
    let mut total_magnitude = 0.0;
    let mut max_magnitude: f64 = 0.0;
    let mut count = 0;
    
    for force in affected.iter() {
        let magnitude = force.force.length() as f64;
        if magnitude > 0.0 {
            total_magnitude += magnitude;
            max_magnitude = max_magnitude.max(magnitude);
            count += 1;
        }
    }
    
    stats.forces_above_threshold = count;
    stats.average_force_magnitude = if count > 0 {
        total_magnitude / count as f64
    } else {
        0.0
    };
    stats.max_force_magnitude = max_magnitude;
}

// ============================================================================
// Plugin
// ============================================================================

pub struct GravityPlugin;

impl Plugin for GravityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GravityConfig>()
            .init_resource::<SpatialPartition>()
            .init_resource::<GravityStats>()
            .register_type::<Mass>()
            .register_type::<PhysicalRadius>()
            .add_systems(FixedUpdate, (
                build_spatial_partition,
                calculate_gravity_forces.after(build_spatial_partition),
                apply_gravity_to_velocity.after(calculate_gravity_forces),
                update_gravity_stats.after(calculate_gravity_forces),
            ))
            .add_systems(Update, debug_draw_gravity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_earth_surface_gravity() {
        let mass = Mass::earth();
        let radius = PhysicalRadius::earth();
        let g = radius.surface_gravity(&mass);
        
        // Should be approximately 9.81 m/s²
        assert!((g - 9.81).abs() < 0.1);
    }
    
    #[test]
    fn test_escape_velocity() {
        let mass = Mass::earth();
        let radius = PhysicalRadius::earth();
        let v_escape = radius.escape_velocity(&mass);
        
        // Should be approximately 11,200 m/s
        assert!((v_escape - 11_200.0).abs() < 100.0);
    }
    
    #[test]
    fn test_orbital_velocity() {
        let earth_mass = Mass::earth();
        let earth_radius = PhysicalRadius::earth();
        
        // ISS altitude: 400 km
        let iss_altitude = 400_000.0;
        let r = earth_radius.meters + iss_altitude;
        let v = orbital_velocity(earth_mass.kg, r);
        
        // Should be approximately 7,660 m/s
        assert!((v - 7_660.0).abs() < 100.0);
    }
    
    #[test]
    fn test_gravitational_force() {
        let m1 = 1000.0; // 1 ton
        let m2 = 1000.0; // 1 ton
        let r = Vec3::new(10.0, 0.0, 0.0); // 10 meters apart
        
        let force = gravitational_force(m1, m2, r);
        let expected = G * m1 * m2 / 100.0; // r² = 100
        
        assert!((force.x as f64 - expected).abs() < 1e-10);
    }
    
    #[test]
    fn test_force_threshold() {
        // Small masses at large distance should produce negligible force
        let m1 = 1.0; // 1 kg
        let m2 = 1.0; // 1 kg
        let r = Vec3::new(1000.0, 0.0, 0.0); // 1 km apart
        
        let force = gravitational_force(m1, m2, r);
        let magnitude = force.length() as f64;
        
        // Should be well below threshold
        assert!(magnitude < FORCE_THRESHOLD);
    }
}
