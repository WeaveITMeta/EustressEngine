//! Enzyme and metabolic kinetics — Michaelis-Menten, Hill, growth.

/// Ideal gas constant in joules per mole per kelvin (used by the biological
/// Arrhenius form).
const R_GAS: f32 = 8.314;

/// Michaelis-Menten reaction velocity: v = Vmax * S / (Km + S).
pub fn michaelis_menten(v_max: f32, km: f32, substrate: f32) -> f32 {
    let denom = km + substrate;
    if denom == 0.0 {
        return 0.0;
    }
    v_max * substrate / denom
}

/// Hill equation for cooperative binding: v = Vmax * S^n / (K^n + S^n).
pub fn hill_equation(v_max: f32, k_half: f32, hill_coeff: f32, substrate: f32) -> f32 {
    let s_n = substrate.powf(hill_coeff);
    let k_n = k_half.powf(hill_coeff);
    let denom = k_n + s_n;
    if denom == 0.0 {
        return 0.0;
    }
    v_max * s_n / denom
}

/// Vmax recovered from a Lineweaver-Burk (double-reciprocal) plot.
///
/// The y-intercept of 1/v vs 1/S equals 1/Vmax, so Vmax = 1 / y_intercept.
pub fn lineweaver_burk_vmax(_slope: f32, y_intercept: f32) -> f32 {
    if y_intercept == 0.0 {
        return f32::INFINITY;
    }
    1.0 / y_intercept
}

/// Km recovered from a Lineweaver-Burk plot.
///
/// The slope equals Km / Vmax, so Km = slope * Vmax = slope / y_intercept.
pub fn lineweaver_burk_km(slope: f32, y_intercept: f32) -> f32 {
    if y_intercept == 0.0 {
        return f32::INFINITY;
    }
    slope / y_intercept
}

/// Catalytic efficiency (specificity constant): kcat / Km.
pub fn catalytic_efficiency(k_cat: f32, km: f32) -> f32 {
    if km == 0.0 {
        return f32::INFINITY;
    }
    k_cat / km
}

/// Turnover number kcat = Vmax / [E] (reactions per enzyme per unit time).
pub fn turnover_number(v_max: f32, enzyme_concentration: f32) -> f32 {
    if enzyme_concentration == 0.0 {
        return f32::INFINITY;
    }
    v_max / enzyme_concentration
}

/// Monod microbial growth rate: mu = mu_max * S / (Ks + S).
pub fn monod_growth_rate(mu_max: f32, ks: f32, substrate: f32) -> f32 {
    let denom = ks + substrate;
    if denom == 0.0 {
        return 0.0;
    }
    mu_max * substrate / denom
}

/// Q10 temperature coefficient: (R2 / R1)^(10 / (T2 - T1)).
///
/// Describes how a rate changes per 10-degree change in temperature.
pub fn q10_temperature_coefficient(rate_t2: f32, rate_t1: f32, t2: f32, t1: f32) -> f32 {
    let dt = t2 - t1;
    if dt == 0.0 || rate_t1 == 0.0 {
        return 1.0;
    }
    (rate_t2 / rate_t1).powf(10.0 / dt)
}

/// Arrhenius rate (biological form): k = A * exp(-Ea / (R * T)).
///
/// `activation_energy` is in joules per mole and `temperature` in kelvin.
pub fn arrhenius_biological(pre_exp: f32, activation_energy: f32, temperature: f32) -> f32 {
    if temperature == 0.0 {
        return 0.0;
    }
    pre_exp * (-activation_energy / (R_GAS * temperature)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn michaelis_menten_half_vmax_at_km() {
        // When S == Km, velocity should be exactly Vmax / 2.
        let v = michaelis_menten(100.0, 5.0, 5.0);
        assert!((v - 50.0).abs() < 1e-4, "expected 50, got {v}");
    }

    #[test]
    fn hill_with_unit_coefficient_matches_michaelis_menten() {
        // Hill coefficient of 1 reduces to Michaelis-Menten.
        let h = hill_equation(100.0, 5.0, 1.0, 5.0);
        let m = michaelis_menten(100.0, 5.0, 5.0);
        assert!((h - m).abs() < 1e-4, "hill {h} != mm {m}");
    }

    #[test]
    fn q10_doubling() {
        // A rate that doubles over 10 degrees should give Q10 == 2.
        let q = q10_temperature_coefficient(2.0, 1.0, 30.0, 20.0);
        assert!((q - 2.0).abs() < 1e-4, "expected 2, got {q}");
    }
}
