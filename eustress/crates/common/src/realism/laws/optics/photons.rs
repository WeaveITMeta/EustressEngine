//! Photon physics — Planck, photoelectric, blackbody, Beer-Lambert.

/// Planck constant (J s).
pub const H_PLANCK: f32 = 6.626_070_15e-34;
/// Speed of light in vacuum (m/s).
pub const C_LIGHT: f32 = 299_792_458.0;
/// Boltzmann constant (J/K).
pub const K_B: f32 = 1.380_649e-23;
/// Elementary charge (C).
pub const ELEMENTARY_CHARGE: f32 = 1.602_176_634e-19;

/// Electron rest mass (kg), used for the Compton shift.
const ELECTRON_MASS: f32 = 9.109_383_7e-31;
/// Stefan-Boltzmann constant (W / m^2 K^4).
const STEFAN_BOLTZMANN: f32 = 5.670_374e-8;
/// Wien displacement constant (m K).
const WIEN_CONSTANT: f32 = 2.897_771_955e-3;

/// Photon energy from frequency: E = h f.
pub fn photon_energy(frequency: f32) -> f32 {
    H_PLANCK * frequency
}

/// Photon energy from wavelength: E = h c / lambda.
pub fn photon_energy_from_wavelength(wavelength: f32) -> f32 {
    H_PLANCK * C_LIGHT / wavelength
}

/// Photon momentum from wavelength: p = h / lambda.
pub fn photon_momentum(wavelength: f32) -> f32 {
    H_PLANCK / wavelength
}

/// Maximum kinetic energy of a photoelectron: KE = h f - phi, clamped at 0
/// (no emission below the threshold frequency).
pub fn photoelectric_max_ke(frequency: f32, work_function_joules: f32) -> f32 {
    (H_PLANCK * frequency - work_function_joules).max(0.0)
}

/// Threshold (cutoff) frequency for the photoelectric effect: f0 = phi / h.
pub fn threshold_frequency(work_function_joules: f32) -> f32 {
    work_function_joules / H_PLANCK
}

/// Compton wavelength shift for a photon scattered by `scattering_angle_rad`:
/// Delta lambda = (h / (m_e c)) (1 - cos theta).
pub fn compton_wavelength_shift(scattering_angle_rad: f32) -> f32 {
    (H_PLANCK / (ELECTRON_MASS * C_LIGHT)) * (1.0 - scattering_angle_rad.cos())
}

/// Wien's displacement law — peak emission wavelength of a blackbody:
/// lambda_max = b / T.
pub fn wien_peak_wavelength(temperature: f32) -> f32 {
    WIEN_CONSTANT / temperature
}

/// Total radiated power from a grey body (Stefan-Boltzmann):
/// P = epsilon sigma A T^4.
pub fn stefan_boltzmann_power(emissivity: f32, area: f32, temperature: f32) -> f32 {
    emissivity * STEFAN_BOLTZMANN * area * temperature.powi(4)
}

/// Planck spectral radiance per unit wavelength:
/// B = (2 h c^2 / lambda^5) / (exp(h c / (lambda kB T)) - 1).
pub fn planck_spectral_radiance(wavelength: f32, temperature: f32) -> f32 {
    let numerator = 2.0 * H_PLANCK * C_LIGHT * C_LIGHT / wavelength.powi(5);
    let exponent = H_PLANCK * C_LIGHT / (wavelength * K_B * temperature);
    numerator / (exponent.exp() - 1.0)
}

/// Beer-Lambert transmitted intensity: I = I0 exp(-alpha x).
pub fn beer_lambert_transmission(incident: f32, absorption_coeff: f32, path_length: f32) -> f32 {
    incident * (-absorption_coeff * path_length).exp()
}

/// Convert electron-volts to joules.
pub fn ev_to_joules(ev: f32) -> f32 {
    ev * ELEMENTARY_CHARGE
}

/// Convert joules to electron-volts.
pub fn joules_to_ev(j: f32) -> f32 {
    j / ELEMENTARY_CHARGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wien_peak_for_sun_is_about_502_nm() {
        // The Sun's effective temperature (~5778 K) peaks near 502 nm.
        let lambda = wien_peak_wavelength(5778.0);
        let nm = lambda * 1e9;
        assert!((nm - 501.5).abs() < 1.0, "got {nm} nm");
    }

    #[test]
    fn ev_joule_round_trip() {
        let j = ev_to_joules(2.5);
        assert!((joules_to_ev(j) - 2.5).abs() < 1e-5);
    }

    #[test]
    fn photoelectric_below_threshold_is_zero() {
        // A work function of 2 eV with a sub-threshold photon yields no KE.
        let phi = ev_to_joules(2.0);
        let f_low = threshold_frequency(phi) * 0.5;
        assert_eq!(photoelectric_max_ke(f_low, phi), 0.0);
        // Above threshold the kinetic energy is positive.
        let f_high = threshold_frequency(phi) * 2.0;
        assert!(photoelectric_max_ke(f_high, phi) > 0.0);
    }
}
