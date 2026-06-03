//! Velocity Verlet (Störmer-Verlet) integrator.
//! Symplectic — conserves the phase-space volume; ideal for Newtonian mechanics.
//!
//! # Algorithm
//!
//! ```text
//!   x_{n+1} = x_n + v_n·dt + ½·a_n·dt²
//!   a_{n+1} = f(x_{n+1}) / m
//!   v_{n+1} = v_n + ½·(a_n + a_{n+1})·dt
//! ```
//!
//! # Why Velocity Verlet instead of forward Euler?
//!
//! Forward Euler is a first-order method: it approximates the future state by
//! extrapolating the current derivative in a straight line.  This is cheap but
//! pathologically bad for Hamiltonian systems:
//!
//! * **Energy drift** — Euler systematically adds energy on every step.  A
//!   planet in a 2-body orbit will spiral outward indefinitely, no matter how
//!   small `dt` is (the error is O(dt) per step and accumulates linearly with
//!   time).
//!
//! * **Not time-reversible** — running Euler backwards does not recover the
//!   original trajectory; the method breaks the time-symmetry of Newton's laws.
//!
//! Velocity Verlet is a **symplectic** integrator: it preserves the geometric
//! structure (phase-space volume) of Hamiltonian flow exactly.  Consequences:
//!
//! * **No secular energy drift** — the total mechanical energy oscillates
//!   around its true value by a bounded O(dt²) amount *forever*, even over
//!   millions of steps.  This makes it suitable for orbital mechanics,
//!   molecular dynamics, and spring networks where long-run stability matters.
//!
//! * **Time-reversible** — negating `dt` and running the integrator backwards
//!   exactly recovers the initial state.  This matches the time-symmetry of
//!   the underlying physics.
//!
//! * **Exact angular-momentum conservation for central forces** — for a force
//!   that points along the displacement vector (gravity, Coulomb, spring
//!   attached to a fixed point), the discrete update preserves angular momentum
//!   to machine precision.  Euler does not.
//!
//! * **Second-order accuracy** — the global error is O(dt²) vs O(dt) for
//!   Euler, so you can take roughly 10× larger time steps for the same
//!   positional accuracy.
//!
//! # Two-stage API
//!
//! The 1-D and 3-D position/half-velocity helpers split the algorithm into two
//! calls so that the **caller can evaluate the force at the new position**
//! between the two stages.  This is necessary because `a_{n+1}` depends on
//! `x_{n+1}`, which is only known after the first stage.
//!
//! ```
//! use eustress_common::realism::numerics::ode::verlet::*;
//!
//! // 1-D spring: F = -k·x, a = -k·x (unit mass)
//! let k = 1.0_f32;
//! let (mut x, mut v, mut a) = (1.0_f32, 0.0_f32, -1.0_f32);
//! let dt = 0.01_f32;
//! for _ in 0..1000 {
//!     let (x2, v_half) = verlet_position_halfvel(x, v, a, dt);
//!     let a2 = -k * x2;
//!     v = verlet_velocity_complete(v_half, a2, dt);
//!     (x, a) = (x2, a2);
//! }
//! ```

// ────────────────────────────────────────────────────────────────
// Scalar (1-D) API
// ────────────────────────────────────────────────────────────────

/// Half-step: advance position to `x_{n+1}` and compute the intermediate
/// half-velocity `v_{n+½}` needed to complete the step.
///
/// # Returns
/// `(x_new, v_half)` where
/// * `x_new   = x + v·dt + ½·a·dt²`
/// * `v_half  = v + ½·a·dt`
///
/// After this call the caller **must** evaluate the force/acceleration at
/// `x_new`, then call [`verlet_velocity_complete`] to get `v_{n+1}`.
#[inline]
pub fn verlet_position_halfvel(x: f32, v: f32, a: f32, dt: f32) -> (f32, f32) {
    let v_half = v + 0.5 * a * dt;
    let x_new = x + v_half * dt;
    (x_new, v_half)
}

/// Complete the velocity update using the acceleration at the **new** position.
///
/// # Returns
/// `v_new = v_half + ½·a_new·dt`
#[inline]
pub fn verlet_velocity_complete(v_half: f32, a_new: f32, dt: f32) -> f32 {
    v_half + 0.5 * a_new * dt
}

