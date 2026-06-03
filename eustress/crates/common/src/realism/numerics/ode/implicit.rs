//! BDF-1 (= Backward Euler) and BDF-2 implicit integrators.
//! These are A-stable methods that handle stiff systems where explicit methods
//! require impossibly small time steps for stability.
//!
//! BDF-1: y_{n+1} - y_n = h·f(t_{n+1}, y_{n+1})
//! BDF-2: (3/2)·y_{n+1} - 2·y_n + (1/2)·y_{n-1} = h·f(t_{n+1}, y_{n+1})
//!
//! # Stiffness context (ARC-1 nuclear kinetics)
//!
//! The point-kinetics equations that govern ARC-1's neutron population have
//! a stiff eigenvalue spectrum: prompt-neutron decay runs at λ/Λ ≈ 3 200 s⁻¹
//! while the slowest delayed-neutron group decays at ≈ 0.08 s⁻¹ — a ratio of
//! ~40 000.  An explicit (forward-Euler / RK4) integrator would need step sizes
//! of Δt < 1/3 200 s ≈ 0.3 ms just to remain stable, forcing ~3 000 steps per
//! simulated second.  BDF-1 is A-stable: all eigenvalues with Re(λ) < 0 are
//! damped regardless of step size, so the nuclear plugin can safely step at
//! Δt = 1/60 s (the game-physics tick) without numerical blow-up.

/// BDF-1 (= Backward Euler) via Newton iteration.
///
/// Solves the implicit equation
///   G(y_new) = y_new - y - dt·f(y_new) = 0
/// using Newton's method:
///   y_new ← y_new - G(y_new) / G'(y_new)
///         = y_new - (y_new - y - dt·f(y_new)) / (1 - dt·df(y_new))
///
/// # Arguments
/// * `y`        – current state y_n
/// * `dt`       – time step h
/// * `f`        – the right-hand side evaluated at the new iterate f(y_new)
/// * `df`       – the Jacobian (scalar df/dy) evaluated at the new iterate
/// * `max_iter` – Newton iteration cap (8–20 is typical)
/// * `tol`      – convergence tolerance on |Δy_new|
///
/// Returns y_{n+1}.
///
/// # Notes
/// ARC-1 nuclear kinetics uses this form because the single dominant stiff
/// eigenvalue (prompt-neutron decay, λ/Λ ≈ 3 200 s⁻¹) means the Jacobian
/// is cheap to evaluate analytically and Newton converges in 2–3 iterations
/// per step.
pub fn bdf1_newton<F, J>(
    y: f32,
    dt: f32,
    f: F,
    df: J,
    max_iter: u32,
    tol: f32,
) -> f32
where
    F: Fn(f32) -> f32,
    J: Fn(f32) -> f32,
{
    let mut y_new = y; // initial guess: explicit Euler predictor would be y + dt*f(y), but plain y is safe
    for _ in 0..max_iter {
        let residual = y_new - y - dt * f(y_new);
        let deriv = 1.0 - dt * df(y_new);
        // Guard against a near-zero denominator; if the Jacobian is degenerate
        // fall back to a no-op update for this iteration.
        if deriv.abs() < f32::EPSILON {
            break;
        }
        let delta = residual / deriv;
        y_new -= delta;
        if delta.abs() <= tol {
            break;
        }
    }
    y_new
}

/// BDF-1 (= Backward Euler) via fixed-point iteration.
///
/// Rearranges the implicit equation as a fixed-point problem:
///   y_new = y + dt·f(y_new)
/// and iterates until convergence.  No Jacobian is required, but convergence
/// is only guaranteed when |dt·(df/dy)| < 1 — i.e. for mildly stiff systems
/// or when dt is small.  For strongly stiff problems prefer [`bdf1_newton`].
///
/// # Arguments
/// * `y`        – current state y_n
/// * `dt`       – time step h
/// * `f`        – the right-hand side f(y_new)
/// * `max_iter` – iteration cap
/// * `tol`      – convergence tolerance on |Δy_new|
///
/// Returns y_{n+1}.
pub fn bdf1_fixed_point<F: Fn(f32) -> f32>(
    y: f32,
    dt: f32,
    f: F,
    max_iter: u32,
    tol: f32,
) -> f32 {
    let mut y_new = y;
    for _ in 0..max_iter {
        let y_next = y + dt * f(y_new);
        let delta = (y_next - y_new).abs();
        y_new = y_next;
        if delta <= tol {
            break;
        }
    }
    y_new
}

/// BDF-2 implicit integrator via Newton iteration.
///
/// BDF-2 is second-order accurate and A-stable.  It uses the two most recent
/// values to form the predictor:
///   (3/2)·y_{n+1} - 2·y_n + (1/2)·y_{n-1} = h·f(t_{n+1}, y_{n+1})
///
/// Rearranged as a Newton residual:
///   G(y_new) = (3/2)·y_new - 2·y_n + (1/2)·y_{n-1} - h·f(y_new) = 0
///   G'(y_new) = 3/2 - h·df(y_new)
///
/// # Arguments
/// * `y_n`   – state at the current step y_n
/// * `y_nm1` – state at the previous step y_{n-1}
/// * `dt`    – time step h (assumed constant; variable-step BDF-2 needs
///             extra coefficient scaling)
/// * `f`     – right-hand side evaluated at the new iterate
/// * `df`    – scalar Jacobian df/dy at the new iterate
/// * `max_iter` – Newton iteration cap
/// * `tol`      – convergence tolerance on |Δy_new|
///
/// Returns y_{n+1}.
///
/// # Bootstrapping
/// BDF-2 needs two prior values.  For the very first step (where y_{n-1} is
/// not yet available) use [`bdf1_newton`] to produce y_1 from y_0, then start
/// BDF-2 from step 2 onward.
pub fn bdf2_newton<F, J>(
    y_n: f32,
    y_nm1: f32,
    dt: f32,
    f: F,
    df: J,
    max_iter: u32,
    tol: f32,
) -> f32
where
    F: Fn(f32) -> f32,
    J: Fn(f32) -> f32,
{
    // Initial guess: simple linear extrapolation from the two known points.
    let mut y_new = 2.0 * y_n - y_nm1;
    for _ in 0..max_iter {
        let residual = 1.5 * y_new - 2.0 * y_n + 0.5 * y_nm1 - dt * f(y_new);
        let deriv = 1.5 - dt * df(y_new);
        if deriv.abs() < f32::EPSILON {
            break;
        }
        let delta = residual / deriv;
        y_new -= delta;
        if delta.abs() <= tol {
            break;
        }
    }
    y_new
}

