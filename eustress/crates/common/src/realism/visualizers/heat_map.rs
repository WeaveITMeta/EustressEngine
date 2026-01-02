//! # Heat Map Visualization
//!
//! Temperature gradient visualization.
//!
//! ## Table of Contents
//!
//! 1. **HeatMapSettings** - Configuration
//! 2. **Color Mapping** - Temperature to color conversion

use bevy::prelude::*;

// ============================================================================
// Settings
// ============================================================================

/// Settings for heat map visualization
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct HeatMapSettings {
    /// Minimum temperature for color scale (K)
    pub min_temperature: f32,
    /// Maximum temperature for color scale (K)
    pub max_temperature: f32,
    /// Color palette
    pub palette: HeatMapPalette,
    /// Opacity (0-1)
    pub opacity: f32,
    /// Show legend
    pub show_legend: bool,
}

impl Default for HeatMapSettings {
    fn default() -> Self {
        Self {
            min_temperature: 200.0,  // -73°C
            max_temperature: 500.0,  // 227°C
            palette: HeatMapPalette::Thermal,
            opacity: 0.8,
            show_legend: true,
        }
    }
}

/// Color palette options
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum HeatMapPalette {
    /// Blue -> Cyan -> Green -> Yellow -> Red
    #[default]
    Thermal,
    /// Black -> Red -> Yellow -> White
    Inferno,
    /// Blue -> White -> Red
    Diverging,
    /// Single color intensity
    Monochrome,
    /// Scientific rainbow
    Rainbow,
}

// ============================================================================
// Color Mapping
// ============================================================================

/// Map temperature to color using selected palette
pub fn temperature_to_color(temperature: f32, settings: &HeatMapSettings) -> Color {
    let t = ((temperature - settings.min_temperature) 
        / (settings.max_temperature - settings.min_temperature))
        .clamp(0.0, 1.0);
    
    let rgb = match settings.palette {
        HeatMapPalette::Thermal => thermal_palette(t),
        HeatMapPalette::Inferno => inferno_palette(t),
        HeatMapPalette::Diverging => diverging_palette(t),
        HeatMapPalette::Monochrome => monochrome_palette(t),
        HeatMapPalette::Rainbow => rainbow_palette(t),
    };
    
    Color::srgba(rgb.0, rgb.1, rgb.2, settings.opacity)
}

/// Thermal palette: Blue -> Cyan -> Green -> Yellow -> Red
fn thermal_palette(t: f32) -> (f32, f32, f32) {
    if t < 0.25 {
        let s = t / 0.25;
        (0.0, s, 1.0)
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        (0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        (s, 1.0, 0.0)
    } else {
        let s = (t - 0.75) / 0.25;
        (1.0, 1.0 - s, 0.0)
    }
}

/// Inferno palette: Black -> Red -> Yellow -> White
fn inferno_palette(t: f32) -> (f32, f32, f32) {
    if t < 0.33 {
        let s = t / 0.33;
        (s, 0.0, 0.0)
    } else if t < 0.66 {
        let s = (t - 0.33) / 0.33;
        (1.0, s, 0.0)
    } else {
        let s = (t - 0.66) / 0.34;
        (1.0, 1.0, s)
    }
}

/// Diverging palette: Blue -> White -> Red
fn diverging_palette(t: f32) -> (f32, f32, f32) {
    if t < 0.5 {
        let s = t / 0.5;
        (s, s, 1.0)
    } else {
        let s = (t - 0.5) / 0.5;
        (1.0, 1.0 - s, 1.0 - s)
    }
}

/// Monochrome palette: Black -> White
fn monochrome_palette(t: f32) -> (f32, f32, f32) {
    (t, t, t)
}

/// Rainbow palette: Full spectrum
fn rainbow_palette(t: f32) -> (f32, f32, f32) {
    let hue = (1.0 - t) * 270.0; // Purple to Red
    hsl_to_rgb(hue, 1.0, 0.5)
}

/// Convert HSL to RGB
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    
    (r + m, g + m, b + m)
}

// ============================================================================
// Grid Heat Map
// ============================================================================