/// Full scalar Velocity Verlet step with explicit acceleration values.
///
/// This convenience function performs both stages when you already have
/// `a_current` (acceleration at step *n*) and `a_next` (acceleration at
/// step *n+1*, which you evaluate from the returned `x_new`).
///
/// Because `a_next` depends on `x_new`, the typical usage pattern is:
///
/// ```ignore
/// let (x_new, _v_half, _) = verlet_step(x, v, a, dt);
/// let a_new = force(x_new);
/// let (_, v_new, _) = verlet_step_finish(v, a, a_new, dt); // not provided
/// ```
///
/// For the split two-call pattern see [`verlet_position_halfvel`] and
/// [`verlet_velocity_complete`].
///
/// # Returns
/// `(x_new, v_new, a_current)` — `a_current` is echoed back so it can be
/// stored as the *next* step's `a_current` after the caller has computed
/// `a_next` and used it to build `v_new` via [`verlet_velocity_complete`].
///
/// **Practical note**: call this when you want a one-liner *and* you already
/// have both `a_current` and `a_next` available (e.g. in tests or analytic
/// force fields).  Pass `a_next` as a separate value and combine with
/// [`verlet_velocity_complete`] yourself, or use the 3-D closure variant
/// [`verlet_3d_step`] which handles the two-stage internally.
#[inline]
pub fn verlet_step(x: f32, v: f32, a_current: f32, dt: f32) -> (f32, f32, f32) {
    // Stage 1 — position and half-velocity
    let (x_new, v_half) = verlet_position_halfvel(x, v, a_current, dt);
    // Stage 2 is deferred to the caller (they need to evaluate force at x_new).
    // We return v_half in the second slot so the caller can finish with:
    //   v_new = verlet_velocity_complete(v_half, a_next, dt)
    // The third return value echoes a_current for bookkeeping convenience.
    (x_new, v_half, a_current)
}

// ────────────────────────────────────────────────────────────────
// 3-D (x, y, z) API
// ────────────────────────────────────────────────────────────────

/// 3-D half-step: advance all three position components and compute the
/// half-velocities.
///
/// Applies [`verlet_position_halfvel`] independently to each axis.
///
/// # Returns
/// `(x_new, v_half)` — both are `[f32; 3]` arrays.
#[inline]
pub fn verlet_3d_position_halfvel(
    x: [f32; 3],
    v: [f32; 3],
    a: [f32; 3],
    dt: f32,
) -> ([f32; 3], [f32; 3]) {
    let mut x_new = [0.0_f32; 3];
    let mut v_half = [0.0_f32; 3];
    for i in 0..3 {
        let (xi, vi) = verlet_position_halfvel(x[i], v[i], a[i], dt);
        x_new[i] = xi;
        v_half[i] = vi;
    }
    (x_new, v_half)
}

/// Complete the 3-D velocity update using acceleration at the new position.
///
/// Applies [`verlet_velocity_complete`] independently to each axis.
///
/// # Returns
/// `v_new: [f32; 3]`
#[inline]
pub fn verlet_3d_velocity_complete(v_half: [f32; 3], a_new: [f32; 3], dt: f32) -> [f32; 3] {
    let mut v_new = [0.0_f32; 3];
    for i in 0..3 {
        v_new[i] = verlet_velocity_complete(v_half[i], a_new[i], dt);
    }
    v_new
}

/// Convenience: full 3-D Velocity Verlet step given a force/acceleration
/// function.
///
/// The closure `f_accel` receives the **new position** `x_{n+1}` (as a shared
/// reference) and must return the corresponding acceleration `a_{n+1}`.  The
/// mass is assumed to be absorbed into the closure.
///
/// ```
/// use eustress_common::realism::numerics::ode::verlet::verlet_3d_step;
///
/// // Sun-centred gravity (G·M = 1, unit mass).
/// let gm = 1.0_f32;
/// let gravity = |p: &[f32; 3]| {
///     let r2 = p[0]*p[0] + p[1]*p[1] + p[2]*p[2];
///     let r3 = r2 * r2.sqrt();
///     [-gm * p[0] / r3, -gm * p[1] / r3, -gm * p[2] / r3]
/// };
///
/// let x = [1.0_f32, 0.0, 0.0];
/// let v = [0.0_f32, 1.0, 0.0];
/// let a = gravity(&x);
/// let dt = 0.001_f32;
///
/// let (x2, v2, a2) = verlet_3d_step(x, v, a, dt, gravity);
/// ```
///
/// # Returns
/// `(x_new, v_new, a_new)` — the acceleration at `x_new` is returned so it
/// can be fed directly into the next call without a redundant force evaluation.
#[inline]
pub fn verlet_3d_step<F>(
    x: [f32; 3],
    v: [f32; 3],
    a: [f32; 3],
    dt: f32,
    f_accel: F,
) -> ([f32; 3], [f32; 3], [f32; 3])
where
    F: Fn(&[f32; 3]) -> [f32; 3],
{
    // Stage 1 — position + half-velocity
    let (x_new, v_half) = verlet_3d_position_halfvel(x, v, a, dt);
    // Evaluate force at new position (caller-supplied)
    let a_new = f_accel(&x_new);
    // Stage 2 — complete velocity
    let v_new = verlet_3d_velocity_complete(v_half, a_new, dt);
    (x_new, v_new, a_new)
}

