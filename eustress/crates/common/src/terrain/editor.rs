//! Terrain editor systems for runtime painting
//! 
//! Supports both heightmap-based and voxel-based editing modes for
//! fine-grained terrain sculpting.

use bevy::prelude::*;
use super::{Chunk, TerrainConfig, TerrainData, TerrainRoot, generate_chunk_mesh};

// ============================================================================
// Voxel Constants - Fine-grain editing precision
// ============================================================================

/// Default voxel size in world units (0.25m = 25cm precision)
pub const DEFAULT_VOXEL_SIZE: f32 = 0.25;

/// Minimum voxel size for ultra-fine editing (5cm)
pub const MIN_VOXEL_SIZE: f32 = 0.05;

/// Maximum voxel size for coarse editing (2m)
pub const MAX_VOXEL_SIZE: f32 = 2.0;

// ============================================================================
// Brush Settings
// ============================================================================

/// Brush settings for terrain painting
#[derive(Resource, Clone, Debug)]
pub struct TerrainBrush {
    /// Brush radius in world units (0.1 to 50.0)
    pub radius: f32,
    
    /// Brush strength (0-1)
    pub strength: f32,
    
    /// Brush falloff (0 = hard edge, 1 = soft edge)
    pub falloff: f32,
    
    /// Current brush mode
    pub mode: BrushMode,
    
    /// Selected texture layer (0-15) for splat painting
    pub texture_layer: usize,
    
    /// Voxel editing mode enabled
    pub voxel_mode: bool,
    
    /// Voxel size for fine-grain editing (in world units)
    pub voxel_size: f32,
    
    /// Brush shape
    pub shape: BrushShape,
    
    /// Precision level (affects sampling density)
    pub precision: BrushPrecision,
    
    /// Height step for voxel mode (quantizes height changes)
    pub height_step: f32,
}

impl Default for TerrainBrush {
    fn default() -> Self {
        Self {
            radius: 8.0,  // Larger default for usability
            strength: 0.2,  // Lower default for smoother editing
            falloff: 0.5,  // Smooth falloff for natural results
            mode: BrushMode::Raise,
            texture_layer: 0,
            voxel_mode: true,  // Enable voxel mode by default
            voxel_size: DEFAULT_VOXEL_SIZE,
            shape: BrushShape::Circle,
            precision: BrushPrecision::High,
            height_step: 0.1,  // 10cm height steps
        }
    }
}

impl TerrainBrush {
    /// Create a brush optimized for fine detail work
    pub fn fine_detail() -> Self {
        Self {
            radius: 0.5,
            strength: 0.3,
            falloff: 0.1,
            voxel_mode: true,
            voxel_size: MIN_VOXEL_SIZE,
            precision: BrushPrecision::Ultra,
            height_step: 0.05,
            ..Default::default()
        }
    }
    
    /// Create a brush optimized for large area sculpting
    pub fn large_sculpt() -> Self {
        Self {
            radius: 20.0,
            strength: 0.7,
            falloff: 0.8,
            voxel_mode: false,
            voxel_size: MAX_VOXEL_SIZE,
            precision: BrushPrecision::Low,
            height_step: 0.5,
            ..Default::default()
        }
    }
    
    /// Get the number of samples based on precision
    pub fn sample_multiplier(&self) -> f32 {
        match self.precision {
            BrushPrecision::Low => 0.5,
            BrushPrecision::Medium => 1.0,
            BrushPrecision::High => 2.0,
            BrushPrecision::Ultra => 4.0,
        }
    }
    
    /// Get effective voxel size based on mode
    pub fn effective_voxel_size(&self) -> f32 {
        if self.voxel_mode {
            self.voxel_size
        } else {
            // Non-voxel mode uses larger steps
            self.voxel_size * 4.0
        }
    }
}

/// Brush shape options
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BrushShape {
    /// Circular brush
    #[default]
    Circle,
    /// Square brush
    Square,
    /// Diamond/rhombus brush
    Diamond,
}

/// Brush precision levels
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BrushPrecision {
    /// Low precision - faster, coarser edits
    Low,
    /// Medium precision - balanced
    Medium,
    /// High precision - detailed edits
    #[default]
    High,
    /// Ultra precision - maximum detail (slower)
    Ultra,
}

/// Brush painting modes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BrushMode {
    /// Raise terrain height
    #[default]
    Raise,
    /// Lower terrain height
    Lower,
    /// Smooth terrain height
    Smooth,
    /// Flatten to target height
    Flatten,
    /// Paint texture splat
    PaintTexture,
    /// Voxel add - adds voxels at brush location
    VoxelAdd,
    /// Voxel remove - removes voxels at brush location  
    VoxelRemove,
    /// Voxel smooth - smooths voxel edges
    VoxelSmooth,
    /// Select region for bulk operations
    Region,
    /// Fill region with material
    Fill,
}

