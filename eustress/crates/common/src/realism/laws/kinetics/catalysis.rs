//! Catalytic kinetics: Michaelis-Menten, Langmuir-Hinshelwood, inhibition.

// ── Michaelis-Menten enzyme kinetics ─────────────────────────────

/// Reaction rate: v = Vmax·[S] / (Km + [S])
/// Vmax — maximum velocity, Km — Michaelis constant [mol/L], s — substrate concentration.
pub fn michaelis_menten(v_max: f32, km: f32, substrate: f32) -> f32 {
    if km + substrate == 0.0 {
        return 0.0;
    }
    v_max * substrate / (km + substrate)
}

/// Km from Lineweaver-Burk x-intercept. In the 1/v vs 1/[S] plot the line
/// crosses the x-axis at -1/Km, so Km = -1/x_intercept.
pub fn km_from_lineweaver(x_intercept: f32) -> f32 {
    if x_intercept == 0.0 {
        return f32::INFINITY;
    }
    -1.0 / x_intercept
}

/// Vmax from y-intercept: Vmax = 1 / y_intercept
pub fn vmax_from_lineweaver(y_intercept: f32) -> f32 {
    if y_intercept == 0.0 {
        return f32::INFINITY;
    }
    1.0 / y_intercept
}

/// Hill equation (cooperative binding): v = Vmax·[S]ⁿ / (K½ⁿ + [S]ⁿ)
/// n — Hill coefficient (>1 = positive cooperativity, <1 = negative)
pub fn hill_kinetics(v_max: f32, k_half: f32, n: f32, substrate: f32) -> f32 {
    let sn = substrate.powf(n);
    let kn = k_half.powf(n);
    let denom = kn + sn;
    if denom == 0.0 {
        return 0.0;
    }
    v_max * sn / denom
}

/// Hill coefficient from two points at 10% and 90% saturation
pub fn hill_coefficient(s_10pct: f32, s_90pct: f32) -> f32 {
    // At 10% saturation: θ = 0.10 → [S]^n / (K½^n + [S]^n) = 0.10
    //   → [S]^n = 0.10 K½^n / 0.90  → log([S]^n / K½^n) = log(0.10/0.90)
    // At 90% saturation: θ = 0.90 → [S]^n = 0.90 K½^n / 0.10
    // Combining both:
    //   n = log(81) / log(s_90pct / s_10pct)
    // because (0.9/0.1) / (0.1/0.9) = 81
    if s_10pct <= 0.0 || s_90pct <= 0.0 || s_90pct == s_10pct {
        return 1.0;
    }
    let ratio = s_90pct / s_10pct;
    if ratio <= 0.0 {
        return 1.0;
    }
    81.0_f32.ln() / ratio.ln()
}

// ── Enzyme inhibition ─────────────────────────────────────────────

/// Competitive inhibition: apparent Km increases, Vmax unchanged
/// K_m_app = Km·(1 + [I]/Ki)
pub fn competitive_inhibition_rate(
    v_max: f32,
    km: f32,
    substrate: f32,
    inhibitor: f32,
    ki: f32,
) -> f32 {
    if ki == 0.0 {
        return 0.0;
    }
    let km_app = km * (1.0 + inhibitor / ki);
    let denom = km_app + substrate;
    if denom == 0.0 {
        return 0.0;
    }
    v_max * substrate / denom
}

/// Non-competitive (mixed) inhibition: Vmax_app = Vmax/(1 + [I]/Ki)
pub fn noncompetitive_inhibition_rate(
    v_max: f32,
    km: f32,
    substrate: f32,
    inhibitor: f32,
    ki: f32,
) -> f32 {
    if ki == 0.0 {
        return 0.0;
    }
    let alpha = 1.0 + inhibitor / ki;
    let v_max_app = v_max / alpha;
    let denom = km + substrate;
    if denom == 0.0 {
        return 0.0;
    }
    v_max_app * substrate / denom
}

/// Uncompetitive inhibition: both Vmax and Km are reduced
pub fn uncompetitive_inhibition_rate(
    v_max: f32,
    km: f32,
    substrate: f32,
    inhibitor: f32,
    ki: f32,
) -> f32 {
    if ki == 0.0 {
        return 0.0;
    }
    let alpha_prime = 1.0 + inhibitor / ki;
    let v_max_app = v_max / alpha_prime;
    let km_app = km / alpha_prime;
    let denom = km_app + substrate;
    if denom == 0.0 {
        return 0.0;
    }
    v_max_app * substrate / denom
}

// ── Langmuir adsorption isotherm ──────────────────────────────────

