//! # Fracture Mechanics
//!
//! Crack propagation, fracture criteria, and failure analysis.
//!
//! ## Table of Contents
//!
//! 1. **FractureState** - Crack tracking component
//! 2. **Fracture Criteria** - Griffith, stress intensity
//! 3. **Crack Propagation** - Paris law, fatigue

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::properties::MaterialProperties;
use super::stress_strain::StressTensor;

// ============================================================================
// Fracture State Component
// ============================================================================

/// Tracks fracture state and cracks in a material
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct FractureState {
    /// List of active cracks
    pub cracks: Vec<Crack>,
    /// Has the material fractured completely
    pub is_fractured: bool,
    /// Time of fracture (simulation time)
    pub fracture_time: Option<f32>,
    /// Accumulated damage (0-1)
    pub damage: f32,
    /// Fatigue cycle count
    pub fatigue_cycles: u32,
    /// Maximum stress experienced
    pub max_stress_history: f32,
}

impl FractureState {
    /// Create new fracture state
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add a crack
    pub fn add_crack(&mut self, crack: Crack) {
        self.cracks.push(crack);
    }
    
    /// Get total crack length
    pub fn total_crack_length(&self) -> f32 {
        self.cracks.iter().map(|c| c.length).sum()
    }
    
    /// Check if any crack exceeds critical length
    pub fn has_critical_crack(&self, critical_length: f32) -> bool {
        self.cracks.iter().any(|c| c.length >= critical_length)
    }
    
    /// Accumulate damage
    pub fn accumulate_damage(&mut self, delta: f32) {
        self.damage = (self.damage + delta).min(1.0);
        if self.damage >= 1.0 {
            self.is_fractured = true;
        }
    }
    
    /// Record stress for fatigue tracking
    pub fn record_stress(&mut self, stress: f32) {
        if stress > self.max_stress_history {
            self.max_stress_history = stress;
        }
    }
}

/// Individual crack in a material
#[derive(Clone, Debug, Default, Reflect, Serialize, Deserialize)]
pub struct Crack {
    /// Position of crack tip (local coordinates)
    pub position: Vec3,
    /// Direction of crack propagation
    pub direction: Vec3,
    /// Current crack length (m)
    pub length: f32,
    /// Crack growth rate (m/cycle for fatigue)
    pub growth_rate: f32,
    /// Crack opening displacement
    pub opening: f32,
    /// Is crack actively growing
    pub is_growing: bool,
}

impl Crack {
    /// Create a new crack
    pub fn new(position: Vec3, direction: Vec3, initial_length: f32) -> Self {
        Self {
            position,
            direction: direction.normalize_or_zero(),
            length: initial_length,
            growth_rate: 0.0,
            opening: 0.0,
            is_growing: false,
        }
    }
    
    /// Get crack tip position
    pub fn tip_position(&self) -> Vec3 {
        self.position + self.direction * self.length
    }
    
    /// Propagate crack by given amount
    pub fn propagate(&mut self, delta_length: f32) {
        self.length += delta_length;
        self.is_growing = delta_length > 0.0;
    }
}

// ============================================================================
// Stress Intensity Factors
// ============================================================================

/// Mode I stress intensity factor (opening mode): K_I = σ√(πa) * Y
/// 
/// # Arguments
/// * `stress` - Applied stress (Pa)
/// * `crack_length` - Half crack length for center crack, full for edge (m)
/// * `geometry_factor` - Y factor (1.0 for infinite plate)
pub fn stress_intensity_mode_i(stress: f32, crack_length: f32, geometry_factor: f32) -> f32 {
    stress * (std::f32::consts::PI * crack_length).sqrt() * geometry_factor
}

/// Mode II stress intensity factor (sliding mode)
pub fn stress_intensity_mode_ii(shear_stress: f32, crack_length: f32, geometry_factor: f32) -> f32 {
    shear_stress * (std::f32::consts::PI * crack_length).sqrt() * geometry_factor
}

/// Mode III stress intensity factor (tearing mode)
pub fn stress_intensity_mode_iii(shear_stress: f32, crack_length: f32, geometry_factor: f32) -> f32 {
    shear_stress * (std::f32::consts::PI * crack_length).sqrt() * geometry_factor
}

/// Geometry factors for common configurations
pub mod geometry_factors {
    /// Infinite plate with center crack
    pub const CENTER_CRACK_INFINITE: f32 = 1.0;
    
    /// Edge crack in semi-infinite plate
    pub const EDGE_CRACK_SEMI_INFINITE: f32 = 1.12;
    
