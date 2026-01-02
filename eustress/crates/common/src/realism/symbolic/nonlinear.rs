//! # Nonlinear Solvers
//!
//! Nonlinear equation solvers for large deformations and hyperelasticity.
//!
//! ## Table of Contents
//!
//! 1. **Newton-Raphson** - Standard nonlinear solver
//! 2. **Arc-Length** - Path-following for snap-through
//! 3. **Trust Region** - Robust convergence
//! 4. **Hyperelastic Models** - Neo-Hookean, Mooney-Rivlin

use std::collections::HashMap;

// ============================================================================
// Newton-Raphson Solver (Enhanced)
// ============================================================================

/// Enhanced Newton-Raphson solver with line search and convergence control
pub struct NewtonRaphsonNonlinear {
    /// Maximum iterations
    pub max_iterations: usize,
    /// Convergence tolerance (residual norm)
    pub tolerance: f64,
    /// Minimum step size for line search
    pub min_step: f64,
    /// Maximum step size
    pub max_step: f64,
    /// Line search reduction factor
    pub line_search_factor: f64,
    /// Enable line search
    pub use_line_search: bool,
    /// Jacobian update frequency (1 = every iteration)
    pub jacobian_update_freq: usize,
}

impl Default for NewtonRaphsonNonlinear {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            tolerance: 1e-8,
            min_step: 1e-10,
            max_step: 1.0,
            line_search_factor: 0.5,
            use_line_search: true,
            jacobian_update_freq: 1,
        }
    }
}

/// Convergence result
#[derive(Debug, Clone)]
pub struct ConvergenceResult {
    /// Final solution
    pub solution: Vec<f64>,
    /// Number of iterations
    pub iterations: usize,
    /// Final residual norm
    pub residual_norm: f64,
    /// Converged successfully
    pub converged: bool,
    /// Convergence history (residual per iteration)
    pub history: Vec<f64>,
}

impl NewtonRaphsonNonlinear {
    /// Solve nonlinear system F(x) = 0
    /// 
    /// # Arguments
    /// * `residual` - Function computing residual vector F(x)
    /// * `jacobian` - Function computing Jacobian matrix J(x) = ∂F/∂x
    /// * `initial_guess` - Starting point
    pub fn solve<F, J>(
        &self,
        residual: F,
        jacobian: J,
        initial_guess: &[f64],
    ) -> ConvergenceResult
    where
        F: Fn(&[f64]) -> Vec<f64>,
        J: Fn(&[f64]) -> Vec<Vec<f64>>,
    {
        let n = initial_guess.len();
        let mut x = initial_guess.to_vec();
        let mut history = Vec::new();
        let mut cached_jacobian: Option<Vec<Vec<f64>>> = None;
        
        for iter in 0..self.max_iterations {
            // Compute residual
            let r = residual(&x);
            let r_norm = vector_norm(&r);
            history.push(r_norm);
            
            // Check convergence
            if r_norm < self.tolerance {
                return ConvergenceResult {
                    solution: x,
                    iterations: iter,
                    residual_norm: r_norm,
                    converged: true,
                    history,
                };
            }
            
            // Update Jacobian if needed
            if iter % self.jacobian_update_freq == 0 || cached_jacobian.is_none() {
                cached_jacobian = Some(jacobian(&x));
            }
            
            let j = cached_jacobian.as_ref().unwrap();
            
            // Solve J * dx = -r
            let neg_r: Vec<f64> = r.iter().map(|v| -v).collect();
            let dx = match solve_linear_system(j, &neg_r) {
                Ok(dx) => dx,
                Err(_) => {
                    return ConvergenceResult {
                        solution: x,
                        iterations: iter,
                        residual_norm: r_norm,
                        converged: false,
                        history,
                    };
                }
            };
            
            // Line search
            let step = if self.use_line_search {
                self.line_search(&residual, &x, &dx, r_norm)
            } else {
                1.0
            };
            
            // Update solution
            for i in 0..n {
                x[i] += step * dx[i];
            }
        }
        
        let final_r = residual(&x);
        let final_norm = vector_norm(&final_r);
        history.push(final_norm);
        
        ConvergenceResult {
            solution: x,
            iterations: self.max_iterations,
            residual_norm: final_norm,
            converged: final_norm < self.tolerance,
            history,
        }
    }
    
