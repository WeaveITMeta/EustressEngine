//! Electromagnetic wave propagation, optics, and antenna link-budget utilities.
//!
//! # Physical constants
//! - `C`         = 299_792_458 m/s  (speed of light in vacuum)
//! - `MU_0`      = 1.2566371e-6 H/m (permeability of free space)
//! - `EPSILON_0` = 8.854188e-12 F/m (permittivity of free space)

use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// Physical constants
// ---------------------------------------------------------------------------

/// Speed of light in vacuum (m/s).
pub const C: f64 = 299_792_458.0;

/// Permeability of free space (H/m).
pub const MU_0: f64 = 1.256_637_1e-6;

/// Permittivity of free space (F/m).
pub const EPSILON_0: f64 = 8.854_188e-12;

// ---------------------------------------------------------------------------
// Wave kinematics
// ---------------------------------------------------------------------------

/// Phase velocity in a medium with relative permittivity `epsilon_r` and
/// relative permeability `mu_r`.
///
/// v = c / sqrt(epsilon_r * mu_r)
#[inline]
pub fn phase_velocity(epsilon_r: f64, mu_r: f64) -> f64 {
    C / (epsilon_r * mu_r).sqrt()
}

/// Wavelength from phase velocity and frequency.
///
/// lambda = v / f
#[inline]
pub fn wavelength(velocity: f64, frequency_hz: f64) -> f64 {
    velocity / frequency_hz
}

/// Frequency from phase velocity and wavelength.
///
/// f = v / lambda
#[inline]
pub fn frequency(velocity: f64, wavelength_m: f64) -> f64 {
    velocity / wavelength_m
}

/// Wave number from wavelength.
///
/// k = 2*pi / lambda
#[inline]
pub fn wave_number_from_wavelength(wavelength_m: f64) -> f64 {
    2.0 * PI / wavelength_m
}

/// Wave number from angular frequency and phase velocity.
///
/// k = omega / v
#[inline]
pub fn wave_number_from_omega(omega_rad_per_s: f64, velocity: f64) -> f64 {
    omega_rad_per_s / velocity
}

// ---------------------------------------------------------------------------
// Field relationships
// ---------------------------------------------------------------------------

/// Electric field magnitude from magnetic field magnitude in free space.
///
/// E = c * B
#[inline]
pub fn e_from_b(b_tesla: f64) -> f64 {
    C * b_tesla
}

/// Peak Poynting vector magnitude for a plane wave in free space.
///
/// S_peak = E^2 / (mu0 * c)
#[inline]
pub fn poynting_magnitude(e_peak_v_per_m: f64) -> f64 {
    e_peak_v_per_m * e_peak_v_per_m / (MU_0 * C)
}

/// Time-averaged intensity of a plane wave in free space.
///
/// I = E0^2 / (2 * mu0 * c)
#[inline]
pub fn plane_wave_intensity(e0_v_per_m: f64) -> f64 {
    e0_v_per_m * e0_v_per_m / (2.0 * MU_0 * C)
}

// ---------------------------------------------------------------------------
// Fresnel equations at normal incidence
// ---------------------------------------------------------------------------

/// Fresnel reflection coefficient (amplitude) at normal incidence.
///
/// r = (n1 - n2) / (n1 + n2)
#[inline]
pub fn fresnel_reflection_normal(n1: f64, n2: f64) -> f64 {
    (n1 - n2) / (n1 + n2)
}

/// Reflectance (power) at normal incidence.
///
/// R = r^2
#[inline]
pub fn reflectance_normal(n1: f64, n2: f64) -> f64 {
    let r = fresnel_reflection_normal(n1, n2);
    r * r
}

/// Transmittance (power) at normal incidence.
///
/// T = 1 - R
#[inline]
pub fn transmittance_normal(n1: f64, n2: f64) -> f64 {
    1.0 - reflectance_normal(n1, n2)
}

/// Critical angle for total internal reflection (radians).
///
/// Returns `None` when TIR is impossible (n1 <= n2).
///
/// theta_c = arcsin(n2 / n1)
#[inline]
pub fn critical_angle(n1: f64, n2: f64) -> Option<f64> {
    if n1 <= n2 {
        None
    } else {
        Some((n2 / n1).asin())
    }
}

// ---------------------------------------------------------------------------
// Radiation
// ---------------------------------------------------------------------------

/// Power radiated by an oscillating electric dipole (Larmor-like formula).
///
/// P = q^2 * omega^4 * d^2 / (12 * pi * epsilon_0 * c^3)
///
/// - `charge_c`      — charge magnitude (C)
/// - `omega`         — angular frequency (rad/s)
/// - `separation_m`  — dipole arm length (m)
#[inline]
pub fn dipole_radiated_power(charge_c: f64, omega_rad_per_s: f64, separation_m: f64) -> f64 {
    let q2 = charge_c * charge_c;
    let w4 = omega_rad_per_s.powi(4);
    let d2 = separation_m * separation_m;
    q2 * w4 * d2 / (12.0 * PI * EPSILON_0 * C.powi(3))
}

// ---------------------------------------------------------------------------
// Link budget / antenna
// ---------------------------------------------------------------------------

