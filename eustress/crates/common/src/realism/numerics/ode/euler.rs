//! Forward and Backward Euler integrators.
//!
//! Forward Euler (explicit): y_{n+1} = y_n + h·f(t_n, y_n)
//! Backward Euler (implicit): y_{n+1} = y_n + h·f(t_{n+1}, y_{n+1})
//!   — solved via fixed-point iteration (suitable when f is not too stiff)
//!
//! # Choosing between forward and backward Euler
//!
//! **Forward Euler** is simple and cheap per step but conditionally stable.
//! For stiff systems (e.g. nuclear decay chains, thermal conduction, stiff
//! springs) the step size `dt` must satisfy `dt < 2 / |λ_max|` where `λ_max`
//! is the largest (in magnitude) eigenvalue of the Jacobian. Violating this
//! bound causes exponential blow-up.
//!
//! **Backward Euler** is A-stable: it remains bounded for any `dt` regardless
//! of stiffness. This makes it far more suitable for stiff systems because you
//! can take large time steps without instability — trading per-step cost
//! (a few fixed-point iterations) for the ability to stride over many forward
//! steps at once. Use backward Euler when:
//!
//! * The system has widely separated timescales (stiff ODEs).
//! * You need long simulation horizons without accumulating instability.
//! * The derivative `f` is a contraction mapping in `y` for the given `dt`
//!   (i.e. `|dt · ∂f/∂y| < 1`), so fixed-point iteration converges.

/// Advance one scalar state variable by `dt` using forward (explicit) Euler.
///
/// # Arguments
/// * `y`      — current state value
/// * `dy_dt`  — derivative evaluated at the current state
/// * `dt`     — time step
///
/// # Returns
/// The state value at `t + dt`.
#[inline]
pub fn euler_step(y: f32, dy_dt: f32, dt: f32) -> f32 {
    y + dy_dt * dt
}

/// Advance a state vector by `dt` using forward Euler.
///
/// Each component is updated independently:
/// `y[i]_{n+1} = y[i]_n + dy_dt[i] * dt`
///
/// # Panics
/// Panics in debug mode if `y` and `dy_dt` have different lengths.
///
/// # Arguments
/// * `y`      — current state vector
/// * `dy_dt`  — derivative vector evaluated at the current state (same length as `y`)
/// * `dt`     — time step
///
/// # Returns
/// A new `Vec<f32>` containing the updated state.
pub fn euler_step_slice(y: &[f32], dy_dt: &[f32], dt: f32) -> Vec<f32> {
    debug_assert_eq!(
        y.len(),
        dy_dt.len(),
        "euler_step_slice: y and dy_dt must have the same length"
    );
    y.iter()
        .zip(dy_dt.iter())
        .map(|(&yi, &di)| yi + di * dt)
        .collect()
}

/// Backward (implicit) Euler via fixed-point iteration for a scalar state.
///
/// Solves `y_{n+1} = y_n + dt · f(y_{n+1})` by iterating:
/// `y^(k+1) = y_n + dt · f(y^(k))`
/// starting from `y^(0) = y_n` (forward Euler predictor).
///
/// Convergence is guaranteed when `|dt · ∂f/∂y| < 1` at the solution.
/// If the iteration does not converge within `max_iter` steps the best
/// available iterate is returned — the caller should either tighten `dt`
/// or switch to a Newton-based solver for highly stiff cases.
///
/// # Arguments
/// * `y`        — current state value at time `t`
/// * `dt`       — time step
/// * `f`        — function returning `dy/dt` given a candidate `y` value
/// * `max_iter` — maximum fixed-point iterations (8–16 is usually sufficient)
/// * `tol`      — convergence tolerance; iteration stops when `|y^(k+1) - y^(k)| < tol`
///
/// # Returns
/// Approximation of the state value at `t + dt`.
pub fn backward_euler_step<F: Fn(f32) -> f32>(
    y: f32,
    dt: f32,
    f: F,
    max_iter: u32,
    tol: f32,
) -> f32 {
    let mut y_next = y; // initial guess: y_n
    for _ in 0..max_iter {
        let y_new = y + dt * f(y_next);
        if (y_new - y_next).abs() < tol {
            return y_new;
        }
        y_next = y_new;
    }
    y_next
}

