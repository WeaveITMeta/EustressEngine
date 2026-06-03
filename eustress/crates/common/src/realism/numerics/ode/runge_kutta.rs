//! Runge-Kutta ODE integrators.
//!
//! Provides:
//! - [`rk4_step`]          -- scalar fixed-step RK4
//! - [`rk4_step_slice`]    -- vector fixed-step RK4
//! - [`rk45_step`]         -- Dormand-Prince FSAL single step (returns `(y4, y5)`)
//! - [`rk45_integrate`]    -- adaptive Dormand-Prince driver over `[t0, t_end]`

// ---------------------------------------------------------------------------
// Scalar RK4
// ---------------------------------------------------------------------------

/// Advance a scalar ODE `dy/dt = f(t, y)` by one fixed step `dt`.
///
/// Uses the classical four-stage Runge-Kutta tableau:
/// ```text
/// k1 = f(t,        y)
/// k2 = f(t + dt/2, y + dt/2 * k1)
/// k3 = f(t + dt/2, y + dt/2 * k2)
/// k4 = f(t + dt,   y + dt   * k3)
/// y_new = y + (dt/6)(k1 + 2*k2 + 2*k3 + k4)
/// ```
pub fn rk4_step<F>(f: F, t: f32, y: f32, dt: f32) -> f32
where
    F: Fn(f32, f32) -> f32,
{
    let k1 = f(t, y);
    let k2 = f(t + dt * 0.5, y + dt * 0.5 * k1);
    let k3 = f(t + dt * 0.5, y + dt * 0.5 * k2);
    let k4 = f(t + dt, y + dt * k3);
    y + (dt / 6.0) * (k1 + 2.0 * k2 + 2.0 * k3 + k4)
}

// ---------------------------------------------------------------------------
// Vector RK4
// ---------------------------------------------------------------------------

