//! # Bose-Einstein Condensates
//!
//! Simulation of Bose-Einstein condensation and superfluidity.
//!
//! ## Table of Contents
//!
//! 1. **BEC Transition** - Critical temperature calculation
//! 2. **Condensate Fraction** - Ground state occupation
//! 3. **Gross-Pitaevskii** - Mean-field dynamics

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::statistics::{K_B, HBAR, PLANCK};

// ============================================================================
// BEC Component
// ============================================================================

/// Bose-Einstein condensate state
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BoseEinsteinCondensate {
    /// Number of particles
    pub particle_count: f64,
    /// Particle mass (kg)
    pub particle_mass: f64,
    /// Trap frequency ω (rad/s) for harmonic trap
    pub trap_frequency: f64,
    /// Scattering length a (m)
    pub scattering_length: f64,
    /// Current temperature (K)
    pub temperature: f64,
    /// Critical temperature T_c (K)
    pub critical_temperature: f64,
    /// Condensate fraction N_0/N
    pub condensate_fraction: f64,
    /// Chemical potential μ (J)
    pub chemical_potential: f64,
    /// Is system in condensed phase
    pub is_condensed: bool,
}

impl Default for BoseEinsteinCondensate {
    fn default() -> Self {
        Self {
            particle_count: 1e6,
            particle_mass: 1.44e-25, // Rb-87
            trap_frequency: 2.0 * std::f64::consts::PI * 100.0, // 100 Hz
            scattering_length: 5.3e-9, // Rb-87
            temperature: 100e-9, // 100 nK
            critical_temperature: 0.0,
            condensate_fraction: 0.0,
            chemical_potential: 0.0,
            is_condensed: false,
        }
    }
}

impl BoseEinsteinCondensate {
    /// Create BEC for Rubidium-87
    pub fn rubidium_87(particle_count: f64, trap_frequency: f64) -> Self {
        let mut bec = Self {
            particle_count,
            particle_mass: 1.44e-25,
            trap_frequency,
            scattering_length: 5.3e-9,
            ..default()
        };
        bec.update();
        bec
    }
    
    /// Create BEC for Sodium-23
    pub fn sodium_23(particle_count: f64, trap_frequency: f64) -> Self {
        let mut bec = Self {
            particle_count,
            particle_mass: 3.82e-26,
            trap_frequency,
            scattering_length: 2.75e-9,
            ..default()
        };
        bec.update();
        bec
    }
    
    /// Update derived quantities
    pub fn update(&mut self) {
        self.critical_temperature = self.calculate_critical_temperature();
        self.condensate_fraction = self.calculate_condensate_fraction();
        self.chemical_potential = self.calculate_chemical_potential();
        self.is_condensed = self.temperature < self.critical_temperature;
    }
    
    /// Calculate critical temperature for ideal gas in harmonic trap
    /// T_c = (ℏω/k_B) * (N/ζ(3))^(1/3)
    fn calculate_critical_temperature(&self) -> f64 {
        let zeta_3 = 1.202; // Riemann zeta(3)
        let prefactor = HBAR * self.trap_frequency / K_B;
        prefactor * (self.particle_count / zeta_3).powf(1.0 / 3.0)
    }
    
    /// Calculate condensate fraction
    /// N_0/N = 1 - (T/T_c)³ for T < T_c
    fn calculate_condensate_fraction(&self) -> f64 {
        if self.critical_temperature <= 0.0 || self.temperature >= self.critical_temperature {
            return 0.0;
        }
        
        let ratio = self.temperature / self.critical_temperature;
        (1.0 - ratio.powi(3)).max(0.0)
    }
    
    /// Calculate chemical potential in Thomas-Fermi approximation
    /// μ = (15N a/a_ho)^(2/5) * ℏω/2
    fn calculate_chemical_potential(&self) -> f64 {
        if !self.is_condensed {
            return 0.0;
        }
        
        let a_ho = harmonic_oscillator_length(self.particle_mass, self.trap_frequency);
        let factor = 15.0 * self.particle_count * self.scattering_length / a_ho;
        0.5 * HBAR * self.trap_frequency * factor.powf(2.0 / 5.0)
    }
    
    /// Get number of condensed particles
    pub fn condensed_particles(&self) -> f64 {
        self.particle_count * self.condensate_fraction
    }
    
    /// Get number of thermal particles
    pub fn thermal_particles(&self) -> f64 {
        self.particle_count * (1.0 - self.condensate_fraction)
    }
    
    /// Thomas-Fermi radius: R_TF = a_ho * (15N a/a_ho)^(1/5)
    pub fn thomas_fermi_radius(&self) -> f64 {
        if !self.is_condensed {
            return 0.0;
        }
        
        let a_ho = harmonic_oscillator_length(self.particle_mass, self.trap_frequency);
        let factor = 15.0 * self.particle_count * self.scattering_length / a_ho;
        a_ho * factor.powf(1.0 / 5.0)
    }
    
