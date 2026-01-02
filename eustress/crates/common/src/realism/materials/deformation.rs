//! # Deformation
//!
//! Elastic and plastic deformation tracking.
//!
//! ## Table of Contents
//!
//! 1. **DeformationState** - Tracks elastic/plastic strain
//! 2. **Plasticity Models** - Von Mises, isotropic hardening
//! 3. **Thermal Deformation** - Thermal expansion

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::properties::MaterialProperties;
use super::stress_strain::{StressTensor, StrainTensor};

// ============================================================================
// Deformation State
// ============================================================================

/// Tracks deformation state of a material
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct DeformationState {
    /// Total strain
    pub total_strain: StrainTensor,
    /// Elastic strain (recoverable)
    pub elastic_strain: StrainTensor,
    /// Plastic strain (permanent)
    pub plastic_strain: StrainTensor,
    /// Thermal strain
    pub thermal_strain: StrainTensor,
    /// Equivalent plastic strain (accumulated)
    pub equivalent_plastic_strain: f32,
    /// Has material yielded
    pub has_yielded: bool,
    /// Current yield stress (may increase with hardening)
    pub current_yield_stress: f32,
    /// Reference temperature for thermal strain (K)
    pub reference_temperature: f32,
    /// Current temperature (K)
    pub current_temperature: f32,
}

impl DeformationState {
    /// Create new deformation state
    pub fn new(initial_yield_stress: f32, reference_temp: f32) -> Self {
        Self {
            current_yield_stress: initial_yield_stress,
            reference_temperature: reference_temp,
            current_temperature: reference_temp,
            ..default()
        }
    }
    
    /// Create from material properties
    pub fn from_material(material: &MaterialProperties) -> Self {
        Self::new(material.yield_strength, 293.15)
    }
    
    /// Get total strain magnitude
    pub fn total_strain_magnitude(&self) -> f32 {
        self.total_strain.equivalent
    }
    
    /// Get plastic strain magnitude
    pub fn plastic_strain_magnitude(&self) -> f32 {
        self.plastic_strain.equivalent
    }
    
    /// Check if deformation is purely elastic
    pub fn is_elastic(&self) -> bool {
        !self.has_yielded && self.equivalent_plastic_strain < 1e-10
    }
    
    /// Reset to undeformed state
    pub fn reset(&mut self) {
        self.total_strain = StrainTensor::default();
        self.elastic_strain = StrainTensor::default();
        self.plastic_strain = StrainTensor::default();
        self.thermal_strain = StrainTensor::default();
        self.equivalent_plastic_strain = 0.0;
        self.has_yielded = false;
    }
}

// ============================================================================
// Plasticity
// ============================================================================

/// Apply plastic deformation using von Mises yield criterion with isotropic hardening
pub fn apply_plasticity(
    stress: &StressTensor,
    deformation: &mut DeformationState,
    material: &MaterialProperties,
    dt: f32,
) {
    // Check yield condition
    if stress.von_mises < deformation.current_yield_stress {
        deformation.has_yielded = false;
        return;
    }
    
    deformation.has_yielded = true;
    
    // Calculate plastic strain increment (radial return algorithm simplified)
    let overstress = stress.von_mises - deformation.current_yield_stress;
    let plastic_multiplier = overstress / (3.0 * material.shear_modulus());
    
    // Deviatoric stress direction
    let dev = stress.deviatoric();
    let dev_magnitude = {
        let mut sum = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                sum += dev[i][j] * dev[i][j];
            }
        }
        (sum / 2.0).sqrt()
    };
    
    if dev_magnitude > 1e-10 {
        // Plastic strain increment in direction of deviatoric stress
        let scale = plastic_multiplier / dev_magnitude * dt;
        
        for i in 0..3 {
            for j in 0..3 {
                deformation.plastic_strain.components[i][j] += dev[i][j] * scale;
            }
        }
        
        // Update equivalent plastic strain
        let delta_eps_p = plastic_multiplier * dt;
        deformation.equivalent_plastic_strain += delta_eps_p;
        
        // Isotropic hardening (linear)
        let hardening_modulus = material.young_modulus * 0.01; // 1% of E
        deformation.current_yield_stress += hardening_modulus * delta_eps_p;
        
        // Cap at ultimate strength
        deformation.current_yield_stress = deformation.current_yield_stress.min(material.ultimate_strength);
        
        deformation.plastic_strain.update_invariants();
    }
}

/// Calculate elastic strain from total and plastic strain
pub fn calculate_elastic_strain(deformation: &mut DeformationState) {
    for i in 0..3 {
        for j in 0..3 {
            deformation.elastic_strain.components[i][j] = 
                deformation.total_strain.components[i][j] 
                - deformation.plastic_strain.components[i][j]
                - deformation.thermal_strain.components[i][j];
        }
    }
    deformation.elastic_strain.update_invariants();
}

// ============================================================================
// Thermal Deformation
// ============================================================================

