//! Evaluation: splits and folds, classification and regression metrics, the
//! residual comparison of a simulation against measurement, cross-validation,
//! and permutation importance.
//!
//! This module is what separates a finding from a coincidence. Every learner
//! in [`super`] will fit its training data; only a score on rows it never saw
//! says whether it learned anything. The residual comparison applies the same
//! discipline to a simulation: a model of a physical system is only as good as
//! its agreement with what was measured.

use serde::Serialize;

use super::classify::ClassifierSpec;
use super::regress::RegressorSpec;
use super::{Matrix, Rng};
use crate::{DataError, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Splits and folds
// ─────────────────────────────────────────────────────────────────────────────

/// One partition of row indices into training and held-out rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fold {
    pub train: Vec<usize>,
    pub test: Vec<usize>,
}

fn check_fraction(test_fraction: f64) -> Result<()> {
    if !(test_fraction > 0.0 && test_fraction < 1.0) {
        return Err(DataError::Schema(format!(
            "test fraction must be strictly between 0 and 1, got {test_fraction}"
        )));
    }
    Ok(())
}

/// Shuffle `0..n` with `seed` and hold out `test_fraction` of it.
///
/// Both sides always get at least one row, so a tiny dataset produces a
/// degenerate-but-valid split rather than an empty side that would make every
/// downstream metric undefined.
pub fn train_test_split(n: usize, test_fraction: f64, seed: u64) -> Result<Fold> {
    check_fraction(test_fraction)?;
    if n < 2 {
        return Err(DataError::Schema(format!("a split needs at least 2 rows, got {n}")));
    }
    let mut idx: Vec<usize> = (0..n).collect();
    Rng::new(seed).shuffle(&mut idx);
    let n_test = ((n as f64 * test_fraction).round() as usize).clamp(1, n - 1);
    let mut test = idx[..n_test].to_vec();
    let mut train = idx[n_test..].to_vec();
    test.sort_unstable();
    train.sort_unstable();
    Ok(Fold { train, test })
}

/// Group row indices by class, each group shuffled with `seed`.
fn shuffled_by_class(y: &[usize], n_classes: usize, seed: u64) -> Result<Vec<Vec<usize>>> {
    let mut groups: Vec<Vec<usize>> = vec![Vec::new(); n_classes];
    for (i, &c) in y.iter().enumerate() {
        let g = groups.get_mut(c).ok_or_else(|| {
            DataError::Schema(format!("label {c} is outside 0..{n_classes}"))
        })?;
        g.push(i);
    }
    let mut rng = Rng::new(seed);
    for g in &mut groups {
        rng.shuffle(g);
    }
    Ok(groups)
}

/// Split that preserves each class's proportion on both sides.
///
/// Plain random splitting can leave a rare class entirely out of the test set,
/// and then its recall silently vanishes from the report. Every class with at
/// least two members is guaranteed a place on both sides.
pub fn stratified_split(y: &[usize], n_classes: usize, test_fraction: f64, seed: u64) -> Result<Fold> {
    check_fraction(test_fraction)?;
    if y.len() < 2 {
        return Err(DataError::Schema(format!("a split needs at least 2 rows, got {}", y.len())));
    }
    let mut train = Vec::new();
    let mut test = Vec::new();
    for g in shuffled_by_class(y, n_classes, seed)? {
        let c = g.len();
        let n_test = if c >= 2 {
            ((c as f64 * test_fraction).round() as usize).clamp(1, c - 1)
        } else {
            0
        };
        test.extend_from_slice(&g[..n_test]);
        train.extend_from_slice(&g[n_test..]);
    }
    if train.is_empty() || test.is_empty() {
        return Err(DataError::Schema(
            "every class has a single member, so no stratified split exists".into(),
        ));
    }
    train.sort_unstable();
    test.sort_unstable();
    Ok(Fold { train, test })
}

/// `k` folds over `0..n`: each row is held out exactly once.
pub fn k_fold(n: usize, k: usize, seed: u64) -> Result<Vec<Fold>> {
    if k < 2 || n < k {
        return Err(DataError::Schema(format!(
            "k-fold needs 2 <= k <= rows, got k = {k} for {n} rows"
        )));
    }
    let mut idx: Vec<usize> = (0..n).collect();
    Rng::new(seed).shuffle(&mut idx);
    Ok((0..k)
        .map(|f| {
            let (lo, hi) = (f * n / k, (f + 1) * n / k);
            let mut test = idx[lo..hi].to_vec();
            let mut train: Vec<usize> = idx[..lo].iter().chain(&idx[hi..]).copied().collect();
            test.sort_unstable();
            train.sort_unstable();
            Fold { train, test }
        })
        .collect())
}

