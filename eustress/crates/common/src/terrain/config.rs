//! Terrain configuration and data structures

use bevy::prelude::*;

use super::material::{MaterialCell, MATERIAL_SLOT_NONE};
use super::material_slots::TerrainSlotPalette;

/// Configuration for terrain generation and rendering.
///
/// Both a `Component` (per-chunk override) *and* a `Resource`
/// (world-level defaults) — the part-to-terrain converter reads it
/// as a `Res<TerrainConfig>`, and the chunk spawner copies the
/// resource into a matching component on each chunk entity so
/// reflection-driven tools (Inspector / TOML mirror) still pick it
/// up per-entity.
// 0.19: Resource is now a subtrait of Component. TerrainConfig is load-bearing
// as a Component (spawned on the terrain root, queried by chunk/LOD/water/editor
// systems), so keep Component and drop Resource; the lone Res reader
// (part_to_terrain) was converted to a query.
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct TerrainConfig {
    /// Size of each chunk in world units
    pub chunk_size: f32,
    
    /// Resolution of each chunk (vertices per side)
    pub chunk_resolution: u32,
    
    /// Number of chunks in X direction (from center)
    pub chunks_x: u32,
    
    /// Number of chunks in Z direction (from center)
    pub chunks_z: u32,

    /// Chunk at the centre of the grid, which spans `center_chunk.x - chunks_x ..= center_chunk.x + chunks_x`
    /// in X and likewise in Z. Zero centres the grid on the world origin.
    pub center_chunk: IVec2,

    /// Number of LOD levels (0 = highest detail)
    pub lod_levels: u32,
    
    /// Distance thresholds for each LOD level
    pub lod_distances: Vec<f32>,
    
    /// Maximum view distance for chunk culling
    pub view_distance: f32,
    
    /// World-space range (metres) that normalized heights `[0, 1]` span,
    /// measured up from `height_offset`.
    pub height_scale: f32,

    /// World Y of normalized height 0. Negative values let the surface sit
    /// below world Y = 0 (seabeds, basins, digging into a flat plate).
    /// Convert with [`TerrainConfig::world_height`] and
    /// [`TerrainConfig::normalized_height`] rather than scaling by
    /// `height_scale` alone, which silently drops this term.
    pub height_offset: f32,

    /// Seed for procedural generation
    pub seed: u32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64.0,
            chunk_resolution: 32,
            chunks_x: 3,
            chunks_z: 3,
            center_chunk: IVec2::ZERO,
            lod_levels: 4,
            lod_distances: vec![64.0, 128.0, 256.0, 512.0],
            view_distance: 512.0,
            height_scale: 50.0,
            height_offset: 0.0,
            seed: 42,
        }
    }
}

impl TerrainConfig {
    /// Create a small terrain for testing
    pub fn small() -> Self {
        Self {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 4,
            lod_distances: vec![250.0, 500.0, 750.0, 1000.0],
            view_distance: 2500.0,
            height_scale: 20.0,
            ..default()
        }
    }
    
    /// Create a large terrain for production
    pub fn large() -> Self {
        Self {
            chunk_size: 128.0,
            chunk_resolution: 128,
            chunks_x: 8,
            chunks_z: 8,
            lod_levels: 5,
            lod_distances: vec![200.0, 400.0, 800.0, 1600.0, 3200.0],
            view_distance: 4000.0,
            height_scale: 100.0,
            ..default()
        }
    }
    
    /// Create a massive 10km² terrain (3.16km x 3.16km)
    /// 
    /// Total area: ~10,000,000 m² (10 km²)
    /// Chunk layout: 25x25 chunks = 625 chunks total
    /// Each chunk: 128m x 128m = 16,384 m²
    /// Total size: 3,200m x 3,200m = 10,240,000 m²
    pub fn massive_10km() -> Self {
        Self {
            chunk_size: 128.0,           // 128m per chunk
            chunk_resolution: 64,         // 64x64 vertices per chunk (balanced for performance)
            chunks_x: 12,                 // 25 chunks across (12 on each side of center + center)
            chunks_z: 12,                 // 25 chunks deep
            center_chunk: IVec2::ZERO,    // Centred on the world origin
            lod_levels: 6,                // 6 LOD levels for massive view distances
            lod_distances: vec![
                200.0,   // LOD 0: Full detail within 200m
                500.0,   // LOD 1: High detail within 500m
                1000.0,  // LOD 2: Medium detail within 1km
                2000.0,  // LOD 3: Low detail within 2km
                4000.0,  // LOD 4: Very low detail within 4km
                8000.0,  // LOD 5: Minimal detail beyond 4km
            ],
            view_distance: 10000.0,       // 10km view distance
            height_scale: 500.0,          // Tall mountains (500m max height)
            height_offset: 0.0,
            seed: 12345,                  // Reproducible seed
        }
    }
    
    /// Create an epic 10km² terrain with extreme detail
    /// 
    /// Higher resolution for more detailed terrain at the cost of performance.
    /// Recommended for high-end systems only.
    pub fn epic_10km() -> Self {
        Self {
            chunk_size: 128.0,           // 128m per chunk
            chunk_resolution: 128,        // 128x128 vertices per chunk (high detail)
            chunks_x: 12,                 // 25 chunks across
            chunks_z: 12,                 // 25 chunks deep
            center_chunk: IVec2::ZERO,    // Centred on the world origin
            lod_levels: 6,
            lod_distances: vec![
                300.0,   // LOD 0: Full detail within 300m
                600.0,   // LOD 1
                1200.0,  // LOD 2
                2400.0,  // LOD 3
                4800.0,  // LOD 4
                9600.0,  // LOD 5
            ],
            view_distance: 12000.0,       // 12km view distance
            height_scale: 800.0,          // Very tall mountains (800m max)
            height_offset: 0.0,
            seed: 54321,
        }
    }

