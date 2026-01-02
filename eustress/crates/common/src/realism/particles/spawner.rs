//! # Particle Spawner
//!
//! Particle emission and spawning utilities.
//!
//! ## Table of Contents
//!
//! 1. **ParticleSpawner** - Component for emitting particles
//! 2. **SpawnPatterns** - Grid, sphere, cone, etc.
//! 3. **Spawner Systems** - Automatic particle emission

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use rand::Rng;

use super::components::*;

// ============================================================================
// Spawner Configuration
// ============================================================================

/// Global spawner configuration
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct ParticleSpawnerConfig {
    /// Maximum total particles in simulation
    pub max_particles: u32,
    /// Current particle count
    pub current_count: u32,
    /// Enable spawning
    pub enabled: bool,
}

impl Default for ParticleSpawnerConfig {
    fn default() -> Self {
        Self {
            max_particles: 100_000,
            current_count: 0,
            enabled: true,
        }
    }
}

// ============================================================================
// Particle Spawner Component
// ============================================================================

/// Particle emitter component
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ParticleSpawner {
    /// Particle type to spawn
    pub particle_type: ParticleType,
    /// Spawn rate (particles per second)
    pub rate: f32,
    /// Particle mass range (min, max)
    pub mass_range: (f32, f32),
    /// Particle radius range (min, max)
    pub radius_range: (f32, f32),
    /// Initial velocity range (min, max magnitude)
    pub velocity_range: (f32, f32),
    /// Velocity direction (normalized, or random if zero)
    pub velocity_direction: Vec3,
    /// Velocity spread angle (radians, 0 = no spread)
    pub velocity_spread: f32,
    /// Initial temperature range (Kelvin)
    pub temperature_range: (f32, f32),
    /// Particle lifetime range (seconds, None = infinite)
    pub lifetime_range: Option<(f32, f32)>,
    /// Spawn pattern
    pub pattern: SpawnPattern,
    /// Is spawner active
    pub active: bool,
    /// Accumulated time for spawning
    #[serde(skip)]
    pub accumulated_time: f32,
}

impl Default for ParticleSpawner {
    fn default() -> Self {
        Self {
            particle_type: ParticleType::Gas,
            rate: 10.0,
            mass_range: (0.001, 0.001),
            radius_range: (0.05, 0.1),
            velocity_range: (1.0, 5.0),
            velocity_direction: Vec3::Y,
            velocity_spread: 0.2,
            temperature_range: (300.0, 300.0),
            lifetime_range: Some((5.0, 10.0)),
            pattern: SpawnPattern::Point,
            active: true,
            accumulated_time: 0.0,
        }
    }
}

impl ParticleSpawner {
    /// Create a gas emitter
    pub fn gas() -> Self {
        Self {
            particle_type: ParticleType::Gas,
            ..default()
        }
    }
    
    /// Create a water/liquid emitter
    pub fn water() -> Self {
        Self {
            particle_type: ParticleType::Liquid,
            mass_range: (0.001, 0.001),
            radius_range: (0.02, 0.03),
            velocity_range: (2.0, 5.0),
            velocity_direction: Vec3::NEG_Y,
            temperature_range: (293.0, 293.0),
            lifetime_range: Some((10.0, 20.0)),
            ..default()
        }
    }
    
    /// Create a fire emitter
    pub fn fire() -> Self {
        Self {
            particle_type: ParticleType::Fire,
            rate: 50.0,
            mass_range: (0.0001, 0.0005),
            radius_range: (0.05, 0.15),
            velocity_range: (2.0, 5.0),
            velocity_direction: Vec3::Y,
            velocity_spread: 0.3,
            temperature_range: (800.0, 1200.0),
            lifetime_range: Some((0.5, 2.0)),
            ..default()
        }
    }
    
    /// Create a smoke emitter
    pub fn smoke() -> Self {
        Self {
            particle_type: ParticleType::Smoke,
            rate: 20.0,
            mass_range: (0.0001, 0.0003),
            radius_range: (0.1, 0.3),
            velocity_range: (0.5, 2.0),
            velocity_direction: Vec3::Y,
            velocity_spread: 0.5,
            temperature_range: (350.0, 450.0),
            lifetime_range: Some((3.0, 8.0)),
            ..default()
        }
    }
    
    /// Set spawn rate
    pub fn with_rate(mut self, rate: f32) -> Self {
        self.rate = rate;
        self
    }
    
    /// Set velocity
    pub fn with_velocity(mut self, direction: Vec3, min: f32, max: f32) -> Self {
        self.velocity_direction = direction;
        self.velocity_range = (min, max);
        self
    }
    
