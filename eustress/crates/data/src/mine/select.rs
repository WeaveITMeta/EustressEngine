//! Feature selection, in the three families the field distinguishes by how
//! they score a subset.
//!
//! - **Filters** score each feature against the target with a model-free
//!   statistic (variance, correlation, ANOVA F, mutual information). Cheap and
//!   general, but blind to which model will use the features.
//! - **Wrappers** score a subset by training a model on it and measuring it
//!   under cross-validation. Tuned to that model and far more expensive: a
//!   greedy forward pass fits `features × steps × folds` models.
//! - **Embedded** methods select while fitting. The LASSO's L1 penalty drives
//!   unneeded coefficients to exactly zero.
//!
//! [`super::eval::permutation_importance`] is the fourth tool: it scores a
//! feature by what a fitted model loses when that feature is scrambled.

use serde::Serialize;

use super::classify::ClassifierSpec;
use super::eval::{cross_validate_classifier, Fold};
use super::regress::Lasso;
use super::Matrix;
use crate::{DataError, Result};

/// One feature's score under some criterion.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeatureScore {
    pub feature: usize,
    pub score: f64,
}

/// Features ordered by score, highest first. Ties keep feature order; NaN
/// scores sort last.
pub fn ranked(scores: &[f64]) -> Vec<FeatureScore> {
    let mut out: Vec<FeatureScore> =
        scores.iter().enumerate().map(|(feature, &score)| FeatureScore { feature, score }).collect();
    out.sort_by(|a, b| match (a.score.is_nan(), b.score.is_nan()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => b.score.total_cmp(&a.score).then(a.feature.cmp(&b.feature)),
    });
    out
}

fn column_stats(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;
    (mean, var)
}

// ─────────────────────────────────────────────────────────────────────────────
// Filters
// ─────────────────────────────────────────────────────────────────────────────

/// Indices of features whose population variance exceeds `threshold`. With
/// `threshold = 0` this drops exactly the constant columns, which carry no
/// information for any model.
pub fn variance_threshold(x: &Matrix, threshold: f64) -> Vec<usize> {
    (0..x.cols()).filter(|&j| column_stats(&x.column(j)).1 > threshold).collect()
}

/// Absolute Pearson correlation of each feature with a numeric target. A
/// constant feature scores 0. Linear association only: a feature related to
/// the target by a U-shape can score near zero here and high on
/// [`mutual_information`].
pub fn pearson_scores(x: &Matrix, y: &[f64]) -> Result<Vec<f64>> {
    if x.rows() != y.len() || x.rows() < 2 {
        return Err(DataError::Schema("pearson: need at least 2 rows, one target per row".into()));
    }
    let (ym, yv) = column_stats(y);
    Ok((0..x.cols())
        .map(|j| {
            let c = x.column(j);
            let (xm, xv) = column_stats(&c);
            if xv == 0.0 || yv == 0.0 {
                return 0.0;
            }
            let cov = c.iter().zip(y).map(|(a, b)| (a - xm) * (b - ym)).sum::<f64>() / c.len() as f64;
            (cov / (xv.sqrt() * yv.sqrt())).abs()
        })
        .collect())
}

/// One-way ANOVA F statistic of each feature across the classes: between-class
/// variance over within-class variance. Large means the class means differ by
/// more than the spread inside each class explains. Infinite when the classes
/// are perfectly separated by that feature; 0 for a constant feature.
pub fn anova_f(x: &Matrix, y: &[usize], n_classes: usize) -> Result<Vec<f64>> {
    if x.rows() != y.len() {
        return Err(DataError::Schema("anova: one label per row".into()));
    }
    let mut count = vec![0usize; n_classes];
    for &c in y {
        *count.get_mut(c).ok_or_else(|| DataError::Schema(format!("label {c} is outside 0..{n_classes}")))? += 1;
    }
    let k = count.iter().filter(|&&c| c > 0).count();
    let n = x.rows();
    if k < 2 || n <= k {
        return Err(DataError::Schema(
            "anova needs at least 2 classes and more rows than classes".into(),
        ));
    }
    Ok((0..x.cols())
        .map(|j| {
            let col = x.column(j);
            let grand = col.iter().sum::<f64>() / n as f64;
            let mut sum = vec![0.0; n_classes];
            for (v, &c) in col.iter().zip(y) {
                sum[c] += v;
            }
            let mean: Vec<f64> = sum
                .iter()
                .zip(&count)
                .map(|(s, &m)| if m > 0 { s / m as f64 } else { 0.0 })
                .collect();
            let between: f64 = (0..n_classes)
                .map(|c| count[c] as f64 * (mean[c] - grand).powi(2))
                .sum();
            let within: f64 = col.iter().zip(y).map(|(v, &c)| (v - mean[c]).powi(2)).sum();
            if within == 0.0 {
                return if between == 0.0 { 0.0 } else { f64::INFINITY };
            }
            (between / (k - 1) as f64) / (within / (n - k) as f64)
        })
        .collect())
}

