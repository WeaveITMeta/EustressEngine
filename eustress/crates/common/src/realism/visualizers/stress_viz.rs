//! # Stress Visualization
//!
//! Stress tensor and fracture visualization.
//!
//! ## Table of Contents
//!
//! 1. **Stress Indicators** - Von Mises, principal stresses
//! 2. **Fracture Visualization** - Crack display

use bevy::prelude::*;

use crate::realism::materials::stress_strain::StressTensor;
use crate::realism::materials::fracture::FractureState;
use crate::realism::materials::properties::MaterialProperties;

// ============================================================================
// Systems
// ============================================================================

/// Draw stress indicators using gizmos
pub fn draw_stress_indicators(
    query: Query<(&Transform, &StressTensor, &MaterialProperties, Option<&FractureState>)>,
    mut gizmos: Gizmos,
) {
    for (transform, stress, material, fracture) in query.iter() {
        let pos = transform.translation;
        
        // Color based on stress ratio to yield
        let ratio = stress.von_mises / material.yield_strength;
        let color = stress_ratio_to_color(ratio);
        
        // Draw principal stress directions
        draw_principal_stresses(&mut gizmos, pos, transform.rotation, stress, 0.5, color);
        
        // Draw cracks if present
        if let Some(fracture) = fracture {
            for crack in &fracture.cracks {
                draw_crack(&mut gizmos, pos, transform.rotation, crack);
            }
            
            // Draw damage indicator
            if fracture.damage > 0.01 {
                let damage_color = damage_to_color(fracture.damage);
                gizmos.sphere(
                    Isometry3d::from_translation(pos),
                    0.1 + fracture.damage * 0.2,
                    damage_color,
                );
            }
        }
    }
}

/// Draw principal stress directions
fn draw_principal_stresses(
    gizmos: &mut Gizmos,
    position: Vec3,
    rotation: Quat,
    stress: &StressTensor,
    scale: f32,
    base_color: Color,
) {
    // Principal directions (simplified - assuming aligned with local axes)
    let directions = [
        rotation * Vec3::X,
        rotation * Vec3::Y,
        rotation * Vec3::Z,
    ];
    
    for (i, (dir, &principal)) in directions.iter().zip(stress.principal.iter()).enumerate() {
        if principal.abs() < 1e-3 {
            continue;
        }
        
        let length = (principal.abs() / 1e6).min(scale); // Scale by MPa
        let color = if principal > 0.0 {
            // Tension - red tint
            Color::srgb(1.0, 0.3, 0.3)
        } else {
            // Compression - blue tint
            Color::srgb(0.3, 0.3, 1.0)
        };
        
        // Draw bidirectional arrow for stress
        let end1 = position + *dir * length;
        let end2 = position - *dir * length;
        
        gizmos.line(end1, end2, color);
        
        // Arrow heads
        let arrow_size = length * 0.2;
        if principal > 0.0 {
            // Tension arrows point outward
            draw_arrow_head(gizmos, end1, *dir, arrow_size, color);
            draw_arrow_head(gizmos, end2, -*dir, arrow_size, color);
        } else {
            // Compression arrows point inward
            draw_arrow_head(gizmos, position + *dir * (length - arrow_size), -*dir, arrow_size, color);
            draw_arrow_head(gizmos, position - *dir * (length - arrow_size), *dir, arrow_size, color);
        }
    }
}

/// Draw arrow head
fn draw_arrow_head(gizmos: &mut Gizmos, tip: Vec3, direction: Vec3, size: f32, color: Color) {
    let dir = direction.normalize();
    
    // Find perpendicular vectors
    let perp1 = if dir.x.abs() < 0.9 {
        dir.cross(Vec3::X).normalize()
    } else {
        dir.cross(Vec3::Y).normalize()
    };
    let perp2 = dir.cross(perp1).normalize();
    
    let base = tip - dir * size;
    let half_width = size * 0.3;
    
    gizmos.line(tip, base + perp1 * half_width, color);
    gizmos.line(tip, base - perp1 * half_width, color);
    gizmos.line(tip, base + perp2 * half_width, color);
    gizmos.line(tip, base - perp2 * half_width, color);
}

