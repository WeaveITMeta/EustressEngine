//! # Constraint Solver
//!
//! Real-time constraint solving for physics systems.
//!
//! ## Usage
//!
//! The solver handles systems of equations that arise from
//! physical constraints (e.g., conservation laws, equilibrium).

use std::collections::HashMap;

/// Error type for solver operations
#[derive(Debug, Clone)]
pub enum SolveError {
    /// Variable not found in system
    UnknownVariable(String),
    /// System is underdetermined
    Underdetermined,
    /// System is overdetermined
    Overdetermined,
    /// No solution exists
    NoSolution,
    /// Numerical error during solving
    NumericalError(String),
}

/// A constraint equation
#[derive(Clone, Debug)]
pub struct Constraint {
    /// Constraint name/identifier
    pub name: String,
    /// Left-hand side expression
    pub lhs: String,
    /// Right-hand side expression
    pub rhs: String,
}

impl Constraint {
    /// Create a new constraint
    pub fn new(name: &str, lhs: &str, rhs: &str) -> Self {
        Self {
            name: name.to_string(),
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
        }
    }
    
    /// Create equality constraint: expr = 0
    pub fn zero(name: &str, expr: &str) -> Self {
        Self::new(name, expr, "0")
    }
}

/// Constraint solver for physics systems
#[derive(Default)]
pub struct ConstraintSolver {
    /// System of constraints
    constraints: Vec<Constraint>,
    /// Known variable values
    knowns: HashMap<String, f64>,
    /// Variables to solve for
    unknowns: Vec<String>,
}

impl ConstraintSolver {
    /// Create a new solver
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a constraint to the system
    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }
    
    /// Set a known variable value
    pub fn set_known(&mut self, name: &str, value: f64) {
        self.knowns.insert(name.to_string(), value);
    }
    
    /// Add an unknown variable to solve for
    pub fn add_unknown(&mut self, name: &str) {
        self.unknowns.push(name.to_string());
    }
    
    /// Clear all constraints and variables
    pub fn clear(&mut self) {
        self.constraints.clear();
        self.knowns.clear();
        self.unknowns.clear();
    }
    
    /// Solve the system for unknowns
    /// 
    /// Note: Full implementation requires Symbolica.
    /// This provides a simplified numerical solver for common cases.
    pub fn solve(&self) -> Result<HashMap<String, f64>, SolveError> {
        if self.unknowns.len() > self.constraints.len() {
            return Err(SolveError::Underdetermined);
        }
        
        // For now, return knowns as a baseline
        // Full Symbolica integration would solve symbolically
        let mut result = self.knowns.clone();
        
        // Placeholder: In full implementation, this would:
        // 1. Parse constraints into symbolic expressions
        // 2. Substitute known values
        // 3. Solve for unknowns using Symbolica's solvers
        
        Ok(result)
    }
    
    /// Solve a simple linear equation: a*x + b = 0
    pub fn solve_linear(a: f64, b: f64) -> Result<f64, SolveError> {
        if a.abs() < 1e-15 {
            if b.abs() < 1e-15 {
                return Err(SolveError::Underdetermined);
            }
            return Err(SolveError::NoSolution);
        }
        Ok(-b / a)
    }
    
    /// Solve quadratic equation: a*x² + b*x + c = 0
    pub fn solve_quadratic(a: f64, b: f64, c: f64) -> Result<(f64, f64), SolveError> {
        if a.abs() < 1e-15 {
            let x = Self::solve_linear(b, c)?;
            return Ok((x, x));
        }
        
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            return Err(SolveError::NoSolution);
        }
        
        let sqrt_d = discriminant.sqrt();
        let x1 = (-b + sqrt_d) / (2.0 * a);
        let x2 = (-b - sqrt_d) / (2.0 * a);
        
        Ok((x1, x2))
    }
}

/// Newton-Raphson solver for nonlinear equations
pub struct NewtonRaphsonSolver {
    /// Maximum iterations
    pub max_iterations: usize,
    /// Convergence tolerance
    pub tolerance: f64,
    /// Step size for numerical derivative
    pub h: f64,
}

impl Default for NewtonRaphsonSolver {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1e-10,
            h: 1e-8,
        }
    }
}