    /// Set temperature
    pub fn with_temperature(mut self, min: f32, max: f32) -> Self {
        self.temperature_range = (min, max);
        self
    }
    
    /// Set lifetime
    pub fn with_lifetime(mut self, min: f32, max: f32) -> Self {
        self.lifetime_range = Some((min, max));
        self
    }
    
    /// Set spawn pattern
    pub fn with_pattern(mut self, pattern: SpawnPattern) -> Self {
        self.pattern = pattern;
        self
    }
}

// ============================================================================
// Spawn Patterns
// ============================================================================

/// Pattern for particle spawning
#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
pub enum SpawnPattern {
    /// Single point
    Point,
    /// Sphere surface
    Sphere { radius: f32 },
    /// Sphere volume
    SphereVolume { radius: f32 },
    /// Box surface
    Box { half_extents: Vec3 },
    /// Box volume
    BoxVolume { half_extents: Vec3 },
    /// Disc (XZ plane)
    Disc { radius: f32 },
    /// Cone
    Cone { radius: f32, height: f32 },
    /// Line between two points
    Line { offset: Vec3 },
    /// Ring
    Ring { radius: f32 },
}

impl Default for SpawnPattern {
    fn default() -> Self {
        Self::Point
    }
}

impl SpawnPattern {
    /// Get a random offset for this pattern
    pub fn random_offset(&self, rng: &mut impl Rng) -> Vec3 {
        match self {
            SpawnPattern::Point => Vec3::ZERO,
            
            SpawnPattern::Sphere { radius } => {
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                let phi = rng.gen_range(0.0..std::f32::consts::PI);
                Vec3::new(
                    radius * phi.sin() * theta.cos(),
                    radius * phi.cos(),
                    radius * phi.sin() * theta.sin(),
                )
            }
            
            SpawnPattern::SphereVolume { radius } => {
                let r = radius * rng.gen_range(0.0f32..1.0).cbrt();
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                let phi = rng.gen_range(0.0..std::f32::consts::PI);
                Vec3::new(
                    r * phi.sin() * theta.cos(),
                    r * phi.cos(),
                    r * phi.sin() * theta.sin(),
                )
            }
            
            SpawnPattern::Box { half_extents } => {
                // Random face, then random position on that face
                let face = rng.gen_range(0..6);
                let u = rng.gen_range(-1.0..1.0);
                let v = rng.gen_range(-1.0..1.0);
                match face {
                    0 => Vec3::new(half_extents.x, u * half_extents.y, v * half_extents.z),
                    1 => Vec3::new(-half_extents.x, u * half_extents.y, v * half_extents.z),
                    2 => Vec3::new(u * half_extents.x, half_extents.y, v * half_extents.z),
                    3 => Vec3::new(u * half_extents.x, -half_extents.y, v * half_extents.z),
                    4 => Vec3::new(u * half_extents.x, v * half_extents.y, half_extents.z),
                    _ => Vec3::new(u * half_extents.x, v * half_extents.y, -half_extents.z),
                }
            }
            
            SpawnPattern::BoxVolume { half_extents } => {
                Vec3::new(
                    rng.gen_range(-half_extents.x..half_extents.x),
                    rng.gen_range(-half_extents.y..half_extents.y),
                    rng.gen_range(-half_extents.z..half_extents.z),
                )
            }
            
            SpawnPattern::Disc { radius } => {
                let r = radius * rng.gen_range(0.0f32..1.0).sqrt();
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                Vec3::new(r * theta.cos(), 0.0, r * theta.sin())
            }
            
            SpawnPattern::Cone { radius, height } => {
                let t = rng.gen_range(0.0f32..1.0);
                let r = radius * t;
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                Vec3::new(r * theta.cos(), height * t, r * theta.sin())
            }
            
            SpawnPattern::Line { offset } => {
                let t = rng.gen_range(0.0..1.0);
                *offset * t
            }
            
            SpawnPattern::Ring { radius } => {
                let theta = rng.gen_range(0.0..std::f32::consts::TAU);
                Vec3::new(radius * theta.cos(), 0.0, radius * theta.sin())
            }
        }
    }
}

// ============================================================================
// Spawn Functions
// ============================================================================

