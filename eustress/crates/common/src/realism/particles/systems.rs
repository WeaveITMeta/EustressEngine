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
use rayon::prelude::*;

use super::components::*;
use super::spatial::SpatialHash;
use crate::realism::constants;
use crate::realism::laws::{thermodynamics, mechanics};
use crate::realism::RealismConfig;

// ============================================================================
// Spatial Hash Update
// ============================================================================

/// Update spatial hash with current particle positions
pub fn update_spatial_hash(
    mut spatial_hash: ResMut<SpatialHash>,
    query: Query<(Entity, &Transform), With<Particle>>,
    config: Res<RealismConfig>,
) {
    if !config.thermodynamics_enabled && !config.fluids_enabled {
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
    mut query: Query<(&Particle, &mut ThermodynamicState, &Transform)>,
    spatial_hash: Res<SpatialHash>,
    config: Res<RealismConfig>,
    time: Res<Time>,
) {
    if !config.thermodynamics_enabled {
        return;
    }
    
    let dt = time.delta_secs() * config.time_scale;
    if dt <= 0.0 {
        return;
    }
    
    // Collect positions and temperatures for heat transfer calculations
    let particle_data: Vec<(Entity, Vec3, f32)> = query
        .iter()
        .map(|(_, thermo, transform)| {
            (Entity::PLACEHOLDER, transform.translation, thermo.temperature)
        })
        .collect();
    
    // Update each particle's thermodynamic state
    for (particle, mut thermo, transform) in query.iter_mut() {
        if !particle.active {
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
    mut query: Query<(&Particle, &mut KineticState, &mut Transform)>,
    config: Res<RealismConfig>,
    time: Res<Time>,
) {
    let dt = time.delta_secs() * config.time_scale;
    if dt <= 0.0 {
        return;
    }
    
    // Parallel iteration for performance
    if config.parallel_enabled {
        query.par_iter_mut().for_each(|(particle, mut kinetic, mut transform)| {
            if !particle.active {
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
        for (particle, mut kinetic, mut transform) in query.iter_mut() {
            if !particle.active {
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
pub fn apply_particle_forces(
    mut query: Query<(&Particle, &mut KineticState, &Transform, Option<&ThermodynamicState>, Option<&FluidProperties>)>,
    config: Res<RealismConfig>,
) {
    if !config.thermodynamics_enabled && !config.fluids_enabled {
        return;
    }
    
    let gravity = Vec3::new(0.0, -9.81, 0.0);
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
                    // Hot gas rises: buoyancy = (ρ_air - ρ_gas) * V * g
                    let particle_density = thermo.density(0.029); // Assuming air-like gas
                    let volume = (4.0 / 3.0) * std::f32::consts::PI * particle.radius.powi(3);
                    let buoyancy = (air_density - particle_density) * volume * 9.81;
                    kinetic.apply_force(Vec3::new(0.0, buoyancy, 0.0));
                }
                _ => {}
            }
        }
        
        // Fluid-specific forces
        if let Some(fluid) = fluid {
            // Buoyancy in water
            if fluid.phase == FluidPhase::Liquid {
                let volume = (4.0 / 3.0) * std::f32::consts::PI * particle.radius.powi(3);
                let buoyancy = fluid.rest_density * volume * 9.81;
                kinetic.apply_force(Vec3::new(0.0, buoyancy, 0.0));
            }
        }
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
                commands.entity(entity).despawn_recursive();
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
    config: Res<RealismConfig>,
) {
    if !config.visualizers_enabled {
        return;
    }
    
    for (particle, transform, kinetic, thermo) in query.iter() {
        if !particle.active {
            continue;
        }
        
        // Color based on temperature if available
        let color = if let Some(thermo) = thermo {
            temperature_to_color(thermo.temperature)
        } else {
            Color::WHITE
        };
        
        // Draw particle sphere
        gizmos.sphere(
            Isometry3d::from_translation(transform.translation),
            particle.radius,
            color,
        );
        
        // Draw velocity vector
        if let Some(kinetic) = kinetic {
            if kinetic.velocity.length() > 0.01 {
                gizmos.arrow(
                    transform.translation,
                    transform.translation + kinetic.velocity * 0.1,
                    Color::srgb(0.0, 1.0, 0.0),
                );
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
