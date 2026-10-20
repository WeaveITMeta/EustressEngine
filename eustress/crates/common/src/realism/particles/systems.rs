//! # Particle Systems
//!
//! ECS systems for updating particle physics.
//!
//! ## Table of Contents
//!
//! 1. **Spatial Hash Update** - Rebuild spatial hash for neighbor queries
//! 2. **Thermodynamics Update** - Update temperature, pressure, entropy
//! 3. **Kinematics Update** - Update velocity, position
//! 4. **Force Application** - Apply accumulated forces

use bevy::prelude::*;
use bevy::diagnostic::FrameCount;
use rayon::prelude::*;

use super::components::*;
use super::spatial::SpatialHash;
use crate::realism::constants;
use crate::realism::laws::{thermodynamics, mechanics};
use crate::realism::lod::SimLodTier;
use crate::realism::{PhysicsDomain, RealismConfig};
use crate::services::physics::PhysicsService;
use crate::services::workspace::{live_gravity, Workspace};

/// Whether a physics domain is switched on in PhysicsService.
///
/// These systems used to read on/off flags off `RealismConfig`, a second,
/// unexposed switch for the same domains PhysicsService now controls. Two
/// switches for one thing meant turning Thermodynamics off in the properties
/// panel left particle heat transfer running. Absent service (a host that never
/// added the physics plugin) means nothing is switched off.
fn domain_on(physics: &Option<Res<PhysicsService>>, domain: PhysicsDomain) -> bool {
    physics.as_ref().map_or(true, |p| p.domain_enabled(domain))
}

// ============================================================================
// Spatial Hash Update
// ============================================================================

/// Update spatial hash with current particle positions
pub fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &Transform), With<Particle>>,
    config: Res<RealismConfig>,
    physics: Option<Res<PhysicsService>>,
) {
    if !domain_on(&physics, PhysicsDomain::Thermodynamics)
        && !domain_on(&physics, PhysicsDomain::Fluids)
    {
        return;
    }
    
    spatial_hash.clear();
    spatial_hash.cell_size = config.spatial_cell_size;
    
    for (entity, transform) in query.iter() {
        spatial_hash.insert(entity, transform.translation);
    }
}

// ============================================================================
// Thermodynamics Update
// ============================================================================

/// Update thermodynamic properties of particles
pub fn update_thermodynamics(
    mut query: Query<(&Particle, &mut ThermodynamicState, &Transform, Option<&SimLodTier>)>,
    spatial_hash: Res<SpatialHash>,
    config: Res<RealismConfig>,
    time: Res<Time>,
    frame: Res<FrameCount>,
    physics: Option<Res<PhysicsService>>,
) {
    if !domain_on(&physics, PhysicsDomain::Thermodynamics) {
        return;
    }

    let dt = time.delta_secs() * config.time_scale;
    if dt <= 0.0 {
        return;
    }

    // Collect positions and temperatures for heat transfer calculations
    let particle_data: Vec<(Entity, Vec3, f32)> = query
        .iter()
        .map(|(_, thermo, transform, _)| {
            (Entity::PLACEHOLDER, transform.translation, thermo.temperature)
        })
        .collect();

    // Update each particle's thermodynamic state
    for (particle, mut thermo, transform, lod) in query.iter_mut() {
        if !particle.active {
            continue;
        }
        let tier = lod.copied().unwrap_or_default();
        if !tier.should_update(frame.0) {
            continue;
        }
        
        // Update pressure from ideal gas law
        thermo.update_pressure();
        
        // Heat transfer with neighbors (simplified conduction)
        let neighbors = spatial_hash.query_radius(transform.translation, config.spatial_cell_size * 2.0);
        
        let mut heat_transfer = 0.0;
        for neighbor_entity in neighbors {
            // Find neighbor temperature (simplified - in production use parallel-safe access)
            for (_, neighbor_pos, neighbor_temp) in &particle_data {
                let distance = (transform.translation - *neighbor_pos).length();
                if distance > 0.01 && distance < config.spatial_cell_size * 2.0 {
                    // Simplified heat conduction: Q = k * A * ΔT / d
                    let delta_t = *neighbor_temp - thermo.temperature;
                    let k = 0.1; // Simplified thermal conductivity
                    let area = 4.0 * std::f32::consts::PI * particle.radius * particle.radius;
                    heat_transfer += k * area * delta_t / distance * dt;
                    break;
                }
            }
        }
        
        // Apply heat transfer
        if heat_transfer.abs() > 1e-10 {
            thermo.add_heat_isochoric(heat_transfer);
        }
        
        // Update internal energy and enthalpy
        thermo.update_internal_energy();
        thermo.update_enthalpy();
    }
}

// ============================================================================
// Kinematics Update
// ============================================================================

