//! Linear and polynomial regression via least squares.

// ── Simple linear regression ──────────────────────────────────────

/// Fit y = a*x + b to data. Returns (a, b, r_squared).
pub fn linear_regression(xs: &[f32], ys: &[f32]) -> (f32, f32, f32) {
    assert_eq!(xs.len(), ys.len(), "xs and ys must have the same length");
    let n = xs.len();
    assert!(n >= 2, "Need at least 2 data points for linear regression");

    let n_f = n as f32;
    let sum_x: f32 = xs.iter().sum();
    let sum_y: f32 = ys.iter().sum();
    let sum_xx: f32 = xs.iter().map(|&x| x * x).sum();
    let sum_xy: f32 = xs.iter().zip(ys.iter()).map(|(&x, &y)| x * y).sum();

    let denom = n_f * sum_xx - sum_x * sum_x;
    let (a, b) = if denom.abs() < f32::EPSILON {
        (0.0_f32, sum_y / n_f)
    } else {
        let a = (n_f * sum_xy - sum_x * sum_y) / denom;
        let b = (sum_y - a * sum_x) / n_f;
        (a, b)
    };

    let predicted: Vec<f32> = xs.iter().map(|&x| linear_predict(a, b, x)).collect();
    let r2 = r_squared(&predicted, ys);

    (a, b, r2)
}

/// Predict y from a fitted line.
#[inline]
pub fn linear_predict(a: f32, b: f32, x: f32) -> f32 {
    a * x + b
}

// ── Polynomial regression ────────────────────────────────────────

/// Fit y = c[0] + c[1]*x + c[2]*x² + ... + c[degree]*x^degree
/// via normal equations (Vandermonde matrix, Gaussian elimination).
/// Returns coefficient vector of length (degree+1).
pub fn polynomial_regression(xs: &[f32], ys: &[f32], degree: usize) -> Vec<f32> {
    assert_eq!(xs.len(), ys.len(), "xs and ys must have the same length");
    let n = xs.len();
    let m = degree + 1; // number of coefficients
    assert!(n >= m, "Need at least (degree+1) data points for polynomial regression of given degree");

    // Build Vandermonde matrix V: n x m, V[i][j] = x_i^j
    let mut v: Vec<Vec<f64>> = vec![vec![0.0_f64; m]; n];
    for i in 0..n {
        let x = xs[i] as f64;
        let mut xpow = 1.0_f64;
        for j in 0..m {
            v[i][j] = xpow;
            xpow *= x;
        }
    }

    // Compute A = V^T * V (m x m) and rhs = V^T * y (m)
    let mut a_mat: Vec<Vec<f64>> = vec![vec![0.0_f64; m]; m];
    let mut rhs: Vec<f64> = vec![0.0_f64; m];

    for row in 0..m {
        for col in 0..m {
            let mut s = 0.0_f64;
            for i in 0..n {
                s += v[i][row] * v[i][col];
            }
            a_mat[row][col] = s;
        }
        let mut s = 0.0_f64;
        for i in 0..n {
            s += v[i][row] * ys[i] as f64;
        }
        rhs[row] = s;
    }

    // Gaussian elimination with partial pivoting on augmented matrix [A | rhs]
    let mut aug: Vec<Vec<f64>> = a_mat
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.push(rhs[i]);
            r
        })
        .collect();

    for col in 0..m {
        // Find pivot row
        let mut max_val = aug[col][col].abs();
        let mut max_row = col;
        for row in (col + 1)..m {
            if aug[row][col].abs() > max_val {
                max_val = aug[row][col].abs();
                max_row = row;
            }
        }
        // Swap
        aug.swap(col, max_row);

        let pivot = aug[col][col];
        if pivot.abs() < 1e-12 {
            // Singular or near-singular: return zero coefficients for this column
            continue;
        }

        // Eliminate below
        for row in (col + 1)..m {
            let factor = aug[row][col] / pivot;
            for k in col..=m {
                let val = aug[col][k] * factor;
                aug[row][k] -= val;
            }
        }
    }

    // Back substitution
    let mut coeffs = vec![0.0_f64; m];
    for i in (0..m).rev() {
        let mut sum = aug[i][m];
        for j in (i + 1)..m {
            sum -= aug[i][j] * coeffs[j];
        }
        let diag = aug[i][i];
        coeffs[i] = if diag.abs() < 1e-12 { 0.0 } else { sum / diag };
    }

    coeffs.iter().map(|&c| c as f32).collect()
}

