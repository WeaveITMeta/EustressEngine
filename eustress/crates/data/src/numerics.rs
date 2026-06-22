//! Numerical analysis over columns (Data Platform P4) — descriptive stats,
//! derivative, integral, interpolation, and least-squares curve fitting.
//!
//! Pure `std` — no dependencies, always compiled. Operates on raw `f64` (and
//! `i64` widened to `f64`); nulls and non-finite values are dropped. **Unit /
//! dimension propagation is the engine's job**, not this leaf's: the engine
//! reads the column `dimension` strings and composes results via
//! `common::dimension::Dimension` (e.g. derivative → `y.dim / x.dim`). This
//! module is the dimensionless numeric kernel those callers wrap.

use crate::{ColumnData, DataError, Result};

/// Widen a numeric column to `Vec<Option<f64>>` (F64 as-is, I64 cast). Errors
/// on a non-numeric column.
pub(crate) fn as_f64_opt(col: &ColumnData) -> Result<Vec<Option<f64>>> {
    match col {
        ColumnData::F64(v) => Ok(v.clone()),
        ColumnData::I64(v) => Ok(v.iter().map(|o| o.map(|i| i as f64)).collect()),
        other => Err(DataError::Schema(format!(
            "numeric op requires an F64/I64 column, got {:?}",
            other.dtype()
        ))),
    }
}

/// Pair two numeric columns, keeping only rows where BOTH are present + finite.
pub(crate) fn paired_xy(x: &ColumnData, y: &ColumnData) -> Result<(Vec<f64>, Vec<f64>)> {
    let xv = as_f64_opt(x)?;
    let yv = as_f64_opt(y)?;
    if xv.len() != yv.len() {
        return Err(DataError::Schema(format!(
            "x/y length mismatch: {} vs {}",
            xv.len(),
            yv.len()
        )));
    }
    let mut xs = Vec::with_capacity(xv.len());
    let mut ys = Vec::with_capacity(yv.len());
    for (a, b) in xv.into_iter().zip(yv) {
        if let (Some(a), Some(b)) = (a, b) {
            if a.is_finite() && b.is_finite() {
                xs.push(a);
                ys.push(b);
            }
        }
    }
    Ok((xs, ys))
}

/// Descriptive statistics over a numeric column (nulls / non-finite dropped).
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    /// Number of finite values.
    pub count: usize,
    /// Arithmetic mean.
    pub mean: f64,
    /// Minimum.
    pub min: f64,
    /// Maximum.
    pub max: f64,
    /// Sum.
    pub sum: f64,
    /// Population variance (divisor `count`).
    pub variance: f64,
    /// Population standard deviation.
    pub std_dev: f64,
}

/// Compute [`Stats`] for a numeric column. Errors if no finite values remain.
pub fn stats(col: &ColumnData) -> Result<Stats> {
    let vals: Vec<f64> = as_f64_opt(col)?
        .into_iter()
        .flatten()
        .filter(|x| x.is_finite())
        .collect();
    if vals.is_empty() {
        return Err(DataError::Schema("stats: column has no finite values".into()));
    }
    let count = vals.len();
    let sum: f64 = vals.iter().sum();
    let mean = sum / count as f64;
    let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
    let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let variance = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
    Ok(Stats {
        count,
        mean,
        min,
        max,
        sum,
        variance,
        std_dev: variance.sqrt(),
    })
}

/// Trapezoidal integral of `y` with respect to `x` over the cleaned series.
/// Points are taken in column order (no internal sort), so `x` should be
/// monotonic. Returns `0.0` for fewer than two points.
pub fn integral(x: &ColumnData, y: &ColumnData) -> Result<f64> {
    let (xs, ys) = paired_xy(x, y)?;
    if xs.len() < 2 {
        return Ok(0.0);
    }
    let mut area = 0.0;
    for i in 1..xs.len() {
        area += (xs[i] - xs[i - 1]) * (ys[i] + ys[i - 1]) / 2.0;
    }
    Ok(area)
}

