//! Interpolation utilities — linear, cubic spline, bilinear table.

// ── Linear ───────────────────────────────────────────────────────

/// Linear interpolation between two scalars.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Inverse lerp — find t given value between a and b.
pub fn inv_lerp(a: f32, b: f32, value: f32) -> f32 {
    let denom = b - a;
    if denom.abs() < f32::EPSILON {
        0.0
    } else {
        (value - a) / denom
    }
}

/// Bilinear interpolation on a unit square.
/// v00..v11 are values at corners (x=0,y=0), (x=1,y=0), (x=0,y=1), (x=1,y=1).
pub fn bilinear(v00: f32, v10: f32, v01: f32, v11: f32, tx: f32, ty: f32) -> f32 {
    let bottom = lerp(v00, v10, tx);
    let top = lerp(v01, v11, tx);
    lerp(bottom, top, ty)
}

/// Look up value in a sorted 1D table (xs, ys must be same length).
/// Returns the linearly interpolated y for a given x (clamps at boundaries).
pub fn table_lookup_1d(xs: &[f32], ys: &[f32], x: f32) -> f32 {
    assert_eq!(xs.len(), ys.len(), "xs and ys must have the same length");
    let n = xs.len();
    assert!(n >= 2, "table must have at least 2 points");

    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }

    // Binary search for the interval [xs[i], xs[i+1]] containing x.
    let mut lo = 0usize;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let t = (x - xs[lo]) / (xs[hi] - xs[lo]);
    lerp(ys[lo], ys[hi], t)
}

// ── Cubic Hermite spline ─────────────────────────────────────────

/// Cubic Hermite spline interpolation.
/// p0, p1 — endpoint values; m0, m1 — endpoint tangents.
pub fn cubic_hermite(p0: f32, m0: f32, p1: f32, m1: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    h00 * p0 + h10 * m0 + h01 * p1 + h11 * m1
}

/// Smoothstep (Ken Perlin's cubic): 3t² - 2t³
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smootherstep: 6t⁵ - 15t⁴ + 10t³
pub fn smootherstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

// ── Natural cubic spline ──────────────────────────────────────────

/// Build a natural cubic spline from sorted (x, y) data points.
/// Returns the second-derivative array (used with spline_eval).
///
/// Uses the tridiagonal Thomas algorithm.  Natural boundary conditions:
/// second derivative is zero at both endpoints.
pub fn spline_build(xs: &[f32], ys: &[f32]) -> Vec<f32> {
    let n = xs.len();
    assert_eq!(n, ys.len(), "xs and ys must have the same length");
    assert!(n >= 2, "spline requires at least 2 points");

    if n == 2 {
        // Linear segment — second derivatives are both zero.
        return vec![0.0; n];
    }

    // We solve for the n second derivatives d[0..n].
    // Natural conditions: d[0] = 0, d[n-1] = 0.
    // Interior equations (i = 1..n-2):
    //   h[i-1]*d[i-1] + 2*(h[i-1]+h[i])*d[i] + h[i]*d[i+1]
    //     = 6 * ((y[i+1]-y[i])/h[i] - (y[i]-y[i-1])/h[i-1])

    let m = n - 2; // number of interior unknowns
    if m == 0 {
        return vec![0.0; n];
    }

    let mut h = vec![0.0f32; n - 1];
    for i in 0..n - 1 {
        h[i] = xs[i + 1] - xs[i];
        assert!(h[i] > 0.0, "xs must be strictly increasing");
    }

    // Build the right-hand side for interior nodes.
    let mut rhs = vec![0.0f32; m];
    for i in 0..m {
        let idx = i + 1; // interior node index in original arrays
        rhs[i] = 6.0
            * ((ys[idx + 1] - ys[idx]) / h[idx] - (ys[idx] - ys[idx - 1]) / h[idx - 1]);
    }

    // Thomas algorithm (forward sweep + back substitution) for the tridiagonal
    // system with:
    //   lower diagonal: h[i-1]   (for i=1..m-1 in interior indexing)
    //   main  diagonal: 2*(h[i-1]+h[i])
    //   upper diagonal: h[i]

    // Allocate working arrays.
    let mut diag = vec![0.0f32; m];
    let mut upper = vec![0.0f32; m - 1];
    let mut rhs_w = rhs.clone();

    for i in 0..m {
        let orig = i + 1;
        diag[i] = 2.0 * (h[orig - 1] + h[orig]);
    }
    for i in 0..m - 1 {
        let orig = i + 1;
        upper[i] = h[orig]; // upper diagonal = h[orig] = h between interior[i] and interior[i+1]
    }

    // Forward sweep.
    let mut c_prime = vec![0.0f32; m - 1];
    let mut d_prime = vec![0.0f32; m];

    c_prime[0] = upper[0] / diag[0];
    d_prime[0] = rhs_w[0] / diag[0];

    for i in 1..m {
        let lower_i = h[i]; // h[i] is the sub-diagonal connecting interior[i] to interior[i-1]
        let denom = diag[i] - lower_i * if i > 0 { c_prime[i - 1] } else { 0.0 };
        d_prime[i] = (rhs_w[i] - lower_i * d_prime[i - 1]) / denom;
        if i < m - 1 {
            c_prime[i] = upper[i] / denom;
        }
    }

    // Back substitution.
    let mut sol = vec![0.0f32; m];
    sol[m - 1] = d_prime[m - 1];
    if m > 1 {
        for i in (0..m - 1).rev() {
            sol[i] = d_prime[i] - c_prime[i] * sol[i + 1];
        }
    }

    // Assemble full second-derivative array with natural boundary values.
    let mut d2 = vec![0.0f32; n];
    for i in 0..m {
        d2[i + 1] = sol[i];
    }
    // d2[0] and d2[n-1] remain 0.0 (natural conditions).
    d2
}

