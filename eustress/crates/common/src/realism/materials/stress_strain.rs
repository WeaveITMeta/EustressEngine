//! # Stress-Strain Calculations
//!
//! Stress and strain tensor calculations using Hooke's Law.
//!
//! ## Table of Contents
//!
//! 1. **StressTensor** - 3x3 symmetric stress tensor
//! 2. **StrainTensor** - 3x3 symmetric strain tensor
//! 3. **Hooke's Law** - Linear elastic stress-strain relationship
//! 4. **Invariants** - Von Mises, principal stresses

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::properties::MaterialProperties;

// ============================================================================
// Stress Tensor
// ============================================================================

/// 3x3 symmetric stress tensor (σ_ij)
/// 
/// Layout:
/// ```text
/// | σ_xx  τ_xy  τ_xz |
/// | τ_xy  σ_yy  τ_yz |
/// | τ_xz  τ_yz  σ_zz |
/// ```
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct StressTensor {
    /// Tensor components [row][col]
    pub components: [[f32; 3]; 3],
    /// Von Mises equivalent stress (cached)
    pub von_mises: f32,
    /// Principal stresses (σ₁ ≥ σ₂ ≥ σ₃)
    pub principal: [f32; 3],
    /// Hydrostatic stress (mean normal stress)
    pub hydrostatic: f32,
    /// Maximum shear stress
    pub max_shear: f32,
}

impl StressTensor {
    /// Create from components
    pub fn from_components(components: [[f32; 3]; 3]) -> Self {
        let mut tensor = Self {
            components,
            ..default()
        };
        tensor.update_invariants();
        tensor
    }
    
    /// Create from normal stresses only (diagonal)
    pub fn from_normal(sigma_xx: f32, sigma_yy: f32, sigma_zz: f32) -> Self {
        Self::from_components([
            [sigma_xx, 0.0, 0.0],
            [0.0, sigma_yy, 0.0],
            [0.0, 0.0, sigma_zz],
        ])
    }
    
    /// Create hydrostatic stress state (equal in all directions)
    pub fn hydrostatic_state(pressure: f32) -> Self {
        Self::from_normal(-pressure, -pressure, -pressure)
    }
    
    /// Create uniaxial stress state
    pub fn uniaxial(stress: f32, direction: usize) -> Self {
        let mut components = [[0.0f32; 3]; 3];
        components[direction][direction] = stress;
        Self::from_components(components)
    }
    
    /// Get normal stress component
    pub fn normal(&self, i: usize) -> f32 {
        self.components[i][i]
    }
    
    /// Get shear stress component
    pub fn shear(&self, i: usize, j: usize) -> f32 {
        self.components[i][j]
    }
    
    /// Set component (maintains symmetry)
    pub fn set(&mut self, i: usize, j: usize, value: f32) {
        self.components[i][j] = value;
        self.components[j][i] = value;
    }
    
    /// Update cached invariants (von Mises, principal stresses, etc.)
    pub fn update_invariants(&mut self) {
        let s = &self.components;
        
        // Hydrostatic stress (mean)
        self.hydrostatic = (s[0][0] + s[1][1] + s[2][2]) / 3.0;
        
        // Von Mises stress
        let term1 = (s[0][0] - s[1][1]).powi(2) 
                  + (s[1][1] - s[2][2]).powi(2) 
                  + (s[2][2] - s[0][0]).powi(2);
        let term2 = 6.0 * (s[0][1].powi(2) + s[1][2].powi(2) + s[2][0].powi(2));
        self.von_mises = ((term1 + term2) / 2.0).sqrt();
        
        // Principal stresses (eigenvalues of stress tensor)
        // Using analytical solution for 3x3 symmetric matrix
        self.principal = self.compute_principal_stresses();
        
        // Maximum shear stress (Tresca criterion)
        self.max_shear = (self.principal[0] - self.principal[2]) / 2.0;
    }
    
    /// Compute principal stresses (eigenvalues)
    fn compute_principal_stresses(&self) -> [f32; 3] {
        let s = &self.components;
        
        // Invariants
        let i1 = s[0][0] + s[1][1] + s[2][2];
        let i2 = s[0][0] * s[1][1] + s[1][1] * s[2][2] + s[2][2] * s[0][0]
               - s[0][1].powi(2) - s[1][2].powi(2) - s[2][0].powi(2);
        let i3 = s[0][0] * s[1][1] * s[2][2]
               + 2.0 * s[0][1] * s[1][2] * s[2][0]
               - s[0][0] * s[1][2].powi(2)
               - s[1][1] * s[2][0].powi(2)
               - s[2][2] * s[0][1].powi(2);
        
        // Solve cubic equation: σ³ - I₁σ² + I₂σ - I₃ = 0
        // Using trigonometric solution for real roots
        let p = i2 - i1.powi(2) / 3.0;
        let q = 2.0 * i1.powi(3) / 27.0 - i1 * i2 / 3.0 + i3;
        
        let discriminant = q.powi(2) / 4.0 + p.powi(3) / 27.0;
        
        if discriminant < 0.0 {
            // Three real roots
            let r = (-p.powi(3) / 27.0).sqrt();
            let phi = (-q / (2.0 * r)).clamp(-1.0, 1.0).acos();
            let r_cbrt = r.cbrt();
            
            let mut roots = [
                2.0 * r_cbrt * (phi / 3.0).cos() + i1 / 3.0,
                2.0 * r_cbrt * ((phi + 2.0 * std::f32::consts::PI) / 3.0).cos() + i1 / 3.0,
                2.0 * r_cbrt * ((phi + 4.0 * std::f32::consts::PI) / 3.0).cos() + i1 / 3.0,
            ];
            
            // Sort descending
            roots.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            roots
        } else {
            // Degenerate case - use hydrostatic
            [self.hydrostatic; 3]
        }
    }
    
