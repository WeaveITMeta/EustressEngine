//! Named particles and conductor materials, with their literature values.
//!
//! Conduction-electron densities and Drude relaxation times are from
//! Ashcroft and Mermin, *Solid State Physics* (1976), Tables 1.1 and 1.3
//! (tau at 273 K). With them the Drude model gives sigma = n e^2 tau / m,
//! which lands within a few percent of the measured 273 K conductivities
//! listed alongside for comparison.

use crate::constants;
pub use crate::constants::ATOMIC_MASS_UNIT;

/// A named particle type: what one real particle weighs and carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticlePreset {
    Electron,
    Positron,
    Proton,
    /// Charge and mass from the species' `charge_number` / `mass_amu`.
    Ion,
    /// Neutral monatomic gas (argon).
    Argon,
    Water,
    Oil,
    Mercury,
    /// Fluid with the species' own `rest_density` / `viscosity`.
    CustomFluid,
    /// Point particle with the species' own charge and mass.
    Custom,
}

impl ParticlePreset {
    pub const ALL: [ParticlePreset; 10] = [
        Self::Electron,
        Self::Positron,
        Self::Proton,
        Self::Ion,
        Self::Argon,
        Self::Water,
        Self::Oil,
        Self::Mercury,
        Self::CustomFluid,
        Self::Custom,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Electron => "Electron",
            Self::Positron => "Positron",
            Self::Proton => "Proton",
            Self::Ion => "Ion",
            Self::Argon => "Argon",
            Self::Water => "Water",
            Self::Oil => "Oil",
            Self::Mercury => "Mercury",
            Self::CustomFluid => "CustomFluid",
            Self::Custom => "Custom",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.as_str().eq_ignore_ascii_case(s))
    }

    pub fn is_fluid(self) -> bool {
        matches!(self, Self::Water | Self::Oil | Self::Mercury | Self::CustomFluid)
    }

    /// (charge C, mass kg) of one real particle, or `None` when the species'
    /// own fields decide (Ion, Custom, fluids).
    pub fn charge_mass(self) -> Option<(f64, f64)> {
        let e = constants::ELEMENTARY_CHARGE;
        match self {
            Self::Electron => Some((-e, constants::ELECTRON_MASS)),
            Self::Positron => Some((e, constants::ELECTRON_MASS)),
            Self::Proton => Some((e, constants::PROTON_MASS)),
            Self::Argon => Some((0.0, 39.948 * ATOMIC_MASS_UNIT)),
            _ => None,
        }
    }

    /// (rest density kg/m^3, dynamic viscosity Pa s) for the fluid presets.
    pub fn fluid_properties(self) -> Option<(f32, f32)> {
        match self {
            Self::Water => Some((998.2, 1.002e-3)),
            Self::Oil => Some((870.0, 0.08)),
            Self::Mercury => Some((13_534.0, 1.526e-3)),
            _ => None,
        }
    }

    /// Default display colour, sRGB 0-255 (Eustress's colour convention).
    pub fn color_srgb8(self) -> [u8; 3] {
        match self {
            Self::Electron => [26, 115, 255],
            Self::Positron => [255, 89, 153],
            Self::Proton => [255, 64, 38],
            Self::Ion => [255, 153, 26],
            Self::Argon => [191, 140, 255],
            Self::Water => [31, 107, 242],
            Self::Oil => [204, 140, 13],
            Self::Mercury => [191, 199, 209],
            Self::CustomFluid => [51, 204, 204],
            Self::Custom => [230, 230, 230],
        }
    }

    /// Default display colour, sRGB 0-1.
    pub fn color(self) -> [f32; 3] {
        self.color_srgb8().map(|c| c as f32 / 255.0)
    }
}

/// A metal for the free-electron (Drude) conductor model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConductorMaterial {
    pub name: &'static str,
    /// Conduction electrons per m^3.
    pub electron_density: f64,
    /// Drude relaxation time at 273 K (s).
    pub relaxation_time: f64,
    /// Measured conductivity at 273 K (S/m), for comparison only.
    pub measured_conductivity: f64,
    /// Volumetric heat capacity near room temperature (J/(m^3 K)).
    pub heat_capacity: f64,
}

