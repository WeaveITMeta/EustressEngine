//! # Newtonian Mechanics
//!
//! Implementation of classical mechanics laws and equations.
//!
//! ## Table of Contents
//!
//! 1. **Newton's Laws** - F=ma, action-reaction
//! 2. **Kinematics** - Position, velocity, acceleration
//! 3. **Energy** - Kinetic, potential, work-energy theorem
//! 4. **Momentum** - Linear, angular, impulse
//! 5. **Rotational Dynamics** - Torque, moment of inertia
//! 6. **Gravity** - Universal gravitation, orbital mechanics

use bevy::prelude::*;
use crate::realism::constants;

// ============================================================================
// Newton's Laws of Motion
// ============================================================================

/// Newton's Second Law: F = ma
/// 
/// # Arguments
/// * `mass` - Mass (kg)
/// * `acceleration` - Acceleration vector (m/s²)
/// 
/// # Returns
/// Force vector (N)
#[inline]
pub fn force_from_acceleration(mass: f32, acceleration: Vec3) -> Vec3 {
    mass * acceleration
}

/// Newton's Second Law (inverse): a = F/m
#[inline]
pub fn acceleration_from_force(force: Vec3, mass: f32) -> Vec3 {
    if mass <= 0.0 {
        return Vec3::ZERO;
    }
    force / mass
}

/// Net force from multiple forces
#[inline]
pub fn net_force(forces: &[Vec3]) -> Vec3 {
    forces.iter().copied().sum()
}

// ============================================================================
// Kinematics
// ============================================================================

/// Position update with constant velocity: x = x₀ + vt
#[inline]
pub fn position_constant_velocity(initial_position: Vec3, velocity: Vec3, time: f32) -> Vec3 {
    initial_position + velocity * time
}

/// Position update with constant acceleration: x = x₀ + v₀t + ½at²
#[inline]
pub fn position_constant_acceleration(
    initial_position: Vec3,
    initial_velocity: Vec3,
    acceleration: Vec3,
    time: f32,
) -> Vec3 {
    initial_position + initial_velocity * time + 0.5 * acceleration * time * time
}

/// Velocity update with constant acceleration: v = v₀ + at
#[inline]
pub fn velocity_constant_acceleration(
    initial_velocity: Vec3,
    acceleration: Vec3,
    time: f32,
) -> Vec3 {
    initial_velocity + acceleration * time
}

/// Velocity from position change: v² = v₀² + 2a·Δx
pub fn velocity_from_displacement(
    initial_velocity: f32,
    acceleration: f32,
    displacement: f32,
) -> f32 {
    let v_squared = initial_velocity * initial_velocity + 2.0 * acceleration * displacement;
    if v_squared < 0.0 {
        return 0.0;
    }
    v_squared.sqrt()
}

// ============================================================================
// Energy
// ============================================================================

/// Kinetic energy: KE = ½mv²
#[inline]
pub fn kinetic_energy(mass: f32, velocity: Vec3) -> f32 {
    0.5 * mass * velocity.length_squared()
}

/// Kinetic energy (scalar velocity): KE = ½mv²
#[inline]
pub fn kinetic_energy_scalar(mass: f32, speed: f32) -> f32 {
    0.5 * mass * speed * speed
}

/// Gravitational potential energy: PE = mgh
#[inline]
pub fn gravitational_potential_energy(mass: f32, gravity: f32, height: f32) -> f32 {
    mass * gravity * height
}

/// Gravitational potential energy (universal): U = -GMm/r
#[inline]
pub fn gravitational_potential_universal(m1: f32, m2: f32, distance: f32) -> f32 {
    if distance <= 0.0 {
        return f32::NEG_INFINITY;
    }
    -(constants::G_F32 * m1 * m2) / distance
}

/// Elastic potential energy: PE = ½kx²
#[inline]
pub fn elastic_potential_energy(spring_constant: f32, displacement: f32) -> f32 {
    0.5 * spring_constant * displacement * displacement
}

/// Work done by force: W = F·d
#[inline]
pub fn work(force: Vec3, displacement: Vec3) -> f32 {
    force.dot(displacement)
}

