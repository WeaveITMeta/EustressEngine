//! Dimensionality reduction: principal component analysis.
//!
//! PCA finds the orthogonal directions along which the data vary most. It is
//! feature EXTRACTION rather than selection: each component is a new feature
//! built from all the old ones, so it compresses without discarding any input
//! outright. The explained-variance ratios say how many components the data
//! really need.

use serde::Serialize;

use super::linalg::symmetric_eigen;
use super::{Matrix, Scaler};
use crate::{DataError, Result};

/// Principal component analysis.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Pca {
    /// Components to keep; `None` keeps all of them.
    pub n_components: Option<usize>,
    /// Scale each feature to unit variance first (PCA on the correlation
    /// matrix). Leave it off only when the features share a unit, or the one
    /// with the largest numbers will dominate by accident of measurement.
    pub standardize: bool,
}

/// A fitted PCA.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PcaModel {
    pub mean: Vec<f64>,
    /// 1 per feature unless `standardize` was set.
    pub scale: Vec<f64>,
    /// One row per kept component, each a unit vector over the features.
    pub components: Vec<Vec<f64>>,
    /// Variance captured by each kept component.
    pub explained_variance: Vec<f64>,
    /// Share of the TOTAL variance each kept component captures, so the sum
    /// says how much the kept components represent.
    pub explained_variance_ratio: Vec<f64>,
}

impl Pca {
    pub fn fit_model(&self, x: &Matrix) -> Result<PcaModel> {
        let (n, d) = (x.rows(), x.cols());
        if n < 2 {
            return Err(DataError::Schema("PCA needs at least 2 rows".into()));
        }
        if d == 0 {
            return Err(DataError::Schema("PCA needs at least 1 feature".into()));
        }
        let keep = self.n_components.unwrap_or(d);
        if keep == 0 || keep > d {
            return Err(DataError::Schema(format!("cannot keep {keep} of {d} components")));
        }
        let sc = Scaler::fit(x)?;
        let scale = if self.standardize { sc.std.clone() } else { vec![1.0; d] };
        // Sample covariance of the centred (and optionally scaled) data.
        let mut cov = vec![0.0; d * d];
        for i in 0..n {
            let row = x.row(i);
            for a in 0..d {
                let za = (row[a] - sc.mean[a]) / scale[a];
                for b in a..d {
                    cov[a * d + b] += za * (row[b] - sc.mean[b]) / scale[b];
                }
            }
        }
        for a in 0..d {
            for b in a..d {
                cov[a * d + b] /= (n - 1) as f64;
                cov[b * d + a] = cov[a * d + b];
            }
        }
        let (values, vectors) = symmetric_eigen(&cov, d);
        let mut order: Vec<usize> = (0..d).collect();
        order.sort_by(|&p, &q| values[q].total_cmp(&values[p]).then(p.cmp(&q)));
        // Round-off can leave a zero eigenvalue slightly negative.
        let values: Vec<f64> = values.into_iter().map(|v| v.max(0.0)).collect();
        let total: f64 = values.iter().sum();
        let mut components = Vec::with_capacity(keep);
        let mut explained_variance = Vec::with_capacity(keep);
        for &k in order.iter().take(keep) {
            let mut v: Vec<f64> = (0..d).map(|i| vectors[i * d + k]).collect();
            // An eigenvector's sign is arbitrary; fix it so the same data always
            // yields the same components: the largest-magnitude loading is
            // positive.
            let lead = v
                .iter()
                .enumerate()
                .fold(0, |best, (i, x)| if x.abs() > v[best].abs() { i } else { best });
            if v[lead] < 0.0 {
                v.iter_mut().for_each(|x| *x = -*x);
            }
            components.push(v);
            explained_variance.push(values[k]);
        }
        let explained_variance_ratio = explained_variance
            .iter()
            .map(|v| if total > 0.0 { v / total } else { 0.0 })
            .collect();
        Ok(PcaModel { mean: sc.mean, scale, components, explained_variance, explained_variance_ratio })
    }
}

