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
//!    and water occupancy in the 32³ grid, and
//! 2. for each `(x, z)` column, walks the Y stack and emits the **solid
//!    spans** (contiguous runs of occupied cells), then
//! 3. writes the TOP surface of the highest span into a `TerrainData`
//!    `height_cache` (raw world heights) and the surface material's own id,
//!    a built-in material slot, into the `material_cache`, and
//! 4. once every chunk is in, finishes the surfaces whose cell above lives
//!    in the chunk stacked on top ([`VoxelColumns::refine_surface_tops`]),
//!    builds the water level over every raster cell ([`voxel_water_levels`]),
//!    gives the columns no chunk holds a height ([`fill_hole_heights`]) and
//!    carves the air below each column's top surface (caves, tunnels, the
//!    undersides of overhangs) into the terrain's `TerrainVolume`
//!    ([`carve_voxel_caves`]).
//!
//! It deliberately does NOT read Fjall (that is the engine's job:
//! `eustress-common` must not depend on `eustress-worlddb`, cycle risk).
//! The build in `super::voxel_import` (`build_from_chunks`) takes the chunk
//! records, read from the world database by the engine-side loader
//! (`engine/src/terrain_voxel_load.rs`) or from the importer's
//! `voxel_chunks` files by the Player, and calls [`fill_terrain_from_chunk`]
//! and [`VoxelColumns::record_chunk`] here per chunk, then the passes of
//! step 4 in that order, [`carve_voxel_caves`] last, over the chunks
//! [`VoxelColumns::cave_chunks`] names.
//!
//! ## Surfaces and caves
//!
//! The heightfield holds the TOP surface of every column with its material
//! id in the material map, which is what `generate_chunk_mesh` /
//! `chunk_spawn_system` consume. The surface sits where the cell fill
//! crosses one half between the column's highest solid cell and the cell
//! above it, each cell's fill (`occupancy / 255`, 0 for air and lifted
//! water) standing at the cell's centre ([`isosurface_offset`]). A full cell
//! under air ends on its top face; a part-full top cell lowers the surface
//! and a part-full cell above raises it, by up to half a cell either way,
//! so an imported slope comes out smooth rather than in whole-cell steps.
//!
//! Everything under that surface that is not solid, between the
//! column's lowest and highest solid cell, becomes a CARVE edit in the
//! volume: `C = (occupancy / 255 - 0.5) * cell` at the cell's lattice point,
//! so air (occupancy 0) is carved half a cell deep and solid (255) stays
//! solid, with the carved wall taking the material of the nearest solid
//! cell. Chunks whose columns then hold bricks mesh by marching cubes at
//! LOD 0 and collide on that surface (see `volume` and `marching`).
//!
//! Air below a column's LOWEST solid cell is left alone, so the heightfield
//! keeps the terrain solid all the way down: carving the underside of the
//! whole map would put bricks under every chunk and draw a false floor where
//! the stored voxels end. A floating island over nothing therefore renders
//! as a pillar.
//!
//! ## Holes
//!
//! A raster column no chunk gives a solid cell keeps no material
//! ([`MATERIAL_SLOT_NONE`]). The loader marks the raster sparse
//! (`TerrainData::sparse_surface`), so meshes and colliders leave those
//! columns out and the map has no ground where the import has no voxels.
//! Meshes still read neighbouring heights for their normals, and the water
//! shader compares its level with the ground under it, so
//! [`fill_hole_heights`] gives every hole a height: one cell below its water
//! level where the column holds water, else that of the nearest ground in
//! its raster row.
//!
//! ## Water
//!
//! Every cell carries a water occupancy: the record's water plane when it
//! has one, else the occupancy of a lifted-water cell ([`WATER_MARKER`]).
//! [`voxel_water_levels`] puts a column's water surface at the fill of its
//! highest cell holding water, `cell + water / 255` cells up, which is the
//! top face of a full water cell.
//!
//! ## Units
//!
//! Cell and chunk INDICES are voxel units: 32 cells a chunk, chunks keyed
//! the way the importer and worlddb key them. World positions and heights
//! take the config's cell, [`voxel_cell_size`]: [`ROBLOX_CELL_STUDS`] studs
//! of the unit the import was authored in, so an import in feet (1 stud =
//! 1 ft) lays each cell out at 1.2192 m, where the parts converted from the
//! same unit expect their ground.
//!
//! ## Lattice alignment
//!
//! [`voxel_terrain_config`] makes the volume lattice cell (`chunk_size /
//! chunk_resolution`) exactly the voxel cell ([`voxel_cell_size`]), so voxel
//! cells and lattice points pair one to one. Every voxel cell is sampled at
//! its minimum-corner lattice point, the rule the heightfield fill already
//! uses in X and Z; a face-centred resample would collapse one-cell slabs
//! and one-cell tunnels to zero thickness. The carve reads the top surface
//! through `lattice_surface_height`, wherever the isosurface placed it, so
//! cave walls sit half a cell below the Roblox cell faces and a roof
//! directly under the top surface keeps at least the thickness of its solid
//! cells, never less.
//!
//! A floor under open air is lifted to its top face instead, so a cave floor
//! or the ground under an overhang sits flush with the open ground of full
//! cells beside it, whose surface is their top face, rather than half a
//! cell below it, with no step at the cave mouth. Two places keep the
//! half-cell rule: the foot of a wall (a lateral neighbour solid at the
//! floor's layer), which keeps a half-cell gutter there rather than a floor
//! pinched into the wall, and a one-cell gap, whose floor and roof keep
//! their symmetric crossings so a one-cell tunnel never collapses.
//!
//! The raster itself is not quite on the lattice: `TerrainData::sample_height`
//! spreads the `W` raster columns over the `W + 1` lattice points of the
//! terrain's width, so lattice point `p` (counted from the terrain's first
//! column) reads raster column `p * (W - 1) / W`, which falls behind `p` by
//! up to one column across the map. [`carve_voxel_caves`] resamples the same
//! way: each lattice column takes the voxel column nearest the one the
//! heightfield draws there, so caves stay under the surface they belong to
//! anywhere on the map.
//!
//! ## Chunk byte format (mirrors `roblox-import/src/terrain.rs`)
//!
//! The importer's `encode_eustress_chunk` (spec §6.6) writes, BEFORE LZ4:
//! ```text
//! u8  version (MIN_EUSTRESS_CHUNK_VERSION..=EUSTRESS_CHUNK_VERSION, 1..=2)
//! u8  material_count (informational)
//! u8  flags (bit0 CHUNK_FLAG_WATER: the chunk holds water;
//!            bit1 CHUNK_FLAG_WATER_PLANE: a water plane follows, version 2 only)
//! [ for each of 32^3 cells, Y-outer/Z-middle/X-inner order: ]
//!     u8 eustress_material_id  (255 = Air, 254 = Water)
//!     u8 occupancy_q           (0..=255)
//! [ only with CHUNK_FLAG_WATER_PLANE, for each cell in the same order: ]
//!     u8 water_occupancy       (0..=255)
//! ```
//! The record's linear cell index is Roblox's `y*1024 + z*32 + x` (Y outer,
//! Z middle, X inner). The decoder reorders the cells into
//! [`DecodedChunk`]'s `y*1024 + x*32 + z` (X middle, Z inner), the order
//! everything downstream reads through [`DecodedChunk::index`]. A version 2
//! record without the plane decodes exactly like version 1.
//! On disk / in Fjall the whole record is `lz4_flex::compress_prepend_size`,
//! so we `decompress_size_prepended` first. (We re-declare the small decode
//! here rather than depend on the bevy-free importer crate just for it.)

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use bevy::math::{IVec2, IVec3};

use super::material::{material_cell, TerrainMaterial, MATERIAL_SLOT_NONE};
use super::volume::{
    lattice_cell_size, lattice_surface_height, lattice_to_brick, quantize_distance, TerrainVolume, VolumeBrick,
    MATERIAL_NONE, Q_NONE, Q_STEPS_PER_CELL,
};
use super::{TerrainConfig, TerrainData};

// ---------------------------------------------------------------------------
// Constants, kept in lockstep with roblox-import/src/terrain.rs and
// worlddb/src/keys.rs (VOXEL_CHUNK_EDGE_STUDS). A mismatch would misread
// records or key chunks wrongly, so these are asserted in a test.
// ---------------------------------------------------------------------------

/// Cells along one edge of a voxel chunk (Roblox SmoothGrid uses 32).
pub const CHUNK_EDGE: usize = 32;

/// Total cells in one 32³ chunk.
pub const CELLS_PER_CHUNK: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE; // 32768

/// Roblox terrain cell edge in studs. Its size in the world depends on the
/// unit the import was authored in: [`voxel_terrain_config`] takes the cell
/// in metres (this many studs, each worth the metres of that unit) and
/// [`voxel_cell_size`] reads it back.
pub const ROBLOX_CELL_STUDS: f32 = 4.0;

/// One voxel chunk's edge in studs (`CHUNK_EDGE * ROBLOX_CELL_STUDS`).
/// MUST equal `eustress_worlddb::keys::VOXEL_CHUNK_EDGE_STUDS` (128.0), which
/// keys region queries in stud-space chunk coordinates. World distances come
/// from the config's cell ([`voxel_cell_size`]), not from this.
pub const VOXEL_CHUNK_EDGE_STUDS: f32 = CHUNK_EDGE as f32 * ROBLOX_CELL_STUDS; // 128.0

/// The importer's per-chunk header length, in bytes (version, material
/// count, flags). Cells start at this offset.
pub const CHUNK_HEADER_LEN: usize = 3;

/// The newest Eustress voxel-chunk record version this decoder reads, and
/// the one the importer writes (spec §6.6).
pub const EUSTRESS_CHUNK_VERSION: u8 = 2;

/// The oldest Eustress voxel-chunk record version this decoder reads.
pub const MIN_EUSTRESS_CHUNK_VERSION: u8 = 1;

/// Record flags bit: the chunk holds water.
pub const CHUNK_FLAG_WATER: u8 = 0b01;

/// Record flags bit: a water-occupancy plane, one byte per cell in the
/// cells' order, follows the cells. Version 2 records only.
pub const CHUNK_FLAG_WATER_PLANE: u8 = 0b10;

/// First record version that can carry a water plane.
const FIRST_WATER_PLANE_VERSION: u8 = 2;

/// Importer sentinel: a cell lifted into the separate water layer, its
/// occupancy the amount of water. NOT a terrain-fill material (so it does
/// not decode to a [`TerrainMaterial`]).
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

/// A decoded 32³ voxel chunk: parallel per-cell material, occupancy and
/// water arrays in linear `y*1024 + x*32 + z` order (the decoder reorders
/// the record's Roblox order into it). `material[i]` is an EUSTRESS
/// material id (or [`AIR_MARKER`] / [`WATER_MARKER`]).
#[derive(Debug, Clone)]
pub struct DecodedChunk {
    /// Per-cell Eustress material id (255 = Air, 254 = Water), YXZ order.
    pub material: Vec<u8>,
    /// Per-cell quantised occupancy (0..=255), YXZ order.
    pub occupancy: Vec<u8>,
    /// Per-cell water occupancy (0..=255), YXZ order: the record's water
    /// plane, or without one a lifted-water cell's occupancy (0 elsewhere).
    pub water: Vec<u8>,
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
        // Checked per axis: an X or Z past the edge would otherwise wrap into
        // the next row or layer rather than fall off the chunk.
        if x >= CHUNK_EDGE || y >= CHUNK_EDGE || z >= CHUNK_EDGE {
            return AIR_MARKER;
        }
        self.material
            .get(Self::index(x, y, z))
            .copied()
            .unwrap_or(AIR_MARKER)
    }

    /// Occupancy at local cell `(x, y, z)` (or 0 if OOB).
    #[inline]
    pub fn occupancy_at(&self, x: usize, y: usize, z: usize) -> u8 {
        if x >= CHUNK_EDGE || y >= CHUNK_EDGE || z >= CHUNK_EDGE {
            return 0;
        }
        self.occupancy.get(Self::index(x, y, z)).copied().unwrap_or(0)
    }

    /// Water occupancy at local cell `(x, y, z)` (or 0 if OOB).
    #[inline]
    pub fn water_at(&self, x: usize, y: usize, z: usize) -> u8 {
        if x >= CHUNK_EDGE || y >= CHUNK_EDGE || z >= CHUNK_EDGE {
            return 0;
        }
        self.water.get(Self::index(x, y, z)).copied().unwrap_or(0)
    }
}

