//! Nuclear fusion — Lawson criterion, D-T reaction, gain.
//!
//! Pure-math helpers (no Bevy). Densities are in particles per cubic metre,
//! temperatures in kilo-electron-volts (keV), confinement times in seconds,
//! energies in joules, powers in watts, and the triple product in
//! keV · seconds per cubic metre.

/// Boltzmann constant k_B in joules per kelvin (kept for completeness; the
/// public API works in keV).
#[allow(dead_code)]
const K_B: f32 = 1.380_649e-23;
/// Elementary charge e in coulombs — also the joules-per-electron-volt factor.
const ELEMENTARY_CHARGE: f32 = 1.602_176_634e-19;

/// Coulomb constant k = 1 / (4 · pi · epsilon_0), in newton·metre^2 per coulomb^2.
const COULOMB_CONSTANT: f32 = 8.987_5e9;

/// Deuterium-tritium ignition triple-product threshold, in keV·s/m^3.
const DT_LAWSON_THRESHOLD: f32 = 3.0e21;

/// Energy released per deuterium-tritium fusion reaction: 17.6 MeV.
const DT_REACTION_MEV: f32 = 17.6;

/// Lawson triple product n · T · tau (density · temperature · confinement time),
/// in keV·s/m^3.
pub fn lawson_triple_product(density: f32, temperature_kev: f32, confinement_time: f32) -> f32 {
    density * temperature_kev * confinement_time
}

/// Whether the Lawson triple product meets the deuterium-tritium ignition
/// threshold (~3e21 keV·s/m^3).
pub fn lawson_criterion_met(triple_product: f32) -> bool {
    triple_product >= DT_LAWSON_THRESHOLD
}

/// Deuterium-tritium fusion reactivity <sigma·v>, in cubic metres per second.
///
/// APPROXIMATE: this is a rough smooth fit, not the full Bosch-Hale formula.
/// It uses <sigma·v> ≈ 1.1e-24 · T_keV^2 for temperatures clamped to the
/// [1, 100] keV range, which reproduces the right order of magnitude
/// (~1e-22 m^3/s near 10–20 keV) but should not be relied on for precision
/// reactor design.
pub fn dt_reactivity(temperature_kev: f32) -> f32 {
    let t = temperature_kev.clamp(1.0, 100.0);
    1.1e-24 * t * t
}

/// Fusion power density P = n_D · n_T · <sigma·v> · E, in watts per cubic metre.
pub fn fusion_power_density(
    n_deuterium: f32,
    n_tritium: f32,
    reactivity: f32,
    energy_per_reaction_joules: f32,
) -> f32 {
    n_deuterium * n_tritium * reactivity * energy_per_reaction_joules
}

/// Energy released per deuterium-tritium reaction, in joules (17.6 MeV).
pub fn dt_energy_per_reaction() -> f32 {
    // 17.6 MeV = 17.6e6 eV · (joules per eV).
    DT_REACTION_MEV * 1.0e6 * ELEMENTARY_CHARGE
}

/// Fusion energy gain factor Q = P_fusion / P_heating (dimensionless).
pub fn fusion_gain_q(fusion_power: f32, heating_power: f32) -> f32 {
    if heating_power <= 0.0 {
        return f32::INFINITY;
    }
    fusion_power / heating_power
}

/// Ignition condition on the gain factor.
///
/// True ignition is the Q → infinity limit; practically Q > 20 is treated as
/// near-ignition. NOTE: Q == 1 is "breakeven" (fusion power equals heating
/// power), so this function returns true at q >= 1.0 to flag the breakeven
/// milestone and beyond.
pub fn ignition_condition(q: f32) -> bool {
    q >= 1.0
}

/// Ideal minimum ignition temperature for deuterium-tritium fuel, ~4.3 keV.
pub fn ideal_ignition_temperature_kev() -> f32 {
    4.3
}

/// Coulomb barrier energy E = k · z1 · z2 · e^2 / r, in joules.
///
/// The electrostatic potential energy of two nuclei of charge numbers `z1`
/// and `z2` separated by `separation` metres; this is the barrier that must be
/// overcome (or tunnelled through) for fusion.
pub fn coulomb_barrier_energy(z1: f32, z2: f32, separation: f32) -> f32 {
    if separation <= 0.0 {
        return f32::INFINITY;
    }
    COULOMB_CONSTANT * z1 * z2 * ELEMENTARY_CHARGE * ELEMENTARY_CHARGE / separation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dt_energy_per_reaction_is_about_2_82e_minus_12_joules() {
        // 17.6 MeV ≈ 2.82e-12 J.
        let e = dt_energy_per_reaction();
        assert!((e - 2.82e-12).abs() / 2.82e-12 < 0.01, "E = {e}");
    }

    #[test]
    fn fusion_gain_and_lawson_thresholds() {
        // Q = P_fus / P_heat: 100 MW fusion from 20 MW heating ⇒ Q = 5.
        let q = fusion_gain_q(100.0e6, 20.0e6);
        assert!((q - 5.0).abs() < 1e-3, "Q = {q}");
        // Q = 5 is past breakeven (Q >= 1).
        assert!(ignition_condition(q));
        // A triple product below threshold fails Lawson; above passes.
        assert!(!lawson_criterion_met(1.0e21));
        assert!(lawson_criterion_met(5.0e21));
    }
}
