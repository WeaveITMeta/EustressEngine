//! Data mining: classification, regression, clustering, dimensionality
//! reduction, feature selection, association rules, and the evaluation that
//! keeps every one of them honest.
//!
//! Pure `std`, always compiled, no dependencies. This is the Data Platform's
//! answer to "data mining in Rust without scikit-learn": the six canonical
//! mining tasks (anomaly detection lives in [`crate::ml`], summarization in
//! [`crate::numerics`]) plus the evaluation layer the field treats as
//! non-negotiable, because a pattern that has not been validated on data it
//! was not fitted to is not yet a finding.
//!
//! ## Shared decisions
//!
//! - **Reproducible by construction.** Anything random (splits, folds,
//!   permutations) takes an explicit `seed` and draws from [`Rng`], so the same
//!   inputs give the same result on every machine. An experiment that cannot be
//!   re-run exactly cannot be audited, and auditability is the point of the
//!   platform.
//! - **Nulls are dropped, never invented, for supervised work.** [`features`]
//!   and friends remove any row with a missing or non-finite value in a
//!   selected column and report how many they removed. Imputing a label would
//!   manufacture training data.
//! - **Dense row-major [`Matrix`].** Contiguous storage keeps the inner loops
//!   cache-friendly; [`Matrix::from_rows`] interoperates with the
//!   `&[Vec<f64>]` shape used by [`crate::ml`].
//! - **Specs fit, models predict.** A `*Spec` holds hyperparameters and fits;
//!   the fitted model it returns only predicts. There is no half-trained state,
//!   and cross-validation can fit one spec many times.

/// The JSON front door: one request in, one report out. Needs `import` for
/// file reading and JSON.
#[cfg(feature = "import")]
pub mod api;
pub mod assoc;
pub mod classify;
pub mod cluster;
pub mod eval;
pub(crate) mod linalg;
pub mod reduce;
pub mod regress;
pub mod select;
pub mod workflow;

use crate::numerics::as_f64_opt;
use crate::{ColumnData, DataError, Frame, Result};

// ─────────────────────────────────────────────────────────────────────────────
// Matrix
// ─────────────────────────────────────────────────────────────────────────────

/// Dense row-major matrix of `f64`.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f64>,
}

impl Matrix {
    /// Build from row-major data. Errors when `data.len() != rows * cols`.
    pub fn new(rows: usize, cols: usize, data: Vec<f64>) -> Result<Self> {
        if data.len() != rows * cols {
            return Err(DataError::Schema(format!(
                "matrix: {} values do not fill {rows} x {cols}",
                data.len()
            )));
        }
        Ok(Self { rows, cols, data })
    }

    /// A `rows` x `cols` matrix of zeros.
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self { rows, cols, data: vec![0.0; rows * cols] }
    }

    /// Build from a slice of equal-length rows. Errors on ragged input.
    pub fn from_rows(rows: &[Vec<f64>]) -> Result<Self> {
        let cols = rows.first().map_or(0, Vec::len);
        let mut data = Vec::with_capacity(rows.len() * cols);
        for (i, r) in rows.iter().enumerate() {
            if r.len() != cols {
                return Err(DataError::Schema(format!(
                    "matrix: row {i} has {} values, expected {cols}",
                    r.len()
                )));
            }
            data.extend_from_slice(r);
        }
        Ok(Self { rows: rows.len(), cols, data })
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Row `i` as a slice.
    pub fn row(&self, i: usize) -> &[f64] {
        &self.data[i * self.cols..(i + 1) * self.cols]
    }

    /// Element `(i, j)`.
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.data[i * self.cols + j]
    }

    pub(crate) fn set(&mut self, i: usize, j: usize, v: f64) {
        self.data[i * self.cols + j] = v;
    }

    /// Column `j`, copied.
    pub fn column(&self, j: usize) -> Vec<f64> {
        (0..self.rows).map(|i| self.get(i, j)).collect()
    }

    /// The rows at `idx`, in that order.
    pub fn select_rows(&self, idx: &[usize]) -> Matrix {
        let mut data = Vec::with_capacity(idx.len() * self.cols);
        for &i in idx {
            data.extend_from_slice(self.row(i));
        }
        Matrix { rows: idx.len(), cols: self.cols, data }
    }

    /// The columns at `idx`, in that order.
    pub fn select_cols(&self, idx: &[usize]) -> Matrix {
        let mut data = Vec::with_capacity(self.rows * idx.len());
        for i in 0..self.rows {
            let r = self.row(i);
            for &j in idx {
                data.push(r[j]);
            }
        }
        Matrix { rows: self.rows, cols: idx.len(), data }
    }

    /// The flat row-major storage.
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }

    /// Rows as owned vectors, for callers built on `&[Vec<f64>]`.
    pub fn to_rows(&self) -> Vec<Vec<f64>> {
        (0..self.rows).map(|i| self.row(i).to_vec()).collect()
    }
}