/// Calculate thermal strain from temperature change
/// ε_thermal = α * ΔT * I (isotropic)
pub fn calculate_thermal_strain(
    deformation: &mut DeformationState,
    material: &MaterialProperties,
) {
    let delta_t = deformation.current_temperature - deformation.reference_temperature;
    let thermal_strain = material.thermal_expansion * delta_t;
    
    // Isotropic thermal strain (diagonal only)
    deformation.thermal_strain = StrainTensor::from_normal(
        thermal_strain,
        thermal_strain,
        thermal_strain,
    );
}

/// Update temperature and recalculate thermal strain
pub fn update_temperature(
    deformation: &mut DeformationState,
    new_temperature: f32,
    material: &MaterialProperties,
) {
    deformation.current_temperature = new_temperature;
    calculate_thermal_strain(deformation, material);
}

/// Thermal stress from constrained thermal expansion
/// σ = -E * α * ΔT / (1 - 2ν) for fully constrained
pub fn thermal_stress_constrained(
    delta_temperature: f32,
    material: &MaterialProperties,
) -> f32 {
    let denom = 1.0 - 2.0 * material.poisson_ratio;
    if denom.abs() < 1e-6 {
        return 0.0;
    }
    -material.young_modulus * material.thermal_expansion * delta_temperature / denom
}

// ============================================================================
// Creep (Time-dependent deformation)
// ============================================================================

/// Norton creep law: ε̇_creep = A * σ^n * exp(-Q/RT)
/// 
/// # Arguments
/// * `stress` - Applied stress (Pa)
/// * `temperature` - Temperature (K)
/// * `a` - Creep coefficient
/// * `n` - Stress exponent
/// * `q` - Activation energy (J/mol)
pub fn norton_creep_rate(stress: f32, temperature: f32, a: f32, n: f32, q: f32) -> f32 {
    let r = 8.314; // Gas constant
    a * stress.powf(n) * (-q / (r * temperature)).exp()
}

/// Typical creep parameters for metals at high temperature
pub mod creep_parameters {
    /// Steel at 500°C (A, n, Q in J/mol)
    pub const STEEL_500C: (f32, f32, f32) = (1e-20, 5.0, 250000.0);
    /// Aluminum at 200°C
    pub const ALUMINUM_200C: (f32, f32, f32) = (1e-15, 4.5, 150000.0);
}

// ============================================================================
// Strain Rate Effects
// ============================================================================

/// Johnson-Cook strain rate sensitivity: σ = σ_0 * (1 + C * ln(ε̇/ε̇_0))
pub fn johnson_cook_rate_factor(strain_rate: f32, reference_rate: f32, c: f32) -> f32 {
    if strain_rate <= 0.0 || reference_rate <= 0.0 {
        return 1.0;
    }
    1.0 + c * (strain_rate / reference_rate).ln()
}

/// Cowper-Symonds strain rate sensitivity: σ = σ_0 * (1 + (ε̇/D)^(1/p))
pub fn cowper_symonds_rate_factor(strain_rate: f32, d: f32, p: f32) -> f32 {
    if d <= 0.0 {
        return 1.0;
    }
    1.0 + (strain_rate / d).powf(1.0 / p)
}

/// Typical Cowper-Symonds parameters
pub mod rate_parameters {
    /// Mild steel (D, p)
    pub const MILD_STEEL: (f32, f32) = (40.4, 5.0);
    /// Aluminum
    pub const ALUMINUM: (f32, f32) = (6500.0, 4.0);
    /// Stainless steel
    pub const STAINLESS_STEEL: (f32, f32) = (100.0, 10.0);
}

// ============================================================================
// System
// ============================================================================

/// Apply deformation updates
pub fn apply_deformation(
    mut query: Query<(&StressTensor, &MaterialProperties, &mut DeformationState)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    
    for (stress, material, mut deformation) in query.iter_mut() {
        // Apply plasticity if yielded
        apply_plasticity(stress, &mut deformation, material, dt);
        
        // Calculate elastic strain
        calculate_elastic_strain(&mut deformation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_thermal_strain() {
        let material = MaterialProperties::steel();
        let mut deformation = DeformationState::from_material(&material);
        
        // Heat by 100K
        update_temperature(&mut deformation, 393.15, &material);
        
        // Thermal strain should be α * ΔT = 12e-6 * 100 = 1.2e-3
        assert!((deformation.thermal_strain.components[0][0] - 1.2e-3).abs() < 1e-5);
    }
    
    #[test]
    fn test_thermal_stress() {
        let material = MaterialProperties::steel();
        let sigma = thermal_stress_constrained(100.0, &material);
        
        // Should be compressive (negative) for heating
        assert!(sigma < 0.0);
        // Magnitude should be significant
        assert!(sigma.abs() > 100e6);
    }
    
    #[test]
    fn test_rate_sensitivity() {
        let (d, p) = rate_parameters::MILD_STEEL;
        
        // At reference rate, factor should be ~1
        let factor_low = cowper_symonds_rate_factor(1.0, d, p);
        assert!((factor_low - 1.0).abs() < 0.1);
        
        // At high rate, factor should be > 1
        let factor_high = cowper_symonds_rate_factor(1000.0, d, p);
        assert!(factor_high > 1.5);
    }
}
