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

/// Host-app veto on terrain painting, checked by [`terrain_paint_system`].
///
/// The paint system reads the raw cursor, so on its own it happily sculpts
/// while the pointer is over ribbon buttons or a docked panel. Hosts with
/// editor chrome (the Studio engine) insert this resource and set `allowed`
/// each frame from their viewport bounds + UI focus, exactly the way the
/// other engine tools gate themselves. Hosts without chrome (the Client)
/// never insert it — absent resource means "no veto".
#[derive(Resource, Debug, Clone, Copy)]
pub struct TerrainPaintGate {
    /// `false` = the pointer is over UI chrome this frame; do not paint.
    pub allowed: bool,
}

impl Default for TerrainPaintGate {
    fn default() -> Self {
        Self { allowed: true }
    }
}

/// System for terrain painting with mouse.
///
/// Gated by [`TerrainPaintGate`] when the host inserts one, so a drag that
/// starts on a ribbon button does not carve the ground underneath it.
pub fn terrain_paint_system(
    buttons: Res<ButtonInput<MouseButton>>,
    _keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut terrain_query: Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    mut chunk_query: Query<(Entity, &mut Chunk, &GlobalTransform)>,
    brush: Res<TerrainBrush>,
    gate: Option<Res<TerrainPaintGate>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    // Only paint when LMB is pressed
    if !buttons.pressed(MouseButton::Left) {
        return;
    }

    // Host veto (pointer over editor chrome). Absent resource = no veto.
    if !gate.map(|g| g.allowed).unwrap_or(true) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    // The render camera, NOT `single()`: the Studio engine runs several
    // Camera3d entities at once (scene camera at order 0, the Slint chrome
    // overlay at order 300, the AI camera). `single()` errors out with more
    // than one, which silently disabled every brush in the engine. `order ==
    // 0` is the engine-wide convention for "the camera the user is looking
    // through"; a single-camera host also matches it.
    let Some((camera, camera_transform)) = camera_query.iter().find(|(c, _)| c.order == 0) else {
        return;
    };
    let Ok((config, mut data)) = terrain_query.single_mut() else { return };
    
    // Get cursor position
    let Some(cursor_pos) = window.cursor_position() else { return };
    
    // Raycast from cursor to terrain
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    // Heightfield raymarch against the REAL surface (`height_query`), not a
    // flat Y=0 plane — a plane hit-test picks the wrong point on any sloped
    // ground, badly wrong on a mountain.
    let Some(hit_point) = super::height_query::raycast_terrain(config, &data, ray, 2000.0, 2.0) else {
        return; // Ray never crosses the surface within range
    };
    
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

/// Apply brush effect to terrain data with voxel-based precision.
///
/// Everything here is world-space, routed through [`super::height_query`].
/// It used to index `height_cache` as a single `(chunk_resolution + 1)^2`
/// grid, but a loaded terrain's cache is ONE global raster of
/// `(chunks_x * 2 + 1) * chunk_resolution` samples per axis (see that
/// module's docs, which already name this function as a hand-rolled copy of
/// the math). With any terrain bigger than one chunk the strides disagreed,
/// so a stroke wrote scrambled cells in the raster's first corner instead of
/// the ground under the cursor — and the splat branch reallocated the global
/// splat cache down to chunk size, discarding every material weight the
/// loader had decoded.
fn apply_brush_to_chunk(
    hit_point: &Vec3,
    brush: &TerrainBrush,
    chunk: &Chunk,
    config: &TerrainConfig,
    data: &mut TerrainData,
) {
    use super::height_query::{height_at_world, set_height_at_world, set_splat_at_world};

    // No raster to write into. Sizing one here would have to guess the
    // terrain's extent; `TerrainData::resize_cache` is the one place that
    // knows it, and every spawn path calls it.
    if data.height_cache.is_empty() || data.cache_width == 0 || data.cache_height == 0 {
        return;
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
    let height_scale = config.height_scale.max(1e-3);
    // One sample step in metres — the neighbour offset the Smooth kernel uses.
    let step_m = (config.chunk_size / effective_resolution.max(1) as f32).max(1e-3);

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

            // Scale effect based on voxel mode. Normalized-height units, the
            // same as before — multiplied up to metres at the write.
            let base_effect = if brush.voxel_mode {
                // Voxel mode: stronger, more discrete changes
                brush.strength * falloff * voxel_size
            } else {
                // Smooth mode: gentler changes
                brush.strength * falloff * 0.1
            };

            let current = height_at_world(config, data, world_x, world_z);
            let delta = base_effect * height_scale;
            // Blend factor for the averaging brushes, unchanged tuning.
            let blend = (base_effect * 10.0).clamp(0.0, 1.0);

            let target = match brush.mode {
                BrushMode::Raise | BrushMode::VoxelAdd => Some(current + delta),
                BrushMode::Lower | BrushMode::VoxelRemove => Some(current - delta),
                BrushMode::Smooth | BrushMode::VoxelSmooth => {
                    // Average the four (voxel mode: eight) world-space
                    // neighbours. Sampling by world offset rather than cache
                    // index means the kernel keeps working across a chunk
                    // border instead of wrapping to the far edge of the row.
                    let mut sum = 0.0;
                    let mut count = 0.0;
                    let offsets: &[(f32, f32)] = if brush.voxel_mode {
                        &[
                            (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0),
                            (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0),
                        ]
                    } else {
                        &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)]
                    };
                    for (ox, oz) in offsets {
                        sum += height_at_world(
                            config,
                            data,
                            world_x + ox * step_m,
                            world_z + oz * step_m,
                        );
                        count += 1.0;
                    }
                    Some(current * (1.0 - blend) + (sum / count) * blend)
                }
                BrushMode::Flatten => {
                    // Flatten toward the height the cursor hit.
                    Some(current * (1.0 - blend) + hit_point.y * blend)
                }
                BrushMode::PaintTexture => {
                    // Paint material weight into the GLOBAL splat raster, at
                    // the same cell `mesh.rs::sample_splat_weights` reads.
                    set_splat_at_world(
                        config,
                        data,
                        world_x,
                        world_z,
                        brush.texture_layer.min(3),
                        blend,
                    );
                    data.splat_dirty = true;
                    None
                }
                BrushMode::Region | BrushMode::Fill => {
                    // Region select and Fill are marked-out modes with no
                    // stroke behaviour — see `SetTerrainBrushEvent`'s handler,
                    // which refuses them rather than arming a dead brush.
                    None
                }
            };

            if let Some(world_h) = target {
                let world_h = if brush.voxel_mode && height_step > 0.0 {
                    // Quantise in the same normalized space the previous
                    // implementation used, so voxel steps keep their size.
                    quantize_height(world_h / height_scale, height_step) * height_scale
                } else {
                    world_h
                };
                set_height_at_world(config, data, world_x, world_z, world_h, 1.0);
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


// Keyboard shortcuts moved to engine UI
