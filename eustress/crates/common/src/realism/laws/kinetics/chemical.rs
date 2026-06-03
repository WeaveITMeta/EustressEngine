//! Chemical kinetics: rate laws, Arrhenius, equilibrium, acid-base.

const R: f32 = 8.314; // J/(mol·K)

// ── Arrhenius equation ────────────────────────────────────────────

/// Rate constant: k(T) = A·exp(-E_a / (R·T))
/// A — pre-exponential factor (same units as k), E_a [J/mol], T [K]
pub fn arrhenius_rate(a: f32, e_a: f32, t_kelvin: f32) -> f32 {
    a * (-e_a / (R * t_kelvin)).exp()
}

/// Activation energy from two rate measurements:
/// E_a = R·T1·T2/(T2-T1) · ln(k2/k1)
pub fn activation_energy(k1: f32, t1: f32, k2: f32, t2: f32) -> f32 {
    R * t1 * t2 / (t2 - t1) * (k2 / k1).ln()
}

/// Rate constant at T2 given k at T1:
/// k2 = k1·exp(E_a/R · (1/T1 - 1/T2))
pub fn rate_constant_at_t(k1: f32, e_a: f32, t1: f32, t2: f32) -> f32 {
    k1 * ((e_a / R) * (1.0 / t1 - 1.0 / t2)).exp()
}

// ── Rate laws ────────────────────────────────────────────────────

/// Reaction rate: r = k · ∏ cᵢ^nᵢ
/// concentrations and orders must have the same length.
pub fn reaction_rate(k: f32, concentrations: &[f32], orders: &[f32]) -> f32 {
    let product: f32 = concentrations
        .iter()
        .zip(orders.iter())
        .map(|(&c, &n)| c.powf(n))
        .product();
    k * product
}

/// First-order: c(t) = c₀·e^(-k·t)
pub fn first_order_concentration(c0: f32, k: f32, t: f32) -> f32 {
    c0 * (-k * t).exp()
}

/// Second-order (single reactant): 1/c(t) = 1/c₀ + k·t
pub fn second_order_concentration(c0: f32, k: f32, t: f32) -> f32 {
    1.0 / (1.0 / c0 + k * t)
}

/// Half-life, first-order: t½ = ln(2)/k
pub fn half_life_first_order(k: f32) -> f32 {
    2.0_f32.ln() / k
}

/// Half-life, second-order: t½ = 1/(k·c₀)
pub fn half_life_second_order(k: f32, c0: f32) -> f32 {
    1.0 / (k * c0)
}

// ── Equilibrium ──────────────────────────────────────────────────

/// Equilibrium constant from ΔG°: Kₑq = exp(-ΔG°/(R·T))
pub fn equilibrium_constant(delta_g_std: f32, t_kelvin: f32) -> f32 {
    (-delta_g_std / (R * t_kelvin)).exp()
}

/// ΔG° from Kₑq: ΔG° = -R·T·ln(Kₑq)
pub fn standard_gibbs_from_k(k_eq: f32, t_kelvin: f32) -> f32 {
    -R * t_kelvin * k_eq.ln()
}

/// Van't Hoff: K(T2) = K(T1)·exp(ΔH°/R · (1/T1 - 1/T2))
pub fn equilibrium_constant_at_t(k1: f32, delta_h: f32, t1: f32, t2: f32) -> f32 {
    k1 * ((delta_h / R) * (1.0 / t1 - 1.0 / t2)).exp()
}

/// Reaction quotient Q from concentrations (numerator = products, denominator = reactants).
/// stoich_products and stoich_reactants are the stoichiometric exponents.
pub fn reaction_quotient(
    product_concs: &[f32],
    stoich_products: &[f32],
    reactant_concs: &[f32],
    stoich_reactants: &[f32],
) -> f32 {
    let numerator: f32 = product_concs
        .iter()
        .zip(stoich_products.iter())
        .map(|(&c, &s)| c.powf(s))
        .product();
    let denominator: f32 = reactant_concs
        .iter()
        .zip(stoich_reactants.iter())
        .map(|(&c, &s)| c.powf(s))
        .product();
    numerator / denominator
}

/// Direction of spontaneous reaction: returns ΔG = ΔG° + R·T·ln(Q/K)
/// Negative → forward, positive → reverse, zero → equilibrium.
pub fn reaction_direction(delta_g_std: f32, k_eq: f32, q: f32, t: f32) -> f32 {
    delta_g_std + R * t * (q / k_eq).ln()
}

// ── Acid-base ────────────────────────────────────────────────────

/// pH from [H⁺] concentration [mol/L]
pub fn ph_from_h_concentration(h_conc: f32) -> f32 {
    -h_conc.log10()
}

/// [H⁺] from pH
pub fn h_concentration_from_ph(ph: f32) -> f32 {
    10.0_f32.powf(-ph)
}

/// pOH from [OH⁻]
pub fn poh_from_oh_concentration(oh_conc: f32) -> f32 {
    -oh_conc.log10()
}

/// Water equilibrium at 25°C: pH + pOH = 14
pub fn poh_from_ph(ph: f32) -> f32 {
    14.0 - ph
}

/// Henderson-Hasselbalch equation: pH = pKa + log₁₀([A⁻]/[HA])
pub fn henderson_hasselbalch(pka: f32, c_base: f32, c_acid: f32) -> f32 {
    pka + (c_base / c_acid).log10()
}

