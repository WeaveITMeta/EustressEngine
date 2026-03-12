//! # Dynamic Gravity System
//!
//! Explorer Workspace-integrated gravity with real-time force calculations.
//! Uses tiered object tracking for performance optimization.
//!
//! ## Architecture
//!
//! - **Heavy Objects Table**: Massive bodies (planets, stars) tracked closely
//! - **Light Objects**: Small masses use simplified calculations
//! - **Force Metatables**: Real-time force tracking between object pairs
//! - **Dynamic Properties**: All values editable at runtime via Explorer
//!
//! ## Performance Tiers
//!
//! - **Tier 1 (Heavy)**: Mass > 1e20 kg - Full N-body calculations
//! - **Tier 2 (Medium)**: Mass 1e10 - 1e20 kg - Simplified calculations
//! - **Tier 3 (Light)**: Mass < 1e10 kg - Only affected by Tier 1 objects

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use tracing::info;

use super::{G, FORCE_THRESHOLD};

// ============================================================================
// Tier Thresholds
// ============================================================================

/// Mass threshold for heavy objects (full N-body tracking)
pub const HEAVY_MASS_THRESHOLD: f64 = 1e20; // 100 quintillion kg (small moons+)

/// Mass threshold for medium objects (simplified tracking)
pub const MEDIUM_MASS_THRESHOLD: f64 = 1e10; // 10 billion kg (large asteroids+)

/// Update frequency tiers (Hz)
pub const HEAVY_UPDATE_HZ: f64 = 60.0;   // Every frame
pub const MEDIUM_UPDATE_HZ: f64 = 10.0;  // 10 times per second
pub const LIGHT_UPDATE_HZ: f64 = 1.0;    // Once per second

// ============================================================================
// Dynamic Mass Component (Explorer-editable)
// ============================================================================

/// Dynamic mass component - editable at runtime
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct DynamicMass {
    /// Mass in kilograms (editable in Properties panel)
    #[reflect(default)]
    pub kilograms: f64,
    
    /// Cached tier for performance
    #[reflect(ignore)]
    pub tier: MassTier,
    
    /// Last update time for tier-based scheduling
    #[reflect(ignore)]
    pub last_update: f64,
}

impl DynamicMass {
    pub fn new(kg: f64) -> Self {
        Self {
            kilograms: kg,
            tier: MassTier::from_mass(kg),
            last_update: 0.0,
        }
    }
    
    /// Update tier based on current mass
    pub fn update_tier(&mut self) {
        self.tier = MassTier::from_mass(self.kilograms);
    }
    
    /// Check if this object needs update based on tier frequency
    pub fn needs_update(&self, current_time: f64) -> bool {
        let dt = current_time - self.last_update;
        let interval = match self.tier {
            MassTier::Heavy => 1.0 / HEAVY_UPDATE_HZ,
            MassTier::Medium => 1.0 / MEDIUM_UPDATE_HZ,
            MassTier::Light => 1.0 / LIGHT_UPDATE_HZ,
        };
        dt >= interval
    }
}

impl Default for DynamicMass {
    fn default() -> Self {
        Self::new(1.0)
    }
}

/// Mass tier for performance optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Default)]
pub enum MassTier {
    Heavy,   // > 1e20 kg - planets, stars
    Medium,  // 1e10 - 1e20 kg - large asteroids, small moons
    #[default]
    Light,   // < 1e10 kg - spacecraft, debris
}

impl MassTier {
    pub fn from_mass(kg: f64) -> Self {
        if kg >= HEAVY_MASS_THRESHOLD {
            MassTier::Heavy
        } else if kg >= MEDIUM_MASS_THRESHOLD {
            MassTier::Medium
        } else {
            MassTier::Light
        }
    }
}

// ============================================================================
// Dynamic Radius Component
// ============================================================================

/// Dynamic physical radius - editable at runtime
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct DynamicRadius {
    /// Radius in meters (editable in Properties panel)
    #[reflect(default)]
    pub meters: f64,
}

impl DynamicRadius {
    pub fn new(meters: f64) -> Self {
        Self { meters }
    }
    
    /// Calculate surface gravity (little g)
    pub fn surface_gravity(&self, mass: &DynamicMass) -> f64 {
        G * mass.kilograms / (self.meters * self.meters)
    }
}

impl Default for DynamicRadius {
    fn default() -> Self {
        Self::new(1.0)
    }
}

// ============================================================================
// Force Metatable System
// ============================================================================

/// Pair of entities for force tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EntityPair {
    source: Entity,
    target: Entity,
}