    /// World Y (metres) of a normalized `height_cache` sample. Every reader
    /// of the cache converts through here so `height_offset` is applied in
    /// exactly one place.
    #[inline]
    pub fn world_height(&self, normalized: f32) -> f32 {
        self.height_offset + normalized * self.height_scale
    }

    /// Normalized `height_cache` value for a world Y, the inverse of
    /// [`Self::world_height`]. A zero `height_scale` has no inverse, so it
    /// maps every height to 0 (the band floor) instead of dividing by zero.
    #[inline]
    pub fn normalized_height(&self, world_y: f32) -> f32 {
        if self.height_scale.abs() <= f32::EPSILON {
            return 0.0;
        }
        (world_y - self.height_offset) / self.height_scale
    }

    /// World Y limited to what an R16 save can encode,
    /// `[world_height(0), world_height(1)]`, so an edit never shows a surface
    /// that Save would clamp away. A raster holding raw world heights (unit
    /// scale, zero offset: the voxel loader and a fresh heightmap import) has
    /// no band to respect, so its heights pass through unchanged.
    #[inline]
    pub fn clamp_to_saved_band(&self, world_y: f32) -> f32 {
        if (self.height_scale - 1.0).abs() <= f32::EPSILON && self.height_offset == 0.0 {
            return world_y;
        }
        let (a, b) = (self.world_height(0.0), self.world_height(1.0));
        // max/min rather than clamp, which panics on a NaN or inverted bound.
        world_y.max(a.min(b)).min(a.max(b))
    }

    /// Calculate total terrain size in meters
    pub fn total_size(&self) -> (f32, f32) {
        let width = (self.chunks_x * 2 + 1) as f32 * self.chunk_size;
        let depth = (self.chunks_z * 2 + 1) as f32 * self.chunk_size;
        (width, depth)
    }
    
    /// World XZ rectangle `(min, max)` the chunk grid covers. Chunk `c` spans
    /// `[c * chunk_size, (c + 1) * chunk_size]` from its corner, and the grid
    /// runs from [`Self::chunk_min`] to [`Self::chunk_max`], so the extent is
    /// one chunk wider on the positive side of the centre chunk's corner than
    /// on the negative side.
    pub fn footprint_xz(&self) -> (Vec2, Vec2) {
        let (lo, hi) = (self.chunk_min(), self.chunk_max());
        let min = Vec2::new(lo.x as f32 * self.chunk_size, lo.y as f32 * self.chunk_size);
        let max = Vec2::new(
            (hi.x as f32 + 1.0) * self.chunk_size,
            (hi.y as f32 + 1.0) * self.chunk_size,
        );
        (min, max)
    }

    /// `(chunks_x, chunks_z)` as chunk offsets, held to the `i32` range.
    fn half_extents(&self) -> IVec2 {
        let offset = |n: u32| n.min(i32::MAX as u32) as i32;
        IVec2::new(offset(self.chunks_x), offset(self.chunks_z))
    }

    /// Lowest chunk coordinate on the grid (inclusive).
    pub fn chunk_min(&self) -> IVec2 {
        self.center_chunk.saturating_sub(self.half_extents())
    }

    /// Highest chunk coordinate on the grid (inclusive).
    pub fn chunk_max(&self) -> IVec2 {
        self.center_chunk.saturating_add(self.half_extents())
    }

    /// Whether `chunk` lies on the grid.
    pub fn contains_chunk(&self, chunk: IVec2) -> bool {
        let (min, max) = (self.chunk_min(), self.chunk_max());
        chunk.x >= min.x && chunk.x <= max.x && chunk.y >= min.y && chunk.y <= max.y
    }

    /// Grid-relative index of `chunk`, counted from [`Self::chunk_min`], or
    /// `None` off the grid. Chunk `c` owns the raster tile starting at cell
    /// `chunk_grid_index(c) * chunk_resolution`, the block its `.r16` and
    /// matmap PNG load into.
    pub fn chunk_grid_index(&self, chunk: IVec2) -> Option<UVec2> {
        if !self.contains_chunk(chunk) {
            return None;
        }
        let min = self.chunk_min();
        Some(UVec2::new(chunk.x.abs_diff(min.x), chunk.y.abs_diff(min.y)))
    }

    /// Every chunk on the grid, row by row (Z outer, X inner).
    pub fn grid_chunks(&self) -> impl Iterator<Item = IVec2> {
        let (min, max) = (self.chunk_min(), self.chunk_max());
        (min.y..=max.y).flat_map(move |z| (min.x..=max.x).map(move |x| IVec2::new(x, z)))
    }