/// `k` folds that keep each class's proportion in every fold.
///
/// Rows are dealt round-robin across folds class by class, so fold sizes stay
/// within one row of each other and every class spreads as evenly as it can.
pub fn stratified_k_fold(y: &[usize], n_classes: usize, k: usize, seed: u64) -> Result<Vec<Fold>> {
    let n = y.len();
    if k < 2 || n < k {
        return Err(DataError::Schema(format!(
            "k-fold needs 2 <= k <= rows, got k = {k} for {n} rows"
        )));
    }
    let mut fold_of = vec![0usize; n];
    let mut dealt = 0usize;
    for g in shuffled_by_class(y, n_classes, seed)? {
        for i in g {
            fold_of[i] = dealt % k;
            dealt += 1;
        }
    }
    Ok((0..k)
        .map(|f| {
            let test: Vec<usize> = (0..n).filter(|&i| fold_of[i] == f).collect();
            let train: Vec<usize> = (0..n).filter(|&i| fold_of[i] != f).collect();
            Fold { train, test }
        })
        .collect())
}

/// Each distinct group id with its row count, in ascending id order.
fn group_sizes(groups: &[usize]) -> Vec<(usize, usize)> {
    let mut sizes = std::collections::BTreeMap::new();
    for &g in groups {
        *sizes.entry(g).or_insert(0usize) += 1;
    }
    sizes.into_iter().collect()
}

/// Split so that every group's rows land together on one side.
///
/// Rows that share an entity (a supplier, a machine, a production batch)
/// resemble one another. A random split puts some of an entity's rows on each
/// side, and the test score then measures how well the model recognises
/// entities it has already seen. Holding out whole groups measures how it does
/// on new ones, which is usually the question. Groups move as units, so the
/// held-out share of rows is approximate; both sides always get a group.
pub fn group_split(groups: &[usize], test_fraction: f64, seed: u64) -> Result<Fold> {
    check_fraction(test_fraction)?;
    let mut sizes = group_sizes(groups);
    if sizes.len() < 2 {
        return Err(DataError::Schema(format!(
            "a group split needs at least 2 groups, got {}",
            sizes.len()
        )));
    }
    Rng::new(seed).shuffle(&mut sizes);
    let target = ((groups.len() as f64 * test_fraction).round() as usize).max(1);
    let mut held = std::collections::HashSet::new();
    let mut taken = 0;
    // The last group never moves, so the training side cannot end up empty.
    for &(g, size) in &sizes[..sizes.len() - 1] {
        if taken >= target {
            break;
        }
        held.insert(g);
        taken += size;
    }
    let (mut train, mut test) = (Vec::new(), Vec::new());
    for (i, g) in groups.iter().enumerate() {
        if held.contains(g) { test.push(i) } else { train.push(i) }
    }
    Ok(Fold { train, test })
}