impl EntityPair {
    fn new(source: Entity, target: Entity) -> Self {
        Self { source, target }
    }
}

/// Force entry in the metatable
#[derive(Debug, Clone)]
struct ForceEntry {
    /// Force magnitude in Newtons
    magnitude: f64,
    
    /// Force direction (unit vector)
    direction: Vec3,
    
    /// Distance between objects (meters)
    distance: f64,
    
    /// Last calculation time
    last_calculated: f64,
    
    /// Whether this force is above threshold
    significant: bool,
}

/// Force metatable - tracks all pairwise forces
#[derive(Resource, Default)]
pub struct ForceMetatable {
    /// Map of entity pairs to force entries
    forces: HashMap<EntityPair, ForceEntry>,
    
    /// Cached heavy object list for quick lookups
    heavy_objects: HashSet<Entity>,
    
    /// Statistics
    total_pairs: usize,
    significant_pairs: usize,
    last_cleanup: f64,
}

impl ForceMetatable {
    /// Get force between two objects
    pub fn get_force(&self, source: Entity, target: Entity) -> Option<&ForceEntry> {
        self.forces.get(&EntityPair::new(source, target))
    }
    
    /// Update force between two objects
    pub fn update_force(
        &mut self,
        source: Entity,
        target: Entity,
        magnitude: f64,
        direction: Vec3,
        distance: f64,
        time: f64,
    ) {
        let pair = EntityPair::new(source, target);
        let significant = magnitude >= FORCE_THRESHOLD;
        
        self.forces.insert(pair, ForceEntry {
            magnitude,
            direction,
            distance,
            last_calculated: time,
            significant,
        });
        
        if significant {
            self.significant_pairs += 1;
        }
    }
    
    /// Mark entity as heavy object
    pub fn mark_heavy(&mut self, entity: Entity) {
        self.heavy_objects.insert(entity);
    }
    
    /// Remove entity from heavy objects
    pub fn unmark_heavy(&mut self, entity: Entity) {
        self.heavy_objects.remove(&entity);
    }
    
    /// Check if entity is heavy
    pub fn is_heavy(&self, entity: Entity) -> bool {
        self.heavy_objects.contains(&entity)
    }
    
    /// Clean up old force entries
    pub fn cleanup(&mut self, current_time: f64, max_age: f64) {
        self.forces.retain(|_, entry| {
            current_time - entry.last_calculated < max_age
        });
        self.last_cleanup = current_time;
    }
    
    /// Get statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (self.forces.len(), self.significant_pairs, self.heavy_objects.len())
    }
}

// ============================================================================
// Dynamic Force Accumulator
// ============================================================================

/// Dynamic gravitational force accumulator
#[derive(Component, Debug, Default, Clone)]
pub struct DynamicGravityForce {
    /// Total accumulated force (Newtons)
    pub force: Vec3,
    
    /// Forces from heavy objects (tracked separately)
    pub heavy_force: Vec3,
    
    /// Forces from medium objects
    pub medium_force: Vec3,
    
    /// Forces from light objects
    pub light_force: Vec3,
    
    /// Number of contributing sources by tier
    pub heavy_sources: usize,
    pub medium_sources: usize,
    pub light_sources: usize,
}

impl DynamicGravityForce {
    pub fn reset(&mut self) {
        self.force = Vec3::ZERO;
        self.heavy_force = Vec3::ZERO;
        self.medium_force = Vec3::ZERO;
        self.light_force = Vec3::ZERO;
        self.heavy_sources = 0;
        self.medium_sources = 0;
        self.light_sources = 0;
    }
    
    pub fn add_force(&mut self, force: Vec3, tier: MassTier) {
        self.force += force;
        match tier {
            MassTier::Heavy => {
                self.heavy_force += force;
                self.heavy_sources += 1;
            }
            MassTier::Medium => {
                self.medium_force += force;
                self.medium_sources += 1;
            }
            MassTier::Light => {
                self.light_force += force;
                self.light_sources += 1;
            }
        }
    }
    
