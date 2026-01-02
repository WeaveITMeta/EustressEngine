//! # Aerodynamics
//!
//! Lift, drag, and aerodynamic forces.
//!
//! ## Table of Contents
//!
//! 1. **AerodynamicBody** - Aerodynamic properties component
//! 2. **Drag** - Drag force calculations
//! 3. **Lift** - Lift force calculations
//! 4. **Reynolds Number** - Flow regime classification

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::realism::constants;
use crate::realism::particles::components::KineticState;

// ============================================================================
// Aerodynamic Body Component
// ============================================================================

/// Aerodynamic properties for a body
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct AerodynamicBody {
    /// Drag coefficient (C_d)
    pub drag_coefficient: f32,
    /// Lift coefficient (C_l)
    pub lift_coefficient: f32,
    /// Reference area for drag (m²)
    pub drag_area: f32,
    /// Reference area for lift (m²)
    pub lift_area: f32,
    /// Center of pressure offset from center of mass
    pub center_of_pressure: Vec3,
    /// Lift direction (local, typically up)
    pub lift_direction: Vec3,
    /// Enable aerodynamic forces
    pub enabled: bool,
}

impl Default for AerodynamicBody {
    fn default() -> Self {
        Self {
            drag_coefficient: 0.47, // Sphere
            lift_coefficient: 0.0,
            drag_area: 1.0,
            lift_area: 1.0,
            center_of_pressure: Vec3::ZERO,
            lift_direction: Vec3::Y,
            enabled: true,
        }
    }
}

impl AerodynamicBody {
    /// Create sphere aerodynamics
    pub fn sphere(radius: f32) -> Self {
        let area = std::f32::consts::PI * radius * radius;
        Self {
            drag_coefficient: 0.47,
            lift_coefficient: 0.0,
            drag_area: area,
            lift_area: area,
            ..default()
        }
    }
    
    /// Create cube aerodynamics
    pub fn cube(side: f32) -> Self {
        let area = side * side;
        Self {
            drag_coefficient: 1.05,
            lift_coefficient: 0.0,
            drag_area: area,
            lift_area: area,
            ..default()
        }
    }
    
    /// Create cylinder aerodynamics (flow perpendicular to axis)
    pub fn cylinder(radius: f32, length: f32) -> Self {
        Self {
            drag_coefficient: 0.82,
            lift_coefficient: 0.0,
            drag_area: 2.0 * radius * length,
            lift_area: 2.0 * radius * length,
            ..default()
        }
    }
    
    /// Create flat plate aerodynamics (perpendicular to flow)
    pub fn flat_plate(width: f32, height: f32) -> Self {
        Self {
            drag_coefficient: 1.28,
            lift_coefficient: 0.0,
            drag_area: width * height,
            lift_area: width * height,
            ..default()
        }
    }
    
    /// Create streamlined body (teardrop)
    pub fn streamlined(frontal_area: f32) -> Self {
        Self {
            drag_coefficient: 0.04,
            lift_coefficient: 0.0,
            drag_area: frontal_area,
            lift_area: frontal_area,
            ..default()
        }
    }
    
    /// Create airfoil aerodynamics
    pub fn airfoil(chord: f32, span: f32, angle_of_attack: f32) -> Self {
        // Simplified thin airfoil theory: C_l ≈ 2π * α
        let cl = 2.0 * std::f32::consts::PI * angle_of_attack;
        // Induced drag: C_d ≈ C_l² / (π * AR)
        let aspect_ratio = span / chord;
        let cd = cl * cl / (std::f32::consts::PI * aspect_ratio) + 0.01; // + parasitic
        
        Self {
            drag_coefficient: cd,
            lift_coefficient: cl,
            drag_area: chord * span,
            lift_area: chord * span,
            lift_direction: Vec3::Y,
            ..default()
        }
    }
    
    /// Create car aerodynamics
    pub fn car(frontal_area: f32) -> Self {
        Self {
            drag_coefficient: 0.3, // Modern car
            lift_coefficient: 0.1, // Slight lift
            drag_area: frontal_area,
            lift_area: frontal_area * 2.0,
            ..default()
        }
    }
    
    /// Create human aerodynamics (standing)
    pub fn human_standing() -> Self {
        Self {
            drag_coefficient: 1.0,
            lift_coefficient: 0.0,
            drag_area: 0.7, // ~0.7 m² frontal area
            lift_area: 0.7,
            ..default()
        }
    }
    
    /// Create parachute aerodynamics
    pub fn parachute(diameter: f32) -> Self {
        let area = std::f32::consts::PI * (diameter / 2.0).powi(2);
        Self {
            drag_coefficient: 1.5, // Hemispherical parachute
            lift_coefficient: 0.0,
            drag_area: area,
            lift_area: area,
            ..default()
        }
    }
}