/// Evaluate a polynomial at x given coefficients c.
/// c[0] + c[1]*x + c[2]*x^2 + ...
pub fn polynomial_eval(c: &[f32], x: f32) -> f32 {
    // Horner's method
    if c.is_empty() {
        return 0.0;
    }
    let mut result = 0.0_f32;
    for &coeff in c.iter().rev() {
        result = result * x + coeff;
    }
    result
}

// ── Statistics ───────────────────────────────────────────────────

pub fn mean(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    data.iter().sum::<f32>() / data.len() as f32
}

pub fn variance(data: &[f32]) -> f32 {
    if data.len() < 2 {
        return 0.0;
    }
    let m = mean(data);
    data.iter().map(|&x| (x - m) * (x - m)).sum::<f32>() / data.len() as f32
}

pub fn std_dev(data: &[f32]) -> f32 {
    variance(data).sqrt()
}

pub fn covariance(xs: &[f32], ys: &[f32]) -> f32 {
    assert_eq!(xs.len(), ys.len(), "xs and ys must have the same length");
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let mx = mean(xs);
    let my = mean(ys);
    xs.iter()
        .zip(ys.iter())
        .map(|(&x, &y)| (x - mx) * (y - my))
        .sum::<f32>()
        / n as f32
}

pub fn pearson_r(xs: &[f32], ys: &[f32]) -> f32 {
    let sx = std_dev(xs);
    let sy = std_dev(ys);
    if sx < f32::EPSILON || sy < f32::EPSILON {
        return 0.0;
    }
    covariance(xs, ys) / (sx * sy)
}

/// Root mean squared error between predicted and actual.
pub fn rmse(predicted: &[f32], actual: &[f32]) -> f32 {
    assert_eq!(predicted.len(), actual.len(), "predicted and actual must have the same length");
    let n = predicted.len();
    if n == 0 {
        return 0.0;
    }
    let sum_sq: f32 = predicted
        .iter()
        .zip(actual.iter())
        .map(|(&p, &a)| (p - a) * (p - a))
        .sum();
    (sum_sq / n as f32).sqrt()
}

/// Mean absolute error.
pub fn mae(predicted: &[f32], actual: &[f32]) -> f32 {
    assert_eq!(predicted.len(), actual.len(), "predicted and actual must have the same length");
    let n = predicted.len();
    if n == 0 {
        return 0.0;
    }
    predicted
        .iter()
        .zip(actual.iter())
        .map(|(&p, &a)| (p - a).abs())
        .sum::<f32>()
        / n as f32
}

