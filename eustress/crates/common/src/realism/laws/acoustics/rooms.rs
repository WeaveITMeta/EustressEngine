//! Room acoustics — reverberation, absorption, modes.

use core::f32::consts::PI;

/// Speed of sound in air used for room-acoustics formulas (m/s).
const C_AIR: f32 = 343.0;

/// Sabine reverberation time (RT60): T = 0.161 V / A, with V in cubic metres
/// and A the total absorption in sabins (square-metre units).
pub fn sabine_reverberation_time(volume: f32, total_absorption: f32) -> f32 {
    0.161 * volume / total_absorption
}

/// Eyring (Norris-Eyring) reverberation time, more accurate at high mean
/// absorption: T = 0.161 V / (-S ln(1 - a_bar)).
pub fn eyring_reverberation_time(volume: f32, surface_area: f32, mean_absorption: f32) -> f32 {
    0.161 * volume / (-surface_area * (1.0 - mean_absorption).ln())
}

/// Total absorption A = sum(S_i a_i) over surfaces.
///
/// Pairs each area with its absorption coefficient up to the shorter slice.
pub fn total_absorption(areas: &[f32], coefficients: &[f32]) -> f32 {
    areas
        .iter()
        .zip(coefficients.iter())
        .map(|(s, a)| s * a)
        .sum()
}

/// Critical (reverberation) distance from the room constant:
/// r_c = 0.141 sqrt(R).
pub fn critical_distance(room_constant: f32) -> f32 {
    0.141 * room_constant.sqrt()
}

/// Room constant: R = A / (1 - a_bar).
pub fn room_constant(total_absorption: f32, mean_absorption: f32) -> f32 {
    total_absorption / (1.0 - mean_absorption)
}

/// Schroeder frequency dividing modal and diffuse regimes:
/// f_s = 2000 sqrt(RT60 / V).
pub fn schroeder_frequency(reverberation_time: f32, volume: f32) -> f32 {
    2000.0 * (reverberation_time / volume).sqrt()
}

/// Axial room-mode frequency along one dimension: f = n c / (2 L).
pub fn axial_mode_frequency(mode: u32, dimension: f32) -> f32 {
    mode as f32 * C_AIR / (2.0 * dimension)
}

/// Approximate count of room modes below a given frequency:
/// N = (4 PI / 3)(f / c)^3 V.
pub fn mode_count_below(frequency: f32, volume: f32) -> f32 {
    (4.0 * PI / 3.0) * (frequency / C_AIR).powi(3) * volume
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sabine_basic_room() {
        // 100 m^3 room with 20 sabins of absorption: RT60 = 0.161 * 100 / 20.
        let rt = sabine_reverberation_time(100.0, 20.0);
        assert!((rt - 0.805).abs() < 1e-3, "got {rt}");
    }

    #[test]
    fn axial_mode_fundamental() {
        // Fundamental axial mode of a 5 m dimension: 343 / 10 = 34.3 Hz.
        let f = axial_mode_frequency(1, 5.0);
        assert!((f - 34.3).abs() < 1e-3, "got {f}");
    }
}
