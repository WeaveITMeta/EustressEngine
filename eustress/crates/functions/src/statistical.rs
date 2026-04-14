//! # Stage 11: Statistical — Correlation, Regression & Prediction
//!
//! Pure-Rust statistical operations over entity property series. No external
//! dependencies — all algorithms are implemented directly.
//!
//! ## Table of Contents
//! 1. Result types      — RegressionModel, PredictionResult
//! 2. Rune functions    — correlate / regress / predict
//! 3. Algorithms        — Pearson r, OLS linear regression, prediction
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                    | Purpose                                                     |
//! |-----------------------------|-------------------------------------------------------------|
//! | `correlate(csv_a, csv_b)`   | Pearson correlation coefficient between two series          |
//! | `regress(csv_x, csv_y)`     | Ordinary least squares linear regression (slope + intercept)|
//! | `predict(model, x)`         | Run inference on a fitted regression model                  |
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::statistical;
//!
//! pub fn analyze_thermal_vs_stress(temps_csv, stress_csv) {
//!     let r = statistical::correlate(temps_csv, stress_csv);
//!     eustress::log_info(&format!("Pearson r = {:.4}", r));
//!
//!     let model = statistical::regress(temps_csv, stress_csv);
//!     let predicted_stress = statistical::predict(model, 450.0);
//! }
//! ```

use tracing::info;

// ============================================================================
// 1. Result Types
// ============================================================================

/// A fitted linear regression model (y = slope * x + intercept).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct RegressionModel {
    /// Slope coefficient (rise per unit of x)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub slope: f64,
    /// Y-intercept
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub intercept: f64,
    /// R² coefficient of determination (0.0–1.0, higher = better fit)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub r_squared: f64,
    /// Number of data points used to fit the model
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub sample_count: i64,
    /// Mean absolute error on training data
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub mean_absolute_error: f64,
}

impl RegressionModel {
    /// Null model (no data)
    fn null() -> Self {
        Self {
            slope: 0.0,
            intercept: 0.0,
            r_squared: 0.0,
            sample_count: 0,
            mean_absolute_error: 0.0,
        }
    }

    /// Predict y for a given x using this model
    pub fn predict_y(&self, x: f64) -> f64 {
        self.slope * x + self.intercept
    }
}

// ============================================================================
// 2. Rune Functions
// ============================================================================

/// Compute the Pearson correlation coefficient between two numeric series.
///
/// Both series must have the same length. Shorter of the two is used if
/// lengths differ. Returns a value in [-1.0, 1.0]:
/// - +1.0 = perfect positive correlation
/// -  0.0 = no linear correlation
/// - -1.0 = perfect negative correlation
///
/// # Arguments
/// * `csv_a` — Comma-separated f64 values for series A
/// * `csv_b` — Comma-separated f64 values for series B
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn correlate(csv_a: &str, csv_b: &str) -> f64 {
    let a = parse_csv(csv_a);
    let b = parse_csv(csv_b);

    let r = pearson_correlation(&a, &b);

    info!(
        "[Statistical] correlate({} samples, {} samples) → r={:.6}",
        a.len(), b.len(), r
    );

    r
}

/// Fit an ordinary least squares linear regression model to (x, y) data.
///
/// Returns a `RegressionModel` with slope, intercept, R², sample count,
/// and mean absolute error on the training data.
///
/// # Arguments
/// * `csv_x` — Comma-separated f64 independent variable values
/// * `csv_y` — Comma-separated f64 dependent variable values
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn regress(csv_x: &str, csv_y: &str) -> RegressionModel {
    let x = parse_csv(csv_x);
    let y = parse_csv(csv_y);

    let model = ols_linear_regression(&x, &y);

    info!(
        "[Statistical] regress({} points) slope={:.6} intercept={:.6} r²={:.4} mae={:.4}",
        model.sample_count, model.slope, model.intercept, model.r_squared, model.mean_absolute_error
    );

    model
}

/// Run inference on a fitted regression model for a single x value.
///
/// Computes `y = slope * x + intercept` from the model.
///
/// # Arguments
/// * `model` — A `RegressionModel` returned from `regress()`
/// * `x`     — Input value to predict for
///
/// # Returns
/// Predicted y value as f64.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn predict(model: &RegressionModel, x: f64) -> f64 {
    let y = model.predict_y(x);

    info!(
        "[Statistical] predict(x={}) → y={:.6} (slope={:.4}, intercept={:.4})",
        x, y, model.slope, model.intercept
    );

    y
}

// ============================================================================
// 3. Algorithms (pure Rust, no deps)
// ============================================================================

/// Parse comma-separated f64 values from a string, skipping invalid tokens.
fn parse_csv(csv: &str) -> Vec<f64> {
    csv.split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .collect()
}

/// Pearson correlation coefficient: r = Σ(xi - x̄)(yi - ȳ) / (n·σx·σy)
fn pearson_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }

    let a = &a[..n];
    let b = &b[..n];

    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;

    let numerator: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();

    let std_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>().sqrt();
    let std_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum::<f64>().sqrt();

    let denominator = std_a * std_b;
    if denominator < f64::EPSILON {
        return 0.0;
    }

    (numerator / denominator).clamp(-1.0, 1.0)
}

/// Ordinary Least Squares linear regression.
/// slope = Σ(xi - x̄)(yi - ȳ) / Σ(xi - x̄)²
/// intercept = ȳ - slope·x̄
fn ols_linear_regression(x: &[f64], y: &[f64]) -> RegressionModel {
    let n = x.len().min(y.len());
    if n < 2 {
        return RegressionModel::null();
    }

    let x = &x[..n];
    let y = &y[..n];

    let mean_x = x.iter().sum::<f64>() / n as f64;
    let mean_y = y.iter().sum::<f64>() / n as f64;

    let ss_xy: f64 = x.iter().zip(y).map(|(xi, yi)| (xi - mean_x) * (yi - mean_y)).sum();
    let ss_xx: f64 = x.iter().map(|xi| (xi - mean_x).powi(2)).sum();

    if ss_xx.abs() < f64::EPSILON {
        return RegressionModel::null();
    }

    let slope = ss_xy / ss_xx;
    let intercept = mean_y - slope * mean_x;

    // R² = 1 - SS_res / SS_tot
    let ss_res: f64 = x.iter().zip(y).map(|(xi, yi)| {
        let predicted = slope * xi + intercept;
        (yi - predicted).powi(2)
    }).sum();

    let ss_tot: f64 = y.iter().map(|yi| (yi - mean_y).powi(2)).sum();
    let r_squared = if ss_tot.abs() < f64::EPSILON { 1.0 } else { 1.0 - ss_res / ss_tot };

    // Mean absolute error
    let mae: f64 = x.iter().zip(y).map(|(xi, yi)| {
        ((slope * xi + intercept) - yi).abs()
    }).sum::<f64>() / n as f64;

    RegressionModel {
        slope,
        intercept,
        r_squared: r_squared.clamp(0.0, 1.0),
        sample_count: n as i64,
        mean_absolute_error: mae,
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `statistical` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_statistical_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "statistical"])?;

    module.ty::<RegressionModel>()?;

    module.function_meta(correlate)?;
    module.function_meta(regress)?;
    module.function_meta(predict)?;

    Ok(module)
}
