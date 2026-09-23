//! Terrain editor systems for runtime painting
//!
//! Supports both heightmap-based and voxel-based editing modes for
//! fine-grained terrain sculpting.
//!
//! The heightfield brushes (Raise, Lower, Smooth, Flatten, Paint) write the
//! height raster and the material map. The 3D brushes (`VoxelAdd`, `VoxelRemove`,
//! `VoxelSmooth`) write the sparse [`TerrainVolume`] instead, one CSG dab at
//! a time at the point the ray hits the terrain field (see
//! [`apply_voxel_dab`]), so they can build overhangs and dig caves.

use bevy::prelude::*;
use super::volume::{apply_shape, apply_smooth, CsgOp, CsgShape, VolumeEdit};
use super::{
    TerrainBaked, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainEditRecorder, TerrainMaterial, TerrainRoot,
    TerrainVolume, surface_data, terrain_stroke_label,
};

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
    
    /// Material slot the Paint mode lays down (see the `material` module
    /// docs): a built-in [`TerrainMaterial`] discriminant or a Space's
    /// custom slot. The Studio's material pickers (Terrain panel, Paint
    /// brush settings) set it and show it as selected.
    pub paint_material: u8,
    
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
            paint_material: TerrainMaterial::Grass as u8,
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

    /// The 3D region one dab of a volumetric brush centred on `center`
    /// covers, reaching `radius` from the centre: a sphere for `Circle`, a
    /// cube for `Square`. `Diamond` has no 3D counterpart among the CSG
    /// shapes and dabs a sphere. `VoxelSmooth` always blurs a ball
    /// (`apply_smooth`), so it is a sphere whatever the shape. The brush
    /// preview draws this same shape, so what the user sees is what the dab
    /// changes.
    pub fn voxel_shape(&self, center: Vec3) -> CsgShape {
        let radius = self.radius;
        if self.mode == BrushMode::VoxelSmooth {
            return CsgShape::Sphere { center, radius };
        }
        match self.shape {
            BrushShape::Circle | BrushShape::Diamond => CsgShape::Sphere { center, radius },
            BrushShape::Square => CsgShape::AxisBox { center, half_extents: Vec3::splat(radius) },
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
    /// Paint the brush's material slot into the material map
    PaintTexture,
    /// 3D add: unions the brush shape into the terrain volume, filled with
    /// Rock
    VoxelAdd,
    /// 3D subtract: carves the brush shape out of the terrain, heightfield
    /// included, so it digs caves and tunnels
    VoxelRemove,
    /// 3D smooth: rounds the edges of earlier 3D edits
    VoxelSmooth,
    /// Select region for bulk operations
    Region,
    /// Fill region with material
    Fill,
}

impl BrushMode {
    /// Whether this mode edits the 3D terrain volume instead of the
    /// heightfield raster.
    pub fn is_volumetric(self) -> bool {
        matches!(self, Self::VoxelAdd | Self::VoxelRemove | Self::VoxelSmooth)
    }

    /// The name the Studio's ribbon shows for this mode.
    pub fn label(self) -> &'static str {
        match self {
            Self::Raise => "Raise",
            Self::Lower => "Lower",
            Self::Smooth => "Smooth",
            Self::Flatten => "Flatten",
            Self::PaintTexture => "Paint",
            Self::VoxelAdd => "Add",
            Self::VoxelRemove => "Subtract",
            Self::VoxelSmooth => "Smooth 3D",
            Self::Region => "Region",
            Self::Fill => "Fill",
        }
    }
}

// ============================================================================
// 3D brush dabs
// ============================================================================

/// Seconds between the dabs of a held 3D brush at full strength...
const VOXEL_DAB_INTERVAL_MIN_SECS: f64 = 0.03;
/// ...and at zero strength. A dab lands at the ray's hit on the surface the
/// previous dab made, so a held Add grows toward the camera and a held
/// Subtract digs along the view ray; strength sets how fast.
const VOXEL_DAB_INTERVAL_MAX_SECS: f64 = 0.25;
/// Between two dabs of a drag, extra dabs are laid this many radii apart...
const VOXEL_DAB_SPACING: f32 = 0.5;
/// ...at most this many per tick. A gap longer than the dabs can span (the
/// cursor jumping from a near hill to ground far behind it) gets no fill, so
/// an Add never strings blobs across the air between the two.
const MAX_VOXEL_DABS_PER_TICK: usize = 8;