    /// Get deviatoric stress tensor (stress - hydrostatic)
    pub fn deviatoric(&self) -> [[f32; 3]; 3] {
        let mut dev = self.components;
        for i in 0..3 {
            dev[i][i] -= self.hydrostatic;
        }
        dev
    }
    
    /// Add another stress tensor
    pub fn add(&mut self, other: &StressTensor) {
        for i in 0..3 {
            for j in 0..3 {
                self.components[i][j] += other.components[i][j];
            }
        }
        self.update_invariants();
    }
    
    /// Scale stress tensor
    pub fn scale(&mut self, factor: f32) {
        for i in 0..3 {
            for j in 0..3 {
                self.components[i][j] *= factor;
            }
        }
        self.update_invariants();
    }
}

// ============================================================================
// Strain Tensor
// ============================================================================

/// 3x3 symmetric strain tensor (ε_ij)
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct StrainTensor {
    /// Tensor components [row][col]
    pub components: [[f32; 3]; 3],
    /// Volumetric strain (trace)
    pub volumetric: f32,
    /// Deviatoric strain magnitude
    pub deviatoric: f32,
    /// Principal strains
    pub principal: [f32; 3],
    /// Equivalent strain (von Mises equivalent)
    pub equivalent: f32,
}

impl StrainTensor {
    /// Create from components
    pub fn from_components(components: [[f32; 3]; 3]) -> Self {
        let mut tensor = Self {
            components,
            ..default()
        };
        tensor.update_invariants();
        tensor
    }
    
    /// Create from normal strains only
    pub fn from_normal(eps_xx: f32, eps_yy: f32, eps_zz: f32) -> Self {
        Self::from_components([
            [eps_xx, 0.0, 0.0],
            [0.0, eps_yy, 0.0],
            [0.0, 0.0, eps_zz],
        ])
    }
    
    /// Create uniaxial strain
    pub fn uniaxial(strain: f32, direction: usize) -> Self {
        let mut components = [[0.0f32; 3]; 3];
        components[direction][direction] = strain;
        Self::from_components(components)
    }
    
    /// Get normal strain component
    pub fn normal(&self, i: usize) -> f32 {
        self.components[i][i]
    }
    
    /// Get shear strain component (engineering shear = 2 * tensor shear)
    pub fn shear(&self, i: usize, j: usize) -> f32 {
        self.components[i][j]
    }
    
    /// Get engineering shear strain (γ = 2ε_ij)
    pub fn engineering_shear(&self, i: usize, j: usize) -> f32 {
        2.0 * self.components[i][j]
    }
    
    /// Set component (maintains symmetry)
    pub fn set(&mut self, i: usize, j: usize, value: f32) {
        self.components[i][j] = value;
        self.components[j][i] = value;
    }
    
    /// Update cached invariants
    pub fn update_invariants(&mut self) {
        let e = &self.components;
        
        // Volumetric strain (trace)
        self.volumetric = e[0][0] + e[1][1] + e[2][2];
        
        // Deviatoric strain
        let mean = self.volumetric / 3.0;
        let mut dev_sq = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                let dev_ij = if i == j { e[i][j] - mean } else { e[i][j] };
                dev_sq += dev_ij * dev_ij;
            }
        }
        self.deviatoric = (dev_sq / 2.0).sqrt();
        
        // Equivalent strain (von Mises equivalent)
        let term1 = (e[0][0] - e[1][1]).powi(2) 
                  + (e[1][1] - e[2][2]).powi(2) 
                  + (e[2][2] - e[0][0]).powi(2);
        let term2 = 6.0 * (e[0][1].powi(2) + e[1][2].powi(2) + e[2][0].powi(2));
        self.equivalent = (2.0 / 9.0 * (term1 + term2)).sqrt();
        
        // Principal strains (simplified - same method as stress)
        self.principal = [e[0][0], e[1][1], e[2][2]]; // Simplified for diagonal-dominant
        self.principal.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    }
    
    /// Calculate strain from displacement gradient
    /// ε_ij = (∂u_i/∂x_j + ∂u_j/∂x_i) / 2
    pub fn from_displacement_gradient(grad_u: [[f32; 3]; 3]) -> Self {
        let mut components = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                components[i][j] = (grad_u[i][j] + grad_u[j][i]) / 2.0;
            }
        }
        Self::from_components(components)
    }
}

