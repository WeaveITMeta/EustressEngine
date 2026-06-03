//! Radiation shielding — attenuation, half-value layer, dose.
//!
//! Pure-math helpers (no Bevy). Intensities are in arbitrary consistent units,
//! linear attenuation coefficients μ are per centimetre (matching thicknesses
//! in centimetres), doses are in Gray / Sievert and dose rates per the activity
//! and gamma-constant units supplied by the caller.

use core::f32::consts::LN_2;

/// Beer–Lambert attenuation of a narrow beam: I = I0 · exp(−μ x).
pub fn attenuation(incident_intensity: f32, linear_attenuation_coeff: f32, thickness: f32) -> f32 {
    incident_intensity * (-linear_attenuation_coeff * thickness).exp()
}

/// Broad-beam attenuation including a scatter buildup factor:
/// I = B · I0 · exp(−μ x).
pub fn attenuation_with_buildup(incident: f32, mu: f32, thickness: f32, buildup_factor: f32) -> f32 {
    buildup_factor * incident * (-mu * thickness).exp()
}

/// Half-value layer (thickness that halves intensity): HVL = ln(2) / μ.
pub fn half_value_layer(mu: f32) -> f32 {
    LN_2 / mu
}

/// Tenth-value layer (thickness that reduces intensity to one tenth):
/// TVL = ln(10) / μ.
pub fn tenth_value_layer(mu: f32) -> f32 {
    10.0_f32.ln() / mu
}

/// Thickness required to reach a target transmission `attenuation_factor`:
/// x = ln(1 / factor) / μ.
///
/// `attenuation_factor` is the surviving fraction (e.g. 0.01 for a 100×
/// reduction).
pub fn thickness_for_attenuation(mu: f32, attenuation_factor: f32) -> f32 {
    (1.0 / attenuation_factor).ln() / mu
}

/// Exponent of the attenuation expressed through the mass attenuation
/// coefficient: (μ/ρ) · ρ · x.
///
/// Returns the dimensionless exponent μx so the transmitted fraction is
/// exp(−result). `mass_attenuation_coeff` is μ/ρ in cm²/g, `density` in g/cm³,
/// `thickness` in cm.
pub fn mass_attenuation_thickness(mass_attenuation_coeff: f32, density: f32, thickness: f32) -> f32 {
    mass_attenuation_coeff * density * thickness
}

/// Dose rate from an unshielded point source: Ḋ = Γ · A / r².
///
/// `gamma_constant` Γ is the dose-rate constant for the nuclide and `distance`
/// is the source-to-point separation.
pub fn dose_rate_point_source(activity: f32, gamma_constant: f32, distance: f32) -> f32 {
    gamma_constant * activity / (distance * distance)
}

/// Dose equivalent in Sievert: H = D · Q.
///
/// `absorbed_dose_gray` is the absorbed dose D in Gray and `quality_factor` Q
/// is the radiation weighting factor.
pub fn dose_equivalent(absorbed_dose_gray: f32, quality_factor: f32) -> f32 {
    absorbed_dose_gray * quality_factor
}

/// Inverse-square scaling of dose between two distances:
/// D2 = D1 · (r1 / r2)².
pub fn inverse_square_dose(dose1: f32, r1: f32, r2: f32) -> f32 {
    let ratio = r1 / r2;
    dose1 * ratio * ratio
}

/// Shield thickness needed to drop from `initial_dose` to `target_dose`,
/// counted in half-value layers: x = HVL · log2(initial / target).
pub fn shielding_layers_needed(initial_dose: f32, target_dose: f32, hvl: f32) -> f32 {
    hvl * (initial_dose / target_dose).log2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_half_value_layer_halves_intensity() {
        let mu = 0.5_f32;
        let hvl = half_value_layer(mu);
        let transmitted = attenuation(100.0, mu, hvl);
        assert!((transmitted - 50.0).abs() < 1e-2);
    }

    #[test]
    fn inverse_square_and_dose_equivalent() {
        // Doubling the distance quarters the dose.
        let d2 = inverse_square_dose(100.0, 1.0, 2.0);
        assert!((d2 - 25.0).abs() < 1e-3);
        // Sievert = Gray × quality factor.
        assert!((dose_equivalent(2.0, 20.0) - 40.0).abs() < 1e-6);
    }

    #[test]
    fn layers_for_hundredfold_reduction() {
        let mu = 0.5_f32;
        let hvl = half_value_layer(mu);
        // 100× reduction is log2(100) ≈ 6.64 half-value layers.
        let layers = shielding_layers_needed(100.0, 1.0, hvl);
        let thickness = thickness_for_attenuation(mu, 0.01);
        // Both routes (HVL count × HVL, and direct solve) must agree.
        assert!((layers - thickness).abs() < 1e-2);
    }
}