/// Draw crack visualization
fn draw_crack(
    gizmos: &mut Gizmos,
    body_position: Vec3,
    body_rotation: Quat,
    crack: &crate::realism::materials::fracture::Crack,
) {
    let crack_start = body_position + body_rotation * crack.position;
    let crack_end = crack_start + body_rotation * crack.direction * crack.length;
    
    // Main crack line
    gizmos.line(crack_start, crack_end, Color::srgb(0.2, 0.2, 0.2));
    
    // Crack opening visualization
    if crack.opening > 0.001 {
        let perp = if crack.direction.x.abs() < 0.9 {
            crack.direction.cross(Vec3::X).normalize()
        } else {
            crack.direction.cross(Vec3::Y).normalize()
        };
        
        let mid = (crack_start + crack_end) / 2.0;
        let half_opening = crack.opening / 2.0;
        
        gizmos.line(
            mid + body_rotation * perp * half_opening,
            mid - body_rotation * perp * half_opening,
            Color::srgb(0.5, 0.0, 0.0),
        );
    }
}

// ============================================================================
// Color Utilities
// ============================================================================

/// Convert stress ratio (σ/σ_y) to color
fn stress_ratio_to_color(ratio: f32) -> Color {
    if ratio < 0.5 {
        // Green - safe
        Color::srgb(0.0, 0.8, 0.0)
    } else if ratio < 0.8 {
        // Yellow - caution
        let t = (ratio - 0.5) / 0.3;
        Color::srgb(t, 0.8, 0.0)
    } else if ratio < 1.0 {
        // Orange - warning
        let t = (ratio - 0.8) / 0.2;
        Color::srgb(1.0, 0.8 - t * 0.5, 0.0)
    } else if ratio < 1.5 {
        // Red - yielded
        Color::srgb(1.0, 0.0, 0.0)
    } else {
        // Dark red - critical
        Color::srgb(0.5, 0.0, 0.0)
    }
}

/// Convert damage (0-1) to color
fn damage_to_color(damage: f32) -> Color {
    let t = damage.clamp(0.0, 1.0);
    Color::srgba(t, 0.0, 0.0, 0.3 + t * 0.4)
}

// ============================================================================
// Stress Contour
// ============================================================================

/// Generate stress contour data for a mesh
pub struct StressContour {
    /// Vertex positions
    pub positions: Vec<Vec3>,
    /// Vertex colors based on stress
    pub colors: Vec<Color>,
    /// Stress values at vertices
    pub stress_values: Vec<f32>,
}

impl StressContour {
    /// Create from vertex stresses
    pub fn from_vertex_stresses(
        positions: Vec<Vec3>,
        stresses: Vec<f32>,
        yield_stress: f32,
    ) -> Self {
        let colors = stresses.iter()
            .map(|s| stress_ratio_to_color(*s / yield_stress))
            .collect();
        
        Self {
            positions,
            colors,
            stress_values: stresses,
        }
    }
    
    /// Get maximum stress
    pub fn max_stress(&self) -> f32 {
        self.stress_values.iter().cloned().fold(0.0, f32::max)
    }
    
    /// Get minimum stress
    pub fn min_stress(&self) -> f32 {
        self.stress_values.iter().cloned().fold(f32::INFINITY, f32::min)
    }
    
    /// Get average stress
    pub fn avg_stress(&self) -> f32 {
        if self.stress_values.is_empty() {
            return 0.0;
        }
        self.stress_values.iter().sum::<f32>() / self.stress_values.len() as f32
    }
}

// ============================================================================
// Mohr's Circle Visualization Data
// ============================================================================

/// Data for Mohr's circle visualization
pub struct MohrCircleData {
    /// Center of circle (average normal stress)
    pub center: f32,
    /// Radius of circle (max shear stress)
    pub radius: f32,
    /// Principal stresses (σ₁, σ₂)
    pub principal: (f32, f32),
    /// Maximum shear stress
    pub max_shear: f32,
}

impl MohrCircleData {
    /// Create from 2D stress state
    pub fn from_2d_stress(sigma_x: f32, sigma_y: f32, tau_xy: f32) -> Self {
        let center = (sigma_x + sigma_y) / 2.0;
        let radius = ((sigma_x - sigma_y).powi(2) / 4.0 + tau_xy.powi(2)).sqrt();
        
        Self {
            center,
            radius,
            principal: (center + radius, center - radius),
            max_shear: radius,
        }
    }
    
    /// Create from stress tensor (using σ_xx, σ_yy, τ_xy)
    pub fn from_tensor(stress: &StressTensor) -> Self {
        Self::from_2d_stress(
            stress.components[0][0],
            stress.components[1][1],
            stress.components[0][1],
        )
    }
}