/// `k` folds in which every group is held out whole, exactly once.
///
/// Groups are shuffled with `seed`, then dealt largest first to whichever fold
/// holds the fewest rows so far, which keeps the folds as even as whole groups
/// allow.
pub fn group_k_fold(groups: &[usize], k: usize, seed: u64) -> Result<Vec<Fold>> {
    let mut sizes = group_sizes(groups);
    if k < 2 || sizes.len() < k {
        return Err(DataError::Schema(format!(
            "group k-fold needs 2 <= k <= groups, got k = {k} for {} groups",
            sizes.len()
        )));
    }
    Rng::new(seed).shuffle(&mut sizes);
    // A stable sort: groups of equal size keep their shuffled order.
    sizes.sort_by(|a, b| b.1.cmp(&a.1));
    let mut load = vec![0usize; k];
    let mut fold_of = std::collections::HashMap::new();
    for &(g, size) in &sizes {
        // The first of the lightest folds.
        let f = (0..k).min_by_key(|&f| load[f]).unwrap_or(0);
        load[f] += size;
        fold_of.insert(g, f);
    }
    Ok((0..k)
        .map(|f| {
            let (mut train, mut test) = (Vec::new(), Vec::new());
            for (i, g) in groups.iter().enumerate() {
                if fold_of[g] == f { test.push(i) } else { train.push(i) }
            }
            Fold { train, test }
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Classification metrics
// ─────────────────────────────────────────────────────────────────────────────

/// Confusion matrix: `counts[actual][predicted]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Confusion {
    pub n_classes: usize,
    pub counts: Vec<Vec<usize>>,
}

impl Confusion {
    pub fn new(actual: &[usize], predicted: &[usize], n_classes: usize) -> Result<Self> {
        if actual.len() != predicted.len() {
            return Err(DataError::Schema(format!(
                "confusion: {} actual labels but {} predictions",
                actual.len(),
                predicted.len()
            )));
        }
        let mut counts = vec![vec![0usize; n_classes]; n_classes];
        for (&a, &p) in actual.iter().zip(predicted) {
            if a >= n_classes || p >= n_classes {
                return Err(DataError::Schema(format!(
                    "confusion: label {} is outside 0..{n_classes}",
                    a.max(p)
                )));
            }
            counts[a][p] += 1;
        }
        Ok(Self { n_classes, counts })
    }

    pub fn total(&self) -> usize {
        self.counts.iter().flatten().sum()
    }

    /// Rows whose actual class is `c`.
    pub fn support(&self, c: usize) -> usize {
        self.counts[c].iter().sum()
    }

    fn predicted_as(&self, c: usize) -> usize {
        self.counts.iter().map(|row| row[c]).sum()
    }

    pub fn accuracy(&self) -> f64 {
        let t = self.total();
        if t == 0 {
            return 0.0;
        }
        (0..self.n_classes).map(|c| self.counts[c][c]).sum::<usize>() as f64 / t as f64
    }

    /// Of the rows predicted as `c`, the share that really were `c`. Zero when
    /// nothing was predicted as `c` (the convention scikit-learn calls
    /// `zero_division = 0`).
    pub fn precision(&self, c: usize) -> f64 {
        let p = self.predicted_as(c);
        if p == 0 { 0.0 } else { self.counts[c][c] as f64 / p as f64 }
    }

    /// Of the rows that really were `c`, the share predicted as `c`. Zero when
    /// `c` never occurs.
    pub fn recall(&self, c: usize) -> f64 {
        let s = self.support(c);
        if s == 0 { 0.0 } else { self.counts[c][c] as f64 / s as f64 }
    }

    pub fn f1(&self, c: usize) -> f64 {
        let (p, r) = (self.precision(c), self.recall(c));
        if p + r == 0.0 { 0.0 } else { 2.0 * p * r / (p + r) }
    }

    /// Classes that occur in either the actual or the predicted labels. A class
    /// absent from both says nothing about the model and is left out of the
    /// averages, rather than dragging them toward zero.
    fn present(&self) -> Vec<usize> {
        (0..self.n_classes)
            .filter(|&c| self.support(c) > 0 || self.predicted_as(c) > 0)
            .collect()
    }

    /// Unweighted mean F1: every class counts equally, so a model that ignores
    /// a rare class is penalised for it.
    pub fn macro_f1(&self) -> f64 {
        let cs = self.present();
        if cs.is_empty() {
            return 0.0;
        }
        cs.iter().map(|&c| self.f1(c)).sum::<f64>() / cs.len() as f64
    }

    /// F1 weighted by each class's support.
    pub fn weighted_f1(&self) -> f64 {
        let t = self.total();
        if t == 0 {
            return 0.0;
        }
        (0..self.n_classes)
            .map(|c| self.f1(c) * self.support(c) as f64)
            .sum::<f64>()
            / t as f64
    }

    /// Mean recall over classes that occur: accuracy that a majority class
    /// cannot inflate.
    pub fn balanced_accuracy(&self) -> f64 {
        let cs: Vec<usize> = (0..self.n_classes).filter(|&c| self.support(c) > 0).collect();
        if cs.is_empty() {
            return 0.0;
        }
        cs.iter().map(|&c| self.recall(c)).sum::<f64>() / cs.len() as f64
    }
}

/// Area under the ROC curve for a binary problem, by the rank-sum
/// (Mann-Whitney) identity with midranks for ties.
///
/// 1.0 means every positive outscores every negative, 0.5 is chance, 0.0 is a
/// perfectly inverted ranking. Undefined, and refused, without both classes.
pub fn roc_auc(positive: &[bool], scores: &[f64]) -> Result<f64> {
    if positive.len() != scores.len() {
        return Err(DataError::Schema("roc_auc: labels and scores differ in length".into()));
    }
    if scores.iter().any(|s| !s.is_finite()) {
        return Err(DataError::Schema("roc_auc: scores must be finite".into()));
    }
    let n_pos = positive.iter().filter(|&&p| p).count();
    let n_neg = positive.len() - n_pos;
    if n_pos == 0 || n_neg == 0 {
        return Err(DataError::Schema(
            "roc_auc needs at least one positive and one negative example".into(),
        ));
    }
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[a].total_cmp(&scores[b]));
    let mut rank = vec![0.0; scores.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && scores[order[j + 1]] == scores[order[i]] {
            j += 1;
        }
        // Ranks are 1-based; tied scores share the mean of their positions.
        let mid = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            rank[k] = mid;
        }
        i = j + 1;
    }
    let pos_rank_sum: f64 = (0..scores.len()).filter(|&k| positive[k]).map(|k| rank[k]).sum();
    let (p, q) = (n_pos as f64, n_neg as f64);
    Ok((pos_rank_sum - p * (p + 1.0) / 2.0) / (p * q))
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression metrics and residuals
// ─────────────────────────────────────────────────────────────────────────────

/// Goodness of fit for a regression.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegressionMetrics {
    pub n: usize,
    /// Coefficient of determination. 1 is perfect, 0 matches predicting the
    /// mean, negative is worse than the mean.
    pub r2: f64,
    pub rmse: f64,
    pub mae: f64,
    pub max_error: f64,
    /// Mean of `predicted - actual`: positive means the model reads high.
    pub bias: f64,
}

fn finite_pairs(a: &[f64], b: &[f64], what: &str) -> Result<Vec<(f64, f64)>> {
    if a.len() != b.len() {
        return Err(DataError::Schema(format!(
            "{what}: {} values against {}",
            a.len(),
            b.len()
        )));
    }
    let pairs: Vec<(f64, f64)> = a
        .iter()
        .zip(b)
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .map(|(x, y)| (*x, *y))
        .collect();
    if pairs.is_empty() {
        return Err(DataError::Schema(format!("{what}: no pair of finite values to compare")));
    }
    Ok(pairs)
}

fn metrics_from_pairs(pairs: &[(f64, f64)]) -> RegressionMetrics {
    let n = pairs.len() as f64;
    let mean_a = pairs.iter().map(|p| p.0).sum::<f64>() / n;
    let (mut ss_res, mut ss_tot, mut abs, mut max, mut bias) = (0.0, 0.0, 0.0, 0.0f64, 0.0);
    for &(a, p) in pairs {
        let e = p - a;
        ss_res += e * e;
        ss_tot += (a - mean_a) * (a - mean_a);
        abs += e.abs();
        max = max.max(e.abs());
        bias += e;
    }
    // A constant target makes R² undefined; report 1 for an exact match and 0
    // otherwise (scikit-learn's convention) rather than dividing by zero.
    let r2 = if ss_tot == 0.0 {
        if ss_res == 0.0 { 1.0 } else { 0.0 }
    } else {
        1.0 - ss_res / ss_tot
    };
    RegressionMetrics {
        n: pairs.len(),
        r2,
        rmse: (ss_res / n).sqrt(),
        mae: abs / n,
        max_error: max,
        bias: bias / n,
    }
}

/// Regression metrics over paired values; non-finite pairs are skipped.
pub fn regression_metrics(actual: &[f64], predicted: &[f64]) -> Result<RegressionMetrics> {
    Ok(metrics_from_pairs(&finite_pairs(actual, predicted, "regression_metrics")?))
}

/// How far a simulation's output sits from measured data.
///
/// This is the score that decides everything interventional: whether a model
/// is calibrated, which forked branch won, and how much a withheld input was
/// worth. It is deliberately the same arithmetic as [`RegressionMetrics`] with
/// the simulation in the "predicted" seat, so a simulation and a statistical
/// model are judged by one standard.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Residuals {
    /// Measured points that had a simulated value to compare against.
    pub n: usize,
    pub rmse: f64,
    pub mae: f64,
    /// Mean of `simulated - measured`: positive means the simulation reads high.
    pub bias: f64,
    pub max_abs: f64,
    /// RMSE over the measured range, so fits of quantities in different units
    /// can be compared. Absent when the measured values are constant.
    pub nrmse: Option<f64>,
    pub r2: f64,
}

