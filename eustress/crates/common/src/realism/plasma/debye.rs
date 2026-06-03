//! Plasma fundamentals — Debye length, plasma frequency, gyration.
//!
//! Pure-math helpers (no Bevy). All inputs are SI unless noted:
//! number densities in particles per cubic metre, temperatures in kelvin,
//! masses in kilograms, charges in coulombs, magnetic fields in tesla.
//! Returned lengths are in metres, frequencies in radians per second (or
//! hertz where the function name ends in `_hz`).

use core::f32::consts::PI;

/// Vacuum permittivity epsilon_0 in farads per metre.
const EPSILON_0: f32 = 8.854_188e-12;
/// Boltzmann constant k_B in joules per kelvin.
const K_B: f32 = 1.380_649e-23;
/// Elementary charge e in coulombs.
const ELEMENTARY_CHARGE: f32 = 1.602_176_634e-19;
/// Electron rest mass m_e in kilograms.
const ELECTRON_MASS: f32 = 9.109_383_7e-31;

/// Conversion factor: 1 electron-volt of temperature equals this many kelvin
/// (k_B in eV/K is the reciprocal). 1 eV ≈ 11604.5 K.
const EV_PER_KELVIN: f32 = 11_604.5;

/// Debye length lambda_D = sqrt(epsilon_0 · k_B · T_e / (n_e · e^2)).
///
/// The characteristic screening distance over which mobile charge carriers
/// shield out electric fields in a plasma.
pub fn debye_length(electron_density: f32, electron_temperature_k: f32) -> f32 {
    if electron_density <= 0.0 {
        return f32::INFINITY;
    }
    let numerator = EPSILON_0 * K_B * electron_temperature_k;
    let denominator = electron_density * ELEMENTARY_CHARGE * ELEMENTARY_CHARGE;
    (numerator / denominator).sqrt()
}

/// Electron plasma (angular) frequency omega_pe = sqrt(n_e · e^2 / (epsilon_0 · m_e)),
/// in radians per second.
pub fn plasma_frequency(electron_density: f32) -> f32 {
    let numerator = electron_density * ELEMENTARY_CHARGE * ELEMENTARY_CHARGE;
    let denominator = EPSILON_0 * ELECTRON_MASS;
    (numerator / denominator).sqrt()
}

/// Electron plasma frequency in hertz: f_pe = omega_pe / (2 · pi).
pub fn plasma_frequency_hz(electron_density: f32) -> f32 {
    plasma_frequency(electron_density) / (2.0 * PI)
}

/// Larmor (gyration) radius r_L = m · v_perp / (q · B), in metres.
///
/// The radius of a charged particle's circular motion perpendicular to a
/// uniform magnetic field.
pub fn larmor_radius(
    mass: f32,
    perpendicular_velocity: f32,
    charge: f32,
    magnetic_field: f32,
) -> f32 {
    let denominator = charge.abs() * magnetic_field.abs();
    if denominator <= 0.0 {
        return f32::INFINITY;
    }
    mass * perpendicular_velocity / denominator
}

/// Cyclotron (gyro) angular frequency omega_c = |q| · B / m, in radians per second.
pub fn cyclotron_frequency(charge: f32, magnetic_field: f32, mass: f32) -> f32 {
    if mass <= 0.0 {
        return f32::INFINITY;
    }
    charge.abs() * magnetic_field.abs() / mass
}

/// Plasma parameter N_D — the number of particles inside a Debye sphere:
/// N_D = (4/3) · pi · n_e · lambda_D^3.
///
/// A collective plasma requires N_D >> 1.
pub fn plasma_parameter(electron_density: f32, electron_temperature_k: f32) -> f32 {
    let lambda_d = debye_length(electron_density, electron_temperature_k);
    if !lambda_d.is_finite() {
        return f32::INFINITY;
    }
    (4.0 / 3.0) * PI * electron_density * lambda_d * lambda_d * lambda_d
}

/// Coulomb logarithm ln(Lambda), the standard plasma-physics collision factor.
///
/// Uses the common approximation ln(Lambda) ≈ 23 − ln(sqrt(n_e) / T_e^1.5),
/// with the electron temperature expressed in electron-volts (the kelvin input
/// is converted by dividing by ~11604.5). The result is clamped to be positive
/// because the logarithmic fit is only meaningful for ln(Lambda) > 0.
pub fn coulomb_logarithm(electron_density: f32, electron_temperature_k: f32) -> f32 {
    let temperature_ev = electron_temperature_k / EV_PER_KELVIN;
    if electron_density <= 0.0 || temperature_ev <= 0.0 {
        return 1.0;
    }
    let ratio = electron_density.sqrt() / temperature_ev.powf(1.5);
    let value = 23.0 - ratio.ln();
    // Guard positive: collision logarithm is physically > 0; floor at 1.
    value.max(1.0)
}

/// Thermal velocity v_th = sqrt(2 · k_B · T / m), in metres per second.
pub fn thermal_velocity(temperature_k: f32, mass: f32) -> f32 {
    if mass <= 0.0 {
        return f32::INFINITY;
    }
    (2.0 * K_B * temperature_k / mass).sqrt()
}

/// Test whether an ionized gas behaves as a plasma.
///
/// Both conditions must hold: the Debye length is much smaller than the system
/// size (lambda_D < L) and there are many particles in a Debye sphere
/// (N_D >> 1, taken here as N_D > 1).
pub fn is_plasma(debye_length: f32, system_size: f32, plasma_parameter: f32) -> bool {
    debye_length < system_size && plasma_parameter > 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debye_length_and_plasma_parameter_are_consistent() {
        // A fusion-relevant plasma: n_e = 1e19 m^-3, T_e = 1e7 K (~860 eV).
        let lambda_d = debye_length(1e19, 1e7);
        // Debye length should be a small positive distance (sub-millimetre here).
        assert!(lambda_d > 0.0 && lambda_d < 1e-2, "lambda_D = {lambda_d}");
        // Many particles in the Debye sphere ⇒ genuine plasma.
        let n_d = plasma_parameter(1e19, 1e7);
        assert!(n_d > 1.0, "N_D = {n_d}");
        assert!(is_plasma(lambda_d, 1.0, n_d));
    }

    #[test]
    fn plasma_frequency_hz_matches_known_scaling() {
        // For n_e = 1e18 m^-3 the electron plasma frequency is ~8.98 GHz.
        let f = plasma_frequency_hz(1e18);
        assert!((f - 8.98e9).abs() / 8.98e9 < 0.02, "f_pe = {f}");
        // The angular form is exactly 2*pi times the hertz form.
        let omega = plasma_frequency(1e18);
        assert!((omega - 2.0 * PI * f).abs() / omega < 1e-4);
    }
}
