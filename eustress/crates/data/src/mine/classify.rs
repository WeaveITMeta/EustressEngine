//! Classification: logistic regression, a CART decision tree, k-nearest
//! neighbours, and Gaussian naive Bayes.
//!
//! The four cover the families the field reaches for first, and they fail in
//! different ways, which is the point of having all four: a linear model cannot
//! learn XOR, a tree cannot draw a smooth diagonal boundary, k-NN degrades in
//! high dimension, and naive Bayes assumes features are independent. Comparing
//! them under cross-validation is how you learn which assumption your data
//! breaks.

use serde::Serialize;

use super::{argmax, dist2, Matrix, Scaler};
use crate::{DataError, Result};

/// A fitted classifier.
pub trait Classifier: Send + Sync {
    fn n_classes(&self) -> usize;

    /// Class probabilities per row; each row sums to 1.
    fn predict_proba(&self, x: &Matrix) -> Result<Vec<Vec<f64>>>;

    /// Most probable class per row. Ties go to the lowest class index.
    fn predict(&self, x: &Matrix) -> Result<Vec<usize>> {
        Ok(self.predict_proba(x)?.iter().map(|p| argmax(p)).collect())
    }
}

/// Hyperparameters that fit a [`Classifier`].
pub trait ClassifierSpec {
    fn name(&self) -> &'static str;

    /// The name with every hyperparameter, so a report records exactly which
    /// model ran.
    fn describe(&self) -> String {
        self.name().to_string()
    }