impl Residuals {
    /// Every compared point within `tolerance` of its measurement.
    pub fn within(&self, tolerance: f64) -> bool {
        self.max_abs <= tolerance
    }
}

fn residuals_from_pairs(pairs: &[(f64, f64)]) -> Residuals {
    let m = metrics_from_pairs(pairs);
    let (lo, hi) = pairs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    let range = hi - lo;
    Residuals {
        n: m.n,
        rmse: m.rmse,
        mae: m.mae,
        bias: m.bias,
        max_abs: m.max_error,
        nrmse: if range > 0.0 { Some(m.rmse / range) } else { None },
        r2: m.r2,
    }
}

/// Compare a simulation against measurement, point for point.
pub fn compare(measured: &[f64], simulated: &[f64]) -> Result<Residuals> {
    Ok(residuals_from_pairs(&finite_pairs(measured, simulated, "compare")?))
}

/// Compare two series sampled on their own time axes.
///
/// The simulated series is linearly interpolated onto each measured time.
/// Measured points outside the simulated span are skipped, never
/// extrapolated: a comparison must not invent the simulation's output where it
/// produced none.
pub fn compare_series(t_meas: &[f64], y_meas: &[f64], t_sim: &[f64], y_sim: &[f64]) -> Result<Residuals> {
    if t_meas.len() != y_meas.len() || t_sim.len() != y_sim.len() {
        return Err(DataError::Schema("compare_series: a time axis and its values differ in length".into()));
    }
    let mut sim: Vec<(f64, f64)> = t_sim
        .iter()
        .zip(y_sim)
        .filter(|(t, y)| t.is_finite() && y.is_finite())
        .map(|(t, y)| (*t, *y))
        .collect();
    if sim.is_empty() {
        return Err(DataError::Schema("compare_series: the simulated series is empty".into()));
    }
    sim.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (t0, t1) = (sim[0].0, sim[sim.len() - 1].0);

    let mut pairs = Vec::with_capacity(t_meas.len());
    for (&t, &y) in t_meas.iter().zip(y_meas) {
        if !t.is_finite() || !y.is_finite() || t < t0 || t > t1 {
            continue;
        }
        // First simulated sample at or after t.
        let hi = sim.partition_point(|p| p.0 < t);
        let s = if hi < sim.len() && sim[hi].0 == t {
            sim[hi].1
        } else {
            let (a, b) = (sim[hi - 1], sim[hi]);
            a.1 + (b.1 - a.1) * (t - a.0) / (b.0 - a.0)
        };
        pairs.push((y, s));
    }
    if pairs.is_empty() {
        return Err(DataError::Schema(
            "compare_series: no measured time falls inside the simulated span".into(),
        ));
    }
    Ok(residuals_from_pairs(&pairs))
}

