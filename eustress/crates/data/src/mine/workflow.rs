//! One-call mining runs over a [`Frame`], each returning a report that
//! serializes to JSON.
//!
//! The sibling modules are the parts; these are the assembled procedures, with
//! the evaluation protocol built in so a run cannot skip it:
//!
//! 1. Rows with a missing value in a used column are dropped and counted.
//! 2. The held-out rows are split off first: stratified by class, or by whole
//!    entity when [`RunOptions::group`] names an entity column.
//! 3. Everything fitted (scalers, models, feature choices) sees training rows
//!    only.
//! 4. Cross-validation runs inside the training rows, so the test rows are
//!    used exactly once, for the final score.
//! 5. Every score sits next to a trivial baseline, because a score without one
//!    cannot be read.
//!
//! Each report also carries [`Caution`]s: pitfalls the run detected in the data
//! or in the result, from a feature that is a disguised copy of the target to
//! a model that does no better than guessing. Every caution has a stable code
//! from [`code`], so an agent can act on it as reliably as a person reads it.
//! Every report records its [`RunOptions`], seed included, so the run can be
//! repeated exactly.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::assoc::{transactions_from_flags, transactions_from_frame, Apriori, Mined};
use super::classify::{
    Classifier, ClassifierSpec, DecisionTree, GaussianNaiveBayes, KNearestNeighbors, LogisticRegression,
};
use super::cluster::{calinski_harabasz, davies_bouldin, silhouette, Agglomerative, Dbscan, KMeans};
use super::eval::{
    compare, compare_series, cross_validate_classifier, cross_validate_regressor, group_k_fold, group_split,
    k_fold, permutation_importance, regression_metrics, roc_auc, stratified_k_fold, stratified_split,
    train_test_split, Confusion, CvSummary, Fold, Importance, RegressionMetrics, Residuals,
};
use super::reduce::Pca;
use super::regress::{Lasso, LinearRegression, Regressor, RegressorSpec};
use super::select::{anova_f, forward_selection, mutual_information, pearson_scores, quantile_bins, ranked};
use super::{
    argmax, cell_text, features as extract_features, features_and_labels, features_and_target, Extracted,
    Labels, Matrix, Scaler,
};
use crate::numerics::as_f64_opt;
use crate::{ColumnData, ColumnDtype, ColumnSpec, DataError, Frame, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Options and cautions
// ─────────────────────────────────────────────────────────────────────────────

/// Settings shared by the supervised runs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunOptions {
    /// Share of rows held out for the final test score.
    pub test_fraction: f64,
    /// Seed for every random choice in the run: the split, the folds, and the
    /// permutations.
    pub seed: u64,
    /// Cross-validation folds within the training rows; below 2 skips it.
    pub cv_folds: usize,
    /// Shuffles per feature for permutation importance; 0 skips it.
    pub importance_repeats: usize,
    /// Column naming the entity each row belongs to (a supplier, a machine, a
    /// batch). When set, the split and the folds keep each entity's rows on
    /// one side, so the scores describe entities the model has never seen.
    pub group: Option<String>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { test_fraction: 0.25, seed: 0, cv_folds: 5, importance_repeats: 5, group: None }
    }
}

/// A pitfall a run detected, in words and as a stable code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Caution {
    /// One of the constants in [`code`].
    pub code: &'static str,
    pub message: String,
}

/// The stable codes a [`Caution`] carries. They are never renamed, so a program
/// can match on them.
pub mod code {
    /// Over a tenth of the rows were dropped for a missing value.
    pub const ROWS_DROPPED: &str = "rows_dropped";
    /// A feature takes one value in every row used.
    pub const CONSTANT_FEATURE: &str = "constant_feature";
    /// Fewer than ten training rows per feature.
    pub const FEW_ROWS: &str = "few_rows";
    /// Two features correlate at |r| >= 0.98.
    pub const COLLINEAR: &str = "collinear_features";
    /// One class makes up at least 80% of the rows.
    pub const IMBALANCED: &str = "class_imbalance";
    /// A class has fewer training rows than there are folds.
    pub const RARE_CLASS: &str = "rare_class";
    /// One feature alone separates every class.
    pub const PERFECT_SEPARATOR: &str = "perfect_separator";
    /// One feature tracks a numeric target at |r| >= 0.999.
    pub const TARGET_COPY: &str = "target_copy";
    /// Test rows repeat training rows exactly, in data with continuous values.
    pub const TRAIN_TEST_OVERLAP: &str = "train_test_overlap";
    /// Rows without a group value were each treated as a group of their own.
    pub const GROUP_MISSING: &str = "group_missing";
    /// The model does not beat the trivial baseline on the test rows.
    pub const NO_BETTER_THAN_BASELINE: &str = "no_better_than_baseline";
    /// The training score far exceeds the test score.
    pub const OVERFIT: &str = "overfit";
    /// Cross-validated scores spread widely between folds.
    pub const UNSTABLE_CV: &str = "unstable_cv";
    /// One feature carries nearly all of what the model uses.
    pub const DOMINANT_FEATURE: &str = "dominant_feature";
    /// Models were ranked on the test rows, since no folds ran.
    pub const RANKED_ON_TEST: &str = "ranked_on_test";
    /// Selecting features on all rows inflated the cross-validated score.
    pub const SELECTION_BIAS: &str = "selection_bias";
    /// The selected features change from fold to fold.
    pub const UNSTABLE_SELECTION: &str = "unstable_selection";
    /// Unstandardized features differ in variance by a factor of 1000 or more.
    pub const MIXED_SCALES: &str = "mixed_scales";
    /// Clustering found fewer than two clusters.
    pub const SINGLE_CLUSTER: &str = "single_cluster";
    /// DBSCAN labelled over half the rows as noise.
    pub const MOSTLY_NOISE: &str = "mostly_noise";
    /// An iterative fit stopped at its iteration limit before converging.
    pub const NOT_CONVERGED: &str = "not_converged";
}

fn warn(out: &mut Vec<Caution>, code: &'static str, message: String) {
    out.push(Caution { code, message });
}