    /// Global raster UV (0..1 across the whole raster, unclamped) of point
    /// `(u, v)` (each 0..1 across the chunk) of chunk `chunk`: its offset
    /// from [`Self::chunk_min`] plus `(u, v)`, over the chunks across the
    /// grid. The offset is measured from the centre chunk, with the half
    /// extent added after `u`, which keeps the arithmetic on small numbers
    /// wherever the grid sits and makes a grid centred on the origin evaluate
    /// exactly `(chunk.x + u + chunks_x) / (2 * chunks_x + 1)`. The chunk
    /// meshers and the volume lattice both sample through this, so a lattice
    /// height equals the mesh vertex over it bit for bit.
    pub fn chunk_point_uv(&self, chunk: IVec2, u: f32, v: f32) -> Vec2 {
        let total_x = (self.chunks_x * 2 + 1) as f32;
        let total_z = (self.chunks_z * 2 + 1) as f32;
        let dx = (i64::from(chunk.x) - i64::from(self.center_chunk.x)) as f32;
        let dz = (i64::from(chunk.y) - i64::from(self.center_chunk.y)) as f32;
        Vec2::new(
            (dx + u + self.chunks_x as f32) / total_x,
            (dz + v + self.chunks_z as f32) / total_z,
        )
    }

    /// Global raster UV (unclamped) of world XZ `(world_x, world_z)`, the
    /// inverse of [`Self::chunk_point_uv`]: `world_x / chunk_size` is a chunk
    /// coordinate plus the fraction across that chunk, so no chunk lookup is
    /// needed.
    pub fn world_to_uv(&self, world_x: f32, world_z: f32) -> Vec2 {
        let size = self.chunk_size.max(1e-3);
        let total_x = (self.chunks_x * 2 + 1) as f32;
        let total_z = (self.chunks_z * 2 + 1) as f32;
        Vec2::new(
            (world_x / size - self.center_chunk.x as f32 + self.chunks_x as f32) / total_x,
            (world_z / size - self.center_chunk.y as f32 + self.chunks_z as f32) / total_z,
        )
    }

    /// Calculate total terrain area in square meters
    pub fn total_area_m2(&self) -> f32 {
        let (w, d) = self.total_size();
        w * d
    }
    
    /// Calculate total terrain area in square kilometers
    pub fn total_area_km2(&self) -> f32 {
        self.total_area_m2() / 1_000_000.0
    }
    
    /// Get total chunk count
    pub fn total_chunks(&self) -> u32 {
        (self.chunks_x * 2 + 1) * (self.chunks_z * 2 + 1)
    }
    
    /// Get LOD level for a given distance
    pub fn lod_for_distance(&self, distance: f32) -> u32 {
        for (i, &threshold) in self.lod_distances.iter().enumerate() {
            if distance < threshold {
                return i as u32;
            }
        }
        self.lod_levels.saturating_sub(1)
    }
    
    /// Get resolution for a given LOD level
    pub fn resolution_for_lod(&self, lod: u32) -> u32 {
        (self.chunk_resolution >> lod).max(4)
    }
}

/// Runtime terrain data (height raster, material map)
#[derive(Component, Clone, Reflect, Default, Debug)]
#[reflect(Component)]
pub struct TerrainData {
    /// Heightmap image handle (grayscale, 16-bit preferred)
    pub heightmap: Option<Handle<Image>>,

    /// Cached height values (populated from heightmap or procedural)
    #[reflect(ignore)]
    pub height_cache: Vec<f32>,

    /// Width of height cache
    pub cache_width: u32,

    /// Height of height cache
    pub cache_height: u32,

    /// Per-cell material identity, one [`MaterialCell`] per `height_cache`
    /// sample in the same row-major order: `[id_a, id_b, blend_b, 0]`, two
    /// material slots and the weight of the second (see the `material`
    /// module docs). Empty when the terrain has no material layer
    /// (procedural terrain), which colours by altitude instead; otherwise
    /// exactly `cache_width * cache_height` cells. Written through
    /// `height_query::paint_material_at_world`, loaded from and saved to
    /// `matmap/*.png`.
    #[reflect(ignore)]
    pub material_cache: Vec<MaterialCell>,

    /// Set by every writer of `material_cache` (paint, undo, load), so a GPU
    /// copy of the material map knows to re-upload.
    #[reflect(ignore)]
    pub material_dirty: bool,

    /// The swatch colour of every material slot, which the vertex-colour
    /// meshers paint cells with. A copy of the active
    /// `TerrainMaterialSlots::palette`, kept in step by
    /// `sync_terrain_slot_palette`, so a Space's custom slots colour the
    /// mesh even though the meshers only see config and data. Defaults to
    /// the built-in slots; cheap to clone (shared).
    #[reflect(ignore)]
    pub slot_palette: TerrainSlotPalette,

    /// Columns without ground are holes: when true, a raster cell whose material
    /// `id_a` is `MATERIAL_SLOT_NONE` has no ground, so meshes, colliders and
    /// terrain raycasts leave it out. Imported voxel terrain sets it; every
    /// other terrain keeps a full surface.
    pub sparse_surface: bool,
}

impl TerrainData {
    /// Create procedural terrain data (no heightmap)
    pub fn procedural() -> Self {
        Self::default()
    }
    
    /// Create terrain data from heightmap
    pub fn from_heightmap(heightmap: Handle<Image>) -> Self {
        Self {
            heightmap: Some(heightmap),
            ..default()
        }
    }
    