/// Where and when the current 3D brush stroke last dabbed, kept by
/// [`terrain_paint_system`] between frames.
#[derive(Debug, Default)]
pub struct VoxelStrokeState {
    /// Real time (seconds) and world centre of the latest dab.
    last_dab: Option<(f64, Vec3)>,
}

/// Seconds between dabs of a held 3D brush at `strength` (0 to 1).
fn voxel_dab_interval(strength: f32) -> f64 {
    let t = if strength.is_finite() { strength.clamp(0.0, 1.0) as f64 } else { 0.0 };
    VOXEL_DAB_INTERVAL_MAX_SECS + (VOXEL_DAB_INTERVAL_MIN_SECS - VOXEL_DAB_INTERVAL_MAX_SECS) * t
}

/// Centres to dab this tick, ending at `hit`: the first dab of a stroke is
/// `hit` alone; after that, points from the previous dab to `hit` spaced
/// [`VOXEL_DAB_SPACING`] radii apart, so a quick drag leaves a continuous
/// trail rather than separate blobs.
fn voxel_dab_centres(previous: Option<Vec3>, hit: Vec3, radius: f32) -> Vec<Vec3> {
    let Some(previous) = previous else {
        return vec![hit];
    };
    let spacing = (radius * VOXEL_DAB_SPACING).max(1e-3);
    let gap = previous.distance(hit);
    if !(gap.is_finite() && gap > spacing && gap <= spacing * MAX_VOXEL_DABS_PER_TICK as f32) {
        return vec![hit];
    }
    let steps = ((gap / spacing).ceil() as usize).clamp(1, MAX_VOXEL_DABS_PER_TICK);
    (1..=steps).map(|i| previous.lerp(hit, i as f32 / steps as f32)).collect()
}