/// An independent seed for one purpose within a run, so the split, the folds
/// and the permutations never share a random stream.
fn sub_seed(seed: u64, purpose: u64) -> u64 {
    seed ^ purpose.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

const SPLIT: u64 = 1;
const FOLDS: u64 = 2;
const PERMUTE: u64 = 3;

/// Diagnostics that are quadratic in rows run on at most this many.
const DIAGNOSTIC_ROWS: usize = 5000;
/// The perfect-separator check sorts every feature; it runs on at most this
/// many rows.
const SEPARATOR_ROWS: usize = 50_000;

/// `cap` evenly spaced indices into `0..n`, or all of them when `n <= cap`.
/// Deterministic, and spread across the data rather than taken from one end.
fn spaced(n: usize, cap: usize) -> Vec<usize> {
    if n <= cap {
        (0..n).collect()
    } else {
        (0..cap).map(|i| i * n / cap).collect()
    }
}

fn quoted(names: &[String]) -> String {
    let q: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
    match q.len() {
        0 => String::new(),
        1 => q[0].clone(),
        n => format!("{} and {}", q[..n - 1].join(", "), q[n - 1]),
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn variance(v: &[f64]) -> f64 {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    v.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n
}

/// The index of the first largest count.
fn first_max(count: &[usize]) -> usize {
    (0..count.len()).fold(0, |best, i| if count[i] > count[best] { i } else { best })
}

// ─────────────────────────────────────────────────────────────────────────────
// Data diagnostics
// ─────────────────────────────────────────────────────────────────────────────

fn standardized(c: &[f64]) -> Option<Vec<f64>> {
    let n = c.len() as f64;
    let mean = c.iter().sum::<f64>() / n;
    let sd = (c.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n).sqrt();
    (sd > 0.0 && sd > 1e-12 * mean.abs()).then(|| c.iter().map(|v| (v - mean) / sd).collect())
}

/// Cautions about the extracted data itself: dropped rows, constant features,
/// near-duplicate features.
fn data_cautions(ex: &Extracted, out: &mut Vec<Caution>) {
    let used = ex.x.rows();
    let total = used + ex.dropped;
    if ex.dropped * 10 > total {
        warn(
            out,
            code::ROWS_DROPPED,
            format!(
                "{} of {total} rows ({:.0}%) were dropped for a missing value. If whether a value \
                 is missing relates to the target, the rows that remain are a biased sample.",
                ex.dropped,
                100.0 * ex.dropped as f64 / total as f64
            ),
        );
    }
    let d = ex.x.cols();
    let constant: Vec<String> = (0..d)
        .filter(|&j| {
            let first = ex.x.get(0, j);
            (1..used).all(|i| ex.x.get(i, j) == first)
        })
        .map(|j| ex.feature_names[j].clone())
        .collect();
    if !constant.is_empty() {
        warn(
            out,
            code::CONSTANT_FEATURE,
            format!(
                "{} never {} in the rows used, so no model can learn anything from {}.",
                quoted(&constant),
                plural(constant.len(), "varies", "vary"),
                plural(constant.len(), "it", "them")
            ),
        );
    }
    // Pairwise correlation is quadratic in features; past 200 it is skipped.
    if (2..=200).contains(&d) {
        let rows = spaced(used, DIAGNOSTIC_ROWS);
        let z: Vec<Option<Vec<f64>>> = (0..d)
            .map(|j| standardized(&rows.iter().map(|&i| ex.x.get(i, j)).collect::<Vec<f64>>()))
            .collect();
        let m = rows.len() as f64;
        let mut pairs = Vec::new();
        for a in 0..d {
            for b in a + 1..d {
                if let (Some(za), Some(zb)) = (&z[a], &z[b]) {
                    let r = za.iter().zip(zb).map(|(p, q)| p * q).sum::<f64>() / m;
                    if r.abs() >= 0.98 {
                        pairs.push(format!(
                            "`{}` and `{}` (r = {r:.3})",
                            ex.feature_names[a], ex.feature_names[b]
                        ));
                    }
                }
            }
        }
        if !pairs.is_empty() {
            let shown = pairs.len().min(5);
            let more = if pairs.len() > shown {
                let rest = pairs.len() - shown;
                format!(", and {rest} more {}", plural(rest, "pair", "pairs"))
            } else {
                String::new()
            };
            warn(
                out,
                code::COLLINEAR,
                format!(
                    "{}{more} move almost in lockstep. A model splits one effect between them \
                     arbitrarily, so neither one's coefficient or importance is reliable alone.",
                    pairs[..shown].join("; ")
                ),
            );
        }
    }
}

fn few_rows_caution(train_rows: usize, d: usize, out: &mut Vec<Caution>) {
    if train_rows < 10 * d {
        warn(
            out,
            code::FEW_ROWS,
            format!(
                "{train_rows} training rows for {d} {}, under the rule of thumb of ten rows per \
                 feature. Expect the scores to move with the seed.",
                plural(d, "feature", "features")
            ),
        );
    }
}

/// One 64-bit hash per row, so remembering every training row costs a few
/// bytes each rather than a copy of the row. A collision between two
/// different rows is vanishingly rare at 64 bits, and would only add a
/// caution, never change a score.
fn row_key(row: &[f64]) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    for &v in row {
        // -0.0 and 0.0 are the same value.
        (if v == 0.0 { 0 } else { v.to_bits() }).hash(&mut h);
    }
    h.finish()
}

fn distinct_values(v: impl Iterator<Item = f64>) -> usize {
    let mut keys: Vec<u64> = v.map(|x| if x == 0.0 { 0 } else { x.to_bits() }).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.len()
}

/// Exact repeats of training rows among the test rows. Repeats are expected
/// when every feature takes few values; they point to duplicated records only
/// when some feature is continuous, where a coincidental exact match is
/// unlikely.
fn overlap_caution(train: &Matrix, test: &Matrix, out: &mut Vec<Caution>) {
    let n = train.rows();
    let continuous = (0..train.cols()).any(|j| distinct_values((0..n).map(|i| train.get(i, j))) * 5 >= n);
    if !continuous {
        return;
    }
    let seen: HashSet<u64> = (0..n).map(|i| row_key(train.row(i))).collect();
    let repeats = (0..test.rows()).filter(|&i| seen.contains(&row_key(test.row(i)))).count();
    if repeats > 0 {
        warn(
            out,
            code::TRAIN_TEST_OVERLAP,
            format!(
                "{repeats} of {} test rows repeat a training row exactly. A model can score those \
                 from memory, which inflates the test score. Remove duplicate records, or name a \
                 group column so each entity's rows stay on one side.",
                test.rows()
            ),
        );
    }
}

/// Whether one feature alone orders the rows into class-pure runs, one run per
/// class: a single-feature rule that classifies every row correctly.
fn separates_perfectly(col: &[f64], y: &[usize]) -> bool {
    if col.is_empty() {
        return false;
    }
    let mut idx: Vec<usize> = (0..col.len()).collect();
    idx.sort_by(|&a, &b| col[a].total_cmp(&col[b]));
    let mut seen = HashSet::new();
    seen.insert(y[idx[0]]);
    for w in idx.windows(2) {
        let (a, b) = (w[0], w[1]);
        // A class change must fall between two different values, into a
        // class not seen before.
        if y[a] != y[b] && (col[a] == col[b] || !seen.insert(y[b])) {
            return false;
        }
    }
    seen.len() >= 2
}

fn separator_caution(ex: &Extracted, y: &[usize], target: &str, out: &mut Vec<Caution>) {
    let rows = spaced(ex.x.rows(), SEPARATOR_ROWS);
    let ys: Vec<usize> = rows.iter().map(|&i| y[i]).collect();
    let found: Vec<String> = (0..ex.x.cols())
        .filter(|&j| {
            let col: Vec<f64> = rows.iter().map(|&i| ex.x.get(i, j)).collect();
            separates_perfectly(&col, &ys)
        })
        .map(|j| ex.feature_names[j].clone())
        .collect();
    if found.is_empty() {
        return;
    }
    let scope = if rows.len() < ex.x.rows() {
        format!(" (checked on {} evenly spaced rows)", rows.len())
    } else {
        String::new()
    };
    warn(
        out,
        code::PERFECT_SEPARATOR,
        format!(
            "{} alone {} every class perfectly{scope}. Either the classes really are that easy \
             to tell apart, or the feature is derived from `{target}` or recorded after it. \
             Confirm which before trusting a model built on it.",
            quoted(&found),
            plural(found.len(), "separates", "each separate"),
        ),
    );
}

fn target_copy_caution(ex: &Extracted, y: &[f64], target: &str, out: &mut Vec<Caution>) -> Result<()> {
    let r = pearson_scores(&ex.x, y)?;
    let copies: Vec<String> = r
        .iter()
        .enumerate()
        .filter(|&(_, &v)| v >= 0.999)
        .map(|(j, v)| format!("`{}` (|r| = {v:.4})", ex.feature_names[j]))
        .collect();
    if !copies.is_empty() {
        warn(
            out,
            code::TARGET_COPY,
            format!(
                "{} {} `{target}` almost exactly. {} may be the target in other units, or \
                 computed from it; if so, the model has been handed the answer.",
                copies.join(", "),
                plural(copies.len(), "tracks", "track"),
                plural(copies.len(), "It", "They")
            ),
        );
    }
    Ok(())
}

fn mixed_scales_caution(ex: &Extracted, out: &mut Vec<Caution>) {
    let vars: Vec<f64> = (0..ex.x.cols()).map(|j| variance(&ex.x.column(j))).collect();
    let hi = (0..vars.len()).fold(0, |b, j| if vars[j] > vars[b] { j } else { b });
    let lo = (0..vars.len())
        .filter(|&j| vars[j] > 0.0)
        .fold(None, |b: Option<usize>, j| match b {
            Some(b) if vars[b] <= vars[j] => Some(b),
            _ => Some(j),
        });
    let Some(lo) = lo else { return };
    let ratio = vars[hi] / vars[lo];
    if ratio >= 1e3 {
        warn(
            out,
            code::MIXED_SCALES,
            format!(
                "Unstandardized, `{}` has {ratio:.0} times the variance of `{}`, so distances and \
                 components are mostly `{}`. Standardize unless the features share a unit on \
                 purpose.",
                ex.feature_names[hi], ex.feature_names[lo], ex.feature_names[hi]
            ),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Groups, splits and folds
// ─────────────────────────────────────────────────────────────────────────────

/// Group id per extracted row, from the named column. A row with no value
/// becomes a group of its own.
fn group_ids(frame: &Frame, ex: &Extracted, group: &str, out: &mut Vec<Caution>) -> Result<Vec<usize>> {
    let col = frame
        .column(group)
        .ok_or_else(|| DataError::Schema(format!("no group column `{group}`")))?;
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut ids: Vec<usize> = ex
        .kept_rows
        .iter()
        .map(|&r| match cell_text(col, r) {
            Some(t) => {
                let next = index.len();
                *index.entry(t).or_insert(next)
            }
            None => usize::MAX,
        })
        .collect();
    let mut next = index.len();
    let mut missing = 0;
    for id in ids.iter_mut().filter(|id| **id == usize::MAX) {
        *id = next;
        next += 1;
        missing += 1;
    }
    if missing > 0 {
        warn(
            out,
            code::GROUP_MISSING,
            format!(
                "{missing} {} no `{group}` value; each was treated as a group of its own.",
                plural(missing, "row has", "rows have")
            ),
        );
    }
    Ok(ids)
}

/// Class labels and their count, when splits should stratify.
type Strata<'a> = Option<(&'a [usize], usize)>;

fn split_rows(n: usize, strata: Strata<'_>, groups: Option<&[usize]>, opts: &RunOptions) -> Result<Fold> {
    let seed = sub_seed(opts.seed, SPLIT);
    match (groups, strata) {
        (Some(g), _) => group_split(g, opts.test_fraction, seed),
        (None, Some((y, k))) => stratified_split(y, k, opts.test_fraction, seed),
        (None, None) => train_test_split(n, opts.test_fraction, seed),
    }
}

/// Cross-validation folds within the training rows, indexing into them.
/// `None` when folds are off or there are too few rows or groups to fold.
fn train_folds(
    train: &[usize],
    strata: Strata<'_>,
    groups: Option<&[usize]>,
    opts: &RunOptions,
) -> Result<Option<Vec<Fold>>> {
    let k = opts.cv_folds.min(train.len());
    if k < 2 {
        return Ok(None);
    }
    let seed = sub_seed(opts.seed, FOLDS);
    Ok(Some(match (groups, strata) {
        (Some(g), _) => {
            let local: Vec<usize> = train.iter().map(|&i| g[i]).collect();
            let mut distinct = local.clone();
            distinct.sort_unstable();
            distinct.dedup();
            if distinct.len() < 2 {
                return Ok(None);
            }
            group_k_fold(&local, k.min(distinct.len()), seed)?
        }
        (None, Some((y, classes))) => {
            let local: Vec<usize> = train.iter().map(|&i| y[i]).collect();
            stratified_k_fold(&local, classes, k, seed)?
        }
        (None, None) => k_fold(train.len(), k, seed)?,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Importance
// ─────────────────────────────────────────────────────────────────────────────

/// Permutation importance of one feature, by name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamedImportance {
    pub feature: String,
    /// Test score lost when the feature is shuffled, averaged over the repeats.
    pub mean_drop: f64,
    pub std_drop: f64,
}

fn name_importances(mut imps: Vec<Importance>, names: &[String]) -> Vec<NamedImportance> {
    imps.sort_by(|a, b| b.mean_drop.total_cmp(&a.mean_drop).then(a.feature.cmp(&b.feature)));
    imps.into_iter()
        .map(|i| NamedImportance { feature: names[i.feature].clone(), mean_drop: i.mean_drop, std_drop: i.std_drop })
        .collect()
}

/// `gain` is how far the model's test score clears its baseline; a model that
/// barely clears it has nothing worth attributing.
fn dominance_caution(imps: &[NamedImportance], gain: f64, out: &mut Vec<Caution>) {
    if imps.len() < 2 || gain < 0.1 {
        return;
    }
    let total: f64 = imps.iter().map(|i| i.mean_drop.max(0.0)).sum();
    let top = &imps[0];
    if total > 0.0 && top.mean_drop >= 0.9 * total {
        warn(
            out,
            code::DOMINANT_FEATURE,
            format!(
                "`{}` carries {:.0}% of what the model draws from its features. If it is recorded \
                 after the outcome, or derived from it, the model will fail wherever it is not yet \
                 known.",
                top.feature,
                100.0 * top.mean_drop / total
            ),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Classification
// ─────────────────────────────────────────────────────────────────────────────

/// Precision, recall and F1 of one class on the test rows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClassScore {
    pub class: String,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    /// Test rows of this class.
    pub support: usize,
}

/// Everything one classification run measured.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClassificationReport {
    /// The model with every hyperparameter.
    pub model: String,
    pub target: String,
    pub features: Vec<String>,
    pub classes: Vec<String>,
    pub options: RunOptions,
    pub rows_used: usize,
    pub rows_dropped: usize,
    pub train_rows: usize,
    pub test_rows: usize,
    /// Test accuracy of always predicting the training rows' most common class.
    pub baseline_accuracy: f64,
    pub baseline_balanced_accuracy: f64,
    pub train_accuracy: f64,
    pub accuracy: f64,
    pub balanced_accuracy: f64,
    pub macro_f1: f64,
    pub weighted_f1: f64,
    /// Area under the ROC curve for the second class, when there are exactly
    /// two classes and the test rows hold both.
    pub roc_auc: Option<f64>,
    pub per_class: Vec<ClassScore>,
    /// `confusion[actual][predicted]` over the test rows, in `classes` order.
    pub confusion: Vec<Vec<usize>>,
    /// Accuracy across folds of the training rows.
    pub cv_accuracy: Option<CvSummary>,
    pub cv_macro_f1: Option<CvSummary>,
    /// Test accuracy lost when each feature is shuffled; largest first.
    pub importance: Vec<NamedImportance>,
    pub cautions: Vec<Caution>,
}

/// A classification problem, extracted, split and folded once so several
/// models can be judged on identical rows.
struct ClassProblem {
    ex: Extracted,
    labels: Labels,
    split: Fold,
    folds: Option<Vec<Fold>>,
    x_train: Matrix,
    y_train: Vec<usize>,
    x_test: Matrix,
    y_test: Vec<usize>,
    cautions: Vec<Caution>,
}

impl ClassProblem {
    fn new(frame: &Frame, features: &[&str], target: &str, opts: &RunOptions) -> Result<Self> {
        let (ex, labels) = features_and_labels(frame, features, target)?;
        let k = labels.n_classes();
        let mut cautions = Vec::new();
        data_cautions(&ex, &mut cautions);
        let groups = match &opts.group {
            Some(g) => Some(group_ids(frame, &ex, g, &mut cautions)?),
            None => None,
        };
        let strata = Some((labels.y.as_slice(), k));
        let split = split_rows(ex.x.rows(), strata, groups.as_deref(), opts)?;
        let folds = train_folds(&split.train, strata, groups.as_deref(), opts)?;
        let x_train = ex.x.select_rows(&split.train);
        let x_test = ex.x.select_rows(&split.test);
        let y_train: Vec<usize> = split.train.iter().map(|&i| labels.y[i]).collect();
        let y_test: Vec<usize> = split.test.iter().map(|&i| labels.y[i]).collect();

        let n = labels.y.len();
        let mut count = vec![0usize; k];
        for &c in &labels.y {
            count[c] += 1;
        }
        let major = first_max(&count);
        if count[major] * 5 >= n * 4 {
            warn(
                &mut cautions,
                code::IMBALANCED,
                format!(
                    "`{}` makes up {:.0}% of the rows. Accuracy rewards always predicting it; \
                     read balanced accuracy and per-class recall instead.",
                    labels.classes[major],
                    100.0 * count[major] as f64 / n as f64
                ),
            );
        }
        if let Some(f) = &folds {
            let mut train_count = vec![0usize; k];
            for &c in &y_train {
                train_count[c] += 1;
            }
            let rare: Vec<String> = (0..k)
                .filter(|&c| train_count[c] < f.len())
                .map(|c| {
                    format!(
                        "`{}` ({} training {})",
                        labels.classes[c],
                        train_count[c],
                        plural(train_count[c], "row", "rows")
                    )
                })
                .collect();
            if !rare.is_empty() {
                warn(
                    &mut cautions,
                    code::RARE_CLASS,
                    format!(
                        "{} {} fewer training rows than the {} folds. Some folds cannot test {}, \
                         and {} per-class scores rest on a handful of rows.",
                        rare.join(", "),
                        plural(rare.len(), "has", "have"),
                        f.len(),
                        plural(rare.len(), "it", "them"),
                        plural(rare.len(), "its", "their")
                    ),
                );
            }
        }
        separator_caution(&ex, &labels.y, target, &mut cautions);
        few_rows_caution(split.train.len(), ex.x.cols(), &mut cautions);
        overlap_caution(&x_train, &x_test, &mut cautions);
        Ok(Self { ex, labels, split, folds, x_train, y_train, x_test, y_test, cautions })
    }

    /// Test accuracy and balanced accuracy of always predicting the training
    /// rows' most common class.
    fn baseline(&self) -> Result<(f64, f64)> {
        let k = self.labels.n_classes();
        let mut count = vec![0usize; k];
        for &c in &self.y_train {
            count[c] += 1;
        }
        let guess = vec![first_max(&count); self.y_test.len()];
        let cm = Confusion::new(&self.y_test, &guess, k)?;
        Ok((cm.accuracy(), cm.balanced_accuracy()))
    }
}

struct ClassEval {
    model: String,
    fitted: Box<dyn Classifier>,
    train_accuracy: f64,
    cm: Confusion,
    roc_auc: Option<f64>,
    cv: Option<(CvSummary, CvSummary)>,
}

fn evaluate_classifier(spec: &dyn ClassifierSpec, p: &ClassProblem) -> Result<ClassEval> {
    let k = p.labels.n_classes();
    let fitted = spec.fit(&p.x_train, &p.y_train, k)?;
    let train_pred = fitted.predict(&p.x_train)?;
    let train_accuracy = Confusion::new(&p.y_train, &train_pred, k)?.accuracy();
    let proba = fitted.predict_proba(&p.x_test)?;
    let pred: Vec<usize> = proba.iter().map(|row| argmax(row)).collect();
    let cm = Confusion::new(&p.y_test, &pred, k)?;
    let auc = if k == 2 {
        let positive: Vec<bool> = p.y_test.iter().map(|&c| c == 1).collect();
        let score: Vec<f64> = proba.iter().map(|row| row[1]).collect();
        roc_auc(&positive, &score).ok()
    } else {
        None
    };
    let cv = match &p.folds {
        Some(folds) => Some(cross_validate_classifier(spec, &p.x_train, &p.y_train, k, folds)?),
        None => None,
    };
    Ok(ClassEval { model: spec.describe(), fitted, train_accuracy, cm, roc_auc: auc, cv })
}

fn class_result_cautions(e: &ClassEval, baseline: (f64, f64), target: &str, out: &mut Vec<Caution>) {
    let (acc, bal) = (e.cm.accuracy(), e.cm.balanced_accuracy());
    if acc <= baseline.0 + 1e-12 && bal <= baseline.1 + 0.05 {
        warn(
            out,
            code::NO_BETTER_THAN_BASELINE,
            format!(
                "{} scores {acc:.3} accuracy and {bal:.3} balanced accuracy on the test rows; \
                 always predicting the most common class scores {:.3} and {:.3}. As given, the \
                 features do not let this model predict `{target}`.",
                e.model, baseline.0, baseline.1
            ),
        );
    }
    if e.train_accuracy - acc > 0.15 {
        warn(
            out,
            code::OVERFIT,
            format!(
                "{} scores {:.3} on its training rows but {acc:.3} on the test rows: it has \
                 memorised rather than generalised. Constrain it (a shallower tree, a larger k, \
                 a stronger penalty) or give it more rows.",
                e.model, e.train_accuracy
            ),
        );
    }
    if let Some((cv_acc, _)) = &e.cv {
        if cv_acc.std > 0.1 {
            warn(
                out,
                code::UNSTABLE_CV,
                format!(
                    "{}: cross-validated accuracy varies by {:.3} (one standard deviation) between \
                     folds, so its {:.3} average is not reliable. More rows or fewer features \
                     would steady it.",
                    e.model, cv_acc.std, cv_acc.mean
                ),
            );
        }
    }
}

/// Fit `spec` to predict `target` from `features`, and measure it honestly.
pub fn classify(
    frame: &Frame,
    features: &[&str],
    target: &str,
    spec: &dyn ClassifierSpec,
    opts: &RunOptions,
) -> Result<ClassificationReport> {
    let p = ClassProblem::new(frame, features, target, opts)?;
    let k = p.labels.n_classes();
    let baseline = p.baseline()?;
    let e = evaluate_classifier(spec, &p)?;
    let mut cautions = p.cautions.clone();
    class_result_cautions(&e, baseline, target, &mut cautions);
    let importance = if opts.importance_repeats > 0 && p.x_test.rows() >= 2 {
        let (model, y) = (&e.fitted, &p.y_test);
        let raw = permutation_importance(
            &p.x_test,
            |x| Ok(Confusion::new(y, &model.predict(x)?, k)?.accuracy()),
            opts.importance_repeats,
            sub_seed(opts.seed, PERMUTE),
        )?;
        name_importances(raw, &p.ex.feature_names)
    } else {
        Vec::new()
    };
    dominance_caution(&importance, e.cm.accuracy() - baseline.0, &mut cautions);
    let per_class = (0..k)
        .map(|c| ClassScore {
            class: p.labels.classes[c].clone(),
            precision: e.cm.precision(c),
            recall: e.cm.recall(c),
            f1: e.cm.f1(c),
            support: e.cm.support(c),
        })
        .collect();
    Ok(ClassificationReport {
        model: e.model.clone(),
        target: target.to_string(),
        features: p.ex.feature_names.clone(),
        classes: p.labels.classes.clone(),
        options: opts.clone(),
        rows_used: p.ex.x.rows(),
        rows_dropped: p.ex.dropped,
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        baseline_accuracy: baseline.0,
        baseline_balanced_accuracy: baseline.1,
        train_accuracy: e.train_accuracy,
        accuracy: e.cm.accuracy(),
        balanced_accuracy: e.cm.balanced_accuracy(),
        macro_f1: e.cm.macro_f1(),
        weighted_f1: e.cm.weighted_f1(),
        roc_auc: e.roc_auc,
        per_class,
        confusion: e.cm.counts.clone(),
        cv_accuracy: e.cv.as_ref().map(|c| c.0.clone()),
        cv_macro_f1: e.cv.as_ref().map(|c| c.1.clone()),
        importance,
        cautions,
    })
}

/// One classifier's line in a [`Leaderboard`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LeaderboardEntry {
    pub model: String,
    pub cv_accuracy: Option<CvSummary>,
    pub cv_macro_f1: Option<CvSummary>,
    pub train_accuracy: f64,
    pub accuracy: f64,
    pub balanced_accuracy: f64,
    pub macro_f1: f64,
    pub roc_auc: Option<f64>,
}

/// Several classifiers judged on identical rows, splits and folds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Leaderboard {
    pub target: String,
    pub features: Vec<String>,
    pub classes: Vec<String>,
    pub options: RunOptions,
    pub rows_used: usize,
    pub rows_dropped: usize,
    pub train_rows: usize,
    pub test_rows: usize,
    pub baseline_accuracy: f64,
    pub baseline_balanced_accuracy: f64,
    /// Best first: by cross-validated macro F1 when folds ran, so the test
    /// rows play no part in choosing the winner; otherwise by test macro F1.
    pub entries: Vec<LeaderboardEntry>,
    pub cautions: Vec<Caution>,
}

/// Judge several classifiers on the same problem: the "which model?" run.
pub fn compare_classifiers(
    frame: &Frame,
    features: &[&str],
    target: &str,
    specs: &[&dyn ClassifierSpec],
    opts: &RunOptions,
) -> Result<Leaderboard> {
    if specs.is_empty() {
        return Err(DataError::Schema("no models to compare".into()));
    }
    let p = ClassProblem::new(frame, features, target, opts)?;
    let baseline = p.baseline()?;
    let mut cautions = p.cautions.clone();
    let mut entries = Vec::with_capacity(specs.len());
    for &spec in specs {
        let e = evaluate_classifier(spec, &p)?;
        class_result_cautions(&e, baseline, target, &mut cautions);
        entries.push(LeaderboardEntry {
            model: e.model.clone(),
            cv_accuracy: e.cv.as_ref().map(|c| c.0.clone()),
            cv_macro_f1: e.cv.as_ref().map(|c| c.1.clone()),
            train_accuracy: e.train_accuracy,
            accuracy: e.cm.accuracy(),
            balanced_accuracy: e.cm.balanced_accuracy(),
            macro_f1: e.cm.macro_f1(),
            roc_auc: e.roc_auc,
        });
    }
    let key = |e: &LeaderboardEntry| e.cv_macro_f1.as_ref().map_or(e.macro_f1, |c| c.mean);
    entries.sort_by(|a, b| key(b).total_cmp(&key(a)));
    if p.folds.is_none() && specs.len() > 1 {
        warn(
            &mut cautions,
            code::RANKED_ON_TEST,
            "No cross-validation ran, so the models were ranked on the test rows. Picking the best \
             of several on the same rows that score it makes the winner's test score slightly \
             optimistic."
                .to_string(),
        );
    }
    Ok(Leaderboard {
        target: target.to_string(),
        features: p.ex.feature_names.clone(),
        classes: p.labels.classes.clone(),
        options: opts.clone(),
        rows_used: p.ex.x.rows(),
        rows_dropped: p.ex.dropped,
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        baseline_accuracy: baseline.0,
        baseline_balanced_accuracy: baseline.1,
        entries,
        cautions,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression
// ─────────────────────────────────────────────────────────────────────────────

/// One term of a fitted linear equation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Coefficient {
    pub feature: String,
    /// In the target's units per unit of the feature.
    pub value: f64,
}

/// Everything one regression run measured.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegressionReport {
    pub model: String,
    pub target: String,
    pub features: Vec<String>,
    pub options: RunOptions,
    pub rows_used: usize,
    pub rows_dropped: usize,
    pub train_rows: usize,
    pub test_rows: usize,
    /// Predicting the training rows' mean for every test row.
    pub baseline: RegressionMetrics,
    pub train: RegressionMetrics,
    pub test: RegressionMetrics,
    /// R² across folds of the training rows.
    pub cv_r2: Option<CvSummary>,
    pub cv_rmse: Option<CvSummary>,
    /// For a linear model, the fitted equation in the features' own units.
    pub intercept: Option<f64>,
    pub coefficients: Vec<Coefficient>,
    /// Test R² lost when each feature is shuffled; largest first.
    pub importance: Vec<NamedImportance>,
    pub cautions: Vec<Caution>,
}

struct RegProblem {
    ex: Extracted,
    split: Fold,
    folds: Option<Vec<Fold>>,
    x_train: Matrix,
    y_train: Vec<f64>,
    x_test: Matrix,
    y_test: Vec<f64>,
    cautions: Vec<Caution>,
}

impl RegProblem {
    fn new(frame: &Frame, features: &[&str], target: &str, opts: &RunOptions) -> Result<Self> {
        let (ex, y) = features_and_target(frame, features, target)?;
        let mut cautions = Vec::new();
        data_cautions(&ex, &mut cautions);
        let groups = match &opts.group {
            Some(g) => Some(group_ids(frame, &ex, g, &mut cautions)?),
            None => None,
        };
        let split = split_rows(ex.x.rows(), None, groups.as_deref(), opts)?;
        let folds = train_folds(&split.train, None, groups.as_deref(), opts)?;
        let x_train = ex.x.select_rows(&split.train);
        let x_test = ex.x.select_rows(&split.test);
        let y_train: Vec<f64> = split.train.iter().map(|&i| y[i]).collect();
        let y_test: Vec<f64> = split.test.iter().map(|&i| y[i]).collect();
        target_copy_caution(&ex, &y, target, &mut cautions)?;
        few_rows_caution(split.train.len(), ex.x.cols(), &mut cautions);
        overlap_caution(&x_train, &x_test, &mut cautions);
        Ok(Self { ex, split, folds, x_train, y_train, x_test, y_test, cautions })
    }

    fn baseline(&self) -> Result<RegressionMetrics> {
        let mean = self.y_train.iter().sum::<f64>() / self.y_train.len() as f64;
        regression_metrics(&self.y_test, &vec![mean; self.y_test.len()])
    }
}

struct RegEval {
    model: String,
    fitted: Box<dyn Regressor>,
    train: RegressionMetrics,
    test: RegressionMetrics,
    cv: Option<(CvSummary, CvSummary)>,
}

fn evaluate_regressor(spec: &dyn RegressorSpec, p: &RegProblem) -> Result<RegEval> {
    let fitted = spec.fit(&p.x_train, &p.y_train)?;
    let train = regression_metrics(&p.y_train, &fitted.predict(&p.x_train)?)?;
    let test = regression_metrics(&p.y_test, &fitted.predict(&p.x_test)?)?;
    let cv = match &p.folds {
        Some(folds) => Some(cross_validate_regressor(spec, &p.x_train, &p.y_train, folds)?),
        None => None,
    };
    Ok(RegEval { model: spec.describe(), fitted, train, test, cv })
}

fn reg_result_cautions(e: &RegEval, baseline: &RegressionMetrics, target: &str, out: &mut Vec<Caution>) {
    if e.test.rmse >= baseline.rmse {
        warn(
            out,
            code::NO_BETTER_THAN_BASELINE,
            format!(
                "{} misses by {:.4} (RMSE) on the test rows; predicting the training mean of \
                 `{target}` for every row misses by {:.4}. As given, the features do not let this \
                 model predict `{target}`.",
                e.model, e.test.rmse, baseline.rmse
            ),
        );
    }
    if e.train.r2 - e.test.r2 > 0.2 {
        warn(
            out,
            code::OVERFIT,
            format!(
                "{} explains {:.3} of the variance (R²) on its training rows but {:.3} on the \
                 test rows: it has memorised rather than generalised. Add a penalty, drop \
                 features, or give it more rows.",
                e.model, e.train.r2, e.test.r2
            ),
        );
    }
    if let Some((r2, _)) = &e.cv {
        if r2.std > 0.1 {
            warn(
                out,
                code::UNSTABLE_CV,
                format!(
                    "{}: cross-validated R² varies by {:.3} (one standard deviation) between \
                     folds, so its {:.3} average is not reliable.",
                    e.model, r2.std, r2.mean
                ),
            );
        }
    }
}

/// Fit `spec` to predict the numeric `target` from `features`, and measure it
/// honestly.
pub fn regress(
    frame: &Frame,
    features: &[&str],
    target: &str,
    spec: &dyn RegressorSpec,
    opts: &RunOptions,
) -> Result<RegressionReport> {
    let p = RegProblem::new(frame, features, target, opts)?;
    let baseline = p.baseline()?;
    let e = evaluate_regressor(spec, &p)?;
    let mut cautions = p.cautions.clone();
    reg_result_cautions(&e, &baseline, target, &mut cautions);
    let importance = if opts.importance_repeats > 0 && p.x_test.rows() >= 2 {
        let (model, y) = (&e.fitted, &p.y_test);
        let raw = permutation_importance(
            &p.x_test,
            |x| Ok(regression_metrics(y, &model.predict(x)?)?.r2),
            opts.importance_repeats,
            sub_seed(opts.seed, PERMUTE),
        )?;
        name_importances(raw, &p.ex.feature_names)
    } else {
        Vec::new()
    };
    dominance_caution(&importance, e.test.r2 - baseline.r2, &mut cautions);
    let (coefficients, intercept): (Vec<Coefficient>, Option<f64>) = match e.fitted.linear_terms() {
        Some((c, b)) => (
            c.iter()
                .zip(&p.ex.feature_names)
                .map(|(&value, f)| Coefficient { feature: f.clone(), value })
                .collect(),
            Some(b),
        ),
        None => (Vec::new(), None),
    };
    Ok(RegressionReport {
        model: e.model.clone(),
        target: target.to_string(),
        features: p.ex.feature_names.clone(),
        options: opts.clone(),
        rows_used: p.ex.x.rows(),
        rows_dropped: p.ex.dropped,
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        baseline,
        train: e.train.clone(),
        test: e.test.clone(),
        cv_r2: e.cv.as_ref().map(|c| c.0.clone()),
        cv_rmse: e.cv.as_ref().map(|c| c.1.clone()),
        intercept,
        coefficients,
        importance,
        cautions,
    })
}

/// One regressor's line in a [`RegressionLeaderboard`].
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegressionEntry {
    pub model: String,
    pub cv_r2: Option<CvSummary>,
    pub cv_rmse: Option<CvSummary>,
    pub train_r2: f64,
    pub r2: f64,
    pub rmse: f64,
    pub mae: f64,
}

/// Several regressors judged on identical rows, splits and folds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RegressionLeaderboard {
    pub target: String,
    pub features: Vec<String>,
    pub options: RunOptions,
    pub rows_used: usize,
    pub rows_dropped: usize,
    pub train_rows: usize,
    pub test_rows: usize,
    pub baseline: RegressionMetrics,
    /// Best first: by cross-validated R² when folds ran, otherwise by test R².
    pub entries: Vec<RegressionEntry>,
    pub cautions: Vec<Caution>,
}

/// Judge several regressors on the same problem.
pub fn compare_regressors(
    frame: &Frame,
    features: &[&str],
    target: &str,
    specs: &[&dyn RegressorSpec],
    opts: &RunOptions,
) -> Result<RegressionLeaderboard> {
    if specs.is_empty() {
        return Err(DataError::Schema("no models to compare".into()));
    }
    let p = RegProblem::new(frame, features, target, opts)?;
    let baseline = p.baseline()?;
    let mut cautions = p.cautions.clone();
    let mut entries = Vec::with_capacity(specs.len());
    for &spec in specs {
        let e = evaluate_regressor(spec, &p)?;
        reg_result_cautions(&e, &baseline, target, &mut cautions);
        entries.push(RegressionEntry {
            model: e.model.clone(),
            cv_r2: e.cv.as_ref().map(|c| c.0.clone()),
            cv_rmse: e.cv.as_ref().map(|c| c.1.clone()),
            train_r2: e.train.r2,
            r2: e.test.r2,
            rmse: e.test.rmse,
            mae: e.test.mae,
        });
    }
    let key = |e: &RegressionEntry| e.cv_r2.as_ref().map_or(e.r2, |c| c.mean);
    entries.sort_by(|a, b| key(b).total_cmp(&key(a)));
    if p.folds.is_none() && specs.len() > 1 {
        warn(
            &mut cautions,
            code::RANKED_ON_TEST,
            "No cross-validation ran, so the models were ranked on the test rows. Picking the best \
             of several on the same rows that score it makes the winner's test score slightly \
             optimistic."
                .to_string(),
        );
    }
    Ok(RegressionLeaderboard {
        target: target.to_string(),
        features: p.ex.feature_names.clone(),
        options: opts.clone(),
        rows_used: p.ex.x.rows(),
        rows_dropped: p.ex.dropped,
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        baseline,
        entries,
        cautions,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Fitted models, for scoring new rows
// ─────────────────────────────────────────────────────────────────────────────

/// A classifier fitted on every usable row: the model to deploy once a
/// [`classify`] run has shown the spec generalises.
pub struct FittedClassifier {
    pub model: Box<dyn Classifier>,
    pub features: Vec<String>,
    pub target: String,
    pub classes: Vec<String>,
}

impl FittedClassifier {
    pub fn fit(frame: &Frame, features: &[&str], target: &str, spec: &dyn ClassifierSpec) -> Result<Self> {
        let (ex, labels) = features_and_labels(frame, features, target)?;
        let model = spec.fit(&ex.x, &labels.y, labels.n_classes())?;
        Ok(Self { model, features: ex.feature_names, target: target.to_string(), classes: labels.classes })
    }

    fn extract(&self, frame: &Frame) -> Result<Extracted> {
        let cols: Vec<&str> = self.features.iter().map(String::as_str).collect();
        extract_features(frame, &cols)
    }

    /// The predicted class for each row of `frame`, `None` where a feature is
    /// missing.
    pub fn predict_column(&self, frame: &Frame) -> Result<ColumnData> {
        let ex = self.extract(frame)?;
        let pred = self.model.predict(&ex.x)?;
        let mut out = vec![None; frame.n_rows()];
        for (&r, c) in ex.kept_rows.iter().zip(pred) {
            out[r] = Some(self.classes[c].clone());
        }
        Ok(ColumnData::Str(out))
    }

    /// One probability column per class, named `p(<class>)`.
    pub fn probability_columns(&self, frame: &Frame) -> Result<Vec<(ColumnSpec, ColumnData)>> {
        let ex = self.extract(frame)?;
        let proba = self.model.predict_proba(&ex.x)?;
        Ok(self
            .classes
            .iter()
            .enumerate()
            .map(|(c, name)| {
                let mut col = vec![None; frame.n_rows()];
                for (&r, p) in ex.kept_rows.iter().zip(&proba) {
                    col[r] = Some(p[c]);
                }
                (ColumnSpec::new(format!("p({name})"), ColumnDtype::F64), ColumnData::F64(col))
            })
            .collect())
    }
}

/// A regressor fitted on every usable row.
pub struct FittedRegressor {
    pub model: Box<dyn Regressor>,
    pub features: Vec<String>,
    pub target: String,
}

impl FittedRegressor {
    pub fn fit(frame: &Frame, features: &[&str], target: &str, spec: &dyn RegressorSpec) -> Result<Self> {
        let (ex, y) = features_and_target(frame, features, target)?;
        let model = spec.fit(&ex.x, &y)?;
        Ok(Self { model, features: ex.feature_names, target: target.to_string() })
    }

    /// The prediction for each row of `frame`, `None` where a feature is
    /// missing.
    pub fn predict_column(&self, frame: &Frame) -> Result<ColumnData> {
        let cols: Vec<&str> = self.features.iter().map(String::as_str).collect();
        let ex = extract_features(frame, &cols)?;
        let pred = self.model.predict(&ex.x)?;
        let mut out = vec![None; frame.n_rows()];
        for (&r, v) in ex.kept_rows.iter().zip(pred) {
            out[r] = Some(v);
        }
        Ok(ColumnData::F64(out))
    }
}

/// The classifier named `name`, with default settings. The names are the ones
/// [`ClassifierSpec::name`] reports: `logistic_regression`, `decision_tree`,
/// `k_nearest_neighbors`, `gaussian_naive_bayes`.
pub fn classifier_by_name(name: &str) -> Option<Box<dyn ClassifierSpec>> {
    let spec: Box<dyn ClassifierSpec> = match name {
        "logistic_regression" => Box::new(LogisticRegression::default()),
        "decision_tree" => Box::new(DecisionTree::default()),
        "k_nearest_neighbors" => Box::new(KNearestNeighbors::default()),
        "gaussian_naive_bayes" => Box::new(GaussianNaiveBayes::default()),
        _ => return None,
    };
    Some(spec)
}

/// The regressor named `name`, with default settings: `linear_regression`,
/// `ridge_regression` (penalty 1) or `lasso`.
pub fn regressor_by_name(name: &str) -> Option<Box<dyn RegressorSpec>> {
    let spec: Box<dyn RegressorSpec> = match name {
        "linear_regression" => Box::new(LinearRegression::default()),
        "ridge_regression" => Box::new(LinearRegression { ridge: 1.0 }),
        "lasso" => Box::new(Lasso::default()),
        _ => return None,
    };
    Some(spec)
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature ranking, selection check, forward selection, LASSO path
// ─────────────────────────────────────────────────────────────────────────────

/// Whether the target is a class or a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Classification,
    Regression,
}

/// One feature's filter scores against the target.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeatureRank {
    pub feature: String,
    /// Mutual information with the target, in nats: any dependence, linear or
    /// not.
    pub mutual_information: f64,
    /// ANOVA F across the classes, or |Pearson r| with a numeric target: linear
    /// association only.
    pub linear_score: f64,
    /// Population variance, in the feature's own units squared.
    pub variance: f64,
}

/// Every feature scored against the target by model-free filters.
///
/// A ranking is for exploring. Choosing features from it and then scoring a
/// model on the same rows overstates the score; [`selection_check`] measures
/// by how much.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ranking {
    pub task: Task,
    pub target: String,
    pub rows_used: usize,
    pub rows_dropped: usize,
    /// Equal-frequency bins behind the mutual-information estimate.
    pub bins: usize,
    /// Highest mutual information first.
    pub features: Vec<FeatureRank>,
    pub cautions: Vec<Caution>,
}

/// Bins for a mutual-information estimate on `n` rows: the cube root, which
/// grows slowly enough that the estimate's upward bias stays small.
fn mi_bins(n: usize) -> usize {
    ((n as f64).cbrt().round() as usize).clamp(2, 16)
}

/// Score every feature against the target with filter statistics.
pub fn rank_features(frame: &Frame, features: &[&str], target: &str, task: Task) -> Result<Ranking> {
    let mut cautions = Vec::new();
    let (ex, mi, linear, bins) = match task {
        Task::Classification => {
            let (ex, labels) = features_and_labels(frame, features, target)?;
            data_cautions(&ex, &mut cautions);
            let bins = mi_bins(ex.x.rows());
            let k = labels.n_classes();
            let mi = mutual_information(&ex.x, &labels.y, k, bins)?;
            let f = anova_f(&ex.x, &labels.y, k)?;
            separator_caution(&ex, &labels.y, target, &mut cautions);
            (ex, mi, f, bins)
        }
        Task::Regression => {
            let (ex, y) = features_and_target(frame, features, target)?;
            data_cautions(&ex, &mut cautions);
            let bins = mi_bins(ex.x.rows());
            let yb = quantile_bins(&y, bins);
            let nb = yb.iter().max().map_or(1, |m| m + 1);
            let mi = mutual_information(&ex.x, &yb, nb, bins)?;
            let r = pearson_scores(&ex.x, &y)?;
            target_copy_caution(&ex, &y, target, &mut cautions)?;
            (ex, mi, r, bins)
        }
    };
    let mut order: Vec<usize> = (0..ex.x.cols()).collect();
    order.sort_by(|&a, &b| mi[b].total_cmp(&mi[a]).then(linear[b].total_cmp(&linear[a])).then(a.cmp(&b)));
    let ranked_features = order
        .into_iter()
        .map(|j| FeatureRank {
            feature: ex.feature_names[j].clone(),
            mutual_information: mi[j],
            linear_score: linear[j],
            variance: variance(&ex.x.column(j)),
        })
        .collect();
    Ok(Ranking {
        task,
        target: target.to_string(),
        rows_used: ex.x.rows(),
        rows_dropped: ex.dropped,
        bins,
        features: ranked_features,
        cautions,
    })
}

/// How often a feature was chosen across folds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionFrequency {
    pub feature: String,
    /// Share of folds that chose it.
    pub share: f64,
}

/// The cost of selecting features on the same rows that score the model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionCheck {
    pub model: String,
    pub target: String,
    pub keep: usize,
    pub features_available: usize,
    pub folds: usize,
    /// The `keep` features with the highest ANOVA F on all rows.
    pub selected_on_all_rows: Vec<String>,
    /// Cross-validated accuracy with the features chosen on ALL rows, test
    /// folds included: the common mistake.
    pub naive: CvSummary,
    /// Cross-validated accuracy with the choice repeated inside every fold, on
    /// its training rows only: what to expect on new data.
    pub honest: CvSummary,
    /// `naive.mean - honest.mean`: the accuracy the mistake invents.
    pub optimism: f64,
    /// How often each feature was chosen across the folds; most often first.
    pub stability: Vec<SelectionFrequency>,
    pub cautions: Vec<Caution>,
}

/// Measure selection bias: keep the `keep` best features by ANOVA F, once on
/// all rows and once inside each fold, and cross-validate `spec` both ways.
///
/// Selecting on all rows lets the choice see the rows that later score the
/// model. With many features and few rows, some features correlate with the
/// target by chance, the choice finds exactly those, and the score rises with
/// no real signal behind it. The honest estimate repeats the choice inside
/// each fold, so the held-out rows never influence it.
pub fn selection_check(
    frame: &Frame,
    features: &[&str],
    target: &str,
    spec: &dyn ClassifierSpec,
    keep: usize,
    opts: &RunOptions,
) -> Result<SelectionCheck> {
    let (ex, labels) = features_and_labels(frame, features, target)?;
    let (k, d) = (labels.n_classes(), ex.x.cols());
    if d < 2 {
        return Err(DataError::Schema("a selection check needs at least 2 features to choose from".into()));
    }
    if keep == 0 || keep >= d {
        return Err(DataError::Schema(format!(
            "keep must be between 1 and {}: keeping all {d} features selects nothing",
            d - 1
        )));
    }
    if opts.cv_folds < 2 {
        return Err(DataError::Schema("a selection check needs cross-validation; set cv_folds to 2 or more".into()));
    }
    let mut cautions = Vec::new();
    data_cautions(&ex, &mut cautions);
    let seed = sub_seed(opts.seed, FOLDS);
    let folds = match &opts.group {
        Some(g) => {
            let ids = group_ids(frame, &ex, g, &mut cautions)?;
            let mut distinct = ids.clone();
            distinct.sort_unstable();
            distinct.dedup();
            group_k_fold(&ids, opts.cv_folds.min(distinct.len()), seed)?
        }
        None => stratified_k_fold(&labels.y, k, opts.cv_folds.min(labels.y.len()), seed)?,
    };
    let top = |x: &Matrix, y: &[usize]| -> Result<Vec<usize>> {
        Ok(ranked(&anova_f(x, y, k)?).into_iter().take(keep).map(|s| s.feature).collect())
    };

    let chosen_all = top(&ex.x, &labels.y)?;
    let (naive, _) = cross_validate_classifier(spec, &ex.x.select_cols(&chosen_all), &labels.y, k, &folds)?;

    let mut scores = Vec::with_capacity(folds.len());
    let mut picked = vec![0usize; d];
    for fold in &folds {
        let x_train = ex.x.select_rows(&fold.train);
        let y_train: Vec<usize> = fold.train.iter().map(|&i| labels.y[i]).collect();
        let chosen = top(&x_train, &y_train)?;
        for &f in &chosen {
            picked[f] += 1;
        }
        let model = spec.fit(&x_train.select_cols(&chosen), &y_train, k)?;
        let y_test: Vec<usize> = fold.test.iter().map(|&i| labels.y[i]).collect();
        let pred = model.predict(&ex.x.select_rows(&fold.test).select_cols(&chosen))?;
        scores.push(Confusion::new(&y_test, &pred, k)?.accuracy());
    }
    let honest = CvSummary::from_scores(scores);
    let optimism = naive.mean - honest.mean;

    let nf = folds.len() as f64;
    let mut stability: Vec<SelectionFrequency> = (0..d)
        .filter(|&j| picked[j] > 0)
        .map(|j| SelectionFrequency { feature: ex.feature_names[j].clone(), share: picked[j] as f64 / nf })
        .collect();
    stability.sort_by(|a, b| b.share.total_cmp(&a.share));

    if optimism > 0.02f64.max(honest.std / nf.sqrt()) {
        warn(
            &mut cautions,
            code::SELECTION_BIAS,
            format!(
                "Choosing the {keep} features on all rows and then cross-validating reports {:.3} \
                 accuracy; repeating the choice inside each fold, on its training rows only, \
                 gives {:.3}. The {optimism:.3} difference is invented by letting the choice see \
                 the rows that score it. Select features inside cross-validation.",
                naive.mean, honest.mean
            ),
        );
    }
    let top_share = stability.iter().take(keep).map(|s| s.share).sum::<f64>() / keep as f64;
    if top_share < 0.6 {
        warn(
            &mut cautions,
            code::UNSTABLE_SELECTION,
            format!(
                "The chosen features change from fold to fold: the {keep} chosen most often \
                 appear in {:.0}% of folds on average. No single subset is well supported by \
                 these rows.",
                100.0 * top_share
            ),
        );
    }
    Ok(SelectionCheck {
        model: spec.describe(),
        target: target.to_string(),
        keep,
        features_available: d,
        folds: folds.len(),
        selected_on_all_rows: chosen_all.iter().map(|&j| ex.feature_names[j].clone()).collect(),
        naive,
        honest,
        optimism,
        stability,
        cautions,
    })
}

/// One step of a forward selection, by name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ForwardStep {
    pub added: String,
    /// Cross-validated accuracy on the training rows with every feature
    /// chosen so far.
    pub cv_accuracy: f64,
    pub cv_std: f64,
}

/// A greedy forward selection, and an honest score for what it recommends.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ForwardReport {
    pub model: String,
    pub target: String,
    pub options: RunOptions,
    pub train_rows: usize,
    pub test_rows: usize,
    pub steps: Vec<ForwardStep>,
    /// The fewest features whose accuracy is within one standard error of the
    /// best step: the most economical set the data cannot tell apart from the
    /// best (the one-standard-error rule).
    pub recommended: Vec<String>,
    /// Accuracy of `recommended` on test rows the selection never saw. The
    /// step accuracies chose the features, so they run optimistic; this does
    /// not.
    pub test_accuracy: f64,
    pub baseline_accuracy: f64,
    pub cautions: Vec<Caution>,
}

/// Greedy forward selection on the training rows, scored on the test rows.
pub fn forward_select(
    frame: &Frame,
    features: &[&str],
    target: &str,
    spec: &dyn ClassifierSpec,
    max_features: usize,
    opts: &RunOptions,
) -> Result<ForwardReport> {
    let p = ClassProblem::new(frame, features, target, opts)?;
    let folds = p.folds.as_ref().ok_or_else(|| {
        DataError::Schema("forward selection needs cross-validation; set cv_folds to 2 or more".into())
    })?;
    let k = p.labels.n_classes();
    let steps = forward_selection(spec, &p.x_train, &p.y_train, k, folds, max_features)?;
    if steps.is_empty() {
        return Err(DataError::Schema("max_features must be at least 1".into()));
    }
    let best = (0..steps.len()).fold(0, |b, i| if steps[i].cv_accuracy > steps[b].cv_accuracy { i } else { b });
    let se = steps[best].cv_std / (folds.len() as f64).sqrt();
    let pick = steps.iter().position(|s| s.cv_accuracy >= steps[best].cv_accuracy - se).unwrap_or(best);
    let chosen = &steps[pick].selected;
    let model = spec.fit(&p.x_train.select_cols(chosen), &p.y_train, k)?;
    let pred = model.predict(&p.x_test.select_cols(chosen))?;
    let test_accuracy = Confusion::new(&p.y_test, &pred, k)?.accuracy();
    let baseline = p.baseline()?;
    let mut cautions = p.cautions.clone();
    if test_accuracy <= baseline.0 + 1e-12 {
        warn(
            &mut cautions,
            code::NO_BETTER_THAN_BASELINE,
            format!(
                "The recommended features score {test_accuracy:.3} on the test rows; always \
                 predicting the most common class scores {:.3}.",
                baseline.0
            ),
        );
    }
    let names = &p.ex.feature_names;
    Ok(ForwardReport {
        model: spec.describe(),
        target: target.to_string(),
        options: opts.clone(),
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        steps: steps
            .iter()
            .map(|s| ForwardStep { added: names[s.added].clone(), cv_accuracy: s.cv_accuracy, cv_std: s.cv_std })
            .collect(),
        recommended: chosen.iter().map(|&j| names[j].clone()).collect(),
        test_accuracy,
        baseline_accuracy: baseline.0,
        cautions,
    })
}

/// One penalty on a LASSO path.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LassoPoint {
    pub alpha: f64,
    /// Mean squared error across folds of the training rows.
    pub cv_mse: f64,
    pub cv_mse_std: f64,
    /// Features the penalty keeps, fitted on all training rows.
    pub nonzero: usize,
}

/// A feature the LASSO keeps.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LassoCoefficient {
    pub feature: String,
    /// In the target's units per unit of the feature.
    pub coefficient: f64,
    /// Per standard deviation of the feature: comparable across features.
    pub standardized: f64,
}

/// The LASSO's penalty chosen by cross-validation, and what it keeps.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LassoReport {
    pub target: String,
    pub options: RunOptions,
    pub train_rows: usize,
    pub test_rows: usize,
    /// Strongest penalty first.
    pub path: Vec<LassoPoint>,
    /// The penalty with the lowest cross-validated error.
    pub alpha_min: f64,
    /// The strongest penalty within one standard error of that minimum: the
    /// sparsest model the data cannot tell apart from the best. The model
    /// below uses it.
    pub alpha_1se: f64,
    /// Kept features, largest standardized effect first.
    pub selected: Vec<LassoCoefficient>,
    /// Features driven to exactly zero.
    pub dropped: Vec<String>,
    pub intercept: f64,
    pub baseline: RegressionMetrics,
    pub test: RegressionMetrics,
    pub cautions: Vec<Caution>,
}