/// Free-space path loss (dimensionless power ratio, not in dB).
///
/// FSPL = (4*pi*d / lambda)^2
///
/// - `distance_m`    — link distance (m)
/// - `wavelength_m`  — carrier wavelength (m)
#[inline]
pub fn free_space_path_loss(distance_m: f64, wavelength_m: f64) -> f64 {
    let ratio = 4.0 * PI * distance_m / wavelength_m;
    ratio * ratio
}

/// Received power via Friis transmission equation.
///
/// Pr = Pt * Gt * Gr * (lambda / (4*pi*d))^2
///
/// - `tx_power_w`    — transmit power (W)
/// - `tx_gain`       — transmitter antenna gain (linear)
/// - `rx_gain`       — receiver antenna gain (linear)
/// - `wavelength_m`  — carrier wavelength (m)
/// - `distance_m`    — link distance (m)
#[inline]
pub fn friis_received_power(
    tx_power_w: f64,
    tx_gain: f64,
    rx_gain: f64,
    wavelength_m: f64,
    distance_m: f64,
) -> f64 {
    let factor = wavelength_m / (4.0 * PI * distance_m);
    tx_power_w * tx_gain * rx_gain * factor * factor
}

// ---------------------------------------------------------------------------
// Unit conversions
// ---------------------------------------------------------------------------

/// Convert watts to dBm.
///
/// P_dBm = 10 * log10(P_W / 1e-3)
#[inline]
pub fn watts_to_dbm(watts: f64) -> f64 {
    10.0 * (watts / 1e-3_f64).log10()
}

/// Convert dBm to watts.
///
/// P_W = 1e-3 * 10^(P_dBm / 10)
#[inline]
pub fn dbm_to_watts(dbm: f64) -> f64 {
    1e-3 * 10.0_f64.powf(dbm / 10.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    // 1. phase_velocity in vacuum (epsilon_r=1, mu_r=1) must equal C.
    #[test]
    fn test_phase_velocity_vacuum() {
        let v = phase_velocity(1.0, 1.0);
        assert!((v - C).abs() < 1.0, "phase_velocity vacuum: got {v}");
    }

    // 2. wavelength / frequency roundtrip.
    #[test]
    fn test_wavelength_frequency_roundtrip() {
        let f0 = 2.4e9_f64; // 2.4 GHz Wi-Fi
        let v = C;
        let lam = wavelength(v, f0);
        let f1 = frequency(v, lam);
        assert!((f1 - f0).abs() / f0 < EPS, "roundtrip: {f0} -> {lam} -> {f1}");
    }

    // 3. wave_number both forms agree.
    #[test]
    fn test_wave_number_consistency() {
        let lam = 0.125; // m
        let v = C;
        let omega = 2.0 * PI * (v / lam);
        let k1 = wave_number_from_wavelength(lam);
        let k2 = wave_number_from_omega(omega, v);
        assert!((k1 - k2).abs() / k1 < EPS, "k1={k1} k2={k2}");
    }

    // 4. 0 dBm == 1 mW.
    #[test]
    fn test_0_dbm_is_1_mw() {
        let w = dbm_to_watts(0.0);
        assert!((w - 1e-3).abs() < 1e-15, "0 dBm -> {w} W (expected 1e-3)");
    }

    // 5. dBm roundtrip.
    #[test]
    fn test_dbm_roundtrip() {
        let p = 0.050; // 50 mW
        let dbm = watts_to_dbm(p);
        let p2 = dbm_to_watts(dbm);
        assert!((p2 - p).abs() / p < EPS, "dBm roundtrip: {p} -> {dbm} -> {p2}");
    }

    // 6. Fresnel normal incidence: air (n=1) / glass (n=1.5) -> R ~= 4%.
    #[test]
    fn test_fresnel_air_glass() {
        let r = reflectance_normal(1.0, 1.5);
        // R = ((1-1.5)/(1+1.5))^2 = (0.5/2.5)^2 = 0.04
        assert!((r - 0.04).abs() < 1e-10, "R = {r} (expected 0.04)");
        let t = transmittance_normal(1.0, 1.5);
        assert!((r + t - 1.0).abs() < 1e-14, "R+T != 1: R={r} T={t}");
    }

    // 7. critical_angle: TIR impossible when n1 <= n2.
    #[test]
    fn test_critical_angle_impossible() {
        assert!(critical_angle(1.0, 1.5).is_none());
        assert!(critical_angle(1.5, 1.5).is_none());
    }

    // 8. Poynting peak vs time-average ratio == 0.5.
    #[test]
    fn test_poynting_peak_vs_time_average() {
        let e0 = 100.0; // V/m
        let peak = poynting_magnitude(e0);
        let avg = plane_wave_intensity(e0);
        let ratio = avg / peak;
        assert!((ratio - 0.5).abs() < EPS, "avg/peak = {ratio} (expected 0.5)");
    }

    // 9. Friis inverse-square law: doubling distance quarters received power.
    #[test]
    fn test_friis_inverse_square() {
        let lam = wavelength(C, 1e9); // 1 GHz
        let p1 = friis_received_power(1.0, 1.0, 1.0, lam, 1000.0);
        let p2 = friis_received_power(1.0, 1.0, 1.0, lam, 2000.0);
        let ratio = p1 / p2;
        assert!((ratio - 4.0).abs() < 1e-10, "Friis ratio = {ratio} (expected 4.0)");
    }
}
