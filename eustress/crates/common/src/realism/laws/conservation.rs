//! # Conservation Laws
//!
//! Implementation of fundamental conservation principles.
//!
//! ## Table of Contents
//!
//! 1. **Mass Conservation** - Continuity equation
//! 2. **Energy Conservation** - Total mechanical energy
//! 3. **Momentum Conservation** - Linear and angular
//! 4. **Charge Conservation** - (Future) Electromagnetic

use bevy::prelude::*;

// ============================================================================
// Mass Conservation
// ============================================================================

/// Check mass conservation in a system
/// Returns the difference from initial mass (should be ~0)
#[inline]
pub fn mass_conservation_check(initial_mass: f32, current_masses: &[f32]) -> f32 {
    let current_total: f32 = current_masses.iter().sum();
    current_total - initial_mass
}

/// Continuity equation for fluids: ρ₁A₁v₁ = ρ₂A₂v₂
/// Returns mass flow rate (kg/s)
#[inline]
pub fn mass_flow_rate(density: f32, area: f32, velocity: f32) -> f32 {
    density * area * velocity
}

/// Volume flow rate: Q = Av
#[inline]
pub fn volume_flow_rate(area: f32, velocity: f32) -> f32 {
    area * velocity
}

/// Velocity from continuity: v₂ = (A₁/A₂)v₁ (incompressible)
#[inline]
pub fn velocity_from_continuity(area1: f32, velocity1: f32, area2: f32) -> f32 {
    if area2 <= 0.0 {
        return f32::INFINITY;
    }
    (area1 / area2) * velocity1
}

// ============================================================================
// Energy Conservation
// ============================================================================

/// Total mechanical energy: E = KE + PE
#[inline]
pub fn total_mechanical_energy(kinetic: f32, potential: f32) -> f32 {
    kinetic + potential
}

/// Check energy conservation
/// Returns the difference from initial energy (should be ~0 for conservative systems)
#[inline]
pub fn energy_conservation_check(initial_energy: f32, current_energy: f32) -> f32 {
    current_energy - initial_energy
}

/// Energy dissipated by non-conservative forces
#[inline]
pub fn energy_dissipated(initial_energy: f32, final_energy: f32) -> f32 {
    initial_energy - final_energy
}

/// Bernoulli's equation: P₁ + ½ρv₁² + ρgh₁ = P₂ + ½ρv₂² + ρgh₂
/// Returns the Bernoulli constant for a streamline
#[inline]
pub fn bernoulli_constant(pressure: f32, density: f32, velocity: f32, height: f32, gravity: f32) -> f32 {
    pressure + 0.5 * density * velocity * velocity + density * gravity * height
}

/// Pressure from Bernoulli's equation
pub fn bernoulli_pressure(
    p1: f32, v1: f32, h1: f32,
    v2: f32, h2: f32,
    density: f32, gravity: f32,
) -> f32 {
    p1 + 0.5 * density * (v1 * v1 - v2 * v2) + density * gravity * (h1 - h2)
}

/// Velocity from Bernoulli's equation (Torricelli's theorem for tank drainage)
/// v = √(2gh) for free drainage
#[inline]
pub fn torricelli_velocity(gravity: f32, height: f32) -> f32 {
    (2.0 * gravity * height).sqrt()
}

// ============================================================================
// Momentum Conservation
// ============================================================================

/// Total momentum of a system
pub fn total_momentum(masses: &[f32], velocities: &[Vec3]) -> Vec3 {
    masses.iter()
        .zip(velocities.iter())
        .map(|(m, v)| *m * *v)
        .sum()
}

/// Check momentum conservation
/// Returns the difference vector from initial momentum
pub fn momentum_conservation_check(initial_momentum: Vec3, current_momentum: Vec3) -> Vec3 {
    current_momentum - initial_momentum
}

/// Center of mass position
pub fn center_of_mass(masses: &[f32], positions: &[Vec3]) -> Vec3 {
    let total_mass: f32 = masses.iter().sum();
    if total_mass <= 0.0 {
        return Vec3::ZERO;
    }
    
    let weighted_sum: Vec3 = masses.iter()
        .zip(positions.iter())
        .map(|(m, p)| *m * *p)
        .sum();
    
    weighted_sum / total_mass
}

/// Center of mass velocity
pub fn center_of_mass_velocity(masses: &[f32], velocities: &[Vec3]) -> Vec3 {
    let total_mass: f32 = masses.iter().sum();
    if total_mass <= 0.0 {
        return Vec3::ZERO;
    }
    
    total_momentum(masses, velocities) / total_mass
}

// ============================================================================
// Angular Momentum Conservation
// ============================================================================

/// Angular momentum of a point mass: L = r × p = r × mv
#[inline]
pub fn angular_momentum_point(position: Vec3, mass: f32, velocity: Vec3) -> Vec3 {
    position.cross(mass * velocity)
}

/// Total angular momentum of a system
pub fn total_angular_momentum(
    positions: &[Vec3],
    masses: &[f32],
    velocities: &[Vec3],
) -> Vec3 {
    positions.iter()
        .zip(masses.iter())
        .zip(velocities.iter())
        .map(|((p, m), v)| angular_momentum_point(*p, *m, *v))
        .sum()
}