/// Penalties searched, from the smallest that zeroes every feature down to a
/// thousandth of it.
const LASSO_STEPS: usize = 30;

/// Embedded selection: search the LASSO penalty by cross-validation on the
/// training rows, fit at the one-standard-error penalty, and score it on the
/// test rows.
pub fn lasso_cv(frame: &Frame, features: &[&str], target: &str, opts: &RunOptions) -> Result<LassoReport> {
    let p = RegProblem::new(frame, features, target, opts)?;
    let folds = p.folds.as_ref().ok_or_else(|| {
        DataError::Schema("a LASSO path needs cross-validation; set cv_folds to 2 or more".into())
    })?;
    // The smallest penalty that zeroes every coefficient: at the origin, the
    // largest correlation of a standardized feature with the centred target.
    let z = Scaler::fit(&p.x_train)?.transform(&p.x_train)?;
    let n = z.rows() as f64;
    let y_mean = p.y_train.iter().sum::<f64>() / n;
    let alpha_max = (0..z.cols())
        .map(|j| (0..z.rows()).map(|i| z.get(i, j) * (p.y_train[i] - y_mean)).sum::<f64>().abs() / n)
        .fold(0.0, f64::max);
    if !(alpha_max > 0.0) {
        return Err(DataError::Schema(format!(
            "no feature varies with `{target}` in the training rows, so there is no penalty to search"
        )));
    }
    let data: Vec<(Matrix, Vec<f64>, Matrix, Vec<f64>)> = folds
        .iter()
        .map(|f| {
            (
                p.x_train.select_rows(&f.train),
                f.train.iter().map(|&i| p.y_train[i]).collect(),
                p.x_train.select_rows(&f.test),
                f.test.iter().map(|&i| p.y_train[i]).collect(),
            )
        })
        .collect();
    let mut path = Vec::with_capacity(LASSO_STEPS);
    for s in 0..LASSO_STEPS {
        let alpha = alpha_max * 1e-3f64.powf(s as f64 / (LASSO_STEPS - 1) as f64);
        let spec = Lasso { alpha, ..Default::default() };
        let mut mse = Vec::with_capacity(data.len());
        for (xtr, ytr, xte, yte) in &data {
            let pred = spec.fit_model(xtr, ytr)?.predict(xte)?;
            mse.push(yte.iter().zip(&pred).map(|(a, b)| (a - b) * (a - b)).sum::<f64>() / yte.len() as f64);
        }
        let summary = CvSummary::from_scores(mse);
        let nonzero = spec
            .fit_model(&p.x_train, &p.y_train)?
            .standardized_coefficients
            .iter()
            .filter(|&&b| b != 0.0)
            .count();
        path.push(LassoPoint { alpha, cv_mse: summary.mean, cv_mse_std: summary.std, nonzero });
    }
    let best = (0..path.len()).fold(0, |b, i| if path[i].cv_mse < path[b].cv_mse { i } else { b });
    let se = path[best].cv_mse_std / (folds.len() as f64).sqrt();
    let chosen = (0..path.len()).find(|&i| path[i].cv_mse <= path[best].cv_mse + se).unwrap_or(best);
    let alpha = path[chosen].alpha;
    let model = Lasso { alpha, ..Default::default() }.fit_model(&p.x_train, &p.y_train)?;
    let test = regression_metrics(&p.y_test, &model.predict(&p.x_test)?)?;
    let baseline = p.baseline()?;

    let names = &p.ex.feature_names;
    let mut selected: Vec<LassoCoefficient> = (0..names.len())
        .filter(|&j| model.standardized_coefficients[j] != 0.0)
        .map(|j| LassoCoefficient {
            feature: names[j].clone(),
            coefficient: model.linear.coefficients[j],
            standardized: model.standardized_coefficients[j],
        })
        .collect();
    selected.sort_by(|a, b| b.standardized.abs().total_cmp(&a.standardized.abs()));
    let dropped = (0..names.len())
        .filter(|&j| model.standardized_coefficients[j] == 0.0)
        .map(|j| names[j].clone())
        .collect();

    let mut cautions = p.cautions.clone();
    if test.rmse >= baseline.rmse {
        warn(
            &mut cautions,
            code::NO_BETTER_THAN_BASELINE,
            format!(
                "The LASSO misses by {:.4} (RMSE) on the test rows; predicting the training mean \
                 of `{target}` misses by {:.4}.",
                test.rmse, baseline.rmse
            ),
        );
    }
    if !model.converged {
        warn(
            &mut cautions,
            code::NOT_CONVERGED,
            format!(
                "The LASSO stopped at its limit of {} sweeps before converging, so its \
                 coefficients are approximate.",
                model.iterations
            ),
        );
    }
    Ok(LassoReport {
        target: target.to_string(),
        options: opts.clone(),
        train_rows: p.split.train.len(),
        test_rows: p.split.test.len(),
        alpha_min: path[best].alpha,
        alpha_1se: alpha,
        path,
        selected,
        dropped,
        intercept: model.linear.intercept,
        baseline,
        test,
        cautions,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Clustering and PCA
// ─────────────────────────────────────────────────────────────────────────────

/// Which clustering to run.
#[derive(Clone, Debug, PartialEq)]
pub enum ClusterMethod {
    KMeans(KMeans),
    Dbscan(Dbscan),
    Agglomerative(Agglomerative),
}

impl ClusterMethod {
    fn describe(&self) -> String {
        match self {
            Self::KMeans(m) => {
                format!("k_means(k={}, n_init={}, max_iter={}, seed={})", m.k, m.n_init, m.max_iter, m.seed)
            }
            Self::Dbscan(m) => format!("dbscan(eps={}, min_samples={})", m.eps, m.min_samples),
            Self::Agglomerative(m) => {
                format!("agglomerative(n_clusters={}, linkage={:?})", m.n_clusters, m.linkage)
            }
        }
    }
}

/// A clustering of a frame's rows, with the indices that judge it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClusterReport {
    pub method: String,
    pub features: Vec<String>,
    pub standardized: bool,
    pub rows_used: usize,
    pub rows_dropped: usize,
    /// Cluster per frame row: `None` for a dropped row or a DBSCAN noise point.
    pub assignments: Vec<Option<usize>>,
    pub n_clusters: usize,
    /// Rows per cluster, by cluster number.
    pub sizes: Vec<usize>,
    pub noise: usize,
    /// Each cluster's mean, in the features' own units.
    pub centroids: Vec<Vec<f64>>,
    /// Mean silhouette over at most 5000 evenly spaced clustered rows.
    pub silhouette: Option<f64>,
    pub davies_bouldin: Option<f64>,
    pub calinski_harabasz: Option<f64>,
    pub cautions: Vec<Caution>,
}

impl ClusterReport {
    /// The assignments as an integer column, ready to join onto the frame.
    pub fn to_column(&self) -> ColumnData {
        ColumnData::I64(self.assignments.iter().map(|a| a.map(|c| c as i64)).collect())
    }
}

/// Cluster the rows of `frame` on `features`. `standardize` puts every feature
/// on unit variance first, which is right unless the features share a unit.
pub fn cluster(frame: &Frame, features: &[&str], method: &ClusterMethod, standardize: bool) -> Result<ClusterReport> {
    let ex = extract_features(frame, features)?;
    let mut cautions = Vec::new();
    data_cautions(&ex, &mut cautions);
    let input = if standardize {
        Scaler::fit(&ex.x)?.transform(&ex.x)?
    } else {
        mixed_scales_caution(&ex, &mut cautions);
        ex.x.clone()
    };
    let labels: Vec<Option<usize>> = match method {
        ClusterMethod::KMeans(m) => m.fit(&input)?.labels.into_iter().map(Some).collect(),
        ClusterMethod::Dbscan(m) => m.fit(&input)?,
        ClusterMethod::Agglomerative(m) => m.fit(&input)?.into_iter().map(Some).collect(),
    };
    let n_clusters = labels.iter().flatten().max().map_or(0, |m| m + 1);
    let mut sizes = vec![0usize; n_clusters];
    for &c in labels.iter().flatten() {
        sizes[c] += 1;
    }
    let noise = labels.iter().filter(|l| l.is_none()).count();

    let d = ex.x.cols();
    let mut centroids = vec![vec![0.0; d]; n_clusters];
    for (i, l) in labels.iter().enumerate() {
        if let Some(c) = l {
            for (s, v) in centroids[*c].iter_mut().zip(ex.x.row(i)) {
                *s += v;
            }
        }
    }
    for (centre, &size) in centroids.iter_mut().zip(&sizes) {
        if size > 0 {
            centre.iter_mut().for_each(|v| *v /= size as f64);
        }
    }

    // The indices judge the clustering in the space it was computed in.
    let (clustered, flat): (Vec<usize>, Vec<usize>) =
        labels.iter().enumerate().filter_map(|(i, l)| l.map(|c| (i, c))).unzip();
    let xi = input.select_rows(&clustered);
    let sample = spaced(xi.rows(), DIAGNOSTIC_ROWS);
    let sample_labels: Vec<usize> = sample.iter().map(|&i| flat[i]).collect();
    let sil = silhouette(&xi.select_rows(&sample), &sample_labels).ok();
    let db = davies_bouldin(&xi, &flat).ok();
    let ch = calinski_harabasz(&xi, &flat).ok();

    let present = sizes.iter().filter(|&&s| s > 0).count();
    if present < 2 {
        let hint = match method {
            ClusterMethod::Dbscan(_) => {
                " For DBSCAN, change eps: too large and every chain of points merges into one \
                 cluster, too small and nothing clusters at all."
            }
            _ => "",
        };
        warn(
            &mut cautions,
            code::SINGLE_CLUSTER,
            format!(
                "The clustering found {present} {}, so it says nothing about structure and the \
                 quality indices are undefined.{hint}",
                plural(present, "cluster", "clusters")
            ),
        );
    }
    if noise * 2 > labels.len() {
        warn(
            &mut cautions,
            code::MOSTLY_NOISE,
            format!(
                "DBSCAN labelled {noise} of {} rows as noise. eps is likely small for the scale \
                 of these features; standardize them or raise it.",
                labels.len()
            ),
        );
    }

    let mut assignments = vec![None; frame.n_rows()];
    for (&r, &l) in ex.kept_rows.iter().zip(&labels) {
        assignments[r] = l;
    }
    Ok(ClusterReport {
        method: method.describe(),
        features: ex.feature_names.clone(),
        standardized: standardize,
        rows_used: ex.x.rows(),
        rows_dropped: ex.dropped,
        assignments,
        n_clusters,
        sizes,
        noise,
        centroids,
        silhouette: sil,
        davies_bouldin: db,
        calinski_harabasz: ch,
        cautions,
    })
}

/// One feature's weight in a component.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Loading {
    pub feature: String,
    pub weight: f64,
}

/// One principal component.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Component {
    pub explained_variance: f64,
    pub explained_ratio: f64,
    pub cumulative_ratio: f64,
    /// The component's direction; largest magnitude first.
    pub loadings: Vec<Loading>,
}

