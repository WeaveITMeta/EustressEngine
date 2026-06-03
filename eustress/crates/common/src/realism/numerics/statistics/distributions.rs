//! Statistical distributions — fully deterministic, no_std-compatible.
//!
//! All functions are pure (no global state). The LCG state is passed by
//! `&mut u64` so callers own their RNG stream.
//!
//! References
//! ----------
//! - Abramowitz & Stegun, §7.1.26 (erf rational approximation)
//! - Peter Acklam, "An algorithm for computing the inverse normal cumulative
//!   distribution function" (2010) — rational approximation, ~1e-9 accuracy
//! - Park & Miller, "Random Number Generators: Good Ones Are Hard To Find",
//!   CACM 31(10), 1988 — Lehmer LCG multiplier 16807

// ── LCG ──────────────────────────────────────────────────────────────────────

/// Advance a Lehmer LCG (Park-Miller, modulus 2^31-1, multiplier 16807).
///
/// Returns a sample in `[0, 1)`.
///
/// # Panics
/// Never panics; `state` must be non-zero on first call — seed with any
/// non-zero value (e.g. 1).
#[inline]
pub fn lcg_next(state: &mut u64) -> f64 {
    const A: u64 = 16807;
    const M: u64 = 0x7fff_ffff; // 2^31 - 1
    *state = (A.wrapping_mul(*state)) % M;
    (*state as f64) / (M as f64)
}

// ── erf / erfc ───────────────────────────────────────────────────────────────

