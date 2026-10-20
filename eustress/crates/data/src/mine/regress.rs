//! Regression: least squares with an optional ridge penalty, and the LASSO.
//!
//! Least squares is exact (the normal equations, solved by Cholesky). The LASSO
//! trades a little of that fit for sparsity: its L1 penalty drives unhelpful
//! coefficients to exactly zero, which makes it the embedded feature selector
//! the literature points to when a model should say which inputs it needs.

use serde::Serialize;

use super::linalg::cholesky_solve;
use super::{Matrix, Scaler};
use crate::{DataError, Result};

/// A fitted regressor.
pub trait Regressor: Send + Sync {
    fn predict(&self, x: &Matrix) -> Result<Vec<f64>>;

    /// For a linear model, its coefficients (one per feature, in the
    /// features' own units) and intercept: the fitted equation itself.
    fn linear_terms(&self) -> Option<(&[f64], f64)> {
        None
    }
}

/// Hyperparameters that fit a [`Regressor`].
pub trait RegressorSpec {
    fn name(&self) -> &'static str;

    /// The name with every hyperparameter, so a report records exactly which
    /// model ran.
    fn describe(&self) -> String {
        self.name().to_string()
    }

    fn fit(&self, x: &Matrix, y: &[f64]) -> Result<Box<dyn Regressor>>;
}

fn check_training(x: &Matrix, y: &[f64]) -> Result<()> {
    if x.is_empty() {
        return Err(DataError::Schema("cannot fit a regressor on zero rows".into()));
    }
    if x.rows() != y.len() {
        return Err(DataError::Schema(format!(
            "{} feature rows but {} targets",
            x.rows(),
            y.len()
        )));
    }
    if y.iter().any(|v| !v.is_finite()) {
        return Err(DataError::Schema("the target holds a non-finite value".into()));
    }
    Ok(())
}

/// A linear model `y = intercept + Σ coefficient_j · x_j`, in the original
/// units of the features.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LinearModel {
    pub coefficients: Vec<f64>,
    pub intercept: f64,
}

impl Regressor for LinearModel {
    fn predict(&self, x: &Matrix) -> Result<Vec<f64>> {
        if x.cols() != self.coefficients.len() {
            return Err(DataError::Schema(format!(
                "model was fitted on {} features, given {}",
                self.coefficients.len(),
                x.cols()
            )));
        }
        Ok((0..x.rows())
            .map(|i| {
                self.intercept
                    + x.row(i).iter().zip(&self.coefficients).map(|(a, b)| a * b).sum::<f64>()
            })
            .collect())
    }

    fn linear_terms(&self) -> Option<(&[f64], f64)> {
        Some((&self.coefficients, self.intercept))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Least squares / ridge
// ─────────────────────────────────────────────────────────────────────────────

/// Ordinary least squares, or ridge regression when `ridge > 0`.
///
/// Minimises `‖y − Xβ − b‖² + ridge·‖β‖²`. The intercept is never penalised:
/// the data are centred first, so the penalty shrinks slopes without dragging
/// the prediction toward zero.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LinearRegression {
    pub ridge: f64,
}

impl LinearRegression {
    pub fn fit_model(&self, x: &Matrix, y: &[f64]) -> Result<LinearModel> {
        check_training(x, y)?;
        if !(self.ridge >= 0.0) {
            return Err(DataError::Schema("the ridge penalty must be non-negative".into()));
        }
        let (n, d) = (x.rows(), x.cols());
        if self.ridge == 0.0 && n <= d {
            return Err(DataError::Schema(format!(
                "least squares on {d} features needs more than {d} rows, got {n}; \
                 add a ridge penalty or drop features"
            )));
        }
        let nf = n as f64;
        let x_mean: Vec<f64> = (0..d).map(|j| x.column(j).iter().sum::<f64>() / nf).collect();
        let y_mean = y.iter().sum::<f64>() / nf;

        // Gram matrix XcᵀXc (+ ridge on the diagonal) and XcᵀYc, centred.
        let mut gram = vec![0.0; d * d];
        let mut rhs = vec![0.0; d];
        for i in 0..n {
            let row = x.row(i);
            let yc = y[i] - y_mean;
            for a in 0..d {
                let xa = row[a] - x_mean[a];
                rhs[a] += xa * yc;
                for b in a..d {
                    gram[a * d + b] += xa * (row[b] - x_mean[b]);
                }
            }
        }
        for a in 0..d {
            for b in 0..a {
                gram[a * d + b] = gram[b * d + a];
            }
            gram[a * d + a] += self.ridge;
        }
        let coefficients = cholesky_solve(&gram, &rhs, d)?;
        let intercept = y_mean - coefficients.iter().zip(&x_mean).map(|(c, m)| c * m).sum::<f64>();
        Ok(LinearModel { coefficients, intercept })
    }
}

impl RegressorSpec for LinearRegression {
    fn name(&self) -> &'static str {
        if self.ridge > 0.0 { "ridge_regression" } else { "linear_regression" }
    }

