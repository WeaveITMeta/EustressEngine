//! # Stage 7: Refinement — Data Cleansing, Transformation & Validation
//!
//! Pure-Rust data processing with no external dependencies. Operates on
//! `rune::runtime::Vec` (list of f64) and entity property maps passed
//! from scripts. All logic is stateless — no bridge required.
//!
//! ## Table of Contents
//! 1. Result types      — ValidationResult
//! 2. Rune functions    — cleanse / transform / validate
//! 3. Built-in transforms
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                      | Purpose                                                    |
//! |-------------------------------|------------------------------------------------------------|
//! | `cleanse(values_csv)`         | Remove NaN, clamp to finite range, return cleaned CSV      |
//! | `transform(values_csv, name)` | Apply a named transformation pipeline to a data series     |
//! | `validate(value, rule)`       | Check a single value against a named constraint rule       |
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::refinement;
//!
//! pub fn process_sensor_data(raw_csv) {
//!     let clean = refinement::cleanse(raw_csv);
//!     let scaled = refinement::transform(clean, "normalize_0_1");
//!     let ok = refinement::validate(0.75, "range_0_1");
//!     if !ok.passed { eustress::log_warn(&ok.message); }
//! }
//! ```

use tracing::{info, warn};

// ============================================================================
// 1. ValidationResult — returned from validate()
// ============================================================================

/// Result of a validation check.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct ValidationResult {
    /// Whether the validation passed
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub passed: bool,
    /// Human-readable result message
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub message: String,
    /// The rule that was applied
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub rule: String,
    /// The input value that was checked
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub value: f64,
}

impl ValidationResult {
    fn pass(value: f64, rule: &str) -> Self {
        Self {
            passed: true,
            message: format!("{} passed rule '{}'", value, rule),
            rule: rule.to_string(),
            value,
        }
    }

    fn fail(value: f64, rule: &str, reason: &str) -> Self {
        Self {
            passed: false,
            message: format!("{} failed rule '{}': {}", value, rule, reason),
            rule: rule.to_string(),
            value,
        }
    }
}

// ============================================================================
// 2. Rune Functions
// ============================================================================

/// Sanitize a comma-separated data series — remove NaN/Inf, clamp to
/// [-1e15, 1e15], and deduplicate adjacent identical values.
///
/// # Arguments
/// * `values_csv` — Comma-separated f64 values (e.g. `"1.0,NaN,3.5,Inf"`)
///
/// # Returns
/// Cleaned comma-separated string (e.g. `"1.0,3.5"`)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn cleanse(values_csv: &str) -> String {
    const MAX_VAL: f64 = 1e15;

    let cleaned: Vec<f64> = values_csv
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(-MAX_VAL, MAX_VAL))
        .collect();

    // Deduplicate adjacent identical values (sensor flatlines)
    let deduped: Vec<f64> = cleaned
        .windows(2)
        .filter_map(|w| if (w[0] - w[1]).abs() > f64::EPSILON { Some(w[0]) } else { None })
        .chain(cleaned.last().copied())
        .collect();

    info!(
        "[Refinement] cleanse: {} → {} values after cleaning",
        values_csv.split(',').count(),
        deduped.len()
    );

    deduped
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Apply a named transformation pipeline to a comma-separated data series.
///
/// ## Built-in Pipelines
///
/// | Name              | Effect                                              |
/// |-------------------|-----------------------------------------------------|
/// | `normalize_0_1`   | Min-max normalization → [0, 1]                      |
/// | `normalize_neg1_1`| Min-max normalization → [-1, 1]                     |
/// | `log`             | Natural log transform (ln(x+1) for x ≥ 0)          |
/// | `sqrt`            | Square root transform (√|x|, sign preserved)        |
/// | `zscore`          | Z-score standardization (μ=0, σ=1)                  |
/// | `scale_100`       | Scale all values so max = 100.0                     |
/// | `delta`           | First-order differences (x[i] - x[i-1])             |
/// | `abs`             | Absolute value of each element                      |
/// | `invert`          | 1 / x (skips zeros)                                 |
///
/// Unknown pipeline names return the input unchanged with a warning.
///
/// # Arguments
/// * `values_csv`   — Comma-separated f64 values
/// * `pipeline_name`— Name of the transform to apply
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn transform(values_csv: &str, pipeline_name: &str) -> String {
    let values: Vec<f64> = values_csv
        .split(',')
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .collect();

    if values.is_empty() {
        return String::new();
    }

    let result = apply_transform(&values, pipeline_name);

    info!(
        "[Refinement] transform({} values, '{}') completed",
        values.len(), pipeline_name
    );

    result
        .iter()
        .map(|v| format!("{:.6}", v))
        .collect::<Vec<_>>()
        .join(",")
}