    /// Sample height at normalized world UV coordinates (0-1 across entire terrain)
    /// Uses bilinear interpolation for smooth sampling
    pub fn sample_height(&self, world_u: f32, world_v: f32) -> f32 {
        if self.height_cache.is_empty() || self.cache_width == 0 || self.cache_height == 0 {
            return 0.0;
        }
        
        // Convert to pixel coordinates
        let px = world_u * (self.cache_width - 1) as f32;
        let pz = world_v * (self.cache_height - 1) as f32;
        
        // Integer and fractional parts for bilinear interpolation
        let x0 = px.floor() as usize;
        let z0 = pz.floor() as usize;
        let x1 = (x0 + 1).min(self.cache_width as usize - 1);
        let z1 = (z0 + 1).min(self.cache_height as usize - 1);
        let fx = px - px.floor();
        let fz = pz - pz.floor();
        
        // Sample four corners
        let stride = self.cache_width as usize;
        let h00 = self.height_cache.get(z0 * stride + x0).copied().unwrap_or(0.0);
        let h10 = self.height_cache.get(z0 * stride + x1).copied().unwrap_or(0.0);
        let h01 = self.height_cache.get(z1 * stride + x0).copied().unwrap_or(0.0);
        let h11 = self.height_cache.get(z1 * stride + x1).copied().unwrap_or(0.0);
        
        // Bilinear interpolation
        let h0 = h00 + (h10 - h00) * fx;
        let h1 = h01 + (h11 - h01) * fx;
        h0 + (h1 - h0) * fz
    }
    
    /// Initialize/resize height cache for world size
    pub fn resize_cache(&mut self, config: &TerrainConfig) {
        let world_width = (config.chunks_x * 2 + 1) * config.chunk_resolution;
        let world_height = (config.chunks_z * 2 + 1) * config.chunk_resolution;
        
        self.cache_width = world_width;
        self.cache_height = world_height;
        self.height_cache.resize((world_width * world_height) as usize, 0.0);
    }

    /// The material layer covers every cell of the raster, the only layout
    /// material reads and writes index into.
    pub fn has_material_layer(&self) -> bool {
        let total = self.cache_width as usize * self.cache_height as usize;
        total > 0 && self.material_cache.len() == total
    }

    /// Cache cell `(column, row)` nearest global `world_u, world_v`: the one
    /// cell a write there lands in. [`Self::set_height`] and
    /// `height_query::paint_material_at_world` both index through this, so
    /// undo recording (`height_query::cache_cell_at_world`) names exactly the
    /// cell they write. Callers check the cache dimensions are non-zero.
    #[inline]
    pub fn cell_at_uv(&self, world_u: f32, world_v: f32) -> (usize, usize) {
        let x = (world_u * self.cache_width.saturating_sub(1) as f32).round() as usize;
        let z = (world_v * self.cache_height.saturating_sub(1) as f32).round() as usize;
        (x, z)
    }

    /// Whether raster cell `index` (row-major, like `height_cache`) is a
    /// hole: on a sparse surface (see [`Self::sparse_surface`]), a cell whose
    /// material `id_a` is [`MATERIAL_SLOT_NONE`]. Never on a full surface,
    /// nor for an index past the material layer.
    #[inline]
    pub fn cell_is_hole(&self, index: usize) -> bool {
        self.sparse_surface && self.material_cache.get(index).is_some_and(|cell| cell[0] == MATERIAL_SLOT_NONE)
    }

    /// Whether the mesh vertex at `(u, v)` (each 0..1 across the chunk) of
    /// chunk `chunk` stands on a hole: the raster cell its global UV
    /// ([`TerrainConfig::chunk_point_uv`], clamped onto the raster) lands in
    /// ([`Self::cell_at_uv`]) is one ([`Self::cell_is_hole`]). The heightfield
    /// mesher, the marching-cubes lattice and the terrain raycasts all judge
    /// holes through this, at the vertices they share.
    pub fn point_is_hole(&self, config: &TerrainConfig, chunk: IVec2, u: f32, v: f32) -> bool {
        if !self.sparse_surface || self.cache_width == 0 || self.cache_height == 0 {
            return false;
        }
        let uv = config.chunk_point_uv(chunk, u, v);
        let (x, z) = self.cell_at_uv(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0));
        self.cell_is_hole(z * self.cache_width as usize + x)
    }

    /// Whether chunk `chunk` has any ground: every chunk of a full surface
    /// does; on a sparse one, a chunk does when a raster cell of its tile
    /// (the `chunk_resolution`-square block starting at cell
    /// `chunk_grid_index(chunk) * chunk_resolution`) is not a hole. A chunk
    /// off the grid has none. On a raster laid out by [`Self::resize_cache`]
    /// with at least 4 cells a chunk, every quad of a chunk's mesh, at any
    /// LOD, has a corner standing on its tile, so a chunk without ground keeps
    /// no quad at all.
    pub fn chunk_has_ground(&self, config: &TerrainConfig, chunk: IVec2) -> bool {
        if !self.sparse_surface {
            return true;
        }
        let Some(tile) = config.chunk_grid_index(chunk) else {
            return false;
        };
        let side = config.chunk_resolution as usize;
        let width = self.cache_width as usize;
        let x0 = (tile.x as usize).saturating_mul(side);
        let z0 = (tile.y as usize).saturating_mul(side);
        let x1 = x0.saturating_add(side).min(width);
        let z1 = z0.saturating_add(side).min(self.cache_height as usize);
        (z0..z1).any(|z| (x0..x1).any(|x| !self.cell_is_hole(z * width + x)))
    }

    /// Set height at world UV coordinates (for editing)
    pub fn set_height(&mut self, world_u: f32, world_v: f32, height: f32) {
        if self.height_cache.is_empty() || self.cache_width == 0 || self.cache_height == 0 {
            return;
        }

        let (x, z) = self.cell_at_uv(world_u, world_v);
        let idx = z * self.cache_width as usize + x;
        
        if idx < self.height_cache.len() {
            self.height_cache[idx] = height;
        }
    }
}