/// A principal component analysis of a frame's features.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PcaReport {
    pub features: Vec<String>,
    pub standardized: bool,
    pub rows_used: usize,
    pub rows_dropped: usize,
    pub components: Vec<Component>,
    /// Components needed to keep 95% of the variance.
    pub components_for_95: usize,
    pub cautions: Vec<Caution>,
}

/// Principal components of `features`, every one of them, so the report shows
/// how many the data really need.
pub fn pca(frame: &Frame, features: &[&str], standardize: bool) -> Result<PcaReport> {
    let ex = extract_features(frame, features)?;
    let mut cautions = Vec::new();
    data_cautions(&ex, &mut cautions);
    if !standardize {
        mixed_scales_caution(&ex, &mut cautions);
    }
    let m = Pca { n_components: None, standardize }.fit_model(&ex.x)?;
    let cumulative = m.cumulative_ratio();
    let components = (0..m.components.len())
        .map(|c| {
            let mut loadings: Vec<Loading> = m.components[c]
                .iter()
                .zip(&ex.feature_names)
                .map(|(&weight, f)| Loading { feature: f.clone(), weight })
                .collect();
            loadings.sort_by(|a, b| b.weight.abs().total_cmp(&a.weight.abs()));
            Component {
                explained_variance: m.explained_variance[c],
                explained_ratio: m.explained_variance_ratio[c],
                cumulative_ratio: cumulative[c],
                loadings,
            }
        })
        .collect();
    let components_for_95 =
        cumulative.iter().position(|&c| c >= 0.95 - 1e-12).map_or(cumulative.len(), |i| i + 1);
    Ok(PcaReport {
        features: ex.feature_names.clone(),
        standardized: standardize,
        rows_used: ex.x.rows(),
        rows_dropped: ex.dropped,
        components,
        components_for_95,
        cautions,
    })
}

