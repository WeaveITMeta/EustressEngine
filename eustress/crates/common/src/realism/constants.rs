//! # Physical Constants
//!
//! Fundamental physical constants used throughout the realism system.
//! All values are in SI units unless otherwise specified.
//!
//! ## Table of Contents
//!
//! 1. **Universal Constants** - G, c, h, etc.
//! 2. **Thermodynamic Constants** - R, k_B, N_A
//! 3. **Electromagnetic Constants** - ε₀, μ₀, e
//! 4. **Material Constants** - Common material properties
//! 5. **Atmospheric Constants** - Standard atmosphere values

// ============================================================================
// Universal Constants
// ============================================================================

/// Gravitational constant (m³/(kg·s²))
pub const G: f64 = 6.674_30e-11;
/// Gravitational constant (f32)
pub const G_F32: f32 = 6.674_30e-11;

/// Speed of light in vacuum (m/s)
pub const C: f64 = 299_792_458.0;
/// Speed of light (f32)
pub const C_F32: f32 = 299_792_458.0;

/// Planck constant (J·s)
pub const H: f64 = 6.626_070_15e-34;
/// Reduced Planck constant ℏ = h/(2π) (J·s)
pub const H_BAR: f64 = 1.054_571_817e-34;

/// Stefan-Boltzmann constant (W/(m²·K⁴))
pub const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;
pub const STEFAN_BOLTZMANN_F32: f32 = 5.670_374_419e-8;

// ============================================================================
// Thermodynamic Constants
// ============================================================================

/// Universal gas constant (J/(mol·K))
pub const R: f64 = 8.314_462_618;
/// Universal gas constant (f32)
pub const R_F32: f32 = 8.314_462_618;

/// Boltzmann constant (J/K)
pub const K_B: f64 = 1.380_649e-23;
/// Boltzmann constant (f32)
pub const K_B_F32: f32 = 1.380_649e-23;

/// Avogadro constant (1/mol)
pub const N_A: f64 = 6.022_140_76e23;

/// Standard temperature (K) - 25°C
pub const STANDARD_TEMPERATURE: f32 = 298.15;

/// Standard pressure (Pa) - 1 atm
pub const STANDARD_PRESSURE: f32 = 101_325.0;

/// Absolute zero (K)
pub const ABSOLUTE_ZERO: f32 = 0.0;

/// Triple point of water (K)
pub const WATER_TRIPLE_POINT: f32 = 273.16;

/// Boiling point of water at 1 atm (K)
pub const WATER_BOILING_POINT: f32 = 373.15;

// ============================================================================
// Electromagnetic Constants
// ============================================================================

/// Vacuum permittivity ε₀ (F/m)
pub const EPSILON_0: f64 = 8.854_187_8128e-12;

/// Vacuum permeability μ₀ (H/m)
pub const MU_0: f64 = 1.256_637_062_12e-6;

/// Elementary charge (C)
pub const ELEMENTARY_CHARGE: f64 = 1.602_176_634e-19;

/// Electron mass (kg)
pub const ELECTRON_MASS: f64 = 9.109_383_7015e-31;

/// Proton mass (kg)
pub const PROTON_MASS: f64 = 1.672_621_923_69e-27;

// ============================================================================
// Atmospheric Constants
// ============================================================================

/// Standard air density at sea level (kg/m³)
pub const AIR_DENSITY_SEA_LEVEL: f32 = 1.225;

/// Standard air dynamic viscosity (Pa·s)
pub const AIR_VISCOSITY: f32 = 1.81e-5;

/// Standard air kinematic viscosity (m²/s)
pub const AIR_KINEMATIC_VISCOSITY: f32 = 1.48e-5;

/// Specific gas constant for dry air (J/(kg·K))
pub const AIR_SPECIFIC_GAS_CONSTANT: f32 = 287.05;

/// Ratio of specific heats for air (γ = Cp/Cv)
pub const AIR_GAMMA: f32 = 1.4;

/// Speed of sound in air at 20°C (m/s)
pub const SPEED_OF_SOUND_AIR: f32 = 343.0;