/// Degree of dissociation for weak acid: Ka = α²·c / (1-α)
/// Quadratic approximation for small Ka/c
pub fn weak_acid_dissociation(ka: f32, c_acid: f32) -> f32 {
    // Ka = α²·c / (1-α)  =>  Ka·(1-α) = α²·c
    // α²·c + Ka·α - Ka = 0
    // α = (-Ka + sqrt(Ka² + 4·c·Ka)) / (2·c)
    let discriminant = ka * ka + 4.0 * c_acid * ka;
    (-ka + discriminant.sqrt()) / (2.0 * c_acid)
}

/// pKa from Ka: pKa = -log₁₀(Ka)
pub fn pka(ka: f32) -> f32 {
    -ka.log10()
}

// ── Enthalpy of reaction ──────────────────────────────────────────

/// Standard enthalpy of reaction: ΔH°_rxn = Σ ΔH°_f(products) - Σ ΔH°_f(reactants)
pub fn enthalpy_of_reaction(
    h_products: &[f32],
    stoich_products: &[f32],
    h_reactants: &[f32],
    stoich_reactants: &[f32],
) -> f32 {
    let sum_products: f32 = h_products
        .iter()
        .zip(stoich_products.iter())
        .map(|(&h, &s)| h * s)
        .sum();
    let sum_reactants: f32 = h_reactants
        .iter()
        .zip(stoich_reactants.iter())
        .map(|(&h, &s)| h * s)
        .sum();
    sum_products - sum_reactants
}

/// Kirchhoff's law (temperature correction): ΔH(T2) = ΔH(T1) + ΔCp·(T2-T1)
pub fn enthalpy_at_temperature(delta_h_ref: f32, delta_cp: f32, t_ref: f32, t: f32) -> f32 {
    delta_h_ref + delta_cp * (t - t_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrhenius_rate() {
        // At very high T, exp(-Ea/RT) -> 1, so k -> A
        let k = arrhenius_rate(1.0e10, 0.0, 300.0);
        assert!((k - 1.0e10).abs() < 1.0);
    }

    #[test]
    fn test_activation_energy_roundtrip() {
        let e_a = 50_000.0_f32;
        let t1 = 300.0_f32;
        let t2 = 400.0_f32;
        let a = 1.0e12_f32;
        let k1 = arrhenius_rate(a, e_a, t1);
        let k2 = arrhenius_rate(a, e_a, t2);
        let e_a_calc = activation_energy(k1, t1, k2, t2);
        assert!((e_a_calc - e_a).abs() / e_a < 1e-4);
    }

    #[test]
    fn test_first_order_half_life() {
        let k = 0.1_f32;
        let t_half = half_life_first_order(k);
        let c0 = 1.0_f32;
        let c = first_order_concentration(c0, k, t_half);
        assert!((c - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_second_order_half_life() {
        let k = 0.1_f32;
        let c0 = 2.0_f32;
        let t_half = half_life_second_order(k, c0);
        let c = second_order_concentration(c0, k, t_half);
        assert!((c - c0 / 2.0).abs() < 1e-5);
    }

    #[test]
    fn test_equilibrium_gibbs_roundtrip() {
        let delta_g = -5000.0_f32;
        let t = 298.0_f32;
        let k_eq = equilibrium_constant(delta_g, t);
        let delta_g_back = standard_gibbs_from_k(k_eq, t);
        assert!((delta_g_back - delta_g).abs() / delta_g.abs() < 1e-5);
    }

    #[test]
    fn test_ph_roundtrip() {
        let h = 1.0e-7_f32;
        let ph = ph_from_h_concentration(h);
        assert!((ph - 7.0).abs() < 1e-5);
        let h_back = h_concentration_from_ph(ph);
        assert!((h_back - h).abs() / h < 1e-4);
    }

    #[test]
    fn test_poh_from_ph() {
        assert!((poh_from_ph(7.0) - 7.0).abs() < 1e-5);
        assert!((poh_from_ph(3.0) - 11.0).abs() < 1e-5);
    }

    #[test]
    fn test_henderson_hasselbalch() {
        // Equal concentrations => pH = pKa
        let pka_val = 4.75_f32;
        let ph = henderson_hasselbalch(pka_val, 1.0, 1.0);
        assert!((ph - pka_val).abs() < 1e-5);
    }

    #[test]
    fn test_weak_acid_dissociation_range() {
        let alpha = weak_acid_dissociation(1.8e-5, 0.1);
        assert!(alpha > 0.0 && alpha < 1.0);
        // For acetic acid at 0.1 M, alpha ~ 0.0134
        assert!((alpha - 0.01342).abs() < 1e-4);
    }

    #[test]
    fn test_enthalpy_of_reaction() {
        // H2 + 0.5 O2 -> H2O: ΔH°f(H2O) = -241.8, reactants both 0
        let delta_h = enthalpy_of_reaction(
            &[-241.8],
            &[1.0],
            &[0.0, 0.0],
            &[1.0, 0.5],
        );
        assert!((delta_h - (-241.8)).abs() < 1e-3);
    }

    #[test]
    fn test_enthalpy_at_temperature() {
        let result = enthalpy_at_temperature(-100.0, 2.0, 298.0, 398.0);
        assert!((result - (-100.0 + 200.0)).abs() < 1e-3);
    }
}