/// BDF-1 (Backward Euler) for a state vector, fixed-point iteration,
/// component-wise convergence test.
///
/// For each component i the fixed-point update is:
///   y_new[i] = y[i] + dt · f(y_new)[i]
///
/// Convergence is declared when every component change is ≤ `tol`.
///
/// # Arguments
/// * `y`        – current state vector y_n (slice, any length)
/// * `dt`       – time step h
/// * `f`        – maps a state slice to the derivative vector; must return a
///               `Vec<f32>` of the same length as `y`
/// * `max_iter` – iteration cap
/// * `tol`      – per-component convergence tolerance
///
/// Returns y_{n+1} as a fresh `Vec<f32>`.
///
/// # Panics
/// Panics (debug only via `debug_assert`) if `f` returns a vector whose
/// length differs from `y.len()`.
pub fn bdf1_vec_fixed_point<F: Fn(&[f32]) -> Vec<f32>>(
    y: &[f32],
    dt: f32,
    f: F,
    max_iter: u32,
    tol: f32,
) -> Vec<f32> {
    let n = y.len();
    let mut y_new: Vec<f32> = y.to_vec();

    for _ in 0..max_iter {
        let dy = f(&y_new);
        debug_assert_eq!(dy.len(), n, "f must return the same length as y");

        let mut converged = true;
        for i in 0..n {
            let y_next_i = y[i] + dt * dy[i];
            let delta = (y_next_i - y_new[i]).abs();
            y_new[i] = y_next_i;
            if delta > tol {
                converged = false;
            }
        }
        if converged {
            break;
        }
    }
    y_new
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exponential decay: dy/dt = -k·y  =>  y(t) = y0·exp(-k·t)
    /// Exact solution at t=dt is y0·exp(-k·dt).
    /// BDF-1 is first-order so error ~ O(dt).
    #[test]
    fn bdf1_newton_exponential_decay() {
        let y0 = 1.0_f32;
        let k = 100.0_f32; // stiff: decay time constant 0.01 s
        // BDF-1 is FIRST-ORDER: single-step error is O((k*dt)^2/2). At k*dt=1
        // the error is ~36 %, so accuracy needs k*dt << 1. Use k*dt = 0.01.
        let dt = 0.0001_f32;
        // f(y_new) = -k * y_new,  df(y_new) = -k. Newton stays stable for any dt
        // (A-stable) — this test checks ACCURACY in the small-step regime.
        let y_new = bdf1_newton(y0, dt, |y| -k * y, |_| -k, 20, 1e-7);
        let exact = y0 * (-k * dt).exp();
        assert!(
            (y_new - exact).abs() / exact < 0.02,
            "bdf1_newton: y_new={y_new}, exact={exact}"
        );
    }

    #[test]
    fn bdf1_fixed_point_gentle_decay() {
        let y0 = 2.0_f32;
        let k = 0.5_f32; // mild stiffness
        let dt = 0.1_f32;
        let y_new = bdf1_fixed_point(y0, dt, |y| -k * y, 50, 1e-6);
        let exact = y0 * (-k * dt).exp();
        assert!(
            (y_new - exact).abs() / exact < 0.02,
            "bdf1_fixed_point: y_new={y_new}, exact={exact}"
        );
    }

    #[test]
    fn bdf2_newton_exponential_decay() {
        let y0 = 1.0_f32;
        let k = 50.0_f32;
        // BDF-2 is second-order, but a single step still needs k*dt modest for
        // the 2 % accuracy check. Use k*dt = 0.05.
        let dt = 0.001_f32;
        // Bootstrap: y_{-1} using exact solution at -dt
        let y_nm1 = y0 * (k * dt).exp(); // y at t = -dt
        let y_new = bdf2_newton(y0, y_nm1, dt, |y| -k * y, |_| -k, 20, 1e-7);
        let exact = y0 * (-k * dt).exp();
        assert!(
            (y_new - exact).abs() / exact < 0.02,
            "bdf2_newton: y_new={y_new}, exact={exact}"
        );
    }

    #[test]
    fn bdf1_vec_fixed_point_two_component() {
        // Two independent decays
        let y = vec![1.0_f32, 2.0_f32];
        let k = [0.3_f32, 0.7_f32];
        let dt = 0.05_f32;
        let y_new = bdf1_vec_fixed_point(&y, dt, |yv| vec![-k[0] * yv[0], -k[1] * yv[1]], 50, 1e-6);
        for i in 0..2 {
            let exact = y[i] * (-k[i] * dt).exp();
            assert!(
                (y_new[i] - exact).abs() / exact < 0.02,
                "component {i}: y_new={}, exact={exact}", y_new[i]
            );
        }
    }
}