/// Update kinematic state (velocity, position) from forces
pub fn update_kinematics(
    mut query: Query<(&Particle, &mut KineticState, &mut Transform, Option<&SimLodTier>)>,
    config: Res<RealismConfig>,
    time: Res<Time>,
    frame: Res<FrameCount>,
    physics: Option<Res<PhysicsService>>,
) {
    let dt = time.delta_secs() * config.time_scale;
    if dt <= 0.0 {
        return;
    }

    // Parallel iteration for performance
    if physics.as_ref().map_or(true, |p| p.parallel) {
        query.par_iter_mut().for_each(|(particle, mut kinetic, mut transform, lod)| {
            if !particle.active {
                return;
            }
            let tier = lod.copied().unwrap_or_default();
            if !tier.should_update(frame.0) {
                return;
            }
            
            // Calculate acceleration from accumulated force: a = F/m
            let acceleration = if particle.mass > 0.0 {
                kinetic.accumulated_force / particle.mass
            } else {
                Vec3::ZERO
            };
            
            // Update velocity: v = v + a*dt
            kinetic.velocity += acceleration * dt;
            
            // Update position: x = x + v*dt
            transform.translation += kinetic.velocity * dt;
            
            // Update momentum
            kinetic.update_momentum(particle.mass);
            
            // Handle angular motion
            if kinetic.angular_velocity.length_squared() > 1e-10 {
                let angular_acceleration = kinetic.accumulated_torque / (particle.mass * particle.radius * particle.radius * 0.4);
                kinetic.angular_velocity += angular_acceleration * dt;
                
                // Apply rotation
                let rotation_delta = Quat::from_scaled_axis(kinetic.angular_velocity * dt);
                transform.rotation = rotation_delta * transform.rotation;
            }
            
            // Clear accumulated forces for next frame
            kinetic.clear_forces();
        });
    } else {
        for (particle, mut kinetic, mut transform, lod) in query.iter_mut() {
            if !particle.active {
                continue;
            }
            let tier = lod.copied().unwrap_or_default();
            if !tier.should_update(frame.0) {
                continue;
            }

            let acceleration = if particle.mass > 0.0 {
                kinetic.accumulated_force / particle.mass
            } else {
                Vec3::ZERO
            };

            kinetic.velocity += acceleration * dt;
            transform.translation += kinetic.velocity * dt;
            kinetic.update_momentum(particle.mass);
            kinetic.clear_forces();
        }
    }
}

// ============================================================================
// Force Application
// ============================================================================

/// Apply standard forces to particles (gravity, drag, buoyancy)
///
/// Gravity is the Workspace's, the same the parts around them fall with, so a
/// Space on the Moon lets smoke and dust drift down slowly too.
pub fn apply_particle_forces(
    mut query: Query<(&Particle, &mut KineticState, &Transform, Option<&ThermodynamicState>, Option<&FluidProperties>)>,
    physics: Option<Res<PhysicsService>>,
    workspace: Option<Res<Workspace>>,
) {
    if !domain_on(&physics, PhysicsDomain::Thermodynamics)
        && !domain_on(&physics, PhysicsDomain::Fluids)
    {
        return;
    }

    let gravity = live_gravity(workspace.as_deref());
    let air_density = constants::AIR_DENSITY_SEA_LEVEL;
    
    for (particle, mut kinetic, transform, thermo, fluid) in query.iter_mut() {
        if !particle.active {
            continue;
        }
        
        // Gravity
        let gravity_force = particle.mass * gravity;
        kinetic.apply_force(gravity_force);
        
        // Air drag (simplified)
        let speed = kinetic.velocity.length();
        if speed > 0.01 {
            let drag_coefficient = match particle.particle_type {
                ParticleType::Gas => 0.1,
                ParticleType::Liquid => 0.47,
                ParticleType::Solid => 0.47,
                ParticleType::Dust => 1.0,
                ParticleType::Smoke => 1.5,
                ParticleType::Fire => 0.5,
                ParticleType::Plasma => 0.1,
            };
            
            let area = std::f32::consts::PI * particle.radius * particle.radius;
            let drag_magnitude = 0.5 * air_density * speed * speed * drag_coefficient * area;
            let drag_force = -kinetic.velocity.normalize() * drag_magnitude;
            kinetic.apply_force(drag_force);
        }
        
        // Buoyancy for gas/smoke/fire particles
        if let Some(thermo) = thermo {
            match particle.particle_type {
                ParticleType::Gas | ParticleType::Smoke | ParticleType::Fire => {
                    // Hot gas rises. Archimedes: the parcel displaces its own
                    // volume of air, pushed against gravity. That volume is
                    // its mass over its density at its temperature, not the
                    // sphere its radius draws, so a parcel lighter than the
                    // air it displaces rises; its weight is the gravity above.
                    let particle_density = thermo.density(0.029); // Assuming air-like gas
                    if particle_density > 0.0 {
                        let displaced = particle.mass / particle_density;
                        kinetic.apply_force(-air_density * displaced * gravity);
                    }
                }
                _ => {}
            }
        }
        // A liquid SPH parcel is not buoyant in itself: its support comes
        // from the pressure force in `fluids::sph::update_sph_forces`, which
        // is also what floats anything immersed in it.
        let _ = fluid;
    }
}