/// Work done by force at angle: W = Fd·cos(θ)
#[inline]
pub fn work_at_angle(force_magnitude: f32, displacement: f32, angle_radians: f32) -> f32 {
    force_magnitude * displacement * angle_radians.cos()
}

/// Power: P = W/t = F·v
#[inline]
pub fn power_from_work(work: f32, time: f32) -> f32 {
    if time <= 0.0 {
        return f32::INFINITY;
    }
    work / time
}

/// Power: P = F·v
#[inline]
pub fn power_from_force_velocity(force: Vec3, velocity: Vec3) -> f32 {
    force.dot(velocity)
}

// ============================================================================
// Momentum
// ============================================================================

/// Linear momentum: p = mv
#[inline]
pub fn momentum(mass: f32, velocity: Vec3) -> Vec3 {
    mass * velocity
}

/// Impulse: J = FΔt = Δp
#[inline]
pub fn impulse(force: Vec3, delta_time: f32) -> Vec3 {
    force * delta_time
}

/// Velocity change from impulse: Δv = J/m
#[inline]
pub fn velocity_change_from_impulse(impulse: Vec3, mass: f32) -> Vec3 {
    if mass <= 0.0 {
        return Vec3::ZERO;
    }
    impulse / mass
}

/// Elastic collision in 1D: final velocities
pub fn elastic_collision_1d(m1: f32, v1: f32, m2: f32, v2: f32) -> (f32, f32) {
    let total_mass = m1 + m2;
    if total_mass <= 0.0 {
        return (v1, v2);
    }
    
    let v1_final = ((m1 - m2) * v1 + 2.0 * m2 * v2) / total_mass;
    let v2_final = ((m2 - m1) * v2 + 2.0 * m1 * v1) / total_mass;
    (v1_final, v2_final)
}

/// Elastic collision in 3D
pub fn elastic_collision_3d(
    m1: f32, v1: Vec3, pos1: Vec3,
    m2: f32, v2: Vec3, pos2: Vec3,
) -> (Vec3, Vec3) {
    let total_mass = m1 + m2;
    if total_mass <= 0.0 {
        return (v1, v2);
    }
    
    let n = (pos2 - pos1).normalize_or_zero();
    let v_rel = v1 - v2;
    let v_rel_n = v_rel.dot(n);
    
    // Only collide if approaching
    if v_rel_n <= 0.0 {
        return (v1, v2);
    }
    
    let j = (2.0 * v_rel_n) / (1.0 / m1 + 1.0 / m2);
    
    let v1_final = v1 - (j / m1) * n;
    let v2_final = v2 + (j / m2) * n;
    
    (v1_final, v2_final)
}

/// Inelastic collision (objects stick together)
pub fn inelastic_collision(m1: f32, v1: Vec3, m2: f32, v2: Vec3) -> Vec3 {
    let total_mass = m1 + m2;
    if total_mass <= 0.0 {
        return Vec3::ZERO;
    }
    (m1 * v1 + m2 * v2) / total_mass
}

/// Coefficient of restitution: e = (v2' - v1')/(v1 - v2)
pub fn collision_with_restitution(
    m1: f32, v1: f32,
    m2: f32, v2: f32,
    restitution: f32,
) -> (f32, f32) {
    let total_mass = m1 + m2;
    if total_mass <= 0.0 {
        return (v1, v2);
    }
    
    // Conservation of momentum + restitution
    let v_cm = (m1 * v1 + m2 * v2) / total_mass;
    let v1_final = v_cm - restitution * m2 * (v1 - v2) / total_mass;
    let v2_final = v_cm + restitution * m1 * (v1 - v2) / total_mass;
    
    (v1_final, v2_final)
}

// ============================================================================
// Rotational Dynamics
// ============================================================================

/// Angular momentum: L = Iω
#[inline]
pub fn angular_momentum(moment_of_inertia: f32, angular_velocity: Vec3) -> Vec3 {
    moment_of_inertia * angular_velocity
}