    /// Backtracking line search
    fn line_search<F>(&self, residual: &F, x: &[f64], dx: &[f64], r0_norm: f64) -> f64
    where
        F: Fn(&[f64]) -> Vec<f64>,
    {
        let mut step = self.max_step;
        let n = x.len();
        let mut x_new = vec![0.0; n];
        
        for _ in 0..20 {
            for i in 0..n {
                x_new[i] = x[i] + step * dx[i];
            }
            
            let r_new = residual(&x_new);
            let r_new_norm = vector_norm(&r_new);
            
            // Armijo condition: sufficient decrease
            if r_new_norm < (1.0 - 1e-4 * step) * r0_norm {
                return step;
            }
            
            step *= self.line_search_factor;
            
            if step < self.min_step {
                return self.min_step;
            }
        }
        
        step
    }
}

// ============================================================================
// Arc-Length Method (Riks/Crisfield)
// ============================================================================

/// Arc-length method for path-following through limit points
/// Useful for snap-through buckling and softening materials
pub struct ArcLengthSolver {
    /// Arc-length increment
    pub arc_length: f64,
    /// Maximum iterations per step
    pub max_iterations: usize,
    /// Convergence tolerance
    pub tolerance: f64,
    /// Maximum load steps
    pub max_load_steps: usize,
    /// Desired iterations per step (for arc-length adaptation)
    pub desired_iterations: usize,
}

impl Default for ArcLengthSolver {
    fn default() -> Self {
        Self {
            arc_length: 0.1,
            max_iterations: 25,
            tolerance: 1e-6,
            max_load_steps: 100,
            desired_iterations: 5,
        }
    }
}

/// Load-displacement point on equilibrium path
#[derive(Debug, Clone)]
pub struct EquilibriumPoint {
    /// Displacement vector
    pub displacement: Vec<f64>,
    /// Load factor λ
    pub load_factor: f64,
    /// Residual norm
    pub residual_norm: f64,
}

