//! Rune bindings for the numerics laws.
//!
//! Exposed to scripts under `eustress::realism::numerics::*`.
//!
//! Two kernel families back these bindings, with **different** scalar widths:
//!
//! - `crate::realism::numerics::interpolation` is an **f32** kernel, so those
//!   wrappers cast `f64 → f32` on the way in and `f32 → f64` on the way out
//!   (Rune works in f64).
//! - `crate::realism::numerics::statistics::distributions` is an **f64-native**
//!   kernel, so those wrappers pass `f64` straight through with no cast. The
//!   Poisson functions take an integer count `k: u64`; the wrappers accept
//!   `i64` from Rune and cast `as u64`.
//!
//! Only pure scalar→scalar helpers are bound here. Spline build/eval, the table
//! lookups, the LCG, and Box–Muller operate on slices, mutate state, or return
//! tuples, so they are not exposed to Rune.

use rune::{ContextError, Module};
use crate::realism::numerics::interpolation;
use crate::realism::numerics::statistics::distributions;

// ── interpolation (f32 kernel → f64 wrappers) ────────────────────

#[rune::function]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    interpolation::lerp(a as f32, b as f32, t as f32) as f64
}

#[rune::function]
fn inv_lerp(a: f64, b: f64, value: f64) -> f64 {
    interpolation::inv_lerp(a as f32, b as f32, value as f32) as f64
}

// NOTE: bilinear (6 args) omitted — Rune 0.14 binds at most 5 args.

#[rune::function]
fn cubic_hermite(p0: f64, m0: f64, p1: f64, m1: f64, t: f64) -> f64 {
    interpolation::cubic_hermite(p0 as f32, m0 as f32, p1 as f32, m1 as f32, t as f32) as f64
}

#[rune::function]
fn smoothstep(t: f64) -> f64 {
    interpolation::smoothstep(t as f32) as f64
}

#[rune::function]
fn smootherstep(t: f64) -> f64 {
    interpolation::smootherstep(t as f32) as f64
}

// ── distributions (f64-native kernel → pass-through) ─────────────

#[rune::function]
fn erf(x: f64) -> f64 {
    distributions::erf(x)
}

#[rune::function]
fn erfc(x: f64) -> f64 {
    distributions::erfc(x)
}

#[rune::function]
fn gaussian_pdf(x: f64, mu: f64, sigma: f64) -> f64 {
    distributions::gaussian_pdf(x, mu, sigma)
}

#[rune::function]
fn gaussian_cdf(x: f64, mu: f64, sigma: f64) -> f64 {
    distributions::gaussian_cdf(x, mu, sigma)
}

#[rune::function]
fn gaussian_quantile(p: f64, mu: f64, sigma: f64) -> f64 {
    distributions::gaussian_quantile(p, mu, sigma)
}

#[rune::function]
fn uniform_pdf(x: f64, a: f64, b: f64) -> f64 {
    distributions::uniform_pdf(x, a, b)
}

#[rune::function]
fn exponential_pdf(x: f64, lambda: f64) -> f64 {
    distributions::exponential_pdf(x, lambda)
}

#[rune::function]
fn exponential_cdf(x: f64, lambda: f64) -> f64 {
    distributions::exponential_cdf(x, lambda)
}

#[rune::function]
fn weibull_pdf(x: f64, k: f64, lambda: f64) -> f64 {
    distributions::weibull_pdf(x, k, lambda)
}

#[rune::function]
fn weibull_cdf(x: f64, k: f64, lambda: f64) -> f64 {
    distributions::weibull_cdf(x, k, lambda)
}

#[rune::function]
fn poisson_pmf(k: i64, lambda: f64) -> f64 {
    distributions::poisson_pmf(k as u64, lambda)
}

#[rune::function]
fn poisson_cdf(k: i64, lambda: f64) -> f64 {
    distributions::poisson_cdf(k as u64, lambda)
}

/// Build the `eustress::realism::numerics` Rune module.
pub fn create_module() -> Result<Module, ContextError> {
    let mut m = Module::with_crate_item("eustress", ["realism", "numerics"])?;
    m.function_meta(lerp)?;
    m.function_meta(inv_lerp)?;
    m.function_meta(cubic_hermite)?;
    m.function_meta(smoothstep)?;
    m.function_meta(smootherstep)?;
    m.function_meta(erf)?;
    m.function_meta(erfc)?;
    m.function_meta(gaussian_pdf)?;
    m.function_meta(gaussian_cdf)?;
    m.function_meta(gaussian_quantile)?;
    m.function_meta(uniform_pdf)?;
    m.function_meta(exponential_pdf)?;
    m.function_meta(exponential_cdf)?;
    m.function_meta(weibull_pdf)?;
    m.function_meta(weibull_cdf)?;
    m.function_meta(poisson_pmf)?;
    m.function_meta(poisson_cdf)?;
    Ok(m)
}
