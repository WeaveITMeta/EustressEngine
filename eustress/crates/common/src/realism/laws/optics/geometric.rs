//! Geometric optics — Snell's law, lenses, mirrors, Fresnel.

/// Refraction angle from Snell's law: n1 sin(theta1) = n2 sin(theta2).
///
/// Returns `None` when total internal reflection occurs (the refracted-ray
/// argument exceeds 1, i.e. there is no real refraction angle).
pub fn snell_refraction_angle(n1: f32, theta1_rad: f32, n2: f32) -> Option<f32> {
    let sin_theta2 = n1 * theta1_rad.sin() / n2;
    if sin_theta2.abs() > 1.0 {
        None
    } else {
        Some(sin_theta2.asin())
    }
}

/// Critical angle for total internal reflection: arcsin(n2 / n1).
///
/// Defined only when going from a denser to a rarer medium (n1 > n2);
/// returns `None` otherwise.
pub fn critical_angle(n1: f32, n2: f32) -> Option<f32> {
    if n1 > n2 {
        Some((n2 / n1).asin())
    } else {
        None
    }
}

/// Thin-lens image distance from 1/f = 1/do + 1/di.
pub fn thin_lens_image_distance(focal_length: f32, object_distance: f32) -> f32 {
    1.0 / (1.0 / focal_length - 1.0 / object_distance)
}

/// Thin-lens focal length from 1/f = 1/do + 1/di.
pub fn thin_lens_focal_length(image_distance: f32, object_distance: f32) -> f32 {
    1.0 / (1.0 / object_distance + 1.0 / image_distance)
}

/// Linear magnification: m = -di / do.
pub fn magnification(image_distance: f32, object_distance: f32) -> f32 {
    -image_distance / object_distance
}

/// Lensmaker's equation: 1/f = (n - 1)(1/R1 - 1/R2).
pub fn lensmaker_focal_length(n: f32, r1: f32, r2: f32) -> f32 {
    1.0 / ((n - 1.0) * (1.0 / r1 - 1.0 / r2))
}

/// Mirror image distance from 1/f = 1/do + 1/di (same form as a thin lens).
pub fn mirror_image_distance(focal_length: f32, object_distance: f32) -> f32 {
    1.0 / (1.0 / focal_length - 1.0 / object_distance)
}

/// Spherical-mirror focal length: f = R / 2.
pub fn mirror_focal_length(radius: f32) -> f32 {
    radius / 2.0
}

/// Fresnel reflectance at normal incidence: ((n1 - n2) / (n1 + n2))^2.
pub fn fresnel_reflectance_normal(n1: f32, n2: f32) -> f32 {
    let r = (n1 - n2) / (n1 + n2);
    r * r
}

/// Fresnel transmittance at normal incidence: T = 1 - R.
pub fn fresnel_transmittance_normal(n1: f32, n2: f32) -> f32 {
    1.0 - fresnel_reflectance_normal(n1, n2)
}

/// Brewster's angle: theta_B = atan(n2 / n1), where reflected light is
/// fully polarized.
pub fn brewster_angle(n1: f32, n2: f32) -> f32 {
    (n2 / n1).atan()
}

/// Radius of Snell's window seen from below the surface at a given depth:
/// r = depth * tan(critical angle).
///
/// `n_water` is the refractive index of the lower (denser) medium; the
/// upper medium is assumed to be air (n = 1).
pub fn snells_window_radius(depth: f32, n_water: f32) -> f32 {
    match critical_angle(n_water, 1.0) {
        Some(theta_c) => depth * theta_c.tan(),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::PI;

    const EPS: f32 = 1e-3;

    #[test]
    fn brewster_water_is_about_53_deg() {
        // Air (n1 = 1) to water (n2 = 1.33): Brewster angle ~ 53.06 degrees.
        let theta = brewster_angle(1.0, 1.33);
        let deg = theta * 180.0 / PI;
        assert!((deg - 53.06).abs() < 0.05, "got {deg} deg");
    }

    #[test]
    fn total_internal_reflection_returns_none() {
        // Water (1.33) to air (1.0) past the critical angle (~48.75 deg).
        let theta_c = critical_angle(1.33, 1.0).unwrap();
        // Just beyond the critical angle there is no refracted ray.
        assert!(snell_refraction_angle(1.33, theta_c + 0.05, 1.0).is_none());
        // Just inside it, a refracted ray exists.
        assert!(snell_refraction_angle(1.33, theta_c - 0.05, 1.0).is_some());
    }

    #[test]
    fn thin_lens_round_trip() {
        // Object at 30, focal length 10 -> image at 15.
        let di = thin_lens_image_distance(10.0, 30.0);
        assert!((di - 15.0).abs() < EPS, "image distance {di}");
        // Magnification = -di/do = -0.5.
        assert!((magnification(di, 30.0) + 0.5).abs() < EPS);
    }
}
