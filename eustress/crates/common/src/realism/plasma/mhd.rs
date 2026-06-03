//! Magnetohydrodynamics — Alfven waves, magnetic pressure, beta.
//!
//! Pure-math helpers (no Bevy). All inputs are SI: magnetic fields in tesla,
//! mass densities in kilograms per cubic metre, pressures in pascals,
//! velocities and speeds in metres per second, lengths in metres, electrical
//! resistivity in ohm-metres.

/// Vacuum permeability mu_0 in henries per metre.
const MU_0: f32 = 1.256_637e-6;

/// Alfven speed v_A = B / sqrt(mu_0 · rho), in metres per second.
///
/// The propagation speed of transverse magnetohydrodynamic (Alfven) waves
/// along magnetic field lines.
pub fn alfven_speed(magnetic_field: f32, density: f32) -> f32 {
    if density <= 0.0 {
        return f32::INFINITY;
    }
    magnetic_field / (MU_0 * density).sqrt()
}

/// Magnetic pressure p_mag = B^2 / (2 · mu_0), in pascals.
pub fn magnetic_pressure(magnetic_field: f32) -> f32 {
    magnetic_field * magnetic_field / (2.0 * MU_0)
}

/// Plasma beta = p_thermal / p_magnetic = p / (B^2 / (2 · mu_0)).
///
/// Ratio of thermal pressure to magnetic pressure; beta < 1 means the field
/// dominates, beta > 1 means the plasma pressure dominates.
pub fn plasma_beta(thermal_pressure: f32, magnetic_field: f32) -> f32 {
    let p_mag = magnetic_pressure(magnetic_field);
    if p_mag <= 0.0 {
        return f32::INFINITY;
    }
    thermal_pressure / p_mag
}

/// Magnetic Reynolds number R_m = v · L / eta (dimensionless).
///
/// Ratio of advection to diffusion of the magnetic field; large R_m implies
/// the field is frozen into the flow.
pub fn magnetic_reynolds_number(velocity: f32, length: f32, magnetic_diffusivity: f32) -> f32 {
    if magnetic_diffusivity <= 0.0 {
        return f32::INFINITY;
    }
    velocity * length / magnetic_diffusivity
}

/// Magnetic diffusivity eta = resistivity / mu_0, in square metres per second.
pub fn magnetic_diffusivity(resistivity: f32) -> f32 {
    resistivity / MU_0
}

/// Magnetic tension force per unit volume magnitude T = B^2 / (mu_0 · R_c),
/// in pascals per metre, where R_c is the field-line radius of curvature.
pub fn magnetic_tension(magnetic_field: f32, radius_of_curvature: f32) -> f32 {
    if radius_of_curvature <= 0.0 {
        return f32::INFINITY;
    }
    magnetic_field * magnetic_field / (MU_0 * radius_of_curvature)
}

/// Fast magnetosonic speed v_ms = sqrt(c_s^2 + v_A^2), in metres per second,
/// combining the sound speed and the Alfven speed.
pub fn magnetosonic_speed(sound_speed: f32, alfven_speed: f32) -> f32 {
    (sound_speed * sound_speed + alfven_speed * alfven_speed).sqrt()
}

/// Magnetic energy density u_B = B^2 / (2 · mu_0), in joules per cubic metre.
///
/// Numerically equal to the magnetic pressure (energy density and pressure
/// share the same expression for the magnetic field).
pub fn magnetic_energy_density(magnetic_field: f32) -> f32 {
    magnetic_field * magnetic_field / (2.0 * MU_0)
}

/// Frozen-in flux condition: true when the magnetic Reynolds number is large
/// (R_m >> 1, taken here as R_m > 1), so field lines move with the fluid.
pub fn frozen_in_condition(magnetic_reynolds: f32) -> bool {
    magnetic_reynolds > 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnetic_pressure_equals_energy_density() {
        // Pressure and energy density share the same B^2 / (2 mu_0) form.
        let b = 2.5_f32;
        let p = magnetic_pressure(b);
        let u = magnetic_energy_density(b);
        assert!((p - u).abs() < 1e-3, "p = {p}, u = {u}");
        // Sanity: a 1 T field stores ~3.98e5 J/m^3.
        let u1 = magnetic_energy_density(1.0);
        assert!((u1 - 3.98e5).abs() / 3.98e5 < 0.01, "u(1 T) = {u1}");
    }

    #[test]
    fn alfven_speed_and_beta_behave() {
        // B = 1 T, rho = 1e-7 kg/m^3 ⇒ v_A ≈ 2.82e6 m/s.
        let v_a = alfven_speed(1.0, 1e-7);
        assert!((v_a - 2.82e6).abs() / 2.82e6 < 0.02, "v_A = {v_a}");
        // When thermal pressure equals magnetic pressure, beta == 1.
        let p_mag = magnetic_pressure(1.0);
        let beta = plasma_beta(p_mag, 1.0);
        assert!((beta - 1.0).abs() < 1e-3, "beta = {beta}");
        // Large magnetic Reynolds number means the field is frozen in.
        assert!(frozen_in_condition(magnetic_reynolds_number(1e5, 10.0, 1.0)));
    }
}