impl PcaModel {
    /// Project rows onto the kept components (the component scores).
    pub fn transform(&self, x: &Matrix) -> Result<Matrix> {
        let d = self.mean.len();
        if x.cols() != d {
            return Err(DataError::Schema(format!("PCA was fitted on {d} features, given {}", x.cols())));
        }
        let k = self.components.len();
        let mut data = Vec::with_capacity(x.rows() * k);
        for i in 0..x.rows() {
            let row = x.row(i);
            for comp in &self.components {
                data.push(
                    (0..d).map(|j| (row[j] - self.mean[j]) / self.scale[j] * comp[j]).sum::<f64>(),
                );
            }
        }
        Matrix::new(x.rows(), k, data)
    }

    /// Cumulative explained-variance ratio, component by component: how many
    /// components it takes to reach, say, 95%.
    pub fn cumulative_ratio(&self) -> Vec<f64> {
        let mut acc = 0.0;
        self.explained_variance_ratio
            .iter()
            .map(|r| {
                acc += r;
                acc
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_on_a_line_need_one_component() {
        // y = 2x exactly: all the variance lies along (1, 2)/√5.
        let rows: Vec<Vec<f64>> = (0..10).map(|i| vec![i as f64, 2.0 * i as f64]).collect();
        let m = Pca::default().fit_model(&Matrix::from_rows(&rows).unwrap()).unwrap();
        assert!((m.explained_variance_ratio[0] - 1.0).abs() < 1e-10, "{m:?}");
        assert!(m.explained_variance_ratio[1].abs() < 1e-10);
        let s5 = 5f64.sqrt();
        assert!((m.components[0][0] - 1.0 / s5).abs() < 1e-10, "{m:?}");
        assert!((m.components[0][1] - 2.0 / s5).abs() < 1e-10, "{m:?}");
    }

    #[test]
    fn components_are_orthonormal_and_ordered_by_variance() {
        let rows: Vec<Vec<f64>> = (0..30)
            .map(|i| {
                let t = i as f64;
                vec![t, (t * 0.7).sin() * 3.0, (t * 1.3).cos()]
            })
            .collect();
        let m = Pca::default().fit_model(&Matrix::from_rows(&rows).unwrap()).unwrap();
        for p in 0..3 {
            for q in 0..3 {
                let dot: f64 = m.components[p].iter().zip(&m.components[q]).map(|(a, b)| a * b).sum();
                let want = if p == q { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-9, "{p},{q}: {dot}");
            }
        }
        assert!(m.explained_variance.windows(2).all(|w| w[0] >= w[1]));
        assert!((m.cumulative_ratio()[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn scores_carry_the_explained_variance() {
        let rows: Vec<Vec<f64>> = (0..20).map(|i| vec![i as f64, ((i * 3) % 7) as f64]).collect();
        let x = Matrix::from_rows(&rows).unwrap();
        let m = Pca::default().fit_model(&x).unwrap();
        let scores = m.transform(&x).unwrap();
        for k in 0..2 {
            let col = scores.column(k);
            let mean = col.iter().sum::<f64>() / 20.0;
            let var = col.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / 19.0;
            assert!(mean.abs() < 1e-9, "scores are centred");
            assert!((var - m.explained_variance[k]).abs() < 1e-9, "component {k}: {var} vs {}", m.explained_variance[k]);
        }
    }

    #[test]
    fn standardising_stops_a_large_unit_from_dominating() {
        // Two independent-looking features, one measured in huge numbers.
        let rows: Vec<Vec<f64>> =
            (0..20).map(|i| vec![i as f64 * 1000.0, ((i * 7) % 5) as f64]).collect();
        let x = Matrix::from_rows(&rows).unwrap();
        let raw = Pca::default().fit_model(&x).unwrap();
        let std = Pca { standardize: true, ..Default::default() }.fit_model(&x).unwrap();
        assert!(raw.explained_variance_ratio[0] > 0.999, "raw PCA is all the big feature");
        assert!(std.explained_variance_ratio[0] < 0.999, "standardised PCA sees both");
    }

    #[test]
    fn the_same_data_gives_the_same_signs() {
        let rows: Vec<Vec<f64>> = (0..10).map(|i| vec![i as f64, -(i as f64)]).collect();
        let x = Matrix::from_rows(&rows).unwrap();
        let a = Pca::default().fit_model(&x).unwrap();
        let b = Pca::default().fit_model(&x).unwrap();
        assert_eq!(a, b);
        let lead = a.components[0].iter().map(|v| v.abs()).fold(0.0, f64::max);
        assert!(a.components[0].iter().any(|&v| v == lead), "the leading loading is positive");
    }
}
