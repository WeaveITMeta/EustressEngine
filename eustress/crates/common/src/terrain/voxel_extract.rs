//! # Voxel-chunk decode + multi-span column extractor (Wave 9.C)
//!
//! Spec: `docs/architecture/TERRAIN_FJALL_MIGRATION.md` §9.C + the SCOPE
//! DECISION section (multi-span, 23 materials).
//!
//! ## What this module is (and is NOT)
//!
//! This is the **pure, engine-free** core of the imported-terrain render
//! path: given the LZ4 bytes of one Roblox voxel chunk (the format the
//! importer's `roblox-import/src/terrain.rs` writes), it
//!
//! 1. decodes the chunk to per-cell `(eustress_material_id, occupancy)`
//!    in the 32³ grid, and
//! 2. for each `(x, z)` column, walks the Y stack and emits the **solid
//!    spans** (contiguous runs of occupied cells), then
//! 3. writes the TOP surface of the highest span into a `TerrainData`
//!    `height_cache` (raw studs) and the surface material's
//!    [`TerrainMaterial::splat_bucket`] into the 4-wide `splat_cache`.
//!
//! It deliberately does NOT read Fjall (that is the engine's job —
//! `eustress-common` must not depend on `eustress-worlddb`; cycle risk).
//! The engine-side loader (`engine/src/terrain_voxel_load.rs`) holds the
//! `WorldDb` handle, region-queries chunks, and calls
//! [`fill_terrain_from_chunk`] here per chunk.
//!
//! ## What renders vs. what is deferred
//!
//! - **Renders now:** the TOP surface of every column (heightfield) with
//!   the correct per-cell material colour bucket. This is exactly what the
//!   existing `generate_chunk_mesh` / `chunk_spawn_system` path consumes —
//!   unchanged. Scales via the engine's region query (camera-local
//!   chunks only).
//! - **Deferred (documented):** true multi-span CAVE GEOMETRY. The
//!   extractor already FINDS every span per column (floor + overhang/
//!   tunnel-roof), and [`column_spans`] exposes them, but the current
//!   renderer is a single-surface heightfield, so only the top span's top
//!   surface meshes. Under-surfaces (a cave roof, the underside of an
//!   overhang) need the surface-nets / dual-contouring volumetric mesher —
//!   a later wave. The voxels are stored WHOLE in Fjall (lossless), so that
//!   mesher can read the same store with nothing dropped on import.
//!
//! ## Chunk byte format (mirrors `roblox-import/src/terrain.rs`)
//!
//! The importer's `encode_eustress_chunk` (spec §6.6) writes, BEFORE LZ4:
//! ```text
//! u8  version (== EUSTRESS_CHUNK_VERSION = 1)
//! u8  material_count (informational)
//! u8  flags (bit0 = contains water marker)
//! [ for each of 32^3 cells, Y-outer/X-middle/Z-inner order: ]
//!     u8 eustress_material_id  (255 = Air, 254 = Water)
//!     u8 occupancy_q           (0..=255)
//! ```
//! Linear cell index is `y*1024 + x*32 + z` (Y outer, X middle, Z inner).
//! On disk / in Fjall the whole record is `lz4_flex::compress_prepend_size`,
//! so we `decompress_size_prepended` first. (We re-declare the small decode
//! here rather than depend on the bevy-free importer crate just for it.)

use bevy::math::IVec2;

use super::material::TerrainMaterial;
use super::{TerrainConfig, TerrainData};

// ---------------------------------------------------------------------------
// Constants — kept in lockstep with roblox-import/src/terrain.rs and
// worlddb/src/keys.rs (VOXEL_CHUNK_EDGE_STUDS). A mismatch would land
// columns at the wrong world position, so these are asserted in a test.
// ---------------------------------------------------------------------------

/// Cells along one edge of a voxel chunk (Roblox SmoothGrid uses 32).
pub const CHUNK_EDGE: usize = 32;

/// Total cells in one 32³ chunk.
pub const CELLS_PER_CHUNK: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE; // 32768

/// Roblox terrain cell edge in studs (= meters in Eustress).
pub const ROBLOX_CELL_STUDS: f32 = 4.0;

