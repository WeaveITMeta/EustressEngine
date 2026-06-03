//! Population dynamics — logistic, Lotka-Volterra, SIR epidemic.

/// Exponential (Malthusian) growth: N(t) = N0 * exp(r * t).
pub fn exponential_growth(n0: f32, rate: f32, time: f32) -> f32 {
    n0 * (rate * time).exp()
}

/// Logistic growth closed form: K / (1 + ((K - N0) / N0) * exp(-r * t)).
///
/// Falls back gracefully when the initial population is non-positive
/// (no individuals means no growth).
pub fn logistic_growth(n0: f32, rate: f32, carrying_capacity: f32, time: f32) -> f32 {
    if n0 <= 0.0 {
        return 0.0;
    }
    let a = (carrying_capacity - n0) / n0;
    carrying_capacity / (1.0 + a * (-rate * time).exp())
}

/// Instantaneous logistic rate of change: dN/dt = r * N * (1 - N / K).
pub fn logistic_derivative(n: f32, rate: f32, carrying_capacity: f32) -> f32 {
    if carrying_capacity == 0.0 {
        return 0.0;
    }
    rate * n * (1.0 - n / carrying_capacity)
}

/// One forward-Euler step of the Lotka-Volterra predator-prey system.
///
/// dPrey = (alpha * prey - beta * prey * pred) * dt
/// dPred = (delta * prey * pred - gamma * pred) * dt
///
/// Both populations are clamped to be non-negative.
pub fn lotka_volterra_step(
    prey: f32,
    predator: f32,
    alpha: f32,
    beta: f32,
    gamma: f32,
    delta: f32,
    dt: f32,
) -> (f32, f32) {
    let d_prey = (alpha * prey - beta * prey * predator) * dt;
    let d_pred = (delta * prey * predator - gamma * predator) * dt;
    let new_prey = (prey + d_prey).max(0.0);
    let new_pred = (predator + d_pred).max(0.0);
    (new_prey, new_pred)
}

/// One forward-Euler step of the SIR epidemic model.
///
/// dS = -beta * S * I
/// dI =  beta * S * I - gamma * I
/// dR =  gamma * I
///
/// All compartments are clamped to be non-negative.
pub fn sir_step(s: f32, i: f32, r: f32, beta: f32, gamma: f32, dt: f32) -> (f32, f32, f32) {
    let new_infections = beta * s * i;
    let recoveries = gamma * i;
    let new_s = (s - new_infections * dt).max(0.0);
    let new_i = (i + (new_infections - recoveries) * dt).max(0.0);
    let new_r = (r + recoveries * dt).max(0.0);
    (new_s, new_i, new_r)
}

/// Basic reproduction number R0 = beta / gamma for a normalized SIR model
/// where the initial susceptible fraction S is approximately 1.
pub fn basic_reproduction_number(beta: f32, gamma: f32) -> f32 {
    if gamma == 0.0 {
        return f32::INFINITY;
    }
    beta / gamma
}

/// Herd immunity threshold: 1 - 1 / R0 (fraction that must be immune).
pub fn herd_immunity_threshold(r0: f32) -> f32 {
    if r0 == 0.0 {
        return 0.0;
    }
    1.0 - 1.0 / r0
}

/// Population doubling time under exponential growth: ln(2) / r.
pub fn doubling_time_population(rate: f32) -> f32 {
    if rate == 0.0 {
        return f32::INFINITY;
    }
    core::f32::consts::LN_2 / rate
}

/// Carrying capacity inferred from a logistic equilibrium.
///
/// At equilibrium dN/dt = 0 with N > 0, so N = K. The growth rate does not
/// affect the equilibrium value; it is accepted for API symmetry.
pub fn carrying_capacity_from_equilibrium(_rate: f32, equilibrium_n: f32) -> f32 {
    equilibrium_n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logistic_approaches_carrying_capacity() {
        let k = 1000.0;
        let n = logistic_growth(10.0, 0.5, k, 1000.0);
        // After a very long time the population should be essentially K.
        assert!((n - k).abs() < 1.0, "expected ~{k}, got {n}");
    }

    #[test]
    fn herd_immunity_for_r0_four() {
        // R0 = 4 -> threshold = 1 - 1/4 = 0.75.
        let t = herd_immunity_threshold(4.0);
        assert!((t - 0.75).abs() < 1e-6, "expected 0.75, got {t}");
    }

    #[test]
    fn sir_conserves_total_population() {
        let (s, i, r) = sir_step(0.99, 0.01, 0.0, 0.4, 0.1, 0.5);
        let total = s + i + r;
        assert!((total - 1.0).abs() < 1e-5, "total drifted to {total}");
    }

    #[test]
    fn lotka_volterra_clamps_non_negative() {
        // Huge predation pressure should not push prey below zero.
        let (prey, _pred) = lotka_volterra_step(1.0, 100.0, 0.1, 10.0, 0.1, 0.01, 5.0);
        assert!(prey >= 0.0, "prey went negative: {prey}");
    }
}
