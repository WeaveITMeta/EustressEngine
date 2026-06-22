//! Min-max LOD decimation (Data Platform P3, data side).
//!
//! "Millions of points" never reach the GPU per frame; they are decimated to
//! ~one bucket per screen pixel. **Min-max** decimation (not averaging) is
//! used on purpose: each bucket keeps both the minimum and maximum `y`, so the
//! visual envelope — spikes included — survives. Averaging would smear away the
//! exact extrema that curve-fit / FFT analysts care about, so analysis always
//! runs on the full-resolution column, never the proxy (plan §V.3a).
//!
//! Pure `std`, always compiled. The GPU upload of these buckets is engine-side.

use crate::numerics::paired_xy;
use crate::{ColumnData, Result};

/// One decimation bucket: the x-span it covers and the y min/max within it.
/// A renderer draws two points (or a vertical bar) per bucket, preserving the
/// envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct MinMaxBucket {
    /// First x in the bucket.
    pub x_lo: f64,
    /// Last x in the bucket.
    pub x_hi: f64,
    /// Minimum y in the bucket.
    pub y_min: f64,
    /// Maximum y in the bucket.
    pub y_max: f64,
    /// Number of source points in the bucket.
    pub n: usize,
}

/// Decimate the cleaned `(x, y)` series into at most `buckets` contiguous
/// min-max buckets (by position; assumes `x` ascending). Fewer points than
/// `buckets` returns one bucket per point. Empty input → empty output.
pub fn min_max_decimate(x: &ColumnData, y: &ColumnData, buckets: usize) -> Result<Vec<MinMaxBucket>> {
    let (xs, ys) = paired_xy(x, y)?;
    let n = xs.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let buckets = buckets.max(1).min(n);
    let mut out = Vec::with_capacity(buckets);
    for b in 0..buckets {
        // Even index split: bucket b spans [b·n/buckets, (b+1)·n/buckets).
        let lo = b * n / buckets;
        let hi = ((b + 1) * n / buckets).max(lo + 1).min(n);
        let yr = &ys[lo..hi];
        let y_min = yr.iter().copied().fold(f64::INFINITY, f64::min);
        let y_max = yr.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        out.push(MinMaxBucket {
            x_lo: xs[lo],
            x_hi: xs[hi - 1],
            y_min,
            y_max,
            n: hi - lo,
        });
    }
    Ok(out)
}

/// Build an LOD pyramid: successive min-max levels at `target_buckets`,
/// `target_buckets/factor`, `target_buckets/factor²`, … down to 1, capped at
/// `max_levels`. Level 0 is the finest. A chart picks the level whose bucket
/// count is closest to the panel's pixel width (plan §V.3c).
pub fn lod_pyramid(
    x: &ColumnData,
    y: &ColumnData,
    target_buckets: usize,
    factor: usize,
    max_levels: usize,
) -> Result<Vec<Vec<MinMaxBucket>>> {
    let factor = factor.max(2);
    let mut levels = Vec::new();
    let mut buckets = target_buckets.max(1);
    while levels.len() < max_levels.max(1) {
        levels.push(min_max_decimate(x, y, buckets)?);
        if buckets <= 1 {
            break;
        }
        buckets = (buckets / factor).max(1);
    }
    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64col(v: &[f64]) -> ColumnData {
        ColumnData::F64(v.iter().map(|&x| Some(x)).collect())
    }

    #[test]
    fn decimate_preserves_envelope_extremes() {
        // 8 points; decimate to 2 buckets. A spike in the first half must survive.
        let x = f64col(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        let y = f64col(&[0.0, 99.0, 1.0, 2.0, 3.0, 4.0, 5.0, -50.0]);
        let b = min_max_decimate(&x, &y, 2).unwrap();
        assert_eq!(b.len(), 2);
        // first bucket spans points 0..4 → contains the +99 spike
        assert_eq!(b[0].y_max, 99.0);
        assert_eq!(b[0].x_lo, 0.0);
        // second bucket spans 4..8 → contains the -50 trough
        assert_eq!(b[1].y_min, -50.0);
        assert_eq!(b[1].x_hi, 7.0);
        assert_eq!(b[0].n + b[1].n, 8);
    }

    #[test]
    fn fewer_points_than_buckets_one_per_point() {
        let x = f64col(&[0.0, 1.0, 2.0]);
        let y = f64col(&[10.0, 20.0, 30.0]);
        let b = min_max_decimate(&x, &y, 100).unwrap();
        assert_eq!(b.len(), 3);
        assert!(b.iter().all(|bk| bk.n == 1 && bk.y_min == bk.y_max));
    }

    #[test]
    fn pyramid_levels_shrink() {
        let xs: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|x| (x / 50.0).sin()).collect();
        let levels = lod_pyramid(&f64col(&xs), &f64col(&ys), 256, 4, 5).unwrap();
        assert!(levels.len() >= 2);
        // each level has no more buckets than the previous (monotone shrink)
        for w in levels.windows(2) {
            assert!(w[1].len() <= w[0].len());
        }
        assert!(levels[0].len() <= 256);
    }

    #[test]
    fn empty_input_empty_output() {
        let e = ColumnData::F64(vec![]);
        assert!(min_max_decimate(&e, &e, 4).unwrap().is_empty());
    }
}
