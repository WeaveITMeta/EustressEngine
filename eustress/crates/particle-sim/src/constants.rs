//! The physical constants the solver uses (SI). They are the values of
//! `eustress_common::realism::constants`, and a test there holds them equal;
//! this crate sits below `eustress-common`, so it carries its own copy.

/// Boltzmann constant (J/K)
pub const K_B: f64 = 1.380_649e-23;

/// Vacuum permittivity ε₀ (F/m)
pub const EPSILON_0: f64 = 8.854_187_8128e-12;

/// Elementary charge (C)
pub const ELEMENTARY_CHARGE: f64 = 1.602_176_634e-19;

/// Electron mass (kg)
pub const ELECTRON_MASS: f64 = 9.109_383_7015e-31;

/// Proton mass (kg)
pub const PROTON_MASS: f64 = 1.672_621_923_69e-27;

/// Atomic mass unit (Dalton) [kg]
pub const ATOMIC_MASS_UNIT: f64 = 1.660_539_066_6e-27;

/// Coulomb constant k = 1/(4πε₀) [N·m²/C²]
pub const COULOMB_K: f32 = 8.987_551_8e9;

/// Water density at 4°C (kg/m³)
pub const WATER_DENSITY: f32 = 1000.0;

/// Water dynamic viscosity at 20°C (Pa·s)
pub const WATER_VISCOSITY: f32 = 1.002e-3;