/// Surface coverage θ = K·P / (1 + K·P)  (Langmuir)
/// K — adsorption equilibrium constant, P — partial pressure or concentration
pub fn langmuir_coverage(k: f32, p: f32) -> f32 {
    let kp = k * p;
    let denom = 1.0 + kp;
    if denom == 0.0 {
        return 0.0;
    }
    kp / denom
}

/// Multi-component competitive Langmuir: θ_i = K_i·P_i / (1 + Σ K_j·P_j)
pub fn langmuir_competitive(ki: f32, pi: f32, sum_kp: f32) -> f32 {
    let denom = 1.0 + sum_kp;
    if denom == 0.0 {
        return 0.0;
    }
    ki * pi / denom
}

/// BET multi-layer adsorption (N = 1 → Langmuir):
/// θ = c·x / ((1-x)·(1 - x + c·x))  where x = P/P_sat
pub fn bet_coverage(c: f32, p: f32, p_sat: f32) -> f32 {
    if p_sat == 0.0 {
        return 0.0;
    }
    let x = p / p_sat;
    // BET is only physically meaningful for 0 < x < 1
    if x <= 0.0 || x >= 1.0 {
        return if x <= 0.0 { 0.0 } else { f32::INFINITY };
    }
    let one_minus_x = 1.0 - x;
    let denom = one_minus_x * (1.0 - x + c * x);
    if denom == 0.0 {
        return 0.0;
    }
    c * x / denom
}

// ── Langmuir-Hinshelwood mechanism ───────────────────────────────

/// Bimolecular LH rate: r = k·θ_A·θ_B·S_total
/// θ_A, θ_B are surface coverages of reactants A and B.
pub fn langmuir_hinshelwood(k: f32, theta_a: f32, theta_b: f32, active_sites: f32) -> f32 {
    k * theta_a * theta_b * active_sites
}

/// Eley-Rideal mechanism: one species adsorbed, one from gas phase
/// r = k·θ_A·P_B
pub fn eley_rideal(k: f32, theta_a: f32, p_b: f32) -> f32 {
    k * theta_a * p_b
}

// ── Temperature dependence ────────────────────────────────────────

/// Activation energy from two temperature measurements (Arrhenius for surface reactions)
pub fn surface_activation_energy(k1: f32, t1: f32, k2: f32, t2: f32) -> f32 {
    // Arrhenius: ln(k2/k1) = -Ea/R · (1/T2 - 1/T1)
    // Ea = -R · ln(k2/k1) / (1/T2 - 1/T1)
    const R: f32 = 8.314_f32; // J/(mol·K)
    if k1 <= 0.0 || k2 <= 0.0 || t1 == 0.0 || t2 == 0.0 {
        return 0.0;
    }
    let inv_diff = 1.0 / t2 - 1.0 / t1;
    if inv_diff == 0.0 {
        return 0.0;
    }
    -R * (k2 / k1).ln() / inv_diff
}

