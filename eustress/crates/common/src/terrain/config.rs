//! Terrain configuration and data structures

use bevy::prelude::*;

use super::material::MaterialCell;
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
    /// runs `-chunks_x..=chunks_x`, so the extent is one chunk wider on the
    /// positive side than on the negative side of the origin.
    pub fn footprint_xz(&self) -> (Vec2, Vec2) {
        let min = Vec2::new(
            -(self.chunks_x as f32) * self.chunk_size,
            -(self.chunks_z as f32) * self.chunk_size,
        );
        let max = Vec2::new(
            (self.chunks_x as f32 + 1.0) * self.chunk_size,
            (self.chunks_z as f32 + 1.0) * self.chunk_size,
        );
        (min, max)
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
/// are built on: a change means none of them fits. The height band is left
/// out, since Save re-expresses heights in a new band without moving them.
/// `chunk_size` is compared by its bits so a NaN size still equals itself and
/// does not rebuild everything every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainGridKey {
    chunk_size_bits: u32,
    chunk_resolution: u32,
    chunks_x: u32,
    chunks_z: u32,
}

impl TerrainGridKey {
    /// The grid `config` lays its chunks on.
    pub fn of(config: &TerrainConfig) -> Self {
        Self {
            chunk_size_bits: config.chunk_size.to_bits(),
            chunk_resolution: config.chunk_resolution,
            chunks_x: config.chunks_x,
            chunks_z: config.chunks_z,
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
}