/// The world-height range normalized raster samples span: normalized 0 is
/// `offset`, normalized 1 is `offset + scale`. A config's `height_offset` and
/// `height_scale`, and what an R16 save can encode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightBand {
    pub offset: f32,
    pub scale: f32,
}

impl HeightBand {
    /// The band `config` stores its raster in.
    pub fn of(config: &TerrainConfig) -> Self {
        Self { offset: config.height_offset, scale: config.height_scale }
    }

    /// World Y of normalized sample `normalized`, with the arithmetic of
    /// [`TerrainConfig::world_height`].
    #[inline]
    pub fn world(self, normalized: f32) -> f32 {
        self.offset + normalized * self.scale
    }

    /// Normalized sample of world Y `world_y`, with the arithmetic (and the
    /// zero-scale guard) of [`TerrainConfig::normalized_height`].
    #[inline]
    pub fn normalized(self, world_y: f32) -> f32 {
        if self.scale.abs() <= f32::EPSILON {
            return 0.0;
        }
        (world_y - self.offset) / self.scale
    }

    /// Re-express `samples`, normalized in this band, in band `to`, so each
    /// still stands for the world height it did. Anything that keeps copies
    /// of a terrain's raster (undo entries, an open stroke, the layer bake)
    /// runs its copies through this when the terrain's band moves.
    pub fn rebase_into(self, to: HeightBand, samples: &mut [f32]) {
        if self == to {
            return;
        }
        for sample in samples {
            *sample = to.normalized(self.world(*sample));
        }
    }
}

/// The grid a terrain's derived pieces (scatter batches, water body surfaces)
/// are built on: a change means none of them fits, and that includes the grid
/// moving to another centre chunk. The height band is left out, since Save
/// re-expresses heights in a new band without moving them. `chunk_size` is
/// compared by its bits so a NaN size still equals itself and does not
/// rebuild everything every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainGridKey {
    chunk_size_bits: u32,
    chunk_resolution: u32,
    chunks_x: u32,
    chunks_z: u32,
    center_chunk: IVec2,
}

impl TerrainGridKey {
    /// The grid `config` lays its chunks on.
    pub fn of(config: &TerrainConfig) -> Self {
        Self {
            chunk_size_bits: config.chunk_size.to_bits(),
            chunk_resolution: config.chunk_resolution,
            chunks_x: config.chunks_x,
            chunks_z: config.chunks_z,
            center_chunk: config.center_chunk,
        }
    }
}

/// Half of one R16 code in normalized units. A sample that far past 0 or 1
/// still encodes to the end code it would have clamped to, so it needs no
/// wider band.
const R16_HALF_STEP: f32 = 0.5 / 65535.0;
/// Headroom a widened band gets past the data, as a fraction of its span.
const BAND_MARGIN_FRACTION: f32 = 0.05;
/// Least headroom a widened band gets past the data, in world units.
const BAND_MARGIN_MIN: f32 = 1.0;

impl TerrainConfig {
    /// The band an R16 save needs to hold every cached height of `data`, or
    /// `None` when this config's band already holds them (or there are no
    /// finite heights to hold).
    ///
    /// The result covers the current band as well as the data, so it only
    /// ever widens and edits keep the headroom they had, and it reaches a
    /// margin past the data on each side the data escaped, so a sculpt that
    /// just crossed the band does not force a new band on the next save.
    pub fn band_covering(&self, data: &TerrainData) -> Option<HeightBand> {
        let (lowest, highest) = data
            .height_cache
            .iter()
            .map(|&n| self.world_height(n))
            .filter(|w| w.is_finite())
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), w| (lo.min(w), hi.max(w)));
        if !(lowest.is_finite() && highest.is_finite()) {
            return None;
        }
        let (a, b) = (self.world_height(0.0), self.world_height(1.0));
        let (band_lo, band_hi) = (a.min(b), a.max(b));
        let slack = R16_HALF_STEP * self.height_scale.abs();
        let escapes_low = lowest < band_lo - slack;
        let escapes_high = highest > band_hi + slack;
        if !escapes_low && !escapes_high {
            return None;
        }
        let mut lo = band_lo.min(lowest);
        let mut hi = band_hi.max(highest);
        let margin = ((hi - lo) * BAND_MARGIN_FRACTION).max(BAND_MARGIN_MIN);
        if escapes_low {
            lo -= margin;
        }
        if escapes_high {
            hi += margin;
        }
        let band = HeightBand { offset: lo, scale: hi - lo };
        (band.offset.is_finite() && band.scale.is_finite() && band.scale > 0.0).then_some(band)
    }
}

/// Move `config` to band `to`, re-expressing every cached height of `data`
/// so its world height stays put. Returns the band it left, which every other
/// holder of this terrain's normalized samples must rebase from (see
/// [`HeightBand::rebase_into`]).
pub fn rebase_height_band(config: &mut TerrainConfig, data: &mut TerrainData, to: HeightBand) -> HeightBand {
    let from = HeightBand::of(config);
    from.rebase_into(to, &mut data.height_cache);
    config.height_offset = to.offset;
    config.height_scale = to.scale;
    from
}