/// Advance a vector ODE `dy/dt = f(t, y)` by one fixed step `dt`.
///
/// `y` is a slice of length `n`; `f` returns a `Vec<f32>` of the same length.
/// Applies the same classical RK4 tableau as [`rk4_step`] component-wise.
pub fn rk4_step_slice<F>(f: F, t: f32, y: &[f32], dt: f32) -> Vec<f32>
where
    F: Fn(f32, &[f32]) -> Vec<f32>,
{
    let n = y.len();

    let k1 = f(t, y);

    let y2: Vec<f32> = (0..n).map(|i| y[i] + dt * 0.5 * k1[i]).collect();
    let k2 = f(t + dt * 0.5, &y2);

    let y3: Vec<f32> = (0..n).map(|i| y[i] + dt * 0.5 * k2[i]).collect();
    let k3 = f(t + dt * 0.5, &y3);

    let y4: Vec<f32> = (0..n).map(|i| y[i] + dt * k3[i]).collect();
    let k4 = f(t + dt, &y4);

    (0..n)
        .map(|i| y[i] + (dt / 6.0) * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
        .collect()
}

// ---------------------------------------------------------------------------
// Dormand-Prince RK45 (FSAL) -- single step
// ---------------------------------------------------------------------------

/// Dormand-Prince Butcher tableau (DOPRI5 / RK45 FSAL).
///
/// Nodes (c):
/// ```text
/// c2 = 1/5,  c3 = 3/10, c4 = 4/5, c5 = 8/9, c6 = 1, c7 = 1
/// ```
///
/// Coefficients (a):
/// ```text
/// a21 =  1/5
/// a31 =  3/40,       a32 = 9/40
/// a41 =  44/45,      a42 = -56/15,      a43 = 32/9
/// a51 =  19372/6561, a52 = -25360/2187, a53 = 64448/6561, a54 = -212/729
/// a61 =  9017/3168,  a62 = -355/33,     a63 = 46732/5247, a64 = 49/176,  a65 = -5103/18656
/// a71 =  35/384,     a72 = 0,           a73 = 500/1113,   a74 = 125/192, a75 = -2187/6784, a76 = 11/84
/// ```
///
/// 4th-order weights (b4):
/// ```text
/// 5179/57600, 0, 7571/16695, 393/640, -92097/339200, 187/2100, 1/40
/// ```
///
/// 5th-order weights (b5*):
/// ```text
/// 35/384, 0, 500/1113, 125/192, -2187/6784, 11/84, 0
/// ```
///
/// Returns `(y4, y5)` -- the 4th- and 5th-order solutions after one step.
pub fn rk45_step<F>(f: F, t: f32, y: f32, dt: f32) -> (f32, f32)
where
    F: Fn(f32, f32) -> f32,
{
    // Stage 1
    let k1 = f(t, y);

    // Stage 2
    let k2 = f(t + dt / 5.0, y + dt * (1.0 / 5.0) * k1);

    // Stage 3
    let k3 = f(
        t + dt * 3.0 / 10.0,
        y + dt * ((3.0 / 40.0) * k1 + (9.0 / 40.0) * k2),
    );

    // Stage 4
    let k4 = f(
        t + dt * 4.0 / 5.0,
        y + dt * ((44.0 / 45.0) * k1 - (56.0 / 15.0) * k2 + (32.0 / 9.0) * k3),
    );

    // Stage 5
    let k5 = f(
        t + dt * 8.0 / 9.0,
        y + dt
            * ((19372.0 / 6561.0) * k1 - (25360.0 / 2187.0) * k2 + (64448.0 / 6561.0) * k3
                - (212.0 / 729.0) * k4),
    );

    // Stage 6
    let k6 = f(
        t + dt,
        y + dt
            * ((9017.0 / 3168.0) * k1 - (355.0 / 33.0) * k2 + (46732.0 / 5247.0) * k3
                + (49.0 / 176.0) * k4
                - (5103.0 / 18656.0) * k5),
    );

    // 5th-order solution (b5* weights; k7 FSAL stage not needed for y5 itself)
    let y5 = y
        + dt * ((35.0 / 384.0) * k1
            + (500.0 / 1113.0) * k3
            + (125.0 / 192.0) * k4
            - (2187.0 / 6784.0) * k5
            + (11.0 / 84.0) * k6);

    // Stage 7 -- FSAL: f(t+dt, y5) reused as k1 of the next step
    let k7 = f(t + dt, y5);

    // 4th-order solution (b4 weights)
    let y4 = y
        + dt * ((5179.0 / 57600.0) * k1
            + (7571.0 / 16695.0) * k3
            + (393.0 / 640.0) * k4
            - (92097.0 / 339200.0) * k5
            + (187.0 / 2100.0) * k6
            + (1.0 / 40.0) * k7);

    (y4, y5)
}

// ---------------------------------------------------------------------------
// Adaptive Dormand-Prince driver
// ---------------------------------------------------------------------------

/// Integrate `dy/dt = f(t, y)` from `t0` to `t_end` using adaptive step-size
/// Dormand-Prince (RK45 FSAL).
///
/// # Arguments
/// * `f`      -- right-hand side `f(t, y) -> dy/dt`
/// * `t0`     -- initial time
/// * `y0`     -- initial value
/// * `t_end`  -- final time (`t_end > t0` required)
/// * `dt0`    -- initial step-size hint
/// * `rtol`   -- relative tolerance
/// * `atol`   -- absolute tolerance
///
/// # Returns
/// `Vec<(t, y)>` of every accepted step, starting with `(t0, y0)` and ending
/// at `(t_end, y_accepted)`.
///
/// # Step-size control
/// A step is accepted when `|y5 - y4| <= atol + rtol * |y4|`.
/// The next step is scaled by `0.9 * (tol / err)^0.2` clamped to `[0.2, 5.0]`
/// times the current `dt` (i.e. `[dt/5, dt*5]`).
/// A safety cap of 1 000 000 steps prevents infinite loops.
pub fn rk45_integrate<F>(
    f: F,
    t0: f32,
    y0: f32,
    t_end: f32,
    dt0: f32,
    rtol: f32,
    atol: f32,
) -> Vec<(f32, f32)>
where
    F: Fn(f32, f32) -> f32,
{
    const MAX_STEPS: usize = 1_000_000;

    let mut path = Vec::new();
    path.push((t0, y0));

    let mut t = t0;
    let mut y = y0;
    let mut dt = dt0.min(t_end - t0);

    for _ in 0..MAX_STEPS {
        if t >= t_end {
            break;
        }

        // Clamp final step to hit t_end exactly.
        let dt_use = if t + dt > t_end { t_end - t } else { dt };

        let (y4, y5) = rk45_step(&f, t, y, dt_use);

        let err = (y5 - y4).abs();
        let tol = atol + rtol * y4.abs();

        if err <= tol {
            // Accept step.
            t += dt_use;
            y = y5; // take 5th-order solution (local extrapolation)
            path.push((t, y));

            // Scale next step.
            let scale = if err == 0.0 {
                5.0_f32
            } else {
                0.9 * (tol / err).powf(0.2)
            };
            let scale = scale.clamp(0.2, 5.0);
            dt = (dt_use * scale).min(t_end - t);
        } else {
            // Reject step -- shrink dt and retry.
            let scale = 0.9 * (tol / err).powf(0.2);
            let scale = scale.clamp(0.2, 5.0);
            dt = dt_use * scale;
        }
    }

    path
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact solution to `dy/dt = -y`, `y(0) = 1` is `y(t) = e^{-t}`.
    fn decay(t: f32, y: f32) -> f32 {
        let _ = t;
        -y
    }

    // ------------------------------------------------------------------
    // Test 1 -- scalar RK4 accuracy on exponential decay
    // ------------------------------------------------------------------
    #[test]
    fn rk4_scalar_decay() {
        let dt = 0.01_f32;
        let steps = 100; // integrate from 0 to 1
        let mut t = 0.0_f32;
        let mut y = 1.0_f32;
        for _ in 0..steps {
            y = rk4_step(decay, t, y, dt);
            t += dt;
        }
        let exact = (-1.0_f32).exp();
        // RK4 global error is O(dt^4); with dt=0.01 over 100 steps expect < 1e-6.
        assert!(
            (y - exact).abs() < 1e-6,
            "rk4_scalar: got {y}, expected {exact}, err = {}",
            (y - exact).abs()
        );
    }

    // ------------------------------------------------------------------
    // Test 2 -- vector RK4 accuracy on exponential decay
    // ------------------------------------------------------------------
    #[test]
    fn rk4_slice_decay() {
        let dt = 0.01_f32;
        let steps = 100;
        let mut t = 0.0_f32;
        let mut y = vec![1.0_f32, 2.0_f32]; // two independent decays
        for _ in 0..steps {
            y = rk4_step_slice(|t, y| vec![-y[0], -y[1]], t, &y, dt);
            t += dt;
        }
        let exact0 = (-1.0_f32).exp();
        let exact1 = 2.0 * (-1.0_f32).exp();
        assert!(
            (y[0] - exact0).abs() < 1e-6,
            "rk4_slice[0]: got {}, expected {}",
            y[0],
            exact0
        );
        assert!(
            (y[1] - exact1).abs() < 1e-6,
            "rk4_slice[1]: got {}, expected {}",
            y[1],
            exact1
        );
    }

    // ------------------------------------------------------------------
    // Test 3 -- RK45 adaptive integration accuracy
    // ------------------------------------------------------------------
    #[test]
    fn rk45_adaptive_accuracy() {
        // Integrate dy/dt = -y from 0 to 2; exact = e^{-2}.
        let path = rk45_integrate(decay, 0.0, 1.0, 2.0, 0.1, 1e-6, 1e-8);

        assert!(!path.is_empty(), "path must not be empty");

        let (t_last, y_last) = *path.last().unwrap();
        let exact = (-2.0_f32).exp();

        assert!(
            (t_last - 2.0).abs() < 1e-5,
            "final t = {t_last}, expected 2.0"
        );
        assert!(
            (y_last - exact).abs() < 1e-5,
            "rk45_adaptive: got {y_last}, expected {exact}, err = {}",
            (y_last - exact).abs()
        );
    }

    // ------------------------------------------------------------------
    // Test 4 -- RK45 single step order verification
    // ------------------------------------------------------------------
    /// Verify that the local truncation error of the 5th-order solution is
    /// O(dt^5): halving dt should reduce the error by ~32x.
    #[test]
    fn rk45_single_step_order() {
        let t0 = 0.0_f32;
        let y0 = 1.0_f32;

        let dt_coarse = 0.2_f32;
        let dt_fine = dt_coarse / 2.0;

        let (_, y5_coarse) = rk45_step(decay, t0, y0, dt_coarse);
        let (_, y5_fine) = rk45_step(decay, t0, y0, dt_fine);

        let exact_coarse = (-dt_coarse).exp();
        let exact_fine = (-dt_fine).exp();

        let err_coarse = (y5_coarse - exact_coarse).abs();
        let err_fine = (y5_fine - exact_fine).abs();

        // 5th-order method: err proportional to dt^5, so ratio should be near 2^5 = 32.
        // Allow a generous band (> 10) to avoid floating-point sensitivity.
        if err_coarse > 1e-12 && err_fine > 1e-12 {
            let ratio = err_coarse / err_fine;
            assert!(
                ratio > 10.0,
                "expected 5th-order convergence (ratio ~ 32), got ratio = {ratio}"
            );
        }
    }
}