/// Torque: τ = r × F
#[inline]
pub fn torque(position: Vec3, force: Vec3) -> Vec3 {
    position.cross(force)
}

/// Torque from angular acceleration: τ = Iα
#[inline]
pub fn torque_from_angular_acceleration(moment_of_inertia: f32, angular_acceleration: Vec3) -> Vec3 {
    moment_of_inertia * angular_acceleration
}

/// Angular acceleration from torque: α = τ/I
#[inline]
pub fn angular_acceleration_from_torque(torque: Vec3, moment_of_inertia: f32) -> Vec3 {
    if moment_of_inertia <= 0.0 {
        return Vec3::ZERO;
    }
    torque / moment_of_inertia
}

/// Rotational kinetic energy: KE = ½Iω²
#[inline]
pub fn rotational_kinetic_energy(moment_of_inertia: f32, angular_velocity: Vec3) -> f32 {
    0.5 * moment_of_inertia * angular_velocity.length_squared()
}

/// Moment of inertia for common shapes
pub mod moment_of_inertia {
    /// Solid sphere: I = (2/5)mr²
    #[inline]
    pub fn solid_sphere(mass: f32, radius: f32) -> f32 {
        0.4 * mass * radius * radius
    }
    
    /// Hollow sphere: I = (2/3)mr²
    #[inline]
    pub fn hollow_sphere(mass: f32, radius: f32) -> f32 {
        (2.0 / 3.0) * mass * radius * radius
    }
    
    /// Solid cylinder (about axis): I = (1/2)mr²
    #[inline]
    pub fn solid_cylinder(mass: f32, radius: f32) -> f32 {
        0.5 * mass * radius * radius
    }
    
    /// Hollow cylinder: I = mr²
    #[inline]
    pub fn hollow_cylinder(mass: f32, radius: f32) -> f32 {
        mass * radius * radius
    }
    
    /// Solid rod (about center): I = (1/12)mL²
    #[inline]
    pub fn solid_rod_center(mass: f32, length: f32) -> f32 {
        (1.0 / 12.0) * mass * length * length
    }
    
    /// Solid rod (about end): I = (1/3)mL²
    #[inline]
    pub fn solid_rod_end(mass: f32, length: f32) -> f32 {
        (1.0 / 3.0) * mass * length * length
    }
    
    /// Rectangular plate (about center): I = (1/12)m(a² + b²)
    #[inline]
    pub fn rectangular_plate(mass: f32, width: f32, height: f32) -> f32 {
        (1.0 / 12.0) * mass * (width * width + height * height)
    }
}

// ============================================================================
// Gravity
// ============================================================================

/// Gravitational force (universal): F = GMm/r²
#[inline]
pub fn gravitational_force(m1: f32, m2: f32, distance: f32) -> f32 {
    if distance <= 0.0 {
        return f32::INFINITY;
    }
    (constants::G_F32 * m1 * m2) / (distance * distance)
}

/// Gravitational force vector (attractive)
pub fn gravitational_force_vector(m1: f32, pos1: Vec3, m2: f32, pos2: Vec3) -> Vec3 {
    let r = pos2 - pos1;
    let distance = r.length();
    if distance <= 0.0 {
        return Vec3::ZERO;
    }
    
    let magnitude = gravitational_force(m1, m2, distance);
    r.normalize() * magnitude
}

/// Gravitational acceleration at surface: g = GM/r²
#[inline]
pub fn surface_gravity(mass: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return f32::INFINITY;
    }
    (constants::G_F32 * mass) / (radius * radius)
}

/// Escape velocity: v = √(2GM/r)
#[inline]
pub fn escape_velocity(mass: f32, radius: f32) -> f32 {
    if radius <= 0.0 {
        return f32::INFINITY;
    }
    (2.0 * constants::G_F32 * mass / radius).sqrt()
}

/// Orbital velocity (circular): v = √(GM/r)
#[inline]
pub fn orbital_velocity(central_mass: f32, orbital_radius: f32) -> f32 {
    if orbital_radius <= 0.0 {
        return f32::INFINITY;
    }
    (constants::G_F32 * central_mass / orbital_radius).sqrt()
}