// ============================================================================
// Drag Calculations
// ============================================================================

/// Calculate drag force: F_d = ½ρv²C_dA
/// 
/// # Arguments
/// * `density` - Fluid density (kg/m³)
/// * `velocity` - Relative velocity (m/s)
/// * `drag_coefficient` - Drag coefficient
/// * `area` - Reference area (m²)
/// 
/// # Returns
/// Drag force vector (N), opposite to velocity
pub fn drag_force(density: f32, velocity: Vec3, drag_coefficient: f32, area: f32) -> Vec3 {
    let speed = velocity.length();
    if speed < 1e-6 {
        return Vec3::ZERO;
    }
    
    let magnitude = 0.5 * density * speed * speed * drag_coefficient * area;
    -velocity.normalize() * magnitude
}

/// Calculate drag force magnitude
pub fn drag_force_magnitude(density: f32, speed: f32, drag_coefficient: f32, area: f32) -> f32 {
    0.5 * density * speed * speed * drag_coefficient * area
}

/// Dynamic pressure: q = ½ρv²
#[inline]
pub fn dynamic_pressure(density: f32, speed: f32) -> f32 {
    0.5 * density * speed * speed
}

// ============================================================================
// Lift Calculations
// ============================================================================

/// Calculate lift force: F_l = ½ρv²C_lA
/// 
/// # Arguments
/// * `density` - Fluid density (kg/m³)
/// * `velocity` - Relative velocity (m/s)
/// * `lift_coefficient` - Lift coefficient
/// * `area` - Reference area (m²)
/// * `lift_direction` - Direction of lift (world space, normalized)
/// 
/// # Returns
/// Lift force vector (N)
pub fn lift_force(
    density: f32,
    velocity: Vec3,
    lift_coefficient: f32,
    area: f32,
    lift_direction: Vec3,
) -> Vec3 {
    let speed = velocity.length();
    if speed < 1e-6 {
        return Vec3::ZERO;
    }
    
    let magnitude = 0.5 * density * speed * speed * lift_coefficient * area;
    lift_direction.normalize() * magnitude
}

/// Calculate lift coefficient from angle of attack (thin airfoil theory)
/// C_l ≈ 2π * sin(α) for small angles
pub fn lift_coefficient_thin_airfoil(angle_of_attack: f32) -> f32 {
    2.0 * std::f32::consts::PI * angle_of_attack.sin()
}

/// Calculate lift coefficient with stall (simplified)
pub fn lift_coefficient_with_stall(angle_of_attack: f32, stall_angle: f32) -> f32 {
    let alpha = angle_of_attack.abs();
    
    if alpha < stall_angle {
        // Linear region
        2.0 * std::f32::consts::PI * angle_of_attack.sin()
    } else {
        // Post-stall (simplified)
        let cl_max = 2.0 * std::f32::consts::PI * stall_angle.sin();
        let decay = ((alpha - stall_angle) / (std::f32::consts::FRAC_PI_2 - stall_angle)).min(1.0);
        cl_max * (1.0 - decay * 0.5) * angle_of_attack.signum()
    }
}

// ============================================================================
// Reynolds Number
// ============================================================================

/// Calculate Reynolds number: Re = ρvL/μ
/// 
/// # Arguments
/// * `density` - Fluid density (kg/m³)
/// * `velocity` - Flow velocity (m/s)
/// * `characteristic_length` - Characteristic length (m)
/// * `dynamic_viscosity` - Dynamic viscosity (Pa·s)
pub fn reynolds_number(
    density: f32,
    velocity: f32,
    characteristic_length: f32,
    dynamic_viscosity: f32,
) -> f32 {
    if dynamic_viscosity < 1e-10 {
        return f32::INFINITY;
    }
    density * velocity * characteristic_length / dynamic_viscosity
}

/// Reynolds number using kinematic viscosity: Re = vL/ν
pub fn reynolds_number_kinematic(
    velocity: f32,
    characteristic_length: f32,
    kinematic_viscosity: f32,
) -> f32 {
    if kinematic_viscosity < 1e-10 {
        return f32::INFINITY;
    }
    velocity * characteristic_length / kinematic_viscosity
}

/// Flow regime based on Reynolds number
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowRegime {
    /// Re < 2300 (pipe) or Re < 5×10⁵ (flat plate)
    Laminar,
    /// Transitional flow
    Transitional,
    /// Fully turbulent
    Turbulent,
}

/// Determine flow regime for pipe flow
pub fn pipe_flow_regime(reynolds: f32) -> FlowRegime {
    if reynolds < 2300.0 {
        FlowRegime::Laminar
    } else if reynolds < 4000.0 {
        FlowRegime::Transitional
    } else {
        FlowRegime::Turbulent
    }
}