impl ArcLengthSolver {
    /// Trace equilibrium path using arc-length method
    /// 
    /// # Arguments
    /// * `internal_force` - Function computing internal force P_int(u)
    /// * `external_force` - Reference external force vector P_ext
    /// * `tangent_stiffness` - Function computing tangent stiffness K(u)
    /// * `initial_displacement` - Starting displacement
    /// * `initial_load_factor` - Starting load factor
    pub fn trace_path<F, K>(
        &self,
        internal_force: F,
        external_force: &[f64],
        tangent_stiffness: K,
        initial_displacement: &[f64],
        initial_load_factor: f64,
    ) -> Vec<EquilibriumPoint>
    where
        F: Fn(&[f64]) -> Vec<f64>,
        K: Fn(&[f64]) -> Vec<Vec<f64>>,
    {
        let n = initial_displacement.len();
        let mut path = Vec::new();
        
        let mut u = initial_displacement.to_vec();
        let mut lambda = initial_load_factor;
        let mut delta_lambda_prev = 0.01;
        let mut arc_length = self.arc_length;
        
        // Initial point
        path.push(EquilibriumPoint {
            displacement: u.clone(),
            load_factor: lambda,
            residual_norm: 0.0,
        });
        
        for _step in 0..self.max_load_steps {
            // Predictor: tangent direction
            let k = tangent_stiffness(&u);
            let k_inv_f = match solve_linear_system(&k, external_force) {
                Ok(v) => v,
                Err(_) => break,
            };
            
            // Arc-length constraint determines delta_lambda
            let k_inv_f_norm = vector_norm(&k_inv_f);
            if k_inv_f_norm < 1e-15 {
                break;
            }
            
            let delta_lambda = arc_length / (1.0 + k_inv_f_norm * k_inv_f_norm).sqrt();
            let sign = if delta_lambda_prev >= 0.0 { 1.0 } else { -1.0 };
            let delta_lambda = sign * delta_lambda;
            
            // Predictor step
            let mut delta_u: Vec<f64> = k_inv_f.iter().map(|v| v * delta_lambda).collect();
            lambda += delta_lambda;
            for i in 0..n {
                u[i] += delta_u[i];
            }
            
            // Corrector iterations
            let mut converged = false;
            let mut final_residual = 0.0;
            
            for _iter in 0..self.max_iterations {
                // Residual: R = λ*P_ext - P_int(u)
                let p_int = internal_force(&u);
                let residual: Vec<f64> = (0..n)
                    .map(|i| lambda * external_force[i] - p_int[i])
                    .collect();
                
                let r_norm = vector_norm(&residual);
                final_residual = r_norm;
                
                if r_norm < self.tolerance {
                    converged = true;
                    break;
                }
                
                // Solve for correction
                let k = tangent_stiffness(&u);
                let du = match solve_linear_system(&k, &residual) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                
                // Update
                for i in 0..n {
                    u[i] += du[i];
                    delta_u[i] += du[i];
                }
            }
            
            if !converged {
                // Reduce arc-length and retry
                arc_length *= 0.5;
                if arc_length < 1e-10 {
                    break;
                }
                continue;
            }
            
            // Store converged point
            path.push(EquilibriumPoint {
                displacement: u.clone(),
                load_factor: lambda,
                residual_norm: final_residual,
            });
            
            delta_lambda_prev = delta_lambda;
            
            // Adapt arc-length
            arc_length *= (self.desired_iterations as f64 / 5.0).sqrt();
            arc_length = arc_length.clamp(self.arc_length * 0.1, self.arc_length * 10.0);
        }
        
        path
    }
}

// ============================================================================
// Trust Region Method
// ============================================================================

/// Trust region solver for robust nonlinear convergence
pub struct TrustRegionSolver {
    /// Initial trust region radius
    pub initial_radius: f64,
    /// Maximum trust region radius
    pub max_radius: f64,
    /// Minimum trust region radius
    pub min_radius: f64,
    /// Acceptance threshold η
    pub eta: f64,
    /// Maximum iterations
    pub max_iterations: usize,
    /// Convergence tolerance
    pub tolerance: f64,
}

impl Default for TrustRegionSolver {
    fn default() -> Self {
        Self {
            initial_radius: 1.0,
            max_radius: 100.0,
            min_radius: 1e-12,
            eta: 0.1,
            max_iterations: 100,
            tolerance: 1e-8,
        }
    }
}

