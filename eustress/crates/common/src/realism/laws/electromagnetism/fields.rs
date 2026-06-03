//! Fundamental electromagnetic field equations for the Eustress Engine.
//!
//! ## Maxwell Equations Coverage
//!
//! - **Gauss's Law for E** (∇·E = ρ/ε₀):
//!   [`gauss_flux`], [`electric_field_point`], [`electric_potential_point`],
//!   [`coulomb_force`], [`coulomb_potential_energy`]
//!
//! - **Gauss's Law for B** (∇·B = 0):
//!   Implicitly satisfied by all magnetic field computations derived from
//!   the Biot-Savart law ([`biot_savart`]).
//!
//! - **Faraday's Law** (∇×E = -∂B/∂t):
//!   [`electric_field_from_potential`] provides the electrostatic case (∂B/∂t = 0).
//!
//! - **Ampère-Maxwell Law** (∇×B = μ₀J + μ₀ε₀∂E/∂t):
//!   [`biot_savart`] covers the magnetostatic J term;
//!   [`displacement_current_density`] covers the ε₀∂E/∂t term.
//!
//! - **Lorentz Force** (F = q(E + v×B)):
//!   [`lorentz_force`], [`lorentz_force_magnetic`]
//!
//! - **Energy densities**: [`electric_energy_density`], [`magnetic_energy_density`]
//!
//! - **EM wave propagation**: [`em_wave_speed`], [`refractive_index`]

use bevy::math::Vec3;

// ── Physical Constants ────────────────────────────────────────────

/// Coulomb's constant k = 1/(4πε₀)  [N·m²/C²]
const COULOMB_K: f32 = 8.987_551_8e9;

/// Vacuum permeability μ₀  [H/m]
const VACUUM_PERMEABILITY: f32 = 1.256_637e-6;

/// Vacuum permittivity ε₀  [F/m]
const VACUUM_PERMITTIVITY: f32 = 8.854_188e-12;

/// Speed of light in vacuum c  [m/s]
const SPEED_OF_LIGHT: f32 = 299_792_458.0;

/// Elementary charge e  [C]
pub const ELEMENTARY_CHARGE: f32 = 1.602_176e-19;

// ── Electrostatics ────────────────────────────────────────────────

/// Coulomb force on charge `q1` due to charge `q2`.
///
/// `r_vec` = position of q1 minus position of q2.
///
/// F = k·q1·q2 / r² · r̂
///
/// Implements **Gauss's Law for E** in its integral (Coulomb) form.
/// Returns the zero vector when `r_vec` is zero-length (charges coincide).
pub fn coulomb_force(q1: f32, q2: f32, r_vec: Vec3) -> Vec3 {
    let r2 = r_vec.length_squared();
    if r2 == 0.0 {
        return Vec3::ZERO;
    }
    let r = r2.sqrt();
    let r_hat = r_vec / r;
    r_hat * (COULOMB_K * q1 * q2 / r2)
}

/// Electric field from a point charge `q` at the field point described by `r_vec`.
///
/// `r_vec` = field_pos − charge_pos.
///
/// E = k·q / r² · r̂
///
/// Implements **Gauss's Law for E** (differential form: ∇·E = ρ/ε₀).
/// Returns the zero vector when `r_vec` is zero-length.
pub fn electric_field_point(q: f32, r_vec: Vec3) -> Vec3 {
    let r2 = r_vec.length_squared();
    if r2 == 0.0 {
        return Vec3::ZERO;
    }
    let r = r2.sqrt();
    let r_hat = r_vec / r;
    r_hat * (COULOMB_K * q / r2)
}

/// Potential energy of two point charges separated by scalar distance `r`.
///
/// U = k·q1·q2 / r
///
/// Implements the electrostatic potential energy derived from Gauss's Law.
/// Returns `f32::INFINITY` (or large value) when `r` ≈ 0; caller should guard.
pub fn coulomb_potential_energy(q1: f32, q2: f32, r: f32) -> f32 {
    if r == 0.0 {
        return f32::INFINITY;
    }
    COULOMB_K * q1 * q2 / r
}