// ============================================================================
// Water Constants
// ============================================================================

/// Water density at 4°C (kg/m³)
pub const WATER_DENSITY: f32 = 1000.0;

/// Water dynamic viscosity at 20°C (Pa·s)
pub const WATER_VISCOSITY: f32 = 1.002e-3;

/// Water surface tension at 20°C (N/m)
pub const WATER_SURFACE_TENSION: f32 = 0.0728;

/// Water specific heat capacity (J/(kg·K))
pub const WATER_SPECIFIC_HEAT: f32 = 4186.0;

/// Water thermal conductivity (W/(m·K))
pub const WATER_THERMAL_CONDUCTIVITY: f32 = 0.606;

/// Latent heat of vaporization for water (J/kg)
pub const WATER_LATENT_HEAT_VAPORIZATION: f32 = 2.26e6;

/// Latent heat of fusion for water (J/kg)
pub const WATER_LATENT_HEAT_FUSION: f32 = 3.34e5;

// ============================================================================
// Common Material Properties
// ============================================================================

/// Material property constants for common materials
pub mod materials {
    /// Steel properties
    pub mod steel {
        /// Young's modulus (Pa)
        pub const YOUNG_MODULUS: f32 = 200e9;
        /// Poisson's ratio
        pub const POISSON_RATIO: f32 = 0.30;
        /// Yield strength (Pa)
        pub const YIELD_STRENGTH: f32 = 250e6;
        /// Ultimate tensile strength (Pa)
        pub const ULTIMATE_STRENGTH: f32 = 400e6;
        /// Density (kg/m³)
        pub const DENSITY: f32 = 7850.0;
        /// Thermal conductivity (W/(m·K))
        pub const THERMAL_CONDUCTIVITY: f32 = 50.0;
        /// Specific heat (J/(kg·K))
        pub const SPECIFIC_HEAT: f32 = 500.0;
    }
    
    /// Aluminum properties
    pub mod aluminum {
        pub const YOUNG_MODULUS: f32 = 70e9;
        pub const POISSON_RATIO: f32 = 0.33;
        pub const YIELD_STRENGTH: f32 = 270e6;
        pub const ULTIMATE_STRENGTH: f32 = 310e6;
        pub const DENSITY: f32 = 2700.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 237.0;
        pub const SPECIFIC_HEAT: f32 = 900.0;
    }
    
    /// Concrete properties
    pub mod concrete {
        pub const YOUNG_MODULUS: f32 = 30e9;
        pub const POISSON_RATIO: f32 = 0.20;
        pub const COMPRESSIVE_STRENGTH: f32 = 30e6;
        pub const TENSILE_STRENGTH: f32 = 3e6;
        pub const DENSITY: f32 = 2400.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 1.7;
        pub const SPECIFIC_HEAT: f32 = 880.0;
    }
    
    /// Glass properties
    pub mod glass {
        pub const YOUNG_MODULUS: f32 = 70e9;
        pub const POISSON_RATIO: f32 = 0.22;
        pub const TENSILE_STRENGTH: f32 = 45e6;
        pub const DENSITY: f32 = 2500.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 1.0;
        pub const SPECIFIC_HEAT: f32 = 840.0;
    }
    
    /// Rubber properties
    pub mod rubber {
        pub const YOUNG_MODULUS: f32 = 0.01e9;
        pub const POISSON_RATIO: f32 = 0.49;
        pub const TENSILE_STRENGTH: f32 = 15e6;
        pub const DENSITY: f32 = 1100.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 0.16;
        pub const SPECIFIC_HEAT: f32 = 2000.0;
    }
    
    /// Wood (oak) properties
    pub mod wood {
        pub const YOUNG_MODULUS: f32 = 12e9;
        pub const POISSON_RATIO: f32 = 0.35;
        pub const TENSILE_STRENGTH: f32 = 100e6;
        pub const DENSITY: f32 = 700.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 0.17;
        pub const SPECIFIC_HEAT: f32 = 2400.0;
    }
}

