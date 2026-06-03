//! State-space system representation and analysis.
//! Supports up to MAX_STATES (8) state variables for real-time use.
//!
//! State-space form: dx/dt = A·x + B·u,  y = C·x + D·u
//! Pure std, no Bevy, no external linear algebra crates.

pub const MAX_STATES: usize = 8;

/// State-space model: dx/dt = A·x + B·u, y = C·x + D·u
///
/// n = number of states, m = number of inputs, p = number of outputs
/// A: n×n, B: n×m, C: p×n, D: p×m
///
/// All matrices are stored as fixed MAX_STATES × MAX_STATES arrays (row-major).
/// Only the leading n×n (or n×m / p×n / p×m) sub-block is used.
#[derive(Clone, Debug)]
pub struct StateSpaceModel {
    /// Number of state variables (≤ MAX_STATES)
    pub n: usize,
    /// Number of inputs (≤ MAX_STATES)
    pub m: usize,
    /// Number of outputs (≤ MAX_STATES)
    pub p: usize,
    /// State matrix  A: n×n  (row-major, padded to MAX_STATES×MAX_STATES)
    pub a: [[f32; MAX_STATES]; MAX_STATES],
    /// Input matrix  B: n×m  (row-major, padded)
    pub b: [[f32; MAX_STATES]; MAX_STATES],
    /// Output matrix C: p×n  (row-major, padded)
    pub c: [[f32; MAX_STATES]; MAX_STATES],
    /// Feed-through  D: p×m  (row-major, padded)
    pub d: [[f32; MAX_STATES]; MAX_STATES],
}

impl StateSpaceModel {
    /// Create a zero-initialised state-space model with the given dimensions.
    ///
    /// # Panics
    /// Panics if any dimension exceeds MAX_STATES.
    pub fn new(n: usize, m: usize, p: usize) -> Self {
        assert!(n <= MAX_STATES, "n={n} exceeds MAX_STATES={MAX_STATES}");
        assert!(m <= MAX_STATES, "m={m} exceeds MAX_STATES={MAX_STATES}");
        assert!(p <= MAX_STATES, "p={p} exceeds MAX_STATES={MAX_STATES}");
        Self {
            n,
            m,
            p,
            a: [[0.0; MAX_STATES]; MAX_STATES],
            b: [[0.0; MAX_STATES]; MAX_STATES],
            c: [[0.0; MAX_STATES]; MAX_STATES],
            d: [[0.0; MAX_STATES]; MAX_STATES],
        }
    }

    // -------------------------------------------------------------------------
    // Matrix element setters
    // -------------------------------------------------------------------------

    /// Set element (row, col) of the state matrix A (n×n).
    #[inline]
    pub fn set_a(&mut self, row: usize, col: usize, val: f32) {
        self.a[row][col] = val;
    }

    /// Set element (row, col) of the input matrix B (n×m).
    #[inline]
    pub fn set_b(&mut self, row: usize, col: usize, val: f32) {
        self.b[row][col] = val;
    }

    /// Set element (row, col) of the output matrix C (p×n).
    #[inline]
    pub fn set_c(&mut self, row: usize, col: usize, val: f32) {
        self.c[row][col] = val;
    }

    /// Set element (row, col) of the feed-through matrix D (p×m).
    #[inline]
    pub fn set_d(&mut self, row: usize, col: usize, val: f32) {
        self.d[row][col] = val;
    }

    // -------------------------------------------------------------------------
    // Core computations
    // -------------------------------------------------------------------------

    /// Compute the state derivative **dx/dt = A·x + B·u**.
    ///
    /// Only the first `n` elements of the returned array are meaningful.
    /// `x` must have at least `n` elements; `u` must have at least `m` elements.
    pub fn state_derivative(&self, x: &[f32], u: &[f32]) -> [f32; MAX_STATES] {
        let mut dx = [0.0f32; MAX_STATES];
        // A·x  (n×n · n×1)
        for i in 0..self.n {
            let mut acc = 0.0f32;
            for j in 0..self.n {
                acc += self.a[i][j] * x[j];
            }
            dx[i] += acc;
        }
        // B·u  (n×m · m×1)
        for i in 0..self.n {
            let mut acc = 0.0f32;
            for j in 0..self.m {
                acc += self.b[i][j] * u[j];
            }
            dx[i] += acc;
        }
        dx
    }