/// R² coefficient of determination.
/// r² = 1 - SS_res/SS_tot where SS_res = Σ(y_i - ŷ_i)² and SS_tot = Σ(y_i - ȳ)².
pub fn r_squared(predicted: &[f32], actual: &[f32]) -> f32 {
    assert_eq!(predicted.len(), actual.len(), "predicted and actual must have the same length");
    let n = actual.len();
    if n == 0 {
        return 0.0;
    }
    let y_mean = mean(actual);
    let ss_res: f32 = predicted
        .iter()
        .zip(actual.iter())
        .map(|(&yhat, &y)| (y - yhat) * (y - yhat))
        .sum();
    let ss_tot: f32 = actual.iter().map(|&y| (y - y_mean) * (y - y_mean)).sum();
    if ss_tot < f32::EPSILON {
        // All actual values are equal; perfect fit only if residuals are zero
        if ss_res < f32::EPSILON {
            1.0
        } else {
            0.0
        }
    } else {
        1.0 - ss_res / ss_tot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_regression_perfect_line() {
        // y = 2x + 1
        let xs: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let ys: Vec<f32> = xs.iter().map(|&x| 2.0 * x + 1.0).collect();
        let (a, b, r2) = linear_regression(&xs, &ys);
        assert!((a - 2.0).abs() < 1e-4, "slope should be 2.0, got {}", a);
        assert!((b - 1.0).abs() < 1e-4, "intercept should be 1.0, got {}", b);
        assert!((r2 - 1.0).abs() < 1e-4, "r² should be 1.0, got {}", r2);
    }

    #[test]
    fn test_linear_predict() {
        assert!((linear_predict(3.0, -1.0, 2.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn test_polynomial_regression_degree1() {
        // Should match linear regression
        let xs: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0];
        let ys: Vec<f32> = xs.iter().map(|&x| 3.0 * x + 0.5).collect();
        let c = polynomial_regression(&xs, &ys, 1);
        assert_eq!(c.len(), 2);
        assert!((c[0] - 0.5).abs() < 1e-3, "c[0] should be 0.5, got {}", c[0]);
        assert!((c[1] - 3.0).abs() < 1e-3, "c[1] should be 3.0, got {}", c[1]);
    }

    #[test]
    fn test_polynomial_regression_degree2() {
        // y = x^2 - 2x + 1
        let xs: Vec<f32> = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let ys: Vec<f32> = xs.iter().map(|&x| x * x - 2.0 * x + 1.0).collect();
        let c = polynomial_regression(&xs, &ys, 2);
        assert_eq!(c.len(), 3);
        assert!((c[0] - 1.0).abs() < 1e-3, "c[0] should be 1.0, got {}", c[0]);
        assert!((c[1] - (-2.0)).abs() < 1e-3, "c[1] should be -2.0, got {}", c[1]);
        assert!((c[2] - 1.0).abs() < 1e-3, "c[2] should be 1.0, got {}", c[2]);
    }

    #[test]
    fn test_polynomial_eval() {
        // 1 + 2x + 3x^2 at x=2 => 1 + 4 + 12 = 17
        let c = vec![1.0_f32, 2.0, 3.0];
        assert!((polynomial_eval(&c, 2.0) - 17.0).abs() < 1e-5);
    }

    #[test]
    fn test_mean() {
        assert!((mean(&[1.0, 2.0, 3.0, 4.0, 5.0]) - 3.0).abs() < 1e-6);
        assert_eq!(mean(&[]), 0.0);
    }

    #[test]
    fn test_variance() {
        // population variance of [2, 4, 4, 4, 5, 5, 7, 9] = 4.0
        let data = vec![2.0_f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((variance(&data) - 4.0).abs() < 1e-4);
    }

    #[test]
    fn test_std_dev() {
        let data = vec![2.0_f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((std_dev(&data) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn test_pearson_r_perfect_positive() {
        let xs: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys: Vec<f32> = xs.iter().map(|&x| 2.0 * x + 1.0).collect();
        assert!((pearson_r(&xs, &ys) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_rmse() {
        let predicted = vec![2.0_f32, 3.0, 4.0];
        let actual = vec![1.0_f32, 3.0, 5.0];
        // errors: 1, 0, -1 => mse = 2/3 => rmse = sqrt(2/3)
        let expected = (2.0_f32 / 3.0).sqrt();
        assert!((rmse(&predicted, &actual) - expected).abs() < 1e-5);
    }

    #[test]
    fn test_mae() {
        let predicted = vec![2.0_f32, 3.0, 4.0];
        let actual = vec![1.0_f32, 3.0, 5.0];
        // |1| + |0| + |-1| = 2, /3 = 0.667
        assert!((mae(&predicted, &actual) - 2.0 / 3.0).abs() < 1e-5);
    }

    #[test]
    fn test_r_squared_perfect() {
        let actual = vec![1.0_f32, 2.0, 3.0, 4.0];
        assert!((r_squared(&actual, &actual) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_r_squared_zero() {
        let actual = vec![1.0_f32, 2.0, 3.0, 4.0];
        // predict always the mean => r² = 0
        let m = mean(&actual);
        let predicted = vec![m; 4];
        assert!(r_squared(&predicted, &actual).abs() < 1e-5);
    }
}