// ============================================================================
// Electrochemistry Constants
// ============================================================================

/// Faraday constant (C/mol) — charge carried by one mole of electrons
pub const FARADAY: f64 = 96_485.332_12;
/// Faraday constant (f32)
pub const FARADAY_F32: f32 = 96_485.332_12;

/// Standard hydrogen electrode potential (V) — reference for all E° values
pub const SHE_REFERENCE: f32 = 0.0;

/// Electron charge (C) — same as ELEMENTARY_CHARGE, alias for electrochemistry context
pub const ELECTRON_CHARGE: f64 = 1.602_176_634e-19;

/// Thermal voltage at 298.15 K: V_T = k_B × T / e = RT/F ≈ 25.7 mV
pub const THERMAL_VOLTAGE_25C: f32 = 0.025_693;

/// Universal gas constant for electrochemistry (same as R, alias)
pub const R_ELECTRO: f32 = 8.314_462_618;

/// Standard Na-S electrochemistry
pub mod na_s {
    /// Standard cell potential for 2Na + S → Na₂S (V vs SHE)
    pub const STANDARD_POTENTIAL: f32 = 2.23;

    /// Standard anode potential Na/Na⁺ (V vs SHE)
    pub const ANODE_POTENTIAL: f32 = -2.714;

    /// Standard cathode potential S/S²⁻ (V vs SHE)
    pub const CATHODE_POTENTIAL: f32 = -0.480;

    /// Electrons transferred per formula unit (Na₂S)
    pub const ELECTRONS: f32 = 2.0;

    /// Theoretical specific capacity of sulfur (mAh/g)
    pub const SULFUR_CAPACITY_MAH_G: f32 = 1_672.0;

    /// Theoretical specific capacity of sodium (mAh/g)
    pub const SODIUM_CAPACITY_MAH_G: f32 = 1_166.0;

    /// Theoretical gravimetric energy density (Wh/kg) — pure reactants
    pub const THEORETICAL_ENERGY_DENSITY: f32 = 5_517.0;

    /// Sulfur volume expansion on full discharge S → Na₂S (fraction)
    pub const SULFUR_VOLUME_EXPANSION: f32 = 0.80;

    /// Entropic coefficient dE/dT for Na-S (V/K) — used for entropic heat
    pub const ENTROPY_COEFFICIENT: f32 = -0.000_15;

    /// Sulfur atomic mass (g/mol)
    pub const SULFUR_MOLAR_MASS: f32 = 32.06;

    /// Sodium atomic mass (g/mol)
    pub const SODIUM_MOLAR_MASS: f32 = 22.990;

    /// Na₂S molar mass (g/mol)
    pub const NA2S_MOLAR_MASS: f32 = 78.04;

    /// S₈ molar mass (g/mol) — elemental sulfur ring
    pub const S8_MOLAR_MASS: f32 = 256.48;
}

/// Sc-doped NASICON solid electrolyte (Na₂.₈Sc₀.₂Zr₁.₈Si₂PO₁₂)
pub mod sc_nasicon {
    /// Ionic conductivity at 25°C target — breakthrough value (S/cm)
    pub const IONIC_CONDUCTIVITY_TARGET: f32 = 1.0e-2;

    /// Ionic conductivity at 25°C demonstrated for doped NASICON (S/cm)
    pub const IONIC_CONDUCTIVITY_DEMONSTRATED: f32 = 1.0e-3;

    /// Activation energy for Na⁺ migration (eV)
    pub const ACTIVATION_ENERGY_EV: f32 = 0.22;

    /// Activation energy in Joules (eV × 1.602e-19 × N_A)
    pub const ACTIVATION_ENERGY_J_MOL: f32 = 21_224.0;

    /// Pre-exponential factor for Arrhenius conductivity (S/cm)
    pub const ARRHENIUS_PREFACTOR: f32 = 1_500.0;

    /// Electronic conductivity — must be negligible (S/cm)
    pub const ELECTRONIC_CONDUCTIVITY: f32 = 1.0e-10;

