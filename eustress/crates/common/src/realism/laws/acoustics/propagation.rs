//! Sound propagation — Doppler, attenuation, Mach effects.

use core::f32::consts::PI;

/// Doppler-shifted observed frequency:
/// f' = f (c + vo) / (c - vs).
///
/// Sign convention: `observer_velocity` (vo) is positive when the observer
/// moves toward the source; `source_velocity` (vs) is positive when the
/// source moves toward the observer.
pub fn doppler_observed_frequency(
    source_freq: f32,
    sound_speed: f32,
    observer_velocity: f32,
    source_velocity: f32,
) -> f32 {
    source_freq * (sound_speed + observer_velocity) / (sound_speed - source_velocity)
}

/// Mach number: M = v / c.
pub fn mach_number(velocity: f32, sound_speed: f32) -> f32 {
    velocity / sound_speed
}

/// Half-angle of the Mach cone: theta = arcsin(1 / M), defined only for
/// supersonic motion (M >= 1).
pub fn mach_cone_angle(mach: f32) -> Option<f32> {
    if mach >= 1.0 {
        Some((1.0 / mach).asin())
    } else {
        None
    }
}

/// Intensity after spherical spreading from radius r0 to r:
/// I = I0 (r0 / r)^2.
pub fn spherical_spreading_intensity(source_intensity: f32, r0: f32, r: f32) -> f32 {
    let ratio = r0 / r;
    source_intensity * ratio * ratio
}

/// Level drop in decibels due to spherical spreading: 20 log10(r / r0).
pub fn spherical_spreading_db_loss(r0: f32, r: f32) -> f32 {
    20.0 * (r / r0).log10()
}

/// Atmospheric absorption loss in decibels over a distance:
/// loss = alpha * distance.
pub fn atmospheric_absorption_db(distance: f32, absorption_coeff_db_per_m: f32) -> f32 {
    distance * absorption_coeff_db_per_m
}

/// Intensity from an isotropic point source (inverse-square law):
/// I = P / (4 PI r^2).
pub fn inverse_square_law(power: f32, distance: f32) -> f32 {
    power / (4.0 * PI * distance * distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doppler_approaching_source_raises_pitch() {
        // Source moving toward a stationary observer raises the frequency.
        let observed = doppler_observed_frequency(440.0, 343.0, 0.0, 34.3);
        assert!(observed > 440.0, "got {observed}");
        // 10% of c closing -> f * c / (0.9 c) = f / 0.9.
        assert!((observed - 440.0 / 0.9).abs() < 0.5, "got {observed}");
    }

    #[test]
    fn mach_cone_only_supersonic() {
        assert!(mach_cone_angle(0.8).is_none());
        // At Mach 2 the half-angle is arcsin(0.5) = 30 degrees.
        let theta = mach_cone_angle(2.0).unwrap();
        assert!((theta - PI / 6.0).abs() < 1e-5, "got {theta}");
    }
}