/// Numerical derivative `dy/dx` at each cleaned point (central difference
/// interior, one-sided at the ends). Length equals the cleaned-series length.
pub fn derivative(x: &ColumnData, y: &ColumnData) -> Result<Vec<f64>> {
    let (xs, ys) = paired_xy(x, y)?;
    let n = xs.len();
    if n < 2 {
        return Err(DataError::Schema("derivative needs >= 2 finite points".into()));
    }
    let dx = |a: usize, b: usize| xs[b] - xs[a];
    if dx(0, 1) == 0.0 {
        return Err(DataError::Schema("derivative: repeated x value".into()));
    }
    let mut d = vec![0.0; n];
    d[0] = (ys[1] - ys[0]) / dx(0, 1);
    d[n - 1] = (ys[n - 1] - ys[n - 2]) / dx(n - 2, n - 1);
    for i in 1..n - 1 {
        let denom = dx(i - 1, i + 1);
        if denom == 0.0 {
            return Err(DataError::Schema("derivative: repeated x value".into()));
        }
        d[i] = (ys[i + 1] - ys[i - 1]) / denom;
    }
    Ok(d)
}

/// Linear interpolation of `y` at `at`, over the cleaned series (assumed sorted
/// ascending in `x`). Clamps to the endpoints outside the range.
pub fn interpolate_linear(x: &ColumnData, y: &ColumnData, at: f64) -> Result<f64> {
    let (xs, ys) = paired_xy(x, y)?;
    let n = xs.len();
    if n < 2 {
        return Err(DataError::Schema("interpolate needs >= 2 finite points".into()));
    }
    if at <= xs[0] {
        return Ok(ys[0]);
    }
    if at >= xs[n - 1] {
        return Ok(ys[n - 1]);
    }
    for i in 1..n {
        if at <= xs[i] {
            let span = xs[i] - xs[i - 1];
            if span == 0.0 {
                return Ok(ys[i]);
            }
            let t = (at - xs[i - 1]) / span;
            return Ok(ys[i - 1] + t * (ys[i] - ys[i - 1]));
        }
    }
    Ok(ys[n - 1])
}

/// Result of an ordinary least-squares straight-line fit `y = slope·x + intercept`.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearFit {
    /// Slope.
    pub slope: f64,
    /// Intercept.
    pub intercept: f64,
    /// Coefficient of determination R².
    pub r_squared: f64,
}

/// Ordinary least-squares straight-line fit. Errors if `x` has no spread.
pub fn fit_linear(x: &ColumnData, y: &ColumnData) -> Result<LinearFit> {
    let (xs, ys) = paired_xy(x, y)?;
    let n = xs.len();
    if n < 2 {
        return Err(DataError::Schema("fit_linear needs >= 2 finite points".into()));
    }
    let nf = n as f64;
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let sxx: f64 = xs.iter().map(|v| v * v).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(a, b)| a * b).sum();
    let denom = nf * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return Err(DataError::Schema("fit_linear: x has no spread (vertical fit)".into()));
    }
    let slope = (nf * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / nf;
    let ybar = sy / nf;
    let ss_tot: f64 = ys.iter().map(|v| (v - ybar).powi(2)).sum();
    let ss_res: f64 = xs
        .iter()
        .zip(&ys)
        .map(|(a, b)| (b - (slope * a + intercept)).powi(2))
        .sum();
    let r_squared = if ss_tot.abs() < 1e-12 {
        1.0
    } else {
        1.0 - ss_res / ss_tot
    };
    Ok(LinearFit {
        slope,
        intercept,
        r_squared,
    })
}

/// Polynomial least-squares fit of the given `degree`, returning coefficients
/// `[c0, c1, … c_degree]` for `c0 + c1·x + c2·x² + …`.
///
/// Solves the normal equations by Gaussian elimination with partial pivoting.
/// Fine for low degree; for high degree the normal equations are
/// ill-conditioned (a QR/SVD path is future work).
pub fn fit_poly(x: &ColumnData, y: &ColumnData, degree: usize) -> Result<Vec<f64>> {
    let (xs, ys) = paired_xy(x, y)?;
    let n = xs.len();
    let m = degree + 1;
    if n < m {
        return Err(DataError::Schema(format!(
            "fit_poly degree {degree} needs >= {m} finite points, got {n}"
        )));
    }
    // Power sums Σ x^s for s in 0..=2·degree, and the RHS Σ y·x^j.
    let mut powsum = vec![0.0; 2 * degree + 1];
    for &xi in &xs {
        let mut p = 1.0;
        for s in powsum.iter_mut() {
            *s += p;
            p *= xi;
        }
    }
    let mut b = vec![0.0; m];
    for (xi, yi) in xs.iter().zip(&ys) {
        let mut p = 1.0;
        for bj in b.iter_mut() {
            *bj += yi * p;
            p *= xi;
        }
    }
    let mut a = vec![vec![0.0; m]; m];
    for (j, row) in a.iter_mut().enumerate() {
        for (k, cell) in row.iter_mut().enumerate() {
            *cell = powsum[j + k];
        }
    }
    gaussian_solve(&mut a, &mut b)?;
    Ok(b)
}