    /// Compute the system output **y = C·x + D·u**.
    ///
    /// Only the first `p` elements of the returned array are meaningful.
    /// `x` must have at least `n` elements; `u` must have at least `m` elements.
    pub fn output(&self, x: &[f32], u: &[f32]) -> [f32; MAX_STATES] {
        let mut y = [0.0f32; MAX_STATES];
        // C·x  (p×n · n×1)
        for i in 0..self.p {
            let mut acc = 0.0f32;
            for j in 0..self.n {
                acc += self.c[i][j] * x[j];
            }
            y[i] += acc;
        }
        // D·u  (p×m · m×1)
        for i in 0..self.p {
            let mut acc = 0.0f32;
            for j in 0..self.m {
                acc += self.d[i][j] * u[j];
            }
            y[i] += acc;
        }
        y
    }

    /// Advance the state by `dt` using the classical **4th-order Runge-Kutta** method.
    ///
    /// The input `u` is held constant over the step (zero-order hold).
    /// Only the first `n` elements of the returned array are meaningful.
    pub fn step_rk4(&self, x: &[f32], u: &[f32], dt: f32) -> [f32; MAX_STATES] {
        let n = self.n;

        // k1 = f(x)
        let k1 = self.state_derivative(x, u);

        // k2 = f(x + dt/2 * k1)
        let mut x2 = [0.0f32; MAX_STATES];
        for i in 0..n {
            x2[i] = x[i] + 0.5 * dt * k1[i];
        }
        let k2 = self.state_derivative(&x2, u);

        // k3 = f(x + dt/2 * k2)
        let mut x3 = [0.0f32; MAX_STATES];
        for i in 0..n {
            x3[i] = x[i] + 0.5 * dt * k2[i];
        }
        let k3 = self.state_derivative(&x3, u);

        // k4 = f(x + dt * k3)
        let mut x4 = [0.0f32; MAX_STATES];
        for i in 0..n {
            x4[i] = x[i] + dt * k3[i];
        }
        let k4 = self.state_derivative(&x4, u);

        // x_new = x + dt/6 * (k1 + 2·k2 + 2·k3 + k4)
        let mut x_new = [0.0f32; MAX_STATES];
        for i in 0..n {
            x_new[i] = x[i] + (dt / 6.0) * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
        x_new
    }

    /// Advance the state by `dt` using **forward Euler** integration.
    ///
    /// Faster than RK4 but only first-order accurate.  Suitable for small `dt`
    /// or when computational budget is tight.
    /// Only the first `n` elements of the returned array are meaningful.
    pub fn step_euler(&self, x: &[f32], u: &[f32], dt: f32) -> [f32; MAX_STATES] {
        let dx = self.state_derivative(x, u);
        let mut x_new = [0.0f32; MAX_STATES];
        for i in 0..self.n {
            x_new[i] = x[i] + dt * dx[i];
        }
        x_new
    }
}

// =============================================================================
// Transfer-function → state-space conversion (controllable canonical form)
// =============================================================================

/// Convert a transfer function given by polynomial coefficient slices to a
/// **controllable canonical form** state-space model.
///
/// The transfer function is:
/// ```text
/// H(s) = (num[0] + num[1]·s + num[2]·s² + …) / (den[0] + den[1]·s + … + den[n]·sⁿ)
/// ```
/// where `den` must be monic (leading coefficient = 1) **or** is normalised
/// internally.  `den.len()` determines the order `n`; `den` has `n+1` entries
/// (constant … sⁿ).  `num` may have fewer entries than `den` (zero-padded).
///
/// The returned model has `n` states, 1 input, 1 output.
///
/// # Panics
/// Panics if `den.len() < 2` or the system order exceeds MAX_STATES.
pub fn tf_to_state_space(num: &[f32], den: &[f32]) -> StateSpaceModel {
    assert!(den.len() >= 2, "den must have at least 2 coefficients (order ≥ 1)");
    let n = den.len() - 1; // system order
    assert!(n <= MAX_STATES, "system order {n} exceeds MAX_STATES={MAX_STATES}");

    // Normalise by leading denominator coefficient so the polynomial is monic.
    let lead = den[n];
    assert!(lead.abs() > f32::EPSILON, "leading denominator coefficient must be non-zero");

    // Normalised denominator coefficients a[0..n] (coefficient of s^i = den[i]/lead).
    // The characteristic polynomial is: s^n + a[n-1]·s^(n-1) + … + a[0]
    let mut a_coeff = [0.0f32; MAX_STATES]; // a[i] = den[i] / lead  for i in 0..n
    for i in 0..n {
        a_coeff[i] = den[i] / lead;
    }

    // Normalised numerator coefficients b[0..n+1].
    let mut b_coeff = [0.0f32; MAX_STATES];
    let num_len = num.len().min(n + 1);
    for i in 0..num_len {
        b_coeff[i] = num[i] / lead;
    }

    // ----- Build the controllable canonical form -----
    // States are ordered [x1, x2, …, xn] where the companion matrix rows are:
    //   A = [[0, 1, 0, …, 0],
    //        [0, 0, 1, …, 0],
    //        …
    //        [-a0, -a1, …, -a(n-1)]]   (bottom row)
    //
    // B = [0, 0, …, 0, 1]^T            (last row = 1)
    //
    // C = [b0 - a0·bn, b1 - a1·bn, …, b(n-1) - a(n-1)·bn]
    //
    // D = [bn]
    //
    // where bn = b_coeff[n] (degree-n numerator term, direct feed-through).

    let mut model = StateSpaceModel::new(n, 1, 1);

    // A matrix: companion / controllable canonical form
    // Sub-diagonal of 1s (shift register)
    for i in 0..(n - 1) {
        model.a[i][i + 1] = 1.0;
    }
    // Bottom row: -a_coeff
    for j in 0..n {
        model.a[n - 1][j] = -a_coeff[j];
    }

    // B matrix: last row = 1
    model.b[n - 1][0] = 1.0;

    // D matrix: direct feed-through = bn
    let b_n = b_coeff[n.min(MAX_STATES - 1)];
    // Only set if numerator degree == denominator degree (proper but not strictly proper)
    if num.len() > n {
        model.d[0][0] = b_n;
    }

    // C matrix: b_i - a_i * bn  for i in 0..n
    for j in 0..n {
        let bj = if j < num.len() { b_coeff[j] } else { 0.0 };
        let feed = if num.len() > n { b_n * a_coeff[j] } else { 0.0 };
        model.c[0][j] = bj - feed;
    }

    model
}

// =============================================================================
// Stability analysis: Routh-Hurwitz criterion
// =============================================================================

/// Check stability of a linear system using the **Routh-Hurwitz criterion**.
///
/// `a_poly` contains the coefficients of the **characteristic polynomial**
/// in ascending power order: `a_poly[i]` is the coefficient of `s^i`.
/// The polynomial must be of the form:
/// ```text
/// a0 + a1·s + a2·s² + … + an·sⁿ
/// ```
/// where `a0 = a_poly[0]`, …, `a_poly.last()` = leading coefficient.
///
/// Returns `true` if **all roots have strictly negative real parts**
/// (i.e. the system is asymptotically stable).
///
/// Handles polynomials up to 6th order (7 coefficients).  Higher orders
/// return `false` (unknown / unsupported).
///
/// # Implementation notes
/// * All coefficients must be the same sign and non-zero for stability (necessary
///   condition checked first).
/// * The full Routh array is constructed; a sign change in the first column
///   indicates instability.
pub fn is_stable_routh(a_poly: &[f32]) -> bool {
    let len = a_poly.len();
    if len < 2 {
        return false; // degenerate
    }
    let order = len - 1; // polynomial degree

    if order > 6 {
        return false; // not supported
    }

    // --- Necessary condition: all coefficients must be non-zero and same sign ---
    let sign_ref = a_poly[0].signum();
    if sign_ref == 0.0 {
        return false;
    }
    for &c in a_poly.iter() {
        if c.signum() != sign_ref {
            return false;
        }
        if c == 0.0 {
            return false;
        }
    }

    // Normalise so that leading coefficient is positive (divide through).
    // Coefficients in descending power order for the Routh table construction.
    // routh_coeffs[0] = a_poly[order] (highest), routh_coeffs[order] = a_poly[0]
    let mut coeffs = [0.0f32; 8]; // max order 6 → 7 elements; use 8 for safety
    for i in 0..=order {
        coeffs[i] = a_poly[order - i]; // descending
    }
    // Ensure positive leading coefficient
    if coeffs[0] < 0.0 {
        for c in coeffs.iter_mut() {
            *c = -*c;
        }
    }

    // --- Build Routh array ---
    // The Routh array has (order+1) rows, each up to ceil((order+1)/2) wide.
    // We store it as a 2D array [row][col].
    const MAX_ROUTH_COLS: usize = 4; // ceil(7/2)
    let mut routh = [[0.0f32; MAX_ROUTH_COLS]; 8];

    // Row 0: even-indexed coefficients (0, 2, 4, 6, …)
    // Row 1: odd-indexed coefficients  (1, 3, 5, 7, …)
    let num_cols = (order + 2) / 2; // columns per row (roughly)
    for k in 0..num_cols {
        let idx = 2 * k;
        if idx <= order {
            routh[0][k] = coeffs[idx];
        }
    }
    let num_cols1 = (order + 1) / 2;
    for k in 0..num_cols1 {
        let idx = 2 * k + 1;
        if idx <= order {
            routh[1][k] = coeffs[idx];
        }
    }

    // Rows 2..=order
    for row in 2..=(order) {
        // Copy rows into local arrays — prevents simultaneous borrow + assign on `routh`
        let prev2 = routh[row - 2];   // [f32; N] is Copy
        let prev1 = routh[row - 1];   // [f32; N] is Copy
        let pivot = prev1[0];
        if pivot.abs() < 1e-12 {
            // Zero pivot: system has roots on imaginary axis → not stable
            return false;
        }
        // Number of columns in this row = cols in row-2 minus 1
        let cols_above = if row == 2 { num_cols } else { MAX_ROUTH_COLS };
        for k in 0..(MAX_ROUTH_COLS - 1) {
            let _ = cols_above; // used for clarity; loop fills conservatively
            let a = prev2[k + 1];
            let b = prev1[k + 1];
            routh[row][k] = (pivot * a - prev2[0] * b) / pivot;
        }
    }

    // --- Check first column for sign changes ---
    let mut prev_sign = routh[0][0].signum();
    for row in 1..=(order) {
        let s = routh[row][0].signum();
        if s == 0.0 {
            return false; // zero in first column → imaginary-axis root
        }
        if s != prev_sign {
            return false; // sign change → right-half-plane root
        }
        prev_sign = s;
    }

    true
}

// =============================================================================
// Matrix utilities
// =============================================================================

/// Compute the **trace** of the A matrix (sum of diagonal elements).
///
/// The trace equals the sum of eigenvalues of A.
pub fn matrix_trace(model: &StateSpaceModel) -> f32 {
    let mut tr = 0.0f32;
    for i in 0..model.n {
        tr += model.a[i][i];
    }
    tr
}

/// Compute **A²** (matrix square of the state matrix A) and return a new
/// `StateSpaceModel` whose A field is set to A².  B, C, D are zeroed.
pub fn matrix_square(model: &StateSpaceModel) -> StateSpaceModel {
    let mut result = StateSpaceModel::new(model.n, model.m, model.p);
    let n = model.n;
    // result.a = model.a · model.a
    for i in 0..n {
        for j in 0..n {
            let mut acc = 0.0f32;
            for k in 0..n {
                acc += model.a[i][k] * model.a[k][j];
            }
            result.a[i][j] = acc;
        }
    }
    result
}

// =============================================================================
// Convenience constructors
// =============================================================================

/// Build a state-space model for a **second-order system**:
/// ```text
/// H(s) = ωn² / (s² + 2ζωn·s + ωn²)
/// ```
/// State vector: x = [x1, x2]  where x2 = ẋ1.
///
/// The resulting model has n=2, m=1, p=1.
///
/// # Arguments
/// * `omega_n` — Natural frequency (rad/s).
/// * `zeta`    — Damping ratio.
pub fn second_order_to_ss(omega_n: f32, zeta: f32) -> StateSpaceModel {
    // Denominator (ascending powers): ωn² + 2ζωn·s + s²
    let den = [omega_n * omega_n, 2.0 * zeta * omega_n, 1.0];
    // Numerator: ωn²
    let num = [omega_n * omega_n];
    tf_to_state_space(&num, &den)
}

/// Build a state-space model for a **first-order system**:
/// ```text
/// H(s) = K / (τs + 1)
/// ```
/// The resulting model has n=1, m=1, p=1.
///
/// # Arguments
/// * `gain` — Static gain K.
/// * `tau`  — Time constant τ (seconds).
pub fn first_order_to_ss(gain: f32, tau: f32) -> StateSpaceModel {
    // Denominator (ascending powers): 1 + τ·s
    let den = [1.0, tau];
    // Numerator: K
    let num = [gain];
    tf_to_state_space(&num, &den)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    // ------------------------------------------------------------------
    // Basic model construction
    // ------------------------------------------------------------------

    #[test]
    fn new_zeroed() {
        let m = StateSpaceModel::new(2, 1, 1);
        assert_eq!(m.n, 2);
        assert_eq!(m.m, 1);
        assert_eq!(m.p, 1);
        for i in 0..MAX_STATES {
            for j in 0..MAX_STATES {
                assert_eq!(m.a[i][j], 0.0);
            }
        }
    }

    // ------------------------------------------------------------------
    // state_derivative
    // ------------------------------------------------------------------

    #[test]
    fn state_derivative_identity() {
        // A = I2, B = 0, x = [1, 2], u = [0]
        // dx/dt should be [1, 2]
        let mut m = StateSpaceModel::new(2, 1, 1);
        m.set_a(0, 0, 1.0);
        m.set_a(1, 1, 1.0);
        let x = [1.0f32, 2.0];
        let u = [0.0f32];
        let dx = m.state_derivative(&x, &u);
        assert!(approx_eq(dx[0], 1.0, 1e-6));
        assert!(approx_eq(dx[1], 2.0, 1e-6));
    }

    #[test]
    fn state_derivative_with_input() {
        // Simple integrator: A = [[0]], B = [[1]], x=[0], u=[1]  → dx/dt = 1
        let mut m = StateSpaceModel::new(1, 1, 1);
        m.set_b(0, 0, 1.0);
        let x = [0.0f32];
        let u = [1.0f32];
        let dx = m.state_derivative(&x, &u);
        assert!(approx_eq(dx[0], 1.0, 1e-6));
    }

    // ------------------------------------------------------------------
    // output
    // ------------------------------------------------------------------

    #[test]
    fn output_direct_feedthrough() {
        // C=0, D=[[3]], x=[1], u=[2]  → y = 6
        let mut m = StateSpaceModel::new(1, 1, 1);
        m.set_d(0, 0, 3.0);
        let x = [1.0f32];
        let u = [2.0f32];
        let y = m.output(&x, &u);
        assert!(approx_eq(y[0], 6.0, 1e-6));
    }

    // ------------------------------------------------------------------
    // Euler step
    // ------------------------------------------------------------------

    #[test]
    fn euler_integrator_step() {
        // dx/dt = u, y = x. After dt=0.1 with u=1: x should be ≈0.1
        let mut m = StateSpaceModel::new(1, 1, 1);
        m.set_b(0, 0, 1.0);
        m.set_c(0, 0, 1.0);
        let x0 = [0.0f32];
        let u = [1.0f32];
        let x1 = m.step_euler(&x0, &u, 0.1);
        assert!(approx_eq(x1[0], 0.1, 1e-6));
    }

    // ------------------------------------------------------------------
    // RK4 step
    // ------------------------------------------------------------------

    #[test]
    fn rk4_integrator_step() {
        // dx/dt = u (constant), so RK4 and Euler agree exactly
        let mut m = StateSpaceModel::new(1, 1, 1);
        m.set_b(0, 0, 1.0);
        let x0 = [0.0f32];
        let u = [1.0f32];
        let x1 = m.step_rk4(&x0, &u, 0.1);
        assert!(approx_eq(x1[0], 0.1, 1e-5));
    }

    #[test]
    fn rk4_exponential_decay() {
        // dx/dt = -x  →  x(t) = x0 * e^{-t}
        // After one step of dt=0.01, x should be ≈ e^{-0.01} ≈ 0.99004983
        let mut m = StateSpaceModel::new(1, 1, 1);
        m.set_a(0, 0, -1.0);
        let x0 = [1.0f32];
        let u = [0.0f32];
        let x1 = m.step_rk4(&x0, &u, 0.01);
        let expected = (-0.01f32).exp();
        assert!(approx_eq(x1[0], expected, 1e-5));
    }

    // ------------------------------------------------------------------
    // matrix_trace
    // ------------------------------------------------------------------

    #[test]
    fn trace_of_diagonal() {
        let mut m = StateSpaceModel::new(3, 1, 1);
        m.set_a(0, 0, 1.0);
        m.set_a(1, 1, -2.0);
        m.set_a(2, 2, 3.0);
        assert!(approx_eq(matrix_trace(&m), 2.0, 1e-6));
    }

    // ------------------------------------------------------------------
    // matrix_square
    // ------------------------------------------------------------------

    #[test]
    fn square_of_identity() {
        let mut m = StateSpaceModel::new(2, 1, 1);
        m.set_a(0, 0, 1.0);
        m.set_a(1, 1, 1.0);
        let sq = matrix_square(&m);
        // I² = I
        assert!(approx_eq(sq.a[0][0], 1.0, 1e-6));
        assert!(approx_eq(sq.a[0][1], 0.0, 1e-6));
        assert!(approx_eq(sq.a[1][0], 0.0, 1e-6));
        assert!(approx_eq(sq.a[1][1], 1.0, 1e-6));
    }

    #[test]
    fn square_known_matrix() {
        // A = [[1,2],[3,4]]  →  A² = [[7,10],[15,22]]
        let mut m = StateSpaceModel::new(2, 1, 1);
        m.set_a(0, 0, 1.0);
        m.set_a(0, 1, 2.0);
        m.set_a(1, 0, 3.0);
        m.set_a(1, 1, 4.0);
        let sq = matrix_square(&m);
        assert!(approx_eq(sq.a[0][0], 7.0, 1e-5));
        assert!(approx_eq(sq.a[0][1], 10.0, 1e-5));
        assert!(approx_eq(sq.a[1][0], 15.0, 1e-5));
        assert!(approx_eq(sq.a[1][1], 22.0, 1e-5));
    }

    // ------------------------------------------------------------------
    // first_order_to_ss
    // ------------------------------------------------------------------

    #[test]
    fn first_order_dimensions() {
        let m = first_order_to_ss(2.0, 0.5);
        assert_eq!(m.n, 1);
        assert_eq!(m.m, 1);
        assert_eq!(m.p, 1);
    }

    #[test]
    fn first_order_steady_state() {
        // H(s) = K/(τs+1). At steady state (s=0) y/u = K.
        // In SS form: dx/dt = -1/τ * x + K/τ * u
        //             y     = x
        // Steady state: 0 = -1/τ * x + K/τ * u  → x = K*u, y = K*u
        let gain = 3.0f32;
        let tau = 0.5f32;
        let m = first_order_to_ss(gain, tau);
        // Run for many steps to reach steady state
        let u = [1.0f32];
        let mut x = [0.0f32; MAX_STATES];
        for _ in 0..10_000 {
            let nx = m.step_rk4(&x, &u, 0.001);
            x[0] = nx[0];
        }
        let y = m.output(&x, &u);
        assert!(approx_eq(y[0], gain, 0.01), "got {}", y[0]);
    }

    // ------------------------------------------------------------------
    // second_order_to_ss
    // ------------------------------------------------------------------

    #[test]
    fn second_order_dimensions() {
        let m = second_order_to_ss(1.0, 0.7);
        assert_eq!(m.n, 2);
        assert_eq!(m.m, 1);
        assert_eq!(m.p, 1);
    }

    #[test]
    fn second_order_steady_state() {
        // H(s) = ωn² / (s² + 2ζωn s + ωn²)
        // At s=0: H(0) = ωn²/ωn² = 1.0
        let omega_n = 2.0f32;
        let zeta = 0.7f32;
        let m = second_order_to_ss(omega_n, zeta);
        let u = [1.0f32];
        let mut x = [0.0f32; MAX_STATES];
        for _ in 0..20_000 {
            let nx = m.step_rk4(&x, &u, 0.001);
            x[0] = nx[0];
            x[1] = nx[1];
        }
        let y = m.output(&x, &u);
        assert!(approx_eq(y[0], 1.0, 0.01), "got {}", y[0]);
    }

    // ------------------------------------------------------------------
    // is_stable_routh
    // ------------------------------------------------------------------

    #[test]
    fn stable_first_order() {
        // s + 1  →  a_poly = [1, 1]  (ascending: a0=1, a1=1)
        assert!(is_stable_routh(&[1.0, 1.0]));
    }

    #[test]
    fn unstable_first_order() {
        // s - 1  →  coeffs [−1, 1]  (mixed signs)
        assert!(!is_stable_routh(&[-1.0, 1.0]));
    }

    #[test]
    fn stable_second_order() {
        // s² + 3s + 2 = (s+1)(s+2)  → roots -1, -2  → stable
        // a_poly = [2, 3, 1]
        assert!(is_stable_routh(&[2.0, 3.0, 1.0]));
    }

    #[test]
    fn unstable_second_order_missing_coeff() {
        // s² - 1 = (s-1)(s+1)  → one root at +1  → unstable
        // a_poly = [-1, 0, 1]  (zero coefficient → unstable)
        assert!(!is_stable_routh(&[-1.0, 0.0, 1.0]));
    }

    #[test]
    fn stable_third_order() {
        // s³ + 6s² + 11s + 6 = (s+1)(s+2)(s+3)  → all roots negative
        // a_poly = [6, 11, 6, 1]
        assert!(is_stable_routh(&[6.0, 11.0, 6.0, 1.0]));
    }

    #[test]
    fn unstable_third_order() {
        // s³ + 2s² - s + 1  (mixed signs → unstable)
        assert!(!is_stable_routh(&[1.0, -1.0, 2.0, 1.0]));
    }

    // ------------------------------------------------------------------
    // tf_to_state_space smoke test
    // ------------------------------------------------------------------

    #[test]
    fn tf_integrator() {
        // H(s) = 1/s  → den = [0, 1], num = [1]
        // Singularity: den[0]=0 would violate all-same-sign, but we only test
        // the SS construction is dimension-correct here.
        let den = [0.0, 1.0]; // s
        let num = [1.0];
        let m = tf_to_state_space(&num, &den);
        assert_eq!(m.n, 1);
        assert_eq!(m.m, 1);
        assert_eq!(m.p, 1);
    }

    #[test]
    fn tf_to_ss_first_order_matches_helper() {
        // first_order_to_ss and tf_to_state_space should agree
        let gain = 2.0f32;
        let tau = 0.3f32;
        let m1 = first_order_to_ss(gain, tau);
        let m2 = tf_to_state_space(&[gain], &[1.0, tau]);
        // A matrices should agree
        assert!(approx_eq(m1.a[0][0], m2.a[0][0], 1e-5));
        assert!(approx_eq(m1.c[0][0], m2.c[0][0], 1e-5));
    }
}