impl TrustRegionSolver {
    /// Solve using dogleg trust region method
    pub fn solve<F, J>(
        &self,
        residual: F,
        jacobian: J,
        initial_guess: &[f64],
    ) -> ConvergenceResult
    where
        F: Fn(&[f64]) -> Vec<f64>,
        J: Fn(&[f64]) -> Vec<Vec<f64>>,
    {
        let n = initial_guess.len();
        let mut x = initial_guess.to_vec();
        let mut radius = self.initial_radius;
        let mut history = Vec::new();
        
        for iter in 0..self.max_iterations {
            let r = residual(&x);
            let r_norm = vector_norm(&r);
            history.push(r_norm);
            
            if r_norm < self.tolerance {
                return ConvergenceResult {
                    solution: x,
                    iterations: iter,
                    residual_norm: r_norm,
                    converged: true,
                    history,
                };
            }
            
            let j = jacobian(&x);
            
            // Compute dogleg step
            let step = self.dogleg_step(&j, &r, radius);
            
            // Trial point
            let x_new: Vec<f64> = (0..n).map(|i| x[i] + step[i]).collect();
            let r_new = residual(&x_new);
            let r_new_norm = vector_norm(&r_new);
            
            // Predicted reduction (linear model)
            let j_step = matrix_vector_mult(&j, &step);
            let predicted: f64 = r.iter().zip(j_step.iter())
                .map(|(ri, ji)| ri * ri - (ri + ji).powi(2))
                .sum::<f64>() * 0.5;
            
            // Actual reduction
            let actual = 0.5 * (r_norm * r_norm - r_new_norm * r_new_norm);
            
            // Ratio
            let rho = if predicted.abs() > 1e-15 { actual / predicted } else { 0.0 };
            
            // Update trust region
            if rho < 0.25 {
                radius *= 0.25;
            } else if rho > 0.75 && (vector_norm(&step) - radius).abs() < 1e-10 {
                radius = (2.0 * radius).min(self.max_radius);
            }
            
            // Accept or reject step
            if rho > self.eta {
                x = x_new;
            }
            
            if radius < self.min_radius {
                break;
            }
        }
        
        let final_r = residual(&x);
        let final_norm = vector_norm(&final_r);
        
        ConvergenceResult {
            solution: x,
            iterations: self.max_iterations,
            residual_norm: final_norm,
            converged: final_norm < self.tolerance,
            history,
        }
    }
    
    /// Compute dogleg step
    fn dogleg_step(&self, j: &[Vec<f64>], r: &[f64], radius: f64) -> Vec<f64> {
        let n = r.len();
        
        // Steepest descent direction: -J^T * r
        let jt_r = matrix_transpose_vector_mult(j, r);
        let grad: Vec<f64> = jt_r.iter().map(|v| -v).collect();
        let grad_norm = vector_norm(&grad);
        
        if grad_norm < 1e-15 {
            return vec![0.0; n];
        }
        
        // Cauchy point
        let j_grad = matrix_vector_mult(j, &grad);
        let j_grad_norm_sq: f64 = j_grad.iter().map(|v| v * v).sum();
        let alpha = if j_grad_norm_sq > 1e-15 {
            (grad_norm * grad_norm) / j_grad_norm_sq
        } else {
            1.0
        };
        
        let cauchy: Vec<f64> = grad.iter().map(|v| alpha * v).collect();
        let cauchy_norm = vector_norm(&cauchy);
        
        if cauchy_norm >= radius {
            // Return scaled steepest descent
            return grad.iter().map(|v| v * radius / grad_norm).collect();
        }
        
        // Newton step
        let neg_r: Vec<f64> = r.iter().map(|v| -v).collect();
        let newton = match solve_linear_system(j, &neg_r) {
            Ok(v) => v,
            Err(_) => return cauchy,
        };
        let newton_norm = vector_norm(&newton);
        
        if newton_norm <= radius {
            return newton;
        }
        
        // Dogleg interpolation
        let diff: Vec<f64> = (0..n).map(|i| newton[i] - cauchy[i]).collect();
        let a: f64 = diff.iter().map(|v| v * v).sum();
        let b: f64 = 2.0 * cauchy.iter().zip(diff.iter()).map(|(c, d)| c * d).sum::<f64>();
        let c = cauchy_norm * cauchy_norm - radius * radius;
        
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 || a.abs() < 1e-15 {
            return cauchy;
        }
        
        let tau = (-b + discriminant.sqrt()) / (2.0 * a);
        
        (0..n).map(|i| cauchy[i] + tau * diff[i]).collect()
    }
}

// ============================================================================
// Hyperelastic Material Models
// ============================================================================

/// Neo-Hookean hyperelastic model
/// W = μ/2 * (I₁ - 3) - μ*ln(J) + λ/2 * (ln(J))²
pub struct NeoHookean {
    /// Shear modulus μ
    pub mu: f64,
    /// Lamé parameter λ
    pub lambda: f64,
}

