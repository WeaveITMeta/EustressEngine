//! Lightweight ML over columns (Data Platform P4) — k-means clustering and
//! kNN-distance anomaly scoring.
//!
//! Pure `std`, always compiled, no deps. This is the **k-means floor** and the
//! brute-force kNN anomaly score; the HNSW-accelerated versions (via
//! `eustress-embedvec`) are the future scale path. Features are z-score
//! normalized per column, which strips physical dimension by construction — so
//! cluster ids and anomaly scores are intentionally **dimensionless** (the
//! documented exception to D3).

use crate::numerics::as_f64_opt;
use crate::{ColumnData, DataError, Frame, Result};

fn euclid2(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// Build a z-score-normalized feature matrix (`n_rows` × `cols.len()`) from the
/// named numeric columns. Null / non-finite cells become 0 (the post-z-score
/// mean), so every row yields a feature vector aligned to the frame.
fn feature_matrix(frame: &Frame, cols: &[&str]) -> Result<Vec<Vec<f64>>> {
    if cols.is_empty() {
        return Err(DataError::Schema("feature_matrix: no columns selected".into()));
    }
    let n = frame.n_rows();
    let mut feats = vec![Vec::with_capacity(cols.len()); n];
    for &name in cols {
        let col = frame
            .column(name)
            .ok_or_else(|| DataError::Schema(format!("feature_matrix: no column `{name}`")))?;
        let raw = as_f64_opt(col)?;
        if raw.len() != n {
            return Err(DataError::Schema("feature_matrix: ragged columns".into()));
        }
        let finite: Vec<f64> = raw.iter().flatten().filter(|x| x.is_finite()).copied().collect();
        if finite.is_empty() {
            return Err(DataError::Schema(format!("feature_matrix: column `{name}` all null")));
        }
        let mean = finite.iter().sum::<f64>() / finite.len() as f64;
        let var = finite.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / finite.len() as f64;
        let denom = if var.sqrt() < 1e-12 { 1.0 } else { var.sqrt() };
        for (i, v) in raw.iter().enumerate() {
            let z = match v {
                Some(x) if x.is_finite() => (x - mean) / denom,
                _ => 0.0,
            };
            feats[i].push(z);
        }
    }
    Ok(feats)
}

/// k-means (Lloyd's algorithm) over `points`, returning the cluster index per
/// point. **Deterministic**: initial centroids are the points at evenly-spaced
/// indices, so repeated runs give identical labels (no RNG). Empty clusters
/// retain their previous centroid.
pub fn kmeans(points: &[Vec<f64>], k: usize, max_iters: usize) -> Result<Vec<usize>> {
    let n = points.len();
    if k == 0 || k > n {
        return Err(DataError::Schema(format!("kmeans: need 1 <= k <= n, got k={k} n={n}")));
    }
    let dims = points[0].len();
    // Deterministic seeding: evenly-spaced points across the set.
    let mut centroids: Vec<Vec<f64>> = (0..k).map(|c| points[c * n / k].clone()).collect();
    let mut assign = vec![0usize; n];
    for _ in 0..max_iters.max(1) {
        let mut changed = false;
        // Assignment step.
        for (i, p) in points.iter().enumerate() {
            let mut best = 0;
            let mut best_d = f64::INFINITY;
            for (c, cen) in centroids.iter().enumerate() {
                let d = euclid2(p, cen);
                if d < best_d {
                    best_d = d;
                    best = c;
                }
            }
            if assign[i] != best {
                assign[i] = best;
                changed = true;
            }
        }
        // Update step.
        let mut sums = vec![vec![0.0; dims]; k];
        let mut counts = vec![0usize; k];
        for (i, p) in points.iter().enumerate() {
            counts[assign[i]] += 1;
            for d in 0..dims {
                sums[assign[i]][d] += p[d];
            }
        }
        for c in 0..k {
            if counts[c] > 0 {
                for d in 0..dims {
                    centroids[c][d] = sums[c][d] / counts[c] as f64;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Ok(assign)
}

/// Mean Euclidean distance from each point to its `k_neighbors` nearest other
/// points — a brute-force anomaly score (higher = more isolated). O(n²).
pub fn anomaly_scores(points: &[Vec<f64>], k_neighbors: usize) -> Result<Vec<f64>> {
    let n = points.len();
    if n < 2 {
        return Err(DataError::Schema("anomaly_scores needs >= 2 points".into()));
    }
    let k = k_neighbors.clamp(1, n - 1);
    let mut out = Vec::with_capacity(n);
    for (i, p) in points.iter().enumerate() {
        let mut dists: Vec<f64> = points
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, q)| euclid2(p, q).sqrt())
            .collect();
        dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        out.push(dists[..k].iter().sum::<f64>() / k as f64);
    }
    Ok(out)
}

/// Cluster the rows of a frame by the named numeric columns into `k` clusters,
/// returning an `I64` `cluster_id` column aligned to the frame.
pub fn cluster_column(frame: &Frame, cols: &[&str], k: usize) -> Result<ColumnData> {
    let pts = feature_matrix(frame, cols)?;
    let labels = kmeans(&pts, k, 50)?;
    Ok(ColumnData::I64(labels.into_iter().map(|c| Some(c as i64)).collect()))
}

/// Score each row's anomaly by the named numeric columns, returning an `F64`
/// `anomaly_score` column (mean distance to `k_neighbors` nearest rows).
pub fn anomaly_column(frame: &Frame, cols: &[&str], k_neighbors: usize) -> Result<ColumnData> {
    let pts = feature_matrix(frame, cols)?;
    let scores = anomaly_scores(&pts, k_neighbors)?;
    Ok(ColumnData::F64(scores.into_iter().map(Some).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frame_from_columns, ColumnDtype, ColumnSpec};

    #[test]
    fn kmeans_splits_two_separated_clusters() {
        // Cluster A near origin (idx 0,1,2), cluster B near (10,10) (idx 3,4,5).
        let pts = vec![
            vec![0.0, 0.1],
            vec![0.2, 0.0],
            vec![0.1, 0.2],
            vec![10.0, 10.1],
            vec![10.2, 10.0],
            vec![9.9, 10.2],
        ];
        let a = kmeans(&pts, 2, 50).unwrap();
        assert_eq!(a[0], a[1]);
        assert_eq!(a[1], a[2]);
        assert_eq!(a[3], a[4]);
        assert_eq!(a[4], a[5]);
        assert_ne!(a[0], a[3], "the two clusters must get different labels");
    }

    #[test]
    fn anomaly_flags_the_outlier() {
        // A tight cluster + one far outlier (last point).
        let pts = vec![
            vec![0.0, 0.0],
            vec![0.1, 0.0],
            vec![0.0, 0.1],
            vec![0.1, 0.1],
            vec![50.0, 50.0],
        ];
        let s = anomaly_scores(&pts, 2).unwrap();
        let max_i = s
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(max_i, 4, "the far point should have the highest anomaly score");
    }

    #[test]
    fn cluster_and_anomaly_columns_from_a_frame() {
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("x", ColumnDtype::F64),
                ColumnData::F64(vec![Some(0.0), Some(0.1), Some(10.0), Some(10.1)]),
            ),
            (
                ColumnSpec::new("y", ColumnDtype::F64),
                ColumnData::F64(vec![Some(0.0), Some(0.1), Some(10.0), Some(10.1)]),
            ),
        ])
        .unwrap();
        let clusters = cluster_column(&frame, &["x", "y"], 2).unwrap();
        match clusters {
            ColumnData::I64(v) => {
                assert_eq!(v.len(), 4);
                assert_eq!(v[0], v[1]);
                assert_eq!(v[2], v[3]);
                assert_ne!(v[0], v[2]);
            }
            _ => panic!("cluster column should be I64"),
        }
        let anom = anomaly_column(&frame, &["x", "y"], 1).unwrap();
        assert!(matches!(anom, ColumnData::F64(ref v) if v.len() == 4));
    }
}