/// System for terrain painting with mouse
/// Note: In engine, this should be gated by egui pointer check
pub fn terrain_paint_system(
    buttons: Res<ButtonInput<MouseButton>>,
    _keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut terrain_query: Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    mut chunk_query: Query<(Entity, &mut Chunk, &GlobalTransform)>,
    brush: Res<TerrainBrush>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    // Only paint when LMB is pressed
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    
    // Adjust brush with scroll or keys
    // (handled separately in UI)
    
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = camera_query.single() else { return };
    let Ok((config, mut data)) = terrain_query.single_mut() else { return };
    
    // Get cursor position
    let Some(cursor_pos) = window.cursor_position() else { return };
    
    // Raycast from cursor to terrain
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };
    
    // Simple ground plane intersection (Y = 0)
    // TODO: Proper terrain raycast
    let t = -ray.origin.y / ray.direction.y;
    if t < 0.0 {
        return;  // Ray pointing away from ground
    }
    
    let hit_point = ray.origin + ray.direction * t;
    
    // Find affected chunks
    for (entity, mut chunk, transform) in chunk_query.iter_mut() {
        let chunk_center = transform.translation();
        let chunk_half_size = config.chunk_size * 0.5;
        
        // Check if brush overlaps this chunk
        let min_x = chunk_center.x - chunk_half_size - brush.radius;
        let max_x = chunk_center.x + chunk_half_size + brush.radius;
        let min_z = chunk_center.z - chunk_half_size - brush.radius;
        let max_z = chunk_center.z + chunk_half_size + brush.radius;
        
        if hit_point.x >= min_x && hit_point.x <= max_x &&
           hit_point.z >= min_z && hit_point.z <= max_z {
            // Mark chunk as dirty for regeneration
            chunk.dirty = true;
            
            // Apply brush to height cache
            apply_brush_to_chunk(
                &hit_point,
                &brush,
                &chunk,
                config,
                &mut data,
            );
            
            // Regenerate chunk mesh
            let new_mesh = generate_chunk_mesh(
                chunk.position,
                chunk.lod,
                config,
                &data,
                &mut meshes,
            );
            
            commands.entity(entity).insert(Mesh3d(new_mesh));
        }
    }
}