    /// Na⁺ vacancy fraction from Sc substitution at x=0.2
    pub const VACANCY_FRACTION: f32 = 0.067;

    /// Electrochemical window lower limit (V vs Na/Na⁺)
    pub const WINDOW_MIN: f32 = 0.0;

    /// Electrochemical window upper limit (V vs Na/Na⁺)
    pub const WINDOW_MAX: f32 = 5.0;

    /// Density (kg/m³) — sintered >98% theoretical
    pub const DENSITY: f32 = 3_200.0;
}

/// V-Cell specific battery material constants
pub mod vcell_materials {
    /// Sodium metal (99.9%)
    pub mod sodium {
        pub const YOUNG_MODULUS: f32 = 10.0e9;
        pub const POISSON_RATIO: f32 = 0.29;
        pub const YIELD_STRENGTH: f32 = 0.3e6;
        pub const ULTIMATE_STRENGTH: f32 = 2.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 10.0e6;
        pub const HARDNESS: f32 = 0.5;
        pub const THERMAL_CONDUCTIVITY: f32 = 142.0;
        pub const SPECIFIC_HEAT: f32 = 1_228.0;
        pub const THERMAL_EXPANSION: f32 = 71.0e-6;
        pub const MELTING_POINT: f32 = 370.95;
        pub const DENSITY: f32 = 971.0;
        pub const FRICTION_STATIC: f32 = 0.8;
        pub const FRICTION_KINETIC: f32 = 0.6;
        pub const RESTITUTION: f32 = 0.1;
    }

    /// Sc-doped NASICON ceramic membrane
    pub mod sc_nasicon {
        pub const YOUNG_MODULUS: f32 = 80.0e9;
        pub const POISSON_RATIO: f32 = 0.25;
        pub const YIELD_STRENGTH: f32 = 120.0e6;
        pub const ULTIMATE_STRENGTH: f32 = 150.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 1.5e6;
        pub const HARDNESS: f32 = 600.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 1.5;
        pub const SPECIFIC_HEAT: f32 = 700.0;
        pub const THERMAL_EXPANSION: f32 = 8.5e-6;
        pub const MELTING_POINT: f32 = 1_553.0;
        pub const DENSITY: f32 = 3_200.0;
        pub const FRICTION_STATIC: f32 = 0.5;
        pub const FRICTION_KINETIC: f32 = 0.4;
        pub const RESTITUTION: f32 = 0.3;
    }

    /// Sulfur@VACNT composite cathode
    pub mod sulfur_vacnt {
        pub const YOUNG_MODULUS: f32 = 50.0e9;
        pub const POISSON_RATIO: f32 = 0.24;
        pub const YIELD_STRENGTH: f32 = 25.0e6;
        pub const ULTIMATE_STRENGTH: f32 = 35.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 2.0e6;
        pub const HARDNESS: f32 = 40.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 15.0;
        pub const SPECIFIC_HEAT: f32 = 705.0;
        pub const THERMAL_EXPANSION: f32 = 10.0e-6;
        pub const MELTING_POINT: f32 = 388.36;
        pub const DENSITY: f32 = 1_075.0;
        pub const FRICTION_STATIC: f32 = 0.3;
        pub const FRICTION_KINETIC: f32 = 0.2;
        pub const RESTITUTION: f32 = 0.3;
    }

    /// Aluminum hexagonal lattice current collector (92% porosity)
    pub mod al_hex_lattice {
        pub const YOUNG_MODULUS: f32 = 4.2e9;
        pub const POISSON_RATIO: f32 = 0.33;
        pub const YIELD_STRENGTH: f32 = 16.2e6;
        pub const ULTIMATE_STRENGTH: f32 = 22.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 2.3e6;
        pub const HARDNESS: f32 = 7.6;
        pub const THERMAL_CONDUCTIVITY: f32 = 19.0;
        pub const SPECIFIC_HEAT: f32 = 897.0;
        pub const THERMAL_EXPANSION: f32 = 23.1e-6;
        pub const MELTING_POINT: f32 = 933.47;
        pub const DENSITY: f32 = 216.0;
        pub const FRICTION_STATIC: f32 = 0.5;
        pub const FRICTION_KINETIC: f32 = 0.35;
        pub const RESTITUTION: f32 = 0.5;
        /// Hex cell edge length (m)
        pub const HEX_EDGE_LENGTH: f32 = 50.0e-6;
        /// Wall thickness (m)
        pub const WALL_THICKNESS: f32 = 5.0e-6;
        /// Porosity fraction
        pub const POROSITY: f32 = 0.92;
    }