/// Electric potential (voltage) from a point charge `q` at scalar distance `r`.
///
/// V = k·q / r
///
/// Implements the scalar form of **Gauss's Law for E**.
/// Returns `f32::INFINITY` when `r` = 0.
pub fn electric_potential_point(q: f32, r: f32) -> f32 {
    if r == 0.0 {
        return f32::INFINITY;
    }
    COULOMB_K * q / r
}

/// Electric field from the negative potential gradient: E = −∇V.
///
/// Uses a central finite-difference approximation with step `h`.
///
/// - `vx[0]` = V(x − h),  `vx[1]` = V(x + h)
/// - `vy[0]` = V(y − h),  `vy[1]` = V(y + h)
/// - `vz[0]` = V(z − h),  `vz[1]` = V(z + h)
///
/// Implements **Faraday's Law** in the electrostatic limit (∇×E = 0 ⟹ E = −∇V).
pub fn electric_field_from_potential(
    vx: [f32; 2],
    vy: [f32; 2],
    vz: [f32; 2],
    h: f32,
) -> Vec3 {
    let two_h = 2.0 * h;
    Vec3::new(
        -(vx[1] - vx[0]) / two_h,
        -(vy[1] - vy[0]) / two_h,
        -(vz[1] - vz[0]) / two_h,
    )
}

/// Energy density of an electric field: u = ½·ε₀·E²  [J/m³].
///
/// Derived from the energy stored in the electric field (Maxwell stress tensor).
pub fn electric_energy_density(e_field_magnitude: f32) -> f32 {
    0.5 * VACUUM_PERMITTIVITY * e_field_magnitude * e_field_magnitude
}

/// Total electric flux through a closed surface enclosing charge `q_enclosed`.
///
/// Φ_E = Q_enc / ε₀
///
/// Direct statement of **Gauss's Law for E** in integral form.
pub fn gauss_flux(q_enclosed: f32) -> f32 {
    q_enclosed / VACUUM_PERMITTIVITY
}

// ── Magnetostatics ────────────────────────────────────────────────

/// Biot-Savart law: differential magnetic field `dB` from a current element `I·dl`
/// at the observation point described by `r_vec`.
///
/// `r_vec` = observation_pos − current_element_pos.
///
/// dB = (μ₀ / 4π) · (I · dl × r̂) / r²
///
/// Implements the magnetostatic part of the **Ampère-Maxwell Law** (∇×B = μ₀J).
/// Returns the zero vector when `r_vec` is zero-length.
pub fn biot_savart(current_amps: f32, dl: Vec3, r_vec: Vec3) -> Vec3 {
    let r2 = r_vec.length_squared();
    if r2 == 0.0 {
        return Vec3::ZERO;
    }
    let r = r2.sqrt();
    let r_hat = r_vec / r;
    let prefactor = (VACUUM_PERMEABILITY / (4.0 * std::f32::consts::PI)) * current_amps / r2;
    dl.cross(r_hat) * prefactor
}

/// Magnetic component of the Lorentz force on a moving charge.
///
/// F = q · v × B
///
/// Part of the full **Lorentz Force Law**.
pub fn lorentz_force_magnetic(q: f32, v: Vec3, b: Vec3) -> Vec3 {
    q * v.cross(b)
}

/// Full Lorentz force on a charge moving in combined electric and magnetic fields.
///
/// F = q · (E + v × B)
///
/// Implements the complete **Lorentz Force Law**, which couples Maxwell's equations
/// to particle dynamics.
pub fn lorentz_force(q: f32, v: Vec3, e: Vec3, b: Vec3) -> Vec3 {
    q * (e + v.cross(b))
}