/// [`DecodedChunk::index`] of the cell at linear index `roblox` of a chunk
/// record, whose cells run in Roblox order: `y*1024 + z*32 + x`, X inner.
#[inline]
fn index_from_roblox_order(roblox: usize) -> usize {
    let layer = CHUNK_EDGE * CHUNK_EDGE;
    let (y, rest) = (roblox / layer, roblox % layer);
    DecodedChunk::index(rest % CHUNK_EDGE, y, rest / CHUNK_EDGE)
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
/// `CELLS_PER_CHUNK` `(material, occupancy)` byte pairs and, when the flags
/// say one follows, the water plane. Never panics; any malformed input is an
/// `Err(ChunkDecodeError)` so the caller can skip the chunk and keep loading
/// the rest.
///
/// The LZ4 size prefix sizes the decompression buffer, so it is checked
/// first: a record holds one of two lengths, and a corrupt prefix is refused
/// before it can ask for a buffer of up to 4 GiB.
pub fn decode_voxel_chunk(compressed: &[u8]) -> Result<DecodedChunk, ChunkDecodeError> {
    let cells_end = CHUNK_HEADER_LEN + CELLS_PER_CHUNK * 2;
    let Some(prefix) = compressed.get(..4) else {
        return Err(ChunkDecodeError(format!(
            "chunk record too short for its size prefix: {} byte(s)",
            compressed.len()
        )));
    };
    let claimed = u32::from_le_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]) as usize;
    if claimed != cells_end && claimed != cells_end + CELLS_PER_CHUNK {
        return Err(ChunkDecodeError(format!(
            "chunk record claims {claimed} byte(s); a record holds {cells_end} or {}",
            cells_end + CELLS_PER_CHUNK
        )));
    }
    let raw = lz4_flex::decompress_size_prepended(compressed)
        .map_err(|e| ChunkDecodeError(format!("lz4 decompress failed: {e}")))?;
    decode_voxel_chunk_raw(&raw)
}

/// Decode an ALREADY-DECOMPRESSED chunk record (header, cells, and the water
/// plane when [`CHUNK_FLAG_WATER_PLANE`] is set). Split out so tests can
/// build a record without the LZ4 layer.
///
/// Cells are reordered from the record's Roblox order into
/// [`DecodedChunk`]'s (see the module docs), the water plane with them.
/// Rejects a version outside [`MIN_EUSTRESS_CHUNK_VERSION`]`..=`
/// [`EUSTRESS_CHUNK_VERSION`], a water-plane flag on a version 1 record, and
/// a length that does not match the flags.
pub fn decode_voxel_chunk_raw(raw: &[u8]) -> Result<DecodedChunk, ChunkDecodeError> {
    if raw.len() < CHUNK_HEADER_LEN {
        return Err(ChunkDecodeError(format!(
            "chunk record too short for its {CHUNK_HEADER_LEN}-byte header: {} byte(s)",
            raw.len()
        )));
    }
    let version = raw[0];
    if !(MIN_EUSTRESS_CHUNK_VERSION..=EUSTRESS_CHUNK_VERSION).contains(&version) {
        return Err(ChunkDecodeError(format!(
            "unsupported chunk version {version} (expected {MIN_EUSTRESS_CHUNK_VERSION}..={EUSTRESS_CHUNK_VERSION})"
        )));
    }
    // raw[1] = material_count (informational). Of the flags only the plane
    // bit shapes the record; the per-cell material id and water are
    // authoritative over CHUNK_FLAG_WATER.
    let flags = raw[2];
    let has_plane = (flags & CHUNK_FLAG_WATER_PLANE) != 0;
    if has_plane && version < FIRST_WATER_PLANE_VERSION {
        return Err(ChunkDecodeError(format!(
            "chunk version {version} cannot carry a water plane (flags {flags:#010b})"
        )));
    }
    let cells_end = CHUNK_HEADER_LEN + CELLS_PER_CHUNK * 2;
    let expected = if has_plane { cells_end + CELLS_PER_CHUNK } else { cells_end };
    if raw.len() != expected {
        return Err(ChunkDecodeError(format!(
            "chunk record wrong length: {} (expected {expected} = {CHUNK_HEADER_LEN} header + {CELLS_PER_CHUNK}*2 cells{})",
            raw.len(),
            if has_plane { format!(" + {CELLS_PER_CHUNK} water plane") } else { String::new() }
        )));
    }

    let mut material = vec![AIR_MARKER; CELLS_PER_CHUNK];
    let mut occupancy = vec![0; CELLS_PER_CHUNK];
    for (roblox, pair) in raw[CHUNK_HEADER_LEN..cells_end].chunks_exact(2).enumerate() {
        let i = index_from_roblox_order(roblox);
        material[i] = pair[0];
        occupancy[i] = pair[1];
    }
    let water = if has_plane {
        let mut water = vec![0; CELLS_PER_CHUNK];
        for (roblox, &amount) in raw[cells_end..].iter().enumerate() {
            water[index_from_roblox_order(roblox)] = amount;
        }
        water
    } else {
        // Without a plane only lifted water holds water, its occupancy the
        // amount.
        material
            .iter()
            .zip(&occupancy)
            .map(|(&cell_material, &cell_occupancy)| if cell_material == WATER_MARKER { cell_occupancy } else { 0 })
            .collect()
    };
    Ok(DecodedChunk { material, occupancy, water })
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
/// This is the multi-span detection the SCOPE DECISION calls for. The
/// heightfield draws the top span's surface; the gaps between spans are
/// what [`carve_voxel_caves`] carves out of the volume.
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

/// A `TerrainConfig` sized so one voxel chunk maps to one terrain chunk and
/// stored heights are raw WORLD heights (so `height_scale = 1.0` and
/// `height_offset = 0.0`).
///
/// The chunk grid spans `center_chunk ± (half_x, half_z)` chunks: the load
/// window the engine loader picks from the imported chunk columns.
/// `cell_size` is one voxel cell's edge in metres, [`ROBLOX_CELL_STUDS`]
/// studs of the unit the import was authored in (4 m in metres, 1.2192 m in
/// feet). `chunk_resolution = CHUNK_EDGE (32)` so there is one height sample
/// per voxel cell column; `chunk_size = CHUNK_EDGE * cell_size` so a chunk's
/// world footprint equals a voxel chunk's, which [`voxel_cell_size`] reads
/// back. The LOD distances and the view distance scale with the cell, so
/// each LOD band spans as many chunks whatever the unit.
pub fn voxel_terrain_config(center_chunk: IVec2, half_x: u32, half_z: u32, cell_size: f32) -> TerrainConfig {
    let scale = cell_size / ROBLOX_CELL_STUDS;
    TerrainConfig {
        chunk_size: CHUNK_EDGE as f32 * cell_size,
        chunk_resolution: CHUNK_EDGE as u32,
        chunks_x: half_x,
        chunks_z: half_z,
        center_chunk,
        lod_levels: 4,
        lod_distances: [256.0_f32, 512.0, 1024.0, 2048.0].iter().map(|distance| distance * scale).collect(),
        view_distance: 4096.0 * scale,
        // Heights stored as raw world heights, so `world_height` must be
        // the identity: unit scale, zero offset.
        height_scale: 1.0,
        height_offset: 0.0,
        seed: 0,
    }
}

/// Edge of one voxel cell in world units under a config from
/// [`voxel_terrain_config`]: `chunk_size / CHUNK_EDGE`. Every placement of
/// voxel cells in the world (positions, surface heights, water levels) goes
/// through this; the cell and chunk indices themselves stay in voxel units.
pub fn voxel_cell_size(config: &TerrainConfig) -> f32 {
    config.chunk_size / CHUNK_EDGE as f32
}

/// Fill of a cell for surface placement: occupancy / 255 for a terrain
/// material, 0 for air and lifted water.
#[inline]
fn cell_fill(material: u8, occupancy: u8) -> f32 {
    if material == AIR_MARKER || material == WATER_MARKER {
        0.0
    } else {
        occupancy as f32 / 255.0
    }
}

/// Offset, in cells above the top solid cell's floor, where the 0.5 isovalue
/// crosses between that cell (fill `f_top`, above 0.5) and the cell above it
/// (fill `f_above`, at most 0.5). Cell centres sit at 0.5 and 1.5, so
/// full-over-empty gives 1.0 (the cell's top face) and the offset stays
/// within `0.5..=1.5`. Fills with no span between them (equal, or not
/// finite) also give the top face, rather than a division by zero.
pub fn isosurface_offset(f_top: f32, f_above: f32) -> f32 {
    let span = f_top - f_above;
    if !span.is_finite() || span <= f32::EPSILON {
        return 1.0;
    }
    0.5 + ((f_top - 0.5) / span).clamp(0.0, 1.0)
}

/// Fill ONE voxel chunk's TOP-surface heights + materials into a
/// `TerrainData`, at the terrain-chunk grid position derived from the voxel
/// chunk coords. Returns the `(chunk_x, chunk_z)` grid cell written (so the
/// caller can track which terrain chunks were touched).
///
/// - `cx, cy, cz` are the voxel chunk's SIGNED coordinates (from the region
///   query / Morton key). The terrain grid is 2.5D, so the terrain chunk is
///   `(cx, cz)`; `cy` only contributes to the absolute world Y of the
///   surface (a chunk stacked higher in Y raises its columns).
/// - For each `(x, z)` column, the WORLD-Y surface over the highest solid
///   cell goes into `height_cache`, at the isosurface between that cell and
///   the cell above it (see the module docs), in cells of the config's
///   [`voxel_cell_size`]. On the chunk's top layer the cell above belongs to
///   the chunk stacked on this one, so the surface is placed as if that cell
///   were empty and [`VoxelColumns::refine_surface_tops`] finishes it once
///   every chunk is in. The top cell's material, as the built-in material
///   slot of the same id, goes into `material_cache` alone. Air columns
///   write nothing, so a column no chunk gives a solid cell keeps no
///   material ([`MATERIAL_SLOT_NONE`]): a hole (see the module docs).
/// - A chunk off the config's chunk grid writes nothing.
///
/// The height is stored as a RAW world height and the config uses
/// `height_scale = 1.0` with `height_offset = 0.0`, so the
/// `TerrainConfig::world_height` conversion `generate_chunk_mesh` applies is
/// the identity and yields the correct world Y with no normalization
/// round-trip.
///
/// Columns land where the config's chunk grid puts chunk `(cx, cz)` in the
/// raster (`TerrainConfig::chunk_grid_index`), the cells the renderer reads
/// back for that chunk.
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
    ensure_material_sized(data);

    let resolution = config.chunk_resolution as usize; // == CHUNK_EDGE
    let cell_size = voxel_cell_size(config);
    // Global index of this chunk's cell y == 0 (see `ColumnExtent`).
    let base = cy.saturating_mul(CHUNK_EDGE as i32);
    let chunk_pos = IVec2::new(cx, cz);

    // The heightfield is 2.5D but the voxel grid is 3D: MANY chunks stack at
    // the same `(cx, cz)`, each one addressing this same cache column. Writing
    // the whole tile unconditionally therefore let the LAST chunk to arrive
    // win — and since a chunk sitting above the surface is mostly air, whose
    // columns carry no height at all, it flattened real terrain to y=0.
    //
    // So combine instead of overwrite: keep the HIGHEST solid surface, and let
    // an air column contribute nothing. `already_has_surface` reads the
    // material cache, which holds a material ONLY where some chunk wrote a
    // real surface, so "has anything been written here yet" needs no
    // separate init/finalize pass. That also makes the result independent of
    // the order chunks arrive in, which matters because the store iterates
    // Morton order, not ascending Y.
    for z in 0..resolution {
        for x in 0..resolution {
            let Some((top_y, mat_id)) = column_top_surface(chunk, x, z) else {
                continue; // air column: leave any surface below it intact
            };
            let f_top = cell_fill(mat_id, chunk.occupancy_at(x, top_y, z));
            // The cell above the top one. Past the chunk's top layer it lives
            // in the chunk stacked above, which may not have arrived yet, so
            // it counts as empty here and `refine_surface_tops` finishes the
            // surface.
            let f_above = if top_y + 1 < CHUNK_EDGE {
                cell_fill(chunk.material_at(x, top_y + 1, z), chunk.occupancy_at(x, top_y + 1, z))
            } else {
                0.0
            };
            // World-Y of the surface: the isosurface's height in cells over
            // global cell 0, one voxel cell each (a cell at y occupies
            // [y, y+1) cells, so full over empty lands on the top cell's top
            // face, `top_y + 1` cells up the chunk).
            let top = base.saturating_add(top_y as i32);
            let world_top = (top as f32 + isosurface_offset(f_top, f_above)) * cell_size;
            let Some(px) = cache_pixel(data, config, chunk_pos, x, z) else {
                continue; // outside the sized cache
            };
            if already_has_surface(data, px) && data.height_cache[px] >= world_top {
                continue; // a higher surface is already recorded here
            }
            data.height_cache[px] = world_top;
            // The voxel id IS the built-in slot; an unknown id reads as the
            // default material rather than naming a custom slot by accident.
            if let Some(cell) = data.material_cache.get_mut(px) {
                *cell = material_cell(TerrainMaterial::from_u8_or_default(mat_id).to_u8());
            }
        }
    }

    data.material_dirty = true;
    chunk_pos
}

/// Linear `height_cache` index for one voxel column, or `None` when it falls
/// outside the sized cache.
///
/// Addresses the raster through the config's chunk grid
/// (`TerrainConfig::chunk_grid_index`), so a grid centred off the origin
/// places each chunk where the renderer reads it, and a chunk off the grid
/// is skipped rather than wrapped to some other index.
fn cache_pixel(
    data: &TerrainData,
    config: &TerrainConfig,
    chunk_pos: IVec2,
    x: usize,
    z: usize,
) -> Option<usize> {
    let resolution = config.chunk_resolution as usize;
    let cache_width = data.cache_width as usize;
    let grid = config.chunk_grid_index(chunk_pos)?;
    let px_x = grid.x as usize * resolution + x;
    let px_z = grid.y as usize * resolution + z;
    if px_x >= cache_width || px_z >= data.cache_height as usize {
        return None;
    }
    let idx = px_z * cache_width + px_x;
    (idx < data.height_cache.len()).then_some(idx)
}