pub const CONDUCTORS: [ConductorMaterial; 6] = [
    ConductorMaterial {
        name: "Copper",
        electron_density: 8.47e28,
        relaxation_time: 2.7e-14,
        measured_conductivity: 6.41e7,
        heat_capacity: 3.45e6,
    },
    ConductorMaterial {
        name: "Silver",
        electron_density: 5.86e28,
        relaxation_time: 4.0e-14,
        measured_conductivity: 6.62e7,
        heat_capacity: 2.47e6,
    },
    ConductorMaterial {
        name: "Gold",
        electron_density: 5.90e28,
        relaxation_time: 3.0e-14,
        measured_conductivity: 4.88e7,
        heat_capacity: 2.49e6,
    },
    ConductorMaterial {
        name: "Aluminum",
        electron_density: 18.1e28,
        relaxation_time: 0.80e-14,
        measured_conductivity: 4.08e7,
        heat_capacity: 2.42e6,
    },
    ConductorMaterial {
        name: "Iron",
        electron_density: 17.0e28,
        relaxation_time: 0.24e-14,
        measured_conductivity: 1.12e7,
        heat_capacity: 3.54e6,
    },
    ConductorMaterial {
        name: "Sodium",
        electron_density: 2.65e28,
        relaxation_time: 3.2e-14,
        measured_conductivity: 2.38e7,
        heat_capacity: 1.19e6,
    },
];

pub fn conductor(name: &str) -> Option<ConductorMaterial> {
    CONDUCTORS.iter().copied().find(|c| c.name.eq_ignore_ascii_case(name))
}

/// Drude conductivity sigma = n q^2 tau / m (S/m).
pub fn drude_conductivity(number_density: f64, charge: f64, relaxation_time: f64, mass: f64) -> f64 {
    if mass <= 0.0 {
        return 0.0;
    }
    number_density * charge * charge * relaxation_time / mass
}

/// Plasma angular frequency sqrt(n q^2 / (eps0 m)) (rad/s).
pub fn plasma_frequency(number_density: f64, charge: f64, mass: f64) -> f64 {
    if mass <= 0.0 || number_density <= 0.0 {
        return 0.0;
    }
    (number_density * charge * charge / (constants::EPSILON_0 * mass)).sqrt()
}

/// Debye length sqrt(eps0 k_B T / (n q^2)) (m).
pub fn debye_length(number_density: f64, charge: f64, temperature: f64) -> f64 {
    if number_density <= 0.0 || charge == 0.0 || temperature <= 0.0 {
        return 0.0;
    }
    (constants::EPSILON_0 * constants::K_B * temperature / (number_density * charge * charge)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Drude model with Ashcroft-Mermin inputs reproduces measured
    /// conductivities: the textbook check on the constants above.
    #[test]
    fn drude_reproduces_measured_conductivity() {
        let e = constants::ELEMENTARY_CHARGE;
        for c in CONDUCTORS {
            let sigma = drude_conductivity(c.electron_density, e, c.relaxation_time, constants::ELECTRON_MASS);
            let err = (sigma - c.measured_conductivity).abs() / c.measured_conductivity;
            assert!(err < 0.06, "{}: drude {sigma:.3e} vs measured {:.3e}", c.name, c.measured_conductivity);
        }
    }

    #[test]
    fn copper_plasma_frequency_matches_textbook() {
        // Free-electron copper: omega_p ~ 1.64e16 rad/s.
        let w = plasma_frequency(8.47e28, constants::ELEMENTARY_CHARGE, constants::ELECTRON_MASS);
        assert!((w - 1.64e16).abs() / 1.64e16 < 0.01, "{w:e}");
    }

    #[test]
    fn preset_names_round_trip() {
        for p in ParticlePreset::ALL {
            assert_eq!(ParticlePreset::parse(p.as_str()), Some(p));
        }
    }
}