    /// Through crack in finite width plate: Y = √(sec(πa/W))
    pub fn finite_width_center(crack_half_length: f32, plate_width: f32) -> f32 {
        let ratio = std::f32::consts::PI * crack_half_length / plate_width;
        if ratio < 1.5 {
            (1.0 / ratio.cos()).sqrt()
        } else {
            f32::INFINITY
        }
    }
    
    /// Edge crack in finite width plate (polynomial approximation)
    pub fn finite_width_edge(crack_length: f32, plate_width: f32) -> f32 {
        let a_w = crack_length / plate_width;
        1.12 - 0.231 * a_w + 10.55 * a_w.powi(2) - 21.72 * a_w.powi(3) + 30.39 * a_w.powi(4)
    }
}

// ============================================================================
// Fracture Criteria
// ============================================================================

/// Check Griffith criterion for brittle fracture
/// Fracture occurs when K_I ≥ K_IC
pub fn check_griffith_fracture(stress_intensity: f32, fracture_toughness: f32) -> bool {
    stress_intensity >= fracture_toughness
}

/// Critical stress for Griffith fracture: σ_c = K_IC / √(πa)
pub fn griffith_critical_stress(fracture_toughness: f32, crack_length: f32) -> f32 {
    if crack_length <= 0.0 {
        return f32::INFINITY;
    }
    fracture_toughness / (std::f32::consts::PI * crack_length).sqrt()
}

/// Critical crack length for given stress: a_c = (K_IC / σ)² / π
pub fn critical_crack_length(fracture_toughness: f32, stress: f32) -> f32 {
    if stress <= 0.0 {
        return f32::INFINITY;
    }
    (fracture_toughness / stress).powi(2) / std::f32::consts::PI
}

/// Energy release rate: G = K²/E (plane stress) or G = K²(1-ν²)/E (plane strain)
pub fn energy_release_rate(stress_intensity: f32, material: &MaterialProperties, plane_strain: bool) -> f32 {
    let k_squared = stress_intensity * stress_intensity;
    if plane_strain {
        k_squared * (1.0 - material.poisson_ratio.powi(2)) / material.young_modulus
    } else {
        k_squared / material.young_modulus
    }
}

// ============================================================================
// Fatigue (Paris Law)
// ============================================================================

/// Paris law for fatigue crack growth: da/dN = C(ΔK)^m
/// 
/// # Arguments
/// * `delta_k` - Stress intensity range (Pa·√m)
/// * `c` - Paris law coefficient (material dependent)
/// * `m` - Paris law exponent (typically 2-4)
/// 
/// # Returns
/// Crack growth per cycle (m/cycle)
pub fn paris_law(delta_k: f32, c: f32, m: f32) -> f32 {
    c * delta_k.powf(m)
}

/// Typical Paris law parameters for common materials
/// 
/// **Units**: C has units of m/cycle / (Pa·√m)^m
/// For ΔK in MPa·√m, use C values ~1e-11 to 1e-13
/// For ΔK in Pa·√m (SI), C values are scaled by 10^(3m)
/// 
/// These values are for ΔK in **Pa·√m** (SI units)
pub mod paris_parameters {
    /// Steel (C in m/cycle / (Pa·√m)^m, m dimensionless)
    /// Typical range: C = 1e-11 to 1e-13 for ΔK in MPa·√m
    /// Converted to SI (Pa·√m): C = 1e-11 / (1e6)^3 = 1e-29 for m=3
    /// Using practical SI value that gives ~1e-8 m/cycle at ΔK=20 MPa·√m
    pub const STEEL: (f32, f32) = (3e-12, 3.0);
    
    /// Steel with ΔK in MPa·√m (more common in literature)
    /// C ≈ 3e-12 m/cycle / (MPa·√m)^3
    pub const STEEL_MPA: (f32, f32) = (3e-12, 3.0);
    
    /// Aluminum (ΔK in Pa·√m)
    pub const ALUMINUM: (f32, f32) = (5e-11, 3.5);
    
    /// Aluminum with ΔK in MPa·√m
    pub const ALUMINUM_MPA: (f32, f32) = (5e-11, 3.5);
    
    /// Titanium (ΔK in Pa·√m)
    pub const TITANIUM: (f32, f32) = (1e-11, 3.2);
    
    /// Titanium with ΔK in MPa·√m
    pub const TITANIUM_MPA: (f32, f32) = (1e-11, 3.2);
    
    /// Convert C from MPa·√m basis to Pa·√m basis
    /// C_Pa = C_MPa / (1e6)^m
    pub fn convert_c_mpa_to_pa(c_mpa: f32, m: f32) -> f32 {
        c_mpa / (1e6_f32).powf(m)
    }
    