/// Whether any chunk has already written a real surface at this pixel.
///
/// The material cache holds a material per surface column and
/// [`MATERIAL_SLOT_NONE`] everywhere else, so a material is an exact record
/// of "a surface was written here", which is what lets the height combine
/// distinguish "unset" from a genuine height of 0.0, and from legitimately
/// NEGATIVE terrain heights.
fn already_has_surface(data: &TerrainData, px: usize) -> bool {
    data.material_cache.get(px).is_some_and(|cell| cell[0] != MATERIAL_SLOT_NONE)
}

/// Size `material_cache` to `cache_width * cache_height` cells of "no
/// material" if it isn't already, so [`already_has_surface`] reads false
/// wherever no chunk has written. Deliberately not the all-Grass layer
/// `height_query::ensure_material_cache` allocates: that would read as a
/// surface everywhere.
fn ensure_material_sized(data: &mut TerrainData) {
    let needed = data.cache_width as usize * data.cache_height as usize;
    if data.material_cache.len() != needed {
        data.material_cache = vec![[MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]; needed];
    }
}

/// Give every hole of an imported raster a height, once every chunk has been
/// filled in and [`VoxelColumns::refine_surface_tops`] has run. A hole is a
/// column no chunk gave a solid cell, whose material stays
/// [`MATERIAL_SLOT_NONE`]: a sparse-surface terrain draws no ground there,
/// but its meshes still read neighbouring heights for their normals, and the
/// water shader compares the water level with the ground under it.
///
/// A hole under water (a finite entry of `water_levels`, laid out like
/// `height_cache` as [`voxel_water_levels`] builds it) sits one voxel cell
/// ([`voxel_cell_size`]) below its water level. Any other hole takes the
/// height of the nearest ground column in its raster row, the one before it
/// on a tie, and a row without ground copies the nearest row that has some,
/// the one before it on a tie. Materials stay as they are. Linear in the
/// raster's cells.
pub fn fill_hole_heights(config: &TerrainConfig, data: &mut TerrainData, water_levels: Option<&[f32]>) {
    let (width, depth) = (data.cache_width as usize, data.cache_height as usize);
    let cells = width * depth;
    if cells == 0 || data.height_cache.len() != cells || data.material_cache.len() != cells {
        return;
    }
    let cell_size = voxel_cell_size(config);
    let water = water_levels.filter(|levels| levels.len() == cells);
    let under_water = |i: usize| water.and_then(|levels| levels.get(i).copied()).filter(|level| level.is_finite());
    let heights = &mut data.height_cache;
    let materials = &data.material_cache;
    let is_ground = |i: usize| materials[i][0] != MATERIAL_SLOT_NONE;

    // Each hole of a row with ground takes the nearest ground column in it.
    let mut ground_before: Vec<Option<usize>> = vec![None; width];
    let mut row_has_ground = vec![false; depth];
    for z in 0..depth {
        let row = z * width;
        let mut last = None;
        for (x, before) in ground_before.iter_mut().enumerate() {
            if is_ground(row + x) {
                last = Some(x);
            }
            *before = last;
        }
        if last.is_none() {
            continue;
        }
        row_has_ground[z] = true;
        let mut after = None;
        for x in (0..width).rev() {
            let i = row + x;
            if is_ground(i) {
                after = Some(x);
                continue;
            }
            if let Some(level) = under_water(i) {
                heights[i] = level - cell_size;
                continue;
            }
            let nearest = match (ground_before[x], after) {
                (Some(before), Some(after)) => {
                    if x - before <= after - x {
                        before
                    } else {
                        after
                    }
                }
                (Some(side), None) | (None, Some(side)) => side,
                (None, None) => continue,
            };
            heights[i] = heights[row + nearest];
        }
    }

    // A row without ground copies the nearest row with some, now filled.
    let mut source: Vec<Option<usize>> = vec![None; depth];
    let mut last = None;
    for z in 0..depth {
        if row_has_ground[z] {
            last = Some(z);
        }
        source[z] = last;
    }
    let mut next = None;
    for z in (0..depth).rev() {
        if row_has_ground[z] {
            next = Some(z);
            continue;
        }
        source[z] = match (source[z], next) {
            (Some(before), Some(after)) => Some(if z - before <= after - z { before } else { after }),
            (before, after) => before.or(after),
        };
    }
    for z in (0..depth).filter(|&z| !row_has_ground[z]) {
        let row = z * width;
        for x in 0..width {
            let i = row + x;
            if let Some(level) = under_water(i) {
                heights[i] = level - cell_size;
            } else if let Some(from) = source[z] {
                heights[i] = heights[from * width + x];
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Column extents: where each column is solid and holds water, across chunks
// ---------------------------------------------------------------------------

/// Where one voxel column is solid, across every chunk stacked on it. Cell
/// indices are global: cell `y` of the chunk at `cy` is `cy * 32 + y`, which
/// is also the lattice Y index the cell is sampled at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnExtent {
    /// Lowest solid cell.
    pub lowest: i32,
    /// Highest solid cell. The heightfield surface sits over it, at the
    /// isosurface it makes with the cell above (see the module docs).
    pub highest: i32,
    /// Occupancy of the highest solid cell, the fill that places the surface
    /// over it.
    pub top_occupancy: u8,
    /// Solid cells in the column.
    pub solid_cells: u32,
}

impl ColumnExtent {
    /// A column no chunk has put a solid cell in.
    const NONE: Self = Self { lowest: i32::MAX, highest: i32::MIN, top_occupancy: 0, solid_cells: 0 };

    /// Some cell between the lowest and the highest solid cell is not solid:
    /// the column runs through a cave, a tunnel or the space under an
    /// overhang.
    pub fn has_gap(&self) -> bool {
        self.solid_cells > 0 && (self.solid_cells as i64) < self.highest as i64 - self.lowest as i64 + 1
    }
}

/// Where one voxel column holds water, across every chunk stacked on it, in
/// the global cells of [`ColumnExtent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaterExtent {
    /// Highest cell holding water.
    pub highest: i32,
    /// Water occupancy of that cell, 1..=255.
    pub occupancy: u8,
}

impl WaterExtent {
    /// A column no chunk has put water in.
    const NONE: Self = Self { highest: i32::MIN, occupancy: 0 };
}

/// Column extents of an import, gathered chunk by chunk, in any order, as
/// the chunks are filled into the heightfield ([`Self::record_chunk`]):
/// where each column is solid, where it holds water, and the part-full cells
/// of every chunk's bottom layer, which finish the surfaces under them
/// ([`Self::refine_surface_tops`]). Kept per chunk column (16 KB each, 8 KB
/// more for one holding water) rather than per voxel column, so a large
/// import does not pay for a map entry per column; a bottom layer takes
/// 1 KB, and only for a chunk that has a part-full bottom cell.
#[derive(Debug, Default)]
pub struct VoxelColumns {
    tiles: HashMap<IVec2, Box<[ColumnExtent; CHUNK_EDGE * CHUNK_EDGE]>>,
    water: HashMap<IVec2, Box<[WaterExtent; CHUNK_EDGE * CHUNK_EDGE]>>,
    /// By chunk coordinate: the occupancy of each bottom-layer cell that is
    /// neither solid nor empty, 0 for every other cell.
    bottom_fills: HashMap<IVec3, Box<[u8; CHUNK_EDGE * CHUNK_EDGE]>>,
}

impl VoxelColumns {
    /// Fold the chunk at `cx, cy, cz` into the extents of its columns: its
    /// solid cells, its water, and the part-full cells of its bottom layer.
    pub fn record_chunk(&mut self, cx: i32, cy: i32, cz: i32, chunk: &DecodedChunk) {
        let base = cy.saturating_mul(CHUNK_EDGE as i32);
        let mut bottom: Option<Box<[u8; CHUNK_EDGE * CHUNK_EDGE]>> = None;
        for z in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                let column = x + z * CHUNK_EDGE;
                let mut lowest: Option<usize> = None;
                let mut highest = 0usize;
                let mut solid = 0u32;
                let mut top_water: Option<(usize, u8)> = None;
                for y in 0..CHUNK_EDGE {
                    if cell_is_solid(chunk.material_at(x, y, z), chunk.occupancy_at(x, y, z)) {
                        if lowest.is_none() {
                            lowest = Some(y);
                        }
                        highest = y;
                        solid += 1;
                    }
                    let water = chunk.water_at(x, y, z);
                    if water > 0 {
                        top_water = Some((y, water));
                    }
                }
                // A part-full bottom cell is the cell above the top of the
                // column in the chunk under this one, when that top is the
                // chunk's top layer.
                let (bottom_material, bottom_occupancy) = (chunk.material_at(x, 0, z), chunk.occupancy_at(x, 0, z));
                if !cell_is_solid(bottom_material, bottom_occupancy) && cell_fill(bottom_material, bottom_occupancy) > 0.0 {
                    bottom.get_or_insert_with(|| Box::new([0; CHUNK_EDGE * CHUNK_EDGE]))[column] = bottom_occupancy;
                }
                if let Some((y, amount)) = top_water {
                    let tile = self
                        .water
                        .entry(IVec2::new(cx, cz))
                        .or_insert_with(|| Box::new([WaterExtent::NONE; CHUNK_EDGE * CHUNK_EDGE]));
                    let top = base.saturating_add(y as i32);
                    if top > tile[column].highest {
                        tile[column] = WaterExtent { highest: top, occupancy: amount };
                    }
                }
                let Some(lowest) = lowest else { continue };
                let tile = self
                    .tiles
                    .entry(IVec2::new(cx, cz))
                    .or_insert_with(|| Box::new([ColumnExtent::NONE; CHUNK_EDGE * CHUNK_EDGE]));
                let extent = &mut tile[column];
                let top = base.saturating_add(highest as i32);
                if top > extent.highest {
                    extent.highest = top;
                    extent.top_occupancy = chunk.occupancy_at(x, highest, z);
                }
                extent.lowest = extent.lowest.min(base.saturating_add(lowest as i32));
                extent.solid_cells = extent.solid_cells.saturating_add(solid);
            }
        }
        if let Some(bottom) = bottom {
            self.bottom_fills.insert(IVec3::new(cx, cy, cz), bottom);
        }
    }

    /// Finish the surfaces [`fill_terrain_from_chunk`] placed without the
    /// cell above them: a column whose highest solid cell is the top layer of
    /// its chunk has that cell in the chunk stacked on it, and when that cell
    /// is part full (from that chunk's bottom layer) the surface rises to the
    /// isosurface between the two. Call once every chunk has been filled into
    /// `data` under `config` and recorded here; any other column keeps the
    /// height the fill gave it.
    pub fn refine_surface_tops(&self, data: &mut TerrainData, config: &TerrainConfig) {
        let edge = CHUNK_EDGE as i32;
        let cell_size = voxel_cell_size(config);
        for (tile, extents) in &self.tiles {
            for z in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let column = x + z * CHUNK_EDGE;
                    let extent = extents[column];
                    if extent.solid_cells == 0 || extent.highest.rem_euclid(edge) != edge - 1 {
                        continue;
                    }
                    let above = IVec3::new(tile.x, extent.highest.div_euclid(edge).saturating_add(1), tile.y);
                    let f_above = self
                        .bottom_fills
                        .get(&above)
                        .map_or(0.0, |cells| cells[column] as f32 / 255.0);
                    if f_above <= 0.0 {
                        continue;
                    }
                    let Some(px) = cache_pixel(data, config, *tile, x, z) else { continue };
                    if !already_has_surface(data, px) {
                        continue;
                    }
                    // The top cell is solid, so its fill is its occupancy.
                    let f_top = extent.top_occupancy as f32 / 255.0;
                    data.height_cache[px] = (extent.highest as f32 + isosurface_offset(f_top, f_above)) * cell_size;
                }
            }
        }
    }

    /// Whether any recorded chunk holds water.
    pub fn has_water(&self) -> bool {
        !self.water.is_empty()
    }

    /// Extent of global voxel column `column` (`cx * 32 + x`, `cz * 32 + z`),
    /// `None` when no recorded chunk has a solid cell in it.
    pub fn extent(&self, column: IVec2) -> Option<ColumnExtent> {
        let edge = CHUNK_EDGE as i32;
        let tile = IVec2::new(column.x.div_euclid(edge), column.y.div_euclid(edge));
        let local = column.x.rem_euclid(edge) as usize + column.y.rem_euclid(edge) as usize * CHUNK_EDGE;
        let extent = self.tiles.get(&tile)?[local];
        (extent.solid_cells > 0).then_some(extent)
    }

    /// Whether any column has a gap, i.e. the import holds a cave.
    pub fn has_caves(&self) -> bool {
        self.tiles.values().any(|tile| tile.iter().any(ColumnExtent::has_gap))
    }

    /// The voxel chunks [`carve_voxel_caves`] reads: every chunk holding a
    /// cell between some column's lowest and highest solid cell, and the
    /// chunks around those, whose cells it reads for the walls and their
    /// materials. Empty for an import without caves, which then decodes no
    /// chunk a second time.
    pub fn cave_chunks(&self) -> HashSet<IVec3> {
        let edge = CHUNK_EDGE as i32;
        let mut chunks = HashSet::new();
        for (tile, extents) in &self.tiles {
            let (mut y0, mut y1) = (i32::MAX, i32::MIN);
            for extent in extents.iter().filter(|extent| extent.has_gap()) {
                y0 = y0.min(extent.lowest.saturating_add(1).div_euclid(edge));
                y1 = y1.max(extent.highest.saturating_sub(1).div_euclid(edge));
            }
            if y0 > y1 {
                continue;
            }
            for cy in y0.saturating_sub(1)..=y1.saturating_add(1) {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        chunks.insert(IVec3::new(tile.x + dx, cy, tile.y + dz));
                    }
                }
            }
        }
        chunks
    }

    /// Lattice tiles (32 x 32 lattice columns, one chunk column) that can
    /// show a column with a gap: the tiles holding one and their neighbours,
    /// since a lattice column shows the voxel column at or one before it.
    fn carve_tiles(&self) -> Vec<IVec2> {
        let mut tiles = HashSet::new();
        for (tile, extents) in &self.tiles {
            if extents.iter().any(ColumnExtent::has_gap) {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        tiles.insert(*tile + IVec2::new(dx, dz));
                    }
                }
            }
        }
        let mut tiles: Vec<IVec2> = tiles.into_iter().collect();
        tiles.sort_unstable_by_key(|tile| (tile.y, tile.x));
        tiles
    }
}