/// Sample temperature field on a grid
pub fn sample_temperature_grid(
    particles: &[(Vec3, f32)], // (position, temperature)
    grid_origin: Vec3,
    grid_size: UVec3,
    cell_size: f32,
    smoothing_radius: f32,
) -> Vec<f32> {
    let total_cells = (grid_size.x * grid_size.y * grid_size.z) as usize;
    let mut temperatures = vec![0.0f32; total_cells];
    let mut weights = vec![0.0f32; total_cells];
    
    for (pos, temp) in particles {
        let local = (*pos - grid_origin) / cell_size;
        let cell_x = local.x as i32;
        let cell_y = local.y as i32;
        let cell_z = local.z as i32;
        
        let cells_radius = (smoothing_radius / cell_size).ceil() as i32;
        
        for dx in -cells_radius..=cells_radius {
            for dy in -cells_radius..=cells_radius {
                for dz in -cells_radius..=cells_radius {
                    let cx = cell_x + dx;
                    let cy = cell_y + dy;
                    let cz = cell_z + dz;
                    
                    if cx < 0 || cy < 0 || cz < 0 
                        || cx >= grid_size.x as i32 
                        || cy >= grid_size.y as i32 
                        || cz >= grid_size.z as i32 {
                        continue;
                    }
                    
                    let cell_center = grid_origin + Vec3::new(
                        (cx as f32 + 0.5) * cell_size,
                        (cy as f32 + 0.5) * cell_size,
                        (cz as f32 + 0.5) * cell_size,
                    );
                    
                    let dist = (*pos - cell_center).length();
                    if dist < smoothing_radius {
                        let weight = 1.0 - dist / smoothing_radius;
                        let idx = (cx as u32 + cy as u32 * grid_size.x + cz as u32 * grid_size.x * grid_size.y) as usize;
                        temperatures[idx] += *temp * weight;
                        weights[idx] += weight;
                    }
                }
            }
        }
    }
    
    // Normalize
    for i in 0..total_cells {
        if weights[i] > 0.0 {
            temperatures[i] /= weights[i];
        }
    }
    
    temperatures
}

/// Generate isotherm contour points
pub fn generate_isotherm(
    temperatures: &[f32],
    grid_size: UVec3,
    target_temperature: f32,
    grid_origin: Vec3,
    cell_size: f32,
) -> Vec<Vec3> {
    let mut points = Vec::new();
    
    // Simple marching cubes-like approach for finding isotherm surface
    for z in 0..grid_size.z.saturating_sub(1) {
        for y in 0..grid_size.y.saturating_sub(1) {
            for x in 0..grid_size.x.saturating_sub(1) {
                let idx = |x: u32, y: u32, z: u32| -> usize {
                    (x + y * grid_size.x + z * grid_size.x * grid_size.y) as usize
                };
                
                // Check if isotherm crosses this cell
                let t000 = temperatures[idx(x, y, z)];
                let t100 = temperatures[idx(x + 1, y, z)];
                let t010 = temperatures[idx(x, y + 1, z)];
                let t001 = temperatures[idx(x, y, z + 1)];
                
                let min_t = t000.min(t100).min(t010).min(t001);
                let max_t = t000.max(t100).max(t010).max(t001);
                
                if target_temperature >= min_t && target_temperature <= max_t {
                    // Isotherm crosses this cell - add center point
                    let pos = grid_origin + Vec3::new(
                        (x as f32 + 0.5) * cell_size,
                        (y as f32 + 0.5) * cell_size,
                        (z as f32 + 0.5) * cell_size,
                    );
                    points.push(pos);
                }
            }
        }
    }
    
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_thermal_palette() {
        // Cold should be blue
        let cold = thermal_palette(0.0);
        assert!(cold.2 > cold.0 && cold.2 > cold.1);
        
        // Hot should be red
        let hot = thermal_palette(1.0);
        assert!(hot.0 > hot.1 && hot.0 > hot.2);
    }
    
    #[test]
    fn test_temperature_to_color() {
        let settings = HeatMapSettings::default();
        
        let cold = temperature_to_color(settings.min_temperature, &settings);
        let hot = temperature_to_color(settings.max_temperature, &settings);
        
        // Just verify they're different
        assert_ne!(cold, hot);
    }
}
