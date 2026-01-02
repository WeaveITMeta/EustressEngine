//! # Vector Field Visualization
//!
//! Velocity and force field visualization using gizmos.
//!
//! ## Table of Contents
//!
//! 1. **VectorFieldSettings** - Configuration
//! 2. **Systems** - Drawing systems

use bevy::prelude::*;

use crate::realism::particles::components::{Particle, KineticState};

// ============================================================================
// Settings
// ============================================================================

/// Settings for vector field visualization
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct VectorFieldSettings {
    /// Show velocity vectors
    pub show_velocity: bool,
    /// Show force vectors
    pub show_forces: bool,
    /// Show momentum vectors
    pub show_momentum: bool,
    /// Vector scale factor
    pub scale: f32,
    /// Maximum vector length (clamped)
    pub max_length: f32,
    /// Minimum speed to show vector
    pub min_speed: f32,
    /// Maximum speed for color mapping
    pub max_speed: f32,
    /// Arrow head size
    pub arrow_size: f32,
    /// Line thickness
    pub line_width: f32,
}

impl Default for VectorFieldSettings {
    fn default() -> Self {
        Self {
            show_velocity: true,
            show_forces: false,
            show_momentum: false,
            scale: 0.1,
            max_length: 5.0,
            min_speed: 0.1,
            max_speed: 50.0,
            arrow_size: 0.1,
            line_width: 2.0,
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Draw vector field for particles
pub fn draw_vector_field(
    query: Query<(&Transform, &KineticState), With<Particle>>,
    mut gizmos: Gizmos,
    settings: Res<VectorFieldSettings>,
) {
    if !settings.show_velocity && !settings.show_forces && !settings.show_momentum {
        return;
    }
    
    for (transform, kinetic) in query.iter() {
        let pos = transform.translation;
        
        // Velocity vector
        if settings.show_velocity {
            let speed = kinetic.velocity.length();
            if speed > settings.min_speed {
                let direction = kinetic.velocity / speed;
                let length = (speed * settings.scale).min(settings.max_length);
                let end = pos + direction * length;
                
                let color = velocity_to_color(speed, settings.max_speed);
                gizmos.arrow(pos, end, color);
            }
        }
        
        // Force vector
        if settings.show_forces {
            let force_mag = kinetic.accumulated_force.length();
            if force_mag > 0.01 {
                let direction = kinetic.accumulated_force / force_mag;
                let length = (force_mag * settings.scale * 0.01).min(settings.max_length);
                let end = pos + direction * length;
                
                gizmos.arrow(pos, end, Color::srgb(1.0, 0.5, 0.0)); // Orange for force
            }
        }
        
        // Momentum vector
        if settings.show_momentum {
            let momentum_mag = kinetic.momentum.length();
            if momentum_mag > 0.01 {
                let direction = kinetic.momentum / momentum_mag;
                let length = (momentum_mag * settings.scale * 0.1).min(settings.max_length);
                let end = pos + direction * length;
                
                gizmos.arrow(pos, end, Color::srgb(0.5, 0.0, 1.0)); // Purple for momentum
            }
        }
    }
}

/// Color mapping for velocity magnitude
fn velocity_to_color(speed: f32, max_speed: f32) -> Color {
    let t = (speed / max_speed).clamp(0.0, 1.0);
    
    // Blue (slow) -> Cyan -> Green -> Yellow -> Red (fast)
    if t < 0.25 {
        let s = t / 0.25;
        Color::srgb(0.0, s, 1.0)
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        Color::srgb(0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        Color::srgb(s, 1.0, 0.0)
    } else {
        let s = (t - 0.75) / 0.25;
        Color::srgb(1.0, 1.0 - s, 0.0)
    }
}

// ============================================================================
// Grid-based Vector Field
// ============================================================================

/// Sample velocity field on a grid
pub fn sample_velocity_grid(
    particles: &[(Vec3, Vec3)], // (position, velocity)
    grid_origin: Vec3,
    grid_size: UVec3,
    cell_size: f32,
    smoothing_radius: f32,
) -> Vec<Vec3> {
    let total_cells = (grid_size.x * grid_size.y * grid_size.z) as usize;
    let mut velocities = vec![Vec3::ZERO; total_cells];
    let mut weights = vec![0.0f32; total_cells];
    
    for (pos, vel) in particles {
        // Find affected cells
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
                        velocities[idx] += *vel * weight;
                        weights[idx] += weight;
                    }
                }
            }
        }
    }
    
    // Normalize by weights
    for i in 0..total_cells {
        if weights[i] > 0.0 {
            velocities[i] /= weights[i];
        }
    }
    
    velocities
}

/// Draw grid-based vector field
pub fn draw_velocity_grid(
    gizmos: &mut Gizmos,
    velocities: &[Vec3],
    grid_origin: Vec3,
    grid_size: UVec3,
    cell_size: f32,
    scale: f32,
    max_speed: f32,
) {
    for z in 0..grid_size.z {
        for y in 0..grid_size.y {
            for x in 0..grid_size.x {
                let idx = (x + y * grid_size.x + z * grid_size.x * grid_size.y) as usize;
                let vel = velocities[idx];
                let speed = vel.length();
                
                if speed < 0.01 {
                    continue;
                }
                
                let pos = grid_origin + Vec3::new(
                    (x as f32 + 0.5) * cell_size,
                    (y as f32 + 0.5) * cell_size,
                    (z as f32 + 0.5) * cell_size,
                );
                
                let end = pos + vel.normalize() * (speed * scale).min(cell_size * 0.8);
                let color = velocity_to_color(speed, max_speed);
                
                gizmos.arrow(pos, end, color);
            }
        }
    }
}

// ============================================================================
// Streamlines
// ============================================================================

/// Generate streamline points from a velocity field
pub fn generate_streamline(
    start: Vec3,
    velocity_field: impl Fn(Vec3) -> Vec3,
    step_size: f32,
    max_steps: usize,
    bounds: (Vec3, Vec3),
) -> Vec<Vec3> {
    let mut points = vec![start];
    let mut pos = start;
    
    for _ in 0..max_steps {
        let vel = velocity_field(pos);
        let speed = vel.length();
        
        if speed < 1e-6 {
            break;
        }
        
        // RK4 integration
        let k1 = vel.normalize();
        let k2 = velocity_field(pos + k1 * step_size * 0.5).normalize_or_zero();
        let k3 = velocity_field(pos + k2 * step_size * 0.5).normalize_or_zero();
        let k4 = velocity_field(pos + k3 * step_size).normalize_or_zero();
        
        let direction = (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
        pos += direction * step_size;
        
        // Check bounds
        if pos.x < bounds.0.x || pos.x > bounds.1.x
            || pos.y < bounds.0.y || pos.y > bounds.1.y
            || pos.z < bounds.0.z || pos.z > bounds.1.z {
            break;
        }
        
        points.push(pos);
    }
    
    points
}

/// Draw streamline
pub fn draw_streamline(gizmos: &mut Gizmos, points: &[Vec3], color: Color) {
    for i in 0..points.len().saturating_sub(1) {
        gizmos.line(points[i], points[i + 1], color);
    }
}
