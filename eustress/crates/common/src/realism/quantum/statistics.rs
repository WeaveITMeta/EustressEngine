//! # Quantum Statistics
//!
//! Bose-Einstein and Fermi-Dirac distributions for quantum systems.
//!
//! ## Table of Contents
//!
//! 1. **Distributions** - BE, FD occupation numbers
//! 2. **Partition Functions** - Quantum partition functions
//! 3. **Thermodynamic Quantities** - Energy, entropy, heat capacity

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::realism::constants;

// ============================================================================
// Constants
// ============================================================================

/// Planck constant (J·s)
pub const PLANCK: f64 = 6.62607015e-34;

/// Reduced Planck constant ℏ = h/(2π)
pub const HBAR: f64 = 1.054571817e-34;

/// Boltzmann constant (J/K)
pub const K_B: f64 = 1.380649e-23;

// ============================================================================
// Quantum State Component
// ============================================================================

/// Quantum state for a particle or system
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct QuantumState {
    /// Particle type (boson or fermion)
    pub particle_type: QuantumParticleType,
    /// Chemical potential μ (J)
    pub chemical_potential: f64,
    /// Degeneracy factor g
    pub degeneracy: f64,
    /// Spin quantum number
    pub spin: f64,
    /// Energy level (J)
    pub energy: f64,
    /// Occupation number ⟨n⟩
    pub occupation: f64,
}

impl Default for QuantumState {
    fn default() -> Self {
        Self {
            particle_type: QuantumParticleType::Boson,
            chemical_potential: 0.0,
            degeneracy: 1.0,
            spin: 0.0,
            energy: 0.0,
            occupation: 0.0,
        }
    }
}

/// Quantum particle type
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum QuantumParticleType {
    /// Integer spin (photons, phonons, He-4)
    #[default]
    Boson,
    /// Half-integer spin (electrons, protons, He-3)
    Fermion,
    /// Classical (Maxwell-Boltzmann) approximation
    Classical,
}

// ============================================================================
// Distribution Functions
// ============================================================================

/// Bose-Einstein distribution: ⟨n⟩ = 1 / (exp((ε-μ)/(k_B·T)) - 1)
/// 
/// # Arguments
/// * `energy` - Energy level ε (J)
/// * `chemical_potential` - Chemical potential μ (J)
/// * `temperature` - Temperature T (K)
/// 
/// # Returns
/// Average occupation number
pub fn bose_einstein_distribution(energy: f64, chemical_potential: f64, temperature: f64) -> f64 {
    if temperature <= 0.0 {
        return 0.0;
    }
    
    let x = (energy - chemical_potential) / (K_B * temperature);
    
    // Prevent overflow/underflow
    if x > 700.0 {
        return 0.0;
    }
    if x < -700.0 {
        return f64::INFINITY; // Condensation
    }
    
    let exp_x = x.exp();
    if exp_x <= 1.0 {
        return f64::INFINITY; // μ ≥ ε not allowed for bosons at finite T
    }
    
    1.0 / (exp_x - 1.0)
}

/// Fermi-Dirac distribution: ⟨n⟩ = 1 / (exp((ε-μ)/(k_B·T)) + 1)
/// 
/// # Arguments
/// * `energy` - Energy level ε (J)
/// * `chemical_potential` - Chemical potential μ (J), also called Fermi energy at T=0
/// * `temperature` - Temperature T (K)
/// 
/// # Returns
/// Average occupation number (0 to 1)
pub fn fermi_dirac_distribution(energy: f64, chemical_potential: f64, temperature: f64) -> f64 {
    if temperature <= 0.0 {
        // T=0 limit: step function at Fermi energy
        return if energy < chemical_potential { 1.0 } else { 0.0 };
    }
    
    let x = (energy - chemical_potential) / (K_B * temperature);
    
    // Prevent overflow
    if x > 700.0 {
        return 0.0;
    }
    if x < -700.0 {
        return 1.0;
    }
    
    1.0 / (x.exp() + 1.0)
}

/// Maxwell-Boltzmann distribution (classical limit): ⟨n⟩ = exp(-(ε-μ)/(k_B·T))
pub fn maxwell_boltzmann_distribution(energy: f64, chemical_potential: f64, temperature: f64) -> f64 {
    if temperature <= 0.0 {
        return 0.0;
    }
    
    let x = (energy - chemical_potential) / (K_B * temperature);
    
    if x > 700.0 {
        return 0.0;
    }
    
    (-x).exp()
}

/// Get occupation number based on particle type
pub fn occupation_number(
    particle_type: QuantumParticleType,
    energy: f64,
    chemical_potential: f64,
    temperature: f64,
) -> f64 {
    match particle_type {
        QuantumParticleType::Boson => bose_einstein_distribution(energy, chemical_potential, temperature),
        QuantumParticleType::Fermion => fermi_dirac_distribution(energy, chemical_potential, temperature),
        QuantumParticleType::Classical => maxwell_boltzmann_distribution(energy, chemical_potential, temperature),
    }
}

// ============================================================================
// Partition Functions
// ============================================================================

/// Single-particle partition function for ideal gas: Z₁ = V/λ³
/// where λ = h/√(2πmk_B T) is the thermal de Broglie wavelength
pub fn ideal_gas_partition_function(volume: f64, mass: f64, temperature: f64) -> f64 {
    let lambda = thermal_de_broglie_wavelength(mass, temperature);
    volume / lambda.powi(3)
}

/// Thermal de Broglie wavelength: λ = h/√(2πmk_B T)
pub fn thermal_de_broglie_wavelength(mass: f64, temperature: f64) -> f64 {
    if mass <= 0.0 || temperature <= 0.0 {
        return f64::INFINITY;
    }
    PLANCK / (2.0 * std::f64::consts::PI * mass * K_B * temperature).sqrt()
}