impl NeoHookean {
    /// Create from Young's modulus and Poisson's ratio
    pub fn from_engineering(young_modulus: f64, poisson_ratio: f64) -> Self {
        let mu = young_modulus / (2.0 * (1.0 + poisson_ratio));
        let lambda = young_modulus * poisson_ratio / ((1.0 + poisson_ratio) * (1.0 - 2.0 * poisson_ratio));
        Self { mu, lambda }
    }
    
    /// Strain energy density
    pub fn strain_energy(&self, deformation_gradient: &[[f64; 3]; 3]) -> f64 {
        let f = deformation_gradient;
        let j = determinant_3x3(f);
        let i1 = trace_ftf(f);
        
        0.5 * self.mu * (i1 - 3.0) - self.mu * j.ln() + 0.5 * self.lambda * j.ln().powi(2)
    }
    
    /// First Piola-Kirchhoff stress: P = μ(F - F^-T) + λ*ln(J)*F^-T
    pub fn first_piola_stress(&self, deformation_gradient: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
        let f = deformation_gradient;
        let j = determinant_3x3(f);
        let f_inv_t = inverse_transpose_3x3(f);
        
        let mut p = [[0.0; 3]; 3];
        for i in 0..3 {
            for j_idx in 0..3 {
                p[i][j_idx] = self.mu * (f[i][j_idx] - f_inv_t[i][j_idx])
                    + self.lambda * j.ln() * f_inv_t[i][j_idx];
            }
        }
        p
    }
    
    /// Cauchy stress: σ = (1/J) * P * F^T
    pub fn cauchy_stress(&self, deformation_gradient: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
        let p = self.first_piola_stress(deformation_gradient);
        let f = deformation_gradient;
        let j = determinant_3x3(f);
        
        let mut sigma = [[0.0; 3]; 3];
        for i in 0..3 {
            for k in 0..3 {
                for l in 0..3 {
                    sigma[i][k] += p[i][l] * f[k][l];
                }
                sigma[i][k] /= j;
            }
        }
        sigma
    }
}

/// Mooney-Rivlin hyperelastic model
/// W = C₁(I₁ - 3) + C₂(I₂ - 3)
pub struct MooneyRivlin {
    /// Material constant C₁
    pub c1: f64,
    /// Material constant C₂
    pub c2: f64,
    /// Bulk modulus K
    pub bulk_modulus: f64,
}

impl MooneyRivlin {
    /// Create from shear modulus (μ = 2(C₁ + C₂))
    pub fn from_shear_modulus(shear_modulus: f64, c1_ratio: f64, bulk_modulus: f64) -> Self {
        let c1 = shear_modulus * c1_ratio / 2.0;
        let c2 = shear_modulus * (1.0 - c1_ratio) / 2.0;
        Self { c1, c2, bulk_modulus }
    }
    
    /// Strain energy density (incompressible)
    pub fn strain_energy(&self, deformation_gradient: &[[f64; 3]; 3]) -> f64 {
        let i1 = trace_ftf(deformation_gradient);
        let i2 = second_invariant(deformation_gradient);
        
        self.c1 * (i1 - 3.0) + self.c2 * (i2 - 3.0)
    }
}

// ============================================================================
// Linear Algebra Helpers
// ============================================================================

fn vector_norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn solve_linear_system(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, &'static str> {
    let n = b.len();
    if a.len() != n || a.iter().any(|row| row.len() != n) {
        return Err("Matrix dimension mismatch");
    }
    
    // Augmented matrix
    let mut aug: Vec<Vec<f64>> = a.iter()
        .zip(b.iter())
        .map(|(row, &bi)| {
            let mut new_row = row.clone();
            new_row.push(bi);
            new_row
        })
        .collect();
    
    // Forward elimination with partial pivoting
    for i in 0..n {
        let mut max_row = i;
        for k in (i + 1)..n {
            if aug[k][i].abs() > aug[max_row][i].abs() {
                max_row = k;
            }
        }
        aug.swap(i, max_row);
        
        if aug[i][i].abs() < 1e-15 {
            return Err("Singular matrix");
        }
        
        for k in (i + 1)..n {
            let factor = aug[k][i] / aug[i][i];
            for j in i..=n {
                aug[k][j] -= factor * aug[i][j];
            }
        }
    }
    
    // Back substitution
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = aug[i][n];
        for j in (i + 1)..n {
            x[i] -= aug[i][j] * x[j];
        }
        x[i] /= aug[i][i];
    }
    
    Ok(x)
}