/// Energy density of a magnetic field: u = B² / (2·μ₀)  [J/m³].
///
/// Derived from the energy stored in the magnetic field (Maxwell stress tensor).
pub fn magnetic_energy_density(b_magnitude: f32) -> f32 {
    (b_magnitude * b_magnitude) / (2.0 * VACUUM_PERMEABILITY)
}

/// Force per unit length between two infinite parallel wires carrying currents
/// `i1` and `i2` separated by scalar distance `distance`.
///
/// F/L = μ₀·I1·I2 / (2π·d)  [N/m]
///
/// Positive result = attractive (currents in the same direction).
/// Negative result = repulsive (currents in opposite directions).
///
/// Derived from the **Biot-Savart Law** and the Lorentz force.
pub fn parallel_wire_force_per_length(i1: f32, i2: f32, distance: f32) -> f32 {
    if distance == 0.0 {
        return f32::INFINITY;
    }
    (VACUUM_PERMEABILITY * i1 * i2) / (2.0 * std::f32::consts::PI * distance)
}

// ── Maxwell Displacement Current ─────────────────────────────────

/// Displacement current density from a time-varying electric field.
///
/// J_d = ε₀ · ∂E/∂t  [A/m²]
///
/// `d_electric_field_dt` is the magnitude of ∂E/∂t [V/(m·s)].
///
/// Implements Maxwell's addition to **Ampère's Law**:
/// ∇×B = μ₀J + μ₀ε₀∂E/∂t, ensuring charge conservation and predicting EM waves.
pub fn displacement_current_density(d_electric_field_dt: f32) -> f32 {
    VACUUM_PERMITTIVITY * d_electric_field_dt
}

/// Electromagnetic wave speed in a medium with relative permittivity `epsilon_r`
/// and relative permeability `mu_r`.
///
/// v = 1 / √(ε · μ) = c / √(ε_r · μ_r)  [m/s]
///
/// Derived from the **wave equation** obtained by combining Faraday's Law and the
/// Ampère-Maxwell Law (∇²E = με ∂²E/∂t²).
pub fn em_wave_speed(epsilon_r: f32, mu_r: f32) -> f32 {
    SPEED_OF_LIGHT / (epsilon_r * mu_r).sqrt()
}

