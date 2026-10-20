//! Small dense linear algebra for the mining layer: a Cholesky solver for the
//! normal equations and a Jacobi eigensolver for symmetric matrices.
//!
//! Both work on flat row-major `n x n` slices. They target the modest sizes a
//! feature covariance reaches (tens to a few hundred features), where these
//! classical methods are exact, dependency-free, and easy to audit.

use crate::{DataError, Result};

/// Solve `A x = b` for symmetric positive-definite `A` by Cholesky
/// factorisation (`A = L Lᵀ`).
///
/// Fails when `A` is not positive definite, which for normal equations means
/// the features are collinear (one is a combination of others). The error says
/// so, because the fix is a ridge penalty, not a better solver.
pub(crate) fn cholesky_solve(a: &[f64], b: &[f64], n: usize) -> Result<Vec<f64>> {
    debug_assert_eq!(a.len(), n * n);
    debug_assert_eq!(b.len(), n);
    let mut l = vec![0.0; n * n];
    for i in 0..n {
        for j in 0..=i {
            let mut sum = a[i * n + j];
            for k in 0..j {
                sum -= l[i * n + k] * l[j * n + k];
            }
            if i == j {
                // Relative threshold: a pivot this small relative to its own
                // diagonal means the column is (numerically) dependent.
                if sum <= 1e-12 * (1.0 + a[i * n + i].abs()) {
                    return Err(DataError::Schema(
                        "the features are collinear (one is a combination of others); \
                         drop the redundant feature or add a ridge penalty"
                            .into(),
                    ));
                }
                l[i * n + i] = sum.sqrt();
            } else {
                l[i * n + j] = sum / l[j * n + j];
            }
        }
    }
    // Forward substitution: L y = b.
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i * n + k] * y[k];
        }
        y[i] = s / l[i * n + i];
    }
    // Back substitution: Lᵀ x = y.
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in i + 1..n {
            s -= l[k * n + i] * x[k];
        }
        x[i] = s / l[i * n + i];
    }
    Ok(x)
}

/// Eigen-decomposition of a symmetric `n x n` matrix by cyclic Jacobi
/// rotations.
///
/// Returns `(eigenvalues, eigenvectors)` where eigenvector `k` is column `k` of
/// the row-major `n x n` result, i.e. `vectors[i * n + k]`. Unsorted; callers
/// order them. Jacobi is chosen over faster methods because every rotation is
/// orthogonal, so the eigenvectors come out orthonormal to machine precision,
/// which is the property PCA actually depends on.
pub(crate) fn symmetric_eigen(a: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    debug_assert_eq!(a.len(), n * n);
    let mut m = a.to_vec();
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let scale: f64 = m.iter().map(|x| x * x).sum::<f64>().max(f64::MIN_POSITIVE);

    for _sweep in 0..100 {
        let mut off = 0.0;
        for p in 0..n {
            for q in p + 1..n {
                off += m[p * n + q] * m[p * n + q];
            }
        }
        if off <= 1e-26 * scale {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = m[p * n + q];
                if apq.abs() <= 1e-300 {
                    continue;
                }
                let app = m[p * n + p];
                let aqq = m[q * n + q];
                // The rotation that zeroes m[p][q]: t is the smaller root of
                // t² + 2θt − 1 = 0, which keeps the rotation angle under 45°
                // and the update numerically stable.
                let theta = (aqq - app) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                // M ← Jᵀ M J: columns p and q, then rows p and q.
                for k in 0..n {
                    let mkp = m[k * n + p];
                    let mkq = m[k * n + q];
                    m[k * n + p] = c * mkp - s * mkq;
                    m[k * n + q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let mpk = m[p * n + k];
                    let mqk = m[q * n + k];
                    m[p * n + k] = c * mpk - s * mqk;
                    m[q * n + k] = s * mpk + c * mqk;
                }
                // V ← V J accumulates the eigenvectors.
                for k in 0..n {
                    let vkp = v[k * n + p];
                    let vkq = v[k * n + q];
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let values = (0..n).map(|i| m[i * n + i]).collect();
    (values, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cholesky_solves_a_known_system() {
        // A = [[4, 2], [2, 3]], x = [1, 2]  =>  b = [8, 8]
        let a = [4.0, 2.0, 2.0, 3.0];
        let x = cholesky_solve(&a, &[8.0, 8.0], 2).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-12 && (x[1] - 2.0).abs() < 1e-12, "{x:?}");
    }

    #[test]
    fn cholesky_refuses_a_singular_matrix_and_says_why() {
        // Second row is twice the first: collinear.
        let a = [1.0, 2.0, 2.0, 4.0];
        let err = cholesky_solve(&a, &[1.0, 2.0], 2).unwrap_err();
        assert!(format!("{err}").contains("collinear"));
    }

    #[test]
    fn jacobi_recovers_known_eigenvalues() {
        // [[2, 1], [1, 2]] has eigenvalues 1 and 3.
        let (mut vals, _) = symmetric_eigen(&[2.0, 1.0, 1.0, 2.0], 2);
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((vals[0] - 1.0).abs() < 1e-10 && (vals[1] - 3.0).abs() < 1e-10, "{vals:?}");
    }

    #[test]
    fn jacobi_eigenvectors_are_orthonormal_and_reconstruct_the_matrix() {
        let a = [4.0, 1.0, 0.5, 1.0, 3.0, 0.25, 0.5, 0.25, 2.0];
        let n = 3;
        let (vals, vecs) = symmetric_eigen(&a, n);
        // Orthonormal columns.
        for p in 0..n {
            for q in 0..n {
                let dot: f64 = (0..n).map(|i| vecs[i * n + p] * vecs[i * n + q]).sum();
                let want = if p == q { 1.0 } else { 0.0 };
                assert!((dot - want).abs() < 1e-10, "columns {p},{q}: {dot}");
            }
        }
        // A v = λ v for every pair.
        for k in 0..n {
            for i in 0..n {
                let av: f64 = (0..n).map(|j| a[i * n + j] * vecs[j * n + k]).sum();
                assert!((av - vals[k] * vecs[i * n + k]).abs() < 1e-9);
            }
        }
    }
}