/// Backward (implicit) Euler via fixed-point iteration for a state vector.
///
/// Solves `y_{n+1} = y_n + dt · f(y_{n+1})` component-wise by iterating:
/// `y^(k+1)[i] = y_n[i] + dt · f(y^(k))[i]`
/// starting from `y^(0) = y_n`.
///
/// Convergence check: the iteration stops when the maximum absolute difference
/// across all components satisfies `max_i |y^(k+1)[i] - y^(k)[i]| < tol`.
///
/// See [`backward_euler_step`] for guidance on when to prefer this over
/// forward Euler and for caveats about non-convergence.
///
/// # Arguments
/// * `y`        — current state vector at time `t`
/// * `dt`       — time step
/// * `f`        — function returning `dy/dt` as a `Vec<f32>` given a candidate state slice
/// * `max_iter` — maximum fixed-point iterations (8–16 is usually sufficient)
/// * `tol`      — convergence tolerance on the max-norm of the update
///
/// # Returns
/// A new `Vec<f32>` containing the approximated state at `t + dt`.
pub fn backward_euler_step_slice<F: Fn(&[f32]) -> Vec<f32>>(
    y: &[f32],
    dt: f32,
    f: F,
    max_iter: u32,
    tol: f32,
) -> Vec<f32> {
    let n = y.len();
    let mut y_next: Vec<f32> = y.to_vec(); // initial guess: y_n

    for _ in 0..max_iter {
        let deriv = f(&y_next);
        debug_assert_eq!(
            deriv.len(),
            n,
            "backward_euler_step_slice: f must return a Vec of the same length as y"
        );

        let y_new: Vec<f32> = y
            .iter()
            .zip(deriv.iter())
            .map(|(&yi, &di)| yi + dt * di)
            .collect();

        // Convergence: max absolute difference across all components
        let max_diff = y_new
            .iter()
            .zip(y_next.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);

        y_next = y_new;

        if max_diff < tol {
            break;
        }
    }

    y_next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_euler_scalar_constant_derivative() {
        // dy/dt = 1, y(0) = 0 → y(1) = 1
        let y = euler_step(0.0, 1.0, 1.0);
        assert!((y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn forward_euler_scalar_zero_dt() {
        let y = euler_step(3.14, 100.0, 0.0);
        assert!((y - 3.14).abs() < 1e-6);
    }

    #[test]
    fn forward_euler_slice_matches_scalar() {
        let y = [1.0_f32, 2.0, 3.0];
        let dy = [0.1_f32, 0.2, 0.3];
        let dt = 0.5;
        let result = euler_step_slice(&y, &dy, dt);
        for i in 0..3 {
            let expected = euler_step(y[i], dy[i], dt);
            assert!((result[i] - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn backward_euler_scalar_linear() {
        // dy/dt = -10 * y  (stiff decay), y(0) = 1
        // Exact: y(dt) = exp(-10 * dt)
        // Backward Euler: y_{n+1} = y_n / (1 + 10*dt)
        let dt = 0.5_f32;
        let y0 = 1.0_f32;
        let result = backward_euler_step(y0, dt, |y| -10.0 * y, 64, 1e-8);
        let expected = y0 / (1.0 + 10.0 * dt);
        assert!(
            (result - expected).abs() < 1e-4,
            "result={result}, expected={expected}"
        );
    }

    #[test]
    fn backward_euler_slice_linear() {
        // Two independent decays: dy_i/dt = -k_i * y_i
        let k = [2.0_f32, 5.0_f32];
        let y0 = [1.0_f32, 1.0_f32];
        let dt = 0.2_f32;
        let result = backward_euler_step_slice(
            &y0,
            dt,
            |y| vec![-k[0] * y[0], -k[1] * y[1]],
            64,
            1e-8,
        );
        for i in 0..2 {
            let expected = y0[i] / (1.0 + k[i] * dt);
            assert!(
                (result[i] - expected).abs() < 1e-4,
                "component {i}: result={}, expected={expected}",
                result[i]
            );
        }
    }

    #[test]
    fn backward_euler_convergence_tolerance() {
        // Constant derivative — converges in one iteration
        let result = backward_euler_step(0.0, 1.0, |_y| 3.0, 16, 1e-6);
        assert!((result - 3.0).abs() < 1e-6);
    }
}