// ─────────────────────────────────────────────────────────────────────────────
// Cross-validation
// ─────────────────────────────────────────────────────────────────────────────

/// Scores across folds, with their mean and spread.
///
/// The spread matters as much as the mean: a model whose score swings widely
/// between folds is telling you its average is not to be trusted.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CvSummary {
    pub scores: Vec<f64>,
    pub mean: f64,
    /// Sample standard deviation across folds.
    pub std: f64,
}

impl CvSummary {
    pub fn from_scores(scores: Vec<f64>) -> Self {
        let n = scores.len() as f64;
        let mean = if scores.is_empty() { 0.0 } else { scores.iter().sum::<f64>() / n };
        let std = if scores.len() < 2 {
            0.0
        } else {
            (scores.iter().map(|s| (s - mean) * (s - mean)).sum::<f64>() / (n - 1.0)).sqrt()
        };
        Self { scores, mean, std }
    }
}

/// Cross-validated accuracy and macro F1 of a classifier spec.
pub fn cross_validate_classifier(
    spec: &dyn ClassifierSpec,
    x: &Matrix,
    y: &[usize],
    n_classes: usize,
    folds: &[Fold],
) -> Result<(CvSummary, CvSummary)> {
    let mut acc = Vec::with_capacity(folds.len());
    let mut f1 = Vec::with_capacity(folds.len());
    for fold in folds {
        let ytr: Vec<usize> = fold.train.iter().map(|&i| y[i]).collect();
        let yte: Vec<usize> = fold.test.iter().map(|&i| y[i]).collect();
        let model = spec.fit(&x.select_rows(&fold.train), &ytr, n_classes)?;
        let pred = model.predict(&x.select_rows(&fold.test))?;
        let cm = Confusion::new(&yte, &pred, n_classes)?;
        acc.push(cm.accuracy());
        f1.push(cm.macro_f1());
    }
    Ok((CvSummary::from_scores(acc), CvSummary::from_scores(f1)))
}

