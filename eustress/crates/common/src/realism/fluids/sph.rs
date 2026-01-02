//! # Smoothed Particle Hydrodynamics (SPH)
//!
//! Particle-based fluid simulation using SPH method.
//!
//! ## Table of Contents
//!
//! 1. **Kernels** - Smoothing kernel functions
//! 2. **Density** - Density estimation
//! 3. **Pressure** - Pressure forces
//! 4. **Viscosity** - Viscous forces
//! 5. **Surface Tension** - Surface tension forces

use bevy::prelude::*;
use rayon::prelude::*;

use crate::realism::particles::components::{Particle, FluidProperties, KineticState};
use crate::realism::particles::spatial::SpatialHash;
use crate::realism::constants;

// ============================================================================
// SPH Configuration
// ============================================================================

/// SPH simulation configuration
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct SphConfig {
    /// Smoothing length (kernel radius)
    pub smoothing_length: f32,
    /// Rest density (kg/m³)
    pub rest_density: f32,
    /// Gas constant for pressure (stiffness)
    pub gas_constant: f32,
    /// Viscosity coefficient
    pub viscosity: f32,
    /// Surface tension coefficient
    pub surface_tension: f32,
    /// Gravity vector
    pub gravity: Vec3,
    /// Enable viscosity
    pub viscosity_enabled: bool,
    /// Enable surface tension
    pub surface_tension_enabled: bool,
}

impl Default for SphConfig {
    fn default() -> Self {
        Self {
            smoothing_length: 0.1,
            rest_density: constants::WATER_DENSITY,
            gas_constant: 2000.0,
            viscosity: constants::WATER_VISCOSITY * 1000.0, // Scaled for stability
            surface_tension: constants::WATER_SURFACE_TENSION,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            viscosity_enabled: true,
            surface_tension_enabled: true,
        }
    }
}

// ============================================================================
// SPH Kernels
// ============================================================================

/// Poly6 kernel for density estimation
/// W(r, h) = (315 / 64πh⁹) * (h² - r²)³ for r ≤ h
#[inline]
pub fn poly6_kernel(r: f32, h: f32) -> f32 {
    if r > h {
        return 0.0;
    }
    let h2 = h * h;
    let r2 = r * r;
    let diff = h2 - r2;
    let coeff = 315.0 / (64.0 * std::f32::consts::PI * h.powi(9));
    coeff * diff.powi(3)
}

/// Gradient of Poly6 kernel
#[inline]
pub fn poly6_gradient(r_vec: Vec3, h: f32) -> Vec3 {
    let r = r_vec.length();
    if r > h || r < 1e-6 {
        return Vec3::ZERO;
    }
    let h2 = h * h;
    let r2 = r * r;
    let diff = h2 - r2;
    let coeff = -945.0 / (32.0 * std::f32::consts::PI * h.powi(9));
    coeff * diff.powi(2) * r_vec
}

/// Laplacian of Poly6 kernel
#[inline]
pub fn poly6_laplacian(r: f32, h: f32) -> f32 {
    if r > h {
        return 0.0;
    }
    let h2 = h * h;
    let r2 = r * r;
    let coeff = -945.0 / (32.0 * std::f32::consts::PI * h.powi(9));
    coeff * (h2 - r2) * (3.0 * h2 - 7.0 * r2)
}

/// Spiky kernel gradient for pressure forces
/// ∇W(r, h) = -(45 / πh⁶) * (h - r)² * r̂ for r ≤ h
#[inline]
pub fn spiky_gradient(r_vec: Vec3, h: f32) -> Vec3 {
    let r = r_vec.length();
    if r > h || r < 1e-6 {
        return Vec3::ZERO;
    }
    let diff = h - r;
    let coeff = -45.0 / (std::f32::consts::PI * h.powi(6));
    coeff * diff.powi(2) / r * r_vec
}

/// Viscosity kernel Laplacian
/// ∇²W(r, h) = (45 / πh⁶) * (h - r) for r ≤ h
#[inline]
pub fn viscosity_laplacian(r: f32, h: f32) -> f32 {
    if r > h {
        return 0.0;
    }
    let coeff = 45.0 / (std::f32::consts::PI * h.powi(6));
    coeff * (h - r)
}

/// Cubic spline kernel (more stable)
#[inline]
pub fn cubic_spline_kernel(r: f32, h: f32) -> f32 {
    let q = r / h;
    let sigma = 8.0 / (std::f32::consts::PI * h.powi(3));
    
    if q <= 0.5 {
        sigma * (6.0 * (q.powi(3) - q.powi(2)) + 1.0)
    } else if q <= 1.0 {
        sigma * 2.0 * (1.0 - q).powi(3)
    } else {
        0.0
    }
}