/// One voxel chunk's world edge in studs (`CHUNK_EDGE * ROBLOX_CELL_STUDS`).
/// MUST equal `eustress_worlddb::keys::VOXEL_CHUNK_EDGE_STUDS` (128.0) so a
/// region query's chunk coords place columns at the right world position.
pub const VOXEL_CHUNK_EDGE_STUDS: f32 = CHUNK_EDGE as f32 * ROBLOX_CELL_STUDS; // 128.0

/// The importer's per-chunk header length, in bytes (version, material
/// count, flags). Cells start at this offset.
pub const CHUNK_HEADER_LEN: usize = 3;

/// The Eustress voxel-chunk format version we decode (spec §6.6).
pub const EUSTRESS_CHUNK_VERSION: u8 = 1;

/// Importer sentinel: a cell lifted into the separate water layer. NOT a
/// terrain-fill material (so it does not decode to a [`TerrainMaterial`]).
pub const WATER_MARKER: u8 = 254;

/// Importer sentinel: air / empty cell.
pub const AIR_MARKER: u8 = 255;

/// Occupancy strictly above this counts a cell as SOLID for span building.
/// Roblox occupancy decodes as `(q + 1) / 256`; the half-full boundary is
/// `q == 127` (≈0.5), so a cell is solid when `q > 127`. (Air is `q == 0`.)
pub const SOLID_OCCUPANCY_THRESHOLD: u8 = 127;

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

/// A decoded 32³ voxel chunk: parallel per-cell material + occupancy arrays
/// in linear `y*1024 + x*32 + z` order. `material[i]` is an EUSTRESS
/// material id (or [`AIR_MARKER`] / [`WATER_MARKER`]).
#[derive(Debug, Clone)]
pub struct DecodedChunk {
    /// Per-cell Eustress material id (255 = Air, 254 = Water), YXZ order.
    pub material: Vec<u8>,
    /// Per-cell quantised occupancy (0..=255), YXZ order.
    pub occupancy: Vec<u8>,
}

impl DecodedChunk {
    /// Linear index for local cell `(x, y, z)` (Y outer, X middle, Z inner).
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        y * (CHUNK_EDGE * CHUNK_EDGE) + x * CHUNK_EDGE + z
    }

    /// Material id at local cell `(x, y, z)` (or [`AIR_MARKER`] if OOB).
    #[inline]
    pub fn material_at(&self, x: usize, y: usize, z: usize) -> u8 {
        self.material
            .get(Self::index(x, y, z))
            .copied()
            .unwrap_or(AIR_MARKER)
    }

    /// Occupancy at local cell `(x, y, z)` (or 0 if OOB).
    #[inline]
    pub fn occupancy_at(&self, x: usize, y: usize, z: usize) -> u8 {
        self.occupancy.get(Self::index(x, y, z)).copied().unwrap_or(0)
    }
}

/// Why a chunk failed to decode (kept as a string for log routing — the
/// engine loader logs and skips the chunk, never panics on bad bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDecodeError(pub String);