    /// Aluminum Nitride thermal pad
    pub mod aluminum_nitride {
        pub const YOUNG_MODULUS: f32 = 310.0e9;
        pub const POISSON_RATIO: f32 = 0.24;
        pub const YIELD_STRENGTH: f32 = 300.0e6;
        pub const ULTIMATE_STRENGTH: f32 = 350.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 3.0e6;
        pub const HARDNESS: f32 = 1_200.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 170.0;
        pub const SPECIFIC_HEAT: f32 = 740.0;
        pub const THERMAL_EXPANSION: f32 = 4.6e-6;
        pub const MELTING_POINT: f32 = 2_473.0;
        pub const DENSITY: f32 = 3_260.0;
        pub const FRICTION_STATIC: f32 = 0.4;
        pub const FRICTION_KINETIC: f32 = 0.3;
        pub const RESTITUTION: f32 = 0.4;
    }

    /// Aluminum 6061-T6 housing
    pub mod al_6061_t6 {
        pub const YOUNG_MODULUS: f32 = 68.9e9;
        pub const POISSON_RATIO: f32 = 0.33;
        pub const YIELD_STRENGTH: f32 = 276.0e6;
        pub const ULTIMATE_STRENGTH: f32 = 310.0e6;
        pub const FRACTURE_TOUGHNESS: f32 = 29.0e6;
        pub const HARDNESS: f32 = 95.0;
        pub const THERMAL_CONDUCTIVITY: f32 = 167.0;
        pub const SPECIFIC_HEAT: f32 = 896.0;
        pub const THERMAL_EXPANSION: f32 = 23.6e-6;
        pub const MELTING_POINT: f32 = 855.0;
        pub const DENSITY: f32 = 2_700.0;
        pub const FRICTION_STATIC: f32 = 0.61;
        pub const FRICTION_KINETIC: f32 = 0.47;
        pub const RESTITUTION: f32 = 0.7;
    }
}

// ============================================================================
// Conversion Helpers
// ============================================================================

/// Convert Celsius to Kelvin
#[inline]
pub fn celsius_to_kelvin(celsius: f32) -> f32 {
    celsius + 273.15
}

/// Convert Kelvin to Celsius
#[inline]
pub fn kelvin_to_celsius(kelvin: f32) -> f32 {
    kelvin - 273.15
}

/// Convert atmospheres to Pascals
#[inline]
pub fn atm_to_pascals(atm: f32) -> f32 {
    atm * 101_325.0
}

/// Convert Pascals to atmospheres
#[inline]
pub fn pascals_to_atm(pascals: f32) -> f32 {
    pascals / 101_325.0
}

/// Convert bar to Pascals
#[inline]
pub fn bar_to_pascals(bar: f32) -> f32 {
    bar * 100_000.0
}

/// Convert Pascals to bar
#[inline]
pub fn pascals_to_bar(pascals: f32) -> f32 {
    pascals / 100_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_temperature_conversion() {
        assert!((celsius_to_kelvin(0.0) - 273.15).abs() < 0.01);
        assert!((celsius_to_kelvin(100.0) - 373.15).abs() < 0.01);
        assert!((kelvin_to_celsius(273.15) - 0.0).abs() < 0.01);
    }
    
    #[test]
    fn test_pressure_conversion() {
        assert!((atm_to_pascals(1.0) - 101_325.0).abs() < 1.0);
        assert!((pascals_to_atm(101_325.0) - 1.0).abs() < 0.001);
    }
}