/// Cubic spline kernel gradient
#[inline]
pub fn cubic_spline_gradient(r_vec: Vec3, h: f32) -> Vec3 {
    let r = r_vec.length();
    if r < 1e-6 || r > h {
        return Vec3::ZERO;
    }
    
    let q = r / h;
    let sigma = 48.0 / (std::f32::consts::PI * h.powi(4));
    let direction = r_vec / r;
    
    let grad_magnitude = if q <= 0.5 {
        sigma * q * (3.0 * q - 2.0)
    } else if q <= 1.0 {
        -sigma * (1.0 - q).powi(2)
    } else {
        0.0
    };
    
    grad_magnitude * direction
}

// ============================================================================
// Density Estimation
// ============================================================================

/// Estimate density at a particle using SPH
pub fn estimate_density(
    position: Vec3,
    neighbors: &[(Vec3, f32)], // (position, mass)
    smoothing_length: f32,
) -> f32 {
    neighbors.iter()
        .map(|(pos, mass)| {
            let r = (position - *pos).length();
            mass * poly6_kernel(r, smoothing_length)
        })
        .sum()
}

/// Calculate pressure from density using equation of state
/// P = k * (ρ - ρ₀) (Tait equation simplified)
#[inline]
pub fn pressure_from_density(density: f32, rest_density: f32, gas_constant: f32) -> f32 {
    gas_constant * (density - rest_density).max(0.0)
}

/// Tait equation of state (more stable for water)
/// P = B * ((ρ/ρ₀)^γ - 1)
#[inline]
pub fn tait_pressure(density: f32, rest_density: f32, bulk_modulus: f32, gamma: f32) -> f32 {
    bulk_modulus * ((density / rest_density).powf(gamma) - 1.0)
}

// ============================================================================
// Force Calculations
// ============================================================================

/// Calculate pressure force on a particle
pub fn pressure_force(
    particle_pos: Vec3,
    particle_density: f32,
    particle_pressure: f32,
    neighbors: &[(Vec3, f32, f32, f32)], // (pos, mass, density, pressure)
    smoothing_length: f32,
) -> Vec3 {
    if particle_density < 1e-6 {
        return Vec3::ZERO;
    }
    
    let mut force = Vec3::ZERO;
    
    for (pos, mass, density, pressure) in neighbors {
        if *density < 1e-6 {
            continue;
        }
        
        let r_vec = particle_pos - *pos;
        let grad_w = spiky_gradient(r_vec, smoothing_length);
        
        // Symmetric pressure term
        let pressure_term = (particle_pressure / (particle_density * particle_density))
                          + (*pressure / (*density * *density));
        
        force -= mass * pressure_term * grad_w;
    }
    
    force * particle_density
}

/// Calculate viscosity force on a particle
pub fn viscosity_force(
    particle_pos: Vec3,
    particle_velocity: Vec3,
    particle_density: f32,
    neighbors: &[(Vec3, f32, f32, Vec3)], // (pos, mass, density, velocity)
    smoothing_length: f32,
    viscosity: f32,
) -> Vec3 {
    if particle_density < 1e-6 {
        return Vec3::ZERO;
    }
    
    let mut force = Vec3::ZERO;
    
    for (pos, mass, density, velocity) in neighbors {
        if *density < 1e-6 {
            continue;
        }
        
        let r = (particle_pos - *pos).length();
        let lap_w = viscosity_laplacian(r, smoothing_length);
        let velocity_diff = *velocity - particle_velocity;
        
        force += (mass / *density) * velocity_diff * lap_w;
    }
    
    force * viscosity
}

/// Calculate surface tension force (simplified)
pub fn surface_tension_force(
    particle_pos: Vec3,
    neighbors: &[(Vec3, f32, f32)], // (pos, mass, density)
    smoothing_length: f32,
    surface_tension: f32,
) -> Vec3 {
    // Calculate color field gradient (normal)
    let mut normal = Vec3::ZERO;
    let mut laplacian = 0.0;
    
    for (pos, mass, density) in neighbors {
        if *density < 1e-6 {
            continue;
        }
        
        let r_vec = particle_pos - *pos;
        let r = r_vec.length();
        
        normal += (mass / *density) * poly6_gradient(r_vec, smoothing_length);
        laplacian += (mass / *density) * poly6_laplacian(r, smoothing_length);
    }
    
    let normal_length = normal.length();
    if normal_length < 0.01 {
        return Vec3::ZERO;
    }
    
    -surface_tension * laplacian * normal / normal_length
}

// ============================================================================
// Systems
// ============================================================================