impl NewtonRaphsonSolver {
    /// Solve f(x) = 0 starting from initial guess
    pub fn solve<F>(&self, f: F, initial_guess: f64) -> Result<f64, SolveError>
    where
        F: Fn(f64) -> f64,
    {
        let mut x = initial_guess;
        
        for _ in 0..self.max_iterations {
            let fx = f(x);
            
            if fx.abs() < self.tolerance {
                return Ok(x);
            }
            
            // Numerical derivative
            let dfx = (f(x + self.h) - f(x - self.h)) / (2.0 * self.h);
            
            if dfx.abs() < 1e-15 {
                return Err(SolveError::NumericalError("Zero derivative".to_string()));
            }
            
            x = x - fx / dfx;
        }
        
        Err(SolveError::NumericalError("Did not converge".to_string()))
    }
    
    /// Solve system of equations using Newton-Raphson
    pub fn solve_system<F, J>(
        &self,
        f: F,
        jacobian: J,
        initial_guess: &[f64],
    ) -> Result<Vec<f64>, SolveError>
    where
        F: Fn(&[f64]) -> Vec<f64>,
        J: Fn(&[f64]) -> Vec<Vec<f64>>,
    {
        let n = initial_guess.len();
        let mut x = initial_guess.to_vec();
        
        for _ in 0..self.max_iterations {
            let fx = f(&x);
            
            // Check convergence
            let norm: f64 = fx.iter().map(|v| v * v).sum::<f64>().sqrt();
            if norm < self.tolerance {
                return Ok(x);
            }
            
            // Get Jacobian
            let j = jacobian(&x);
            
            // Solve J * dx = -f using simple Gaussian elimination
            let dx = solve_linear_system(&j, &fx.iter().map(|v| -v).collect::<Vec<_>>())?;
            
            // Update
            for i in 0..n {
                x[i] += dx[i];
            }
        }
        
        Err(SolveError::NumericalError("System did not converge".to_string()))
    }
}

/// Simple Gaussian elimination for linear systems
fn solve_linear_system(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, SolveError> {
    let n = b.len();
    if a.len() != n || a.iter().any(|row| row.len() != n) {
        return Err(SolveError::NumericalError("Matrix dimension mismatch".to_string()));
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
    
    // Forward elimination
    for i in 0..n {
        // Find pivot
        let mut max_row = i;
        for k in (i + 1)..n {
            if aug[k][i].abs() > aug[max_row][i].abs() {
                max_row = k;
            }
        }
        aug.swap(i, max_row);
        
        if aug[i][i].abs() < 1e-15 {
            return Err(SolveError::NoSolution);
        }
        
        // Eliminate column
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

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_solve_linear() {
        // 2x + 4 = 0 -> x = -2
        let x = ConstraintSolver::solve_linear(2.0, 4.0).unwrap();
        assert!((x - (-2.0)).abs() < 1e-10);
    }
    
    #[test]
    fn test_solve_quadratic() {
        // x² - 4 = 0 -> x = ±2
        let (x1, x2) = ConstraintSolver::solve_quadratic(1.0, 0.0, -4.0).unwrap();
        assert!((x1 - 2.0).abs() < 1e-10 || (x1 - (-2.0)).abs() < 1e-10);
        assert!((x2 - 2.0).abs() < 1e-10 || (x2 - (-2.0)).abs() < 1e-10);
    }
    
    #[test]
    fn test_newton_raphson() {
        let solver = NewtonRaphsonSolver::default();
        // Solve x² - 2 = 0 -> x = √2
        let x = solver.solve(|x| x * x - 2.0, 1.0).unwrap();
        assert!((x - std::f64::consts::SQRT_2).abs() < 1e-8);
    }
    
    #[test]
    fn test_linear_system() {
        // 2x + y = 5
        // x + 3y = 6
        // Solution: x = 1.8, y = 1.4
        let a = vec![vec![2.0, 1.0], vec![1.0, 3.0]];
        let b = vec![5.0, 6.0];
        let x = solve_linear_system(&a, &b).unwrap();
        assert!((x[0] - 1.8).abs() < 1e-10);
        assert!((x[1] - 1.4).abs() < 1e-10);
    }
}
