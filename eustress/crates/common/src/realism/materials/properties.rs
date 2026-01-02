//! # Material Properties
//!
//! Physical properties of materials for simulation.
//!
//! ## Table of Contents
//!
//! 1. **MaterialProperties** - Core material component
//! 2. **Presets** - Common material presets (steel, aluminum, etc.)
//! 3. **Thermal Properties** - Heat capacity, conductivity

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use crate::realism::constants;

// ============================================================================
// Material Properties Component
// ============================================================================

/// Physical properties of a material
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct MaterialProperties {
    /// Material name/identifier
    pub name: String,
    
    // Mechanical properties
    /// Young's modulus / Elastic modulus (Pa)
    pub young_modulus: f32,
    /// Poisson's ratio (dimensionless, 0-0.5)
    pub poisson_ratio: f32,
    /// Yield strength (Pa) - onset of plastic deformation
    pub yield_strength: f32,
    /// Ultimate tensile strength (Pa) - maximum stress before failure
    pub ultimate_strength: f32,
    /// Fracture toughness K_IC (Pa·√m)
    pub fracture_toughness: f32,
    /// Hardness (Vickers, HV)
    pub hardness: f32,
    
    // Thermal properties
    /// Thermal conductivity (W/(m·K))
    pub thermal_conductivity: f32,
    /// Specific heat capacity (J/(kg·K))
    pub specific_heat: f32,
    /// Coefficient of thermal expansion (1/K)
    pub thermal_expansion: f32,
    /// Melting point (K)
    pub melting_point: f32,
    
    // Physical properties
    /// Density (kg/m³)
    pub density: f32,
    
    // Surface properties
    /// Friction coefficient (static)
    pub friction_static: f32,
    /// Friction coefficient (kinetic)
    pub friction_kinetic: f32,
    /// Coefficient of restitution (bounciness, 0-1)
    pub restitution: f32,
}

impl Default for MaterialProperties {
    fn default() -> Self {
        Self::steel()
    }
}

impl MaterialProperties {
    /// Create steel material
    pub fn steel() -> Self {
        Self {
            name: "Steel".to_string(),
            young_modulus: constants::materials::steel::YOUNG_MODULUS,
            poisson_ratio: constants::materials::steel::POISSON_RATIO,
            yield_strength: constants::materials::steel::YIELD_STRENGTH,
            ultimate_strength: constants::materials::steel::ULTIMATE_STRENGTH,
            fracture_toughness: 50e6, // ~50 MPa·√m
            hardness: 200.0,
            thermal_conductivity: constants::materials::steel::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::steel::SPECIFIC_HEAT,
            thermal_expansion: 12e-6,
            melting_point: 1800.0,
            density: constants::materials::steel::DENSITY,
            friction_static: 0.74,
            friction_kinetic: 0.57,
            restitution: 0.6,
        }
    }
    
    /// Create aluminum material
    pub fn aluminum() -> Self {
        Self {
            name: "Aluminum".to_string(),
            young_modulus: constants::materials::aluminum::YOUNG_MODULUS,
            poisson_ratio: constants::materials::aluminum::POISSON_RATIO,
            yield_strength: constants::materials::aluminum::YIELD_STRENGTH,
            ultimate_strength: constants::materials::aluminum::ULTIMATE_STRENGTH,
            fracture_toughness: 30e6,
            hardness: 75.0,
            thermal_conductivity: constants::materials::aluminum::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::aluminum::SPECIFIC_HEAT,
            thermal_expansion: 23e-6,
            melting_point: 933.0,
            density: constants::materials::aluminum::DENSITY,
            friction_static: 0.61,
            friction_kinetic: 0.47,
            restitution: 0.7,
        }
    }
    
    /// Create concrete material
    pub fn concrete() -> Self {
        Self {
            name: "Concrete".to_string(),
            young_modulus: constants::materials::concrete::YOUNG_MODULUS,
            poisson_ratio: constants::materials::concrete::POISSON_RATIO,
            yield_strength: constants::materials::concrete::COMPRESSIVE_STRENGTH,
            ultimate_strength: constants::materials::concrete::COMPRESSIVE_STRENGTH,
            fracture_toughness: 1e6,
            hardness: 500.0,
            thermal_conductivity: constants::materials::concrete::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::concrete::SPECIFIC_HEAT,
            thermal_expansion: 10e-6,
            melting_point: 1500.0,
            density: constants::materials::concrete::DENSITY,
            friction_static: 0.6,
            friction_kinetic: 0.5,
            restitution: 0.2,
        }
    }
    
    /// Create glass material
    pub fn glass() -> Self {
        Self {
            name: "Glass".to_string(),
            young_modulus: constants::materials::glass::YOUNG_MODULUS,
            poisson_ratio: constants::materials::glass::POISSON_RATIO,
            yield_strength: constants::materials::glass::TENSILE_STRENGTH,
            ultimate_strength: constants::materials::glass::TENSILE_STRENGTH,
            fracture_toughness: 0.7e6, // Very brittle
            hardness: 500.0,
            thermal_conductivity: constants::materials::glass::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::glass::SPECIFIC_HEAT,
            thermal_expansion: 9e-6,
            melting_point: 1700.0,
            density: constants::materials::glass::DENSITY,
            friction_static: 0.94,
            friction_kinetic: 0.4,
            restitution: 0.5,
        }
    }
    