/// Update SPH density for fluid particles
pub fn update_sph_density(
    mut query: Query<(Entity, &Transform, &Particle, &mut FluidProperties)>,
    spatial_hash: Res<SpatialHash>,
    config: Res<SphConfig>,
) {
    // Collect particle data for neighbor queries
    let particle_data: Vec<(Entity, Vec3, f32)> = query
        .iter()
        .map(|(e, t, p, _)| (e, t.translation, p.mass))
        .collect();
    
    // Update density for each particle
    for (entity, transform, particle, mut fluid) in query.iter_mut() {
        let neighbors = spatial_hash.query_radius_with_positions(
            transform.translation,
            config.smoothing_length * 2.0,
        );
        
        // Build neighbor data
        let neighbor_data: Vec<(Vec3, f32)> = neighbors.iter()
            .filter(|(e, _)| *e != entity)
            .filter_map(|(_, pos)| {
                particle_data.iter()
                    .find(|(e, p, _)| (*p - *pos).length() < 0.01)
                    .map(|(_, _, mass)| (*pos, *mass))
            })
            .collect();
        
        // Estimate density
        let mut density = particle.mass * poly6_kernel(0.0, config.smoothing_length);
        for (pos, mass) in &neighbor_data {
            let r = (transform.translation - *pos).length();
            density += mass * poly6_kernel(r, config.smoothing_length);
        }
        
        fluid.density = density.max(1.0); // Prevent zero density
    }
}

/// Update SPH forces for fluid particles
pub fn update_sph_forces(
    mut query: Query<(Entity, &Transform, &Particle, &FluidProperties, &mut KineticState)>,
    spatial_hash: Res<SpatialHash>,
    config: Res<SphConfig>,
) {
    // Collect all particle data
    let particle_data: Vec<(Entity, Vec3, f32, f32, f32, Vec3)> = query
        .iter()
        .map(|(e, t, p, f, k)| {
            let pressure = pressure_from_density(f.density, config.rest_density, config.gas_constant);
            (e, t.translation, p.mass, f.density, pressure, k.velocity)
        })
        .collect();
    
    // Calculate forces for each particle
    for (entity, transform, particle, fluid, mut kinetic) in query.iter_mut() {
        let particle_pressure = pressure_from_density(fluid.density, config.rest_density, config.gas_constant);
        
        // Get neighbors
        let neighbors = spatial_hash.query_radius_with_positions(
            transform.translation,
            config.smoothing_length * 2.0,
        );
        
        // Build neighbor data for pressure force
        let pressure_neighbors: Vec<(Vec3, f32, f32, f32)> = neighbors.iter()
            .filter(|(e, _)| *e != entity)
            .filter_map(|(_, pos)| {
                particle_data.iter()
                    .find(|(_, p, _, _, _, _)| (*p - *pos).length() < 0.01)
                    .map(|(_, _, mass, density, pressure, _)| (*pos, *mass, *density, *pressure))
            })
            .collect();
        
        // Pressure force
        let f_pressure = pressure_force(
            transform.translation,
            fluid.density,
            particle_pressure,
            &pressure_neighbors,
            config.smoothing_length,
        );
        kinetic.apply_force(f_pressure);
        
        // Viscosity force
        if config.viscosity_enabled {
            let viscosity_neighbors: Vec<(Vec3, f32, f32, Vec3)> = neighbors.iter()
                .filter(|(e, _)| *e != entity)
                .filter_map(|(_, pos)| {
                    particle_data.iter()
                        .find(|(_, p, _, _, _, _)| (*p - *pos).length() < 0.01)
                        .map(|(_, _, mass, density, _, velocity)| (*pos, *mass, *density, *velocity))
                })
                .collect();
            
            let f_viscosity = viscosity_force(
                transform.translation,
                kinetic.velocity,
                fluid.density,
                &viscosity_neighbors,
                config.smoothing_length,
                config.viscosity,
            );
            kinetic.apply_force(f_viscosity);
        }
        
        // Surface tension
        if config.surface_tension_enabled {
            let surface_neighbors: Vec<(Vec3, f32, f32)> = pressure_neighbors.iter()
                .map(|(pos, mass, density, _)| (*pos, *mass, *density))
                .collect();
            
            let f_surface = surface_tension_force(
                transform.translation,
                &surface_neighbors,
                config.smoothing_length,
                config.surface_tension,
            );
            kinetic.apply_force(f_surface);
        }
        
        // Gravity
        kinetic.apply_force(particle.mass * config.gravity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_poly6_kernel() {
        let h = 0.1;
        
        // At r=0, kernel should be maximum
        let w0 = poly6_kernel(0.0, h);
        assert!(w0 > 0.0);
        
        // At r=h, kernel should be 0
        let wh = poly6_kernel(h, h);
        assert!(wh.abs() < 1e-6);
        
        // At r>h, kernel should be 0
        let w_far = poly6_kernel(h * 1.5, h);
        assert_eq!(w_far, 0.0);
    }
    
    #[test]
    fn test_pressure_from_density() {
        let rest = 1000.0;
        let k = 2000.0;
        
        // At rest density, pressure should be 0
        let p_rest = pressure_from_density(rest, rest, k);
        assert_eq!(p_rest, 0.0);
        
        // Above rest density, pressure should be positive
        let p_high = pressure_from_density(1100.0, rest, k);
        assert!(p_high > 0.0);
    }
}