impl std::fmt::Display for ChunkDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Decode the LZ4-compressed bytes of ONE voxel chunk (the value stored in
/// the Fjall `voxels` partition) into a [`DecodedChunk`].
///
/// This mirrors the importer's `encode_eustress_chunk` inverse: LZ4
/// size-prepended decompress, validate the 3-byte header, then read
/// `CELLS_PER_CHUNK` `(material, occupancy)` byte pairs. Never panics; any
/// malformed input is an `Err(ChunkDecodeError)` so the caller can skip the
/// chunk and keep loading the rest.
pub fn decode_voxel_chunk(compressed: &[u8]) -> Result<DecodedChunk, ChunkDecodeError> {
    let raw = lz4_flex::decompress_size_prepended(compressed)
        .map_err(|e| ChunkDecodeError(format!("lz4 decompress failed: {e}")))?;
    decode_voxel_chunk_raw(&raw)
}

/// Decode an ALREADY-DECOMPRESSED chunk record (header + cells). Split out
/// so tests can build a tiny record without the LZ4 layer.
pub fn decode_voxel_chunk_raw(raw: &[u8]) -> Result<DecodedChunk, ChunkDecodeError> {
    let expected = CHUNK_HEADER_LEN + CELLS_PER_CHUNK * 2;
    if raw.len() != expected {
        return Err(ChunkDecodeError(format!(
            "chunk record wrong length: {} (expected {expected} = {CHUNK_HEADER_LEN} header + {CELLS_PER_CHUNK}*2 cells)",
            raw.len()
        )));
    }
    let version = raw[0];
    if version != EUSTRESS_CHUNK_VERSION {
        return Err(ChunkDecodeError(format!(
            "unsupported chunk version {version} (expected {EUSTRESS_CHUNK_VERSION})"
        )));
    }
    // raw[1] = material_count (informational), raw[2] = flags — both unused
    // here (the per-cell material id is authoritative).
    let mut material = Vec::with_capacity(CELLS_PER_CHUNK);
    let mut occupancy = Vec::with_capacity(CELLS_PER_CHUNK);
    let cells = &raw[CHUNK_HEADER_LEN..];
    for pair in cells.chunks_exact(2) {
        material.push(pair[0]);
        occupancy.push(pair[1]);
    }
    Ok(DecodedChunk { material, occupancy })
}

// ---------------------------------------------------------------------------
// Multi-span column extractor (the core of 9.C)
// ---------------------------------------------------------------------------

/// One contiguous run of SOLID cells in a single `(x, z)` column, in LOCAL
/// chunk cell coordinates (`0..CHUNK_EDGE`). `bottom_y..=top_y` inclusive.
/// `top_material` is the Eustress material id of the cell at `top_y` — the
/// surface the heightfield renderer would shade for this span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Lowest solid cell Y in the run (inclusive).
    pub bottom_y: usize,
    /// Highest solid cell Y in the run (inclusive) — the span's TOP surface.
    pub top_y: usize,
    /// Eustress material id at the top cell of the span.
    pub top_material: u8,
}

impl Span {
    /// Number of solid cells in this span.
    #[inline]
    pub fn thickness(&self) -> usize {
        self.top_y - self.bottom_y + 1
    }
}

/// A cell is SOLID when it is neither air nor the water-layer sentinel AND
/// its occupancy clears the half-full threshold. (Water cells that survived
/// into terrain — `TerrainMaterial::Water`, id 22 — ARE solid; only the
/// importer's lifted-water sentinel 254 is excluded.)
#[inline]
pub fn cell_is_solid(material: u8, occupancy: u8) -> bool {
    material != AIR_MARKER
        && material != WATER_MARKER
        && occupancy > SOLID_OCCUPANCY_THRESHOLD
}

/// Walk one `(x, z)` column's Y stack (bottom→top) and emit its solid spans.
///
/// A column may yield MULTIPLE spans: e.g. a solid floor with a separate
/// floating slab above a gap → two spans. An all-solid column → one span
/// spanning the whole height. An all-air column → no spans. The returned
/// vec is ordered bottom→top, so `.last()` is the highest span (whose
/// `top_y` is the heightfield surface for this column).
///
/// This is the multi-span detection the SCOPE DECISION calls for; the
/// renderer currently meshes only the top span's surface (see module docs),
/// but the full set is available for the future volumetric mesher.
pub fn column_spans(chunk: &DecodedChunk, x: usize, z: usize) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut run_bottom: Option<usize> = None;
    for y in 0..CHUNK_EDGE {
        let solid = cell_is_solid(chunk.material_at(x, y, z), chunk.occupancy_at(x, y, z));
        match (solid, run_bottom) {
            (true, None) => run_bottom = Some(y), // run opens
            (false, Some(bottom)) => {
                // run closes at y-1
                let top_y = y - 1;
                spans.push(Span {
                    bottom_y: bottom,
                    top_y,
                    top_material: chunk.material_at(x, top_y, z),
                });
                run_bottom = None;
            }
            _ => {}
        }
    }
    // A run that reaches the top of the chunk closes at CHUNK_EDGE-1.
    if let Some(bottom) = run_bottom {
        let top_y = CHUNK_EDGE - 1;
        spans.push(Span {
            bottom_y: bottom,
            top_y,
            top_material: chunk.material_at(x, top_y, z),
        });
    }
    spans
}

