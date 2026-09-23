//! Particle-mesh electrostatics: cloud-in-cell charge deposition, a
//! finite-volume Poisson solve by conjugate gradients, and field gather.
//!
//! Solves  -div(eps0 grad phi) = rho  on a node grid covering the domain.
//! Per axis the boundary is periodic, Dirichlet (fixed potential, used for
//! grounded walls and electrodes) or Neumann (insulating). The finite-volume
//! form (half control volumes on Neumann faces) keeps the operator
//! symmetric positive semi-definite, so plain CG converges; with no
//! Dirichlet face the constant null space is removed by enforcing
//! neutrality. Reductions are chunked in a fixed order so results do not
//! depend on the thread count.

use bevy_math::Vec3;
use rayon::prelude::*;

use crate::constants;

/// Boundary condition for one axis of the potential.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisBc {
    Periodic,
    /// Fixed potential on the min and max faces (V).
    Dirichlet { lo: f32, hi: f32 },
    /// Zero normal field on both faces.
    Neumann,
}

const CHUNK: usize = 4096;

/// Dot product summed chunk by chunk in a fixed order (deterministic for
/// any thread count); small grids stay on one thread, where fork-join
/// overhead would dominate a conjugate-gradient iteration.
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f64 {
    let chunk_sum = |(x, y): (&[f32], &[f32])| x.iter().zip(y).map(|(p, q)| *p as f64 * *q as f64).sum::<f64>();
    if a.len() < 16 * CHUNK {
        return a.chunks(CHUNK).zip(b.chunks(CHUNK)).map(chunk_sum).sum();
    }
    a.par_chunks(CHUNK)
        .zip(b.par_chunks(CHUNK))
        .map(chunk_sum)
        .collect::<Vec<f64>>()
        .iter()
        .sum()
}

#[derive(Clone, Debug)]
pub struct MeshField {
    /// Cells per axis.
    pub cells: [usize; 3],
    /// Nodes per axis (periodic: cells, otherwise cells + 1).
    pub nodes: [usize; 3],
    /// Cell edge per axis (m).
    pub d: Vec3,
    /// Domain minimum corner (node 0).
    pub origin: Vec3,
    pub bc: [AxisBc; 3],
    /// Right-hand side  rho * volume_weight / eps0  per node.
    b: Vec<f32>,
    /// Potential (V) per node.
    pub phi: Vec<f32>,
    /// Field (V/m) per node.
    pub e: Vec<Vec3>,
    fixed: Vec<bool>,
    vol: Vec<f32>,
    r: Vec<f32>,
    p: Vec<f32>,
    ap: Vec<f32>,
    has_dirichlet: bool,
    pub iterations: u32,
    pub relative_residual: f32,
}

impl MeshField {
    /// Grid with `resolution` cells along the longest axis and near-cubic cells.
    pub fn new(origin: Vec3, size: Vec3, resolution: u32, bc: [AxisBc; 3]) -> Self {
        let res = resolution.clamp(4, 256) as f32;
        let h = size.max_element() / res;
        let mut cells = [0usize; 3];
        let mut nodes = [0usize; 3];
        let mut d = Vec3::ZERO;
        for a in 0..3 {
            let n = (size[a] / h).round().max(2.0) as usize;
            cells[a] = n;
            d[a] = size[a] / n as f32;
            nodes[a] = if matches!(bc[a], AxisBc::Periodic) { n } else { n + 1 };
        }
        let count = nodes[0] * nodes[1] * nodes[2];
        let mut field = Self {
            cells,
            nodes,
            d,
            origin,
            bc,
            b: vec![0.0; count],
            phi: vec![0.0; count],
            e: vec![Vec3::ZERO; count],
            fixed: vec![false; count],
            vol: vec![1.0; count],
            r: vec![0.0; count],
            p: vec![0.0; count],
            ap: vec![0.0; count],
            has_dirichlet: false,
            iterations: 0,
            relative_residual: 0.0,
        };
        field.init_boundaries();
        field
    }

