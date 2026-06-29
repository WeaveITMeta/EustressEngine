//! 1D linear finite-element MVP (Phase 4 / Way A3, 26). Axial bar elements:
//! assemble the global stiffness matrix, apply boundary conditions, solve
//! `K u = f` for nodal displacements, recover per-member axial stress. Real
//! math, pure std (small dense Gaussian elimination with partial pivoting) —
//! the first rung of PHYSICAL verification beyond closed-form fitness.

/// A 1D axial bar element between two nodes (indices into the node array).
#[derive(Clone, Copy, Debug)]
pub struct BarElement {
    pub from: usize,
    pub to: usize,
    /// Young's modulus E (Pa).
    pub youngs_modulus: f64,
    /// Cross-section area A (m^2).
    pub area: f64,
    /// Element length L (m).
    pub length: f64,
}

/// A 1D axial FEA problem: node count, elements, nodal axial loads, and the
/// set of fixed (zero-displacement) DOFs (supports).
#[derive(Clone, Debug, Default)]
pub struct Fea1d {
    pub num_nodes: usize,
    pub elements: Vec<BarElement>,
    /// External axial load at each node (N); resized to `num_nodes`.
    pub loads: Vec<f64>,
    /// Node indices clamped to zero displacement.
    pub fixed: Vec<usize>,
}

/// Result of a solve: nodal displacements (m) + per-element axial stress (Pa).
#[derive(Clone, Debug)]
pub struct FeaResult {
    pub displacements: Vec<f64>,
    pub element_stress: Vec<f64>,
}

impl Fea1d {
    /// Assemble + solve `K u = f`. Returns `None` if the system is singular
    /// (under-constrained — insufficient supports).
    pub fn solve(&self) -> Option<FeaResult> {
        let n = self.num_nodes;
        if n == 0 {
            return None;
        }
        // Dense global stiffness K (n x n), row-major.
        let mut k = vec![0.0f64; n * n];
        for e in &self.elements {
            if e.length <= 0.0 {
                continue;
            }
            let ke = (e.youngs_modulus * e.area) / e.length;
            let (i, j) = (e.from, e.to);
            k[i * n + i] += ke;
            k[j * n + j] += ke;
            k[i * n + j] -= ke;
            k[j * n + i] -= ke;
        }
        let mut f = self.loads.clone();
        f.resize(n, 0.0);
        // Apply fixed DOFs: zero the row + column, 1 on the diagonal, 0 in f.
        for &d in &self.fixed {
            if d >= n {
                continue;
            }
            for c in 0..n {
                k[d * n + c] = 0.0;
                k[c * n + d] = 0.0;
            }
            k[d * n + d] = 1.0;
            f[d] = 0.0;
        }
        let u = gaussian_solve(&mut k, &mut f, n)?;
        // sigma = E * (u_to - u_from) / L
        let element_stress = self
            .elements
            .iter()
            .map(|e| {
                if e.length <= 0.0 {
                    0.0
                } else {
                    e.youngs_modulus * (u[e.to] - u[e.from]) / e.length
                }
            })
            .collect();
        Some(FeaResult { displacements: u, element_stress })
    }
}

/// Dense Gaussian elimination with partial pivoting. `a` is row-major n x n,
/// `b` length n; both are consumed (mutated). Returns `None` if singular.
fn gaussian_solve(a: &mut [f64], b: &mut [f64], n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        // Partial pivot: largest-magnitude entry in this column.
        let mut piv = col;
        let mut best = a[col * n + col].abs();
        for r in (col + 1)..n {
            let v = a[r * n + col].abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if best < 1e-12 {
            return None;
        }
        if piv != col {
            for c in 0..n {
                a.swap(col * n + c, piv * n + c);
            }
            b.swap(col, piv);
        }
        let diag = a[col * n + col];
        for r in (col + 1)..n {
            let factor = a[r * n + col] / diag;
            if factor == 0.0 {
                continue;
            }
            for c in col..n {
                a[r * n + c] -= factor * a[col * n + c];
            }
            b[r] -= factor * b[col];
        }
    }
    // Back-substitution.
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for c in (r + 1)..n {
            s -= a[r * n + c] * x[c];
        }
        x[r] = s / a[r * n + r];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_bar_axial_deflection() {
        // One steel bar, fixed at node 0, 1000 N pull at node 1.
        // Closed form: u = F L / (E A) = 1000 * 1 / (200e9 * 0.01) = 5e-7 m.
        let fea = Fea1d {
            num_nodes: 2,
            elements: vec![BarElement {
                from: 0,
                to: 1,
                youngs_modulus: 200e9,
                area: 0.01,
                length: 1.0,
            }],
            loads: vec![0.0, 1000.0],
            fixed: vec![0],
        };
        let r = fea.solve().expect("solvable");
        assert!((r.displacements[0]).abs() < 1e-12, "fixed node stays put");
        assert!((r.displacements[1] - 5e-7).abs() < 1e-10, "u1 = FL/EA");
        // stress = F / A = 1000 / 0.01 = 1e5 Pa
        assert!((r.element_stress[0] - 1e5).abs() < 1.0);
    }

    #[test]
    fn unconstrained_is_singular() {
        let fea = Fea1d {
            num_nodes: 2,
            elements: vec![BarElement { from: 0, to: 1, youngs_modulus: 1.0, area: 1.0, length: 1.0 }],
            loads: vec![1.0, -1.0],
            fixed: vec![], // no support -> rigid-body mode -> singular
        };
        assert!(fea.solve().is_none());
    }
}