/// Apply one dab of the 3D `brush` centred on `center` to `volume`.
///
/// `VoxelAdd` unions [`TerrainBrush::voxel_shape`] filled with Rock;
/// `VoxelRemove` carves it, leaving the default Rock on the walls it exposes;
/// `VoxelSmooth` blurs earlier edits inside the ball of `radius`. Any other
/// mode changes nothing.
///
/// `recorder` is told about every brick the dab can write before the write,
/// over the same box the CSG op visits, so an undo puts back exactly the
/// bricks from before the stroke. The returned edit goes to
/// `TerrainDirtyChunks::mark_volume_edit`.
pub fn apply_voxel_dab(
    brush: &TerrainBrush,
    center: Vec3,
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    recorder: Option<&mut TerrainEditRecorder>,
) -> VolumeEdit {
    let shape = brush.voxel_shape(center);
    match brush.mode {
        BrushMode::VoxelAdd | BrushMode::VoxelRemove => {
            let (lo, hi) = shape.edit_bounds(config);
            if let Some(recorder) = recorder {
                recorder.record_volume_aabb(config, volume, lo, hi);
            }
            let (op, material) = if brush.mode == BrushMode::VoxelAdd {
                (CsgOp::Add, Some(TerrainMaterial::Rock))
            } else {
                (CsgOp::Carve, None)
            };
            apply_shape(config, volume, shape, op, material)
        }
        BrushMode::VoxelSmooth => {
            // `apply_smooth` visits the lattice points of the ball's box.
            let (lo, hi) = shape.bounds();
            if let Some(recorder) = recorder {
                recorder.record_volume_aabb(config, volume, lo, hi);
            }
            apply_smooth(config, volume, center, brush.radius, brush.strength)
        }
        BrushMode::Raise
        | BrushMode::Lower
        | BrushMode::Smooth
        | BrushMode::Flatten
        | BrushMode::PaintTexture
        | BrushMode::Region
        | BrushMode::Fill => VolumeEdit::default(),
    }
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
///
/// Writes the stroke into the height raster or material map and marks the touched
/// chunks in [`TerrainDirtyChunks`]; `apply_terrain_dirty_chunks` remeshes
/// them and rebuilds their colliders once the stroke pauses.
///
/// When the host inserts a [`TerrainEditRecorder`] (the Studio engine does,
/// the Client does not), the stroke opens an edit on its first write and
/// records every tile before writing into it; the host closes the edit when
/// the button comes up and pushes it onto its undo stack.
///
/// The 3D modes write the root's [`TerrainVolume`] instead: one dab (see
/// [`apply_voxel_dab`]) per tick at the ray's hit on the terrain field,
/// ticking faster the higher the strength, with the bricks each dab can
/// write recorded before it writes. They need the height raster as much as
/// the heightfield modes do, since the field's heightfield term reads it.
pub fn terrain_paint_system(
    mut commands: Commands,
    real_time: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    _keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut terrain_query: Query<
        (Entity, &TerrainConfig, &mut TerrainData, Option<&mut TerrainVolume>, Option<&TerrainBaked>),
        With<TerrainRoot>,
    >,
    brush: Res<TerrainBrush>,
    gate: Option<Res<TerrainPaintGate>>,
    mut dirty: ResMut<TerrainDirtyChunks>,
    mut recorder: Option<ResMut<TerrainEditRecorder>>,
    mut voxel_stroke: Local<VoxelStrokeState>,
) {
    // Only paint when LMB is pressed. A fresh press starts a fresh 3D
    // stroke: its first dab lands at once and is not joined to the last one.
    if !buttons.pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Left) {
        voxel_stroke.last_dab = None;
    }
    if !buttons.pressed(MouseButton::Left) {
        return;
    }

    // Host veto (pointer over editor chrome). Absent resource = no veto.
    if !gate.map(|g| g.allowed).unwrap_or(true) {
        return;
    }

    // Every held frame of a 3D stroke counts as the stroke being live, the
    // ones between throttled dabs and the ones whose ray misses included:
    // the trimesh colliders stay deferred until the button is released.
    if brush.mode.is_volumetric() {
        dirty.hold_colliders();
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
    let Ok((root, config, mut data, volume, baked)) = terrain_query.single_mut() else { return };

    // Get cursor position
    let Some(cursor_pos) = window.cursor_position() else { return };

    // Raycast from cursor to terrain
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    // Raymarch against the REAL surface (`height_query`), not a flat Y=0
    // plane: a plane hit-test picks the wrong point on any sloped ground,
    // badly wrong on a mountain. With volumetric edits the ray traces the
    // whole terrain field, so it can land on a cave wall or an overhang.
    // The ray finds the ground the user sees (the layer bake, when there is
    // one); the stroke below still writes the base under that point.
    let Some(hit_point) = super::height_query::raycast_terrain_surface(
        config,
        surface_data(&data, baked),
        volume.as_deref(),
        ray,
        2000.0,
        2.0,
    ) else {
        return; // Ray never crosses the surface within range
    };

    // Procedural terrain has no raster to write into (see
    // `apply_brush_to_chunk`), so there is nothing to rebuild either. The 3D
    // modes stop here too: the marching-cubes mesher refuses a terrain
    // without a raster, so a volume edit there would never be drawn.
    if data.height_cache.is_empty() {
        return;
    }

    if brush.mode.is_volumetric() {
        let Some(mut volume) = volume else {
            // Every spawn path gives the root a volume; one that arrived
            // without it gets an empty one now and takes dabs from the next
            // frame, once the insert has landed.
            commands.entity(root).try_insert(TerrainVolume::default());
            return;
        };
        let now = real_time.elapsed_secs_f64();
        let previous = voxel_stroke.last_dab;
        if previous.is_some_and(|(at, _)| now - at < voxel_dab_interval(brush.strength)) {
            return;
        }
        voxel_stroke.last_dab = Some((now, hit_point));

        if let Some(recorder) = recorder.as_deref_mut() {
            if !recorder.is_recording() {
                recorder.begin(terrain_stroke_label(brush.mode), Some(root), config, &data);
            }
        }
        let mut changed = VolumeEdit::default();
        for center in voxel_dab_centres(previous.map(|(_, at)| at), hit_point, brush.radius) {
            let dab = apply_voxel_dab(&brush, center, config, &mut volume, recorder.as_deref_mut());
            changed.merge(&dab);
        }
        dirty.mark_volume_edit(config, &changed);
        return;
    }

    // The ray hit the baked surface, but the heightfield brushes write the
    // base. Move the hit into base space by the layer offset under it, or
    // Flatten over an additive layer (Noise, an Add stamp) would chase
    // base + offset every frame as the bake follows the base, and drift.
    let base_hit = {
        use super::height_query::height_at_world;
        let offset = height_at_world(config, surface_data(&data, baked), hit_point.x, hit_point.z)
            - height_at_world(config, &data, hit_point.x, hit_point.z);
        hit_point - Vec3::Y * offset
    };

    // Open the undo edit on the stroke's first write. An edit left open by
    // earlier frames of this stroke keeps collecting tiles.
    if let Some(recorder) = recorder.as_deref_mut() {
        if !recorder.is_recording() {
            recorder.begin(terrain_stroke_label(brush.mode), Some(root), config, &data);
        }
    }

    // Chunks under the brush footprint, from grid math over the terrain's
    // extent rather than a scan of chunk entities. A chunk entity sits at its
    // corner, not its centre, and the raster is global, so ground under a
    // chunk that has not streamed in yet still takes the stroke.
    let reach = brush.radius.max(0.0);
    let min_xz = Vec2::new(hit_point.x - reach, hit_point.z - reach);
    let max_xz = Vec2::new(hit_point.x + reach, hit_point.z + reach);
    let size = config.chunk_size.max(1e-3);
    let extent_x = config.chunks_x as i32;
    let extent_z = config.chunks_z as i32;
    let x0 = ((min_xz.x / size).floor() as i32).max(-extent_x);
    let x1 = ((max_xz.x / size).floor() as i32).min(extent_x);
    let z0 = ((min_xz.y / size).floor() as i32).max(-extent_z);
    let z1 = ((max_xz.y / size).floor() as i32).min(extent_z);
    for cx in x0..=x1 {
        for cz in z0..=z1 {
            apply_brush_to_chunk(
                &base_hit,
                &brush,
                IVec2::new(cx, cz),
                config,
                &mut data,
                recorder.as_deref_mut(),
            );
        }
    }

    // A paint stroke moves no height, so it leaves the height counter the
    // water's height texture re-uploads on.
    if brush.mode == BrushMode::PaintTexture {
        dirty.mark_world_rect_materials(config, min_xz, max_xz);
    } else {
        dirty.mark_world_rect(config, min_xz, max_xz);
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
/// the ground under the cursor.
///
/// `recorder` is told about every write before it happens, at the same world
/// position the write uses, so the tile it snapshots is the tile the write
/// changes.
fn apply_brush_to_chunk(
    hit_point: &Vec3,
    brush: &TerrainBrush,
    chunk_pos: IVec2,
    config: &TerrainConfig,
    data: &mut TerrainData,
    mut recorder: Option<&mut TerrainEditRecorder>,
) {
    use super::height_query::{height_at_world, paint_material_at_world, set_height_at_world};

    // No raster to write into. Sizing one here would have to guess the
    // terrain's extent; `TerrainData::resize_cache` is the one place that
    // knows it, and every spawn path calls it.
    if data.height_cache.is_empty() || data.cache_width == 0 || data.cache_height == 0 {
        return;
    }

    let chunk_world_x = chunk_pos.x as f32 * config.chunk_size;
    let chunk_world_z = chunk_pos.y as f32 * config.chunk_size;

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
                BrushMode::Raise => Some(current + delta),
                BrushMode::Lower => Some(current - delta),
                BrushMode::Smooth => {
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
                    // Paint the brush's material slot into the GLOBAL
                    // material map, at the cell every material reader
                    // samples back.
                    if let Some(recorder) = recorder.as_deref_mut() {
                        recorder.record_world_point(config, data, world_x, world_z);
                    }
                    paint_material_at_world(config, data, world_x, world_z, brush.paint_material, blend);
                    None
                }
                BrushMode::Region | BrushMode::Fill => {
                    // Region select and Fill are marked-out modes with no
                    // stroke behaviour — see `SetTerrainBrushEvent`'s handler,
                    // which refuses them rather than arming a dead brush.
                    None
                }
                BrushMode::VoxelAdd | BrushMode::VoxelRemove | BrushMode::VoxelSmooth => {
                    // The 3D modes write the volume, never the raster:
                    // `terrain_paint_system` sends them to `apply_voxel_dab`.
                    None
                }
            };

            if let Some(world_h) = target {
                let world_h = if brush.voxel_mode && height_step > 0.0 {
                    // Quantise in normalized space so voxel steps keep their
                    // size, and so the step grid is anchored at the band
                    // floor (`height_offset`) the R16 values count up from.
                    config.world_height(quantize_height(
                        config.normalized_height(world_h),
                        height_step,
                    ))
                } else {
                    world_h
                };
                if let Some(recorder) = recorder.as_deref_mut() {
                    recorder.record_world_point(config, data, world_x, world_z);
                }
                // Clamped after quantising, since rounding to a step can land
                // past the band ceiling that Save encodes as 1.0.
                set_height_at_world(config, data, world_x, world_z, config.clamp_to_saved_band(world_h), 1.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{apply_terrain_tiles, TerrainTileSide};

    #[test]
    fn brush_stroke_across_a_tile_border_records_both_tiles_before_writing() {
        // 3 x 3 chunks of 8 x 8 cells. The brush below writes cell column 7
        // (tile column 0) and columns 8 and 9 (tile column 1), all in tile
        // row 1.
        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = (i % 7) as f32 * 0.01;
        }
        let original = data.clone();
        let brush = TerrainBrush {
            radius: 3.0,
            strength: 1.0,
            falloff: 0.0,
            mode: BrushMode::Raise,
            voxel_mode: false,
            ..TerrainBrush::default()
        };
        let hit = Vec3::new(0.7, 0.0, 8.0);

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(brush.mode), None, &config, &data));
        // The chunks `terrain_paint_system` visits for this footprint.
        for cx in -1..=0 {
            apply_brush_to_chunk(&hit, &brush, IVec2::new(cx, 0), &config, &mut data, Some(&mut recorder));
        }
        let edit = recorder.finish(None, &data).expect("the stroke raised the ground");
        assert_eq!(edit.label, "Sculpt Terrain");
        let tiles: Vec<UVec2> = edit.tiles.iter().map(|t| t.tile).collect();
        assert_eq!(tiles, vec![UVec2::new(0, 1), UVec2::new(1, 1)]);

        // Undo puts back exactly the raster from before the stroke, which
        // holds only if each tile was snapshotted before its first write.
        let mut undone = data.clone();
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert_eq!(undone.height_cache, original.height_cache);
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::After).unwrap();
        assert_eq!(undone.height_cache, data.height_cache);
    }

    #[test]
    fn a_paint_stroke_lays_down_the_brush_material_and_undoes_exactly() {
        use crate::terrain::height_query::material_at_world;

        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let original = data.clone();
        let basalt = TerrainMaterial::Basalt.to_u8();
        let brush = TerrainBrush {
            radius: 3.0,
            strength: 1.0,
            falloff: 0.0,
            mode: BrushMode::PaintTexture,
            paint_material: basalt,
            voxel_mode: false,
            ..TerrainBrush::default()
        };
        let hit = Vec3::new(1.0, 0.0, 1.0);

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(brush.mode), None, &config, &data));
        for cx in -1..=0 {
            for cz in -1..=0 {
                apply_brush_to_chunk(&hit, &brush, IVec2::new(cx, cz), &config, &mut data, Some(&mut recorder));
            }
        }
        assert!(data.material_dirty);
        assert_eq!(material_at_world(&config, &data, 1.0, 1.0).map(|s| s.primary), Some(basalt));
        let far = material_at_world(&config, &data, 12.0, -12.0).expect("the stroke allocated the layer");
        assert_eq!(far.primary, TerrainMaterial::Grass.to_u8(), "ground outside the brush stays Grass");
        assert_eq!(data.height_cache, original.height_cache, "painting leaves heights alone");

        let edit = recorder.finish(None, &data).expect("the stroke painted");
        assert_eq!(edit.label, "Paint Terrain");
        assert!(edit.tiles.iter().all(|t| t.heights_before.is_empty() && t.touches_materials()));
        let mut undone = data.clone();
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert!(undone.material_cache.is_empty(), "the stroke allocated the layer, so undo drops it");
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::After).unwrap();
        assert_eq!(undone.material_cache, data.material_cache);
    }

    #[test]
    fn voxel_shapes_follow_the_brush_shape() {
        let c = Vec3::new(1.0, 2.0, 3.0);
        let brush = |mode: BrushMode, shape: BrushShape| TerrainBrush { mode, shape, radius: 4.0, ..TerrainBrush::default() };
        assert_eq!(
            brush(BrushMode::VoxelAdd, BrushShape::Circle).voxel_shape(c),
            CsgShape::Sphere { center: c, radius: 4.0 }
        );
        assert_eq!(
            brush(BrushMode::VoxelRemove, BrushShape::Square).voxel_shape(c),
            CsgShape::AxisBox { center: c, half_extents: Vec3::splat(4.0) }
        );
        assert_eq!(
            brush(BrushMode::VoxelRemove, BrushShape::Diamond).voxel_shape(c),
            CsgShape::Sphere { center: c, radius: 4.0 }
        );
        // Smooth blurs a ball whatever the shape.
        assert_eq!(
            brush(BrushMode::VoxelSmooth, BrushShape::Square).voxel_shape(c),
            CsgShape::Sphere { center: c, radius: 4.0 }
        );
        assert!(BrushMode::VoxelSmooth.is_volumetric());
        assert!(!BrushMode::Raise.is_volumetric());
        // The names the ribbon buttons carry.
        assert_eq!(BrushMode::VoxelAdd.label(), "Add");
        assert_eq!(BrushMode::VoxelRemove.label(), "Subtract");
    }

    #[test]
    fn a_drag_fills_the_gap_between_dabs_but_never_a_jump() {
        let start = Vec3::new(0.0, 5.0, 0.0);
        // First dab of a stroke: just the hit.
        assert_eq!(voxel_dab_centres(None, start, 2.0), vec![start]);
        // Within one spacing (1 m at radius 2): just the hit.
        assert_eq!(voxel_dab_centres(Some(start), start + Vec3::X * 0.8, 2.0), vec![start + Vec3::X * 0.8]);
        // A 3 m drag at 1 m spacing: three evenly spaced dabs ending at the hit.
        let hit = start + Vec3::X * 3.0;
        let centres = voxel_dab_centres(Some(start), hit, 2.0);
        assert_eq!(centres.len(), 3);
        assert!((centres[0] - (start + Vec3::X)).length() < 1e-5);
        assert_eq!(*centres.last().unwrap(), hit);
        // A jump past what the dabs can span gets no fill.
        let far = start + Vec3::X * 50.0;
        assert_eq!(voxel_dab_centres(Some(start), far, 2.0), vec![far]);
    }

    #[test]
    fn stronger_3d_brushes_dab_more_often() {
        assert!(voxel_dab_interval(1.0) < voxel_dab_interval(0.2));
        assert!(voxel_dab_interval(0.2) < voxel_dab_interval(0.0));
        assert_eq!(voxel_dab_interval(5.0), voxel_dab_interval(1.0));
        assert_eq!(voxel_dab_interval(f32::NAN), voxel_dab_interval(0.0));
    }

    #[test]
    fn a_subtract_dab_digs_under_the_ground_and_undoes_exactly() {
        use crate::terrain::{apply_terrain_bricks, sample_field_parts};

        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let ground = config.normalized_height(10.0);
        data.height_cache.iter_mut().for_each(|h| *h = ground);

        let brush = TerrainBrush {
            mode: BrushMode::VoxelRemove,
            shape: BrushShape::Square,
            radius: 3.0,
            ..TerrainBrush::default()
        };
        let mut volume = TerrainVolume::new();
        let below = Vec3::new(1.0, 9.0, 1.0);
        assert!(sample_field_parts(&config, &data, &volume, below).is_solid(), "ground before the dab");

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(brush.mode), None, &config, &data));
        let edit = apply_voxel_dab(&brush, Vec3::new(1.0, 10.0, 1.0), &config, &mut volume, Some(&mut recorder));
        assert!(!edit.is_empty());
        assert!(!sample_field_parts(&config, &data, &volume, below).is_solid(), "the box is dug out");
        assert!(
            sample_field_parts(&config, &data, &volume, Vec3::new(12.0, 9.0, 12.0)).is_solid(),
            "ground well away from the box stays"
        );

        let recorded = recorder.finish_with_volume(None, &data, &volume).expect("the dab changed bricks");
        assert_eq!(recorded.label, "Subtract Terrain");
        assert!(recorded.tiles.is_empty());
        let mut undone = volume.clone();
        apply_terrain_bricks(&config, &mut undone, &recorded.bricks, TerrainTileSide::Before).unwrap();
        assert!(undone.is_empty());
        assert!(sample_field_parts(&config, &data, &undone, below).is_solid(), "undo fills the hole back in");
    }

    #[test]
    fn heightfield_modes_do_not_touch_the_volume() {
        let config = TerrainConfig { chunk_size: 16.0, chunk_resolution: 8, chunks_x: 1, chunks_z: 1, ..TerrainConfig::default() };
        let brush = TerrainBrush { mode: BrushMode::Raise, ..TerrainBrush::default() };
        let mut volume = TerrainVolume::new();
        assert!(apply_voxel_dab(&brush, Vec3::ZERO, &config, &mut volume, None).is_empty());
        assert!(volume.is_empty());
    }
}