// ============================================================================
// Lifetime Management
// ============================================================================

/// Update particle lifetimes and despawn expired particles
pub fn update_particle_lifetimes(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Particle)>,
    time: Res<Time>,
    config: Res<RealismConfig>,
) {
    let dt = time.delta_secs() * config.time_scale;
    
    for (entity, mut particle) in query.iter_mut() {
        if let Some(ref mut lifetime) = particle.lifetime {
            *lifetime -= dt;
            if *lifetime <= 0.0 {
                commands.entity(entity).despawn();
            }
        }
    }
}

// ============================================================================
// Debug Visualization
// ============================================================================

/// Draw debug gizmos for particles
pub fn draw_particle_gizmos(
    query: Query<(&Particle, &Transform, Option<&KineticState>, Option<&ThermodynamicState>)>,
    mut gizmos: Gizmos,
    _config: Res<RealismConfig>,
) {
    for (particle, transform, kinetic, thermo) in query.iter() {
        let pos = transform.translation;
        let radius = particle.radius;
        
        // Base color from temperature if available, otherwise white
        let color = if let Some(thermo) = thermo {
            temperature_to_color(thermo.temperature)
        } else {
            Color::srgba(0.5, 0.8, 1.0, 0.6)
        };
        
        // Draw particle sphere
        gizmos.sphere(Isometry3d::from_translation(pos), radius, color);
        
        // Draw velocity vector if available
        if let Some(kinetic) = kinetic {
            let vel = kinetic.velocity;
            if vel.length() > 0.01 {
                let tip = pos + vel * 0.1;
                gizmos.line(pos, tip, Color::srgb(1.0, 1.0, 0.0));
            }
        }
    }
}

/// Convert temperature to color (blue = cold, red = hot)
fn temperature_to_color(temperature: f32) -> Color {
    // Map temperature to 0-1 range (200K to 1000K)
    let t = ((temperature - 200.0) / 800.0).clamp(0.0, 1.0);
    
    // Blue (cold) -> White -> Red (hot)
    if t < 0.5 {
        let t2 = t * 2.0;
        Color::srgb(t2, t2, 1.0)
    } else {
        let t2 = (t - 0.5) * 2.0;
        Color::srgb(1.0, 1.0 - t2, 1.0 - t2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::STANDARD_GRAVITY_F32;
    use bevy::ecs::system::RunSystemOnce;

    const EARTH: Vec3 = Vec3::new(0.0, -STANDARD_GRAVITY_F32, 0.0);

    /// Net force on a still 1 kg air parcel at `temperature` K under `gravity`.
    fn net_force(temperature: f32, gravity: Vec3) -> Vec3 {
        let mut world = World::new();
        world.insert_resource(Workspace { gravity, ..Default::default() });
        let parcel = world
            .spawn(ThermodynamicParticleBundle::gas(Vec3::ZERO, 1.0, temperature))
            .id();
        world.run_system_once(apply_particle_forces).expect("the force system runs");
        world.get::<KineticState>(parcel).unwrap().accumulated_force
    }

    /// Air as dense as the air around it (1.225 kg/m³ at 288.5 K) floats,
    /// hotter air rises and colder air sinks.
    #[test]
    fn a_parcel_rises_or_sinks_by_its_density_against_the_air() {
        let neutral = net_force(288.5, EARTH);
        assert!(neutral.length() < 0.01 * STANDARD_GRAVITY_F32, "ambient air is pushed {neutral:?}");
        let hot = net_force(600.0, EARTH);
        assert!(hot.y > 0.5 * STANDARD_GRAVITY_F32, "600 K air does not rise: {hot:?}");
        let cold = net_force(200.0, EARTH);
        assert!(cold.y < 0.0, "200 K air does not sink: {cold:?}");
    }

    /// Weight and buoyancy both come from the Workspace gravity.
    #[test]
    fn particle_forces_follow_the_workspace_gravity() {
        let moon = Vec3::new(0.0, -1.62, 0.0);
        let ratio = net_force(600.0, moon).y / net_force(600.0, EARTH).y;
        assert!((ratio - 1.62 / STANDARD_GRAVITY_F32).abs() < 1e-4, "Moon/Earth force ratio {ratio}");
        assert_eq!(net_force(600.0, Vec3::ZERO), Vec3::ZERO, "weightless air is still pushed");
    }
}