    fn describe(&self) -> String {
        if self.ridge > 0.0 {
            format!("ridge_regression(ridge={})", self.ridge)
        } else {
            "linear_regression".to_string()
        }
    }

    fn fit(&self, x: &Matrix, y: &[f64]) -> Result<Box<dyn Regressor>> {
        Ok(Box::new(self.fit_model(x, y)?))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LASSO
// ─────────────────────────────────────────────────────────────────────────────

/// LASSO: least squares with an L1 penalty, by cyclic coordinate descent.
///
/// Minimises `(1 / 2n)·‖y − Zβ − b‖² + alpha·‖β‖₁` on STANDARDISED features
/// (the scikit-learn objective), so `alpha` means the same thing whatever units
/// the features are in and every feature competes on equal terms.
#[derive(Clone, Debug, PartialEq)]
pub struct Lasso {
    pub alpha: f64,
    pub max_iter: usize,
    /// Stop when no coefficient moves more than this in a full sweep.
    pub tol: f64,
}

impl Default for Lasso {
    fn default() -> Self {
        Self { alpha: 0.1, max_iter: 1000, tol: 1e-8 }
    }
}

/// A fitted LASSO.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LassoModel {
    /// The model in the original feature units, for prediction.
    pub linear: LinearModel,
    /// Coefficients on the standardised features: comparable across features,
    /// so their magnitudes rank the inputs. Exactly zero means "not needed".
    pub standardized_coefficients: Vec<f64>,
    pub iterations: usize,
    pub converged: bool,
}

impl Regressor for LassoModel {
    fn predict(&self, x: &Matrix) -> Result<Vec<f64>> {
        self.linear.predict(x)
    }

    fn linear_terms(&self) -> Option<(&[f64], f64)> {
        self.linear.linear_terms()
    }
}

fn soft_threshold(rho: f64, alpha: f64) -> f64 {
    if rho > alpha {
        rho - alpha
    } else if rho < -alpha {
        rho + alpha
    } else {
        0.0
    }
}

impl Lasso {
    pub fn fit_model(&self, x: &Matrix, y: &[f64]) -> Result<LassoModel> {
        check_training(x, y)?;
        if !(self.alpha >= 0.0) {
            return Err(DataError::Schema("alpha must be non-negative".into()));
        }
        let scaler = Scaler::fit(x)?;
        let z = scaler.transform(x)?;
        let (n, d) = (z.rows(), z.cols());
        let nf = n as f64;
        let y_mean = y.iter().sum::<f64>() / nf;
        // Residual of the current model on centred y: r = y_c − Zβ.
        let mut r: Vec<f64> = y.iter().map(|v| v - y_mean).collect();
        // (1/n)·Σ z_ij² per column: 1 for a varying standardised column, 0 for
        // a constant one, which can never enter the model.
        let norm: Vec<f64> = (0..d)
            .map(|j| (0..n).map(|i| z.get(i, j).powi(2)).sum::<f64>() / nf)
            .collect();
        let mut beta = vec![0.0; d];
        let (mut iterations, mut converged) = (0, false);
        while iterations < self.max_iter {
            iterations += 1;
            let mut max_move = 0.0f64;
            for j in 0..d {
                if norm[j] < 1e-12 {
                    continue;
                }
                // ρ_j = (1/n)·Σ z_ij·(r_i + z_ij·β_j): the correlation of feature
                // j with the residual that excludes feature j's own term.
                let mut rho = 0.0;
                for i in 0..n {
                    rho += z.get(i, j) * (r[i] + z.get(i, j) * beta[j]);
                }
                rho /= nf;
                let new = soft_threshold(rho, self.alpha) / norm[j];
                let delta = new - beta[j];
                if delta != 0.0 {
                    for i in 0..n {
                        r[i] -= z.get(i, j) * delta;
                    }
                    beta[j] = new;
                    max_move = max_move.max(delta.abs());
                }
            }
            if max_move < self.tol {
                converged = true;
                break;
            }
        }
        // Back to original units: β_orig = β_std / std, and the intercept
        // absorbs the means.
        let coefficients: Vec<f64> = beta.iter().zip(&scaler.std).map(|(b, s)| b / s).collect();
        let intercept =
            y_mean - coefficients.iter().zip(&scaler.mean).map(|(c, m)| c * m).sum::<f64>();
        Ok(LassoModel {
            linear: LinearModel { coefficients, intercept },
            standardized_coefficients: beta,
            iterations,
            converged,
        })
    }
}

impl RegressorSpec for Lasso {
    fn name(&self) -> &'static str {
        "lasso"
    }

    fn describe(&self) -> String {
        format!("lasso(alpha={}, max_iter={}, tol={})", self.alpha, self.max_iter, self.tol)
    }

