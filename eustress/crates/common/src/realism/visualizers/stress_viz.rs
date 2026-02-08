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

/// Draw stress indicators using gizmos - DISABLED for Bevy 0.19
pub fn draw_stress_indicators(
    _query: Query<(&Transform, &StressTensor, &MaterialProperties, Option<&FractureState>)>,
    // mut gizmos: Gizmos,
) {
    // Disabled - Gizmos API changed in Bevy 0.19
}

/// Draw principal stress directions - DISABLED for Bevy 0.19
fn draw_principal_stresses(
    _gizmos: &mut (),
    _position: Vec3,
    _rotation: Quat,
    _stress: &StressTensor,
    _scale: f32,
    _base_color: Color,
) {
    // Disabled - Gizmos API changed in Bevy 0.19
}

/// Draw arrow head - DISABLED for Bevy 0.19
fn draw_arrow_head(_gizmos: &mut (), _tip: Vec3, _direction: Vec3, _size: f32, _color: Color) {
    // Disabled - Gizmos API changed in Bevy 0.19
}

/// Draw crack visualization - DISABLED for Bevy 0.19
fn draw_crack(
    _gizmos: &mut (),
    _body_position: Vec3,
    _body_rotation: Quat,
    _crack: &crate::realism::materials::fracture::Crack,
) {
    // Disabled - Gizmos API changed in Bevy 0.19
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
