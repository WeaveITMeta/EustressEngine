//! # Buoyancy
//!
//! Archimedes' principle and buoyancy forces.
//!
//! ## Table of Contents
//!
//! 1. **BuoyancyBody** - Buoyancy properties component
//! 2. **Archimedes Force** - Buoyancy calculations
//! 3. **Floating Objects** - Equilibrium and stability

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::realism::constants;
use crate::realism::particles::components::KineticState;

// ============================================================================
// Buoyancy Body Component
// ============================================================================

/// Buoyancy properties for a body
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct BuoyancyBody {
    /// Volume of the body (m³)
    pub volume: f32,
    /// Mass of the body (kg)
    pub mass: f32,
    /// Center of buoyancy offset from center of mass
    pub center_of_buoyancy: Vec3,
    /// Drag coefficient in fluid
    pub fluid_drag: f32,
    /// Water level (Y coordinate)
    pub water_level: f32,
    /// Fluid density (kg/m³)
    pub fluid_density: f32,
    /// Enable buoyancy forces
    pub enabled: bool,
}

impl Default for BuoyancyBody {
    fn default() -> Self {
        Self {
            volume: 1.0,
            mass: 500.0, // Less than water, will float
            center_of_buoyancy: Vec3::ZERO,
            fluid_drag: 1.0,
            water_level: 0.0,
            fluid_density: constants::WATER_DENSITY,
            enabled: true,
        }
    }
}

impl BuoyancyBody {
    /// Create buoyancy body from dimensions (box)
    pub fn from_box(width: f32, height: f32, depth: f32, mass: f32) -> Self {
        Self {
            volume: width * height * depth,
            mass,
            ..default()
        }
    }
    
    /// Create buoyancy body from sphere
    pub fn from_sphere(radius: f32, mass: f32) -> Self {
        let volume = (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3);
        Self {
            volume,
            mass,
            ..default()
        }
    }
    
    /// Create buoyancy body from cylinder
    pub fn from_cylinder(radius: f32, height: f32, mass: f32) -> Self {
        let volume = std::f32::consts::PI * radius.powi(2) * height;
        Self {
            volume,
            mass,
            ..default()
        }
    }
    
    /// Get body density
    pub fn density(&self) -> f32 {
        if self.volume > 0.0 {
            self.mass / self.volume
        } else {
            0.0
        }
    }
    
    /// Check if body will float
    pub fn will_float(&self) -> bool {
        self.density() < self.fluid_density
    }
    
    /// Get fraction of volume submerged at equilibrium
    pub fn submerged_fraction(&self) -> f32 {
        if self.fluid_density > 0.0 {
            (self.density() / self.fluid_density).min(1.0)
        } else {
            0.0
        }
    }
    
    /// Get equilibrium draft (depth below water)
    pub fn equilibrium_draft(&self, height: f32) -> f32 {
        height * self.submerged_fraction()
    }
}

// ============================================================================
// Buoyancy Calculations
// ============================================================================

/// Archimedes' principle: F_b = ρ_fluid * V_submerged * g
/// 
/// # Arguments
/// * `fluid_density` - Density of fluid (kg/m³)
/// * `submerged_volume` - Volume of body submerged (m³)
/// * `gravity` - Gravitational acceleration (m/s²)
/// 
/// # Returns
/// Buoyancy force magnitude (N), directed upward
#[inline]
pub fn archimedes_force(fluid_density: f32, submerged_volume: f32, gravity: f32) -> f32 {
    fluid_density * submerged_volume * gravity
}

/// Calculate submerged volume for a box partially in water
pub fn box_submerged_volume(
    width: f32,
    height: f32,
    depth: f32,
    center_y: f32,
    water_level: f32,
) -> f32 {
    let bottom = center_y - height / 2.0;
    let top = center_y + height / 2.0;
    
    if top <= water_level {
        // Fully submerged
        width * height * depth
    } else if bottom >= water_level {
        // Above water
        0.0
    } else {
        // Partially submerged
        let submerged_height = water_level - bottom;
        width * submerged_height * depth
    }
}