/// World Y of the water surface over every raster cell of `data`, laid out
/// and addressed like `height_cache` (through the pixel mapping
/// [`fill_terrain_from_chunk`] writes with), `f32::NAN` where the column is
/// dry. A column's surface sits at the fill of its highest cell holding
/// water, `(cell + water / 255)` voxel cells ([`voxel_cell_size`]) up: the
/// top face of a full water cell. Call once every chunk has been recorded in
/// `columns`, with the `config` and `data` the chunks were filled into.
pub fn voxel_water_levels(config: &TerrainConfig, data: &TerrainData, columns: &VoxelColumns) -> Vec<f32> {
    let cell_size = voxel_cell_size(config);
    let mut levels = vec![f32::NAN; data.height_cache.len()];
    for (tile, extents) in &columns.water {
        for z in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                let extent = extents[x + z * CHUNK_EDGE];
                if extent.occupancy == 0 {
                    continue;
                }
                if let Some(px) = cache_pixel(data, config, *tile, x, z) {
                    levels[px] = (extent.highest as f32 + extent.occupancy as f32 / 255.0) * cell_size;
                }
            }
        }
    }
    levels
}

// ---------------------------------------------------------------------------
// Caves: the air under each column's top surface, carved into the volume
// ---------------------------------------------------------------------------

/// Decoded voxel chunks by chunk coordinate, read as one grid of global
/// cells. A cell of a chunk it does not hold reads as air, as a chunk
/// missing from the store does.
#[derive(Debug, Default)]
pub struct VoxelGrid {
    chunks: HashMap<IVec3, DecodedChunk>,
}

impl VoxelGrid {
    /// Hold `chunk` as the chunk at `coord`.
    pub fn insert(&mut self, coord: IVec3, chunk: DecodedChunk) {
        self.chunks.insert(coord, chunk);
    }

    /// Number of chunks held.
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether no chunk is held.
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Material id and occupancy of global cell `cell`.
    pub fn cell(&self, cell: IVec3) -> (u8, u8) {
        let edge = CHUNK_EDGE as i32;
        let coord = IVec3::new(cell.x.div_euclid(edge), cell.y.div_euclid(edge), cell.z.div_euclid(edge));
        let Some(chunk) = self.chunks.get(&coord) else {
            return (AIR_MARKER, 0);
        };
        let (x, y, z) = (
            cell.x.rem_euclid(edge) as usize,
            cell.y.rem_euclid(edge) as usize,
            cell.z.rem_euclid(edge) as usize,
        );
        (chunk.material_at(x, y, z), chunk.occupancy_at(x, y, z))
    }

    fn is_solid(&self, cell: IVec3) -> bool {
        let (material, occupancy) = self.cell(cell);
        cell_is_solid(material, occupancy)
    }
}

/// Maps lattice columns to the voxel columns the heightfield draws there.
///
/// `TerrainData::sample_height` reads raster column `p * (W - 1) / W` at
/// lattice point `p`, both counted from the terrain's first column (the
/// first of its first chunk, `TerrainConfig::chunk_min`), with `W` raster
/// columns across. This takes the nearest whole column, so a feature one
/// column wide keeps a column instead of being blended into two.
#[derive(Clone, Copy, Debug)]
struct RasterColumns {
    offset_x: i64,
    offset_z: i64,
    width: i64,
    depth: i64,
}

impl RasterColumns {
    fn new(config: &TerrainConfig, data: &TerrainData) -> Option<Self> {
        if data.cache_width < 2 || data.cache_height < 2 {
            return None;
        }
        let resolution = config.chunk_resolution as i64;
        let first_chunk = config.chunk_min();
        Some(Self {
            offset_x: -(first_chunk.x as i64) * resolution,
            offset_z: -(first_chunk.y as i64) * resolution,
            width: data.cache_width as i64,
            depth: data.cache_height as i64,
        })
    }

    /// Voxel column drawn at lattice column `n` of an axis whose first
    /// lattice column is `-offset` and whose raster is `pixels` columns wide,
    /// or `None` off the terrain.
    fn axis(n: i32, offset: i64, pixels: i64) -> Option<i32> {
        let p = n as i64 + offset;
        if p < 0 || p > pixels {
            return None;
        }
        // round(p * (pixels - 1) / pixels), halves rounding up.
        let nearest = (2 * p * (pixels - 1) + pixels).div_euclid(2 * pixels);
        i32::try_from(nearest - offset).ok()
    }

    fn voxel_column(&self, n: IVec2) -> Option<IVec2> {
        Some(IVec2::new(
            Self::axis(n.x, self.offset_x, self.width)?,
            Self::axis(n.y, self.offset_z, self.depth)?,
        ))
    }
}

/// Stored CARVE step of a cell: `C = (occupancy / 255 - 0.5) * cell`, air and
/// lifted water counting as empty. `f = 0` reads as air, so a solid cell
/// never stores a step of 0 or below (an only just solid cell would round
/// to 0 and vanish) and an open cell never stores more than 0.
fn carve_step(material: u8, occupancy: u8, cell: f32) -> i8 {
    let filled = if material == AIR_MARKER || material == WATER_MARKER { 0 } else { occupancy };
    let step = quantize_distance((filled as f32 / 255.0 - 0.5) * cell, cell);
    if cell_is_solid(material, occupancy) {
        step.max(1)
    } else {
        step.min(0)
    }
}

/// Carve step of an open cell whose floor is lifted to the top face of the
/// solid cell under it (see the module docs). Against the solid cell's half
/// cell below, the floor crossing lands at `16/17` of the lattice edge, a
/// seventeenth of a cell under the top face the heightfield ground beside it
/// is drawn at. Negative, so the point's field stays strictly positive:
/// at exactly 0 it would still read as air, but the floor crossing would sit
/// on the lattice point itself.
const LIFTED_FLOOR_STEP: i8 = -1;

/// Whether lattice column `entry` (an entry of `carve_tile`'s `info`: its
/// voxel column and extent) is solid at layer `y`: below its lowest solid
/// cell, which stays solid all the way down, or at a solid cell of its
/// range. Off the terrain, without a solid cell, or above its highest solid
/// cell the heightfield decides, which is air at a layer beside a carved
/// floor.
fn lattice_column_is_solid(entry: Option<(IVec2, Option<ColumnExtent>)>, y: i32, grid: &VoxelGrid) -> bool {
    match entry {
        Some((voxel, Some(extent))) => {
            y < extent.lowest || (y <= extent.highest && grid.is_solid(IVec3::new(voxel.x, y, voxel.y)))
        }
        _ => false,
    }
}

/// Carve step for a lattice point where the heightfield alone decides
/// (above its column's top cell, or in a column with no solid cell), from
/// the heightfield term `heightfield` there: the smallest step that keeps
/// `-C` strictly below `fh`. `max(fh, -C)` stays `fh` on the lattice, and
/// the heightfield also wins `FieldSample::term()` there, whose ties go to
/// the carve: a tie would colour the ground vertex on this point as the
/// carve's material instead of with the heightfield's material map. Between
/// lattice points, where samples interpolate `C`, it keeps the carved air
/// beside it from reaching past it into ground the mesh draws solid.
fn neutral_step(heightfield: f32, cell: f32) -> i8 {
    if !heightfield.is_finite() {
        return Q_NONE;
    }
    let steps = ((-heightfield / cell * Q_STEPS_PER_CELL).floor() + 1.0).max(1.0);
    steps.min(Q_NONE as f32) as i8
}

/// Offsets up to two cells away, nearest first. Ties go downward first, so
/// a wall between a floor and a roof takes the floor's material.
fn material_search_offsets() -> &'static [IVec3] {
    static OFFSETS: OnceLock<Vec<IVec3>> = OnceLock::new();
    OFFSETS.get_or_init(|| {
        let mut offsets = Vec::new();
        for dz in -2..=2 {
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let offset = IVec3::new(dx, dy, dz);
                    if (1..=4).contains(&offset.length_squared()) {
                        offsets.push(offset);
                    }
                }
            }
        }
        offsets.sort_unstable_by_key(|offset| (offset.length_squared(), offset.y, offset.x, offset.z));
        offsets
    })
}

/// `TerrainMaterial` id of the solid cell nearest `cell`, itself included,
/// or `MATERIAL_NONE` (the mesher's default rock) with none within two cells.
fn nearest_solid_material(grid: &VoxelGrid, cell: IVec3) -> u8 {
    std::iter::once(IVec3::ZERO)
        .chain(material_search_offsets().iter().copied())
        .find_map(|offset| {
            let (material, occupancy) = grid.cell(cell + offset);
            cell_is_solid(material, occupancy).then(|| TerrainMaterial::from_u8_or_default(material).to_u8())
        })
        .unwrap_or(MATERIAL_NONE)
}

/// What [`carve_voxel_caves`] wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoxelCaveReport {
    /// Lattice points carved to air.
    pub carved_points: usize,
    /// Lattice points written, the solid walls around the air included.
    pub written_points: usize,
    /// Bricks the volume holds afterwards.
    pub bricks: usize,
}

/// Most layers one lattice tile carves (32,768 studs of 4-stud cells): a
/// column whose gap claims more is corrupt, and its tile is skipped.
const MAX_CARVE_LAYERS: i64 = 8192;

/// Carve the caves of an import into `volume`, after every chunk has been
/// filled into `data` and recorded in `columns`, with `grid` holding the
/// chunks [`VoxelColumns::cave_chunks`] names.
///
/// A lattice point is carved when the voxel cell it samples lies between its
/// column's lowest and highest solid cell and is not solid; its step is
/// [`carve_step`], or [`LIFTED_FLOOR_STEP`] for a floor lifted flush with the
/// ground (see the module docs), and its material that of the nearest solid
/// cell. Every
/// other point within one lattice step of a carved point is written too:
/// a solid cell with its own (positive) step and material, so each wall
/// lands halfway between a carved and a solid point, where the cell faces put
/// it, rather than being pulled toward the carved side by the far weaker
/// heightfield term; a cell below its column's lowest solid cell as solid;
/// and a point above its column's top cell (or in a column with no solid
/// cell) with a [`neutral_step`], which leaves the heightfield alone there.
/// Points further from any carved point stay unwritten, so the volume holds
/// bricks only around caves. Existing carves deeper than the import's are
/// kept.
///
/// Writes nothing, with a warning, for a config whose lattice does not pair
/// one to one with the voxel cells (see the module docs).
pub fn carve_voxel_caves(
    config: &TerrainConfig,
    data: &TerrainData,
    columns: &VoxelColumns,
    grid: &VoxelGrid,
    volume: &mut TerrainVolume,
) -> VoxelCaveReport {
    let mut report = VoxelCaveReport::default();
    let cell = lattice_cell_size(config);
    let voxel_cell = voxel_cell_size(config);
    let aligned = (cell - voxel_cell).abs() <= voxel_cell * 1e-4
        && config.chunk_resolution as usize == CHUNK_EDGE
        && config.resolution_for_lod(0) as usize == CHUNK_EDGE;
    let raster = RasterColumns::new(config, data);
    match raster {
        Some(raster) if aligned => {
            let mut bricks: HashMap<IVec3, VolumeBrick> = HashMap::new();
            let sources = CarveSources { config, data, columns, grid, raster, cell };
            for tile in columns.carve_tiles() {
                carve_tile(tile, &sources, volume, &mut bricks, &mut report);
            }
            for (coord, brick) in bricks {
                volume.set_brick(coord, Some(brick));
            }
        }
        _ if columns.has_caves() => {
            tracing::warn!(
                cell,
                chunk_resolution = config.chunk_resolution,
                "voxel caves: the terrain has no height raster or its lattice does not pair with the voxel cells; caves were not carved"
            );
        }
        _ => {}
    }
    report.bricks = volume.brick_count();
    report
}

/// What [`carve_tile`] reads.
struct CarveSources<'a> {
    config: &'a TerrainConfig,
    data: &'a TerrainData,
    columns: &'a VoxelColumns,
    grid: &'a VoxelGrid,
    raster: RasterColumns,
    cell: f32,
}