/// Refractive index of a medium: n = c / v = √(ε_r · μ_r).
///
/// Derived from the **Maxwell wave equation** relating phase velocity to
/// material permittivity and permeability.
pub fn refractive_index(epsilon_r: f32, mu_r: f32) -> f32 {
    (epsilon_r * mu_r).sqrt()
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-3;

    fn approx_eq(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn test_coulomb_force_magnitude() {
        // Two unit charges 1 m apart: F = k N
        let r = Vec3::X;
        let f = coulomb_force(1.0, 1.0, r);
        assert!(approx_eq(f.length(), COULOMB_K, EPSILON), "F = {}", f.length());
    }

    #[test]
    fn test_coulomb_force_opposite_charges_attract() {
        let r = Vec3::X;
        let f = coulomb_force(1.0, -1.0, r);
        // Force on q1 points toward q2 (negative x)
        assert!(f.x < 0.0);
    }

    #[test]
    fn test_coulomb_force_zero_separation() {
        assert_eq!(coulomb_force(1.0, 1.0, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn test_electric_field_point_magnitude() {
        // E at 1 m from unit charge = k V/m
        let e = electric_field_point(1.0, Vec3::X);
        assert!(approx_eq(e.length(), COULOMB_K, EPSILON));
    }

    #[test]
    fn test_coulomb_potential_energy() {
        let u = coulomb_potential_energy(1.0, 1.0, 1.0);
        assert!(approx_eq(u, COULOMB_K, EPSILON));
    }

    #[test]
    fn test_electric_potential_point() {
        let v = electric_potential_point(1.0, 1.0);
        assert!(approx_eq(v, COULOMB_K, EPSILON));
    }

    #[test]
    fn test_electric_field_from_potential_uniform() {
        // Uniform field E = 10 V/m in x: V(x) = -10·x
        let h = 0.01_f32;
        let vx = [-10.0 * (-h), -10.0 * h]; // [V(x-h), V(x+h)] = [0.1, -0.1]
        let vy = [0.0, 0.0];
        let vz = [0.0, 0.0];
        let e = electric_field_from_potential(vx, vy, vz, h);
        assert!(approx_eq(e.x, 10.0, EPSILON), "Ex = {}", e.x);
        assert!(approx_eq(e.y.abs(), 0.0_f32, 1e-6_f32.max(EPSILON)));
    }

    #[test]
    fn test_electric_energy_density() {
        // u = 0.5 * ε₀ * E²
        let e_mag = 1000.0_f32;
        let u = electric_energy_density(e_mag);
        let expected = 0.5 * VACUUM_PERMITTIVITY * e_mag * e_mag;
        assert!(approx_eq(u, expected, EPSILON));
    }

    #[test]
    fn test_gauss_flux() {
        let q = 1.0_f32;
        let flux = gauss_flux(q);
        assert!(approx_eq(flux, q / VACUUM_PERMITTIVITY, EPSILON));
    }

    #[test]
    fn test_biot_savart_direction() {
        // Current in +z, observation in +x → dB should be in +y
        let db = biot_savart(1.0, Vec3::Z, Vec3::X);
        assert!(db.y > 0.0, "dB.y = {}", db.y);
        assert!(db.x.abs() < 1e-10 && db.z.abs() < 1e-10);
    }

    #[test]
    fn test_biot_savart_zero_r() {
        assert_eq!(biot_savart(1.0, Vec3::Z, Vec3::ZERO), Vec3::ZERO);
    }

    #[test]
    fn test_lorentz_force_magnetic() {
        // q=1, v=+x, B=+z → F = +x × +z = -y
        let f = lorentz_force_magnetic(1.0, Vec3::X, Vec3::Z);
        assert!(f.y < 0.0, "Fy = {}", f.y);
    }

    #[test]
    fn test_lorentz_force_full() {
        let q = 2.0;
        let v = Vec3::X;
        let e = Vec3::Y;
        let b = Vec3::ZERO;
        let f = lorentz_force(q, v, e, b);
        // With B=0 it reduces to qE
        assert!(approx_eq(f.y, q * e.y, EPSILON));
    }

    #[test]
    fn test_magnetic_energy_density() {
        let b = 1.0_f32;
        let u = magnetic_energy_density(b);
        let expected = (b * b) / (2.0 * VACUUM_PERMEABILITY);
        assert!(approx_eq(u, expected, EPSILON));
    }

    #[test]
    fn test_parallel_wire_force_per_length_attractive() {
        // Same direction currents → positive (attractive)
        let f_per_l = parallel_wire_force_per_length(1.0, 1.0, 1.0);
        assert!(f_per_l > 0.0);
        let expected = VACUUM_PERMEABILITY / (2.0 * std::f32::consts::PI);
        assert!(approx_eq(f_per_l, expected, EPSILON));
    }

    #[test]
    fn test_displacement_current_density() {
        let jd = displacement_current_density(1.0);
        assert!(approx_eq(jd, VACUUM_PERMITTIVITY, EPSILON));
    }

    #[test]
    fn test_em_wave_speed_vacuum() {
        // In vacuum ε_r=1, μ_r=1 → v = c
        let v = em_wave_speed(1.0, 1.0);
        assert!(approx_eq(v, SPEED_OF_LIGHT, EPSILON), "v = {}", v);
    }

    #[test]
    fn test_refractive_index_vacuum() {
        let n = refractive_index(1.0, 1.0);
        assert!(approx_eq(n, 1.0, EPSILON));
    }

    #[test]
    fn test_refractive_index_glass() {
        // Typical glass: ε_r ≈ 2.25, μ_r ≈ 1 → n ≈ 1.5
        let n = refractive_index(2.25, 1.0);
        assert!(approx_eq(n, 1.5, EPSILON), "n = {}", n);
    }
}