    /// Healing length: ξ = 1/√(8πna)
    pub fn healing_length(&self) -> f64 {
        let n = self.peak_density();
        if n <= 0.0 || self.scattering_length <= 0.0 {
            return f64::INFINITY;
        }
        1.0 / (8.0 * std::f64::consts::PI * n * self.scattering_length).sqrt()
    }
    
    /// Peak density in Thomas-Fermi approximation
    pub fn peak_density(&self) -> f64 {
        let r_tf = self.thomas_fermi_radius();
        if r_tf <= 0.0 {
            return 0.0;
        }
        // n_0 = 15N / (8π R_TF³)
        15.0 * self.condensed_particles() / (8.0 * std::f64::consts::PI * r_tf.powi(3))
    }
    
    /// Speed of sound: c = √(gn/m) where g = 4πℏ²a/m
    pub fn speed_of_sound(&self) -> f64 {
        let g = interaction_strength(self.particle_mass, self.scattering_length);
        let n = self.peak_density();
        (g * n / self.particle_mass).sqrt()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Harmonic oscillator length: a_ho = √(ℏ/(mω))
pub fn harmonic_oscillator_length(mass: f64, omega: f64) -> f64 {
    if mass <= 0.0 || omega <= 0.0 {
        return f64::INFINITY;
    }
    (HBAR / (mass * omega)).sqrt()
}

/// Interaction strength: g = 4πℏ²a/m
pub fn interaction_strength(mass: f64, scattering_length: f64) -> f64 {
    4.0 * std::f64::consts::PI * HBAR.powi(2) * scattering_length / mass
}

/// Critical temperature for uniform ideal Bose gas
/// T_c = (2πℏ²/mk_B) * (n/ζ(3/2))^(2/3)
pub fn critical_temperature_uniform(number_density: f64, mass: f64) -> f64 {
    let zeta_3_2 = 2.612; // Riemann zeta(3/2)
    let prefactor = 2.0 * std::f64::consts::PI * HBAR.powi(2) / (mass * K_B);
    prefactor * (number_density / zeta_3_2).powf(2.0 / 3.0)
}

/// Condensate fraction for uniform gas: N_0/N = 1 - (T/T_c)^(3/2)
pub fn condensate_fraction_uniform(temperature: f64, critical_temperature: f64) -> f64 {
    if critical_temperature <= 0.0 || temperature >= critical_temperature {
        return 0.0;
    }
    (1.0 - (temperature / critical_temperature).powf(1.5)).max(0.0)
}

// ============================================================================
// Gross-Pitaevskii Equation (simplified)
// ============================================================================

/// Gross-Pitaevskii energy functional (per particle)
/// E/N = ∫ [ℏ²/(2m)|∇ψ|² + V|ψ|² + g/2 |ψ|⁴] dr
pub struct GrossPitaevskiiParams {
    /// Particle mass
    pub mass: f64,
    /// Interaction strength g
    pub interaction: f64,
    /// External potential V(r)
    pub potential: Box<dyn Fn(f64, f64, f64) -> f64 + Send + Sync>,
}

impl Default for GrossPitaevskiiParams {
    fn default() -> Self {
        Self {
            mass: 1.44e-25,
            interaction: 1e-50,
            potential: Box::new(|x, y, z| {
                // Harmonic trap
                let omega = 2.0 * std::f64::consts::PI * 100.0;
                let m = 1.44e-25;
                0.5 * m * omega.powi(2) * (x * x + y * y + z * z)
            }),
        }
    }
}

/// Thomas-Fermi density profile: n(r) = (μ - V(r)) / g for μ > V(r)
pub fn thomas_fermi_density(
    position: (f64, f64, f64),
    chemical_potential: f64,
    interaction: f64,
    potential: impl Fn(f64, f64, f64) -> f64,
) -> f64 {
    let v = potential(position.0, position.1, position.2);
    if chemical_potential > v && interaction > 0.0 {
        (chemical_potential - v) / interaction
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bec_critical_temperature() {
        let bec = BoseEinsteinCondensate::rubidium_87(1e6, 2.0 * std::f64::consts::PI * 100.0);
        
        // T_c should be on order of 100 nK for typical BEC
        assert!(bec.critical_temperature > 1e-9);
        assert!(bec.critical_temperature < 1e-6);
    }
    
    #[test]
    fn test_condensate_fraction() {
        let mut bec = BoseEinsteinCondensate::rubidium_87(1e6, 2.0 * std::f64::consts::PI * 100.0);
        
        // At T = 0, fraction should be 1
        bec.temperature = 0.0;
        bec.update();
        assert!((bec.condensate_fraction - 1.0).abs() < 0.01);
        
        // At T = T_c, fraction should be 0
        bec.temperature = bec.critical_temperature;
        bec.update();
        assert!(bec.condensate_fraction < 0.01);
    }
    
    #[test]
    fn test_harmonic_oscillator_length() {
        let m = 1.44e-25; // Rb-87
        let omega = 2.0 * std::f64::consts::PI * 100.0;
        let a_ho = harmonic_oscillator_length(m, omega);
        
        // Should be on order of 1 μm
        assert!(a_ho > 1e-7);
        assert!(a_ho < 1e-5);
    }
}