/// Principal component scores as frame columns `PC1` to `PCn`, `None` on rows
/// with a missing feature.
pub fn pca_columns(
    frame: &Frame,
    features: &[&str],
    standardize: bool,
    n_components: usize,
) -> Result<Vec<(ColumnSpec, ColumnData)>> {
    let ex = extract_features(frame, features)?;
    let m = Pca { n_components: Some(n_components), standardize }.fit_model(&ex.x)?;
    let scores = m.transform(&ex.x)?;
    Ok((0..scores.cols())
        .map(|c| {
            let mut col = vec![None; frame.n_rows()];
            for (i, &r) in ex.kept_rows.iter().enumerate() {
                col[r] = Some(scores.get(i, c));
            }
            (ColumnSpec::new(format!("PC{}", c + 1), ColumnDtype::F64), ColumnData::F64(col))
        })
        .collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// Association rules and residual comparisons
// ─────────────────────────────────────────────────────────────────────────────

/// Where a frame's baskets come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baskets<'a> {
    /// One row per (basket, item): an order-lines table.
    Long { basket: &'a str, item: &'a str },
    /// One row per basket, one true/false column per item.
    Flags(&'a [&'a str]),
}

/// Frequent itemsets and association rules from a frame's baskets.
pub fn associate(frame: &Frame, baskets: Baskets<'_>, apriori: &Apriori) -> Result<Mined> {
    let transactions = match baskets {
        Baskets::Long { basket, item } => transactions_from_frame(frame, basket, item)?,
        Baskets::Flags(cols) => transactions_from_flags(frame, cols)?,
    };
    apriori.mine(&transactions)
}