/// Spawn a single particle from a spawner
pub fn spawn_particle(
    commands: &mut Commands,
    spawner: &ParticleSpawner,
    spawner_transform: &Transform,
    rng: &mut impl Rng,
) -> Entity {
    // Calculate spawn position
    let offset = spawner.pattern.random_offset(rng);
    let position = spawner_transform.translation + spawner_transform.rotation * offset;
    
    // Calculate velocity
    let speed = rng.gen_range(spawner.velocity_range.0..spawner.velocity_range.1);
    let base_direction = if spawner.velocity_direction.length_squared() > 0.01 {
        spawner.velocity_direction.normalize()
    } else {
        // Random direction
        let theta = rng.gen_range(0.0..std::f32::consts::TAU);
        let phi = rng.gen_range(0.0..std::f32::consts::PI);
        Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin())
    };
    
    // Apply spread
    let velocity = if spawner.velocity_spread > 0.0 {
        let spread_x = rng.gen_range(-spawner.velocity_spread..spawner.velocity_spread);
        let spread_z = rng.gen_range(-spawner.velocity_spread..spawner.velocity_spread);
        let perturbed = (base_direction + Vec3::new(spread_x, 0.0, spread_z)).normalize();
        spawner_transform.rotation * perturbed * speed
    } else {
        spawner_transform.rotation * base_direction * speed
    };
    
    // Random properties
    let mass = rng.gen_range(spawner.mass_range.0..spawner.mass_range.1);
    let radius = rng.gen_range(spawner.radius_range.0..spawner.radius_range.1);
    let temperature = rng.gen_range(spawner.temperature_range.0..spawner.temperature_range.1);
    let lifetime = spawner.lifetime_range.map(|(min, max)| rng.gen_range(min..max));
    
    // Create particle
    let particle = Particle {
        mass,
        radius,
        particle_type: spawner.particle_type,
        lifetime,
        active: true,
    };
    
    let kinetic = KineticState::with_velocity(velocity);
    let thermo = ThermodynamicState::at_conditions(
        mass / 0.029, // Approximate moles for air-like gas
        temperature,
        crate::realism::constants::STANDARD_PRESSURE,
    );
    
    // Spawn entity
    let mut entity_commands = commands.spawn((
        particle,
        kinetic,
        thermo,
        Transform::from_translation(position),
        GlobalTransform::default(),
    ));
    
    // Add fluid properties for liquid particles
    if spawner.particle_type == ParticleType::Liquid {
        entity_commands.insert(FluidProperties::default());
    }
    
    entity_commands.id()
}

/// Spawn multiple particles in a grid pattern
pub fn spawn_particle_grid(
    commands: &mut Commands,
    center: Vec3,
    grid_size: UVec3,
    spacing: f32,
    particle_type: ParticleType,
    mass: f32,
    radius: f32,
    temperature: f32,
) -> Vec<Entity> {
    let mut entities = Vec::with_capacity((grid_size.x * grid_size.y * grid_size.z) as usize);
    
    let half_size = Vec3::new(
        (grid_size.x - 1) as f32 * spacing * 0.5,
        (grid_size.y - 1) as f32 * spacing * 0.5,
        (grid_size.z - 1) as f32 * spacing * 0.5,
    );
    
    for x in 0..grid_size.x {
        for y in 0..grid_size.y {
            for z in 0..grid_size.z {
                let position = center + Vec3::new(
                    x as f32 * spacing - half_size.x,
                    y as f32 * spacing - half_size.y,
                    z as f32 * spacing - half_size.z,
                );
                
                let particle = Particle {
                    mass,
                    radius,
                    particle_type,
                    lifetime: None,
                    active: true,
                };
                
                let thermo = ThermodynamicState::at_conditions(
                    mass / 0.029,
                    temperature,
                    crate::realism::constants::STANDARD_PRESSURE,
                );
                
                let entity = commands.spawn((
                    particle,
                    KineticState::default(),
                    thermo,
                    Transform::from_translation(position),
                    GlobalTransform::default(),
                )).id();
                
                entities.push(entity);
            }
        }
    }
    
    entities
}

/// Spawn water block (for SPH simulation)
pub fn spawn_water_block(
    commands: &mut Commands,
    center: Vec3,
    size: Vec3,
    particle_spacing: f32,
) -> Vec<Entity> {
    let count_x = (size.x / particle_spacing).ceil() as u32;
    let count_y = (size.y / particle_spacing).ceil() as u32;
    let count_z = (size.z / particle_spacing).ceil() as u32;
    
    let mut entities = Vec::with_capacity((count_x * count_y * count_z) as usize);
    
    for x in 0..count_x {
        for y in 0..count_y {
            for z in 0..count_z {
                let position = center + Vec3::new(
                    (x as f32 - count_x as f32 * 0.5) * particle_spacing,
                    (y as f32 - count_y as f32 * 0.5) * particle_spacing,
                    (z as f32 - count_z as f32 * 0.5) * particle_spacing,
                );
                
                let entity = commands.spawn(FluidParticleBundle::water(position)).id();
                entities.push(entity);
            }
        }
    }
    
    entities
}