// ============================================================================
// Hooke's Law
// ============================================================================

/// Calculate stress from strain using generalized Hooke's Law (3D)
/// σ = λ·tr(ε)·I + 2μ·ε
pub fn hookes_law_3d(strain: &StrainTensor, material: &MaterialProperties) -> StressTensor {
    let lambda = material.lame_lambda();
    let mu = material.lame_mu();
    
    let trace = strain.volumetric;
    let mut stress = [[0.0f32; 3]; 3];
    
    for i in 0..3 {
        for j in 0..3 {
            stress[i][j] = 2.0 * mu * strain.components[i][j];
            if i == j {
                stress[i][j] += lambda * trace;
            }
        }
    }
    
    StressTensor::from_components(stress)
}

/// Calculate strain from stress using inverse Hooke's Law
/// ε = (1+ν)/E · σ - ν/E · tr(σ)·I
pub fn inverse_hookes_law_3d(stress: &StressTensor, material: &MaterialProperties) -> StrainTensor {
    let e = material.young_modulus;
    let nu = material.poisson_ratio;
    
    let trace = stress.components[0][0] + stress.components[1][1] + stress.components[2][2];
    let mut strain = [[0.0f32; 3]; 3];
    
    for i in 0..3 {
        for j in 0..3 {
            strain[i][j] = (1.0 + nu) / e * stress.components[i][j];
            if i == j {
                strain[i][j] -= nu / e * trace;
            }
        }
    }
    
    StrainTensor::from_components(strain)
}

/// Calculate stress for plane stress condition (σ_zz = 0)
pub fn plane_stress(eps_xx: f32, eps_yy: f32, gamma_xy: f32, material: &MaterialProperties) -> StressTensor {
    let e = material.young_modulus;
    let nu = material.poisson_ratio;
    let factor = e / (1.0 - nu * nu);
    
    let sigma_xx = factor * (eps_xx + nu * eps_yy);
    let sigma_yy = factor * (eps_yy + nu * eps_xx);
    let tau_xy = material.shear_modulus() * gamma_xy;
    
    StressTensor::from_components([
        [sigma_xx, tau_xy, 0.0],
        [tau_xy, sigma_yy, 0.0],
        [0.0, 0.0, 0.0],
    ])
}

/// Calculate stress for plane strain condition (ε_zz = 0)
pub fn plane_strain(eps_xx: f32, eps_yy: f32, gamma_xy: f32, material: &MaterialProperties) -> StressTensor {
    let lambda = material.lame_lambda();
    let mu = material.lame_mu();
    
    let sigma_xx = (lambda + 2.0 * mu) * eps_xx + lambda * eps_yy;
    let sigma_yy = lambda * eps_xx + (lambda + 2.0 * mu) * eps_yy;
    let sigma_zz = lambda * (eps_xx + eps_yy);
    let tau_xy = mu * gamma_xy;
    
    StressTensor::from_components([
        [sigma_xx, tau_xy, 0.0],
        [tau_xy, sigma_yy, 0.0],
        [0.0, 0.0, sigma_zz],
    ])
}

// ============================================================================
// Yield Criteria
// ============================================================================

/// Check von Mises yield criterion: σ_vm ≥ σ_y
pub fn check_von_mises_yield(stress: &StressTensor, yield_strength: f32) -> bool {
    stress.von_mises >= yield_strength
}

/// Check Tresca yield criterion: τ_max ≥ σ_y/2
pub fn check_tresca_yield(stress: &StressTensor, yield_strength: f32) -> bool {
    stress.max_shear >= yield_strength / 2.0
}

/// Safety factor (von Mises)
pub fn safety_factor_von_mises(stress: &StressTensor, yield_strength: f32) -> f32 {
    if stress.von_mises > 0.0 {
        yield_strength / stress.von_mises
    } else {
        f32::INFINITY
    }
}

// ============================================================================
// System
// ============================================================================

/// Update stress from strain for entities with both components
pub fn update_stress_strain(
    mut query: Query<(&StrainTensor, &mut StressTensor, &MaterialProperties), Changed<StrainTensor>>,
) {
    for (strain, mut stress, material) in query.iter_mut() {
        *stress = hookes_law_3d(strain, material);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_von_mises_uniaxial() {
        // Uniaxial stress should give von Mises = |σ|
        let stress = StressTensor::uniaxial(100e6, 0);
        assert!((stress.von_mises - 100e6).abs() < 1e3);
    }
    
    #[test]
    fn test_hydrostatic_von_mises() {
        // Pure hydrostatic stress should give von Mises = 0
        let stress = StressTensor::hydrostatic_state(100e6);
        assert!(stress.von_mises < 1e3);
    }
    
    #[test]
    fn test_hookes_law_roundtrip() {
        let material = MaterialProperties::steel();
        let strain = StrainTensor::from_normal(0.001, 0.0005, -0.0003);
        let stress = hookes_law_3d(&strain, &material);
        let strain_back = inverse_hookes_law_3d(&stress, &material);
        
        for i in 0..3 {
            assert!((strain.components[i][i] - strain_back.components[i][i]).abs() < 1e-9);
        }
    }
}