fn numeric(frame: &Frame, name: &str) -> Result<Vec<f64>> {
    let col = frame
        .column(name)
        .ok_or_else(|| DataError::Schema(format!("no column `{name}`")))?;
    Ok(as_f64_opt(col)?.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect())
}

/// Residuals of a simulated column against a measured one, row for row. Rows
/// missing either value are skipped.
pub fn compare_columns(frame: &Frame, measured: &str, simulated: &str) -> Result<Residuals> {
    compare(&numeric(frame, measured)?, &numeric(frame, simulated)?)
}

/// A time series held in two columns of a frame.
#[derive(Clone, Copy, Debug)]
pub struct Series<'a> {
    pub frame: &'a Frame,
    pub time: &'a str,
    pub value: &'a str,
}

/// Residuals of a simulated series against a measured one, each on its own
/// clock. The simulation is interpolated onto each measured time inside its
/// span and never extrapolated beyond it.
pub fn compare_frames(measured: Series<'_>, simulated: Series<'_>) -> Result<Residuals> {
    compare_series(
        &numeric(measured.frame, measured.time)?,
        &numeric(measured.frame, measured.value)?,
        &numeric(simulated.frame, simulated.time)?,
        &numeric(simulated.frame, simulated.value)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mine::Rng;
    use crate::{frame_from_columns, ColumnDtype as T};

    fn has(cautions: &[Caution], code: &str) -> bool {
        cautions.iter().any(|c| c.code == code)
    }

    fn f64_col(name: &str, v: Vec<f64>) -> (ColumnSpec, ColumnData) {
        (ColumnSpec::new(name, T::F64), ColumnData::F64(v.into_iter().map(Some).collect()))
    }

    fn str_col(name: &str, v: Vec<&str>) -> (ColumnSpec, ColumnData) {
        (ColumnSpec::new(name, T::Str), ColumnData::Str(v.into_iter().map(|s| Some(s.to_string())).collect()))
    }

    /// 40 rows: class `a` near (0, 0), class `b` near (10, 10), no two rows
    /// alike, and a `noise` column balanced across the classes.
    fn blobs() -> Frame {
        let x = (0..40).map(|i| if i < 20 { 0.0 } else { 10.0 } + (i % 5) as f64 * 0.3).collect();
        let y = (0..40).map(|i| if i < 20 { 0.0 } else { 10.0 } + (i % 4) as f64 * 0.2).collect();
        let noise = (0..40).map(|i| ((i * 37) % 11) as f64).collect();
        let kind = (0..40).map(|i| if i < 20 { "a" } else { "b" }).collect();
        frame_from_columns(vec![f64_col("x", x), f64_col("y", y), f64_col("noise", noise), str_col("kind", kind)])
            .unwrap()
    }

    /// `t = 3a - 2b + 1` exactly, plus a `c` the target ignores.
    fn plane() -> Frame {
        let a: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let b: Vec<f64> = (0..40).map(|i| ((i * 7) % 5) as f64).collect();
        let c: Vec<f64> = (0..40).map(|i| ((i * 37) % 11) as f64).collect();
        let t: Vec<f64> = a.iter().zip(&b).map(|(a, b)| 3.0 * a - 2.0 * b + 1.0).collect();
        frame_from_columns(vec![f64_col("a", a), f64_col("b", b), f64_col("c", c), f64_col("t", t)]).unwrap()
    }

    #[test]
    fn a_clean_problem_scores_perfectly_and_the_separator_is_flagged() {
        let r = classify(&blobs(), &["x", "y", "noise"], "kind", &DecisionTree::default(), &RunOptions::default())
            .unwrap();
        assert_eq!(r.classes, vec!["a", "b"]);
        assert_eq!((r.rows_used, r.train_rows, r.test_rows), (40, 30, 10));
        assert_eq!(r.accuracy, 1.0);
        // Stratified: 5 of each class held out, and the tied training majority
        // goes to the first class.
        assert_eq!(r.baseline_accuracy, 0.5);
        assert_eq!(r.roc_auc, Some(1.0));
        assert_eq!(r.cv_accuracy.as_ref().map(|c| c.mean), Some(1.0));
        assert_eq!(r.confusion, vec![vec![5, 0], vec![0, 5]]);
        assert!(has(&r.cautions, code::PERFECT_SEPARATOR), "{:?}", r.cautions);
        assert!(!has(&r.cautions, code::NO_BETTER_THAN_BASELINE));
        // The tree splits on `x`, the first of two equally perfect features.
        assert_eq!(r.importance[0].feature, "x");
        assert!(has(&r.cautions, code::DOMINANT_FEATURE));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"code\":\"perfect_separator\""), "{json}");
    }

    #[test]
    fn a_feature_that_cannot_help_is_reported_as_no_better_than_guessing() {
        let label: Vec<&str> = (0..30).map(|i| if i < 10 { "no" } else { "yes" }).collect();
        let f = frame_from_columns(vec![f64_col("c", vec![1.0; 30]), str_col("label", label)]).unwrap();
        let r = classify(&f, &["c"], "label", &DecisionTree::default(), &RunOptions::default()).unwrap();
        assert_eq!(r.accuracy, r.baseline_accuracy);
        assert!(has(&r.cautions, code::CONSTANT_FEATURE));
        assert!(has(&r.cautions, code::NO_BETTER_THAN_BASELINE), "{:?}", r.cautions);
    }

    #[test]
    fn a_dominant_class_and_a_rare_one_are_both_flagged() {
        let x: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let label: Vec<&str> = (0..50).map(|i| if i >= 45 { "fault" } else { "ok" }).collect();
        let f = frame_from_columns(vec![f64_col("x", x), str_col("label", label)]).unwrap();
        let r = classify(&f, &["x"], "label", &KNearestNeighbors::default(), &RunOptions::default()).unwrap();
        // 45 of 50 is 90%; 4 training rows of `fault` against 5 folds.
        assert!(has(&r.cautions, code::IMBALANCED), "{:?}", r.cautions);
        assert!(has(&r.cautions, code::RARE_CLASS), "{:?}", r.cautions);
    }

    #[test]
    fn a_group_column_keeps_an_entity_off_both_sides() {
        // 20 entities, three identical rows each.
        let entity: Vec<i64> = (0..60).map(|i| i / 3).collect();
        let x: Vec<f64> = entity.iter().map(|&e| e as f64 * 1.37).collect();
        let label: Vec<&str> = entity.iter().map(|&e| if e % 2 == 0 { "even" } else { "odd" }).collect();
        let f = frame_from_columns(vec![
            (ColumnSpec::new("entity", T::I64), ColumnData::I64(entity.into_iter().map(Some).collect())),
            f64_col("x", x),
            str_col("label", label),
        ])
        .unwrap();
        let spec = KNearestNeighbors::default();
        // 8 test rows per class cannot be whole triplets, so a plain split
        // divides at least one entity.
        let loose = classify(&f, &["x"], "label", &spec, &RunOptions::default()).unwrap();
        assert!(has(&loose.cautions, code::TRAIN_TEST_OVERLAP), "{:?}", loose.cautions);
        let opts = RunOptions { group: Some("entity".into()), ..Default::default() };
        let grouped = classify(&f, &["x"], "label", &spec, &opts).unwrap();
        assert!(!has(&grouped.cautions, code::TRAIN_TEST_OVERLAP), "{:?}", grouped.cautions);
        assert_eq!(grouped.test_rows % 3, 0, "whole entities only");
    }

    #[test]
    fn regression_recovers_the_equation_and_beats_the_mean() {
        let r = regress(&plane(), &["a", "b"], "t", &LinearRegression::default(), &RunOptions::default()).unwrap();
        assert!(r.test.r2 > 1.0 - 1e-9, "{:?}", r.test);
        assert!(r.baseline.r2 <= 1e-12, "predicting a constant never beats the test mean");
        let coef: Vec<(&str, f64)> = r.coefficients.iter().map(|c| (c.feature.as_str(), c.value)).collect();
        assert_eq!(coef[0].0, "a");
        assert!((coef[0].1 - 3.0).abs() < 1e-9 && (coef[1].1 + 2.0).abs() < 1e-9, "{coef:?}");
        assert!((r.intercept.unwrap() - 1.0).abs() < 1e-9);
        assert!(!has(&r.cautions, code::NO_BETTER_THAN_BASELINE));
    }

    #[test]
    fn the_leaderboard_ranks_by_cross_validation() {
        // XOR on four corners, ten copies each: no line separates it, a tree
        // does.
        let p: Vec<f64> = (0..40).map(|i| (i % 2) as f64).collect();
        let q: Vec<f64> = (0..40).map(|i| ((i / 2) % 2) as f64).collect();
        let xor: Vec<&str> = p.iter().zip(&q).map(|(a, b)| if a == b { "same" } else { "differ" }).collect();
        let f = frame_from_columns(vec![f64_col("p", p), f64_col("q", q), str_col("xor", xor)]).unwrap();
        let (logistic, tree) = (LogisticRegression::default(), DecisionTree::default());
        let board =
            compare_classifiers(&f, &["p", "q"], "xor", &[&logistic, &tree], &RunOptions::default()).unwrap();
        assert!(board.entries[0].model.starts_with("decision_tree"), "{:?}", board.entries);
        assert_eq!(board.entries[0].cv_macro_f1.as_ref().map(|c| c.mean), Some(1.0));
        assert_eq!(board.entries[0].accuracy, 1.0);
        assert!(!has(&board.cautions, code::RANKED_ON_TEST));
    }

    #[test]
    fn regressors_rank_exact_least_squares_above_a_shrunk_fit() {
        let (ols, lasso) = (LinearRegression::default(), Lasso { alpha: 1.0, ..Default::default() });
        let board =
            compare_regressors(&plane(), &["a", "b"], "t", &[&lasso, &ols], &RunOptions::default()).unwrap();
        assert_eq!(board.entries[0].model, "linear_regression");
    }

    #[test]
    fn ranking_puts_the_informative_feature_first() {
        let f = frame_from_columns(vec![
            f64_col("noise", (0..40).map(|i| ((i * 37) % 11) as f64).collect()),
            f64_col("x", (0..40).map(|i| if i < 20 { 0.0 } else { 10.0 } + (i % 5) as f64 * 0.3).collect()),
            f64_col("c", vec![2.0; 40]),
            str_col("kind", (0..40).map(|i| if i < 20 { "a" } else { "b" }).collect()),
        ])
        .unwrap();
        let r = rank_features(&f, &["noise", "x", "c"], "kind", Task::Classification).unwrap();
        let order: Vec<&str> = r.features.iter().map(|f| f.feature.as_str()).collect();
        assert_eq!(order, vec!["x", "noise", "c"]);
        assert_eq!(r.bins, 3);
        // x: bins of 12, 12 and 16 rows split the classes 12:0, 8:4, 0:16.
        let want = 0.3 * 2f64.ln() + 0.2 * (0.2f64 / 0.15).ln() + 0.1 * (0.1f64 / 0.15).ln() + 0.4 * 2f64.ln();
        assert!((r.features[0].mutual_information - want).abs() < 1e-12, "{:?}", r.features[0]);
        // The noise column falls 6:6, 7:7, 7:7 across its bins: exactly zero.
        assert!(r.features[1].mutual_information.abs() < 1e-12);
        assert!(has(&r.cautions, code::CONSTANT_FEATURE));
    }

    /// 60 rows with alternating labels and 200 columns of uniform noise: no
    /// feature carries any information about the label.
    fn pure_noise() -> (Frame, Vec<String>) {
        let mut rng = Rng::new(11);
        let names: Vec<String> = (0..200).map(|j| format!("f{j}")).collect();
        let mut cols: Vec<(ColumnSpec, ColumnData)> =
            names.iter().map(|n| f64_col(n, (0..60).map(|_| rng.next_f64()).collect())).collect();
        cols.push(str_col("label", (0..60).map(|i| if i % 2 == 0 { "a" } else { "b" }).collect()));
        (frame_from_columns(cols).unwrap(), names)
    }

    #[test]
    fn selecting_on_all_rows_is_caught_inflating_the_score() {
        let (f, names) = pure_noise();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let c = selection_check(&f, &refs, "label", &GaussianNaiveBayes::default(), 10, &RunOptions::default())
            .unwrap();
        assert!(c.optimism > 0.15, "naive {:?} honest {:?}", c.naive, c.honest);
        assert!(has(&c.cautions, code::SELECTION_BIAS), "{:?}", c.cautions);
        assert!(has(&c.cautions, code::UNSTABLE_SELECTION), "{:?}", c.cautions);
        assert_eq!(c.selected_on_all_rows.len(), 10);
    }

    #[test]
    fn the_lasso_path_keeps_what_the_target_uses() {
        let r = lasso_cv(&plane(), &["a", "b", "c"], "t", &RunOptions::default()).unwrap();
        let kept: Vec<&str> = r.selected.iter().map(|s| s.feature.as_str()).collect();
        assert_eq!(kept, vec!["a", "b"], "{:?}", r.selected);
        assert_eq!(r.dropped, vec!["c"]);
        assert!(r.alpha_1se >= r.alpha_min);
        assert!(r.test.r2 > 0.99, "{:?}", r.test);
        assert_eq!(r.path.len(), LASSO_STEPS);
        assert_eq!(r.path[0].nonzero, 0, "the largest penalty zeroes everything");
    }

    #[test]
    fn clustering_aligns_assignments_with_the_frame_rows() {
        let mut x: Vec<Option<f64>> =
            (0..40).map(|i| Some(if i < 20 { 0.0 } else { 10.0 } + (i % 5) as f64 * 0.3)).collect();
        x[7] = None;
        let y = (0..40).map(|i| if i < 20 { 0.0 } else { 10.0 } + (i % 4) as f64 * 0.2).collect();
        let f = frame_from_columns(vec![(ColumnSpec::new("x", T::F64), ColumnData::F64(x)), f64_col("y", y)])
            .unwrap();
        let method = ClusterMethod::KMeans(KMeans { k: 2, ..Default::default() });
        let r = cluster(&f, &["x", "y"], &method, true).unwrap();
        assert_eq!((r.rows_used, r.rows_dropped), (39, 1));
        assert_eq!(r.assignments[7], None);
        assert_eq!(r.assignments[0], Some(0));
        assert_eq!(r.assignments[39], Some(1));
        assert_eq!(r.sizes, vec![19, 20]);
        assert!(r.silhouette.unwrap() > 0.8, "{:?}", r.silhouette);
        assert!((r.centroids[1][0] - 10.6).abs() < 1e-9, "{:?}", r.centroids);
        assert_eq!(r.to_column().len(), 40);
    }

    #[test]
    fn a_dbscan_radius_that_is_too_small_is_diagnosed() {
        let method = ClusterMethod::Dbscan(Dbscan { eps: 1e-6, min_samples: 3 });
        let r = cluster(&blobs(), &["x", "y"], &method, true).unwrap();
        assert_eq!(r.noise, 40);
        assert!(has(&r.cautions, code::MOSTLY_NOISE));
        assert!(has(&r.cautions, code::SINGLE_CLUSTER));
    }

    #[test]
    fn pca_sees_two_copies_of_one_signal_as_one_component() {
        let a: Vec<f64> = (0..20).map(|i| i as f64).collect();
        let b: Vec<f64> = a.iter().map(|v| 2.0 * v).collect();
        let f = frame_from_columns(vec![f64_col("a", a), f64_col("b", b)]).unwrap();
        let r = pca(&f, &["a", "b"], true).unwrap();
        assert_eq!(r.components_for_95, 1);
        let w = &r.components[0].loadings;
        assert!((w[0].weight.abs() - w[1].weight.abs()).abs() < 1e-9, "{w:?}");
        assert!(has(&r.cautions, code::COLLINEAR));
    }

    #[test]
    fn unstandardized_mixed_units_are_flagged() {
        let f = frame_from_columns(vec![
            f64_col("grams", (0..20).map(|i| i as f64 * 1000.0).collect()),
            f64_col("count", (0..20).map(|i| ((i * 7) % 5) as f64).collect()),
        ])
        .unwrap();
        assert!(has(&pca(&f, &["grams", "count"], false).unwrap().cautions, code::MIXED_SCALES));
        assert!(!has(&pca(&f, &["grams", "count"], true).unwrap().cautions, code::MIXED_SCALES));
    }

    #[test]
    fn a_fitted_model_scores_new_rows_and_skips_incomplete_ones() {
        let fitted = FittedClassifier::fit(&blobs(), &["x", "y"], "kind", &DecisionTree::default()).unwrap();
        let new = frame_from_columns(vec![
            (ColumnSpec::new("x", T::F64), ColumnData::F64(vec![Some(0.1), Some(10.5), None])),
            f64_col("y", vec![0.2, 10.1, 3.0]),
        ])
        .unwrap();
        assert_eq!(
            fitted.predict_column(&new).unwrap(),
            ColumnData::Str(vec![Some("a".into()), Some("b".into()), None])
        );
        let p = fitted.probability_columns(&new).unwrap();
        assert_eq!(p[0].0.name, "p(a)");
        assert_eq!(p[0].1, ColumnData::F64(vec![Some(1.0), Some(0.0), None]));
    }

    #[test]
    fn models_are_found_by_the_names_they_report() {
        for name in ["logistic_regression", "decision_tree", "k_nearest_neighbors", "gaussian_naive_bayes"] {
            assert_eq!(classifier_by_name(name).map(|s| s.name()), Some(name));
        }
        for name in ["linear_regression", "ridge_regression", "lasso"] {
            assert_eq!(regressor_by_name(name).map(|s| s.name()), Some(name));
        }
        assert!(classifier_by_name("svm").is_none());
    }

    #[test]
    fn baskets_are_mined_from_either_table_shape() {
        let f = frame_from_columns(vec![
            (ColumnSpec::new("order", T::I64), ColumnData::I64(vec![Some(1), Some(1), Some(2), Some(2), Some(3)])),
            str_col("sku", vec!["A", "B", "A", "B", "A"]),
        ])
        .unwrap();
        let apriori = Apriori { min_support: 0.5, min_confidence: 0.5, max_len: 2 };
        let m = associate(&f, Baskets::Long { basket: "order", item: "sku" }, &apriori).unwrap();
        assert_eq!(m.transactions, 3);
        let r = m.rules.iter().find(|r| r.antecedent == ["B"] && r.consequent == ["A"]).unwrap();
        assert_eq!(r.confidence, 1.0);
    }

    #[test]
    fn simulation_residuals_come_from_columns_or_from_two_clocks() {
        let f = frame_from_columns(vec![
            (ColumnSpec::new("measured", T::F64), ColumnData::F64(vec![Some(1.0), Some(2.0), Some(3.0), None])),
            f64_col("simulated", vec![1.5, 2.5, 3.5, 4.0]),
        ])
        .unwrap();
        let r = compare_columns(&f, "measured", "simulated").unwrap();
        assert_eq!(r.n, 3);
        assert!((r.bias - 0.5).abs() < 1e-12 && (r.rmse - 0.5).abs() < 1e-12);

        let meas = frame_from_columns(vec![
            f64_col("t", vec![0.0, 1.0, 2.0, 3.0]),
            f64_col("y", vec![0.0, 10.0, 20.0, 30.0]),
        ])
        .unwrap();
        let sim = frame_from_columns(vec![f64_col("time", vec![0.0, 2.0, 4.0]), f64_col("out", vec![0.0, 20.0, 40.0])])
            .unwrap();
        let r = compare_frames(
            Series { frame: &meas, time: "t", value: "y" },
            Series { frame: &sim, time: "time", value: "out" },
        )
        .unwrap();
        assert_eq!(r.n, 4);
        assert!(r.rmse < 1e-12);
    }
}