    fn fit(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<Box<dyn Classifier>>;
}

fn check_training(x: &Matrix, y: &[usize], n_classes: usize) -> Result<()> {
    if x.is_empty() {
        return Err(DataError::Schema("cannot fit a classifier on zero rows".into()));
    }
    if x.rows() != y.len() {
        return Err(DataError::Schema(format!(
            "{} feature rows but {} labels",
            x.rows(),
            y.len()
        )));
    }
    if n_classes < 2 {
        return Err(DataError::Schema("classification needs at least 2 classes".into()));
    }
    if let Some(&bad) = y.iter().find(|&&c| c >= n_classes) {
        return Err(DataError::Schema(format!("label {bad} is outside 0..{n_classes}")));
    }
    Ok(())
}

fn check_width(x: &Matrix, n_features: usize) -> Result<()> {
    if x.cols() != n_features {
        return Err(DataError::Schema(format!(
            "model was fitted on {n_features} features, given {}",
            x.cols()
        )));
    }
    Ok(())
}

/// Numerically stable log-sum-exp.
fn log_sum_exp(v: &[f64]) -> f64 {
    let m = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !m.is_finite() {
        return m;
    }
    m + v.iter().map(|x| (x - m).exp()).sum::<f64>().ln()
}

/// Softmax in place, shifted by the maximum so no exponent overflows.
fn softmax_in_place(v: &mut [f64]) {
    let lse = log_sum_exp(v);
    for x in v.iter_mut() {
        *x = (*x - lse).exp();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Logistic regression
// ─────────────────────────────────────────────────────────────────────────────

/// Multinomial logistic regression with an L2 penalty.
///
/// Features are standardised internally (on the training rows), which keeps the
/// optimisation well conditioned and makes the penalty treat every feature
/// alike. Fitted by full-batch gradient descent with a backtracking line search:
/// the loss is convex, so this reaches the optimum without a learning rate to
/// tune.
#[derive(Clone, Debug, PartialEq)]
pub struct LogisticRegression {
    /// L2 penalty on the weights (never the intercepts). Some penalty is needed
    /// when classes separate perfectly, where the unpenalised optimum is at
    /// infinity.
    pub l2: f64,
    pub max_iter: usize,
    /// Stop once the gradient norm falls below this.
    pub tol: f64,
}

impl Default for LogisticRegression {
    fn default() -> Self {
        Self { l2: 1e-3, max_iter: 2000, tol: 1e-6 }
    }
}

/// A fitted logistic regression.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LogisticModel {
    #[serde(skip)]
    pub scaler: Scaler,
    pub n_classes: usize,
    pub n_features: usize,
    /// `n_classes` rows of `n_features + 1` values: the weights on the
    /// STANDARDISED features, then the intercept.
    pub weights: Vec<Vec<f64>>,
    /// Iterations the optimiser used.
    pub iterations: usize,
    /// Whether the gradient fell below `tol` before `max_iter`.
    pub converged: bool,
}

impl LogisticModel {
    fn logits_row(&self, z: &[f64], out: &mut [f64]) {
        let d = self.n_features;
        for (c, w) in self.weights.iter().enumerate() {
            let mut s = w[d];
            for j in 0..d {
                s += w[j] * z[j];
            }
            out[c] = s;
        }
    }
}

impl Classifier for LogisticModel {
    fn n_classes(&self) -> usize {
        self.n_classes
    }

    fn predict_proba(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        check_width(x, self.n_features)?;
        let z = self.scaler.transform(x)?;
        let mut out = Vec::with_capacity(z.rows());
        for i in 0..z.rows() {
            let mut p = vec![0.0; self.n_classes];
            self.logits_row(z.row(i), &mut p);
            softmax_in_place(&mut p);
            out.push(p);
        }
        Ok(out)
    }
}

impl LogisticRegression {
    /// Mean negative log-likelihood plus the L2 term, and its gradient.
    fn loss_and_grad(&self, z: &Matrix, y: &[usize], k: usize, w: &[f64], grad: &mut [f64]) -> f64 {
        let d = z.cols();
        let stride = d + 1;
        let n = z.rows() as f64;
        grad.iter_mut().for_each(|g| *g = 0.0);
        let mut loss = 0.0;
        let mut logits = vec![0.0; k];
        for i in 0..z.rows() {
            let row = z.row(i);
            for (c, l) in logits.iter_mut().enumerate() {
                let wc = &w[c * stride..(c + 1) * stride];
                let mut s = wc[d];
                for j in 0..d {
                    s += wc[j] * row[j];
                }
                *l = s;
            }
            let lse = log_sum_exp(&logits);
            // log p(y_i) = logit_y - lse: exact, no clamping of probabilities.
            loss -= logits[y[i]] - lse;
            for c in 0..k {
                let p = (logits[c] - lse).exp();
                let r = p - if c == y[i] { 1.0 } else { 0.0 };
                let gc = &mut grad[c * stride..(c + 1) * stride];
                for j in 0..d {
                    gc[j] += r * row[j];
                }
                gc[d] += r;
            }
        }
        loss /= n;
        for g in grad.iter_mut() {
            *g /= n;
        }
        for c in 0..k {
            for j in 0..d {
                let wv = w[c * stride + j];
                loss += 0.5 * self.l2 * wv * wv;
                grad[c * stride + j] += self.l2 * wv;
            }
        }
        loss
    }

    /// Fit and return the concrete model (weights accessible).
    pub fn fit_model(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<LogisticModel> {
        check_training(x, y, n_classes)?;
        if !(self.l2 >= 0.0) {
            return Err(DataError::Schema("the L2 penalty must be non-negative".into()));
        }
        let scaler = Scaler::fit(x)?;
        let z = scaler.transform(x)?;
        let (d, k) = (z.cols(), n_classes);
        let len = k * (d + 1);
        let mut w = vec![0.0; len];
        let mut g = vec![0.0; len];
        let mut trial = vec![0.0; len];
        let mut g_trial = vec![0.0; len];
        let mut loss = self.loss_and_grad(&z, y, k, &w, &mut g);
        let mut step = 1.0;
        let mut iterations = 0;
        let mut converged = false;
        while iterations < self.max_iter {
            let gnorm2: f64 = g.iter().map(|v| v * v).sum();
            if gnorm2.sqrt() < self.tol {
                converged = true;
                break;
            }
            // Backtracking (Armijo): halve the step until the loss falls by at
            // least half the first-order prediction.
            let mut accepted = false;
            while step > 1e-12 {
                for i in 0..len {
                    trial[i] = w[i] - step * g[i];
                }
                let l = self.loss_and_grad(&z, y, k, &trial, &mut g_trial);
                if l <= loss - 0.5 * step * gnorm2 {
                    std::mem::swap(&mut w, &mut trial);
                    std::mem::swap(&mut g, &mut g_trial);
                    loss = l;
                    accepted = true;
                    break;
                }
                step *= 0.5;
            }
            iterations += 1;
            if !accepted {
                // No descent is possible at machine precision: the optimum.
                converged = true;
                break;
            }
            // Let the step grow back so one hard iteration does not slow the
            // rest of the fit.
            step = (step * 2.0).min(1e3);
        }
        Ok(LogisticModel {
            scaler,
            n_classes: k,
            n_features: d,
            weights: w.chunks(d + 1).map(<[f64]>::to_vec).collect(),
            iterations,
            converged,
        })
    }
}

impl ClassifierSpec for LogisticRegression {
    fn name(&self) -> &'static str {
        "logistic_regression"
    }

    fn describe(&self) -> String {
        format!("logistic_regression(l2={}, max_iter={}, tol={})", self.l2, self.max_iter, self.tol)
    }

    fn fit(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<Box<dyn Classifier>> {
        Ok(Box::new(self.fit_model(x, y, n_classes)?))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Decision tree (CART)
// ─────────────────────────────────────────────────────────────────────────────

/// How a split's quality is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Criterion {
    /// Probability that two random draws from the node disagree.
    Gini,
    /// Shannon entropy in bits.
    Entropy,
}

impl Criterion {
    fn impurity(self, counts: &[usize], n: usize) -> f64 {
        if n == 0 {
            return 0.0;
        }
        let nf = n as f64;
        match self {
            Self::Gini => 1.0 - counts.iter().map(|&c| (c as f64 / nf).powi(2)).sum::<f64>(),
            Self::Entropy => -counts
                .iter()
                .filter(|&&c| c > 0)
                .map(|&c| {
                    let p = c as f64 / nf;
                    p * p.log2()
                })
                .sum::<f64>(),
        }
    }
}

/// A CART classification tree: binary splits of the form `feature <= threshold`.
///
/// Built iteratively rather than recursively, so a deep tree on a large dataset
/// cannot overflow the stack. Split ties go to the lowest feature index and then
/// the lowest threshold, so the same data always grows the same tree.
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionTree {
    pub criterion: Criterion,
    /// `None` grows until nodes are pure or too small to split.
    pub max_depth: Option<usize>,
    /// A node needs at least this many rows to be split.
    pub min_samples_split: usize,
    /// Each child must keep at least this many rows.
    pub min_samples_leaf: usize,
}

impl Default for DecisionTree {
    fn default() -> Self {
        Self { criterion: Criterion::Gini, max_depth: None, min_samples_split: 2, min_samples_leaf: 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
enum TreeNode {
    Leaf { proba: Vec<f64> },
    Split { feature: usize, threshold: f64, left: usize, right: usize },
}

/// A fitted decision tree.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TreeModel {
    nodes: Vec<TreeNode>,
    pub n_classes: usize,
    pub n_features: usize,
    /// Mean decrease in impurity per feature, normalised to sum to 1 (all zero
    /// for a tree that never split).
    pub importances: Vec<f64>,
    pub depth: usize,
    pub leaves: usize,
}

impl Classifier for TreeModel {
    fn n_classes(&self) -> usize {
        self.n_classes
    }

    fn predict_proba(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        check_width(x, self.n_features)?;
        Ok((0..x.rows())
            .map(|i| {
                let row = x.row(i);
                let mut at = 0;
                loop {
                    match &self.nodes[at] {
                        TreeNode::Leaf { proba } => break proba.clone(),
                        TreeNode::Split { feature, threshold, left, right } => {
                            at = if row[*feature] <= *threshold { *left } else { *right };
                        }
                    }
                }
            })
            .collect())
    }
}

struct BestSplit {
    gain: f64,
    feature: usize,
    threshold: f64,
    left_impurity: f64,
    right_impurity: f64,
    n_left: usize,
}

impl DecisionTree {
    fn best_split(&self, x: &Matrix, y: &[usize], idx: &[usize], k: usize, parent: f64) -> Option<BestSplit> {
        let n = idx.len();
        let mut total = vec![0usize; k];
        for &i in idx {
            total[y[i]] += 1;
        }
        let mut best: Option<BestSplit> = None;
        let mut order = idx.to_vec();
        for f in 0..x.cols() {
            order.sort_by(|&a, &b| x.get(a, f).total_cmp(&x.get(b, f)).then(a.cmp(&b)));
            let mut left = vec![0usize; k];
            let mut right = total.clone();
            for pos in 0..n - 1 {
                let i = order[pos];
                left[y[i]] += 1;
                right[y[i]] -= 1;
                let v = x.get(i, f);
                let v_next = x.get(order[pos + 1], f);
                // Equal values cannot be separated by a threshold.
                if v_next <= v {
                    continue;
                }
                let n_left = pos + 1;
                let n_right = n - n_left;
                if n_left < self.min_samples_leaf || n_right < self.min_samples_leaf {
                    continue;
                }
                let il = self.criterion.impurity(&left, n_left);
                let ir = self.criterion.impurity(&right, n_right);
                let child = (n_left as f64 * il + n_right as f64 * ir) / n as f64;
                let gain = parent - child;
                // The first valid split is accepted even at zero gain, as
                // scikit-learn does: balanced XOR has NO single split that
                // lowers impurity, yet splitting is the only way to reach the
                // pair that does. Later candidates must strictly improve.
                if gain > best.as_ref().map_or(-1e-12, |b| b.gain + 1e-12) {
                    // Midpoint, unless rounding put it on the upper value, in
                    // which case the lower value itself separates the two.
                    let mid = v + (v_next - v) / 2.0;
                    let threshold = if mid < v_next { mid } else { v };
                    best = Some(BestSplit {
                        gain,
                        feature: f,
                        threshold,
                        left_impurity: il,
                        right_impurity: ir,
                        n_left,
                    });
                }
            }
        }
        best
    }

    /// Fit and return the concrete tree (importances accessible).
    pub fn fit_model(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<TreeModel> {
        check_training(x, y, n_classes)?;
        if self.min_samples_leaf == 0 {
            return Err(DataError::Schema("min_samples_leaf must be at least 1".into()));
        }
        let k = n_classes;
        let mut nodes = vec![TreeNode::Leaf { proba: vec![] }];
        let mut importances = vec![0.0; x.cols()];
        let (mut depth_seen, mut leaves) = (0usize, 0usize);
        // (rows at this node, depth, node slot, impurity if already known)
        let mut work: Vec<(Vec<usize>, usize, usize, Option<f64>)> =
            vec![((0..x.rows()).collect(), 0, 0, None)];
        while let Some((idx, depth, at, known)) = work.pop() {
            depth_seen = depth_seen.max(depth);
            let mut counts = vec![0usize; k];
            for &i in &idx {
                counts[y[i]] += 1;
            }
            let impurity = known.unwrap_or_else(|| self.criterion.impurity(&counts, idx.len()));
            // A pure node never splits; an impure one splits whenever any
            // threshold separates its rows, even at zero immediate gain.
            let splittable = idx.len() >= self.min_samples_split.max(2)
                && impurity > 1e-12
                && self.max_depth.is_none_or(|m| depth < m);
            if splittable {
                if let Some(b) = self.best_split(x, y, &idx, k, impurity) {
                    let n = idx.len() as f64;
                    let n_left = b.n_left as f64;
                    importances[b.feature] += n * impurity
                        - n_left * b.left_impurity
                        - (n - n_left) * b.right_impurity;
                    let (l_idx, r_idx): (Vec<usize>, Vec<usize>) =
                        idx.iter().copied().partition(|&i| x.get(i, b.feature) <= b.threshold);
                    let left = nodes.len();
                    nodes.push(TreeNode::Leaf { proba: vec![] });
                    let right = nodes.len();
                    nodes.push(TreeNode::Leaf { proba: vec![] });
                    nodes[at] = TreeNode::Split { feature: b.feature, threshold: b.threshold, left, right };
                    work.push((r_idx, depth + 1, right, Some(b.right_impurity)));
                    work.push((l_idx, depth + 1, left, Some(b.left_impurity)));
                    continue;
                }
            }
            let nf = idx.len() as f64;
            nodes[at] = TreeNode::Leaf { proba: counts.iter().map(|&c| c as f64 / nf).collect() };
            leaves += 1;
        }
        let total: f64 = importances.iter().sum();
        if total > 0.0 {
            for v in &mut importances {
                *v /= total;
            }
        }
        Ok(TreeModel {
            nodes,
            n_classes: k,
            n_features: x.cols(),
            importances,
            depth: depth_seen,
            leaves,
        })
    }
}

impl ClassifierSpec for DecisionTree {
    fn name(&self) -> &'static str {
        "decision_tree"
    }

    fn describe(&self) -> String {
        let depth = self.max_depth.map_or_else(|| "none".to_string(), |d| d.to_string());
        format!(
            "decision_tree(criterion={:?}, max_depth={depth}, min_samples_split={}, min_samples_leaf={})",
            self.criterion, self.min_samples_split, self.min_samples_leaf
        )
    }

    fn fit(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<Box<dyn Classifier>> {
        Ok(Box::new(self.fit_model(x, y, n_classes)?))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// k-nearest neighbours
// ─────────────────────────────────────────────────────────────────────────────

/// k-nearest neighbours on standardised features (brute force).
///
/// Standardising first means a feature measured in millimetres cannot drown
/// out one measured in kilometres. Neighbour ties go to the lower row index.
#[derive(Clone, Debug, PartialEq)]
pub struct KNearestNeighbors {
    pub k: usize,
    /// Weight votes by inverse distance instead of counting them equally.
    pub distance_weighted: bool,
}

impl Default for KNearestNeighbors {
    fn default() -> Self {
        Self { k: 5, distance_weighted: false }
    }
}

/// A fitted k-NN classifier: it stores the standardised training set.
#[derive(Clone, Debug, PartialEq)]
pub struct KnnModel {
    scaler: Scaler,
    train: Matrix,
    y: Vec<usize>,
    k: usize,
    distance_weighted: bool,
    n_classes: usize,
}

impl Classifier for KnnModel {
    fn n_classes(&self) -> usize {
        self.n_classes
    }

    fn predict_proba(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        check_width(x, self.train.cols())?;
        let z = self.scaler.transform(x)?;
        let k = self.k.min(self.train.rows());
        let mut out = Vec::with_capacity(z.rows());
        // The k best (distance², index) so far, kept sorted; k is small, so an
        // insertion into a short vector beats a heap.
        let mut best: Vec<(f64, usize)> = Vec::with_capacity(k + 1);
        for q in 0..z.rows() {
            best.clear();
            let qrow = z.row(q);
            for t in 0..self.train.rows() {
                let d = dist2(qrow, self.train.row(t));
                if best.len() == k && (d, t) >= best[k - 1] {
                    continue;
                }
                let pos = best.partition_point(|&e| e < (d, t));
                best.insert(pos, (d, t));
                if best.len() > k {
                    best.pop();
                }
            }
            let mut votes = vec![0.0; self.n_classes];
            if self.distance_weighted {
                // An exact match outvotes everything: only zero-distance
                // neighbours count when any exist.
                let exact: Vec<&(f64, usize)> = best.iter().filter(|e| e.0 == 0.0).collect();
                if exact.is_empty() {
                    for &(d, t) in &best {
                        votes[self.y[t]] += 1.0 / d.sqrt();
                    }
                } else {
                    for &&(_, t) in &exact {
                        votes[self.y[t]] += 1.0;
                    }
                }
            } else {
                for &(_, t) in &best {
                    votes[self.y[t]] += 1.0;
                }
            }
            let total: f64 = votes.iter().sum();
            out.push(votes.into_iter().map(|v| v / total).collect());
        }
        Ok(out)
    }
}

impl KNearestNeighbors {
    pub fn fit_model(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<KnnModel> {
        check_training(x, y, n_classes)?;
        if self.k == 0 {
            return Err(DataError::Schema("k must be at least 1".into()));
        }
        let scaler = Scaler::fit(x)?;
        Ok(KnnModel {
            train: scaler.transform(x)?,
            scaler,
            y: y.to_vec(),
            k: self.k,
            distance_weighted: self.distance_weighted,
            n_classes,
        })
    }
}

impl ClassifierSpec for KNearestNeighbors {
    fn name(&self) -> &'static str {
        "k_nearest_neighbors"
    }

    fn describe(&self) -> String {
        format!("k_nearest_neighbors(k={}, distance_weighted={})", self.k, self.distance_weighted)
    }

    fn fit(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<Box<dyn Classifier>> {
        Ok(Box::new(self.fit_model(x, y, n_classes)?))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Gaussian naive Bayes
// ─────────────────────────────────────────────────────────────────────────────

/// Gaussian naive Bayes: each feature normal within each class, features
/// independent given the class.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianNaiveBayes {
    /// Added to every variance as this fraction of the largest feature
    /// variance, so a feature constant within one class does not produce an
    /// infinite likelihood.
    pub var_smoothing: f64,
}

impl Default for GaussianNaiveBayes {
    fn default() -> Self {
        Self { var_smoothing: 1e-9 }
    }
}

/// A fitted Gaussian naive Bayes model.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NaiveBayesModel {
    pub n_classes: usize,
    pub n_features: usize,
    /// Log prior per class; negative infinity for a class absent from training.
    pub log_prior: Vec<f64>,
    pub mean: Vec<Vec<f64>>,
    pub var: Vec<Vec<f64>>,
}

impl Classifier for NaiveBayesModel {
    fn n_classes(&self) -> usize {
        self.n_classes
    }

    fn predict_proba(&self, x: &Matrix) -> Result<Vec<Vec<f64>>> {
        check_width(x, self.n_features)?;
        let two_pi = 2.0 * std::f64::consts::PI;
        Ok((0..x.rows())
            .map(|i| {
                let row = x.row(i);
                let mut jll: Vec<f64> = (0..self.n_classes)
                    .map(|c| {
                        if !self.log_prior[c].is_finite() {
                            return f64::NEG_INFINITY;
                        }
                        let mut s = self.log_prior[c];
                        for j in 0..self.n_features {
                            let v = self.var[c][j];
                            let d = row[j] - self.mean[c][j];
                            s -= 0.5 * (two_pi * v).ln() + d * d / (2.0 * v);
                        }
                        s
                    })
                    .collect();
                softmax_in_place(&mut jll);
                jll
            })
            .collect())
    }
}

impl GaussianNaiveBayes {
    pub fn fit_model(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<NaiveBayesModel> {
        check_training(x, y, n_classes)?;
        let (n, d, k) = (x.rows(), x.cols(), n_classes);
        let mut count = vec![0usize; k];
        let mut mean = vec![vec![0.0; d]; k];
        for i in 0..n {
            count[y[i]] += 1;
            for (m, v) in mean[y[i]].iter_mut().zip(x.row(i)) {
                *m += v;
            }
        }
        for c in 0..k {
            if count[c] > 0 {
                for m in &mut mean[c] {
                    *m /= count[c] as f64;
                }
            }
        }
        let mut var = vec![vec![0.0; d]; k];
        for i in 0..n {
            let c = y[i];
            for j in 0..d {
                let e = x.get(i, j) - mean[c][j];
                var[c][j] += e * e;
            }
        }
        // Epsilon from the largest overall feature variance, floored so an
        // all-constant dataset still yields finite likelihoods.
        let overall = Scaler::fit(x)?;
        let max_var = overall.std.iter().map(|s| s * s).fold(0.0, f64::max);
        let eps = (self.var_smoothing * max_var).max(1e-12);
        for c in 0..k {
            for v in &mut var[c] {
                *v = if count[c] > 0 { *v / count[c] as f64 } else { 0.0 } + eps;
            }
        }
        let log_prior = count
            .iter()
            .map(|&c| if c == 0 { f64::NEG_INFINITY } else { (c as f64 / n as f64).ln() })
            .collect();
        Ok(NaiveBayesModel { n_classes: k, n_features: d, log_prior, mean, var })
    }
}

impl ClassifierSpec for GaussianNaiveBayes {
    fn name(&self) -> &'static str {
        "gaussian_naive_bayes"
    }

    fn describe(&self) -> String {
        format!("gaussian_naive_bayes(var_smoothing={})", self.var_smoothing)
    }

    fn fit(&self, x: &Matrix, y: &[usize], n_classes: usize) -> Result<Box<dyn Classifier>> {
        Ok(Box::new(self.fit_model(x, y, n_classes)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mine::eval::Confusion;

    /// Two well-separated clusters in 2-D, 20 points each.
    fn blobs() -> (Matrix, Vec<usize>) {
        let mut rows = Vec::new();
        let mut y = Vec::new();
        for i in 0..20 {
            let t = i as f64 * 0.1;
            rows.push(vec![t, 1.0 - t]);
            y.push(0);
            rows.push(vec![5.0 + t, 6.0 - t]);
            y.push(1);
        }
        (Matrix::from_rows(&rows).unwrap(), y)
    }

    /// XOR on exact corners, each twice: no straight line separates it, a
    /// depth-2 tree does. Exact corners matter: with jittered points the
    /// greedy splitter meets unequal-gain thresholds and walks into a deeper
    /// tree than the minimal one.
    fn xor() -> (Matrix, Vec<usize>) {
        let corners = [(0.0, 0.0, 0), (0.0, 1.0, 1), (1.0, 0.0, 1), (1.0, 1.0, 0)];
        let mut rows = Vec::new();
        let mut y = Vec::new();
        for _ in 0..2 {
            for &(a, b, c) in &corners {
                rows.push(vec![a, b]);
                y.push(c);
            }
        }
        (Matrix::from_rows(&rows).unwrap(), y)
    }

    fn accuracy(m: &dyn Classifier, x: &Matrix, y: &[usize]) -> f64 {
        Confusion::new(y, &m.predict(x).unwrap(), m.n_classes()).unwrap().accuracy()
    }

    fn rows_sum_to_one(p: &[Vec<f64>]) -> bool {
        p.iter().all(|r| (r.iter().sum::<f64>() - 1.0).abs() < 1e-9)
    }

    #[test]
    fn every_classifier_separates_two_clear_blobs() {
        let (x, y) = blobs();
        let specs: Vec<Box<dyn ClassifierSpec>> = vec![
            Box::new(LogisticRegression::default()),
            Box::new(DecisionTree::default()),
            Box::new(KNearestNeighbors::default()),
            Box::new(GaussianNaiveBayes::default()),
        ];
        for spec in specs {
            let m = spec.fit(&x, &y, 2).unwrap();
            assert_eq!(accuracy(m.as_ref(), &x, &y), 1.0, "{}", spec.name());
            assert!(rows_sum_to_one(&m.predict_proba(&x).unwrap()), "{}", spec.name());
        }
    }

    #[test]
    fn a_tree_learns_xor_and_a_linear_model_cannot() {
        let (x, y) = xor();
        let tree = DecisionTree::default().fit_model(&x, &y, 2).unwrap();
        assert_eq!(accuracy(&tree, &x, &y), 1.0);
        assert_eq!(tree.depth, 2, "XOR needs exactly two levels");

        // The discriminating half of the test: if the linear model also scored
        // 100% here, the data would not be exercising anything nonlinear.
        let lin = LogisticRegression::default().fit_model(&x, &y, 2).unwrap();
        assert!(accuracy(&lin, &x, &y) < 1.0);
    }

    #[test]
    fn max_depth_is_respected() {
        let (x, y) = xor();
        let stump = DecisionTree { max_depth: Some(1), ..Default::default() }.fit_model(&x, &y, 2).unwrap();
        assert_eq!(stump.depth, 1);
        assert!(stump.leaves <= 2);
    }

    #[test]
    fn tree_importances_sum_to_one_and_find_the_only_informative_feature() {
        // Feature 0 decides the class; feature 1 is constant noise.
        let rows: Vec<Vec<f64>> = (0..10).map(|i| vec![i as f64, 3.0]).collect();
        let y: Vec<usize> = (0..10).map(|i| usize::from(i >= 5)).collect();
        let t = DecisionTree::default().fit_model(&Matrix::from_rows(&rows).unwrap(), &y, 2).unwrap();
        assert!((t.importances.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert_eq!(t.importances[0], 1.0);
        assert_eq!(t.importances[1], 0.0);
    }

    #[test]
    fn entropy_and_gini_agree_on_a_clean_split() {
        let (x, y) = blobs();
        for c in [Criterion::Gini, Criterion::Entropy] {
            let t = DecisionTree { criterion: c, ..Default::default() }.fit_model(&x, &y, 2).unwrap();
            assert_eq!(accuracy(&t, &x, &y), 1.0, "{c:?}");
            assert_eq!(t.leaves, 2, "{c:?}: one split suffices");
        }
    }

    #[test]
    fn the_same_data_grows_the_same_tree() {
        let (x, y) = xor();
        let a = DecisionTree::default().fit_model(&x, &y, 2).unwrap();
        let b = DecisionTree::default().fit_model(&x, &y, 2).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn one_nearest_neighbour_reproduces_its_training_labels() {
        let (x, y) = xor();
        let m = KNearestNeighbors { k: 1, distance_weighted: false }.fit_model(&x, &y, 2).unwrap();
        assert_eq!(accuracy(&m, &x, &y), 1.0);
    }

    #[test]
    fn distance_weighting_lets_an_exact_match_win() {
        // The query sits exactly on a class-1 point, with two class-0 points
        // near it. Unweighted 3-NN votes 0; weighted votes 1.
        let x = Matrix::from_rows(&[vec![0.0], vec![1.0], vec![1.2], vec![9.0]]).unwrap();
        let y = vec![1, 0, 0, 1];
        let q = Matrix::from_rows(&[vec![0.0]]).unwrap();
        let plain = KNearestNeighbors { k: 3, distance_weighted: false }.fit_model(&x, &y, 2).unwrap();
        let weighted = KNearestNeighbors { k: 3, distance_weighted: true }.fit_model(&x, &y, 2).unwrap();
        assert_eq!(plain.predict(&q).unwrap(), vec![0]);
        assert_eq!(weighted.predict(&q).unwrap(), vec![1]);
    }

    #[test]
    fn logistic_regression_handles_three_classes() {
        let mut rows = Vec::new();
        let mut y = Vec::new();
        for (c, (cx, cy)) in [(0.0, 0.0), (5.0, 0.0), (0.0, 5.0)].into_iter().enumerate() {
            for i in 0..10 {
                let t = i as f64 * 0.05;
                rows.push(vec![cx + t, cy - t]);
                y.push(c);
            }
        }
        let x = Matrix::from_rows(&rows).unwrap();
        let m = LogisticRegression::default().fit_model(&x, &y, 3).unwrap();
        assert_eq!(accuracy(&m, &x, &y), 1.0);
        assert!(rows_sum_to_one(&m.predict_proba(&x).unwrap()));
    }

    #[test]
    fn a_wider_input_than_the_model_was_fitted_on_is_refused() {
        let (x, y) = blobs();
        let m = GaussianNaiveBayes::default().fit_model(&x, &y, 2).unwrap();
        let wide = Matrix::from_rows(&[vec![0.0, 0.0, 0.0]]).unwrap();
        assert!(m.predict(&wide).is_err());
    }

    #[test]
    fn a_class_absent_from_training_is_never_predicted() {
        // n_classes = 3 but class 2 never appears.
        let (x, y) = blobs();
        let m = GaussianNaiveBayes::default().fit_model(&x, &y, 3).unwrap();
        let p = m.predict_proba(&x).unwrap();
        assert!(p.iter().all(|r| r[2] == 0.0));
        assert!(rows_sum_to_one(&p));
    }

    #[test]
    fn labels_outside_the_class_range_are_refused() {
        let (x, _) = blobs();
        let bad = vec![5; x.rows()];
        assert!(DecisionTree::default().fit_model(&x, &bad, 2).is_err());
    }
}
