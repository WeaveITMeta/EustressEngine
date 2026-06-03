//! ECS components for plasma simulation.
//!
//! Attach `PlasmaState` to any entity to give it ionized-gas properties, and
//! `FusionPlasma` to track fusion-reactor confinement and gain. The component
//! methods delegate to the pure-physics helpers in [`super::debye`] and
//! [`super::mhd`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// State of an ionized gas (plasma) attached to an entity.
///
/// Densities are in particles per cubic metre, temperatures in kelvin,
/// ionization degree is a fraction in [0, 1], and the magnetic field is in
/// tesla.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PlasmaState {
    /// Electron number density n_e, in particles per cubic metre.
    pub electron_density: f32,
    /// Ion number density n_i, in particles per cubic metre.
    pub ion_density: f32,
    /// Electron temperature T_e, in kelvin.
    pub electron_temperature_k: f32,
    /// Ion temperature T_i, in kelvin.
    pub ion_temperature_k: f32,
    /// Ionization degree (fraction of atoms ionized), in [0, 1].
    pub ionization_degree: f32,
    /// Ambient magnetic field magnitude B, in tesla.
    pub magnetic_field: f32,
}

impl Default for PlasmaState {
    fn default() -> Self {
        Self {
            electron_density: 1e19,
            ion_density: 1e19,
            electron_temperature_k: 1e7,
            ion_temperature_k: 1e7,
            ionization_degree: 1.0,
            magnetic_field: 1.0,
        }
    }
}

impl PlasmaState {
    /// Debye screening length for this plasma's electrons, in metres.
    pub fn debye_length(&self) -> f32 {
        super::debye::debye_length(self.electron_density, self.electron_temperature_k)
    }

    /// Electron plasma (angular) frequency, in radians per second.
    pub fn plasma_frequency(&self) -> f32 {
        super::debye::plasma_frequency(self.electron_density)
    }

    /// Plasma beta given an externally supplied thermal pressure (in pascals).
    pub fn plasma_beta(&self, thermal_pressure: f32) -> f32 {
        super::mhd::plasma_beta(thermal_pressure, self.magnetic_field)
    }
}

/// Fusion-reactor confinement and performance state for an entity.
///
/// Confinement time is in seconds, the triple product is in keV·s/m^3 (usually
/// recomputed from a companion [`PlasmaState`]), and the gain factor Q is
/// dimensionless.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct FusionPlasma {
    /// Energy confinement time tau_E, in seconds.
    pub confinement_time: f32,
    /// Lawson triple product n · T · tau, in keV·s/m^3 (computed).
    pub triple_product: f32,
    /// Fusion gain factor Q = P_fusion / P_heating (dimensionless).
    pub gain_q: f32,
    /// Whether the plasma has reached ignition.
    pub is_ignited: bool,
}

impl Default for FusionPlasma {
    fn default() -> Self {
        // Tokamak-ish starting values: sub-second confinement, an order of
        // magnitude below the Lawson threshold, sub-breakeven gain.
        Self {
            confinement_time: 1.0,
            triple_product: 1e20,
            gain_q: 0.1,
            is_ignited: false,
        }
    }
}
