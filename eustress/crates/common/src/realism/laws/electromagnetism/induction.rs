//! Electromagnetic induction — Faraday's law, inductance, transformers.

use bevy::math::Vec3;

/// Permeability of free space μ₀ [H/m]
const MU_0: f32 = 1.256_637_06e-6_f32;

// ── Faraday's law ────────────────────────────────────────────────

/// EMF induced by changing magnetic flux: ε = -dΦ/dt [V]
pub fn faraday_emf(d_flux_dt: f32) -> f32 {
    -d_flux_dt
}

/// Magnetic flux through a surface: Φ = B·A·cos(θ) [Wb]
pub fn magnetic_flux(b_magnitude: f32, area: f32, theta_rad: f32) -> f32 {
    b_magnitude * area * theta_rad.cos()
}

/// Induced EMF in a rotating coil: ε = N·B·A·ω·sin(ωt) [V]
pub fn rotating_coil_emf(n_turns: u32, b: f32, area: f32, omega: f32, t: f32) -> f32 {
    (n_turns as f32) * b * area * omega * (omega * t).sin()
}

/// Lenz's law direction: induced current opposes the change.
/// Returns -1.0 if dΦ/dt > 0, +1.0 if dΦ/dt < 0, 0.0 if no change.
pub fn lenz_sign(d_flux_dt: f32) -> f32 {
    if d_flux_dt > 0.0 {
        -1.0
    } else if d_flux_dt < 0.0 {
        1.0
    } else {
        0.0
    }
}

// ── Self-inductance ──────────────────────────────────────────────

/// EMF from self-inductance: ε = -L·dI/dt [V]
pub fn self_inductance_emf(l: f32, di_dt: f32) -> f32 {
    -l * di_dt
}

/// Self-inductance of a solenoid: L = μ₀·n²·V [H]
/// where n = N/length is the turn density and V = area·length is the volume.
pub fn solenoid_inductance(n_turns: u32, length: f32, area: f32) -> f32 {
    if length <= 0.0 {
        return 0.0;
    }
    let n = n_turns as f32;
    let turn_density = n / length;
    let volume = area * length;
    MU_0 * turn_density * turn_density * volume
}

/// Self-inductance of a toroid: L = μ₀·N²·A / (2π·r) [H]
pub fn toroid_inductance(n_turns: u32, area: f32, r_mean: f32) -> f32 {
    if r_mean <= 0.0 {
        return 0.0;
    }
    let n = n_turns as f32;
    MU_0 * n * n * area / (2.0 * std::f32::consts::PI * r_mean)
}

// ── Mutual inductance ────────────────────────────────────────────

/// EMF induced in coil 2 by changing current in coil 1: ε₂ = -M·dI₁/dt [V]
pub fn mutual_inductance_emf(m: f32, di1_dt: f32) -> f32 {
    -m * di1_dt
}

/// Coupling coefficient: k = M / √(L1·L2), dimensionless, range [0, 1].
pub fn coupling_coefficient(m: f32, l1: f32, l2: f32) -> f32 {
    let denom = (l1 * l2).sqrt();
    if denom <= 0.0 {
        return 0.0;
    }
    (m / denom).clamp(0.0, 1.0)
}

/// Maximum mutual inductance for perfect coupling: M_max = √(L1·L2) [H]
pub fn max_mutual_inductance(l1: f32, l2: f32) -> f32 {
    (l1 * l2).sqrt()
}

// ── Transformers ─────────────────────────────────────────────────

/// Ideal transformer secondary voltage: V2 = V1·(N2/N1) [V]
pub fn transformer_voltage(v1: f32, n1: u32, n2: u32) -> f32 {
    if n1 == 0 {
        return 0.0;
    }
    v1 * (n2 as f32) / (n1 as f32)
}

/// Ideal transformer secondary current: I2 = I1·(N1/N2) [A]
pub fn transformer_current(i1: f32, n1: u32, n2: u32) -> f32 {
    if n2 == 0 {
        return 0.0;
    }
    i1 * (n1 as f32) / (n2 as f32)
}

/// Reflected impedance from secondary to primary: Z_reflected = Z2·(N1/N2)² [Ω]
pub fn transformer_reflected_impedance(z2: f32, n1: u32, n2: u32) -> f32 {
    if n2 == 0 {
        return 0.0;
    }
    let ratio = (n1 as f32) / (n2 as f32);
    z2 * ratio * ratio
}

/// Transformer efficiency: η = P_out / P_in, where P_in = P_out + P_core + P_copper.
/// Returns a value in [0, 1]. Returns 0 if total input power is zero or negative.
pub fn transformer_efficiency(p_out: f32, p_core_loss: f32, p_copper_loss: f32) -> f32 {
    let p_in = p_out + p_core_loss + p_copper_loss;
    if p_in <= 0.0 {
        return 0.0;
    }
    (p_out / p_in).clamp(0.0, 1.0)
}

// ── Eddy currents / skin effect ──────────────────────────────────

/// Skin depth: δ = √(2ρ / (ω·μ)) [m]
/// resistivity = ρ [Ω·m], omega = angular frequency [rad/s], mu = permeability [H/m]
pub fn skin_depth(resistivity: f32, omega: f32, mu: f32) -> f32 {
    let denom = omega * mu;
    if denom <= 0.0 {
        return f32::INFINITY;
    }
    (2.0 * resistivity / denom).sqrt()
}

/// Power dissipated by eddy currents (simplified lamination model):
/// P ∝ B²·f²·t²·V / ρ [W]
/// b_max = peak flux density [T], freq = frequency [Hz],
/// thickness = lamination thickness [m], volume = material volume [m³],
/// resistivity = ρ [Ω·m]
pub fn eddy_current_power(
    b_max: f32,
    freq: f32,
    thickness: f32,
    volume: f32,
    resistivity: f32,
) -> f32 {
    if resistivity <= 0.0 {
        return f32::INFINITY;
    }
    // Steinmetz-style eddy-current formula: P = (π²·B²·f²·t²·V) / (6·ρ)
    let pi_sq = std::f32::consts::PI * std::f32::consts::PI;
    pi_sq * b_max * b_max * freq * freq * thickness * thickness * volume / (6.0 * resistivity)
}