/// Evaluate the natural cubic spline at a given x.
pub fn spline_eval(xs: &[f32], ys: &[f32], d2: &[f32], x: f32) -> f32 {
    let n = xs.len();
    assert_eq!(n, ys.len());
    assert_eq!(n, d2.len());
    assert!(n >= 2);

    // Clamp to the domain.
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }

    // Binary search for interval.
    let mut lo = 0usize;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    let h = xs[hi] - xs[lo];
    let a = (xs[hi] - x) / h;
    let b = (x - xs[lo]) / h;

    // Standard natural cubic spline evaluation formula.
    a * ys[lo]
        + b * ys[hi]
        + ((a * a * a - a) * d2[lo] + (b * b * b - b) * d2[hi]) * (h * h) / 6.0
}

// ── Bilinear table (2D) ───────────────────────────────────────────

/// Bilinear interpolation in a 2D regular grid.
/// xs, ys — axis values (sorted); zs — row-major grid values [ny][nx].
/// nx = xs.len(), ny = ys.len(), zs.len() == nx * ny.
pub fn table_lookup_2d(xs: &[f32], ys: &[f32], zs: &[f32], x: f32, y: f32) -> f32 {
    let nx = xs.len();
    let ny = ys.len();
    assert!(nx >= 2, "xs must have at least 2 entries");
    assert!(ny >= 2, "ys must have at least 2 entries");
    assert_eq!(zs.len(), nx * ny, "zs.len() must equal nx * ny");

    // Helper: find lower index and fractional position along an axis.
    let find_interval = |axis: &[f32], v: f32| -> (usize, f32) {
        let len = axis.len();
        if v <= axis[0] {
            return (0, 0.0);
        }
        if v >= axis[len - 1] {
            return (len - 2, 1.0);
        }
        let mut lo = 0usize;
        let mut hi = len - 1;
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if axis[mid] <= v {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t = (v - axis[lo]) / (axis[hi] - axis[lo]);
        (lo, t)
    };

    let (xi, tx) = find_interval(xs, x);
    let (yi, ty) = find_interval(ys, y);

    // Row-major: z[iy][ix] = zs[iy * nx + ix]
    let z00 = zs[yi * nx + xi];
    let z10 = zs[yi * nx + (xi + 1)];
    let z01 = zs[(yi + 1) * nx + xi];
    let z11 = zs[(yi + 1) * nx + (xi + 1)];

    bilinear(z00, z10, z01, z11, tx, ty)
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp_endpoints() {
        assert!((lerp(0.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 1.0) - 10.0).abs() < 1e-6);
        assert!((lerp(0.0, 10.0, 0.5) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_inv_lerp_round_trip() {
        let a = 2.0f32;
        let b = 8.0f32;
        let v = 5.0f32;
        let t = inv_lerp(a, b, v);
        assert!((lerp(a, b, t) - v).abs() < 1e-5);
    }

    #[test]
    fn test_bilinear_corners() {
        assert!((bilinear(1.0, 2.0, 3.0, 4.0, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((bilinear(1.0, 2.0, 3.0, 4.0, 1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((bilinear(1.0, 2.0, 3.0, 4.0, 0.0, 1.0) - 3.0).abs() < 1e-6);
        assert!((bilinear(1.0, 2.0, 3.0, 4.0, 1.0, 1.0) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_table_lookup_1d_exact() {
        let xs = [0.0f32, 1.0, 2.0, 3.0];
        let ys = [0.0f32, 1.0, 4.0, 9.0];
        assert!((table_lookup_1d(&xs, &ys, 1.0) - 1.0).abs() < 1e-6);
        assert!((table_lookup_1d(&xs, &ys, 2.0) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_table_lookup_1d_clamp() {
        let xs = [0.0f32, 1.0];
        let ys = [5.0f32, 10.0];
        assert!((table_lookup_1d(&xs, &ys, -1.0) - 5.0).abs() < 1e-6);
        assert!((table_lookup_1d(&xs, &ys, 2.0) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_smoothstep_clamp() {
        assert!((smoothstep(-1.0) - 0.0).abs() < 1e-6);
        assert!((smoothstep(2.0) - 1.0).abs() < 1e-6);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_smootherstep_midpoint() {
        assert!((smootherstep(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_cubic_hermite_endpoints() {
        assert!((cubic_hermite(1.0, 0.0, 2.0, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((cubic_hermite(1.0, 0.0, 2.0, 0.0, 1.0) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_spline_linear_data() {
        // A perfectly linear dataset should be reproduced exactly.
        let xs: Vec<f32> = (0..=5).map(|i| i as f32).collect();
        let ys: Vec<f32> = xs.iter().map(|&x| 2.0 * x + 1.0).collect();
        let d2 = spline_build(&xs, &ys);
        for i in 0..xs.len() {
            let v = spline_eval(&xs, &ys, &d2, xs[i]);
            assert!((v - ys[i]).abs() < 1e-4, "linear spline failed at i={i}: {v} vs {}", ys[i]);
        }
        // Midpoint interpolation should also be linear.
        let mid = spline_eval(&xs, &ys, &d2, 2.5);
        assert!((mid - 6.0).abs() < 1e-4, "midpoint: {mid}");
    }

    #[test]
    fn test_spline_clamping() {
        let xs = [0.0f32, 1.0, 2.0];
        let ys = [0.0f32, 1.0, 0.0];
        let d2 = spline_build(&xs, &ys);
        assert!((spline_eval(&xs, &ys, &d2, -1.0) - 0.0).abs() < 1e-6);
        assert!((spline_eval(&xs, &ys, &d2, 3.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_table_lookup_2d_corners() {
        let xs = [0.0f32, 1.0];
        let ys = [0.0f32, 1.0];
        // zs row-major [ny=2][nx=2]: z00=1, z10=2, z01=3, z11=4
        let zs = [1.0f32, 2.0, 3.0, 4.0];
        assert!((table_lookup_2d(&xs, &ys, &zs, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((table_lookup_2d(&xs, &ys, &zs, 1.0, 0.0) - 2.0).abs() < 1e-6);
        assert!((table_lookup_2d(&xs, &ys, &zs, 0.0, 1.0) - 3.0).abs() < 1e-6);
        assert!((table_lookup_2d(&xs, &ys, &zs, 1.0, 1.0) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn test_table_lookup_2d_center() {
        let xs = [0.0f32, 2.0];
        let ys = [0.0f32, 2.0];
        let zs = [0.0f32, 0.0, 0.0, 4.0]; // only z11=4 is non-zero
        // At center (1,1): bilinear gives 0*0.25 + 0*0.25 + 0*0.25 + 4*0.25 = 1.0
        let v = table_lookup_2d(&xs, &ys, &zs, 1.0, 1.0);
        assert!((v - 1.0).abs() < 1e-6, "center value: {v}");
    }

    #[test]
    fn test_table_lookup_2d_clamp() {
        let xs = [0.0f32, 1.0];
        let ys = [0.0f32, 1.0];
        let zs = [1.0f32, 2.0, 3.0, 4.0];
        // Outside on the x-low side should clamp to xs[0] behaviour.
        let v = table_lookup_2d(&xs, &ys, &zs, -5.0, 0.0);
        assert!((v - 1.0).abs() < 1e-6, "x-clamp low: {v}");
        // Outside on the y-high side should clamp to ys[ny-1].
        let v = table_lookup_2d(&xs, &ys, &zs, 0.5, 5.0);
        assert!((v - 3.5).abs() < 1e-6, "y-clamp high: {v}");
    }
}