/// Determine flow regime for external flow (flat plate)
pub fn external_flow_regime(reynolds: f32) -> FlowRegime {
    if reynolds < 5e5 {
        FlowRegime::Laminar
    } else if reynolds < 1e6 {
        FlowRegime::Transitional
    } else {
        FlowRegime::Turbulent
    }
}

// ============================================================================
// Drag Coefficient Correlations
// ============================================================================

/// Sphere drag coefficient based on Reynolds number
pub fn sphere_drag_coefficient(reynolds: f32) -> f32 {
    if reynolds < 1.0 {
        // Stokes regime: C_d = 24/Re
        24.0 / reynolds.max(0.01)
    } else if reynolds < 1000.0 {
        // Intermediate: C_d ≈ 24/Re + 6/(1+√Re) + 0.4
        24.0 / reynolds + 6.0 / (1.0 + reynolds.sqrt()) + 0.4
    } else if reynolds < 2e5 {
        // Newton regime: C_d ≈ 0.44
        0.44
    } else {
        // Drag crisis: C_d drops
        0.1
    }
}

/// Cylinder drag coefficient based on Reynolds number
pub fn cylinder_drag_coefficient(reynolds: f32) -> f32 {
    if reynolds < 1.0 {
        8.0 * std::f32::consts::PI / (reynolds.max(0.01) * (2.0 - reynolds.max(0.01).ln()))
    } else if reynolds < 1000.0 {
        1.0 + 10.0 / reynolds.powf(2.0 / 3.0)
    } else if reynolds < 2e5 {
        1.2
    } else {
        0.3
    }
}

// ============================================================================
// Terminal Velocity
// ============================================================================

/// Calculate terminal velocity: v_t = √(2mg / (ρC_dA))
pub fn terminal_velocity(
    mass: f32,
    gravity: f32,
    density: f32,
    drag_coefficient: f32,
    area: f32,
) -> f32 {
    let denom = density * drag_coefficient * area;
    if denom < 1e-10 {
        return f32::INFINITY;
    }
    (2.0 * mass * gravity / denom).sqrt()
}

// ============================================================================
// System
// ============================================================================

/// Apply aerodynamic forces to bodies
pub fn apply_aerodynamic_forces(
    mut query: Query<(&AerodynamicBody, &mut KineticState, &Transform)>,
) {
    let air_density = constants::AIR_DENSITY_SEA_LEVEL;
    
    for (aero, mut kinetic, transform) in query.iter_mut() {
        if !aero.enabled {
            continue;
        }
        
        let velocity = kinetic.velocity;
        let speed = velocity.length();
        
        if speed < 0.01 {
            continue;
        }
        
        // Drag force
        let f_drag = drag_force(air_density, velocity, aero.drag_coefficient, aero.drag_area);
        kinetic.apply_force(f_drag);
        
        // Lift force
        if aero.lift_coefficient.abs() > 1e-6 {
            let lift_dir = transform.rotation * aero.lift_direction;
            let f_lift = lift_force(air_density, velocity, aero.lift_coefficient, aero.lift_area, lift_dir);
            kinetic.apply_force(f_lift);
            
            // Torque from offset center of pressure
            if aero.center_of_pressure.length_squared() > 1e-6 {
                let cop_world = transform.rotation * aero.center_of_pressure;
                let torque = cop_world.cross(f_lift + f_drag);
                kinetic.apply_torque(torque);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_drag_force() {
        // Car at 30 m/s (108 km/h)
        let f = drag_force(1.225, Vec3::new(30.0, 0.0, 0.0), 0.3, 2.0);
        // F = 0.5 * 1.225 * 900 * 0.3 * 2 = 330.75 N
        assert!((f.length() - 330.75).abs() < 1.0);
        assert!(f.x < 0.0); // Opposite to velocity
    }
    
    #[test]
    fn test_reynolds_number() {
        // Air flow over 1m plate at 10 m/s
        let re = reynolds_number(1.225, 10.0, 1.0, 1.81e-5);
        // Re ≈ 677000
        assert!((re - 677000.0).abs() < 10000.0);
    }
    
    #[test]
    fn test_terminal_velocity() {
        // Skydiver: 80kg, Cd=1.0, A=0.7m²
        let vt = terminal_velocity(80.0, 9.81, 1.225, 1.0, 0.7);
        // v_t ≈ 48 m/s
        assert!((vt - 48.0).abs() < 5.0);
    }
    
    #[test]
    fn test_sphere_drag_coefficient() {
        // High Re sphere should have Cd ≈ 0.44
        let cd = sphere_drag_coefficient(10000.0);
        assert!((cd - 0.44).abs() < 0.1);
        
        // Low Re (Stokes) should have high Cd
        let cd_low = sphere_drag_coefficient(0.1);
        assert!(cd_low > 100.0);
    }
}