    /// Create rubber material
    pub fn rubber() -> Self {
        Self {
            name: "Rubber".to_string(),
            young_modulus: constants::materials::rubber::YOUNG_MODULUS,
            poisson_ratio: constants::materials::rubber::POISSON_RATIO,
            yield_strength: constants::materials::rubber::TENSILE_STRENGTH,
            ultimate_strength: constants::materials::rubber::TENSILE_STRENGTH,
            fracture_toughness: 5e6,
            hardness: 40.0,
            thermal_conductivity: constants::materials::rubber::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::rubber::SPECIFIC_HEAT,
            thermal_expansion: 200e-6,
            melting_point: 450.0, // Degrades, doesn't truly melt
            density: constants::materials::rubber::DENSITY,
            friction_static: 1.0,
            friction_kinetic: 0.8,
            restitution: 0.8,
        }
    }
    
    /// Create wood material
    pub fn wood() -> Self {
        Self {
            name: "Wood (Oak)".to_string(),
            young_modulus: constants::materials::wood::YOUNG_MODULUS,
            poisson_ratio: constants::materials::wood::POISSON_RATIO,
            yield_strength: constants::materials::wood::TENSILE_STRENGTH * 0.6,
            ultimate_strength: constants::materials::wood::TENSILE_STRENGTH,
            fracture_toughness: 10e6,
            hardness: 100.0,
            thermal_conductivity: constants::materials::wood::THERMAL_CONDUCTIVITY,
            specific_heat: constants::materials::wood::SPECIFIC_HEAT,
            thermal_expansion: 5e-6,
            melting_point: 573.0, // Ignition point
            density: constants::materials::wood::DENSITY,
            friction_static: 0.5,
            friction_kinetic: 0.4,
            restitution: 0.3,
        }
    }
    
    /// Create ice material
    pub fn ice() -> Self {
        Self {
            name: "Ice".to_string(),
            young_modulus: 9e9,
            poisson_ratio: 0.33,
            yield_strength: 1e6,
            ultimate_strength: 2e6,
            fracture_toughness: 0.1e6,
            hardness: 1.5,
            thermal_conductivity: 2.2,
            specific_heat: 2090.0,
            thermal_expansion: 50e-6,
            melting_point: 273.15,
            density: 917.0,
            friction_static: 0.1,
            friction_kinetic: 0.03,
            restitution: 0.3,
        }
    }
    
    /// Create custom material
    pub fn custom(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Self::steel()
        }
    }
    
    // ========================================================================
    // Derived Properties
    // ========================================================================
    
    /// Shear modulus: G = E / (2(1 + ν))
    pub fn shear_modulus(&self) -> f32 {
        self.young_modulus / (2.0 * (1.0 + self.poisson_ratio))
    }
    
    /// Bulk modulus: K = E / (3(1 - 2ν))
    pub fn bulk_modulus(&self) -> f32 {
        let denom = 3.0 * (1.0 - 2.0 * self.poisson_ratio);
        if denom.abs() < 1e-6 {
            return f32::INFINITY; // Incompressible
        }
        self.young_modulus / denom
    }
    
    /// Lamé's first parameter: λ = Eν / ((1+ν)(1-2ν))
    pub fn lame_lambda(&self) -> f32 {
        let denom = (1.0 + self.poisson_ratio) * (1.0 - 2.0 * self.poisson_ratio);
        if denom.abs() < 1e-6 {
            return f32::INFINITY;
        }
        (self.young_modulus * self.poisson_ratio) / denom
    }
    
    /// Lamé's second parameter (same as shear modulus): μ = G
    pub fn lame_mu(&self) -> f32 {
        self.shear_modulus()
    }
    
    /// Thermal diffusivity: α = k / (ρ * c_p)
    pub fn thermal_diffusivity(&self) -> f32 {
        self.thermal_conductivity / (self.density * self.specific_heat)
    }
    
    /// Speed of sound in material: c = √(E/ρ)
    pub fn speed_of_sound(&self) -> f32 {
        (self.young_modulus / self.density).sqrt()
    }
    
    /// Check if material is ductile (yield before fracture)
    pub fn is_ductile(&self) -> bool {
        self.yield_strength < self.ultimate_strength * 0.9
    }
    
    /// Check if material is brittle
    pub fn is_brittle(&self) -> bool {
        !self.is_ductile()
    }
}

// ============================================================================
// Material Bundle
// ============================================================================

/// Bundle for structural elements with material properties
#[derive(Bundle, Clone)]
pub struct StructuralBundle {
    pub material: MaterialProperties,
    pub stress: super::stress_strain::StressTensor,
    pub strain: super::stress_strain::StrainTensor,
    pub fracture: super::fracture::FractureState,
    pub deformation: super::deformation::DeformationState,
}

impl Default for StructuralBundle {
    fn default() -> Self {
        Self {
            material: MaterialProperties::steel(),
            stress: super::stress_strain::StressTensor::default(),
            strain: super::stress_strain::StrainTensor::default(),
            fracture: super::fracture::FractureState::default(),
            deformation: super::deformation::DeformationState::default(),
        }
    }
}

impl StructuralBundle {
    pub fn with_material(material: MaterialProperties) -> Self {
        Self {
            material,
            ..default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_derived_properties() {
        let steel = MaterialProperties::steel();
        
        // Shear modulus should be ~77 GPa for steel
        let g = steel.shear_modulus();
        assert!((g - 77e9).abs() < 5e9);
        
        // Speed of sound in steel ~5000 m/s
        let c = steel.speed_of_sound();
        assert!((c - 5000.0).abs() < 500.0);
    }
    
    #[test]
    fn test_material_classification() {
        let steel = MaterialProperties::steel();
        assert!(steel.is_ductile());
        
        let glass = MaterialProperties::glass();
        assert!(glass.is_brittle());
    }
}