fn matrix_vector_mult(a: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    a.iter()
        .map(|row| row.iter().zip(x.iter()).map(|(a, b)| a * b).sum())
        .collect()
}

fn matrix_transpose_vector_mult(a: &[Vec<f64>], x: &[f64]) -> Vec<f64> {
    let n = a[0].len();
    (0..n)
        .map(|j| a.iter().zip(x.iter()).map(|(row, xi)| row[j] * xi).sum())
        .collect()
}

fn determinant_3x3(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn trace_ftf(f: &[[f64; 3]; 3]) -> f64 {
    let mut trace = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            trace += f[j][i] * f[j][i];
        }
    }
    trace
}

fn second_invariant(f: &[[f64; 3]; 3]) -> f64 {
    let i1 = trace_ftf(f);
    let mut ftf_sq_trace = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            let mut sum = 0.0;
            for k in 0..3 {
                sum += f[k][i] * f[k][j];
            }
            ftf_sq_trace += sum * sum;
        }
    }
    0.5 * (i1 * i1 - ftf_sq_trace)
}

fn inverse_transpose_3x3(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = determinant_3x3(m);
    if det.abs() < 1e-15 {
        return [[0.0; 3]; 3];
    }
    
    let inv_det = 1.0 / det;
    
    [
        [
            inv_det * (m[1][1] * m[2][2] - m[2][1] * m[1][2]),
            inv_det * (m[1][2] * m[2][0] - m[1][0] * m[2][2]),
            inv_det * (m[1][0] * m[2][1] - m[2][0] * m[1][1]),
        ],
        [
            inv_det * (m[0][2] * m[2][1] - m[0][1] * m[2][2]),
            inv_det * (m[0][0] * m[2][2] - m[0][2] * m[2][0]),
            inv_det * (m[2][0] * m[0][1] - m[0][0] * m[2][1]),
        ],
        [
            inv_det * (m[0][1] * m[1][2] - m[0][2] * m[1][1]),
            inv_det * (m[1][0] * m[0][2] - m[0][0] * m[1][2]),
            inv_det * (m[0][0] * m[1][1] - m[1][0] * m[0][1]),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_newton_raphson_quadratic() {
        let solver = NewtonRaphsonNonlinear::default();
        
        // Solve x² - 2 = 0
        let result = solver.solve(
            |x| vec![x[0] * x[0] - 2.0],
            |x| vec![vec![2.0 * x[0]]],
            &[1.0],
        );
        
        assert!(result.converged);
        assert!((result.solution[0] - std::f64::consts::SQRT_2).abs() < 1e-6);
    }
    
    #[test]
    fn test_neo_hookean() {
        let material = NeoHookean::from_engineering(200e9, 0.3);
        
        // Identity deformation (no strain)
        let f = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let w = material.strain_energy(&f);
        
        // Should be zero for no deformation
        assert!(w.abs() < 1e-10);
    }
    
    #[test]
    fn test_trust_region() {
        let solver = TrustRegionSolver::default();
        
        // Rosenbrock function minimum at (1, 1)
        let result = solver.solve(
            |x| vec![
                10.0 * (x[1] - x[0] * x[0]),
                1.0 - x[0],
            ],
            |x| vec![
                vec![-20.0 * x[0], 10.0],
                vec![-1.0, 0.0],
            ],
            &[0.0, 0.0],
        );
        
        assert!(result.converged);
        assert!((result.solution[0] - 1.0).abs() < 1e-4);
        assert!((result.solution[1] - 1.0).abs() < 1e-4);
    }
}