/// The TOP surface of a column: the highest span's `top_y` + its material,
/// or `None` for an all-air column. This is what the heightfield renderer
/// shades. (Thin wrapper over [`column_spans`] for the common case so the
/// height fill doesn't allocate a Vec per column when only the top is
/// needed.)
pub fn column_top_surface(chunk: &DecodedChunk, x: usize, z: usize) -> Option<(usize, u8)> {
    // Walk top→bottom and return the first solid cell — equivalent to the
    // top of the highest span but without building the whole span list.
    for y in (0..CHUNK_EDGE).rev() {
        let m = chunk.material_at(x, y, z);
        let o = chunk.occupancy_at(x, y, z);
        if cell_is_solid(m, o) {
            return Some((y, m));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// TerrainData cache fill
// ---------------------------------------------------------------------------

/// Width of the splat cache per pixel: `[grass, rock, dirt, snow]`. Matches
/// `TerrainData.splat_cache`'s documented 4-channel layout and
/// [`super::material::SPLAT_LAYER_COUNT`].
pub const SPLAT_CHANNELS: usize = super::material::SPLAT_LAYER_COUNT;

/// A `TerrainConfig` sized so one voxel chunk maps to one terrain chunk and
/// stored heights are raw WORLD studs (so `height_scale = 1.0`).
///
/// `radius_chunks` is the half-extent (in chunks) the cache should cover
/// around the origin — derived by the engine loader from the voxel region
/// bounds. `chunk_resolution = CHUNK_EDGE (32)` so there is one height
/// sample per voxel cell column; `chunk_size = VOXEL_CHUNK_EDGE_STUDS (128)`
/// so a chunk's world footprint equals a voxel chunk's.
pub fn voxel_terrain_config(radius_chunks: u32) -> TerrainConfig {
    TerrainConfig {
        chunk_size: VOXEL_CHUNK_EDGE_STUDS,
        chunk_resolution: CHUNK_EDGE as u32,
        chunks_x: radius_chunks,
        chunks_z: radius_chunks,
        lod_levels: 4,
        lod_distances: vec![256.0, 512.0, 1024.0, 2048.0],
        view_distance: 4096.0,
        // Heights stored as raw studs → renderer does `sample_height * 1.0`.
        height_scale: 1.0,
        seed: 0,
    }
}

/// Fill ONE voxel chunk's TOP-surface heights + materials into a
/// `TerrainData`, at the terrain-chunk grid position derived from the voxel
/// chunk coords. Returns the `(chunk_x, chunk_z)` grid cell written (so the
/// caller can track which terrain chunks were touched).
///
/// - `cx, cy, cz` are the voxel chunk's SIGNED coordinates (from the region
///   query / Morton key). The terrain grid is 2.5D, so the terrain chunk is
///   `(cx, cz)`; `cy` only contributes to the absolute world Y of the
///   surface (a chunk stacked higher in Y raises its columns' studs).
/// - For each `(x, z)` column, the highest solid cell's WORLD-Y top (in
///   studs) goes into `height_cache`, and its material's
///   [`TerrainMaterial::splat_bucket`] gets weight 1.0 in `splat_cache`
///   (all other channels 0). Air columns are left at 0 (the cache's init
///   value) and contribute no splat weight.
///
/// The height is stored in RAW STUDS and the config uses `height_scale =
/// 1.0`, so `generate_chunk_mesh`'s `sample_height(..) * height_scale`
/// yields the correct world Y with no normalization round-trip.
///
/// Reuses [`super::toml_loader::write_chunk_to_cache`] for the height write
/// (the exact `.r16`-path offset math) so the voxel surface lands at the
/// same cache location the renderer reads back.
pub fn fill_terrain_from_chunk(
    data: &mut TerrainData,
    config: &TerrainConfig,
    cx: i32,
    cy: i32,
    cz: i32,
    chunk: &DecodedChunk,
) -> IVec2 {
    // Ensure caches are sized before any write.
    if data.height_cache.is_empty() {
        data.resize_cache(config);
    }
    ensure_splat_sized(data);

    let resolution = config.chunk_resolution as usize; // == CHUNK_EDGE
    // World-Y base of this voxel chunk's cell y==0, in studs.
    let chunk_base_y = cy as f32 * VOXEL_CHUNK_EDGE_STUDS;

    // Build this chunk's `resolution*resolution` height tile (raw studs) and
    // capture the surface material per column for the splat write.
    let mut heights = vec![0.0f32; resolution * resolution];
    // (x, z) → splat bucket of the top surface (None = air column).
    let mut surface_bucket: Vec<Option<usize>> = vec![None; resolution * resolution];

    for z in 0..resolution {
        for x in 0..resolution {
            // The renderer indexes its per-chunk tile as row=z, col=x
            // (`src_start = row*resolution`, then x within the row) — match it.
            let tile_idx = z * resolution + x;
            if let Some((top_y, mat_id)) = column_top_surface(chunk, x, z) {
                // World-Y of the TOP of the top cell: base + (cell index +1)
                // cells worth of studs (a cell at y occupies [y, y+1) cells →
                // its top face is (top_y + 1) cells up). Matches the importer's
                // 4-stud cell so the surface sits on the cell's top face.
                let world_top = chunk_base_y + (top_y as f32 + 1.0) * ROBLOX_CELL_STUDS;
                heights[tile_idx] = world_top;
                surface_bucket[tile_idx] =
                    Some(TerrainMaterial::from_u8_or_default(mat_id).splat_bucket());
            }
        }
    }

    let chunk_pos = IVec2::new(cx, cz);
    super::toml_loader::write_chunk_to_cache(data, config, chunk_pos, &heights);
    write_splat_to_cache(data, config, chunk_pos, &surface_bucket);
    data.splat_dirty = true;
    chunk_pos
}

/// Size `splat_cache` to `cache_width * cache_height * SPLAT_CHANNELS` if it
/// isn't already (mirrors `TerrainData::resize_cache` for the splat buffer,
/// which that method does not currently touch).
fn ensure_splat_sized(data: &mut TerrainData) {
    let needed = data.cache_width as usize * data.cache_height as usize * SPLAT_CHANNELS;
    if data.splat_cache.len() != needed {
        data.splat_cache.clear();
        data.splat_cache.resize(needed, 0.0);
    }
}

/// Write one chunk's per-column surface-material splat buckets into the
/// global `splat_cache`, using the SAME chunk→cache offset math as
/// [`super::toml_loader::write_chunk_to_cache`] (so heights and splat align
/// pixel-for-pixel). Each column with a surface gets weight 1.0 in its
/// bucket channel; air columns are skipped (left at 0).
fn write_splat_to_cache(
    data: &mut TerrainData,
    config: &TerrainConfig,
    chunk_pos: IVec2,
    surface_bucket: &[Option<usize>],
) {
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let half_x = config.chunks_x as i32;
    let half_z = config.chunks_z as i32;
    let offset_x = ((chunk_pos.x + half_x) as usize) * resolution;
    let offset_z = ((chunk_pos.y + half_z) as usize) * resolution;

    for row in 0..resolution {
        for col in 0..resolution {
            let src_idx = row * resolution + col;
            let Some(bucket) = surface_bucket.get(src_idx).copied().flatten() else {
                continue;
            };
            let px_x = offset_x + col;
            let px_z = offset_z + row;
            let base = (px_z * cache_width + px_x) * SPLAT_CHANNELS;
            if bucket < SPLAT_CHANNELS && base + SPLAT_CHANNELS <= data.splat_cache.len() {
                // One-hot the surface bucket (clear the other channels first
                // in case this pixel was written by an adjacent chunk's edge).
                for c in 0..SPLAT_CHANNELS {
                    data.splat_cache[base + c] = 0.0;
                }
                data.splat_cache[base + bucket] = 1.0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — runnable via `cargo test -p eustress-common terrain`
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Eustress material ids used in fixtures (mirror roblox-import's
    /// `eustress_material` module — Grass=0, Rock=1).
    const GRASS: u8 = 0;
    const ROCK: u8 = 1;

    /// Build a fully-air `DecodedChunk` (every cell air, occupancy 0).
    fn air_chunk() -> DecodedChunk {
        DecodedChunk {
            material: vec![AIR_MARKER; CELLS_PER_CHUNK],
            occupancy: vec![0; CELLS_PER_CHUNK],
        }
    }

    /// Set one cell solid (`occupancy = 255`) with the given material.
    fn set_solid(chunk: &mut DecodedChunk, x: usize, y: usize, z: usize, material: u8) {
        let i = DecodedChunk::index(x, y, z);
        chunk.material[i] = material;
        chunk.occupancy[i] = 255;
    }

    #[test]
    fn constants_agree_with_worlddb_and_importer() {
        // CHUNK_EDGE * ROBLOX_CELL_STUDS must equal the worlddb chunk edge
        // (128) or region queries place columns at the wrong world position.
        assert_eq!(VOXEL_CHUNK_EDGE_STUDS, 128.0);
        assert_eq!(CELLS_PER_CHUNK, 32_768);
        assert_eq!(CHUNK_HEADER_LEN, 3);
        // The splat cache width matches the material module's layer count.
        assert_eq!(SPLAT_CHANNELS, 4);
    }

    #[test]
    fn single_solid_column_has_one_span_top_at_its_top() {
        // A single solid column: cells y=0..=4 solid Grass, rest air.
        let mut chunk = air_chunk();
        for y in 0..=4 {
            set_solid(&mut chunk, 0, y, 0, GRASS);
        }
        let spans = column_spans(&chunk, 0, 0);
        assert_eq!(spans.len(), 1, "one contiguous run → one span");
        assert_eq!(spans[0].bottom_y, 0);
        assert_eq!(spans[0].top_y, 4, "top surface is the highest solid cell");
        assert_eq!(spans[0].top_material, GRASS);
        assert_eq!(spans[0].thickness(), 5);
        // The top-surface helper agrees with the highest span.
        assert_eq!(column_top_surface(&chunk, 0, 0), Some((4, GRASS)));
    }

    #[test]
    fn flat_floor_plus_floating_slab_yields_two_spans_top_is_upper_slab() {
        // THE multi-span case (spec §9.C): a flat floor (y=0..=2) and a
        // SEPARATE floating slab (y=8..=10) with an air gap between → 2 spans;
        // the top surface = the top of the UPPER slab.
        let mut chunk = air_chunk();
        for y in 0..=2 {
            set_solid(&mut chunk, 5, y, 7, ROCK); // floor (rock)
        }
        for y in 8..=10 {
            set_solid(&mut chunk, 5, y, 7, GRASS); // floating slab (grass)
        }
        let spans = column_spans(&chunk, 5, 7);
        assert_eq!(spans.len(), 2, "floor + floating slab → two spans");
        // Ordered bottom→top.
        assert_eq!(spans[0].bottom_y, 0);
        assert_eq!(spans[0].top_y, 2);
        assert_eq!(spans[0].top_material, ROCK);
        assert_eq!(spans[1].bottom_y, 8);
        assert_eq!(spans[1].top_y, 10);
        assert_eq!(spans[1].top_material, GRASS);
        // The heightfield surface is the TOP of the highest span.
        let top = spans.last().unwrap();
        assert_eq!(top.top_y, 10);
        assert_eq!(column_top_surface(&chunk, 5, 7), Some((10, GRASS)));
    }

    #[test]
    fn three_spans_overhang_stack() {
        // floor (0..=1), overhang (5..=6), roof (12..=12) → 3 spans, all
        // detected even though only the top renders today.
        let mut chunk = air_chunk();
        for y in 0..=1 {
            set_solid(&mut chunk, 1, y, 1, ROCK);
        }
        for y in 5..=6 {
            set_solid(&mut chunk, 1, y, 1, ROCK);
        }
        set_solid(&mut chunk, 1, 12, 1, GRASS);
        let spans = column_spans(&chunk, 1, 1);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[2].top_y, 12);
        assert_eq!(spans[2].top_material, GRASS);
    }

    #[test]
    fn full_solid_column_is_one_span_reaching_chunk_top() {
        let mut chunk = air_chunk();
        for y in 0..CHUNK_EDGE {
            set_solid(&mut chunk, 2, y, 3, GRASS);
        }
        let spans = column_spans(&chunk, 2, 3);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].bottom_y, 0);
        assert_eq!(spans[0].top_y, CHUNK_EDGE - 1, "run reaching the top closes at edge-1");
    }

    #[test]
    fn air_column_has_no_spans_and_no_surface() {
        let chunk = air_chunk();
        assert!(column_spans(&chunk, 0, 0).is_empty());
        assert_eq!(column_top_surface(&chunk, 0, 0), None);
    }

    #[test]
    fn low_occupancy_cells_are_not_solid() {
        // A cell with occupancy at/under the half-full threshold is NOT a
        // surface (prevents a wisp of terrain meshing as a full cell).
        let mut chunk = air_chunk();
        let i = DecodedChunk::index(0, 0, 0);
        chunk.material[i] = GRASS;
        chunk.occupancy[i] = SOLID_OCCUPANCY_THRESHOLD; // exactly at boundary → not solid
        assert!(column_spans(&chunk, 0, 0).is_empty());
        chunk.occupancy[i] = SOLID_OCCUPANCY_THRESHOLD + 1; // just over → solid
        assert_eq!(column_spans(&chunk, 0, 0).len(), 1);
    }

    #[test]
    fn water_marker_cells_are_not_solid_terrain() {
        // The importer's lifted-water sentinel (254) is excluded from spans
        // (water renders in its own layer); but fill-variant Water (id 22)
        // IS solid terrain.
        let mut chunk = air_chunk();
        let i = DecodedChunk::index(0, 0, 0);
        chunk.material[i] = WATER_MARKER;
        chunk.occupancy[i] = 255;
        assert!(column_spans(&chunk, 0, 0).is_empty(), "lifted-water sentinel is not solid");

        chunk.material[i] = TerrainMaterial::Water.to_u8(); // id 22
        assert_eq!(column_spans(&chunk, 0, 0).len(), 1, "fill-variant Water IS solid");
    }

    #[test]
    fn decode_roundtrips_an_importer_style_record() {
        // Build a record exactly like roblox-import's encode_eustress_chunk:
        // 3-byte header + CELLS_PER_CHUNK (material, occupancy) pairs. Put a
        // Grass surface cell at (0,0,0) and verify decode reproduces it.
        let mut raw = vec![EUSTRESS_CHUNK_VERSION, 1u8, 0u8]; // version, mat_count, flags
        let mut cells = vec![0u8; CELLS_PER_CHUNK * 2];
        // Air-fill: material AIR_MARKER, occupancy 0.
        for c in cells.chunks_exact_mut(2) {
            c[0] = AIR_MARKER;
            c[1] = 0;
        }
        // Cell (0,0,0): Grass, occupancy 255.
        let idx0 = DecodedChunk::index(0, 0, 0);
        cells[idx0 * 2] = GRASS;
        cells[idx0 * 2 + 1] = 255;
        raw.extend_from_slice(&cells);

        // Round-trip through the raw decoder.
        let decoded = decode_voxel_chunk_raw(&raw).expect("raw decode");
        assert_eq!(decoded.material.len(), CELLS_PER_CHUNK);
        assert_eq!(decoded.material_at(0, 0, 0), GRASS);
        assert_eq!(decoded.occupancy_at(0, 0, 0), 255);
        assert_eq!(column_top_surface(&decoded, 0, 0), Some((0, GRASS)));

        // And through the LZ4 layer the importer/Fjall actually store.
        let compressed = lz4_flex::compress_prepend_size(&raw);
        let via_lz4 = decode_voxel_chunk(&compressed).expect("lz4 decode");
        assert_eq!(via_lz4.material_at(0, 0, 0), GRASS);
    }

    #[test]
    fn decode_rejects_malformed_records_without_panicking() {
        // Too short.
        assert!(decode_voxel_chunk_raw(&[1, 0, 0]).is_err());
        // Right length, wrong version.
        let mut raw = vec![99u8, 0, 0];
        raw.extend(std::iter::repeat(0u8).take(CELLS_PER_CHUNK * 2));
        assert!(decode_voxel_chunk_raw(&raw).is_err());
        // Garbage LZ4 bytes.
        assert!(decode_voxel_chunk(&[0xff, 0xff, 0xff, 0xff, 0x00]).is_err());
    }

    #[test]
    fn fill_writes_top_surface_height_and_splat_bucket() {
        // A 1-radius terrain (chunks_x = chunks_z = 1 → 3×3 chunks).
        let config = voxel_terrain_config(1);
        assert_eq!(config.height_scale, 1.0, "voxel heights are raw studs");
        let mut data = TerrainData::default();

        // Chunk at voxel coords (0, 0, 0). One Grass column at local (0,0):
        // solid y=0..=3 → top cell y=3 → world top = (3+1)*4 = 16 studs.
        let mut chunk = air_chunk();
        for y in 0..=3 {
            set_solid(&mut chunk, 0, y, 0, GRASS);
        }

        let written = fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);
        assert_eq!(written, IVec2::new(0, 0));

        // The cache must be sized and carry the surface height at chunk (0,0)
        // local (0,0) → cache offset ((0 + chunks_x)*res, (0 + chunks_z)*res).
        let res = config.chunk_resolution as usize;
        let cache_width = data.cache_width as usize;
        let off_x = config.chunks_x as usize * res;
        let off_z = config.chunks_z as usize * res;
        let h = data.height_cache[off_z * cache_width + off_x];
        assert_eq!(h, 16.0, "world-Y top of a 4-cell column is 16 studs");

        // Splat: Grass → bucket 0, weight 1.0 in channel 0, others 0.
        let base = (off_z * cache_width + off_x) * SPLAT_CHANNELS;
        assert_eq!(data.splat_cache[base + 0], 1.0, "grass bucket weighted");
        assert_eq!(data.splat_cache[base + 1], 0.0);
        assert_eq!(data.splat_cache[base + 2], 0.0);
        assert_eq!(data.splat_cache[base + 3], 0.0);
        assert!(data.splat_dirty, "fill marks splat dirty for GPU re-upload");
    }

    #[test]
    fn fill_routes_materials_to_correct_splat_buckets() {
        // Rock → bucket 1, Snow → bucket 3 (the legacy 4-layer order).
        let config = voxel_terrain_config(1);
        let res = config.chunk_resolution as usize;
        let cache_width_chunks = config.chunks_x as usize;

        let mut data = TerrainData::default();
        let mut chunk = air_chunk();
        // Rock surface at column (0,0).
        set_solid(&mut chunk, 0, 0, 0, ROCK);
        // Snow surface at column (1,0). Snow == TerrainMaterial::Snow id 3.
        set_solid(&mut chunk, 1, 0, 0, TerrainMaterial::Snow.to_u8());

        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);

        let cache_width = data.cache_width as usize;
        let off_x = cache_width_chunks * res;
        let off_z = cache_width_chunks * res;

        // Column (0,0) → Rock bucket 1.
        let base0 = (off_z * cache_width + off_x) * SPLAT_CHANNELS;
        assert_eq!(data.splat_cache[base0 + 1], 1.0, "rock → bucket 1");
        assert_eq!(data.splat_cache[base0 + 0], 0.0);

        // Column (1,0) → Snow bucket 3. x advances by 1 cell.
        let base1 = (off_z * cache_width + (off_x + 1)) * SPLAT_CHANNELS;
        assert_eq!(data.splat_cache[base1 + 3], 1.0, "snow → bucket 3");
    }

    #[test]
    fn fill_raises_columns_in_a_higher_y_chunk() {
        // A voxel chunk at cy=1 sits 128 studs higher: a y=0 solid cell's
        // world top is 128 + (0+1)*4 = 132 studs.
        let config = voxel_terrain_config(1);
        let mut data = TerrainData::default();
        let mut chunk = air_chunk();
        set_solid(&mut chunk, 0, 0, 0, GRASS);

        fill_terrain_from_chunk(&mut data, &config, 0, 1, 0, &chunk);

        let res = config.chunk_resolution as usize;
        let cache_width = data.cache_width as usize;
        let off_x = config.chunks_x as usize * res;
        let off_z = config.chunks_z as usize * res;
        let h = data.height_cache[off_z * cache_width + off_x];
        assert_eq!(h, 132.0, "cy=1 chunk raises surface by one chunk edge (128)");
    }
}