/// Squared Euclidean distance.
pub(crate) fn dist2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Index of the largest value; the lowest index wins a tie, so the result is
/// deterministic.
pub(crate) fn argmax(v: &[f64]) -> usize {
    let mut best = 0;
    for (i, &x) in v.iter().enumerate() {
        if x > v[best] {
            best = i;
        }
    }
    best
}

// ─────────────────────────────────────────────────────────────────────────────
// Deterministic randomness
// ─────────────────────────────────────────────────────────────────────────────

/// SplitMix64: a small, fast, well-distributed generator with a 64-bit state.
///
/// Chosen over an external crate because the only requirement is that a seed
/// reproduces the same sequence everywhere, forever. A dependency could change
/// its output between versions and silently change every recorded experiment.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`, from the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `0..n` by rejection, so no value is favoured. `n` must be > 0.
    pub fn below(&mut self, n: usize) -> usize {
        let n = n as u64;
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let v = self.next_u64();
            if v < zone {
                return (v % n) as usize;
            }
        }
    }

    /// Fisher-Yates shuffle in place.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scaling
// ─────────────────────────────────────────────────────────────────────────────

/// Z-score scaler, fitted on training rows and applied to any rows.
///
/// Fitting on the training split only is what keeps a test score honest: a
/// scaler that has seen the test rows has leaked information about them into
/// the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Scaler {
    pub mean: Vec<f64>,
    /// Population standard deviation; a constant column gets 1 so it maps to 0
    /// rather than dividing by zero.
    pub std: Vec<f64>,
}

impl Scaler {
    pub fn fit(x: &Matrix) -> Result<Self> {
        if x.is_empty() {
            return Err(DataError::Schema("scaler: cannot fit on zero rows".into()));
        }
        let n = x.rows() as f64;
        let mut mean = vec![0.0; x.cols()];
        for i in 0..x.rows() {
            for (m, v) in mean.iter_mut().zip(x.row(i)) {
                *m += v;
            }
        }
        for m in &mut mean {
            *m /= n;
        }
        let mut var = vec![0.0; x.cols()];
        for i in 0..x.rows() {
            for ((s, v), m) in var.iter_mut().zip(x.row(i)).zip(&mean) {
                *s += (v - m) * (v - m);
            }
        }
        let std = var
            .into_iter()
            .map(|s| {
                let sd = (s / n).sqrt();
                if sd < 1e-12 { 1.0 } else { sd }
            })
            .collect();
        Ok(Self { mean, std })
    }

    pub fn transform(&self, x: &Matrix) -> Result<Matrix> {
        if x.cols() != self.mean.len() {
            return Err(DataError::Schema(format!(
                "scaler: fitted on {} columns, given {}",
                self.mean.len(),
                x.cols()
            )));
        }
        let mut out = x.clone();
        for i in 0..out.rows() {
            for j in 0..out.cols() {
                let v = (out.get(i, j) - self.mean[j]) / self.std[j];
                out.set(i, j, v);
            }
        }
        Ok(out)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Labels
// ─────────────────────────────────────────────────────────────────────────────

/// Class labels encoded as indices `0..n_classes`, keeping the original values.
///
/// Classes are sorted, so the same data always encodes the same way and a
/// confusion matrix reads in a stable order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    pub y: Vec<usize>,
    pub classes: Vec<String>,
}

impl Labels {
    /// Encode string labels.
    pub fn encode<S: AsRef<str>>(values: &[S]) -> Self {
        let mut classes: Vec<String> = values.iter().map(|s| s.as_ref().to_string()).collect();
        classes.sort();
        classes.dedup();
        let y = values
            .iter()
            .map(|s| classes.binary_search_by(|c| c.as_str().cmp(s.as_ref())).unwrap_or(0))
            .collect();
        Self { y, classes }
    }