/// Turnover frequency (TOF) — reactions per active site per second
pub fn turnover_frequency(rate: f32, active_sites_per_m2: f32, surface_area: f32) -> f32 {
    let total_sites = active_sites_per_m2 * surface_area;
    if total_sites == 0.0 {
        return 0.0;
    }
    rate / total_sites
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn michaelis_menten_half_max() {
        // At [S] = Km, rate should be Vmax/2
        let v = michaelis_menten(10.0, 5.0, 5.0);
        assert!((v - 5.0).abs() < 1e-5, "expected 5.0, got {v}");
    }

    #[test]
    fn michaelis_menten_zero_substrate() {
        let v = michaelis_menten(10.0, 5.0, 0.0);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn lineweaver_burk_roundtrip() {
        let v_max = 12.0_f32;
        let km = 3.0_f32;
        // y-intercept of Lineweaver-Burk = 1/Vmax
        let y_int = 1.0 / v_max;
        // x-intercept = -1/Km
        let x_int = -1.0 / km;
        assert!((vmax_from_lineweaver(y_int) - v_max).abs() < 1e-4);
        assert!((km_from_lineweaver(x_int) - km).abs() < 1e-4);
    }

    #[test]
    fn hill_kinetics_n1_equals_mm() {
        // Hill with n=1 should match Michaelis-Menten
        let v_max = 8.0;
        let k_half = 2.0;
        let s = 4.0;
        let hill = hill_kinetics(v_max, k_half, 1.0, s);
        let mm = michaelis_menten(v_max, k_half, s);
        assert!((hill - mm).abs() < 1e-5, "hill={hill}, mm={mm}");
    }

    #[test]
    fn hill_coefficient_symmetric() {
        // For n=2, K½ = 1.0: s at 10% → solve [S]^2/(1+[S]^2)=0.1 → [S]=sqrt(1/9)≈0.3333
        // s at 90% → solve [S]^2/(1+[S]^2)=0.9 → [S]=sqrt(9)=3.0
        let s10 = (1.0_f32 / 9.0_f32).sqrt();
        let s90 = 3.0_f32;
        let n = hill_coefficient(s10, s90);
        assert!((n - 2.0).abs() < 1e-4, "expected n=2, got {n}");
    }

    #[test]
    fn competitive_inhibition_no_inhibitor() {
        // With inhibitor=0, result should equal plain Michaelis-Menten
        let mm = michaelis_menten(10.0, 4.0, 2.0);
        let ci = competitive_inhibition_rate(10.0, 4.0, 2.0, 0.0, 1.0);
        assert!((mm - ci).abs() < 1e-5);
    }

    #[test]
    fn noncompetitive_reduces_vmax() {
        // High inhibitor should approach zero rate
        let rate = noncompetitive_inhibition_rate(10.0, 1.0, 100.0, 1e6, 1.0);
        assert!(rate < 1e-3, "rate should be near zero: {rate}");
    }

    #[test]
    fn uncompetitive_inhibition_symmetry() {
        // Km and Vmax both halved when [I]=Ki → ratio Vmax_app/Km_app unchanged
        // rate with [I]=Ki and [S]=Km_app should be Vmax_app/2
        let v_max = 10.0_f32;
        let km = 4.0_f32;
        let ki = 2.0_f32;
        let inhibitor = ki; // alpha_prime = 2
        // Km_app = km/2 = 2, Vmax_app = 5
        // At [S] = Km_app = 2: rate = Vmax_app/2 = 2.5
        let rate = uncompetitive_inhibition_rate(v_max, km, 2.0, inhibitor, ki);
        assert!((rate - 2.5).abs() < 1e-4, "expected 2.5, got {rate}");
    }

    #[test]
    fn langmuir_coverage_saturation() {
        // Very high P → θ → 1
        let theta = langmuir_coverage(1e6, 1e6);
        assert!(theta > 0.999, "coverage should saturate near 1: {theta}");
    }

    #[test]
    fn langmuir_coverage_zero_pressure() {
        assert_eq!(langmuir_coverage(1.0, 0.0), 0.0);
    }

    #[test]
    fn langmuir_competitive_sum() {
        // θ_i = K_i·P_i / (1 + ΣK_j·P_j); for a single component sum_kp = Ki*Pi
        let ki = 2.0_f32;
        let pi = 3.0_f32;
        let sum_kp = ki * pi; // only one component
        let theta = langmuir_competitive(ki, pi, sum_kp);
        let reference = langmuir_coverage(ki, pi);
        assert!((theta - reference).abs() < 1e-5, "theta={theta}, ref={reference}");
    }

    #[test]
    fn bet_coverage_low_pressure_approaches_langmuir() {
        // For small x and large c, BET ≈ c·x / (1·1) ≈ c·x (linear regime)
        let c = 100.0_f32;
        let p_sat = 1.0_f32;
        let p = 0.001_f32;
        let theta = bet_coverage(c, p, p_sat);
        assert!(theta > 0.0 && theta < 1.0, "theta={theta}");
    }

    #[test]
    fn langmuir_hinshelwood_linearity() {
        let r = langmuir_hinshelwood(2.0, 0.5, 0.3, 1e18);
        assert!((r - 2.0 * 0.5 * 0.3 * 1e18).abs() < 1.0);
    }

    #[test]
    fn eley_rideal_basic() {
        let r = eley_rideal(3.0, 0.4, 2.0);
        assert!((r - 2.4).abs() < 1e-5);
    }

    #[test]
    fn surface_activation_energy_known() {
        // If k doubles from T1=300 to T2=310K, compute Ea
        // Ea = -R * ln(2) / (1/310 - 1/300)
        const R: f32 = 8.314;
        let t1 = 300.0_f32;
        let t2 = 310.0_f32;
        let k1 = 1.0_f32;
        let k2 = 2.0_f32;
        let ea = surface_activation_energy(k1, t1, k2, t2);
        let expected = -R * (2.0_f32).ln() / (1.0 / t2 - 1.0 / t1);
        assert!((ea - expected).abs() < 0.1, "ea={ea}, expected={expected}");
    }

    #[test]
    fn turnover_frequency_basic() {
        // rate=1e-3 mol/s, 1e18 sites/m2, 0.01 m2 → total_sites=1e16
        let tof = turnover_frequency(1e-3, 1e18, 0.01);
        assert!((tof - 1e-3 / 1e16).abs() < 1e-25, "tof={tof}");
    }

    #[test]
    fn turnover_frequency_zero_sites() {
        assert_eq!(turnover_frequency(1.0, 0.0, 1.0), 0.0);
    }
}
