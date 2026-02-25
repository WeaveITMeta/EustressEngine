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
use std::collections::HashMap;
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
    
    // Domain-specific extensions
    /// Custom properties for domain-specific fields (e.g., porosity, electrical_conductivity)
    #[serde(default)]
    #[reflect(ignore)]
    pub custom_properties: HashMap<String, f64>,
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
            custom_properties: HashMap::new(),
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
    // V-Cell Battery Material Presets
    // ========================================================================

    /// Sodium metal anode (99.9% Na)
    pub fn sodium_metal() -> Self {
        use constants::vcell_materials::sodium as m;
        Self {
            name: "Sodium Metal (Na)".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
        }
    }

    /// Sc-doped NASICON solid electrolyte (Na₂.₈Sc₀.₂Zr₁.₈Si₂PO₁₂)
    pub fn sc_nasicon() -> Self {
        use constants::vcell_materials::sc_nasicon as m;
        Self {
            name: "Sc-NASICON Electrolyte".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
        }
    }

    /// Sulfur@VACNT composite cathode (sulfur infiltrated into vertically-aligned CNT forest)
    pub fn sulfur_vacnt() -> Self {
        use constants::vcell_materials::sulfur_vacnt as m;
        Self {
            name: "Sulfur@VACNT Cathode".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
        }
    }

    /// Aluminum hexagonal lattice current collector (50μm cells, 92% porosity)
    pub fn al_hex_lattice() -> Self {
        use constants::vcell_materials::al_hex_lattice as m;
        Self {
            name: "Al Hex Lattice (92% porosity)".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
        }
    }

    /// Aluminum nitride (AlN) thermal management pad
    pub fn aluminum_nitride() -> Self {
        use constants::vcell_materials::aluminum_nitride as m;
        Self {
            name: "Aluminum Nitride (AlN)".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
        }
    }

    /// Aluminum 6061-T6 housing shell
    pub fn al_6061_t6() -> Self {
        use constants::vcell_materials::al_6061_t6 as m;
        Self {
            name: "Aluminum 6061-T6".to_string(),
            young_modulus: m::YOUNG_MODULUS,
            poisson_ratio: m::POISSON_RATIO,
            yield_strength: m::YIELD_STRENGTH,
            ultimate_strength: m::ULTIMATE_STRENGTH,
            fracture_toughness: m::FRACTURE_TOUGHNESS,
            hardness: m::HARDNESS,
            thermal_conductivity: m::THERMAL_CONDUCTIVITY,
            specific_heat: m::SPECIFIC_HEAT,
            thermal_expansion: m::THERMAL_EXPANSION,
            melting_point: m::MELTING_POINT,
            density: m::DENSITY,
            friction_static: m::FRICTION_STATIC,
            friction_kinetic: m::FRICTION_KINETIC,
            restitution: m::RESTITUTION,
            custom_properties: HashMap::new(),
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

    #[test]
    fn test_vcell_sodium_metal() {
        let na = MaterialProperties::sodium_metal();
        assert!((na.density - 971.0).abs() < 1.0);
        assert!(na.is_ductile(), "Sodium must be ductile");
        assert!(na.melting_point > 370.0, "Na melting point ~371 K");
    }

    #[test]
    fn test_vcell_sc_nasicon() {
        let nsc = MaterialProperties::sc_nasicon();
        assert!((nsc.density - 3200.0).abs() < 10.0);
        assert!(nsc.is_brittle(), "NASICON ceramic must be brittle");
        assert!(nsc.thermal_conductivity < 5.0, "NASICON has low k");
    }

    #[test]
    fn test_vcell_al_hex_lattice() {
        let hex = MaterialProperties::al_hex_lattice();
        assert!((hex.density - 216.0).abs() < 5.0, "92% porosity → ~216 kg/m³");
        assert!(hex.young_modulus < 5e9, "Lattice E << bulk Al");
    }

    #[test]
    fn test_vcell_aluminum_nitride() {
        let aln = MaterialProperties::aluminum_nitride();
        assert!(aln.thermal_conductivity > 150.0, "AlN has very high k");
        assert!(aln.is_brittle(), "AlN is a ceramic");
    }

    #[test]
    fn test_vcell_al_6061_t6() {
        let al = MaterialProperties::al_6061_t6();
        assert!((al.density - 2700.0).abs() < 10.0);
        assert!(al.is_ductile(), "6061-T6 must be ductile");
    }
}