    #[inline]
    pub fn index(&self, i: usize, j: usize, k: usize) -> usize {
        (k * self.nodes[1] + j) * self.nodes[0] + i
    }

    fn init_boundaries(&mut self) {
        let [nx, ny, nz] = self.nodes;
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let idx = self.index(i, j, k);
                    let ijk = [i, j, k];
                    let mut w = 1.0f32;
                    let mut fixed_phi = None;
                    for a in 0..3 {
                        let on_lo = ijk[a] == 0;
                        let on_hi = ijk[a] == self.nodes[a] - 1;
                        match self.bc[a] {
                            AxisBc::Periodic => {}
                            AxisBc::Neumann => {
                                if on_lo || on_hi {
                                    w *= 0.5;
                                }
                            }
                            AxisBc::Dirichlet { lo, hi } => {
                                if on_lo {
                                    fixed_phi = Some(lo);
                                } else if on_hi {
                                    fixed_phi = Some(hi);
                                }
                            }
                        }
                    }
                    self.vol[idx] = w;
                    if let Some(v) = fixed_phi {
                        self.fixed[idx] = true;
                        self.phi[idx] = v;
                        self.has_dirichlet = true;
                    }
                }
            }
        }
    }

    /// Cloud-in-cell weights: the lower node per axis and the fraction toward
    /// the upper one.
    #[inline]
    fn cic(&self, p: Vec3) -> ([usize; 3], [usize; 3], Vec3) {
        let mut lo = [0usize; 3];
        let mut hi = [0usize; 3];
        let mut f = Vec3::ZERO;
        for a in 0..3 {
            let s = (p[a] - self.origin[a]) / self.d[a];
            let n = self.cells[a];
            match self.bc[a] {
                AxisBc::Periodic => {
                    let fl = s.floor();
                    let i0 = (fl as i64).rem_euclid(n as i64) as usize;
                    lo[a] = i0;
                    hi[a] = (i0 + 1) % n;
                    f[a] = s - fl;
                }
                _ => {
                    let s = s.clamp(0.0, n as f32);
                    let i0 = (s.floor() as usize).min(n - 1);
                    lo[a] = i0;
                    hi[a] = i0 + 1;
                    f[a] = s - i0 as f32;
                }
            }
        }
        (lo, hi, f)
    }

    /// Deposit charges (C) at positions, plus an optional uniform background
    /// charge density (C/m^3).
    ///
    /// The system is stored multiplied through by dx^2, so the operator is
    /// O(1) and the right-hand side is in volts. In raw SI units a
    /// nanometre grid puts the conjugate-gradient search direction near
    /// 1e39, past f32 range.
    pub fn deposit(&mut self, positions: &[Vec3], charges: &[f32], background: f32) {
        let h2 = self.d.x as f64 * self.d.x as f64;
        let inv = h2 / (self.d.x as f64 * self.d.y as f64 * self.d.z as f64 * constants::EPSILON_0);
        let bg = h2 * background as f64 / constants::EPSILON_0;
        let mut acc = vec![0.0f64; self.b.len()];
        for (idx, a) in acc.iter_mut().enumerate() {
            *a = bg * self.vol[idx] as f64;
        }
        for (p, &q) in positions.iter().zip(charges) {
            if q == 0.0 {
                continue;
            }
            let (lo, hi, f) = self.cic(*p);
            let qs = q as f64 * inv;
            for c in 0..8 {
                let (i, wx) = if c & 1 == 0 { (lo[0], 1.0 - f.x) } else { (hi[0], f.x) };
                let (j, wy) = if c & 2 == 0 { (lo[1], 1.0 - f.y) } else { (hi[1], f.y) };
                let (k, wz) = if c & 4 == 0 { (lo[2], 1.0 - f.z) } else { (hi[2], f.z) };
                let idx = self.index(i, j, k);
                acc[idx] += qs * (wx * wy * wz) as f64;
            }
        }
        if !self.has_dirichlet {
            // Enforce neutrality so the singular system is consistent.
            let (sb, sv) = acc
                .iter()
                .zip(&self.vol)
                .fold((0.0f64, 0.0f64), |(sb, sv), (b, v)| (sb + b, sv + *v as f64));
            let mean = sb / sv.max(1e-30);
            for (a, v) in acc.iter_mut().zip(&self.vol) {
                *a -= mean * *v as f64;
            }
        }
        for ((b, a), fixed) in self.b.iter_mut().zip(&acc).zip(&self.fixed) {
            *b = if *fixed { 0.0 } else { *a as f32 };
        }
    }

    /// out = A x on free nodes (zero on fixed nodes), with A scaled by dx^2.
    /// Rows of fixed nodes are zero, but neighbour values are read from `x`
    /// whether fixed or not, so `A phi` carries the Dirichlet coupling.
    fn apply(&self, x: &[f32], out: &mut [f32]) {
        let [nx, ny, _] = self.nodes;
        let plane = nx * ny;
        let inv_d2 = Vec3::new(
            1.0,
            (self.d.x / self.d.y).powi(2),
            (self.d.x / self.d.z).powi(2),
        );
        let bc = self.bc;
        let nodes = self.nodes;
        let fixed = &self.fixed;
        let axis_w = |a: usize, i: usize| -> f32 {
            match bc[a] {
                AxisBc::Neumann if i == 0 || i == nodes[a] - 1 => 0.5,
                _ => 1.0,
            }
        };
        out.par_chunks_mut(plane).enumerate().for_each(|(k, slab)| {
            for j in 0..ny {
                for i in 0..nx {
                    let idx = (k * ny + j) * nx + i;
                    let local = j * nx + i;
                    if fixed[idx] {
                        slab[local] = 0.0;
                        continue;
                    }
                    let xc = x[idx];
                    let ijk = [i, j, k];
                    let wv = [axis_w(0, i), axis_w(1, j), axis_w(2, k)];
                    let mut sum = 0.0f32;
                    for a in 0..3 {
                        let face = wv[(a + 1) % 3] * wv[(a + 2) % 3];
                        let n = nodes[a];
                        let stride = match a {
                            0 => 1,
                            1 => nx,
                            _ => plane,
                        };
                        let c = ijk[a];
                        // lower neighbour
                        let lower = if c > 0 {
                            Some(idx - stride)
                        } else if matches!(bc[a], AxisBc::Periodic) {
                            Some(idx + (n - 1) * stride)
                        } else {
                            None
                        };
                        let upper = if c + 1 < n {
                            Some(idx + stride)
                        } else if matches!(bc[a], AxisBc::Periodic) {
                            Some(idx - (n - 1) * stride)
                        } else {
                            None
                        };
                        let mut s = 0.0f32;
                        if let Some(l) = lower {
                            s += xc - x[l];
                        }
                        if let Some(u) = upper {
                            s += xc - x[u];
                        }
                        sum += face * s * inv_d2[a];
                    }
                    slab[local] = sum;
                }
            }
        });
    }

    /// Solve for `phi` (warm-started from the previous solution).
    pub fn solve(&mut self, tolerance: f32, max_iterations: u32) {
        let n = self.b.len();
        let mut ap = std::mem::take(&mut self.ap);
        let mut r = std::mem::take(&mut self.r);
        let mut p = std::mem::take(&mut self.p);
        ap.resize(n, 0.0);
        r.resize(n, 0.0);
        p.resize(n, 0.0);

        // Convergence is measured against the full right-hand side, the
        // charge term plus the electrodes' coupling: r with every free node
        // at zero.
        for i in 0..n {
            p[i] = if self.fixed[i] { self.phi[i] } else { 0.0 };
        }
        self.apply(&p, &mut ap);
        for i in 0..n {
            r[i] = if self.fixed[i] { 0.0 } else { self.b[i] - ap[i] };
        }
        let bnorm = dot(&r, &r).sqrt().max(1e-30);

        // r = b - A phi (warm start from the previous solution).
        self.apply(&self.phi, &mut ap);
        for i in 0..n {
            r[i] = if self.fixed[i] { 0.0 } else { self.b[i] - ap[i] };
        }
        let mut rr = dot(&r, &r);
        p.copy_from_slice(&r);
        let mut it = 0u32;
        while it < max_iterations && (rr.sqrt() / bnorm) > tolerance as f64 {
            self.apply(&p, &mut ap);
            let pap = dot(&p, &ap);
            if pap.abs() < 1e-300 {
                break;
            }
            let alpha = (rr / pap) as f32;
            for i in 0..n {
                self.phi[i] += alpha * p[i];
                r[i] -= alpha * ap[i];
            }
            let rr_new = dot(&r, &r);
            let beta = (rr_new / rr.max(1e-300)) as f32;
            for i in 0..n {
                p[i] = r[i] + beta * p[i];
            }
            rr = rr_new;
            it += 1;
        }
        if !self.has_dirichlet {
            let (s, w) = self
                .phi
                .iter()
                .zip(&self.vol)
                .fold((0.0f64, 0.0f64), |(s, w), (p, v)| (s + (*p * *v) as f64, w + *v as f64));
            let mean = (s / w.max(1e-30)) as f32;
            for v in &mut self.phi {
                *v -= mean;
            }
        }
        self.iterations = it;
        self.relative_residual = (rr.sqrt() / bnorm) as f32;
        self.ap = ap;
        self.r = r;
        self.p = p;
        self.compute_field();
    }

    fn compute_field(&mut self) {
        let [nx, ny, _] = self.nodes;
        let bc = self.bc;
        let d = self.d;
        let phi = &self.phi;
        let nodes = self.nodes;
        let plane = nx * ny;
        self.e.par_chunks_mut(plane).enumerate().for_each(|(k, slab)| {
            for j in 0..ny {
                for i in 0..nx {
                    let ijk = [i, j, k];
                    let idx = (k * ny + j) * nx + i;
                    let mut e = Vec3::ZERO;
                    for a in 0..3 {
                        let n = nodes[a];
                        let stride = match a {
                            0 => 1,
                            1 => nx,
                            _ => plane,
                        };
                        let c = ijk[a];
                        e[a] = match bc[a] {
                            AxisBc::Periodic => {
                                let lo = if c > 0 { idx - stride } else { idx + (n - 1) * stride };
                                let hi = if c + 1 < n { idx + stride } else { idx - (n - 1) * stride };
                                -(phi[hi] - phi[lo]) / (2.0 * d[a])
                            }
                            AxisBc::Neumann if c == 0 || c == n - 1 => 0.0,
                            _ => {
                                if c == 0 {
                                    -(phi[idx + stride] - phi[idx]) / d[a]
                                } else if c == n - 1 {
                                    -(phi[idx] - phi[idx - stride]) / d[a]
                                } else {
                                    -(phi[idx + stride] - phi[idx - stride]) / (2.0 * d[a])
                                }
                            }
                        };
                    }
                    slab[j * nx + i] = e;
                }
            }
        });

    }

    /// Field at a position by cloud-in-cell interpolation.
    #[inline]
    pub fn field_at(&self, p: Vec3) -> Vec3 {
        let (lo, hi, f) = self.cic(p);
        let mut e = Vec3::ZERO;
        for c in 0..8 {
            let (i, wx) = if c & 1 == 0 { (lo[0], 1.0 - f.x) } else { (hi[0], f.x) };
            let (j, wy) = if c & 2 == 0 { (lo[1], 1.0 - f.y) } else { (hi[1], f.y) };
            let (k, wz) = if c & 4 == 0 { (lo[2], 1.0 - f.z) } else { (hi[2], f.z) };
            e += self.e[self.index(i, j, k)] * (wx * wy * wz);
        }
        e
    }

    /// Potential at a position by cloud-in-cell interpolation.
    #[inline]
    pub fn potential_at(&self, p: Vec3) -> f32 {
        let (lo, hi, f) = self.cic(p);
        let mut v = 0.0;
        for c in 0..8 {
            let (i, wx) = if c & 1 == 0 { (lo[0], 1.0 - f.x) } else { (hi[0], f.x) };
            let (j, wy) = if c & 2 == 0 { (lo[1], 1.0 - f.y) } else { (hi[1], f.y) };
            let (k, wz) = if c & 4 == 0 { (lo[2], 1.0 - f.z) } else { (hi[2], f.z) };
            v += self.phi[self.index(i, j, k)] * (wx * wy * wz);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parallel-plate capacitor: no charge, phi = 0 at x=0 and V at x=L. The
    /// solution is linear and the field is uniform -V/L.
    #[test]
    fn capacitor_field_is_uniform() {
        let size = Vec3::new(1.0e-6, 0.5e-6, 0.5e-6);
        let v = 3.0;
        let mut m = MeshField::new(
            -size * 0.5,
            size,
            16,
            [AxisBc::Dirichlet { lo: 0.0, hi: v }, AxisBc::Neumann, AxisBc::Periodic],
        );
        m.deposit(&[], &[], 0.0);
        m.solve(1e-6, 2000);
        let e = m.field_at(Vec3::ZERO);
        let expect = -v / size.x;
        assert!((e.x - expect).abs() < 1e-3 * expect.abs(), "E={e:?} expected {expect}");
        assert!(e.y.abs() < 1e-3 * expect.abs() && e.z.abs() < 1e-3 * expect.abs());
    }

    /// Periodic sinusoidal charge rho = rho0 sin(k x): phi = rho0/(eps0 k^2) sin(k x),
    /// E = -rho0/(eps0 k) cos(k x).
    #[test]
    fn periodic_sine_matches_analytic() {
        let l = 2.0e-8f32;
        let size = Vec3::new(l, l * 0.25, l * 0.25);
        let res = 64;
        let mut m = MeshField::new(-size * 0.5, size, res, [AxisBc::Periodic; 3]);
        // Place point charges on every node so CIC deposits exactly the nodal values.
        let rho0 = 1.0e6f32; // C/m^3
        let k = std::f32::consts::TAU / l;
        let cell_v = m.d.x * m.d.y * m.d.z;
        let mut pos = Vec::new();
        let mut q = Vec::new();
        for kk in 0..m.nodes[2] {
            for jj in 0..m.nodes[1] {
                for ii in 0..m.nodes[0] {
                    let x = m.origin.x + ii as f32 * m.d.x;
                    pos.push(Vec3::new(x, m.origin.y + jj as f32 * m.d.y, m.origin.z + kk as f32 * m.d.z));
                    q.push(rho0 * (k * (x - m.origin.x)).sin() * cell_v);
                }
            }
        }
        m.deposit(&pos, &q, 0.0);
        m.solve(1e-7, 5000);
        let eps0 = constants::EPSILON_0 as f32;
        let mut max_err = 0.0f32;
        let amp = rho0 / (eps0 * k);
        for ii in 0..m.nodes[0] {
            let x = ii as f32 * m.d.x;
            let e = m.e[m.index(ii, 1, 1)].x;
            let expect = -amp * (k * x).cos();
            max_err = max_err.max((e - expect).abs());
        }
        // Second-order discretisation at 64 cells/wavelength: ~0.2%.
        assert!(max_err < 0.01 * amp, "max err {max_err} vs amplitude {amp}");
    }
}