/// Quantum concentration: n_Q = 1/λ³
pub fn quantum_concentration(mass: f64, temperature: f64) -> f64 {
    let lambda = thermal_de_broglie_wavelength(mass, temperature);
    1.0 / lambda.powi(3)
}

/// Check if quantum effects are significant: n/n_Q > 1
pub fn is_quantum_regime(number_density: f64, mass: f64, temperature: f64) -> bool {
    let n_q = quantum_concentration(mass, temperature);
    number_density > n_q
}

// ============================================================================
// Thermodynamic Quantities
// ============================================================================

/// Photon gas energy density: u = (π²/15) * (k_B T)⁴ / (ℏ³c³)
pub fn photon_gas_energy_density(temperature: f64) -> f64 {
    let c = 299792458.0; // Speed of light
    let coeff = std::f64::consts::PI.powi(2) / 15.0;
    coeff * (K_B * temperature).powi(4) / (HBAR.powi(3) * c.powi(3))
}

/// Stefan-Boltzmann law: P = σT⁴ (power per unit area)
pub fn stefan_boltzmann_power(temperature: f64) -> f64 {
    constants::STEFAN_BOLTZMANN as f64 * temperature.powi(4)
}

/// Debye model heat capacity: C_V = 9Nk_B(T/Θ_D)³ ∫₀^(Θ_D/T) x⁴eˣ/(eˣ-1)² dx
/// 
/// # Arguments
/// * `temperature` - Temperature T (K)
/// * `debye_temperature` - Debye temperature Θ_D (K)
/// * `n_atoms` - Number of atoms N
pub fn debye_heat_capacity(temperature: f64, debye_temperature: f64, n_atoms: f64) -> f64 {
    if temperature <= 0.0 || debye_temperature <= 0.0 {
        return 0.0;
    }
    
    let x_d = debye_temperature / temperature;
    
    // High temperature limit: C_V → 3Nk_B (Dulong-Petit)
    if x_d < 0.1 {
        return 3.0 * n_atoms * K_B;
    }
    
    // Low temperature limit: C_V ∝ T³
    if x_d > 20.0 {
        let coeff = 12.0 * std::f64::consts::PI.powi(4) / 5.0;
        return n_atoms * K_B * coeff / x_d.powi(3);
    }
    
    // Numerical integration for intermediate temperatures
    let integral = debye_integral(x_d);
    9.0 * n_atoms * K_B * (temperature / debye_temperature).powi(3) * integral
}

/// Debye integral: D₃(x) = 3(T/Θ)³ ∫₀^(Θ/T) t⁴eᵗ/(eᵗ-1)² dt
fn debye_integral(x_max: f64) -> f64 {
    let n_steps = 100;
    let dx = x_max / n_steps as f64;
    let mut sum = 0.0;
    
    for i in 1..=n_steps {
        let x = i as f64 * dx;
        let exp_x = x.exp();
        if exp_x > 1e10 {
            break;
        }
        let integrand = x.powi(4) * exp_x / (exp_x - 1.0).powi(2);
        sum += integrand * dx;
    }
    
    sum
}

/// Fermi energy for free electron gas: E_F = (ℏ²/2m)(3π²n)^(2/3)
pub fn fermi_energy(electron_density: f64, electron_mass: f64) -> f64 {
    let coeff = HBAR.powi(2) / (2.0 * electron_mass);
    let factor = (3.0 * std::f64::consts::PI.powi(2) * electron_density).powf(2.0 / 3.0);
    coeff * factor
}

/// Fermi temperature: T_F = E_F / k_B
pub fn fermi_temperature(fermi_energy: f64) -> f64 {
    fermi_energy / K_B
}

// ============================================================================
// System
// ============================================================================

/// Update quantum statistics for entities with QuantumState
pub fn update_quantum_statistics(
    mut query: Query<(&mut QuantumState, &crate::realism::particles::components::ThermodynamicState)>,
) {
    for (mut quantum, thermo) in query.iter_mut() {
        let temperature = thermo.temperature as f64;
        
        quantum.occupation = occupation_number(
            quantum.particle_type,
            quantum.energy,
            quantum.chemical_potential,
            temperature,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fermi_dirac_limits() {
        // At T=0, FD is step function
        assert!((fermi_dirac_distribution(0.5, 1.0, 0.0) - 1.0).abs() < 1e-10);
        assert!((fermi_dirac_distribution(1.5, 1.0, 0.0) - 0.0).abs() < 1e-10);
        
        // At ε = μ, FD = 0.5
        let fd = fermi_dirac_distribution(1.0, 1.0, 300.0);
        assert!((fd - 0.5).abs() < 1e-10);
    }
    
    #[test]
    fn test_bose_einstein() {
        // BE should be positive for ε > μ
        let be = bose_einstein_distribution(1e-20, 0.0, 300.0);
        assert!(be > 0.0);
        assert!(be.is_finite());
    }
    
    #[test]
    fn test_thermal_wavelength() {
        // Electron at room temperature: λ ≈ 4 nm
        let m_e = 9.109e-31;
        let lambda = thermal_de_broglie_wavelength(m_e, 300.0);
        assert!((lambda - 4e-9).abs() < 1e-9);
    }
    
    #[test]
    fn test_debye_limits() {
        let n = 1e23; // ~1 mol
        
        // High T limit: C_V → 3Nk_B
        let c_high = debye_heat_capacity(1000.0, 100.0, n);
        let expected = 3.0 * n * K_B;
        assert!((c_high - expected).abs() / expected < 0.1);
    }
}