    /// Get acceleration (F = ma, so a = F/m)
    pub fn acceleration(&self, mass: &DynamicMass) -> Vec3 {
        if mass.kilograms > 0.0 {
            self.force / mass.kilograms as f32
        } else {
            Vec3::ZERO
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Dynamic gravity system configuration
#[derive(Resource, Debug, Clone)]
pub struct DynamicGravityConfig {
    /// Enable dynamic gravity calculations
    pub enabled: bool,
    
    /// Force threshold (N) - forces below this are ignored
    pub force_threshold: f64,
    
    /// Maximum influence distance (m)
    pub max_distance: f64,
    
    /// Enable tier-based optimization
    pub use_tiered_updates: bool,
    
    /// Enable force metatable tracking
    pub track_forces: bool,
    
    /// Metatable cleanup interval (seconds)
    pub cleanup_interval: f64,
    
    /// Metatable max age (seconds)
    pub max_force_age: f64,
    
    /// Debug visualization
    pub debug_draw: bool,
    
    /// Show force metatable in UI
    pub show_metatable: bool,
}

impl Default for DynamicGravityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            force_threshold: FORCE_THRESHOLD,
            max_distance: 1e12,
            use_tiered_updates: true,
            track_forces: true,
            cleanup_interval: 10.0,
            max_force_age: 30.0,
            debug_draw: false,
            show_metatable: false,
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Update mass tiers when mass changes
fn update_mass_tiers(
    mut query: Query<(Entity, &mut DynamicMass), Changed<DynamicMass>>,
    mut metatable: ResMut<ForceMetatable>,
) {
    for (entity, mut mass) in query.iter_mut() {
        let old_tier = mass.tier;
        mass.update_tier();
        
        // Update heavy object tracking
        if mass.tier == MassTier::Heavy && old_tier != MassTier::Heavy {
            metatable.mark_heavy(entity);
            info!("Entity {:?} promoted to Heavy tier ({:.3e} kg)", entity, mass.kilograms);
        } else if mass.tier != MassTier::Heavy && old_tier == MassTier::Heavy {
            metatable.unmark_heavy(entity);
            info!("Entity {:?} demoted from Heavy tier ({:.3e} kg)", entity, mass.kilograms);
        }
    }
}

/// Calculate gravitational forces with tiered optimization
fn calculate_dynamic_gravity(
    config: Res<DynamicGravityConfig>,
    time: Res<Time>,
    mut metatable: ResMut<ForceMetatable>,
    sources: Query<(Entity, &Transform, &DynamicMass, Option<&DynamicRadius>)>,
    mut affected: Query<(Entity, &Transform, &DynamicMass, &mut DynamicGravityForce)>,
) {
    if !config.enabled {
        return;
    }
    
    // Skip gravity calculations if no heavy objects are present
    if metatable.heavy_objects.is_empty() {
        return;
    }
    
    let current_time = time.elapsed_secs_f64();
    
    // Reset all forces
    for (_, _, _, mut force) in affected.iter_mut() {
        force.reset();
    }
    
    // Calculate forces
    for (target_entity, target_transform, target_mass, mut target_force) in affected.iter_mut() {
        let target_pos = target_transform.translation;
        
        // Determine which sources to check based on target tier
        let check_all = !config.use_tiered_updates || target_mass.tier == MassTier::Heavy;
        
        for (source_entity, source_transform, source_mass, source_radius) in sources.iter() {
            // Skip self-interaction
            if source_entity == target_entity {
                continue;
            }
            
            // Tier-based filtering
            if !check_all {
                // Light objects only affected by heavy objects
                if target_mass.tier == MassTier::Light && source_mass.tier != MassTier::Heavy {
                    continue;
                }
                
                // Medium objects affected by heavy and medium
                if target_mass.tier == MassTier::Medium && source_mass.tier == MassTier::Light {
                    continue;
                }
            }
            
            // Check if source needs update based on tier frequency
            if config.use_tiered_updates && !source_mass.needs_update(current_time) {
                // Use cached force from metatable if available
                if let Some(cached) = metatable.get_force(source_entity, target_entity) {
                    if cached.significant {
                        let force_vec = cached.direction * cached.magnitude as f32;
                        target_force.add_force(force_vec, source_mass.tier);
                    }
                    continue;
                }
            }
            
            let source_pos = source_transform.translation;
            
            // Calculate distance vector
            let r_vec = source_pos - target_pos;
            let r_mag = r_vec.length() as f64;
            
            // Skip if beyond max distance
            if r_mag > config.max_distance {
                continue;
            }
            
            // Prevent division by zero and inside-body calculations
            let min_distance = if let Some(radius) = source_radius {
                radius.meters
            } else {
                1.0
            };
            
            if r_mag < min_distance {
                continue;
            }
            
            // Calculate gravitational force: F = G * m1 * m2 / r²
            let force_magnitude = G * target_mass.kilograms * source_mass.kilograms / (r_mag * r_mag);
            
            // Skip if below threshold
            if force_magnitude < config.force_threshold {
                continue;
            }
            
            // Calculate force vector
            let force_direction = r_vec.normalize();
            let force_vec = force_direction * force_magnitude as f32;
            
            // Add to accumulator
            target_force.add_force(force_vec, source_mass.tier);
            
            // Update metatable
            if config.track_forces {
                metatable.update_force(
                    source_entity,
                    target_entity,
                    force_magnitude,
                    force_direction,
                    r_mag,
                    current_time,
                );
            }
        }
    }
    
    // Periodic metatable cleanup
    if config.track_forces && current_time - metatable.last_cleanup > config.cleanup_interval {
        metatable.cleanup(current_time, config.max_force_age);
    }
}

/// Apply forces to velocities
fn apply_dynamic_gravity(
    time: Res<Time>,
    mut query: Query<(&DynamicMass, &DynamicGravityForce, &mut crate::orbital::hybrid_coords::HybridVelocity)>,
) {
    let dt = time.delta_secs_f64();
    
    for (mass, force, mut velocity) in query.iter_mut() {
        let acceleration = force.acceleration(mass).as_dvec3();
        velocity.absolute += acceleration * dt;
        velocity.update_local();
    }
}

/// Debug visualization
fn debug_draw_dynamic_gravity(
    config: Res<DynamicGravityConfig>,
    query: Query<(&Transform, &DynamicGravityForce, &DynamicMass)>,
    mut gizmos: Gizmos,
) {
    if !config.debug_draw {
        return;
    }
    
    for (transform, force, mass) in query.iter() {
        if force.force.length() > 0.0 {
            // Color by tier
            let color = match mass.tier {
                MassTier::Heavy => Color::srgb(1.0, 0.0, 0.0),   // Red
                MassTier::Medium => Color::srgb(1.0, 1.0, 0.0),  // Yellow
                MassTier::Light => Color::srgb(0.0, 1.0, 0.0),   // Green
            };
            
            // Draw force vector (scaled for visibility)
            let scale = 1e-6;
            let end = transform.translation + force.force * scale;
            
            gizmos.line(transform.translation, end, color);
            
            // Draw sphere at object
            let radius = match mass.tier {
                MassTier::Heavy => 100.0,
                MassTier::Medium => 50.0,
                MassTier::Light => 10.0,
            };
            gizmos.sphere(Isometry3d::from_translation(transform.translation), radius, color);
        }
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Statistics for dynamic gravity system
#[derive(Resource, Default, Debug)]
pub struct DynamicGravityStats {
    pub heavy_objects: usize,
    pub medium_objects: usize,
    pub light_objects: usize,
    pub total_forces: usize,
    pub significant_forces: usize,
    pub metatable_size: usize,
}

fn update_dynamic_stats(
    mut stats: ResMut<DynamicGravityStats>,
    metatable: Res<ForceMetatable>,
    query: Query<&DynamicMass>,
) {
    let mut heavy = 0;
    let mut medium = 0;
    let mut light = 0;
    
    for mass in query.iter() {
        match mass.tier {
            MassTier::Heavy => heavy += 1,
            MassTier::Medium => medium += 1,
            MassTier::Light => light += 1,
        }
    }
    
    let (total, significant, _) = metatable.stats();
    
    stats.heavy_objects = heavy;
    stats.medium_objects = medium;
    stats.light_objects = light;
    stats.total_forces = total;
    stats.significant_forces = significant;
    stats.metatable_size = metatable.forces.len();
}

// ============================================================================
// Plugin
// ============================================================================

pub struct DynamicGravityPlugin;

impl Plugin for DynamicGravityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicGravityConfig>()
            .init_resource::<ForceMetatable>()
            .init_resource::<DynamicGravityStats>()
            .register_type::<DynamicMass>()
            .register_type::<DynamicRadius>()
            .register_type::<MassTier>()
            .add_systems(FixedUpdate, (
                update_mass_tiers,
                calculate_dynamic_gravity.after(update_mass_tiers),
                apply_dynamic_gravity.after(calculate_dynamic_gravity),
                update_dynamic_stats.after(calculate_dynamic_gravity),
            ))
            .add_systems(Update, debug_draw_dynamic_gravity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mass_tiers() {
        assert_eq!(MassTier::from_mass(1e25), MassTier::Heavy);
        assert_eq!(MassTier::from_mass(1e15), MassTier::Medium);
        assert_eq!(MassTier::from_mass(1e5), MassTier::Light);
    }
    
    #[test]
    fn test_dynamic_mass() {
        let mut mass = DynamicMass::new(1e25);
        assert_eq!(mass.tier, MassTier::Heavy);
        
        mass.kilograms = 1e15;
        mass.update_tier();
        assert_eq!(mass.tier, MassTier::Medium);
    }
}