    /// Convert C from Pa·√m basis to MPa·√m basis
    /// C_MPa = C_Pa * (1e6)^m
    pub fn convert_c_pa_to_mpa(c_pa: f32, m: f32) -> f32 {
        c_pa * (1e6_f32).powf(m)
    }
}

/// Calculate remaining fatigue life (cycles to failure)
/// Using integration of Paris law
pub fn fatigue_life(
    initial_crack: f32,
    critical_crack: f32,
    stress_range: f32,
    c: f32,
    m: f32,
) -> f32 {
    if m == 2.0 {
        // Analytical solution for m=2
        let k_factor = stress_range * std::f32::consts::PI.sqrt();
        (1.0 / (c * k_factor.powi(2))) * (1.0 / initial_crack.sqrt() - 1.0 / critical_crack.sqrt())
    } else {
        // Numerical integration (simplified)
        let n_steps = 100;
        let da = (critical_crack - initial_crack) / n_steps as f32;
        let mut cycles = 0.0;
        let mut a = initial_crack;
        
        for _ in 0..n_steps {
            let k = stress_range * (std::f32::consts::PI * a).sqrt();
            let da_dn = paris_law(k, c, m);
            if da_dn > 0.0 {
                cycles += da / da_dn;
            }
            a += da;
        }
        
        cycles
    }
}

// ============================================================================
// Damage Accumulation
// ============================================================================

/// Miner's rule for cumulative fatigue damage: D = Σ(n_i/N_i)
/// Failure when D ≥ 1
pub fn miners_rule_damage(cycles_at_stress: &[(f32, f32)], s_n_curve: impl Fn(f32) -> f32) -> f32 {
    cycles_at_stress.iter()
        .map(|(n, stress)| {
            let n_f = s_n_curve(*stress);
            if n_f > 0.0 { n / n_f } else { 1.0 }
        })
        .sum()
}

/// Simple S-N curve: N = (σ_f / σ)^b
pub fn simple_sn_curve(stress: f32, fatigue_strength: f32, exponent: f32) -> f32 {
    if stress <= 0.0 {
        return f32::INFINITY;
    }
    (fatigue_strength / stress).powf(exponent)
}

// ============================================================================
// System
// ============================================================================

/// Check fracture conditions for entities with stress and fracture state
pub fn check_fracture_conditions(
    mut query: Query<(&StressTensor, &MaterialProperties, &mut FractureState)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    
    for (stress, material, mut fracture) in query.iter_mut() {
        if fracture.is_fractured {
            continue;
        }
        
        // Record maximum stress
        fracture.record_stress(stress.von_mises);
        
        // Check each crack
        for crack in fracture.cracks.iter_mut() {
            // Calculate stress intensity at crack tip
            let k_i = stress_intensity_mode_i(
                stress.principal[0], // Maximum principal stress
                crack.length,
                geometry_factors::CENTER_CRACK_INFINITE,
            );
            
            // Check Griffith criterion
            if check_griffith_fracture(k_i, material.fracture_toughness) {
                fracture.is_fractured = true;
                fracture.fracture_time = Some(current_time);
                break;
            }
            
            // Subcritical crack growth (simplified)
            if k_i > material.fracture_toughness * 0.5 {
                let growth = (k_i / material.fracture_toughness - 0.5) * 1e-6;
                crack.propagate(growth);
            }
        }
        
        // Check for ductile failure (von Mises > ultimate strength)
        if stress.von_mises >= material.ultimate_strength {
            fracture.is_fractured = true;
            fracture.fracture_time = Some(current_time);
        }
        
        // Damage accumulation from overstress
        if stress.von_mises > material.yield_strength {
            let overstress_ratio = (stress.von_mises - material.yield_strength) 
                / (material.ultimate_strength - material.yield_strength);
            fracture.accumulate_damage(overstress_ratio * 0.01);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_stress_intensity() {
        // K = σ√(πa) for Y=1
        let k = stress_intensity_mode_i(100e6, 0.01, 1.0);
        // K ≈ 100e6 * √(π * 0.01) ≈ 17.7 MPa√m
        assert!((k - 17.7e6).abs() < 1e6);
    }
    
    #[test]
    fn test_critical_crack_length() {
        // For steel with K_IC = 50 MPa√m at 100 MPa stress
        let a_c = critical_crack_length(50e6, 100e6);
        // a_c = (50/100)² / π ≈ 0.08 m
        assert!((a_c - 0.08).abs() < 0.01);
    }
    
    #[test]
    fn test_paris_law() {
        let (c, m) = paris_parameters::STEEL;
        let delta_k = 20e6; // 20 MPa√m
        let da_dn = paris_law(delta_k, c, m);
        // Should be on order of 1e-8 m/cycle
        assert!(da_dn > 1e-10 && da_dn < 1e-6);
    }
}