/// Widen `config`'s band until it holds every cached height of `data` (see
/// [`TerrainConfig::band_covering`]), so an R16 save clamps nothing. Returns
/// the old and the new band, or `None` when the band already held the data
/// and nothing changed.
pub fn widen_height_band_to_fit(config: &mut TerrainConfig, data: &mut TerrainData) -> Option<(HeightBand, HeightBand)> {
    let to = config.band_covering(data)?;
    let from = rebase_height_band(config, data, to);
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_height_and_normalized_height_are_inverses() {
        let config = TerrainConfig {
            height_offset: -32.0,
            height_scale: 128.0,
            ..TerrainConfig::default()
        };
        assert_eq!(config.world_height(0.0), -32.0, "normalized 0 is the band floor");
        assert_eq!(config.world_height(1.0), 96.0, "normalized 1 is the band ceiling");
        assert_eq!(config.normalized_height(0.0), 0.25);
        for world_y in [-32.0f32, -10.5, 0.0, 17.25, 96.0] {
            let back = config.world_height(config.normalized_height(world_y));
            assert!((back - world_y).abs() < 1e-4, "{world_y} round-tripped to {back}");
        }
    }

    #[test]
    fn footprint_spans_the_whole_chunk_grid() {
        let config = TerrainConfig {
            chunk_size: 64.0,
            chunks_x: 3,
            chunks_z: 2,
            ..TerrainConfig::default()
        };
        let (min, max) = config.footprint_xz();
        assert_eq!(min, Vec2::new(-192.0, -128.0));
        assert_eq!(max, Vec2::new(256.0, 192.0));
        let (width, depth) = config.total_size();
        assert_eq!(max - min, Vec2::new(width, depth));
    }

    /// 5 x 3 chunks of 10 m around chunk (5, -3): chunks 3..=7 in X and
    /// -4..=-2 in Z, nowhere near the origin.
    fn off_centre_config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 10.0,
            chunk_resolution: 4,
            chunks_x: 2,
            chunks_z: 1,
            center_chunk: IVec2::new(5, -3),
            ..TerrainConfig::default()
        }
    }

    #[test]
    fn an_off_centre_grid_spans_its_centre_chunk_and_half_extents() {
        let config = off_centre_config();
        assert_eq!(config.chunk_min(), IVec2::new(3, -4));
        assert_eq!(config.chunk_max(), IVec2::new(7, -2));

        for chunk in [IVec2::new(3, -4), IVec2::new(7, -2), IVec2::new(3, -2), IVec2::new(7, -4), IVec2::new(5, -3)] {
            assert!(config.contains_chunk(chunk), "{chunk} is on the grid");
        }
        // One chunk past each edge, and the origin.
        for chunk in [IVec2::new(2, -3), IVec2::new(8, -3), IVec2::new(5, -5), IVec2::new(5, -1), IVec2::ZERO] {
            assert!(!config.contains_chunk(chunk), "{chunk} is off the grid");
            assert_eq!(config.chunk_grid_index(chunk), None, "{chunk} has no index");
        }
        assert_eq!(config.chunk_grid_index(IVec2::new(3, -4)), Some(UVec2::ZERO));
        assert_eq!(config.chunk_grid_index(IVec2::new(5, -3)), Some(UVec2::new(2, 1)));
        assert_eq!(config.chunk_grid_index(IVec2::new(7, -2)), Some(UVec2::new(4, 2)));

        let chunks: Vec<IVec2> = config.grid_chunks().collect();
        assert_eq!(chunks.len(), config.total_chunks() as usize);
        assert_eq!(chunks.first(), Some(&IVec2::new(3, -4)));
        assert_eq!(chunks.last(), Some(&IVec2::new(7, -2)));
        // Row by row, X running fastest, each chunk at the index its place says.
        for (i, chunk) in chunks.iter().enumerate() {
            let expected = UVec2::new(i as u32 % 5, i as u32 / 5);
            assert_eq!(config.chunk_grid_index(*chunk), Some(expected), "chunk {i} is {chunk}");
        }

        let (min, max) = config.footprint_xz();
        assert_eq!(min, Vec2::new(30.0, -40.0));
        assert_eq!(max, Vec2::new(80.0, -10.0));
        let (width, depth) = config.total_size();
        assert_eq!(max - min, Vec2::new(width, depth));

        // Moving the grid is a new grid for everything built on it.
        let moved = TerrainConfig { center_chunk: IVec2::new(6, -3), ..config.clone() };
        assert_ne!(TerrainGridKey::of(&moved), TerrainGridKey::of(&config));
    }

    #[test]
    fn an_off_centre_grid_maps_its_footprint_onto_the_whole_raster() {
        let config = off_centre_config();
        let (min, max) = config.footprint_xz();
        assert_eq!(config.world_to_uv(min.x, min.y), Vec2::ZERO);
        assert_eq!(config.world_to_uv(max.x, max.y), Vec2::ONE);
        assert_eq!(config.chunk_point_uv(config.chunk_min(), 0.0, 0.0), Vec2::ZERO);
        assert_eq!(config.chunk_point_uv(config.chunk_max(), 1.0, 1.0), Vec2::ONE);

        // A chunk corner reached from the world and from the chunk: the same
        // UV, three fifths across and a third down.
        let chunk = IVec2::new(6, -3);
        let corner = chunk.as_vec2() * config.chunk_size;
        assert_eq!(config.world_to_uv(corner.x, corner.y), config.chunk_point_uv(chunk, 0.0, 0.0));
        assert_eq!(config.chunk_point_uv(chunk, 0.0, 0.0), Vec2::new(3.0 / 5.0, 1.0 / 3.0));
        // The far corner of one chunk is the near corner of the next.
        assert_eq!(config.chunk_point_uv(chunk, 1.0, 1.0), config.chunk_point_uv(chunk + IVec2::ONE, 0.0, 0.0));

        // Unclamped outside the footprint.
        let outside = config.world_to_uv(min.x - 5.0, max.y + 15.0);
        assert!(outside.x < 0.0 && outside.y > 1.0, "{outside}");
    }

    #[test]
    fn a_grid_centred_on_the_origin_keeps_its_formulas_bit_for_bit() {
        let config = TerrainConfig { chunk_size: 48.0, chunks_x: 3, chunks_z: 2, ..TerrainConfig::default() };
        assert_eq!(config.center_chunk, IVec2::ZERO);
        let (hx, hz) = (config.chunks_x as i32, config.chunks_z as i32);
        assert_eq!((config.chunk_min(), config.chunk_max()), (IVec2::new(-hx, -hz), IVec2::new(hx, hz)));
        let expected: Vec<IVec2> = (-hz..=hz).flat_map(|z| (-hx..=hx).map(move |x| IVec2::new(x, z))).collect();
        assert_eq!(config.grid_chunks().collect::<Vec<_>>(), expected);

        let total_x = (config.chunks_x * 2 + 1) as f32;
        let total_z = (config.chunks_z * 2 + 1) as f32;
        let bits = |uv: Vec2| (uv.x.to_bits(), uv.y.to_bits());
        for chunk in [IVec2::new(-3, -2), IVec2::ZERO, IVec2::new(2, -1), IVec2::new(3, 2), IVec2::new(-1, 1), IVec2::new(4, 0)] {
            let inside = chunk.x.abs() <= hx && chunk.y.abs() <= hz;
            assert_eq!(config.contains_chunk(chunk), inside, "{chunk}");
            let index = inside.then(|| UVec2::new((chunk.x + hx) as u32, (chunk.y + hz) as u32));
            assert_eq!(config.chunk_grid_index(chunk), index, "{chunk}");
            for (u, v) in [(0.0f32, 0.0f32), (0.25, 0.75), (1.0 / 3.0, 0.1), (0.9, 1.0 / 7.0), (1.0, 1.0)] {
                let old = Vec2::new(
                    (chunk.x as f32 + u + config.chunks_x as f32) / total_x,
                    (chunk.y as f32 + v + config.chunks_z as f32) / total_z,
                );
                assert_eq!(bits(config.chunk_point_uv(chunk, u, v)), bits(old), "chunk {chunk} at ({u}, {v})");
            }
        }
        for (x, z) in [(0.0f32, 0.0f32), (-144.0, -96.0), (13.7, -55.25), (191.9, 143.9), (-0.3, 7.77), (-1e5, 1e5)] {
            let old = Vec2::new(
                (x / config.chunk_size.max(1e-3) + config.chunks_x as f32) / total_x,
                (z / config.chunk_size.max(1e-3) + config.chunks_z as f32) / total_z,
            );
            assert_eq!(bits(config.world_to_uv(x, z)), bits(old), "world ({x}, {z})");
        }
        let (min, max) = config.footprint_xz();
        let old_min = Vec2::new(-(config.chunks_x as f32) * config.chunk_size, -(config.chunks_z as f32) * config.chunk_size);
        let old_max = Vec2::new((config.chunks_x as f32 + 1.0) * config.chunk_size, (config.chunks_z as f32 + 1.0) * config.chunk_size);
        assert_eq!((bits(min), bits(max)), (bits(old_min), bits(old_max)));
    }

    #[test]
    fn normalized_height_guards_a_zero_scale() {
        let config = TerrainConfig {
            height_offset: 5.0,
            height_scale: 0.0,
            ..TerrainConfig::default()
        };
        assert_eq!(config.normalized_height(42.0), 0.0);
        assert_eq!(config.world_height(config.normalized_height(42.0)), 5.0);
    }

    /// 3 x 3 chunks of 4 x 4 cells: a 12 x 12 raster.
    fn band_config(height_offset: f32, height_scale: f32) -> TerrainConfig {
        TerrainConfig {
            chunk_resolution: 4,
            chunks_x: 1,
            chunks_z: 1,
            height_offset,
            height_scale,
            ..TerrainConfig::default()
        }
    }

    #[test]
    fn widening_the_band_keeps_every_world_height() {
        let mut config = band_config(-10.0, 50.0);
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        // Normalized -0.6 to 1.4: world -40 to 60 against a -10 to 40 band,
        // escaping it on both sides.
        let last = (data.height_cache.len() - 1) as f32;
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = -0.6 + 2.0 * i as f32 / last;
        }
        let worlds: Vec<f32> = data.height_cache.iter().map(|&n| config.world_height(n)).collect();
        let copy = data.height_cache.clone();

        let (from, to) = widen_height_band_to_fit(&mut config, &mut data).expect("heights outside the band widen it");
        assert_eq!(from, HeightBand { offset: -10.0, scale: 50.0 });
        assert_eq!(HeightBand::of(&config), to);
        assert!(to.offset < -40.0 && to.world(1.0) > 60.0, "{to:?} leaves no headroom past the data");
        for (n, w) in data.height_cache.iter().zip(&worlds) {
            assert!((0.0..=1.0).contains(n), "sample {n} is still outside the band");
            let back = config.world_height(*n);
            assert!((back - w).abs() < 1e-4, "world height {w} moved to {back}");
        }

        // A copy rebased from the old band to the new one lands on exactly
        // the values the raster did, which is what keeps undo entries true.
        let mut rebased = copy;
        from.rebase_into(to, &mut rebased);
        assert_eq!(rebased, data.height_cache);

        assert!(widen_height_band_to_fit(&mut config, &mut data).is_none(), "the widened band holds the data");
    }

    #[test]
    fn widening_only_grows_the_side_the_data_escaped() {
        let mut config = band_config(0.0, 100.0);
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        data.height_cache.iter_mut().for_each(|h| *h = 0.5);
        data.height_cache[7] = 1.2; // world 120

        let (_, to) = widen_height_band_to_fit(&mut config, &mut data).unwrap();
        assert_eq!(to.offset, 0.0, "the floor held the data, so it stays");
        assert!(to.world(1.0) > 120.0);
        assert!((config.world_height(data.height_cache[7]) - 120.0).abs() < 1e-4);
        assert!((config.world_height(data.height_cache[0]) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn a_raster_inside_its_band_keeps_the_band() {
        let mut config = band_config(0.0, 50.0);
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        data.height_cache.iter_mut().enumerate().for_each(|(i, h)| *h = (i % 10) as f32 / 9.0);
        // A hair past the ceiling still encodes to the top R16 code.
        data.height_cache[0] = 1.0 + 0.25 / 65535.0;
        let before = data.height_cache.clone();

        assert!(widen_height_band_to_fit(&mut config, &mut data).is_none());
        assert_eq!(data.height_cache, before);
        assert_eq!(HeightBand::of(&config), HeightBand { offset: 0.0, scale: 50.0 });
    }

    #[test]
    fn raw_world_heights_get_a_band_on_their_first_save() {
        // The voxel loader stores world heights with a unit scale and zero
        // offset, so almost every height is outside that 0..1 band.
        let mut config = band_config(0.0, 1.0);
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let heights = [-252.0f32, 132.0, 16.0, 0.0, 4.0];
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = heights[i % heights.len()];
        }

        widen_height_band_to_fit(&mut config, &mut data).expect("raw heights need a band");
        assert!(config.height_scale > 1.0);
        for (i, n) in data.height_cache.iter().enumerate() {
            assert!((0.0..=1.0).contains(n));
            let want = heights[i % heights.len()];
            assert!((config.world_height(*n) - want).abs() < 1e-3, "{want} moved to {}", config.world_height(*n));
        }
    }

    const NO_MATERIAL: MaterialCell = [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0];

    #[test]
    fn a_hole_is_a_cell_without_material_on_a_sparse_surface_only() {
        use crate::terrain::material::{material_cell, TerrainMaterial};

        let config = band_config(0.0, 50.0);
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        let total = data.height_cache.len();
        data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); total];
        data.material_cache[5] = NO_MATERIAL;
        // Only `id_a` decides: a single material leaves `id_b` empty.
        data.material_cache[6] = material_cell(TerrainMaterial::Rock.to_u8());
        assert!(!data.cell_is_hole(5), "a full surface has no holes");

        data.sparse_surface = true;
        assert!(data.cell_is_hole(5));
        assert!(!data.cell_is_hole(4) && !data.cell_is_hole(6));
        assert!(!data.cell_is_hole(total), "past the material layer");
        data.material_cache.clear();
        assert!(!data.cell_is_hole(5), "no material layer, no holes");
    }

    #[test]
    fn a_chunk_has_ground_where_a_cell_of_its_tile_is_not_a_hole() {
        use crate::terrain::material::{material_cell, TerrainMaterial};

        // 5 x 3 chunks of 4 x 4 cells around chunk (5, -3): a 20 x 12 raster.
        let config = off_centre_config();
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        assert_eq!((data.cache_width, data.cache_height), (20, 12));
        data.material_cache = vec![NO_MATERIAL; 240];
        for chunk in config.grid_chunks().chain([IVec2::ZERO]) {
            assert!(data.chunk_has_ground(&config, chunk), "a full surface has ground everywhere, {chunk} too");
        }

        data.sparse_surface = true;
        for chunk in config.grid_chunks() {
            assert!(!data.chunk_has_ground(&config, chunk), "{chunk} is all holes");
        }
        // One cell of ground at column 13, row 5: tile (3, 1), chunk (6, -3).
        data.material_cache[5 * 20 + 13] = material_cell(TerrainMaterial::Grass.to_u8());
        for chunk in config.grid_chunks() {
            assert_eq!(data.chunk_has_ground(&config, chunk), chunk == IVec2::new(6, -3), "{chunk}");
        }
        assert!(!data.chunk_has_ground(&config, IVec2::ZERO), "off the grid there is no ground");

        // The one mesh vertex of chunk (6, -3) on that cell is not a hole, its
        // neighbour is.
        let vertex = |u: f32, v: f32| data.point_is_hole(&config, IVec2::new(6, -3), u, v);
        let on_ground = (0..=4).flat_map(|z| (0..=4).map(move |x| (x, z))).filter(|&(x, z)| !vertex(x as f32 / 4.0, z as f32 / 4.0));
        assert_eq!(on_ground.count(), 1);
    }
}
