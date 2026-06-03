//! Wave optics — interference, diffraction, resolving power.

/// Fringe spacing on a screen for a double-slit pattern: Delta y = lambda L / d.
pub fn double_slit_fringe_spacing(
    wavelength: f32,
    screen_distance: f32,
    slit_separation: f32,
) -> f32 {
    wavelength * screen_distance / slit_separation
}

/// Angular position of the m-th bright fringe (double slit):
/// sin(theta) = m lambda / d.
///
/// Returns NaN if the argument exceeds 1 (no such order exists).
pub fn double_slit_angle(order: i32, wavelength: f32, slit_separation: f32) -> f32 {
    (order as f32 * wavelength / slit_separation).asin()
}

/// Angular position of the m-th intensity minimum for single-slit
/// diffraction: sin(theta) = m lambda / a.
pub fn single_slit_minima_angle(order: i32, wavelength: f32, slit_width: f32) -> f32 {
    (order as f32 * wavelength / slit_width).asin()
}

/// Diffraction-grating equation: sin(theta) = m lambda / d.
pub fn diffraction_grating_angle(order: i32, wavelength: f32, line_spacing: f32) -> f32 {
    (order as f32 * wavelength / line_spacing).asin()
}

/// Wavelength giving constructive interference in a thin film with a single
/// half-wave phase shift: lambda = 2 n t / (m + 0.5).
pub fn thin_film_constructive_wavelength(n_film: f32, thickness: f32, order: u32) -> f32 {
    2.0 * n_film * thickness / (order as f32 + 0.5)
}

/// Rayleigh criterion for the minimum resolvable angle of a circular
/// aperture: theta = 1.22 lambda / D.
pub fn rayleigh_resolution_angle(wavelength: f32, aperture_diameter: f32) -> f32 {
    1.22 * wavelength / aperture_diameter
}

/// Bragg diffraction angle: sin(theta) = m lambda / (2 d).
pub fn bragg_angle(order: i32, wavelength: f32, plane_spacing: f32) -> f32 {
    (order as f32 * wavelength / (2.0 * plane_spacing)).asin()
}

/// Optical path difference for a geometric path through a medium of
/// refractive index n: OPD = n * geometric_path.
pub fn optical_path_difference(n: f32, geometric_path: f32) -> f32 {
    n * geometric_path
}

/// Number of fringes shifted in a Michelson interferometer when one mirror
/// moves by `mirror_displacement`: N = 2 d / lambda.
pub fn michelson_fringe_shift(mirror_displacement: f32, wavelength: f32) -> f32 {
    2.0 * mirror_displacement / wavelength
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn double_slit_spacing_basic() {
        // 500 nm light, 2 m screen, 1 mm slit separation -> 1 mm fringes.
        let dy = double_slit_fringe_spacing(500e-9, 2.0, 1e-3);
        assert!((dy - 1e-3).abs() < 1e-9, "got {dy}");
    }

    #[test]
    fn michelson_quarter_wave_gives_half_fringe() {
        // Moving a mirror by lambda/4 shifts the pattern by half a fringe.
        let wavelength = 632.8e-9;
        let n = michelson_fringe_shift(wavelength / 4.0, wavelength);
        assert!((n - 0.5).abs() < EPS, "got {n}");
    }
}