/// Check a single value against a named constraint rule.
///
/// ## Built-in Rules
///
/// | Rule name         | Constraint                    |
/// |-------------------|-------------------------------|
/// | `range_0_1`       | 0.0 ≤ value ≤ 1.0             |
/// | `range_neg1_1`    | -1.0 ≤ value ≤ 1.0            |
/// | `positive`        | value > 0.0                   |
/// | `non_negative`    | value ≥ 0.0                   |
/// | `finite`          | is finite (not NaN / Inf)     |
/// | `non_zero`        | value ≠ 0.0                   |
/// | `temperature_k`   | 0.0 ≤ value ≤ 50000.0 (Kelvin)|
/// | `probability`     | 0.0 ≤ value ≤ 1.0             |
/// | `stress_level`    | 0.0 ≤ value ≤ 1.0             |
///
/// Unknown rule names return a failed result with an explanation.
///
/// # Arguments
/// * `value` — The numeric value to validate
/// * `rule`  — Rule name
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn validate(value: f64, rule: &str) -> ValidationResult {
    let result = apply_validation(value, rule);

    info!(
        "[Refinement] validate({}, '{}') → {}",
        value, rule, if result.passed { "pass" } else { "FAIL" }
    );

    result
}

// ============================================================================
// 3. Built-in Transforms (pure Rust, no deps)
// ============================================================================

fn apply_transform(values: &[f64], pipeline: &str) -> Vec<f64> {
    match pipeline {
        "normalize_0_1" => {
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let range = max - min;
            if range.abs() < f64::EPSILON {
                return values.iter().map(|_| 0.0).collect();
            }
            values.iter().map(|v| (v - min) / range).collect()
        }

        "normalize_neg1_1" => {
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let range = max - min;
            if range.abs() < f64::EPSILON {
                return values.iter().map(|_| 0.0).collect();
            }
            values.iter().map(|v| 2.0 * (v - min) / range - 1.0).collect()
        }

        "log" => values
            .iter()
            .map(|&v| if v >= 0.0 { (v + 1.0).ln() } else { -(-v + 1.0).ln() })
            .collect(),

        "sqrt" => values
            .iter()
            .map(|&v| if v >= 0.0 { v.sqrt() } else { -(-v).sqrt() })
            .collect(),

        "zscore" => {
            let n = values.len() as f64;
            let mean = values.iter().sum::<f64>() / n;
            let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
            let std = var.sqrt();
            if std < f64::EPSILON {
                return values.iter().map(|_| 0.0).collect();
            }
            values.iter().map(|v| (v - mean) / std).collect()
        }

        "scale_100" => {
            let max = values.iter().cloned().fold(0.0_f64, f64::max).abs();
            if max < f64::EPSILON {
                return values.iter().map(|_| 0.0).collect();
            }
            values.iter().map(|v| v / max * 100.0).collect()
        }

        "delta" => {
            if values.len() < 2 {
                return vec![0.0];
            }
            values.windows(2).map(|w| w[1] - w[0]).collect()
        }

        "abs" => values.iter().map(|v| v.abs()).collect(),

        "invert" => values
            .iter()
            .map(|&v| if v.abs() > f64::EPSILON { 1.0 / v } else { 0.0 })
            .collect(),

        unknown => {
            warn!("[Refinement] Unknown transform '{}' — returning input unchanged", unknown);
            values.to_vec()
        }
    }
}

fn apply_validation(value: f64, rule: &str) -> ValidationResult {
    match rule {
        "range_0_1" | "probability" | "stress_level" => {
            if (0.0..=1.0).contains(&value) {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be in [0.0, 1.0]")
            }
        }

        "range_neg1_1" => {
            if (-1.0..=1.0).contains(&value) {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be in [-1.0, 1.0]")
            }
        }

        "positive" => {
            if value > 0.0 {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be > 0.0")
            }
        }

        "non_negative" => {
            if value >= 0.0 {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be ≥ 0.0")
            }
        }

        "finite" => {
            if value.is_finite() {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be finite (not NaN or Inf)")
            }
        }

        "non_zero" => {
            if value.abs() > f64::EPSILON {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must not be zero")
            }
        }

        "temperature_k" => {
            if (0.0..=50_000.0).contains(&value) {
                ValidationResult::pass(value, rule)
            } else {
                ValidationResult::fail(value, rule, "must be in [0.0, 50000.0] Kelvin")
            }
        }

        unknown => ValidationResult::fail(
            value,
            unknown,
            &format!("unknown rule '{}' — use: range_0_1, positive, non_negative, finite, non_zero, temperature_k, zscore", unknown),
        ),
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `refinement` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_refinement_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "refinement"])?;

    module.ty::<ValidationResult>()?;

    module.function_meta(cleanse)?;
    module.function_meta(transform)?;
    module.function_meta(validate)?;

    Ok(module)
}