/// Check angular momentum conservation
pub fn angular_momentum_conservation_check(initial: Vec3, current: Vec3) -> Vec3 {
    current - initial
}

// ============================================================================
// Conservation System Tracker
// ============================================================================

/// Tracks conservation quantities for a physical system
#[derive(Debug, Clone, Reflect)]
pub struct ConservationTracker {
    /// Initial total mass
    pub initial_mass: f32,
    /// Initial total energy
    pub initial_energy: f32,
    /// Initial total momentum
    pub initial_momentum: Vec3,
    /// Initial total angular momentum
    pub initial_angular_momentum: Vec3,
    /// Tolerance for conservation checks
    pub tolerance: f32,
}

impl Default for ConservationTracker {
    fn default() -> Self {
        Self {
            initial_mass: 0.0,
            initial_energy: 0.0,
            initial_momentum: Vec3::ZERO,
            initial_angular_momentum: Vec3::ZERO,
            tolerance: 1e-6,
        }
    }
}

impl ConservationTracker {
    /// Initialize tracker with current system state
    pub fn initialize(
        &mut self,
        masses: &[f32],
        positions: &[Vec3],
        velocities: &[Vec3],
        potential_energies: &[f32],
    ) {
        self.initial_mass = masses.iter().sum();
        
        let kinetic: f32 = masses.iter()
            .zip(velocities.iter())
            .map(|(m, v)| 0.5 * m * v.length_squared())
            .sum();
        let potential: f32 = potential_energies.iter().sum();
        self.initial_energy = kinetic + potential;
        
        self.initial_momentum = total_momentum(masses, velocities);
        self.initial_angular_momentum = total_angular_momentum(positions, masses, velocities);
    }
    
    /// Check all conservation laws
    pub fn check(
        &self,
        masses: &[f32],
        positions: &[Vec3],
        velocities: &[Vec3],
        potential_energies: &[f32],
    ) -> ConservationResult {
        let current_mass: f32 = masses.iter().sum();
        let mass_error = (current_mass - self.initial_mass).abs();
        
        let kinetic: f32 = masses.iter()
            .zip(velocities.iter())
            .map(|(m, v)| 0.5 * m * v.length_squared())
            .sum();
        let potential: f32 = potential_energies.iter().sum();
        let current_energy = kinetic + potential;
        let energy_error = (current_energy - self.initial_energy).abs();
        
        let current_momentum = total_momentum(masses, velocities);
        let momentum_error = (current_momentum - self.initial_momentum).length();
        
        let current_angular = total_angular_momentum(positions, masses, velocities);
        let angular_error = (current_angular - self.initial_angular_momentum).length();
        
        ConservationResult {
            mass_conserved: mass_error < self.tolerance,
            energy_conserved: energy_error < self.tolerance * self.initial_energy.abs().max(1.0),
            momentum_conserved: momentum_error < self.tolerance * self.initial_momentum.length().max(1.0),
            angular_momentum_conserved: angular_error < self.tolerance * self.initial_angular_momentum.length().max(1.0),
            mass_error,
            energy_error,
            momentum_error,
            angular_momentum_error: angular_error,
        }
    }
}

/// Result of conservation law checks
#[derive(Debug, Clone, Reflect)]
pub struct ConservationResult {
    pub mass_conserved: bool,
    pub energy_conserved: bool,
    pub momentum_conserved: bool,
    pub angular_momentum_conserved: bool,
    pub mass_error: f32,
    pub energy_error: f32,
    pub momentum_error: f32,
    pub angular_momentum_error: f32,
}

impl ConservationResult {
    /// Check if all conservation laws are satisfied
    pub fn all_conserved(&self) -> bool {
        self.mass_conserved 
            && self.energy_conserved 
            && self.momentum_conserved 
            && self.angular_momentum_conserved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mass_flow_rate() {
        // Water in pipe: ρ=1000, A=0.01m², v=2m/s
        let mdot = mass_flow_rate(1000.0, 0.01, 2.0);
        assert!((mdot - 20.0).abs() < 0.01);
    }
    
    #[test]
    fn test_bernoulli() {
        // Constant height, pressure drop = velocity increase
        let p2 = bernoulli_pressure(
            101325.0, 1.0, 0.0,  // P1, v1, h1
            10.0, 0.0,           // v2, h2
            1000.0, 9.81,        // density, gravity
        );
        // P2 should be lower due to higher velocity
        assert!(p2 < 101325.0);
    }
    
    #[test]
    fn test_center_of_mass() {
        let masses = [1.0, 1.0];
        let positions = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)];
        let com = center_of_mass(&masses, &positions);
        assert!((com.x - 1.0).abs() < 0.01);
    }
    
    #[test]
    fn test_torricelli() {
        // Water draining from 5m height
        let v = torricelli_velocity(9.81, 5.0);
        // v = √(2*9.81*5) ≈ 9.9 m/s
        assert!((v - 9.9).abs() < 0.1);
    }
}
