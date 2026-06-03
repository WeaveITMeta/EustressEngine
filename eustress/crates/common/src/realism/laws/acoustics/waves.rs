//! Acoustic waves — speed, impedance, intensity, decibels.

/// Reference sound pressure for SPL in air (20 micropascals).
const REFERENCE_PRESSURE: f32 = 20e-6;
/// Reference sound intensity (1 picowatt per square metre).
const REFERENCE_INTENSITY: f32 = 1e-12;

/// Longitudinal sound speed in a thin solid rod: c = sqrt(E / rho).
pub fn sound_speed_solid(youngs_modulus: f32, density: f32) -> f32 {
    (youngs_modulus / density).sqrt()
}

/// Sound speed in a fluid: c = sqrt(K / rho).
pub fn sound_speed_fluid(bulk_modulus: f32, density: f32) -> f32 {
    (bulk_modulus / density).sqrt()
}

/// Sound speed in an ideal gas: c = sqrt(gamma R T), where R is the specific
/// gas constant and T is absolute temperature.
pub fn sound_speed_ideal_gas(gamma: f32, r_specific: f32, temperature: f32) -> f32 {
    (gamma * r_specific * temperature).sqrt()
}

/// Approximate sound speed in air: c = 331.3 sqrt(1 + Tc / 273.15).
pub fn sound_speed_air(temperature_celsius: f32) -> f32 {
    331.3 * (1.0 + temperature_celsius / 273.15).sqrt()
}

/// Characteristic acoustic impedance: Z = rho c.
pub fn acoustic_impedance(density: f32, sound_speed: f32) -> f32 {
    density * sound_speed
}

/// Time-averaged sound intensity from pressure amplitude: I = p^2 / (2 Z).
pub fn sound_intensity(pressure_amplitude: f32, impedance: f32) -> f32 {
    pressure_amplitude * pressure_amplitude / (2.0 * impedance)
}

/// Sound pressure level in decibels: SPL = 20 log10(p / p_ref).
pub fn sound_pressure_level(pressure: f32) -> f32 {
    20.0 * (pressure / REFERENCE_PRESSURE).log10()
}

/// Sound intensity level in decibels: SIL = 10 log10(I / I_ref).
pub fn sound_intensity_level(intensity: f32) -> f32 {
    10.0 * (intensity / REFERENCE_INTENSITY).log10()
}

/// Acoustic wavelength: lambda = c / f.
pub fn wavelength_acoustic(sound_speed: f32, frequency: f32) -> f32 {
    sound_speed / frequency
}

/// Combine incoherent sound levels (in dB): L = 10 log10(sum 10^(Li / 10)).
///
/// Returns negative infinity for an empty slice (no sources).
pub fn combine_sound_levels_db(levels: &[f32]) -> f32 {
    let sum: f32 = levels.iter().map(|l| 10f32.powf(l / 10.0)).sum();
    10.0 * sum.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_speed_at_20c_is_about_343() {
        let c = sound_speed_air(20.0);
        assert!((c - 343.0).abs() < 0.5, "got {c}");
    }

    #[test]
    fn two_equal_sources_add_3db() {
        // Two equal incoherent sources raise the level by ~3 dB.
        let combined = combine_sound_levels_db(&[80.0, 80.0]);
        assert!((combined - 83.01).abs() < 0.05, "got {combined}");
    }

    #[test]
    fn spl_at_reference_pressure_is_zero() {
        // At exactly the reference pressure the SPL is 0 dB.
        assert!(sound_pressure_level(20e-6).abs() < 1e-4);
    }
}