/// Carve the lattice columns of one lattice tile. See [`carve_voxel_caves`].
fn carve_tile(
    tile: IVec2,
    sources: &CarveSources,
    volume: &TerrainVolume,
    bricks: &mut HashMap<IVec3, VolumeBrick>,
    report: &mut VoxelCaveReport,
) {
    const EDGE: i32 = CHUNK_EDGE as i32;
    // The tile's columns plus one on every side, whose carved points reach
    // into the tile.
    const PAD: i32 = EDGE + 2;
    let CarveSources { config, data, columns, grid, raster, cell } = *sources;
    let origin = tile * EDGE;
    let lattice_column = |px: i32, pz: i32| origin + IVec2::new(px - 1, pz - 1);
    let slot = |px: i32, pz: i32| (pz * PAD + px) as usize;

    // Each padded column's voxel column (`None` off the terrain) and extent
    // (`None` without a solid cell), and the layers any gap among them spans.
    let mut info: Vec<Option<(IVec2, Option<ColumnExtent>)>> = Vec::with_capacity((PAD * PAD) as usize);
    let (mut y0, mut y1) = (i32::MAX, i32::MIN);
    for pz in 0..PAD {
        for px in 0..PAD {
            let entry = raster
                .voxel_column(lattice_column(px, pz))
                .map(|voxel| (voxel, columns.extent(voxel)));
            if let Some((_, Some(extent))) = entry {
                if extent.has_gap() {
                    y0 = y0.min(extent.lowest.saturating_add(1));
                    y1 = y1.max(extent.highest.saturating_sub(1));
                }
            }
            info.push(entry);
        }
    }
    if y0 > y1 {
        return;
    }
    if y1 as i64 - y0 as i64 >= MAX_CARVE_LAYERS {
        tracing::warn!(tile = ?tile, y0, y1, "voxel caves: a column gap spans too many layers; its tile was not carved");
        return;
    }

    // Carved points: open cells strictly between their column's lowest and
    // highest solid cell.
    let layers = (y1 - y0 + 1) as usize;
    let mut carved = vec![false; (PAD * PAD) as usize * layers];
    let mut any = false;
    for pz in 0..PAD {
        for px in 0..PAD {
            let c = slot(px, pz);
            let Some((voxel, Some(extent))) = info[c] else { continue };
            if !extent.has_gap() {
                continue;
            }
            let first = extent.lowest.saturating_add(1).max(y0);
            let last = extent.highest.saturating_sub(1).min(y1);
            for y in first..=last {
                if !grid.is_solid(IVec3::new(voxel.x, y, voxel.y)) {
                    carved[c * layers + (y - y0) as usize] = true;
                    any = true;
                }
            }
        }
    }
    if !any {
        return;
    }

    // `near[c][k]`: column `c` has a carved point at layer `y0 - 1 + k`, or
    // one layer above or below it.
    let near_layers = layers + 2;
    let mut near = vec![false; (PAD * PAD) as usize * near_layers];
    for c in 0..(PAD * PAD) as usize {
        for s in 0..layers {
            if carved[c * layers + s] {
                let start = c * near_layers + s;
                near[start..start + 3].fill(true);
            }
        }
    }

    let solid_step = carve_step(TerrainMaterial::Rock.to_u8(), u8::MAX, cell);
    for pz in 1..=EDGE {
        for px in 1..=EDGE {
            // Off the terrain: no chunk draws this column.
            let Some((voxel, extent)) = info[slot(px, pz)] else { continue };
            let n = lattice_column(px, pz);
            let surface = lattice_surface_height(config, data, n.x, n.y);
            for k in 0..near_layers {
                let touched = (-1..=1).any(|dz| (-1..=1).any(|dx| near[slot(px + dx, pz + dz) * near_layers + k]));
                if !touched {
                    continue;
                }
                let y = y0 - 1 + k as i32;
                let at = IVec3::new(voxel.x, y, voxel.y);
                let (step, material, carves) = match extent {
                    // Below the column's lowest solid cell the terrain stays
                    // solid all the way down (see the module docs).
                    Some(extent) if y < extent.lowest => (solid_step, nearest_solid_material(grid, at), false),
                    Some(extent) if y <= extent.highest => {
                        let (material, occupancy) = grid.cell(at);
                        let step = carve_step(material, occupancy, cell);
                        if cell_is_solid(material, occupancy) {
                            (step, TerrainMaterial::from_u8_or_default(material).to_u8(), false)
                        } else {
                            // A carved point is below `extent.highest`, so the
                            // cell above it is still inside the column's range.
                            let floor_under_open_air = grid.is_solid(at - IVec3::Y)
                                && !grid.is_solid(at + IVec3::Y)
                                && [(-1, 0), (1, 0), (0, -1), (0, 1)]
                                    .iter()
                                    .all(|&(dx, dz)| !lattice_column_is_solid(info[slot(px + dx, pz + dz)], y, grid));
                            let step = if floor_under_open_air { LIFTED_FLOOR_STEP } else { step };
                            (step, nearest_solid_material(grid, at), true)
                        }
                    }
                    // Above the column's top cell, or a column with no solid
                    // cell: the heightfield decides alone.
                    _ => (neutral_step(y as f32 * cell - surface, cell), MATERIAL_NONE, false),
                };
                let (coord, index) = lattice_to_brick(IVec3::new(n.x, y, n.y));
                let brick = bricks.entry(coord).or_insert_with(|| volume.brick(coord).cloned().unwrap_or_default());
                if step < brick.carve[index] {
                    brick.carve[index] = step;
                    brick.material[index] = material;
                }
                report.written_points += 1;
                if carves {
                    report.carved_points += 1;
                }
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

    /// Build a fully-air `DecodedChunk` (every cell air, occupancy 0, dry).
    fn air_chunk() -> DecodedChunk {
        DecodedChunk {
            material: vec![AIR_MARKER; CELLS_PER_CHUNK],
            occupancy: vec![0; CELLS_PER_CHUNK],
            water: vec![0; CELLS_PER_CHUNK],
        }
    }

    /// Set one cell solid (`occupancy = 255`) with the given material.
    fn set_solid(chunk: &mut DecodedChunk, x: usize, y: usize, z: usize, material: u8) {
        let i = DecodedChunk::index(x, y, z);
        chunk.material[i] = material;
        chunk.occupancy[i] = 255;
    }

    /// Set one cell's material, occupancy and water.
    fn set_cell(chunk: &mut DecodedChunk, (x, y, z): (usize, usize, usize), material: u8, occupancy: u8, water: u8) {
        let i = DecodedChunk::index(x, y, z);
        chunk.material[i] = material;
        chunk.occupancy[i] = occupancy;
        chunk.water[i] = water;
    }

    /// Linear index of local cell `(x, y, z)` in a chunk record's cells:
    /// Roblox order, X innermost, the order the importer writes.
    fn roblox_index(x: usize, y: usize, z: usize) -> usize {
        y * CHUNK_EDGE * CHUNK_EDGE + z * CHUNK_EDGE + x
    }

    /// A chunk record the way the importer writes it: the header, then each
    /// cell's `(material, occupancy)` from `cell`, in Roblox order.
    fn record(version: u8, flags: u8, cell: impl Fn(usize, usize, usize) -> (u8, u8)) -> Vec<u8> {
        let mut raw = vec![version, 1, flags];
        raw.resize(CHUNK_HEADER_LEN + CELLS_PER_CHUNK * 2, 0);
        for y in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let (material, occupancy) = cell(x, y, z);
                    let at = CHUNK_HEADER_LEN + roblox_index(x, y, z) * 2;
                    raw[at] = material;
                    raw[at + 1] = occupancy;
                }
            }
        }
        raw
    }

    /// `raw` with a water plane appended, each cell's water from `water`, in
    /// the cells' Roblox order.
    fn with_plane(mut raw: Vec<u8>, water: impl Fn(usize, usize, usize) -> u8) -> Vec<u8> {
        let start = raw.len();
        raw.resize(start + CELLS_PER_CHUNK, 0);
        for y in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    raw[start + roblox_index(x, y, z)] = water(x, y, z);
                }
            }
        }
        raw
    }

    /// The importer's water invariant: a cell holding both a solid material
    /// and water (Roblox Shorelines) is never full of solid, and a pure water
    /// cell's occupancy is its amount of water.
    fn assert_importer_water_invariant(chunk: &DecodedChunk) {
        for i in 0..CELLS_PER_CHUNK {
            let (material, occupancy, water) = (chunk.material[i], chunk.occupancy[i], chunk.water[i]);
            if material == WATER_MARKER {
                assert_eq!(water, occupancy, "cell {i}: pure water holds its occupancy as water");
            } else if material != AIR_MARKER && water > 0 {
                assert!(occupancy < u8::MAX, "cell {i}: a Shorelines cell is never full of solid");
            }
        }
    }

    /// Raster index of local column `(x, z)` of chunk (0, 0) under a grid
    /// centred on the origin.
    fn pixel(config: &TerrainConfig, data: &TerrainData, x: usize, z: usize) -> usize {
        let res = config.chunk_resolution as usize;
        let off_x = config.chunks_x as usize * res;
        let off_z = config.chunks_z as usize * res;
        (off_z + z) * data.cache_width as usize + off_x + x
    }

    #[test]
    fn constants_agree_with_worlddb_and_importer() {
        // CHUNK_EDGE * ROBLOX_CELL_STUDS must equal the worlddb chunk edge
        // (128) or region queries key chunks wrongly.
        assert_eq!(VOXEL_CHUNK_EDGE_STUDS, 128.0);
        assert_eq!(CELLS_PER_CHUNK, 32_768);
        assert_eq!(CHUNK_HEADER_LEN, 3);
        assert!((MIN_EUSTRESS_CHUNK_VERSION..=EUSTRESS_CHUNK_VERSION).contains(&FIRST_WATER_PLANE_VERSION));
        assert_eq!(CHUNK_FLAG_WATER & CHUNK_FLAG_WATER_PLANE, 0, "the flags are distinct bits");
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
        // 3-byte header + CELLS_PER_CHUNK (material, occupancy) pairs, air
        // everywhere but a Grass surface cell at (0,0,0), and verify decode
        // reproduces it.
        let raw = record(EUSTRESS_CHUNK_VERSION, 0, |x, y, z| {
            if (x, y, z) == (0, 0, 0) {
                (GRASS, 255)
            } else {
                (AIR_MARKER, 0)
            }
        });

        // Round-trip through the raw decoder.
        let decoded = decode_voxel_chunk_raw(&raw).expect("raw decode");
        assert_eq!(decoded.material.len(), CELLS_PER_CHUNK);
        assert_eq!(decoded.water.len(), CELLS_PER_CHUNK);
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
        assert!(decode_voxel_chunk_raw(&[EUSTRESS_CHUNK_VERSION]).is_err());
        assert!(decode_voxel_chunk_raw(&[]).is_err());
        // Right length, wrong version.
        let mut raw = vec![99u8, 0, 0];
        raw.extend(std::iter::repeat(0u8).take(CELLS_PER_CHUNK * 2));
        assert!(decode_voxel_chunk_raw(&raw).is_err());
        // Garbage LZ4 bytes.
        assert!(decode_voxel_chunk(&[0xff, 0xff, 0xff, 0xff, 0x00]).is_err());
    }

    /// The importer writes cells X innermost; the decoder reorders them so
    /// `material_at(x, y, z)` names the same cell the importer wrote there.
    #[test]
    fn cells_decode_from_the_records_roblox_order() {
        let raw = record(MIN_EUSTRESS_CHUNK_VERSION, 0, |x, y, z| {
            if (x, y, z) == (3, 5, 7) {
                (ROCK, 255)
            } else {
                (AIR_MARKER, 0)
            }
        });
        let chunk = decode_voxel_chunk_raw(&raw).expect("decodes");
        assert!(cell_is_solid(chunk.material_at(3, 5, 7), chunk.occupancy_at(3, 5, 7)));
        assert_eq!(chunk.material_at(7, 5, 3), AIR_MARKER, "x and z are not swapped");
        assert_eq!(column_top_surface(&chunk, 3, 7), Some((5, ROCK)));
        assert_eq!(column_top_surface(&chunk, 7, 3), None);
    }

    /// Ported from the importer owner's seam metric: across the seam between
    /// two chunks side by side in X, a smooth slope steps no more than it
    /// does between neighbouring columns inside either chunk.
    ///
    /// The slope rises a quarter cell per column in X and an eighth in Z.
    /// With the cells in the importer's order the seam steps a quarter cell,
    /// like the interior (ratio 1). Read with X and Z swapped, each chunk's
    /// columns would hold its transpose: the seam would then join column
    /// `z` of one chunk's first row to row 31 of the other, about 4.1 cells
    /// apart, against interior X steps of an eighth (ratio about 33).
    #[test]
    fn a_slope_across_a_chunk_seam_steps_like_its_interior() {
        // Top solid cell of global column (gx, gz).
        let top = |gx: usize, gz: usize| 2 + (2 * gx + gz) / 8;
        let chunk = |cx: usize| {
            let raw = record(EUSTRESS_CHUNK_VERSION, 0, |x, y, z| {
                if y <= top(cx * CHUNK_EDGE + x, z) {
                    (ROCK, 255)
                } else {
                    (AIR_MARKER, 0)
                }
            });
            decode_voxel_chunk_raw(&raw).expect("decodes")
        };
        let (west, east) = (chunk(0), chunk(1));
        let height = |chunk: &DecodedChunk, x: usize, z: usize| {
            column_top_surface(chunk, x, z).expect("every column has ground").0 as f32
        };

        let (mut seam, mut interior, mut interior_pairs) = (0.0f32, 0.0f32, 0usize);
        for z in 0..CHUNK_EDGE {
            seam += (height(&east, 0, z) - height(&west, CHUNK_EDGE - 1, z)).abs();
            for chunk in [&west, &east] {
                for x in 0..CHUNK_EDGE - 1 {
                    interior += (height(chunk, x + 1, z) - height(chunk, x, z)).abs();
                    interior_pairs += 1;
                }
            }
        }
        let seam_step = seam / CHUNK_EDGE as f32;
        let interior_step = interior / interior_pairs as f32;
        assert!(interior_step > 0.0, "the slope rises inside the chunks");
        let ratio = seam_step / interior_step;
        assert!(ratio < 1.5, "the seam steps {seam_step} cells against {interior_step} inside (ratio {ratio})");
    }

    #[test]
    fn a_version_2_water_plane_decodes_in_the_cells_order() {
        // A Shorelines cell (rock, part full, under water) with a pure water
        // cell above it, as the importer writes them.
        let raw = with_plane(
            record(EUSTRESS_CHUNK_VERSION, CHUNK_FLAG_WATER | CHUNK_FLAG_WATER_PLANE, |x, y, z| {
                match (x, y, z) {
                    (2, 4, 9) => (ROCK, 200),
                    (2, 5, 9) => (WATER_MARKER, 90),
                    _ => (AIR_MARKER, 0),
                }
            }),
            |x, y, z| match (x, y, z) {
                (2, 4, 9) => 180,
                (2, 5, 9) => 90,
                _ => 0,
            },
        );
        let chunk = decode_voxel_chunk_raw(&raw).expect("decodes");
        assert_eq!(chunk.water_at(2, 4, 9), 180, "the plane gives the Shorelines cell its water");
        assert_eq!(chunk.water_at(9, 4, 2), 0, "the plane is reordered with the cells");
        assert_eq!(chunk.water_at(2, 5, 9), 90);
        assert_eq!(chunk.material_at(2, 4, 9), ROCK);
        assert_eq!(chunk.occupancy_at(2, 4, 9), 200);
        assert_eq!(chunk.water_at(CHUNK_EDGE, 0, 0), 0, "out of bounds is dry");
        assert_importer_water_invariant(&chunk);
    }

    #[test]
    fn a_version_2_record_without_a_plane_decodes_like_version_1() {
        let cells = |x: usize, y: usize, z: usize| -> (u8, u8) {
            match (x, y, z) {
                (1, 2, 3) => (GRASS, 255),
                (1, 3, 3) => (WATER_MARKER, 120),
                (4, 3, 5) => (ROCK, 140),
                _ => (AIR_MARKER, 0),
            }
        };
        let v1 = decode_voxel_chunk_raw(&record(MIN_EUSTRESS_CHUNK_VERSION, CHUNK_FLAG_WATER, cells)).expect("v1");
        let v2 = decode_voxel_chunk_raw(&record(EUSTRESS_CHUNK_VERSION, CHUNK_FLAG_WATER, cells)).expect("v2");
        assert_eq!(v1.material, v2.material);
        assert_eq!(v1.occupancy, v2.occupancy);
        assert_eq!(v1.water, v2.water);
        // Without a plane, lifted water holds its occupancy as water and
        // nothing else holds any.
        assert_eq!(v2.water_at(1, 3, 3), 120);
        assert_eq!(v2.water_at(4, 3, 5), 0);
        assert_eq!(v2.water.iter().filter(|&&water| water > 0).count(), 1);
        assert_importer_water_invariant(&v2);
    }

    #[test]
    fn records_whose_flags_and_length_disagree_are_rejected() {
        let air = |_: usize, _: usize, _: usize| (AIR_MARKER, 0u8);
        let dry = |_: usize, _: usize, _: usize| 0u8;
        // A version 1 record cannot carry a plane, with or without its bytes.
        assert!(decode_voxel_chunk_raw(&with_plane(record(1, CHUNK_FLAG_WATER_PLANE, air), dry)).is_err());
        assert!(decode_voxel_chunk_raw(&record(1, CHUNK_FLAG_WATER_PLANE, air)).is_err());
        // The plane flag without the plane, and the plane without the flag.
        assert!(decode_voxel_chunk_raw(&record(2, CHUNK_FLAG_WATER_PLANE, air)).is_err());
        assert!(decode_voxel_chunk_raw(&with_plane(record(2, 0, air), dry)).is_err());
        // One byte short or long.
        let mut short = with_plane(record(2, CHUNK_FLAG_WATER_PLANE, air), dry);
        short.pop();
        assert!(decode_voxel_chunk_raw(&short).is_err());
        let mut long = record(1, 0, air);
        long.push(0);
        assert!(decode_voxel_chunk_raw(&long).is_err());
        // Versions outside the range this decoder reads.
        assert!(decode_voxel_chunk_raw(&record(0, 0, air)).is_err());
        assert!(decode_voxel_chunk_raw(&record(EUSTRESS_CHUNK_VERSION + 1, 0, air)).is_err());
        // The well-formed ones decode.
        assert!(decode_voxel_chunk_raw(&record(1, 0, air)).is_ok());
        assert!(decode_voxel_chunk_raw(&with_plane(record(2, CHUNK_FLAG_WATER_PLANE, air), dry)).is_ok());
    }

    #[test]
    fn fill_writes_top_surface_height_and_material() {
        // A 1-radius terrain (chunks_x = chunks_z = 1 → 3×3 chunks) of 4 m
        // cells.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        assert_eq!(config.height_scale, 1.0, "voxel heights are raw world heights");
        assert_eq!(config.height_offset, 0.0, "raw world heights need a zero offset");
        let mut data = TerrainData::default();

        // Chunk at voxel coords (0, 0, 0). One Grass column at local (0,0):
        // solid y=0..=3 → top cell y=3 → world top = (3+1)*4 = 16 m.
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
        assert_eq!(h, 16.0, "world-Y top of a 4-cell column is 16 m");

        // Material: Grass alone, its own slot.
        let px = off_z * cache_width + off_x;
        assert_eq!(data.material_cache[px], material_cell(GRASS), "grass column");
        assert!(data.material_dirty, "fill marks the material map dirty for GPU re-upload");
        // A column no chunk gave a surface holds no material.
        assert_eq!(data.material_cache[px + 5][0], MATERIAL_SLOT_NONE);
        assert!(data.has_material_layer());
    }

    #[test]
    fn fill_writes_each_surface_material_by_its_own_id() {
        // Materials that share a colour family stay distinct: Rock, Snow and
        // three that sit beside them (Basalt, CrackedLava, Water).
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let res = config.chunk_resolution as usize;
        let cache_width_chunks = config.chunks_x as usize;

        let mut data = TerrainData::default();
        let mut chunk = air_chunk();
        let materials = [
            ROCK,
            TerrainMaterial::Snow.to_u8(),
            TerrainMaterial::Basalt.to_u8(),
            TerrainMaterial::CrackedLava.to_u8(),
            TerrainMaterial::Water.to_u8(),
        ];
        for (x, &material) in materials.iter().enumerate() {
            set_solid(&mut chunk, x, 0, 0, material);
        }

        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);

        let cache_width = data.cache_width as usize;
        let off_x = cache_width_chunks * res;
        let off_z = cache_width_chunks * res;
        for (x, &material) in materials.iter().enumerate() {
            let cell = data.material_cache[off_z * cache_width + off_x + x];
            assert_eq!(cell, material_cell(material), "column {x} keeps material id {material}");
        }

        // A higher surface arriving later replaces the material with its own.
        let mut above = air_chunk();
        set_solid(&mut above, 0, 0, 0, TerrainMaterial::Glacier.to_u8());
        fill_terrain_from_chunk(&mut data, &config, 0, 1, 0, &above);
        assert_eq!(
            data.material_cache[off_z * cache_width + off_x],
            material_cell(TerrainMaterial::Glacier.to_u8())
        );
        assert_eq!(data.material_cache[off_z * cache_width + off_x + 1], material_cell(TerrainMaterial::Snow.to_u8()));
    }

    #[test]
    fn fill_raises_columns_in_a_higher_y_chunk() {
        // A voxel chunk at cy=1 sits one chunk (128 m of 4 m cells) higher: a
        // y=0 solid cell's world top is 128 + (0+1)*4 = 132 m.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
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

    /// Height of column (0,0) of chunk (0,0) in the global cache.
    fn column_height(data: &TerrainData, config: &TerrainConfig) -> f32 {
        let res = config.chunk_resolution as usize;
        let cache_width = data.cache_width as usize;
        let off_x = config.chunks_x as usize * res;
        let off_z = config.chunks_z as usize * res;
        data.height_cache[off_z * cache_width + off_x]
    }

    /// THE BUG: the heightfield is 2.5D but the voxel grid is 3D, so every
    /// chunk stacked at the same `(cx, cz)` addressed the same cache column.
    /// A chunk above the surface is mostly air and carries no height, so
    /// writing its tile unconditionally erased the real terrain to y=0.
    #[test]
    fn air_chunk_above_does_not_erase_the_surface_below() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut data = TerrainData::default();

        let mut ground = air_chunk();
        set_solid(&mut ground, 0, 0, 0, GRASS);
        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &ground);
        let before = column_height(&data, &config);
        assert_eq!(before, 4.0, "ground surface");

        // An ENTIRELY air chunk one layer up must contribute nothing.
        let empty = air_chunk();
        fill_terrain_from_chunk(&mut data, &config, 0, 1, 0, &empty);

        assert_eq!(
            column_height(&data, &config),
            before,
            "an air chunk above must not flatten the terrain below it"
        );
    }

    /// The topmost solid surface wins no matter which order chunks arrive in —
    /// the store iterates Morton order, not ascending Y.
    #[test]
    fn highest_surface_wins_regardless_of_arrival_order() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut low = air_chunk();
        set_solid(&mut low, 0, 0, 0, GRASS);
        let mut high = air_chunk();
        set_solid(&mut high, 0, 0, 0, GRASS);

        // low-then-high
        let mut a = TerrainData::default();
        fill_terrain_from_chunk(&mut a, &config, 0, 0, 0, &low);
        fill_terrain_from_chunk(&mut a, &config, 0, 1, 0, &high);

        // high-then-low
        let mut b = TerrainData::default();
        fill_terrain_from_chunk(&mut b, &config, 0, 1, 0, &high);
        fill_terrain_from_chunk(&mut b, &config, 0, 0, 0, &low);

        assert_eq!(column_height(&a, &config), 132.0, "higher chunk wins");
        assert_eq!(
            column_height(&a, &config),
            column_height(&b, &config),
            "result must not depend on chunk arrival order"
        );
    }

    /// Terrain below y=0 is ordinary (Roblox places sit at negative Y all the
    /// time). A combine that treated the zero-initialised cache as "lowest
    /// possible" would clamp these columns up to 0.
    #[test]
    fn negative_surface_heights_survive_the_combine() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut data = TerrainData::default();
        let mut chunk = air_chunk();
        set_solid(&mut chunk, 0, 0, 0, GRASS);

        // cy = -2 → base -256; a y=0 cell's top is -256 + 4 = -252.
        fill_terrain_from_chunk(&mut data, &config, 0, -2, 0, &chunk);

        assert_eq!(
            column_height(&data, &config),
            -252.0,
            "a genuinely negative height must not be clamped to 0"
        );
    }

    #[test]
    fn the_isosurface_sits_where_the_fill_crosses_one_half() {
        assert_eq!(isosurface_offset(1.0, 0.0), 1.0, "full over empty is the top face");
        assert!((isosurface_offset(1.0, 0.4) - 4.0 / 3.0).abs() < 1e-4);
        assert!((isosurface_offset(0.6, 0.0) - 2.0 / 3.0).abs() < 1e-4);
        // Rising with either fill, and within half a cell of the top face.
        let fills_top = [128.0 / 255.0, 0.6, 0.75, 0.9, 1.0];
        let fills_above = [0.0, 0.1, 0.25, 0.4, 127.0 / 255.0];
        for &f_top in &fills_top {
            for pair in fills_above.windows(2) {
                assert!(isosurface_offset(f_top, pair[0]) < isosurface_offset(f_top, pair[1]));
            }
            for &f_above in &fills_above {
                assert!((0.5..=1.5).contains(&isosurface_offset(f_top, f_above)));
            }
        }
        for &f_above in &fills_above {
            for pair in fills_top.windows(2) {
                assert!(isosurface_offset(pair[0], f_above) < isosurface_offset(pair[1], f_above));
            }
        }
        // Fills with no span between them give the top face, not a division
        // by zero.
        assert_eq!(isosurface_offset(0.5, 0.5), 1.0);
        assert_eq!(isosurface_offset(f32::NAN, 0.0), 1.0);
    }

    #[test]
    fn part_full_cells_place_the_surface_between_whole_cells() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut chunk = air_chunk();
        // Column (0, 0): full cells 0..=2 under a part-full cell that is not
        // solid (occupancy 100).
        for y in 0..=2 {
            set_solid(&mut chunk, 0, y, 0, GRASS);
        }
        set_cell(&mut chunk, (0, 3, 0), GRASS, 100, 0);
        // Column (1, 0): full cells 0..=1 under a solid but part-full top
        // cell (occupancy 200) with air above.
        for y in 0..=1 {
            set_solid(&mut chunk, 1, y, 0, GRASS);
        }
        set_cell(&mut chunk, (1, 2, 0), GRASS, 200, 0);
        let mut data = TerrainData::default();
        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);

        let top_face = 3.0 * ROBLOX_CELL_STUDS;
        let raised = data.height_cache[pixel(&config, &data, 0, 0)];
        assert!(raised > top_face, "a part-full cell above raises the surface ({raised}) past the top face");
        assert!((raised - (2.0 + isosurface_offset(1.0, 100.0 / 255.0)) * ROBLOX_CELL_STUDS).abs() < 1e-4);
        let lowered = data.height_cache[pixel(&config, &data, 1, 0)];
        assert!(lowered < top_face, "a part-full top cell lowers the surface ({lowered}) under its top face");
        assert!((lowered - (2.0 + isosurface_offset(200.0 / 255.0, 0.0)) * ROBLOX_CELL_STUDS).abs() < 1e-4);
    }

    #[test]
    fn a_part_full_cell_in_the_chunk_above_raises_a_surface_at_the_chunk_top() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        // Columns (0, 0) and (1, 0) solid up to the top layer of chunk cy 0.
        let mut below = air_chunk();
        for y in 0..CHUNK_EDGE {
            set_solid(&mut below, 0, y, 0, GRASS);
            set_solid(&mut below, 1, y, 0, GRASS);
        }
        // Chunk cy 1 holds a part-full cell over column (0, 0) alone.
        let mut above = air_chunk();
        set_cell(&mut above, (0, 0, 0), GRASS, 100, 0);

        let mut data = TerrainData::default();
        let mut columns = VoxelColumns::default();
        // The chunk above arrives first, as Morton order allows.
        for (cy, chunk) in [(1, &above), (0, &below)] {
            fill_terrain_from_chunk(&mut data, &config, 0, cy, 0, chunk);
            columns.record_chunk(0, cy, 0, chunk);
        }
        let top_face = CHUNK_EDGE as f32 * ROBLOX_CELL_STUDS;
        assert_eq!(
            data.height_cache[pixel(&config, &data, 0, 0)],
            top_face,
            "the fill places the surface as if the cell above were empty"
        );

        columns.refine_surface_tops(&mut data, &config);
        let raised = data.height_cache[pixel(&config, &data, 0, 0)];
        let expected = (31.0 + isosurface_offset(1.0, 100.0 / 255.0)) * ROBLOX_CELL_STUDS;
        assert!(raised > top_face, "the cell above raises the surface ({raised}) past the top face ({top_face})");
        assert!((raised - expected).abs() < 1e-4, "the surface is at {raised}, not {expected}");
        assert_eq!(data.height_cache[pixel(&config, &data, 1, 0)], top_face, "nothing above column (1, 0)");
    }

    #[test]
    fn holes_take_the_nearest_ground_in_their_row_or_the_nearest_row_with_some() {
        // A 5 x 3 raster: G is ground with its height, W a hole under water
        // at level 7, the rest holes.
        //   row 0:  G10  .   .  G20  .
        //   row 1:   .   .   W   .   .
        //   row 2:   .  G30  .   .   .
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut data = TerrainData {
            cache_width: 5,
            cache_height: 3,
            height_cache: vec![0.0; 15],
            material_cache: vec![[MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]; 15],
            ..TerrainData::default()
        };
        for (i, height) in [(0, 10.0), (3, 20.0), (11, 30.0)] {
            data.height_cache[i] = height;
            data.material_cache[i] = material_cell(GRASS);
        }
        let mut water = vec![f32::NAN; 15];
        water[7] = 7.0;
        fill_hole_heights(&config, &mut data, Some(&water[..]));
        // Row 1 has no ground: it copies row 0, the row before it on the tie
        // with row 2, and its water hole sits one 4 m cell under its level.
        let expected: Vec<f32> = vec![
            10.0, 10.0, 20.0, 20.0, 20.0,
            10.0, 10.0, 3.0, 20.0, 20.0,
            30.0, 30.0, 30.0, 30.0, 30.0,
        ];
        assert_eq!(data.height_cache, expected);
        assert_eq!(data.material_cache[1][0], MATERIAL_SLOT_NONE, "a hole keeps no material");
        assert_eq!(data.material_cache[0], material_cell(GRASS));
    }

    #[test]
    fn water_levels_sit_at_the_fill_of_each_columns_highest_water_cell() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        // Chunk (0, 0, 0): column (3, 4) holds full water in cells 0..=5
        // under a part-full cell 6; column (8, 8) has rock under a Shorelines
        // cell, part-full rock holding water.
        let mut sea = air_chunk();
        for y in 0..=5 {
            set_cell(&mut sea, (3, y, 4), WATER_MARKER, 255, 255);
        }
        set_cell(&mut sea, (3, 6, 4), WATER_MARKER, 128, 128);
        for y in 0..=2 {
            set_solid(&mut sea, 8, y, 8, ROCK);
        }
        set_cell(&mut sea, (8, 3, 8), ROCK, 100, 200);
        // Chunk (0, -1, 0): full water in the top cell of columns (3, 4) and
        // (5, 5).
        let mut deep = air_chunk();
        set_cell(&mut deep, (3, 31, 4), WATER_MARKER, 255, 255);
        set_cell(&mut deep, (5, 31, 5), WATER_MARKER, 255, 255);

        let mut data = TerrainData::default();
        let mut columns = VoxelColumns::default();
        for (cy, chunk) in [(0, &sea), (-1, &deep)] {
            fill_terrain_from_chunk(&mut data, &config, 0, cy, 0, chunk);
            columns.record_chunk(0, cy, 0, chunk);
        }
        assert!(columns.has_water());
        let levels = voxel_water_levels(&config, &data, &columns);
        assert_eq!(levels.len(), data.height_cache.len(), "one level per raster cell");
        let level = |x, z| levels[pixel(&config, &data, x, z)];
        assert!((level(3, 4) - (6.0 + 128.0 / 255.0) * ROBLOX_CELL_STUDS).abs() < 1e-4, "the highest water cell wins");
        assert!((level(8, 8) - (3.0 + 200.0 / 255.0) * ROBLOX_CELL_STUDS).abs() < 1e-4, "a Shorelines cell holds water");
        assert_eq!(level(5, 5), 0.0, "a full water cell's surface is its top face");
        assert!(level(0, 0).is_nan(), "a dry column has no level");
        assert_eq!(levels.iter().filter(|level| level.is_finite()).count(), 3);

        let mut dry = VoxelColumns::default();
        dry.record_chunk(0, 0, 0, &air_chunk());
        assert!(!dry.has_water());
    }

    #[test]
    fn an_off_centre_grid_holds_its_centre_chunk_in_the_middle_of_the_raster() {
        let center = IVec2::new(40, -10);
        let config = voxel_terrain_config(center, 1, 1, ROBLOX_CELL_STUDS);
        assert!(config.contains_chunk(center));
        let mut chunk = air_chunk();
        set_solid(&mut chunk, 0, 0, 0, GRASS);
        let mut data = TerrainData::default();
        assert_eq!(fill_terrain_from_chunk(&mut data, &config, center.x, 0, center.y, &chunk), center);

        // 3 x 3 chunks of 32 columns: the middle one starts at column and
        // row 32.
        let width = data.cache_width as usize;
        assert_eq!(width, 3 * CHUNK_EDGE);
        let middle = CHUNK_EDGE * width + CHUNK_EDGE;
        assert_eq!(data.height_cache[middle], ROBLOX_CELL_STUDS);
        assert_eq!(data.material_cache[middle], material_cell(GRASS));

        // A chunk off the grid (the origin's) writes nothing.
        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);
        assert_eq!(data.material_cache.iter().filter(|cell| cell[0] != MATERIAL_SLOT_NONE).count(), 1);

        // The lattice column at the chunk's first voxel column shows it.
        let raster = RasterColumns::new(&config, &data).expect("a raster");
        let first = center * CHUNK_EDGE as i32;
        assert_eq!(raster.voxel_column(first), Some(first));
    }

    #[test]
    fn an_import_in_feet_lays_its_cells_out_at_four_feet() {
        use crate::units::Unit;
        let config = voxel_terrain_config(IVec2::ZERO, 2, 2, ROBLOX_CELL_STUDS * Unit::Foot.to_meters() as f32);
        assert!((config.chunk_size - 39.0144).abs() < 1e-4, "chunk size {}", config.chunk_size);
        assert!((voxel_cell_size(&config) - 1.2192).abs() < 1e-5);
        // The lattice pairs with the voxel cell in any unit, so caves carve.
        assert!((lattice_cell_size(&config) - voxel_cell_size(&config)).abs() < 1e-6);
        // Each LOD band and the view distance span as many chunks as in
        // metres.
        let metres = voxel_terrain_config(IVec2::ZERO, 2, 2, ROBLOX_CELL_STUDS);
        for (feet, metre) in config.lod_distances.iter().zip(&metres.lod_distances) {
            assert!((feet / config.chunk_size - metre / metres.chunk_size).abs() < 1e-4);
        }
        assert!((config.view_distance / config.chunk_size - metres.view_distance / metres.chunk_size).abs() < 1e-4);

        // A column solid up to local y 9, air above, in chunk cy 0: its
        // surface is 10 cells of 1.2192 m up.
        let mut chunk = air_chunk();
        for y in 0..=9 {
            set_solid(&mut chunk, 0, y, 0, GRASS);
        }
        let mut data = TerrainData::default();
        fill_terrain_from_chunk(&mut data, &config, 0, 0, 0, &chunk);
        let height = data.height_cache[pixel(&config, &data, 0, 0)];
        assert!((height - 10.0 * 1.2192).abs() < 1e-4, "the surface is at {height} m");
    }

    /// A Part and the terrain under it are authored in one unit, so once each
    /// is converted a Part at stud X 1280 stands on the first column of voxel
    /// chunk 10 (1280 studs = 10 chunks of 128).
    #[test]
    fn a_part_and_the_terrain_column_under_it_share_one_world_position() {
        use crate::units::{authored_to_engine_f32, Unit};
        let chunk = IVec2::new(10, 0);
        let config = voxel_terrain_config(chunk, 1, 1, ROBLOX_CELL_STUDS * Unit::Foot.to_meters() as f32);
        assert!(config.contains_chunk(chunk));
        let part_x = authored_to_engine_f32(1280.0, Unit::Foot);
        let column_x = crate::terrain::chunk_world_position(chunk, &config).x;
        assert!((part_x - column_x).abs() < 1e-4, "the Part is at {part_x} m, the terrain column at {column_x} m");
        // The volume lattice puts voxel column 320 at the same place, and the
        // raster reads the chunk's first column there.
        let lattice_x = (chunk.x * CHUNK_EDGE as i32) as f32 * voxel_cell_size(&config);
        assert!((part_x - lattice_x).abs() < 1e-4, "the lattice column is at {lattice_x} m");
        let (part_u, column_u) = (config.world_to_uv(part_x, 0.0).x, config.chunk_point_uv(chunk, 0.0, 0.0).x);
        assert!((part_u - column_u).abs() < 1e-5, "the Part is at u {part_u}, the column at u {column_u}");
    }

    // -- Caves ----------------------------------------------------------------

    /// The terrain field at global lattice point `(x, y, z)`.
    fn field_at(
        config: &TerrainConfig,
        data: &TerrainData,
        volume: &TerrainVolume,
        x: i32,
        y: i32,
        z: i32,
    ) -> crate::terrain::FieldSample {
        crate::terrain::sample_field_lattice(config, data, volume, IVec3::new(x, y, z))
    }

    /// Solid `material` in cells `ys` of columns `columns` on both axes.
    fn fill_box(chunk: &mut DecodedChunk, columns: std::ops::RangeInclusive<usize>, ys: std::ops::RangeInclusive<usize>, material: u8) {
        for z in columns.clone() {
            for x in columns.clone() {
                for y in ys.clone() {
                    set_solid(chunk, x, y, z, material);
                }
            }
        }
    }

    /// Fill, record and carve an import of `chunks` the way the engine
    /// loader runs it: every chunk into the heightfield first, then the
    /// surfaces over chunk seams, the water and the holes, then the chunks
    /// `cave_chunks` names into the grid, then the carve.
    fn import(config: &TerrainConfig, chunks: &[(IVec3, DecodedChunk)]) -> (TerrainData, TerrainVolume, VoxelCaveReport) {
        let mut data = TerrainData::default();
        let mut columns = VoxelColumns::default();
        for (coord, chunk) in chunks {
            fill_terrain_from_chunk(&mut data, config, coord.x, coord.y, coord.z, chunk);
            columns.record_chunk(coord.x, coord.y, coord.z, chunk);
        }
        columns.refine_surface_tops(&mut data, config);
        let water = voxel_water_levels(config, &data, &columns);
        fill_hole_heights(config, &mut data, Some(&water[..]));
        let wanted = columns.cave_chunks();
        let mut grid = VoxelGrid::default();
        for (coord, chunk) in chunks {
            if wanted.contains(coord) {
                grid.insert(*coord, chunk.clone());
            }
        }
        let mut volume = TerrainVolume::default();
        let report = carve_voxel_caves(config, &data, &columns, &grid, &mut volume);
        (data, volume, report)
    }

    #[test]
    fn the_voxel_lattice_cell_is_the_roblox_cell() {
        // One lattice point per voxel cell on every axis: the carve needs no
        // resampling in size, only the raster offset `RasterColumns` handles.
        for radius in [1, 4, 64] {
            let config = voxel_terrain_config(IVec2::ZERO, radius, radius, ROBLOX_CELL_STUDS);
            assert_eq!(lattice_cell_size(&config), ROBLOX_CELL_STUDS);
            assert_eq!(lattice_cell_size(&config), voxel_cell_size(&config));
            assert_eq!(config.resolution_for_lod(0) as usize, CHUNK_EDGE);
        }
    }

    #[test]
    fn a_two_span_column_carves_the_gap_between_its_spans() {
        // Rock ground in cells 0..=2 everywhere, and a grass slab in cells
        // 8..=10 over columns 2..=12, leaving the gap 3..=7 under it.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 0..=CHUNK_EDGE - 1, 0..=2, ROCK);
        fill_box(&mut chunk, 2..=12, 8..=10, GRASS);
        let (data, volume, report) = import(&config, &[(IVec3::ZERO, chunk)]);
        assert!(report.carved_points > 0 && report.bricks > 0, "{report:?}");

        // Lattice column (7, 7) shows voxel column (7, 7). The heightfield
        // alone is solid right up to the slab's top face at 11 cells.
        let heightfield_only = TerrainVolume::default();
        for y in 0..=10 {
            assert!(field_at(&config, &data, &heightfield_only, 7, y, 7).is_solid());
        }
        for y in 3..=7 {
            assert!(!field_at(&config, &data, &volume, 7, y, 7).is_solid(), "gap cell {y} is still solid");
        }
        for y in [0, 1, 2, 8, 9, 10] {
            assert!(field_at(&config, &data, &volume, 7, y, 7).is_solid(), "span cell {y} was carved");
        }
        // The top surface is still exactly the heightfield's.
        assert_eq!(field_at(&config, &data, &volume, 7, 11, 7).value, 0.0);
        assert!(!field_at(&config, &data, &volume, 7, 12, 7).is_solid());

        // The roof lands halfway between a carved and a solid sample, at 7.5
        // cells, half a cell under the Roblox face at 8 by the minimum-corner
        // sampling rule. The floor under the open gap is lifted to 16/17 of
        // its edge, a seventeenth of a cell under its face at 3.
        let wall = |below: i32| {
            let a = field_at(&config, &data, &volume, 7, below, 7).value;
            let b = field_at(&config, &data, &volume, 7, below + 1, 7).value;
            below as f32 + a / (a - b)
        };
        assert!((wall(2) - (2.0 + 16.0 / 17.0)).abs() < 1e-3, "floor at {}", wall(2));
        assert!((wall(7) - 7.5).abs() < 1e-3, "roof at {}", wall(7));

        // Carved points take the nearest solid cell's material: rock over
        // the floor, grass under the roof.
        assert_eq!(volume.cell_at_lattice(IVec3::new(7, 3, 7)).material, ROCK);
        assert_eq!(volume.cell_at_lattice(IVec3::new(7, 7, 7)).material, GRASS);

        // Away from the overhang nothing is stored.
        assert!(!volume.cell_at_lattice(IVec3::new(24, 2, 24)).is_edited());
    }

    #[test]
    fn open_ground_beside_a_cave_mouth_keeps_the_heightfield_colour() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 0..=CHUNK_EDGE - 1, 0..=2, ROCK);
        fill_box(&mut chunk, 2..=12, 8..=10, GRASS);
        let (data, volume, _) = import(&config, &[(IVec3::ZERO, chunk)]);
        // Lattice (1, 3, 7) is the open ground's surface point beside the gap
        // under the slab: written, yet the heightfield still wins it, so the
        // ground vertex there takes the material-map colour, not the carve's.
        assert!(volume.cell_at_lattice(IVec3::new(1, 3, 7)).is_edited());
        let point = field_at(&config, &data, &volume, 1, 3, 7);
        assert_eq!(point.heightfield, 0.0);
        assert_eq!(point.value, 0.0);
        assert_eq!(point.term(), crate::terrain::FieldTerm::Heightfield);
    }

    #[test]
    fn a_floor_under_an_overhang_sits_flush_with_the_open_ground_beside_it() {
        // Rock ground in cells 0..=2 everywhere (its top face at 3 cells) and
        // a grass roof in cells 6..=7 over columns 4..=12, leaving a gap
        // three cells tall under it.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let cell = lattice_cell_size(&config);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 0..=CHUNK_EDGE - 1, 0..=2, ROCK);
        fill_box(&mut chunk, 4..=12, 6..=7, GRASS);
        let (data, volume, _) = import(&config, &[(IVec3::ZERO, chunk)]);

        // The floor under the roof, where the field crosses zero on the
        // vertical lattice edge from layer 2 to layer 3 of column (8, 8).
        let (a, b) = (field_at(&config, &data, &volume, 8, 2, 8).value, field_at(&config, &data, &volume, 8, 3, 8).value);
        assert!(a < 0.0 && b > 0.0, "the floor is between layers 2 and 3 ({a}, {b})");
        let floor = (2.0 + a / (a - b)) * cell;
        // Open ground beside the overhang. Column 14 is the nearest one whose
        // raster neighbours are both open ground, so the heightfield draws it
        // at the ground's top face rather than blended toward the roof.
        let ground = lattice_surface_height(&config, &data, 14, 8);
        assert!((ground - 3.0 * cell).abs() < 1e-3, "the open ground is at its top face, {ground}");
        assert!((floor - ground).abs() < 0.1 * cell, "the floor at {floor} is not flush with the ground at {ground}");
        assert_eq!(volume.cell_at_lattice(IVec3::new(8, 3, 8)).carve, LIFTED_FLOOR_STEP);
    }

    #[test]
    fn a_one_wide_tunnel_keeps_its_half_cell_floor_and_its_width() {
        // Solid rock in cells 0..=10 everywhere except a tunnel one column
        // wide (x = 8) and two cells tall (3..=4) running along z 2..=12.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let cell = lattice_cell_size(&config);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 0..=CHUNK_EDGE - 1, 0..=10, ROCK);
        for z in 2..=12 {
            for y in 3..=4 {
                let i = DecodedChunk::index(8, y, z);
                chunk.material[i] = AIR_MARKER;
                chunk.occupancy[i] = 0;
            }
        }
        let (data, volume, report) = import(&config, &[(IVec3::ZERO, chunk)]);
        assert!(report.carved_points > 0, "{report:?}");

        // The floor point beside the walls keeps carve_step's half cell.
        let half_cell = carve_step(AIR_MARKER, 0, cell);
        assert_eq!(half_cell, -16);
        assert_eq!(volume.cell_at_lattice(IVec3::new(8, 3, 7)).carve, half_cell);
        // At the floor layer the tunnel is exactly one lattice point wide.
        let open: Vec<i32> = (4..=12).filter(|&x| !field_at(&config, &data, &volume, x, 3, 7).is_solid()).collect();
        assert_eq!(open, vec![8]);
    }

    #[test]
    fn the_carve_opens_the_gap_and_leaves_the_rest_of_the_field_alone() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 0..=CHUNK_EDGE - 1, 0..=2, ROCK);
        fill_box(&mut chunk, 2..=12, 8..=10, GRASS);
        let (data, volume, _) = import(&config, &[(IVec3::ZERO, chunk)]);
        let plain = TerrainVolume::default();
        // Lattice columns 0..=16 show voxel columns 0..=16 (the left half of
        // this raster), so the gap cells are exactly these lattice points.
        let under_slab = |c: i32| (2..=12).contains(&c);
        for z in 0..=16 {
            for x in 0..=16 {
                for y in 0..=14 {
                    let with = field_at(&config, &data, &volume, x, y, z);
                    let without = field_at(&config, &data, &plain, x, y, z);
                    if under_slab(x) && under_slab(z) && (3..=7).contains(&y) {
                        assert!(!with.is_solid(), "gap point ({x}, {y}, {z}) is still solid");
                        continue;
                    }
                    assert_eq!(with.is_solid(), without.is_solid(), "point ({x}, {y}, {z}) changed sides");
                    if !(under_slab(x) && under_slab(z)) && y >= 3 {
                        // Above ground-only columns the heightfield decides
                        // alone, however close the cave is.
                        assert_eq!(with.value, without.value, "point ({x}, {y}, {z}) moved");
                    }
                }
            }
        }
    }

    #[test]
    fn air_under_a_columns_lowest_solid_cell_is_not_carved() {
        // A slab floating over nothing has no open cell between its lowest
        // and highest solid cell, so nothing is carved and the heightfield
        // keeps reading it as solid all the way down.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut chunk = air_chunk();
        fill_box(&mut chunk, 2..=12, 8..=10, GRASS);
        let (data, volume, report) = import(&config, &[(IVec3::ZERO, chunk)]);
        assert_eq!(report, VoxelCaveReport::default());
        assert!(volume.is_empty());
        assert!(field_at(&config, &data, &volume, 7, 3, 7).is_solid());
    }

    #[test]
    fn a_gap_across_a_chunk_boundary_is_carved_on_both_sides() {
        // Ground in cells 0..=29 of the chunk at cy 0 and a roof in cells
        // 4..=6 of the chunk above it (global 36..=38): the gap 30..=35
        // crosses global cell 32. The roof chunk arrives first.
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut ground = air_chunk();
        fill_box(&mut ground, 0..=CHUNK_EDGE - 1, 0..=29, ROCK);
        let mut roof = air_chunk();
        fill_box(&mut roof, 2..=12, 4..=6, GRASS);
        let (data, volume, _) = import(&config, &[(IVec3::new(0, 1, 0), roof), (IVec3::ZERO, ground)]);
        for y in 30..=35 {
            assert!(!field_at(&config, &data, &volume, 7, y, 7).is_solid(), "gap cell {y} is still solid");
        }
        for y in [28, 29, 36, 37, 38] {
            assert!(field_at(&config, &data, &volume, 7, y, 7).is_solid(), "span cell {y} was carved");
        }
    }

    #[test]
    fn column_extents_combine_stacked_chunks_in_any_order() {
        let mut low = air_chunk();
        for y in 0..=3 {
            set_solid(&mut low, 1, y, 1, ROCK);
        }
        let mut high = air_chunk();
        set_solid(&mut high, 1, 5, 1, GRASS);
        for order in [[(0, &low), (1, &high)], [(1, &high), (0, &low)]] {
            let mut columns = VoxelColumns::default();
            for (cy, chunk) in order {
                columns.record_chunk(-1, cy, 2, chunk);
            }
            // Chunk (-1, 2), local column (1, 1).
            let extent = columns.extent(IVec2::new(-31, 65)).expect("the column has solid cells");
            assert_eq!(extent, ColumnExtent { lowest: 0, highest: 37, top_occupancy: 255, solid_cells: 5 });
            assert!(extent.has_gap());
            assert!(columns.extent(IVec2::new(-32, 64)).is_none(), "an all-air column has no extent");
            let wanted = columns.cave_chunks();
            assert!(wanted.contains(&IVec3::new(-1, 0, 2)) && wanted.contains(&IVec3::new(-1, 1, 2)));
        }
    }

    #[test]
    fn lattice_columns_show_the_voxel_column_sample_height_draws() {
        let config = voxel_terrain_config(IVec2::ZERO, 1, 1, ROBLOX_CELL_STUDS);
        let mut data = TerrainData::default();
        data.resize_cache(&config);
        let width = data.cache_width as i32;
        let offset = (config.chunks_x * config.chunk_resolution) as i32;
        // Every raster column holds its own index, so `sample_height` reads
        // back the fractional column it samples.
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = (i % width as usize) as f32;
        }
        let raster = RasterColumns::new(&config, &data).unwrap();
        let mut previous: Option<i32> = None;
        for n in -offset..=width - offset {
            let column = raster.voxel_column(IVec2::new(n, -offset)).unwrap().x;
            let drawn = data.sample_height((n + offset) as f32 / width as f32, 0.0) - offset as f32;
            assert!(
                (column as f32 - drawn).abs() <= 0.5 + 1e-4,
                "lattice column {n} takes voxel column {column}, the heightfield draws {drawn}"
            );
            if let Some(previous) = previous {
                assert!(column == previous || column == previous + 1, "column {column} after {previous}");
            }
            previous = Some(column);
        }
        assert_eq!(previous, Some(width - offset - 1), "the last lattice column shows the last voxel column");
        assert!(raster.voxel_column(IVec2::new(-offset - 1, 0)).is_none());
        assert!(raster.voxel_column(IVec2::new(width - offset + 1, 0)).is_none());
    }

    #[test]
    fn carve_steps_follow_the_occupancy_formula_without_flipping_solidity() {
        let cell = ROBLOX_CELL_STUDS;
        assert_eq!(carve_step(ROCK, 255, cell), 16, "full: half a cell of solid");
        assert_eq!(carve_step(AIR_MARKER, 0, cell), -16, "air: half a cell carved");
        assert_eq!(carve_step(WATER_MARKER, 255, cell), -16, "lifted water is open");
        // Either side of half full rounds to 0; solidity picks the side.
        assert_eq!(carve_step(ROCK, SOLID_OCCUPANCY_THRESHOLD + 1, cell), 1);
        assert_eq!(carve_step(ROCK, SOLID_OCCUPANCY_THRESHOLD, cell), 0);
    }
}