/// Equal-frequency bin of each value: `bins` groups of roughly equal size.
/// Equal values always share a bin.
pub(crate) fn quantile_bins(v: &[f64], bins: usize) -> Vec<usize> {
    let mut sorted = v.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let mut edges: Vec<f64> = (1..bins).map(|b| sorted[(b * n / bins).min(n - 1)]).collect();
    edges.dedup();
    v.iter().map(|x| edges.iter().filter(|&&e| *x >= e).count()).collect()
}

/// Mutual information (in nats) between each feature and the class, with each
/// feature discretised into `bins` equal-frequency bins.
///
/// Zero when the feature and the class are independent, and it captures any
/// dependence, not only linear ones. The binned estimate is biased upward on
/// small samples, so compare features on the same data rather than reading
/// the value in isolation.
pub fn mutual_information(x: &Matrix, y: &[usize], n_classes: usize, bins: usize) -> Result<Vec<f64>> {
    if x.rows() != y.len() || x.rows() < 2 {
        return Err(DataError::Schema("mutual information: need at least 2 rows, one label per row".into()));
    }
    if bins < 2 {
        return Err(DataError::Schema("mutual information needs at least 2 bins".into()));
    }
    if let Some(&bad) = y.iter().find(|&&c| c >= n_classes) {
        return Err(DataError::Schema(format!("label {bad} is outside 0..{n_classes}")));
    }
    let n = x.rows() as f64;
    let mut py = vec![0.0; n_classes];
    for &c in y {
        py[c] += 1.0 / n;
    }
    Ok((0..x.cols())
        .map(|j| {
            let b = quantile_bins(&x.column(j), bins);
            let nb = b.iter().max().map_or(1, |m| m + 1);
            let mut joint = vec![vec![0.0; n_classes]; nb];
            let mut pb = vec![0.0; nb];
            for (&bi, &c) in b.iter().zip(y) {
                joint[bi][c] += 1.0 / n;
                pb[bi] += 1.0 / n;
            }
            let mut mi = 0.0;
            for bi in 0..nb {
                for c in 0..n_classes {
                    let p = joint[bi][c];
                    if p > 0.0 {
                        mi += p * (p / (pb[bi] * py[c])).ln();
                    }
                }
            }
            mi.max(0.0)
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Wrapper
// ─────────────────────────────────────────────────────────────────────────────

/// One step of a greedy forward selection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionStep {
    /// The feature added at this step.
    pub added: usize,
    /// Every feature selected so far, in the order chosen.
    pub selected: Vec<usize>,
    /// Mean cross-validated accuracy with exactly these features.
    pub cv_accuracy: f64,
    pub cv_std: f64,
}

/// Greedy forward selection: start empty and, at each step, add the feature
/// whose inclusion gives the best cross-validated accuracy.
///
/// Every step is reported, not just the final set, because the useful answer
/// is usually where the curve flattens: the point after which adding features
/// buys nothing (or starts to cost, as the model begins to fit noise). Ties go
/// to the lower feature index.
pub fn forward_selection(
    spec: &dyn ClassifierSpec,
    x: &Matrix,
    y: &[usize],
    n_classes: usize,
    folds: &[Fold],
    max_features: usize,
) -> Result<Vec<SelectionStep>> {
    if folds.is_empty() {
        return Err(DataError::Schema("forward selection needs cross-validation folds".into()));
    }
    let mut selected: Vec<usize> = Vec::new();
    let mut remaining: Vec<usize> = (0..x.cols()).collect();
    let mut steps = Vec::new();
    while selected.len() < max_features.min(x.cols()) {
        let mut best: Option<(f64, f64, usize)> = None;
        for &f in &remaining {
            let mut trial = selected.clone();
            trial.push(f);
            let (acc, _) = cross_validate_classifier(spec, &x.select_cols(&trial), y, n_classes, folds)?;
            if best.is_none_or(|(m, _, _)| acc.mean > m) {
                best = Some((acc.mean, acc.std, f));
            }
        }
        let Some((mean, std, f)) = best else { break };
        selected.push(f);
        remaining.retain(|&r| r != f);
        steps.push(SelectionStep { added: f, selected: selected.clone(), cv_accuracy: mean, cv_std: std });
    }
    Ok(steps)
}

// ─────────────────────────────────────────────────────────────────────────────
// Embedded
// ─────────────────────────────────────────────────────────────────────────────

/// Features the LASSO keeps at penalty `alpha`, ranked by the magnitude of
/// their standardised coefficient. Features driven to exactly zero are left
/// out: at this penalty the model does not need them.
pub fn lasso_select(x: &Matrix, y: &[f64], alpha: f64) -> Result<Vec<FeatureScore>> {
    let m = Lasso { alpha, ..Default::default() }.fit_model(x, y)?;
    let scores: Vec<f64> = m.standardized_coefficients.iter().map(|b| b.abs()).collect();
    Ok(ranked(&scores).into_iter().filter(|s| s.score > 0.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mine::classify::DecisionTree;
    use crate::mine::eval::stratified_k_fold;

    /// Three features: 0 decides the class, 1 is noise, 2 is constant.
    ///
    /// Feature 0 leaves a gap of 10 between the classes (0..=19, then 30..=49).
    /// Without it, a fold that holds out both 18 and 19 learns a threshold of
    /// 18.5 and misclassifies the held-out 19, so "perfect cross-validated
    /// accuracy" would depend on the shuffle. With the gap, every threshold a
    /// fold can learn falls inside it. The regression target is exactly linear
    /// in feature 0.
    fn data() -> (Matrix, Vec<usize>, Vec<f64>) {
        let x0 = |i: usize| if i >= 20 { i as f64 + 10.0 } else { i as f64 };
        let rows: Vec<Vec<f64>> =
            (0..40).map(|i| vec![x0(i), ((i * 17) % 11) as f64, 4.0]).collect();
        let y: Vec<usize> = (0..40).map(|i| usize::from(i >= 20)).collect();
        let t: Vec<f64> = (0..40).map(|i| 2.0 * x0(i)).collect();
        (Matrix::from_rows(&rows).unwrap(), y, t)
    }

    #[test]
    fn ranking_is_descending_with_nan_last_and_stable_ties() {
        let r = ranked(&[0.5, f64::NAN, 0.9, 0.5]);
        let order: Vec<usize> = r.iter().map(|s| s.feature).collect();
        assert_eq!(order, vec![2, 0, 3, 1]);
    }

    #[test]
    fn a_zero_variance_threshold_drops_exactly_the_constant_column() {
        let (x, _, _) = data();
        assert_eq!(variance_threshold(&x, 0.0), vec![0, 1]);
    }

    #[test]
    fn pearson_puts_the_linear_feature_first() {
        let (x, _, t) = data();
        let s = pearson_scores(&x, &t).unwrap();
        assert!((s[0] - 1.0).abs() < 1e-12, "a perfect linear relation: {s:?}");
        assert_eq!(s[2], 0.0, "a constant feature carries nothing");
        assert_eq!(ranked(&s)[0].feature, 0);
    }

    #[test]
    fn anova_finds_the_feature_that_separates_the_classes() {
        let (x, y, _) = data();
        let f = anova_f(&x, &y, 2).unwrap();
        assert!(f[0] > f[1], "{f:?}");
        assert_eq!(f[2], 0.0);
    }

    #[test]
    fn mutual_information_catches_a_dependence_correlation_misses() {
        // The class depends on |x - 10|: symmetric, so linear correlation with
        // x is zero, but x fully determines the class.
        let rows: Vec<Vec<f64>> = (0..=20).map(|i| vec![i as f64]).collect();
        let y: Vec<usize> = (0..=20).map(|i| usize::from((i as i64 - 10).abs() > 5)).collect();
        let x = Matrix::from_rows(&rows).unwrap();
        let yf: Vec<f64> = y.iter().map(|&c| c as f64).collect();
        let r = pearson_scores(&x, &yf).unwrap()[0];
        let mi = mutual_information(&x, &y, 2, 4).unwrap()[0];
        assert!(r < 1e-9, "no linear signal: {r}");
        assert!(mi > 0.3, "but plenty of information: {mi}");
    }

    #[test]
    fn mutual_information_of_an_independent_feature_is_near_zero() {
        // Class alternates; the feature is a slow ramp: no relation.
        let rows: Vec<Vec<f64>> = (0..40).map(|i| vec![(i / 10) as f64]).collect();
        let y: Vec<usize> = (0..40).map(|i| i % 2).collect();
        let mi = mutual_information(&Matrix::from_rows(&rows).unwrap(), &y, 2, 4).unwrap()[0];
        assert!(mi < 1e-12, "{mi}");
    }

    #[test]
    fn forward_selection_picks_the_informative_feature_first() {
        let (x, y, _) = data();
        let folds = stratified_k_fold(&y, 2, 4, 5).unwrap();
        let spec = DecisionTree { max_depth: Some(2), ..Default::default() };
        let steps = forward_selection(&spec, &x, &y, 2, &folds, 2).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].added, 0);
        assert!((steps[0].cv_accuracy - 1.0).abs() < 1e-12, "{steps:?}");
        assert_eq!(steps[1].selected.len(), 2);
    }

    #[test]
    fn lasso_keeps_only_what_the_target_uses() {
        let (x, _, t) = data();
        let kept = lasso_select(&x, &t, 0.5).unwrap();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].feature, 0);
    }
}