/// In-place Gaussian elimination with partial pivoting: solves `A·x = b`,
/// writing the solution into `b`.
fn gaussian_solve(a: &mut [Vec<f64>], b: &mut [f64]) -> Result<()> {
    let n = b.len();
    for col in 0..n {
        // Partial pivot: largest |a[row][col]| at or below the diagonal.
        let mut pivot = col;
        let mut best = a[col][col].abs();
        for row in (col + 1)..n {
            let v = a[row][col].abs();
            if v > best {
                best = v;
                pivot = row;
            }
        }
        if best < 1e-12 {
            return Err(DataError::Schema("fit_poly: singular normal matrix".into()));
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        // Eliminate below.
        for row in (col + 1)..n {
            let factor = a[row][col] / a[col][col];
            for k in col..n {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }
    // Back-substitution.
    for col in (0..n).rev() {
        let mut s = b[col];
        for k in (col + 1)..n {
            s -= a[col][k] * b[k];
        }
        b[col] = s / a[col][col];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64col(v: &[f64]) -> ColumnData {
        ColumnData::F64(v.iter().map(|&x| Some(x)).collect())
    }

    #[test]
    fn stats_drops_nulls_and_computes() {
        let col = ColumnData::F64(vec![Some(2.0), None, Some(4.0), Some(6.0)]);
        let s = stats(&col).unwrap();
        assert_eq!(s.count, 3);
        assert_eq!(s.sum, 12.0);
        assert_eq!(s.mean, 4.0);
        assert_eq!(s.min, 2.0);
        assert_eq!(s.max, 6.0);
        // population variance of {2,4,6} = ((4)+(0)+(4))/3 = 8/3
        assert!((s.variance - 8.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn integral_of_constant_and_line() {
        // ∫ 2 dx from 0..4 = 8
        let x = f64col(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let y = f64col(&[2.0, 2.0, 2.0, 2.0, 2.0]);
        assert!((integral(&x, &y).unwrap() - 8.0).abs() < 1e-12);
        // ∫ x dx from 0..4 = 8
        let y2 = f64col(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        assert!((integral(&x, &y2).unwrap() - 8.0).abs() < 1e-12);
    }

    #[test]
    fn derivative_of_line_is_slope() {
        let x = f64col(&[0.0, 1.0, 2.0, 3.0]);
        let y = f64col(&[1.0, 3.0, 5.0, 7.0]); // y = 2x + 1
        for d in derivative(&x, &y).unwrap() {
            assert!((d - 2.0).abs() < 1e-12);
        }
    }

    #[test]
    fn interpolate_midpoint() {
        let x = f64col(&[0.0, 10.0]);
        let y = f64col(&[0.0, 100.0]);
        assert!((interpolate_linear(&x, &y, 2.5).unwrap() - 25.0).abs() < 1e-12);
        assert_eq!(interpolate_linear(&x, &y, -5.0).unwrap(), 0.0); // clamp low
        assert_eq!(interpolate_linear(&x, &y, 99.0).unwrap(), 100.0); // clamp high
    }

    #[test]
    fn linear_fit_recovers_known_line() {
        let x = f64col(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let y = f64col(&[1.0, 3.0, 5.0, 7.0, 9.0]); // y = 2x + 1 exactly
        let f = fit_linear(&x, &y).unwrap();
        assert!((f.slope - 2.0).abs() < 1e-9);
        assert!((f.intercept - 1.0).abs() < 1e-9);
        assert!((f.r_squared - 1.0).abs() < 1e-9);
    }

    #[test]
    fn poly_fit_degree1_matches_linear_and_degree2_recovers_quadratic() {
        let x = f64col(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let yl = f64col(&[1.0, 3.0, 5.0, 7.0, 9.0]);
        let c1 = fit_poly(&x, &yl, 1).unwrap();
        assert!((c1[0] - 1.0).abs() < 1e-7 && (c1[1] - 2.0).abs() < 1e-7);
        // y = 1 + 0·x + 2·x²
        let yq = f64col(&[1.0, 3.0, 9.0, 19.0, 33.0]);
        let c2 = fit_poly(&x, &yq, 2).unwrap();
        assert!((c2[0] - 1.0).abs() < 1e-6, "c0 {}", c2[0]);
        assert!((c2[1] - 0.0).abs() < 1e-6, "c1 {}", c2[1]);
        assert!((c2[2] - 2.0).abs() < 1e-6, "c2 {}", c2[2]);
    }
}