// ────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Harmonic oscillator: x'' = -x.  Exact solution: x(t) = cos(t), v(t) = -sin(t).
    /// Energy E = ½(v² + x²) should be conserved.
    #[test]
    fn harmonic_oscillator_energy_conserved() {
        let (mut x, mut v, mut a) = (1.0_f32, 0.0_f32, -1.0_f32);
        let dt = 0.001_f32;
        let e0 = 0.5 * (v * v + x * x);

        for _ in 0..100_000 {
            let (x2, v_half) = verlet_position_halfvel(x, v, a, dt);
            let a2 = -x2; // F = -x, unit mass
            v = verlet_velocity_complete(v_half, a2, dt);
            x = x2;
            a = a2;
        }

        let e_final = 0.5 * (v * v + x * x);
        // Energy should be conserved to within a small fraction over 100k steps.
        let drift = (e_final - e0).abs() / e0;
        assert!(
            drift < 1e-4,
            "energy drift {drift} exceeds tolerance after 100k steps"
        );
    }

    /// 3-D circular orbit under inverse-square gravity.
    /// Angular momentum L = r × v should remain constant.
    #[test]
    fn circular_orbit_angular_momentum_conserved() {
        // Initial conditions for a circular orbit: r=1, v_tangential=1 (G·M=1).
        let x0 = [1.0_f32, 0.0, 0.0];
        let v0 = [0.0_f32, 1.0, 0.0];

        let gm = 1.0_f32;
        let gravity = |p: &[f32; 3]| {
            let r2 = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
            let r3 = r2 * r2.sqrt();
            [
                -gm * p[0] / r3,
                -gm * p[1] / r3,
                -gm * p[2] / r3,
            ]
        };

        let (mut x, mut v, mut a) = (x0, v0, gravity(&x0));
        let dt = 0.001_f32;

        // Angular momentum (z-component only, orbit is in x-y plane): Lz = x*vy - y*vx
        let lz0 = x[0] * v[1] - x[1] * v[0];

        for _ in 0..10_000 {
            let result = verlet_3d_step(x, v, a, dt, &gravity);
            x = result.0;
            v = result.1;
            a = result.2;
        }

        let lz_final = x[0] * v[1] - x[1] * v[0];
        let drift = (lz_final - lz0).abs() / lz0.abs();
        assert!(
            drift < 1e-4,
            "angular momentum drift {drift} exceeds tolerance after 10k steps"
        );
    }

    /// Time-reversibility: integrate forward N steps then backward N steps;
    /// should return to within floating-point rounding of the origin.
    #[test]
    fn time_reversibility_1d() {
        let (x0, v0) = (1.0_f32, 0.5_f32);
        let (mut x, mut v, mut a) = (x0, v0, -x0);
        let dt = 0.01_f32;
        let steps = 500;

        // Forward
        for _ in 0..steps {
            let (x2, v_half) = verlet_position_halfvel(x, v, a, dt);
            let a2 = -x2;
            v = verlet_velocity_complete(v_half, a2, dt);
            x = x2;
            a = a2;
        }

        // Backward (negate dt)
        let dt_neg = -dt;
        for _ in 0..steps {
            let (x2, v_half) = verlet_position_halfvel(x, v, a, dt_neg);
            let a2 = -x2;
            v = verlet_velocity_complete(v_half, a2, dt_neg);
            x = x2;
            a = a2;
        }

        assert!(
            (x - x0).abs() < 1e-4,
            "position not recovered after time reversal: x={x}, x0={x0}"
        );
        assert!(
            (v - v0).abs() < 1e-4,
            "velocity not recovered after time reversal: v={v}, v0={v0}"
        );
    }

    /// `verlet_step` scalar convenience function: half-velocity in second slot.
    #[test]
    fn verlet_step_returns_half_velocity() {
        let (x, v, a, dt) = (2.0_f32, 1.0_f32, -2.0_f32, 0.5_f32);
        let (x_new, v_half, a_echo) = verlet_step(x, v, a, dt);

        // Check against manual calculation
        let expected_v_half = v + 0.5 * a * dt; // 1.0 + 0.5*(-2.0)*0.5 = 0.5
        let expected_x_new = x + expected_v_half * dt; // 2.0 + 0.5*0.5 = 2.25
        assert!((v_half - expected_v_half).abs() < 1e-6);
        assert!((x_new - expected_x_new).abs() < 1e-6);
        assert_eq!(a_echo, a); // echoed unchanged
    }
}