/// Calculate submerged volume for a sphere partially in water
pub fn sphere_submerged_volume(radius: f32, center_y: f32, water_level: f32) -> f32 {
    let bottom = center_y - radius;
    let top = center_y + radius;
    
    if top <= water_level {
        // Fully submerged
        (4.0 / 3.0) * std::f32::consts::PI * radius.powi(3)
    } else if bottom >= water_level {
        // Above water
        0.0
    } else {
        // Partially submerged - spherical cap formula
        let h = water_level - bottom; // Height of submerged portion
        std::f32::consts::PI * h.powi(2) * (3.0 * radius - h) / 3.0
    }
}

/// Calculate submerged fraction (0-1) based on position
pub fn submerged_fraction(
    center_y: f32,
    half_height: f32,
    water_level: f32,
) -> f32 {
    let bottom = center_y - half_height;
    let top = center_y + half_height;
    let height = 2.0 * half_height;
    
    if height <= 0.0 {
        return 0.0;
    }
    
    if top <= water_level {
        1.0
    } else if bottom >= water_level {
        0.0
    } else {
        (water_level - bottom) / height
    }
}

// ============================================================================
// Fluid Drag
// ============================================================================

/// Drag force in fluid (water)
pub fn fluid_drag_force(
    velocity: Vec3,
    fluid_density: f32,
    drag_coefficient: f32,
    reference_area: f32,
) -> Vec3 {
    let speed = velocity.length();
    if speed < 1e-6 {
        return Vec3::ZERO;
    }
    
    let magnitude = 0.5 * fluid_density * speed * speed * drag_coefficient * reference_area;
    -velocity.normalize() * magnitude
}

/// Added mass effect (virtual mass of displaced fluid)
/// F_added = C_a * ρ * V * a
pub fn added_mass_force(
    acceleration: Vec3,
    fluid_density: f32,
    volume: f32,
    added_mass_coefficient: f32, // ~0.5 for sphere
) -> Vec3 {
    -added_mass_coefficient * fluid_density * volume * acceleration
}

// ============================================================================
// Stability
// ============================================================================

/// Metacentric height: GM = BM - BG
/// Positive GM = stable, Negative GM = unstable
pub fn metacentric_height(
    moment_of_inertia_waterplane: f32, // I of waterplane area
    submerged_volume: f32,
    center_of_buoyancy_height: f32,
    center_of_gravity_height: f32,
) -> f32 {
    if submerged_volume <= 0.0 {
        return 0.0;
    }
    
    // BM = I / V
    let bm = moment_of_inertia_waterplane / submerged_volume;
    // BG = height of G above B
    let bg = center_of_gravity_height - center_of_buoyancy_height;
    
    bm - bg
}

/// Check if floating body is stable
pub fn is_stable(metacentric_height: f32) -> bool {
    metacentric_height > 0.0
}

/// Righting moment: M = W * GM * sin(θ)
pub fn righting_moment(weight: f32, gm: f32, heel_angle: f32) -> f32 {
    weight * gm * heel_angle.sin()
}

// ============================================================================
// System
// ============================================================================