/// Orbital period (Kepler's Third Law): T = 2π√(r³/GM)
pub fn orbital_period(central_mass: f32, semi_major_axis: f32) -> f32 {
    if central_mass <= 0.0 || semi_major_axis <= 0.0 {
        return f32::INFINITY;
    }
    2.0 * std::f32::consts::PI * (semi_major_axis.powi(3) / (constants::G_F32 * central_mass)).sqrt()
}

// ============================================================================
// Friction
// ============================================================================

/// Static friction force (maximum): f_s = μ_s N
#[inline]
pub fn static_friction_max(coefficient: f32, normal_force: f32) -> f32 {
    coefficient * normal_force
}

/// Kinetic friction force: f_k = μ_k N
#[inline]
pub fn kinetic_friction(coefficient: f32, normal_force: f32) -> f32 {
    coefficient * normal_force
}

/// Friction force vector (opposes motion)
pub fn friction_force_vector(
    coefficient: f32,
    normal_force: f32,
    velocity: Vec3,
) -> Vec3 {
    let speed = velocity.length();
    if speed < 1e-6 {
        return Vec3::ZERO;
    }
    
    let magnitude = kinetic_friction(coefficient, normal_force);
    -velocity.normalize() * magnitude
}

// ============================================================================
// Spring Forces
// ============================================================================

/// Hooke's Law: F = -kx
#[inline]
pub fn spring_force(spring_constant: f32, displacement: f32) -> f32 {
    -spring_constant * displacement
}

/// Spring force vector
pub fn spring_force_vector(
    spring_constant: f32,
    anchor: Vec3,
    position: Vec3,
    rest_length: f32,
) -> Vec3 {
    let delta = position - anchor;
    let current_length = delta.length();
    if current_length < 1e-6 {
        return Vec3::ZERO;
    }
    
    let displacement = current_length - rest_length;
    let direction = delta.normalize();
    -spring_constant * displacement * direction
}

/// Damped spring force: F = -kx - cv
pub fn damped_spring_force(
    spring_constant: f32,
    damping: f32,
    displacement: f32,
    velocity: f32,
) -> f32 {
    -spring_constant * displacement - damping * velocity
}

/// Spring-damper force vector
pub fn spring_damper_force_vector(
    spring_constant: f32,
    damping: f32,
    anchor: Vec3,
    position: Vec3,
    velocity: Vec3,
    rest_length: f32,
) -> Vec3 {
    let delta = position - anchor;
    let current_length = delta.length();
    if current_length < 1e-6 {
        return Vec3::ZERO;
    }
    
    let direction = delta.normalize();
    let displacement = current_length - rest_length;
    let velocity_along = velocity.dot(direction);
    
    let force_magnitude = -spring_constant * displacement - damping * velocity_along;
    force_magnitude * direction
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_newton_second_law() {
        let f = force_from_acceleration(10.0, Vec3::new(0.0, 9.81, 0.0));
        assert!((f.y - 98.1).abs() < 0.01);
    }
    
    #[test]
    fn test_kinetic_energy() {
        let ke = kinetic_energy(2.0, Vec3::new(3.0, 4.0, 0.0));
        // v = 5, KE = 0.5 * 2 * 25 = 25
        assert!((ke - 25.0).abs() < 0.01);
    }
    
    #[test]
    fn test_elastic_collision() {
        // Equal masses, one at rest
        let (v1, v2) = elastic_collision_1d(1.0, 10.0, 1.0, 0.0);
        assert!(v1.abs() < 0.01);
        assert!((v2 - 10.0).abs() < 0.01);
    }
    
    #[test]
    fn test_moment_of_inertia() {
        let i = moment_of_inertia::solid_sphere(10.0, 1.0);
        assert!((i - 4.0).abs() < 0.01);
    }
    
    #[test]
    fn test_orbital_velocity() {
        // Earth orbit around Sun (approximate)
        let v = orbital_velocity(1.989e30, 1.496e11);
        // Should be ~29.8 km/s
        assert!((v - 29800.0).abs() < 1000.0);
    }
}