/// Apply brush effect to terrain data with voxel-based precision
fn apply_brush_to_chunk(
    hit_point: &Vec3,
    brush: &TerrainBrush,
    chunk: &Chunk,
    config: &TerrainConfig,
    data: &mut TerrainData,
) {
    // Initialize height cache if empty
    if data.height_cache.is_empty() {
        let total_size = (config.chunk_resolution + 1) * (config.chunk_resolution + 1);
        data.height_cache = vec![0.0; total_size as usize];
        data.cache_width = config.chunk_resolution + 1;
        data.cache_height = config.chunk_resolution + 1;
    }
    
    let chunk_world_x = chunk.position.x as f32 * config.chunk_size;
    let chunk_world_z = chunk.position.y as f32 * config.chunk_size;
    
    // Calculate sampling density based on precision and voxel mode
    let sample_mult = brush.sample_multiplier();
    let effective_resolution = if brush.voxel_mode {
        // In voxel mode, use higher resolution sampling
        ((config.chunk_resolution as f32 * sample_mult) as u32).max(config.chunk_resolution)
    } else {
        config.chunk_resolution
    };
    
    // Voxel size determines the minimum edit granularity
    let voxel_size = brush.effective_voxel_size();
    let height_step = if brush.voxel_mode { brush.height_step } else { 0.0 };
    
    // Iterate over vertices with higher precision in voxel mode
    for z in 0..=effective_resolution {
        for x in 0..=effective_resolution {
            let u = x as f32 / effective_resolution as f32;
            let v = z as f32 / effective_resolution as f32;
            
            let world_x = chunk_world_x + u * config.chunk_size;
            let world_z = chunk_world_z + v * config.chunk_size;
            
            // Check brush shape
            let in_brush = match brush.shape {
                BrushShape::Circle => {
                    let dx = world_x - hit_point.x;
                    let dz = world_z - hit_point.z;
                    (dx * dx + dz * dz).sqrt() <= brush.radius
                }
                BrushShape::Square => {
                    let dx = (world_x - hit_point.x).abs();
                    let dz = (world_z - hit_point.z).abs();
                    dx <= brush.radius && dz <= brush.radius
                }
                BrushShape::Diamond => {
                    let dx = (world_x - hit_point.x).abs();
                    let dz = (world_z - hit_point.z).abs();
                    dx + dz <= brush.radius
                }
            };
            
            if !in_brush {
                continue;
            }
            
            // Calculate distance for falloff
            let dx = world_x - hit_point.x;
            let dz = world_z - hit_point.z;
            let dist = (dx * dx + dz * dz).sqrt();
            
            // Calculate falloff
            let falloff = if brush.falloff > 0.0 && dist > 0.0 {
                let t = dist / brush.radius;
                (1.0 - t.powf(1.0 / brush.falloff)).max(0.0)
            } else {
                1.0
            };
            
            // Scale effect based on voxel mode
            let base_effect = if brush.voxel_mode {
                // Voxel mode: stronger, more discrete changes
                brush.strength * falloff * voxel_size
            } else {
                // Smooth mode: gentler changes
                brush.strength * falloff * 0.1
            };
            
            // Map to actual height cache index (may need interpolation for higher res)
            let cache_x = ((u * config.chunk_resolution as f32).round() as u32).min(config.chunk_resolution);
            let cache_z = ((v * config.chunk_resolution as f32).round() as u32).min(config.chunk_resolution);
            let idx = (cache_z * (config.chunk_resolution + 1) + cache_x) as usize;
            
            if idx >= data.height_cache.len() {
                continue;
            }
            
            let current_height = data.height_cache[idx];
            
            match brush.mode {
                BrushMode::Raise | BrushMode::VoxelAdd => {
                    let new_height = current_height + base_effect;
                    // Quantize to voxel grid if in voxel mode
                    data.height_cache[idx] = if brush.voxel_mode && height_step > 0.0 {
                        quantize_height(new_height, height_step)
                    } else {
                        new_height
                    };
                }
                BrushMode::Lower | BrushMode::VoxelRemove => {
                    let new_height = current_height - base_effect;
                    data.height_cache[idx] = if brush.voxel_mode && height_step > 0.0 {
                        quantize_height(new_height, height_step)
                    } else {
                        new_height
                    };
                }
                BrushMode::Smooth | BrushMode::VoxelSmooth => {
                    // Average with neighbors (more neighbors in voxel mode)
                    let neighbors = get_neighbor_heights_extended(data, cache_x, cache_z, config.chunk_resolution, brush.voxel_mode);
                    if !neighbors.is_empty() {
                        let avg = neighbors.iter().sum::<f32>() / neighbors.len() as f32;
                        let smoothed = current_height * (1.0 - base_effect * 10.0) + avg * (base_effect * 10.0);
                        data.height_cache[idx] = if brush.voxel_mode && height_step > 0.0 {
                            quantize_height(smoothed, height_step)
                        } else {
                            smoothed
                        };
                    }
                }
                BrushMode::Flatten => {
                    // Flatten to hit point height
                    let target = hit_point.y / config.height_scale;
                    let flattened = current_height * (1.0 - base_effect * 10.0) + target * (base_effect * 10.0);
                    data.height_cache[idx] = if brush.voxel_mode && height_step > 0.0 {
                        quantize_height(flattened, height_step)
                    } else {
                        flattened
                    };
                }
                BrushMode::PaintTexture => {
                    // TODO: Modify splatmap with voxel precision
                }
                BrushMode::Region | BrushMode::Fill => {
                    // Region selection and fill are handled separately
                    // These modes don't modify terrain directly during brush stroke
                }
            }
        }
    }
}

/// Quantize height to voxel grid steps
#[inline]
fn quantize_height(height: f32, step: f32) -> f32 {
    if step <= 0.0 {
        height
    } else {
        (height / step).round() * step
    }
}

/// Get heights of neighboring vertices for smoothing (extended for voxel mode)
fn get_neighbor_heights_extended(data: &TerrainData, x: u32, z: u32, resolution: u32, extended: bool) -> Vec<f32> {
    let range = if extended { 2i32 } else { 1i32 };
    let capacity = if extended { 24 } else { 8 };
    let mut heights = Vec::with_capacity(capacity);
    let stride = resolution + 1;
    
    for dz in -range..=range {
        for dx in -range..=range {
            if dx == 0 && dz == 0 {
                continue;
            }
            
            let nx = x as i32 + dx;
            let nz = z as i32 + dz;
            
            if nx >= 0 && nx <= resolution as i32 && nz >= 0 && nz <= resolution as i32 {
                let idx = (nz as u32 * stride + nx as u32) as usize;
                if idx < data.height_cache.len() {
                    heights.push(data.height_cache[idx]);
                }
            }
        }
    }
    
    heights
}

// Keyboard shortcuts moved to engine UI