/// Error function via Abramowitz & Stegun 7.1.26.
///
/// Maximum absolute error ≈ 1.5 × 10⁻⁷.
#[inline]
pub fn erf(x: f64) -> f64 {
    let neg = x < 0.0;
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let poly = t
        * (0.254829592
            + t * (-0.284496736
                + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let result = 1.0 - poly * (-x * x).exp();
    if neg {
        -result
    } else {
        result
    }
}

/// Complementary error function: `erfc(x) = 1 - erf(x)`.
#[inline]
pub fn erfc(x: f64) -> f64 {
    1.0 - erf(x)
}

// ── factorial ─────────────────────────────────────────────────────────────────

/// Exact factorial for k ≤ 20 (lookup table); Stirling's approximation beyond.
///
/// Returns `f64` to avoid integer overflow. For k > 170 the result overflows
/// f64 to `+∞`; this is consistent with IEEE 754 behaviour and intentional.
pub fn factorial(k: u64) -> f64 {
    const TABLE: [f64; 21] = [
        1.0,
        1.0,
        2.0,
        6.0,
        24.0,
        120.0,
        720.0,
        5040.0,
        40320.0,
        362880.0,
        3628800.0,
        39916800.0,
        479001600.0,
        6227020800.0,
        87178291200.0,
        1307674368000.0,
        20922789888000.0,
        355687428096000.0,
        6402373705728000.0,
        121645100408832000.0,
        2432902008176640000.0,
    ];
    if k <= 20 {
        TABLE[k as usize]
    } else {
        // Stirling: n! ≈ sqrt(2πn) * (n/e)^n
        let n = k as f64;
        (2.0 * core::f64::consts::PI * n).sqrt() * (n / core::f64::consts::E).powf(n)
    }
}

// ── Gaussian ─────────────────────────────────────────────────────────────────

/// Gaussian (normal) probability density function.
///
/// `pdf(x; mu, sigma) = exp(-0.5*((x-mu)/sigma)^2) / (sigma * sqrt(2*pi))`
#[inline]
pub fn gaussian_pdf(x: f64, mu: f64, sigma: f64) -> f64 {
    debug_assert!(sigma > 0.0, "sigma must be positive");
    let z = (x - mu) / sigma;
    (-0.5 * z * z).exp() / (sigma * (2.0 * core::f64::consts::PI).sqrt())
}

/// Gaussian cumulative distribution function via `erf`.
///
/// `cdf(x; mu, sigma) = 0.5 * (1 + erf((x-mu) / (sigma*sqrt(2))))`
#[inline]
pub fn gaussian_cdf(x: f64, mu: f64, sigma: f64) -> f64 {
    debug_assert!(sigma > 0.0, "sigma must be positive");
    let z = (x - mu) / (sigma * core::f64::consts::SQRT_2);
    0.5 * (1.0 + erf(z))
}

/// Inverse Gaussian CDF (quantile / probit function).
///
/// Uses Peter Acklam's rational approximation — no iteration, ~1e-9 accuracy.
///
/// `p` must be in `(0, 1)`.  Values at the boundary return ±∞.
pub fn gaussian_quantile(p: f64, mu: f64, sigma: f64) -> f64 {
    debug_assert!(sigma > 0.0, "sigma must be positive");
    debug_assert!((0.0..=1.0).contains(&p), "p must be in [0, 1]");
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    // Acklam coefficients
    const A: [f64; 6] = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    let z = if p < P_LOW {
        // Lower tail
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        // Central region
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        // Upper tail
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };

    mu + sigma * z
}

// ── Box-Muller ────────────────────────────────────────────────────────────────

/// Generate two independent standard-normal samples from two uniform samples
/// in `(0, 1]` using the Box-Muller transform.
///
/// `u1` and `u2` must be drawn from a uniform distribution on `(0, 1]`.
/// Pass `lcg_next` output directly — the function guards against `u1 == 0.0`
/// to avoid `ln(0)`.
///
/// Returns `(z0, z1)` — both are standard normal (mu=0, sigma=1).
#[inline]
pub fn box_muller(u1: f64, u2: f64) -> (f64, f64) {
    // Guard against log(0)
    let u1 = if u1 == 0.0 { f64::EPSILON } else { u1 };
    let r = (-2.0 * u1.ln()).sqrt();
    let theta = 2.0 * core::f64::consts::PI * u2;
    (r * theta.cos(), r * theta.sin())
}

// ── Uniform ───────────────────────────────────────────────────────────────────

/// Uniform PDF on `[a, b]`.  Returns 0 outside the interval.
#[inline]
pub fn uniform_pdf(x: f64, a: f64, b: f64) -> f64 {
    debug_assert!(b > a, "b must be greater than a");
    if x >= a && x <= b {
        1.0 / (b - a)
    } else {
        0.0
    }
}

/// Uniform CDF on `[a, b]`.
#[inline]
pub fn uniform_cdf(x: f64, a: f64, b: f64) -> f64 {
    debug_assert!(b > a, "b must be greater than a");
    if x < a {
        0.0
    } else if x > b {
        1.0
    } else {
        (x - a) / (b - a)
    }
}

/// Sample from `Uniform(a, b)` using a `[0,1)` uniform input `u`.
#[inline]
pub fn uniform_sample(u: f64, a: f64, b: f64) -> f64 {
    debug_assert!(b > a, "b must be greater than a");
    a + u * (b - a)
}

// ── Exponential ───────────────────────────────────────────────────────────────

/// Exponential PDF: `lambda * exp(-lambda * x)` for `x >= 0`.
#[inline]
pub fn exponential_pdf(x: f64, lambda: f64) -> f64 {
    debug_assert!(lambda > 0.0, "lambda must be positive");
    if x < 0.0 {
        0.0
    } else {
        lambda * (-lambda * x).exp()
    }
}

/// Exponential CDF: `1 - exp(-lambda * x)` for `x >= 0`.
#[inline]
pub fn exponential_cdf(x: f64, lambda: f64) -> f64 {
    debug_assert!(lambda > 0.0, "lambda must be positive");
    if x < 0.0 {
        0.0
    } else {
        1.0 - (-lambda * x).exp()
    }
}

/// Sample from `Exponential(lambda)` via inverse CDF.
///
/// `u` is a `[0,1)` uniform sample (e.g. from `lcg_next`).
/// Uses `-ln(1 - u) / lambda`; guards against `u` being exactly 1.0.
#[inline]
pub fn exponential_sample(u: f64, lambda: f64) -> f64 {
    debug_assert!(lambda > 0.0, "lambda must be positive");
    let u = if u >= 1.0 { 1.0 - f64::EPSILON } else { u };
    -(1.0 - u).ln() / lambda
}

// ── Poisson ───────────────────────────────────────────────────────────────────

/// Poisson PMF: `P(X = k) = lambda^k * exp(-lambda) / k!`
///
/// Computed in log-space to avoid factorial overflow for large `lambda`.
pub fn poisson_pmf(k: u64, lambda: f64) -> f64 {
    debug_assert!(lambda > 0.0, "lambda must be positive");
    // log P = k*ln(lambda) - lambda - ln(k!)
    let log_p = (k as f64) * lambda.ln() - lambda - log_factorial(k);
    log_p.exp()
}

/// Poisson CDF: `P(X <= k)` — sum of PMF from 0 to k.
pub fn poisson_cdf(k: u64, lambda: f64) -> f64 {
    debug_assert!(lambda > 0.0, "lambda must be positive");
    (0..=k).fold(0.0, |acc, i| acc + poisson_pmf(i, lambda))
}

/// Natural log of k! computed accurately for all k.
fn log_factorial(k: u64) -> f64 {
    if k <= 20 {
        factorial(k).ln()
    } else {
        // Stirling in log-space: ln(n!) ≈ 0.5*ln(2πn) + n*ln(n) - n
        let n = k as f64;
        0.5 * (2.0 * core::f64::consts::PI * n).ln() + n * n.ln() - n
    }
}

// ── Weibull ───────────────────────────────────────────────────────────────────

/// Weibull PDF: `(k/lambda) * (x/lambda)^(k-1) * exp(-(x/lambda)^k)` for `x >= 0`.
///
/// - `k` — shape parameter (k > 0)
/// - `lambda` — scale parameter (lambda > 0)
#[inline]
pub fn weibull_pdf(x: f64, k: f64, lambda: f64) -> f64 {
    debug_assert!(k > 0.0, "k must be positive");
    debug_assert!(lambda > 0.0, "lambda must be positive");
    if x < 0.0 {
        return 0.0;
    }
    if x == 0.0 {
        // Limit at x=0: +∞ if k<1, 1 if k=1, 0 if k>1
        return if k < 1.0 {
            f64::INFINITY
        } else if (k - 1.0).abs() < f64::EPSILON {
            k / lambda
        } else {
            0.0
        };
    }
    let z = x / lambda;
    (k / lambda) * z.powf(k - 1.0) * (-z.powf(k)).exp()
}

/// Weibull CDF: `1 - exp(-(x/lambda)^k)` for `x >= 0`.
#[inline]
pub fn weibull_cdf(x: f64, k: f64, lambda: f64) -> f64 {
    debug_assert!(k > 0.0, "k must be positive");
    debug_assert!(lambda > 0.0, "lambda must be positive");
    if x < 0.0 {
        0.0
    } else {
        1.0 - (-(x / lambda).powf(k)).exp()
    }
}

/// Sample from `Weibull(k, lambda)` via inverse CDF.
///
/// `u` is a `[0,1)` uniform sample. Uses `lambda * (-ln(1-u))^(1/k)`.
#[inline]
pub fn weibull_sample(u: f64, k: f64, lambda: f64) -> f64 {
    debug_assert!(k > 0.0, "k must be positive");
    debug_assert!(lambda > 0.0, "lambda must be positive");
    let u = if u >= 1.0 { 1.0 - f64::EPSILON } else { u };
    lambda * (-(1.0 - u).ln()).powf(1.0 / k)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
        // `<=` so an exact match (diff == 0) passes even when tol == 0.0,
        // which is the case for exact integer factorials.
        assert!(
            (a - b).abs() <= tol,
            "{label}: |{a} - {b}| = {} > {tol}",
            (a - b).abs()
        );
    }

    /// Numerical integration via midpoint rule.
    fn integrate<F: Fn(f64) -> f64>(f: F, a: f64, b: f64, n: usize) -> f64 {
        let h = (b - a) / n as f64;
        (0..n)
            .map(|i| f(a + (i as f64 + 0.5) * h) * h)
            .sum()
    }

    // ── LCG ──────────────────────────────────────────────────────────────────

    #[test]
    fn lcg_range() {
        let mut state = 1u64;
        for _ in 0..10_000 {
            let v = lcg_next(&mut state);
            assert!(v >= 0.0 && v < 1.0, "lcg out of [0,1): {v}");
        }
    }

    #[test]
    fn lcg_not_constant() {
        let mut state = 1u64;
        let first = lcg_next(&mut state);
        let different = (0..20).any(|_| lcg_next(&mut state) != first);
        assert!(different, "LCG appears constant");
    }

    // ── erf / erfc ───────────────────────────────────────────────────────────

    #[test]
    fn erf_known_values() {
        assert_close(erf(0.0), 0.0, 1e-9, "erf(0)");
        // erf(1) ≈ 0.8427007929
        assert_close(erf(1.0), 0.8427007929, 2e-7, "erf(1)");
        // erf(∞) → 1
        assert_close(erf(5.0), 1.0, 1e-6, "erf(5)");
    }

    #[test]
    fn erf_odd_symmetry() {
        for x in [0.3, 0.7, 1.5, 2.5] {
            assert_close(erf(-x), -erf(x), 1e-12, "erf odd");
        }
    }

    #[test]
    fn erfc_complement() {
        for x in [-2.0, -0.5, 0.0, 0.5, 1.0, 2.0] {
            assert_close(erf(x) + erfc(x), 1.0, 1e-12, "erf + erfc = 1");
        }
    }

    // ── factorial ─────────────────────────────────────────────────────────────

    #[test]
    fn factorial_exact() {
        assert_close(factorial(0), 1.0, 0.0, "0!");
        assert_close(factorial(1), 1.0, 0.0, "1!");
        assert_close(factorial(5), 120.0, 0.0, "5!");
        assert_close(factorial(10), 3628800.0, 0.0, "10!");
        assert_close(factorial(20), 2432902008176640000.0, 1.0, "20!");
    }

    #[test]
    fn factorial_stirling_monotone() {
        // For k > 20 Stirling must still be monotonically increasing
        let vals: Vec<f64> = (20..=30).map(factorial).collect();
        for w in vals.windows(2) {
            assert!(w[1] > w[0], "factorial not monotone at Stirling boundary");
        }
    }

    // ── Gaussian ─────────────────────────────────────────────────────────────

    #[test]
    fn gaussian_pdf_integrates_to_one() {
        // ∫_{-10}^{10} pdf(x; 0, 1) dx ≈ 1
        let area = integrate(|x| gaussian_pdf(x, 0.0, 1.0), -10.0, 10.0, 100_000);
        assert_close(area, 1.0, 1e-5, "gaussian pdf area");
    }

    #[test]
    fn gaussian_cdf_known() {
        assert_close(gaussian_cdf(0.0, 0.0, 1.0), 0.5, 1e-6, "Phi(0)");
        // Phi(1.96) ≈ 0.975
        assert_close(gaussian_cdf(1.96, 0.0, 1.0), 0.975, 2e-4, "Phi(1.96)");
        // Phi(-∞) → 0
        assert_close(gaussian_cdf(-10.0, 0.0, 1.0), 0.0, 1e-6, "Phi(-10)");
        // Phi(+∞) → 1
        assert_close(gaussian_cdf(10.0, 0.0, 1.0), 1.0, 1e-6, "Phi(10)");
    }

    #[test]
    fn gaussian_cdf_monotone() {
        let mut prev = 0.0;
        for i in -50i32..=50 {
            let x = i as f64 * 0.1;
            let c = gaussian_cdf(x, 0.0, 1.0);
            assert!(c >= prev, "gaussian_cdf not monotone at x={x}");
            prev = c;
        }
    }

    #[test]
    fn gaussian_quantile_roundtrip() {
        let mu = 2.0;
        let sigma = 1.5;
        for p in [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99] {
            let x = gaussian_quantile(p, mu, sigma);
            let p2 = gaussian_cdf(x, mu, sigma);
            assert_close(p2, p, 5e-5, &format!("quantile roundtrip p={p}"));
        }
    }

    #[test]
    fn gaussian_quantile_boundaries() {
        assert_eq!(gaussian_quantile(0.0, 0.0, 1.0), f64::NEG_INFINITY);
        assert_eq!(gaussian_quantile(1.0, 0.0, 1.0), f64::INFINITY);
    }

    // ── Box-Muller ───────────────────────────────────────────────────────────

    #[test]
    fn box_muller_no_nan() {
        // u1 = 0 edge case is guarded
        let (z0, z1) = box_muller(0.0, 0.5);
        assert!(z0.is_finite(), "z0 NaN on u1=0");
        assert!(z1.is_finite(), "z1 NaN on u1=0");
    }

    #[test]
    fn box_muller_distribution() {
        // Generate ~10k samples and check mean ≈ 0, variance ≈ 1
        let mut state = 42u64;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let n = 10_000usize;
        for _ in 0..n / 2 {
            let u1 = lcg_next(&mut state);
            let u2 = lcg_next(&mut state);
            let (z0, z1) = box_muller(u1, u2);
            sum += z0 + z1;
            sum_sq += z0 * z0 + z1 * z1;
        }
        let mean = sum / n as f64;
        let var = sum_sq / n as f64 - mean * mean;
        assert!(mean.abs() < 0.05, "BM mean too far from 0: {mean}");
        assert!((var - 1.0).abs() < 0.05, "BM variance too far from 1: {var}");
    }

    // ── Uniform ───────────────────────────────────────────────────────────────

    #[test]
    fn uniform_pdf_values() {
        assert_close(uniform_pdf(0.5, 0.0, 1.0), 1.0, 1e-12, "U(0,1) pdf at 0.5");
        assert_close(uniform_pdf(-0.1, 0.0, 1.0), 0.0, 1e-12, "U(0,1) pdf out of range");
        assert_close(uniform_pdf(2.0, 0.0, 1.0), 0.0, 1e-12, "U(0,1) pdf above range");
    }

    #[test]
    fn uniform_cdf_values() {
        assert_close(uniform_cdf(0.0, 0.0, 2.0), 0.0, 1e-12, "U(0,2) cdf at 0");
        assert_close(uniform_cdf(1.0, 0.0, 2.0), 0.5, 1e-12, "U(0,2) cdf at 1");
        assert_close(uniform_cdf(2.0, 0.0, 2.0), 1.0, 1e-12, "U(0,2) cdf at 2");
        assert_close(uniform_cdf(-1.0, 0.0, 2.0), 0.0, 1e-12, "U(0,2) cdf below");
        assert_close(uniform_cdf(3.0, 0.0, 2.0), 1.0, 1e-12, "U(0,2) cdf above");
    }

    #[test]
    fn uniform_sample_range() {
        let mut state = 1u64;
        for _ in 0..1_000 {
            let s = uniform_sample(lcg_next(&mut state), -3.0, 7.0);
            assert!(s >= -3.0 && s < 7.0, "uniform_sample out of range: {s}");
        }
    }

    // ── Exponential ───────────────────────────────────────────────────────────

    #[test]
    fn exponential_pdf_integrates_to_one() {
        let area = integrate(|x| exponential_pdf(x, 2.0), 0.0, 30.0, 100_000);
        assert_close(area, 1.0, 1e-5, "exp pdf area");
    }

    #[test]
    fn exponential_cdf_known() {
        // CDF(0) = 0, CDF(∞) → 1
        assert_close(exponential_cdf(0.0, 1.0), 0.0, 1e-12, "exp cdf at 0");
        assert_close(exponential_cdf(100.0, 1.0), 1.0, 1e-12, "exp cdf at 100");
        // CDF(1/lambda) = 1 - 1/e
        assert_close(
            exponential_cdf(1.0, 1.0),
            1.0 - core::f64::consts::E.recip(),
            1e-10,
            "exp cdf at mean",
        );
    }

    #[test]
    fn exponential_sample_positive() {
        let mut state = 7u64;
        for _ in 0..1_000 {
            let s = exponential_sample(lcg_next(&mut state), 3.0);
            assert!(s >= 0.0, "exponential_sample negative: {s}");
        }
    }

    #[test]
    fn exponential_sample_roundtrip() {
        // sample(cdf(x)) ≈ x via: sample(-ln(1-cdf(x))/lambda) = x
        for x in [0.1, 0.5, 1.0, 2.0, 5.0] {
            let lambda = 1.5;
            let u = exponential_cdf(x, lambda);
            let x2 = exponential_sample(u, lambda);
            assert_close(x2, x, 1e-10, &format!("exp roundtrip x={x}"));
        }
    }

    // ── Poisson ───────────────────────────────────────────────────────────────

    #[test]
    fn poisson_pmf_sums_near_one() {
        // For lambda=5, sum k=0..50 should be ≈ 1
        let total: f64 = (0..=50u64).map(|k| poisson_pmf(k, 5.0)).sum();
        assert_close(total, 1.0, 1e-6, "Poisson pmf total");
    }

    #[test]
    fn poisson_pmf_known() {
        // P(X=0; lambda=1) = e^{-1} ≈ 0.3679
        assert_close(
            poisson_pmf(0, 1.0),
            core::f64::consts::E.recip(),
            1e-7,
            "P(0;1)",
        );
        // P(X=1; lambda=1) = e^{-1}
        assert_close(
            poisson_pmf(1, 1.0),
            core::f64::consts::E.recip(),
            1e-7,
            "P(1;1)",
        );
    }

    #[test]
    fn poisson_cdf_monotone() {
        let lambda = 3.0;
        let mut prev = 0.0;
        for k in 0u64..=20 {
            let c = poisson_cdf(k, lambda);
            assert!(c >= prev, "poisson_cdf not monotone at k={k}");
            prev = c;
        }
    }

    #[test]
    fn poisson_large_lambda() {
        // Large lambda — check no NaN or Inf from log-space computation
        let p = poisson_pmf(100, 100.0);
        assert!(p.is_finite() && p > 0.0, "poisson_pmf(100, 100) should be finite+positive: {p}");
    }

    // ── Weibull ───────────────────────────────────────────────────────────────

    #[test]
    fn weibull_pdf_integrates_to_one() {
        // k=1.5, lambda=2.0
        let area = integrate(|x| weibull_pdf(x, 1.5, 2.0), 0.0, 40.0, 200_000);
        assert_close(area, 1.0, 1e-4, "weibull pdf area");
    }

    #[test]
    fn weibull_cdf_known() {
        // CDF(0) = 0
        assert_close(weibull_cdf(0.0, 1.0, 1.0), 0.0, 1e-12, "W cdf at 0");
        // k=1 → Exponential(1/lambda); CDF(lambda) = 1 - 1/e
        assert_close(
            weibull_cdf(2.0, 1.0, 2.0),
            1.0 - core::f64::consts::E.recip(),
            1e-10,
            "W(1,2) cdf at x=2",
        );
        // CDF(large) → 1
        assert_close(weibull_cdf(1000.0, 2.0, 1.0), 1.0, 1e-10, "W cdf at large x");
    }

    #[test]
    fn weibull_cdf_monotone() {
        let k = 2.5;
        let lambda = 3.0;
        let mut prev = 0.0;
        for i in 0..=100 {
            let x = i as f64 * 0.2;
            let c = weibull_cdf(x, k, lambda);
            assert!(c >= prev - 1e-15, "weibull_cdf not monotone at x={x}");
            prev = c;
        }
    }

    #[test]
    fn weibull_sample_nonnegative() {
        let mut state = 99u64;
        for _ in 0..1_000 {
            let s = weibull_sample(lcg_next(&mut state), 2.0, 3.0);
            assert!(s >= 0.0, "weibull_sample negative: {s}");
        }
    }

    #[test]
    fn weibull_sample_roundtrip() {
        // sample(cdf(x)) ≈ x
        for x in [0.5, 1.0, 2.0, 4.0, 8.0] {
            let k = 2.0;
            let lambda = 3.0;
            let u = weibull_cdf(x, k, lambda);
            let x2 = weibull_sample(u, k, lambda);
            assert_close(x2, x, 1e-10, &format!("weibull roundtrip x={x}"));
        }
    }
}