    pub fn n_classes(&self) -> usize {
        self.classes.len()
    }

    /// The original value of class index `i`.
    pub fn decode(&self, i: usize) -> Option<&str> {
        self.classes.get(i).map(String::as_str)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame extraction
// ─────────────────────────────────────────────────────────────────────────────

/// Features pulled from a [`Frame`], with the rows that survived null removal.
#[derive(Clone, Debug, PartialEq)]
pub struct Extracted {
    pub x: Matrix,
    pub feature_names: Vec<String>,
    /// Original frame row index of each matrix row, so results can be written
    /// back aligned to the frame.
    pub kept_rows: Vec<usize>,
    /// Rows removed for a missing or non-finite value.
    pub dropped: usize,
}

/// Any cell rendered as text, or `None` when missing or non-finite. Used where a
/// value names something (an entity, an item) rather than measures it.
pub(crate) fn cell_text(col: &ColumnData, r: usize) -> Option<String> {
    match col {
        ColumnData::Str(v) => v.get(r).cloned().flatten(),
        ColumnData::I64(v) => v.get(r).copied().flatten().map(|x| x.to_string()),
        ColumnData::F64(v) => v.get(r).copied().flatten().filter(|x| x.is_finite()).map(|x| x.to_string()),
        ColumnData::Bool(v) => v.get(r).copied().flatten().map(|x| x.to_string()),
    }
}

/// A cell of a label column rendered as a class name, or `None` when missing.
fn label_cell(col: &ColumnData, r: usize) -> Result<Option<String>> {
    Ok(match col {
        ColumnData::Str(v) => v.get(r).cloned().flatten(),
        ColumnData::I64(v) => v.get(r).copied().flatten().map(|x| x.to_string()),
        ColumnData::Bool(v) => v.get(r).copied().flatten().map(|x| x.to_string()),
        ColumnData::F64(v) => match v.get(r).copied().flatten() {
            None => None,
            Some(x) if !x.is_finite() => None,
            // Class codes stored as floats (0.0, 1.0) are common; a fractional
            // value is a measurement, not a class, and must not be bucketed.
            Some(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => Some(format!("{}", x as i64)),
            Some(x) => {
                return Err(DataError::Schema(format!(
                    "label column holds the non-integral value {x}; use regression for a continuous target"
                )))
            }
        },
    })
}

fn numeric_columns(frame: &Frame, cols: &[&str]) -> Result<Vec<Vec<Option<f64>>>> {
    if cols.is_empty() {
        return Err(DataError::Schema("no feature columns selected".into()));
    }
    let mut out = Vec::with_capacity(cols.len());
    for &name in cols {
        let col = frame
            .column(name)
            .ok_or_else(|| DataError::Schema(format!("no column `{name}`")))?;
        out.push(as_f64_opt(col)?);
    }
    Ok(out)
}

/// Build the matrix from the rows where every column is present and finite.
/// `extra_ok` lets a caller veto a row for its own target column too.
fn assemble(
    cols: &[&str],
    data: &[Vec<Option<f64>>],
    n: usize,
    mut extra_ok: impl FnMut(usize) -> bool,
) -> Extracted {
    let mut kept = Vec::with_capacity(n);
    let mut flat = Vec::with_capacity(n * cols.len());
    'rows: for r in 0..n {
        for c in data {
            match c.get(r).copied().flatten() {
                Some(v) if v.is_finite() => {}
                _ => continue 'rows,
            }
        }
        if !extra_ok(r) {
            continue;
        }
        for c in data {
            flat.push(c[r].unwrap_or(0.0));
        }
        kept.push(r);
    }
    let rows = kept.len();
    Extracted {
        x: Matrix { rows, cols: cols.len(), data: flat },
        feature_names: cols.iter().map(|s| s.to_string()).collect(),
        dropped: n - rows,
        kept_rows: kept,
    }
}

/// Numeric features from the named columns, dropping incomplete rows.
pub fn features(frame: &Frame, cols: &[&str]) -> Result<Extracted> {
    let data = numeric_columns(frame, cols)?;
    let ex = assemble(cols, &data, frame.n_rows(), |_| true);
    if ex.x.is_empty() {
        return Err(DataError::Schema("every row has a missing feature value".into()));
    }
    Ok(ex)
}

/// Features plus a numeric target, for regression.
pub fn features_and_target(frame: &Frame, cols: &[&str], target: &str) -> Result<(Extracted, Vec<f64>)> {
    if cols.contains(&target) {
        return Err(DataError::Schema(format!(
            "`{target}` is both a feature and the target; a model given its own answer learns nothing"
        )));
    }
    let data = numeric_columns(frame, cols)?;
    let t = as_f64_opt(
        frame
            .column(target)
            .ok_or_else(|| DataError::Schema(format!("no target column `{target}`")))?,
    )?;
    let ex = assemble(cols, &data, frame.n_rows(), |r| {
        matches!(t.get(r).copied().flatten(), Some(v) if v.is_finite())
    });
    if ex.x.is_empty() {
        return Err(DataError::Schema("no row has every feature and the target present".into()));
    }
    let y = ex.kept_rows.iter().map(|&r| t[r].unwrap_or(0.0)).collect();
    Ok((ex, y))
}

/// Features plus a categorical target, for classification.
pub fn features_and_labels(frame: &Frame, cols: &[&str], target: &str) -> Result<(Extracted, Labels)> {
    if cols.contains(&target) {
        return Err(DataError::Schema(format!(
            "`{target}` is both a feature and the target; a model given its own answer learns nothing"
        )));
    }
    let data = numeric_columns(frame, cols)?;
    let tcol = frame
        .column(target)
        .ok_or_else(|| DataError::Schema(format!("no target column `{target}`")))?;
    let mut names = vec![None; frame.n_rows()];
    for (r, slot) in names.iter_mut().enumerate() {
        *slot = label_cell(tcol, r)?;
    }
    let ex = assemble(cols, &data, frame.n_rows(), |r| names[r].is_some());
    if ex.x.is_empty() {
        return Err(DataError::Schema("no row has every feature and a label present".into()));
    }
    let kept: Vec<String> = ex.kept_rows.iter().filter_map(|&r| names[r].clone()).collect();
    let labels = Labels::encode(&kept);
    if labels.n_classes() < 2 {
        return Err(DataError::Schema(format!(
            "target `{target}` has {} class after dropping missing rows; classification needs at least 2",
            labels.n_classes()
        )));
    }
    Ok((ex, labels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frame_from_columns, ColumnDtype, ColumnSpec};

    #[test]
    fn matrix_rejects_a_shape_its_data_cannot_fill() {
        assert!(Matrix::new(2, 3, vec![0.0; 5]).is_err());
        assert!(Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0]]).is_err());
        let m = Matrix::from_rows(&[vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert_eq!(m.row(1), &[3.0, 4.0]);
        assert_eq!(m.column(0), vec![1.0, 3.0]);
        assert_eq!(m.select_rows(&[1, 0]).row(0), &[3.0, 4.0]);
        assert_eq!(m.select_cols(&[1]).as_slice(), &[2.0, 4.0]);
    }

    #[test]
    fn the_same_seed_reproduces_the_same_sequence() {
        let a: Vec<u64> = { let mut r = Rng::new(42); (0..8).map(|_| r.next_u64()).collect() };
        let b: Vec<u64> = { let mut r = Rng::new(42); (0..8).map(|_| r.next_u64()).collect() };
        let c: Vec<u64> = { let mut r = Rng::new(43); (0..8).map(|_| r.next_u64()).collect() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn a_shuffle_is_a_permutation_and_actually_moves_things() {
        let mut v: Vec<usize> = (0..100).collect();
        Rng::new(7).shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>(), "nothing lost or duplicated");
        assert_ne!(v, (0..100).collect::<Vec<_>>(), "and the order changed");
    }

    #[test]
    fn next_f64_stays_in_the_unit_interval() {
        let mut r = Rng::new(1);
        for _ in 0..10_000 {
            let x = r.next_f64();
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn scaled_training_columns_have_zero_mean_and_unit_spread() {
        let x = Matrix::from_rows(&[vec![1.0, 10.0], vec![2.0, 10.0], vec![3.0, 10.0]]).unwrap();
        let s = Scaler::fit(&x).unwrap();
        let z = s.transform(&x).unwrap();
        let c0 = z.column(0);
        assert!(c0.iter().sum::<f64>().abs() < 1e-12);
        let var = c0.iter().map(|v| v * v).sum::<f64>() / 3.0;
        assert!((var - 1.0).abs() < 1e-12);
        // A constant column maps to zero instead of dividing by zero.
        assert!(z.column(1).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn labels_encode_in_sorted_order_and_decode_back() {
        let l = Labels::encode(&["cat", "dog", "cat", "bird"]);
        assert_eq!(l.classes, vec!["bird", "cat", "dog"]);
        assert_eq!(l.y, vec![1, 2, 1, 0]);
        assert_eq!(l.decode(2), Some("dog"));
    }

    fn frame() -> Frame {
        frame_from_columns(vec![
            (
                ColumnSpec::new("a", ColumnDtype::F64),
                ColumnData::F64(vec![Some(1.0), None, Some(3.0), Some(4.0)]),
            ),
            (
                ColumnSpec::new("b", ColumnDtype::I64),
                ColumnData::I64(vec![Some(10), Some(20), Some(30), Some(40)]),
            ),
            (
                ColumnSpec::new("kind", ColumnDtype::Str),
                ColumnData::Str(vec![Some("x".into()), Some("y".into()), None, Some("y".into())]),
            ),
        ])
        .unwrap()
    }

    #[test]
    fn incomplete_rows_are_dropped_and_counted_never_filled() {
        let ex = features(&frame(), &["a", "b"]).unwrap();
        assert_eq!(ex.kept_rows, vec![0, 2, 3]);
        assert_eq!(ex.dropped, 1);
        assert_eq!(ex.x.row(1), &[3.0, 30.0]);
    }

    #[test]
    fn a_missing_label_drops_its_row_too() {
        let (ex, labels) = features_and_labels(&frame(), &["a", "b"], "kind").unwrap();
        // Row 1 lacks `a`, row 2 lacks a label.
        assert_eq!(ex.kept_rows, vec![0, 3]);
        assert_eq!(labels.classes, vec!["x", "y"]);
        assert_eq!(labels.y, vec![0, 1]);
    }

    #[test]
    fn the_target_cannot_also_be_a_feature() {
        assert!(features_and_target(&frame(), &["a", "b"], "b").is_err());
        assert!(features_and_labels(&frame(), &["a", "kind"], "kind").is_err());
    }

    #[test]
    fn a_fractional_float_is_refused_as_a_class() {
        let f = frame_from_columns(vec![
            (ColumnSpec::new("a", ColumnDtype::F64), ColumnData::F64(vec![Some(1.0), Some(2.0)])),
            (ColumnSpec::new("t", ColumnDtype::F64), ColumnData::F64(vec![Some(0.5), Some(1.0)])),
        ])
        .unwrap();
        assert!(features_and_labels(&f, &["a"], "t").is_err());
    }

    #[test]
    fn integral_floats_are_accepted_as_class_codes() {
        let f = frame_from_columns(vec![
            (ColumnSpec::new("a", ColumnDtype::F64), ColumnData::F64(vec![Some(1.0), Some(2.0)])),
            (ColumnSpec::new("t", ColumnDtype::F64), ColumnData::F64(vec![Some(0.0), Some(1.0)])),
        ])
        .unwrap();
        let (_, labels) = features_and_labels(&f, &["a"], "t").unwrap();
        assert_eq!(labels.classes, vec!["0", "1"]);
    }

    #[test]
    fn a_single_class_target_is_refused() {
        let f = frame_from_columns(vec![
            (ColumnSpec::new("a", ColumnDtype::F64), ColumnData::F64(vec![Some(1.0), Some(2.0)])),
            (
                ColumnSpec::new("t", ColumnDtype::Str),
                ColumnData::Str(vec![Some("same".into()), Some("same".into())]),
            ),
        ])
        .unwrap();
        assert!(features_and_labels(&f, &["a"], "t").is_err());
    }
}