/// Apply buoyancy forces to bodies
pub fn apply_buoyancy_forces(
    mut query: Query<(&BuoyancyBody, &mut KineticState, &Transform)>,
) {
    let gravity = 9.81;
    
    for (buoyancy, mut kinetic, transform) in query.iter_mut() {
        if !buoyancy.enabled {
            continue;
        }
        
        let center_y = transform.translation.y;
        
        // Estimate submerged fraction (assuming box-like shape)
        // For more accuracy, use actual geometry
        let half_height = (buoyancy.volume).cbrt() / 2.0; // Approximate
        let fraction = submerged_fraction(center_y, half_height, buoyancy.water_level);
        
        if fraction <= 0.0 {
            // Above water - only gravity
            kinetic.apply_force(Vec3::new(0.0, -buoyancy.mass * gravity, 0.0));
            continue;
        }
        
        // Buoyancy force
        let submerged_vol = buoyancy.volume * fraction;
        let f_buoyancy = archimedes_force(buoyancy.fluid_density, submerged_vol, gravity);
        kinetic.apply_force(Vec3::new(0.0, f_buoyancy, 0.0));
        
        // Gravity
        kinetic.apply_force(Vec3::new(0.0, -buoyancy.mass * gravity, 0.0));
        
        // Fluid drag (only on submerged portion)
        if kinetic.velocity.length() > 0.01 {
            let drag_area = (buoyancy.volume).powf(2.0 / 3.0) * fraction;
            let f_drag = fluid_drag_force(
                kinetic.velocity,
                buoyancy.fluid_density,
                buoyancy.fluid_drag,
                drag_area,
            );
            kinetic.apply_force(f_drag);
        }
        
        // Righting torque from offset center of buoyancy
        if buoyancy.center_of_buoyancy.length_squared() > 1e-6 && fraction > 0.0 {
            let cob_world = transform.rotation * buoyancy.center_of_buoyancy;
            let buoyancy_force = Vec3::new(0.0, f_buoyancy, 0.0);
            let torque = cob_world.cross(buoyancy_force);
            kinetic.apply_torque(torque);
        }
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Calculate waterplane area moment of inertia for a rectangle
/// I = (1/12) * L * B³
pub fn waterplane_moment_rectangle(length: f32, beam: f32) -> f32 {
    (1.0 / 12.0) * length * beam.powi(3)
}

/// Calculate waterplane area moment of inertia for a circle
/// I = (π/64) * D⁴
pub fn waterplane_moment_circle(diameter: f32) -> f32 {
    (std::f32::consts::PI / 64.0) * diameter.powi(4)
}

/// Freeboard (height above water)
pub fn freeboard(total_height: f32, draft: f32) -> f32 {
    total_height - draft
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_archimedes_force() {
        // 1 m³ submerged in water
        let f = archimedes_force(1000.0, 1.0, 9.81);
        // F = 1000 * 1 * 9.81 = 9810 N
        assert!((f - 9810.0).abs() < 1.0);
    }
    
    #[test]
    fn test_will_float() {
        // Wood (density ~700 kg/m³)
        let wood = BuoyancyBody {
            volume: 1.0,
            mass: 700.0,
            fluid_density: 1000.0,
            ..default()
        };
        assert!(wood.will_float());
        
        // Steel (density ~7850 kg/m³)
        let steel = BuoyancyBody {
            volume: 1.0,
            mass: 7850.0,
            fluid_density: 1000.0,
            ..default()
        };
        assert!(!steel.will_float());
    }
    
    #[test]
    fn test_submerged_fraction() {
        // Object centered at water level
        let f = submerged_fraction(0.0, 1.0, 0.0);
        assert!((f - 0.5).abs() < 0.01);
        
        // Object fully submerged
        let f2 = submerged_fraction(-2.0, 1.0, 0.0);
        assert!((f2 - 1.0).abs() < 0.01);
        
        // Object above water
        let f3 = submerged_fraction(2.0, 1.0, 0.0);
        assert!(f3 < 0.01);
    }
    
    #[test]
    fn test_sphere_submerged_volume() {
        let r = 1.0;
        let full_vol = (4.0 / 3.0) * std::f32::consts::PI * r.powi(3);
        
        // Fully submerged
        let v1 = sphere_submerged_volume(r, -2.0, 0.0);
        assert!((v1 - full_vol).abs() < 0.01);
        
        // Half submerged (center at water level)
        let v2 = sphere_submerged_volume(r, 0.0, 0.0);
        assert!((v2 - full_vol / 2.0).abs() < 0.1);
    }
}