    fn fit(&self, x: &Matrix, y: &[f64]) -> Result<Box<dyn Regressor>> {
        Ok(Box::new(self.fit_model(x, y)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// y = 5 + 2·x0 − 3·x1 exactly, on a non-degenerate grid.
    fn plane() -> (Matrix, Vec<f64>) {
        let mut rows = Vec::new();
        let mut y = Vec::new();
        for a in 0..5 {
            for b in 0..4 {
                let (x0, x1) = (a as f64, (b * b) as f64 * 0.5);
                rows.push(vec![x0, x1]);
                y.push(5.0 + 2.0 * x0 - 3.0 * x1);
            }
        }
        (Matrix::from_rows(&rows).unwrap(), y)
    }

    #[test]
    fn least_squares_recovers_an_exact_plane() {
        let (x, y) = plane();
        let m = LinearRegression::default().fit_model(&x, &y).unwrap();
        assert!((m.coefficients[0] - 2.0).abs() < 1e-9, "{m:?}");
        assert!((m.coefficients[1] + 3.0).abs() < 1e-9, "{m:?}");
        assert!((m.intercept - 5.0).abs() < 1e-9, "{m:?}");
        let pred = m.predict(&x).unwrap();
        assert!(pred.iter().zip(&y).all(|(p, t)| (p - t).abs() < 1e-9));
    }

    #[test]
    fn ridge_shrinks_the_slopes_but_not_the_mean_prediction() {
        let (x, y) = plane();
        let ols = LinearRegression::default().fit_model(&x, &y).unwrap();
        let ridge = LinearRegression { ridge: 50.0 }.fit_model(&x, &y).unwrap();
        let norm = |m: &LinearModel| m.coefficients.iter().map(|c| c * c).sum::<f64>();
        assert!(norm(&ridge) < norm(&ols), "the penalty must shrink the slopes");
        // An unpenalised intercept keeps the mean prediction on the mean target.
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let pred = ridge.predict(&x).unwrap();
        assert!((mean(&pred) - mean(&y)).abs() < 1e-9);
    }

    #[test]
    fn collinear_features_are_refused_with_the_remedy() {
        // Column 1 is exactly twice column 0.
        let rows: Vec<Vec<f64>> = (0..6).map(|i| vec![i as f64, 2.0 * i as f64]).collect();
        let y: Vec<f64> = (0..6).map(|i| i as f64).collect();
        let err = LinearRegression::default().fit_model(&Matrix::from_rows(&rows).unwrap(), &y).unwrap_err();
        assert!(format!("{err}").contains("ridge"));
        // And the remedy works.
        assert!(LinearRegression { ridge: 1.0 }.fit_model(&Matrix::from_rows(&rows).unwrap(), &y).is_ok());
    }

    #[test]
    fn too_few_rows_for_least_squares_is_refused() {
        let x = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 5.0]]).unwrap();
        assert!(LinearRegression::default().fit_model(&x, &[1.0, 2.0]).is_err());
    }

    #[test]
    fn lasso_zeroes_the_feature_the_target_ignores() {
        // y depends on x0 only; x1 is an independent pattern.
        let rows: Vec<Vec<f64>> = (0..40).map(|i| vec![i as f64, ((i * 13) % 7) as f64]).collect();
        let y: Vec<f64> = (0..40).map(|i| 3.0 * i as f64 + 1.0).collect();
        let m = Lasso { alpha: 0.5, ..Default::default() }
            .fit_model(&Matrix::from_rows(&rows).unwrap(), &y)
            .unwrap();
        assert!(m.converged);
        assert!(m.standardized_coefficients[0] > 1.0, "{m:?}");
        assert_eq!(m.standardized_coefficients[1], 0.0, "the unused feature is exactly zero");
    }

    #[test]
    fn a_large_alpha_zeroes_everything_and_predicts_the_mean() {
        let (x, y) = plane();
        let m = Lasso { alpha: 1e6, ..Default::default() }.fit_model(&x, &y).unwrap();
        assert!(m.standardized_coefficients.iter().all(|&b| b == 0.0));
        let mean = y.iter().sum::<f64>() / y.len() as f64;
        assert!(m.predict(&x).unwrap().iter().all(|p| (p - mean).abs() < 1e-9));
    }

    #[test]
    fn lasso_with_no_penalty_approaches_least_squares() {
        let (x, y) = plane();
        let m = Lasso { alpha: 0.0, max_iter: 100_000, tol: 1e-12 }.fit_model(&x, &y).unwrap();
        assert!((m.linear.coefficients[0] - 2.0).abs() < 1e-6, "{m:?}");
        assert!((m.linear.coefficients[1] + 3.0).abs() < 1e-6, "{m:?}");
        assert!((m.linear.intercept - 5.0).abs() < 1e-6, "{m:?}");
    }
}