/// Cross-validated R² and RMSE of a regressor spec.
pub fn cross_validate_regressor(
    spec: &dyn RegressorSpec,
    x: &Matrix,
    y: &[f64],
    folds: &[Fold],
) -> Result<(CvSummary, CvSummary)> {
    let mut r2 = Vec::with_capacity(folds.len());
    let mut rmse = Vec::with_capacity(folds.len());
    for fold in folds {
        let ytr: Vec<f64> = fold.train.iter().map(|&i| y[i]).collect();
        let yte: Vec<f64> = fold.test.iter().map(|&i| y[i]).collect();
        let model = spec.fit(&x.select_rows(&fold.train), &ytr)?;
        let pred = model.predict(&x.select_rows(&fold.test))?;
        let m = regression_metrics(&yte, &pred)?;
        r2.push(m.r2);
        rmse.push(m.rmse);
    }
    Ok((CvSummary::from_scores(r2), CvSummary::from_scores(rmse)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Permutation importance
// ─────────────────────────────────────────────────────────────────────────────

/// How much a fitted model's score falls when one feature is scrambled.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Importance {
    pub feature: usize,
    /// Mean of `baseline - permuted` over the repeats: the score the feature
    /// was carrying. Near zero means the model does not rely on it.
    pub mean_drop: f64,
    pub std_drop: f64,
}

/// Permutation importance: shuffle one column at a time and measure the loss.
///
/// This is ablation performed on data rather than on a world. Shuffling a
/// column keeps its distribution but severs its link to the target, so the
/// score that disappears is exactly what the model drew from that feature. It
/// is model-agnostic: `score` wraps any fitted model and returns a
/// higher-is-better number (accuracy, R², negative RMSE) on the given rows.
///
/// Measure it on held-out rows. On training rows it reports what the model
/// memorised, not what generalises.
pub fn permutation_importance(
    x: &Matrix,
    score: impl Fn(&Matrix) -> Result<f64>,
    repeats: usize,
    seed: u64,
) -> Result<Vec<Importance>> {
    if repeats == 0 {
        return Err(DataError::Schema("permutation importance needs at least one repeat".into()));
    }
    if x.rows() < 2 {
        return Err(DataError::Schema("permutation importance needs at least 2 rows".into()));
    }
    let baseline = score(x)?;
    let mut out = Vec::with_capacity(x.cols());
    for j in 0..x.cols() {
        // A seed per feature, so a feature's result does not depend on how many
        // features were scored before it.
        let mut rng = Rng::new(seed ^ ((j as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        let mut drops = Vec::with_capacity(repeats);
        for _ in 0..repeats {
            let mut col = x.column(j);
            rng.shuffle(&mut col);
            let mut permuted = x.clone();
            for (i, v) in col.into_iter().enumerate() {
                permuted.set(i, j, v);
            }
            drops.push(baseline - score(&permuted)?);
        }
        let s = CvSummary::from_scores(drops);
        out.push(Importance { feature: j, mean_drop: s.mean, std_drop: s.std });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_is_disjoint_complete_and_reproducible() {
        let a = train_test_split(100, 0.25, 9).unwrap();
        assert_eq!(a.test.len(), 25);
        assert_eq!(a.train.len(), 75);
        let mut all: Vec<usize> = a.train.iter().chain(&a.test).copied().collect();
        all.sort_unstable();
        assert_eq!(all, (0..100).collect::<Vec<_>>());
        assert_eq!(a, train_test_split(100, 0.25, 9).unwrap(), "same seed, same split");
        assert_ne!(a, train_test_split(100, 0.25, 10).unwrap(), "new seed, new split");
    }

    #[test]
    fn a_split_refuses_a_meaningless_fraction() {
        assert!(train_test_split(10, 0.0, 1).is_err());
        assert!(train_test_split(10, 1.0, 1).is_err());
        assert!(train_test_split(1, 0.5, 1).is_err());
    }

    #[test]
    fn stratification_keeps_a_rare_class_on_both_sides() {
        // 90 of class 0, 10 of class 1.
        let y: Vec<usize> = (0..100).map(|i| usize::from(i >= 90)).collect();
        let f = stratified_split(&y, 2, 0.2, 3).unwrap();
        let rare_test = f.test.iter().filter(|&&i| y[i] == 1).count();
        let rare_train = f.train.iter().filter(|&&i| y[i] == 1).count();
        assert_eq!(rare_test, 2, "20% of the 10 rare rows");
        assert_eq!(rare_train, 8);
    }

    #[test]
    fn every_row_is_held_out_exactly_once_across_folds() {
        let folds = k_fold(23, 5, 4).unwrap();
        let mut seen = vec![0; 23];
        for f in &folds {
            for &i in &f.test {
                seen[i] += 1;
            }
            assert_eq!(f.train.len() + f.test.len(), 23);
        }
        assert!(seen.iter().all(|&c| c == 1), "{seen:?}");
    }

    #[test]
    fn stratified_folds_are_balanced_and_cover_everything() {
        let y: Vec<usize> = (0..30).map(|i| i % 3).collect();
        let folds = stratified_k_fold(&y, 3, 5, 2).unwrap();
        let mut seen = vec![0; 30];
        for f in &folds {
            assert_eq!(f.test.len(), 6);
            for c in 0..3 {
                assert_eq!(f.test.iter().filter(|&&i| y[i] == c).count(), 2, "class {c}");
            }
            for &i in &f.test {
                seen[i] += 1;
            }
        }
        assert!(seen.iter().all(|&c| c == 1));
    }

    #[test]
    fn a_group_split_never_divides_a_group() {
        // 10 groups of 3 rows, interleaved so no group is contiguous.
        let groups: Vec<usize> = (0..30).map(|i| i % 10).collect();
        let f = group_split(&groups, 0.3, 5).unwrap();
        for g in 0..10 {
            let in_test = f.test.iter().any(|&i| groups[i] == g);
            let in_train = f.train.iter().any(|&i| groups[i] == g);
            assert!(in_test != in_train, "group {g} sits on exactly one side");
        }
        assert_eq!(f.test.len(), 9, "three whole groups make up the 30%");
        assert_eq!(f.train.len(), 21);
        assert!(group_split(&[4, 4, 4], 0.5, 0).is_err(), "one group cannot be split");
    }

    #[test]
    fn group_folds_hold_each_group_out_whole_exactly_once() {
        // 8 groups of 5 rows.
        let groups: Vec<usize> = (0..40).map(|i| (i * 7) % 8).collect();
        let folds = group_k_fold(&groups, 4, 1).unwrap();
        let mut held = [0; 8];
        for f in &folds {
            assert_eq!(f.test.len(), 10, "equal groups deal evenly");
            let mut gs: Vec<usize> = f.test.iter().map(|&i| groups[i]).collect();
            gs.sort_unstable();
            gs.dedup();
            for &g in &gs {
                held[g] += 1;
                assert!(f.train.iter().all(|&i| groups[i] != g), "group {g} leaks into training");
            }
        }
        assert!(held.iter().all(|&h| h == 1));
        assert!(group_k_fold(&groups, 9, 1).is_err(), "more folds than groups");
    }

    #[test]
    fn confusion_metrics_match_a_worked_example() {
        // actual:    0 0 0 0 1 1 1 1 1 1
        // predicted: 0 0 0 1 1 1 1 1 0 0
        let a = [0, 0, 0, 0, 1, 1, 1, 1, 1, 1];
        let p = [0, 0, 0, 1, 1, 1, 1, 1, 0, 0];
        let cm = Confusion::new(&a, &p, 2).unwrap();
        assert_eq!(cm.counts, vec![vec![3, 1], vec![2, 4]]);
        assert!((cm.accuracy() - 0.7).abs() < 1e-12);
        // class 1: tp 4, fp 1, fn 2
        assert!((cm.precision(1) - 0.8).abs() < 1e-12);
        assert!((cm.recall(1) - 4.0 / 6.0).abs() < 1e-12);
        let f1 = 2.0 * 0.8 * (4.0 / 6.0) / (0.8 + 4.0 / 6.0);
        assert!((cm.f1(1) - f1).abs() < 1e-12);
        // class 0: tp 3, fp 2, fn 1 -> precision 0.6, recall 0.75
        assert!((cm.balanced_accuracy() - (0.75 + 4.0 / 6.0) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_class_never_predicted_scores_zero_rather_than_nan() {
        let cm = Confusion::new(&[0, 1, 1], &[0, 0, 0], 2).unwrap();
        assert_eq!(cm.precision(1), 0.0);
        assert_eq!(cm.f1(1), 0.0);
        assert!(cm.macro_f1().is_finite());
    }

    #[test]
    fn macro_f1_ignores_a_class_absent_from_both_sides() {
        // Class 2 exists in the label space but in neither vector.
        let cm = Confusion::new(&[0, 1], &[0, 1], 3).unwrap();
        assert!((cm.macro_f1() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn roc_auc_is_one_for_a_perfect_ranking_zero_inverted_and_half_for_ties() {
        let y = [false, false, true, true];
        assert!((roc_auc(&y, &[0.1, 0.2, 0.8, 0.9]).unwrap() - 1.0).abs() < 1e-12);
        assert!((roc_auc(&y, &[0.9, 0.8, 0.2, 0.1]).unwrap() - 0.0).abs() < 1e-12);
        assert!((roc_auc(&y, &[0.5; 4]).unwrap() - 0.5).abs() < 1e-12);
        // One inversion out of four positive-negative pairs.
        assert!((roc_auc(&y, &[0.1, 0.85, 0.8, 0.9]).unwrap() - 0.75).abs() < 1e-12);
        assert!(roc_auc(&[true, true], &[0.1, 0.2]).is_err());
    }

    #[test]
    fn regression_metrics_on_a_known_case() {
        let m = regression_metrics(&[1.0, 2.0, 3.0], &[1.0, 2.0, 5.0]).unwrap();
        assert!((m.rmse - (4.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((m.mae - 2.0 / 3.0).abs() < 1e-12);
        assert!((m.max_error - 2.0).abs() < 1e-12);
        assert!((m.bias - 2.0 / 3.0).abs() < 1e-12);
        // ss_res = 4, ss_tot = 2
        assert!((m.r2 - (1.0 - 4.0 / 2.0)).abs() < 1e-12);
        let perfect = regression_metrics(&[1.0, 2.0], &[1.0, 2.0]).unwrap();
        assert_eq!(perfect.r2, 1.0);
        assert_eq!(perfect.rmse, 0.0);
    }

    #[test]
    fn a_constant_offset_shows_up_as_bias_not_noise() {
        let meas = [1.0, 2.0, 3.0, 4.0];
        let sim: Vec<f64> = meas.iter().map(|v| v + 0.5).collect();
        let r = compare(&meas, &sim).unwrap();
        assert!((r.bias - 0.5).abs() < 1e-12, "the simulation reads high by 0.5");
        assert!((r.rmse - 0.5).abs() < 1e-12);
        assert!((r.max_abs - 0.5).abs() < 1e-12);
        assert!((r.nrmse.unwrap() - 0.5 / 3.0).abs() < 1e-12);
        assert!(r.within(0.5) && !r.within(0.49));
    }

    #[test]
    fn series_on_different_clocks_are_compared_by_interpolation() {
        // Simulation sampled every 1 s: y = 2t.
        let t_sim: Vec<f64> = (0..=10).map(|t| t as f64).collect();
        let y_sim: Vec<f64> = t_sim.iter().map(|t| 2.0 * t).collect();
        // Measured at half-seconds on the same law, plus one point past the end.
        let t_meas = [0.5, 2.5, 7.25, 12.0];
        let y_meas = [1.0, 5.0, 14.5, 24.0];
        let r = compare_series(&t_meas, &y_meas, &t_sim, &y_sim).unwrap();
        assert_eq!(r.n, 3, "t = 12 lies past the simulation and is not extrapolated");
        assert!(r.rmse < 1e-12, "a linear law interpolates exactly: {r:?}");
    }

    #[test]
    fn series_that_never_overlap_are_refused() {
        let err = compare_series(&[100.0], &[1.0], &[0.0, 1.0], &[0.0, 1.0]);
        assert!(err.is_err());
    }

    #[test]
    fn cv_summary_reports_the_spread() {
        let s = CvSummary::from_scores(vec![0.8, 0.9, 1.0]);
        assert!((s.mean - 0.9).abs() < 1e-12);
        assert!((s.std - 0.1).abs() < 1e-12);
    }

    #[test]
    fn scrambling_the_feature_a_score_depends_on_costs_the_score() {
        // Score = negative squared error of predicting y from column 0 alone.
        // Column 1 is noise the score never reads.
        let rows: Vec<Vec<f64>> = (0..40).map(|i| vec![i as f64, ((i * 7) % 11) as f64]).collect();
        let x = Matrix::from_rows(&rows).unwrap();
        let y: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let score = |m: &Matrix| -> Result<f64> {
            Ok(-(0..m.rows()).map(|i| (m.get(i, 0) - y[i]).powi(2)).sum::<f64>())
        };
        let imp = permutation_importance(&x, score, 5, 11).unwrap();
        assert!(imp[0].mean_drop > 100.0, "the informative column: {:?}", imp[0]);
        assert_eq!(imp[1].mean_drop, 0.0, "the ignored column costs nothing");
    }
}
