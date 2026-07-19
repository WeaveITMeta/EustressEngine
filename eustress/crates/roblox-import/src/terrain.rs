//! Roblox `Terrain.SmoothGrid` voxel decode → Eustress voxel chunks.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §6.
//!
//! ## What this module does
//!
//! Roblox stores terrain as a single `Terrain` instance child of
//! `Workspace`, carrying a `SmoothGrid` `BinaryString` (the volumetric
//! voxel grid) plus a `MaterialColors` table and a handful of scalar
//! properties (`WaterColor`, `WaterTransparency`, `WaterWaveSize`, …).
//!
//! [`decode_smooth_grid`] turns the `SmoothGrid` byte blob into a list of
//! 32³ [`VoxelChunk`]s. [`import_terrain`] writes each chunk to
//! `<space>/Workspace/Terrain/voxel_chunks/chunk_<cx>_<cy>_<cz>.bin`
//! (LZ4-compressed per spec §6.6), records the `[material_colors]` table
//! and global terrain props onto `Workspace/Terrain/_instance.toml`, and
//! returns counts for the [`ImportReport`].
//!
//! ## The SmoothGrid binary format
//!
//! ```text
//! SmoothGrid := u8 version (== 1)
//!            || u8 grid_kind (0x05 = log2 of the 32-cell chunk edge)
//!            || ChunkRecord*            (until end of buffer)
//!
//! ChunkRecord := i32 dx || i32 dy || i32 dz   (chunk coord, RELATIVE to the
//!                                              previous chunk, first from 0;
//!                                              byte-plane interleaved,
//!                                              big-endian — read_chunk_delta)
//!             || Cell* (run-length encoded; exactly 32^3 cells decoded)
//!
//! Cell (RLE):
//!   lead byte:  bits 0..=5  material id (0 = Air)
//!               bit  6      occupancy-present flag
//!               bit  7      count-present flag
//!   if occupancy-present:  u8 solid_occupancy (else 255 for solid, 0 for air)
//!   if count-present:      u8 count
//!       count > 0  → a run of (count + 1) voxels
//!       count == 0 → the Shorelines water escape: ONE voxel, followed by a
//!                    u8 water_occupancy byte (the voxel also holds water)
//! ```
//!
//! ### Provenance
//!
//! The **cell encoding**, 32³ chunk size, and hash-map storage model come from
//! Roblox engineer Arseny Kapoulkine
//! (<https://zeux.io/2017/03/27/voxel-terrain-storage/>). The **chunk framing**
//! — the interleaved-i32 delta header and the count-0 water escape — matches
//! the reference decoder in rbx-dom PR #444 ("Initial support for Terrain
//! SmoothGrid data"), and was independently reproduced here: decoding real
//! places yields chunk coordinates that are 100% unique, lexicographically
//! sorted (a serialised sorted chunk map), and land every cell stream on
//! exactly 32768 cells. Mountain Ascension decodes to ~3450 chunks consuming
//! the 11.8 MB payload to the byte.
//!
//! Occupancy is a quantised fraction (`byte / 255`). We keep the raw `u8` on
//! disk; the Eustress mesher applies its own occupancy rule.
//!
//! Cells within a chunk are iterated **Y outer, Z middle, X inner** so the
//! linear index is `y*1024 + z*32 + x`. Runs commonly tile 32-cell columns
//! (e.g. `80 08 88 16` = 9 air then 23 rock — a vertical column).
//!
//! ### Water
//!
//! Roblox Shorelines lets a voxel hold both a solid material and water. The
//! decoder reads the water-occupancy byte (so framing stays aligned) but does
//! not yet store it as a separate channel — the solid material and occupancy
//! are preserved faithfully; the water overlay is dropped for now.
//!
//! ### Defensive decoding
//!
//! Every read is bounds-checked; a chunk whose RLE stream would overrun is
//! reported as a [`TerrainDecodeError`] and decoding stops rather than
//! emitting garbage from a misaligned stream — it never reads out of bounds,
//! and it never invents voxels it cannot prove.

use std::path::Path;

use rbx_dom_weak::types::{MaterialColors, TerrainMaterials, Variant};

use crate::import_report::{ImportReport, TerrainDecodeError, TerrainMaterialApproximation};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Cells along one edge of a SmoothGrid chunk. Roblox uses 32.
pub const CHUNK_EDGE: usize = 32;

/// Total cells in one 32³ chunk.
pub const CELLS_PER_CHUNK: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE; // 32768

/// SmoothGrid format version we understand.
pub const SMOOTH_GRID_VERSION: u8 = 1;

/// Bytes of per-chunk header preceding each chunk's cell stream: three `i32`
/// coordinate deltas in byte-plane-interleaved big-endian form (12 bytes).
/// See [`read_chunk_delta`].
pub const CHUNK_HEADER_LEN: usize = 12;

/// The Eustress voxel-chunk file format version (spec §6.6).
pub const EUSTRESS_CHUNK_VERSION: u8 = 1;

/// Roblox terrain cell edge in studs (= meters in Eustress, STUD_TO_METERS = 1).
pub const ROBLOX_CELL_STUDS: f32 = 4.0;

// ---------------------------------------------------------------------------
// Eustress terrain material id (mirrors common::terrain::TerrainMaterial)
// ---------------------------------------------------------------------------

/// The 8 Eustress terrain materials, kept in sync with
/// `eustress_common::terrain::material::TerrainMaterial` (we re-declare
/// the discriminants here so the importer stays bevy-free — the engine
/// crate is not a dependency).
///
/// | id | material |
/// |----|----------|
/// | 0  | Grass    |
/// | 1  | Rock     |
/// | 2  | Dirt     |
/// | 3  | Snow     |
/// | 4  | Sand     |
/// | 5  | Mud      |
/// | 6  | Concrete |
/// | 7  | Asphalt  |
pub mod eustress_material {
    // Discriminants MUST track `eustress_common::terrain::material::
    // TerrainMaterial`, which stored voxel data depends on. The first 8 are
    // frozen; 8..=21 were appended in Wave 9.E to give Eustress a
    // Roblox-matching palette. (`Water` = 22 there; this crate routes water
    // through `WATER_MARKER` instead, so it is deliberately absent below.)
    //
    // Mirrored rather than imported because this crate stays engine-free.

    // ── Original 8 — frozen ──
    /// Grass.
    pub const GRASS: u8 = 0;
    /// Rock.
    pub const ROCK: u8 = 1;
    /// Dirt.
    pub const DIRT: u8 = 2;
    /// Snow.
    pub const SNOW: u8 = 3;
    /// Sand.
    pub const SAND: u8 = 4;
    /// Mud.
    pub const MUD: u8 = 5;
    /// Concrete.
    pub const CONCRETE: u8 = 6;
    /// Asphalt.
    pub const ASPHALT: u8 = 7;
    // ── Wave 9.E additions — the Roblox-parity tail ──
    /// Slate.
    pub const SLATE: u8 = 8;
    /// Brick.
    pub const BRICK: u8 = 9;
    /// Wood planks.
    pub const WOOD_PLANKS: u8 = 10;
    /// Glacier.
    pub const GLACIER: u8 = 11;
    /// Sandstone.
    pub const SANDSTONE: u8 = 12;
    /// Basalt.
    pub const BASALT: u8 = 13;
    /// Ground.
    pub const GROUND: u8 = 14;
    /// Cracked lava.
    pub const CRACKED_LAVA: u8 = 15;
    /// Cobblestone.
    pub const COBBLESTONE: u8 = 16;
    /// Ice.
    pub const ICE: u8 = 17;
    /// Leafy grass.
    pub const LEAFY_GRASS: u8 = 18;
    /// Salt.
    pub const SALT: u8 = 19;
    /// Limestone.
    pub const LIMESTONE: u8 = 20;
    /// Pavement.
    pub const PAVEMENT: u8 = 21;
}

/// Sentinel Eustress material id for "this voxel is water" — water is
/// pulled out into a separate water layer (spec §6.5) but we still tag
/// the cell so the water-region extraction can find it.
pub const WATER_MARKER: u8 = 254;

/// Sentinel for "air / empty cell" inside a decoded [`VoxelChunk`].
pub const AIR_MARKER: u8 = 255;

// ---------------------------------------------------------------------------
// Roblox material id → name + Eustress mapping
// ---------------------------------------------------------------------------

/// A mapping result for one Roblox terrain material.
struct MaterialMap {
    /// Eustress destination id (or [`WATER_MARKER`] / [`AIR_MARKER`]).
    eustress_id: u8,
    /// Whether this was an inexact "closest match" worth logging.
    approximated: bool,
}

/// Single source-of-truth table for the Roblox terrain material id space,
/// driving BOTH the human-readable name and the Eustress destination so
/// the two never drift.
///
/// Each row is `(roblox_name, eustress_id, exact)`. The index in this
/// array is the Roblox cell material id. Air is index 0; Water is index
/// 1. The remaining ordering follows the `MaterialColors` serialization
/// order documented by `rbx_types` (the most authoritative public source
/// for terrain material ordering), shifted by the two leading
/// Air/Water slots.
///
/// **Validation.** Decoding a real place's cell stream and tallying the
/// material ids yields a distribution that matches the source map's
/// character exactly — Mountain Ascension comes out 45% `Rock`, 33% Air,
/// 6% `Slate`, 5% `Sandstone`, 4% `Mud`, 3% `Grass`, with a trace of
/// `Water`. A mis-ordered table could not produce a coherent mountain, so
/// the id space below is corroborated by evidence, not just inference.
///
/// Every id now maps EXACTLY: `TerrainMaterial` gained the Roblox-parity
/// tail (Slate/Brick/WoodPlanks/Glacier/Sandstone/Basalt/Ground/
/// CrackedLava/Cobblestone/Ice/LeafyGrass/Salt/Limestone/Pavement) in
/// Wave 9.E, so nothing needs collapsing into Rock/Snow/Concrete any more.
/// Unknown / newly-added Roblox ids still fall back to `Rock` (spec §6.8)
/// and are flagged as approximations.
const MATERIAL_TABLE: &[(&str, u8, bool)] = {
    use eustress_material::*;
    &[
        ("Air", AIR_MARKER, true),         // 0
        ("Water", WATER_MARKER, true),     // 1
        ("Grass", GRASS, true),            // 2
        ("Slate", SLATE, true),            // 3
        ("Concrete", CONCRETE, true),      // 4
        ("Brick", BRICK, true),            // 5
        ("Sand", SAND, true),              // 6
        ("WoodPlanks", WOOD_PLANKS, true), // 7
        ("Rock", ROCK, true),              // 8
        ("Glacier", GLACIER, true),        // 9
        ("Snow", SNOW, true),              // 10
        ("Sandstone", SANDSTONE, true),    // 11
        ("Mud", MUD, true),                // 12
        ("Basalt", BASALT, true),          // 13
        ("Ground", GROUND, true),          // 14
        ("CrackedLava", CRACKED_LAVA, true), // 15
        ("Asphalt", ASPHALT, true),        // 16
        ("Cobblestone", COBBLESTONE, true), // 17
        ("Ice", ICE, true),                // 18
        ("LeafyGrass", LEAFY_GRASS, true), // 19
        ("Salt", SALT, true),              // 20
        ("Limestone", LIMESTONE, true),    // 21
        ("Pavement", PAVEMENT, true),      // 22
    ]
};

/// Map a Roblox terrain material id (the byte stored in a SmoothGrid
/// cell) to a Eustress material id, plus whether the mapping is an
/// approximation worth surfacing.
fn map_roblox_material(roblox_id: u8) -> MaterialMap {
    let (eustress_id, exact) = MATERIAL_TABLE
        .get(roblox_id as usize)
        .map(|&(_, id, exact)| (id, exact))
        .unwrap_or((eustress_material::ROCK, false)); // unknown → Rock, flagged
    MaterialMap {
        eustress_id,
        approximated: !exact && eustress_id != AIR_MARKER && eustress_id != WATER_MARKER,
    }
}

/// Human-readable Roblox material name for a cell material id, used in
/// approximation reporting. Falls back to `Material<id>` for unknowns.
fn roblox_material_name(roblox_id: u8) -> String {
    MATERIAL_TABLE
        .get(roblox_id as usize)
        .map(|&(name, _, _)| name.to_string())
        .unwrap_or_else(|| format!("Material{roblox_id}"))
}

// ---------------------------------------------------------------------------
// Decoded voxel chunk
// ---------------------------------------------------------------------------

/// One decoded 32³ chunk of terrain voxels.
///
/// `material` and `occupancy` are parallel arrays in linear
/// `y*1024 + z*32 + x` order (Roblox iterates voxels Y-outer, Z-middle,
/// X-inner). `material[i]` is a **Roblox** material id (mapping to Eustress is
/// applied at write time so the [`TerrainMaterialApproximation`] tally is
/// accurate). `occupancy[i]` is the quantised `u8`.
#[derive(Debug, Clone)]
pub struct VoxelChunk {
    /// Chunk grid X coordinate.
    pub cx: i32,
    /// Chunk grid Y coordinate.
    pub cy: i32,
    /// Chunk grid Z coordinate.
    pub cz: i32,
    /// Per-cell Roblox material id (0 = Air), linear YZX order.
    pub material: Vec<u8>,
    /// Per-cell quantised occupancy (0..=255), linear YZX order.
    pub occupancy: Vec<u8>,
}

impl VoxelChunk {
    /// True when every cell is Air — such chunks are not written to disk.
    pub fn is_empty(&self) -> bool {
        self.material.iter().all(|&m| m == 0)
    }

    /// Linear index for `(x, y, z)` local cell coordinates. Roblox stores
    /// voxels ascending by Y, then Z, then X, so X is the innermost axis.
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        y * (CHUNK_EDGE * CHUNK_EDGE) + z * CHUNK_EDGE + x
    }
}

// ---------------------------------------------------------------------------
// Byte-cursor reader (bounds-checked, never panics)
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    #[inline]
    fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    #[inline]
    fn read_u8(&mut self) -> Option<u8> {
        let b = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    /// Borrow the next `n` bytes, advancing past them. `None` (leaving the
    /// cursor untouched) when fewer than `n` remain.
    #[inline]
    fn read_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }
}

// ---------------------------------------------------------------------------
// SmoothGrid decode
// ---------------------------------------------------------------------------

/// Outcome of a `SmoothGrid` decode call.
#[derive(Debug, Default)]
pub struct DecodeResult {
    /// All non-empty decoded chunks.
    pub chunks: Vec<VoxelChunk>,
    /// Per-chunk decode failures (bounds overruns, etc.).
    pub errors: Vec<TerrainDecodeError>,
}

/// Decode a `SmoothGrid` `BinaryString` payload into voxel chunks.
///
/// Never panics. On any malformed read it appends a
/// [`TerrainDecodeError`] and stops cleanly (returning whatever chunks
/// decoded so far). An empty / version-only buffer returns no chunks and
/// no errors.
pub fn decode_smooth_grid(buf: &[u8]) -> DecodeResult {
    let mut result = DecodeResult::default();

    // Empty terrain is the 2-byte `[0x01, 0x05]` marker (or shorter). Nothing
    // shorter than the file header + one chunk header can hold a chunk, so
    // treat it as empty rather than as a malformed grid.
    if buf.len() < 2 + CHUNK_HEADER_LEN {
        return result;
    }

    let mut cur = Cursor::new(buf);

    let version = match cur.read_u8() {
        Some(v) => v,
        None => return result,
    };
    if version != SMOOTH_GRID_VERSION {
        result.errors.push(TerrainDecodeError {
            cx: 0,
            cy: 0,
            cz: 0,
            reason: format!(
                "unsupported SmoothGrid version {version} (expected {SMOOTH_GRID_VERSION})"
            ),
        });
        return result;
    }

    // The file header is two bytes. Empty terrain serialises as exactly
    // `[0x01, 0x05]` (base64 `AQU=` — the value in every rbx_binary terrain
    // fixture), and a zero-chunk grid cannot contain a chunk record, so `0x05`
    // is structural rather than data. Its meaning is unconfirmed: log2(32) = 5
    // lines up with the 32-cell chunk edge, but that is inference, not fact.
    let _grid_kind = match cur.read_u8() {
        Some(v) => v,
        None => return result,
    };

    // Cap the chunk count so a corrupt buffer can never spin forever.
    // 1 km³ at 4-stud cells is ~480 chunks; a million-instance world is
    // far larger, so allow generously but finite.
    const MAX_CHUNKS: usize = 1_000_000;
    let mut chunk_count = 0usize;

    // Chunk coordinates are stored as DELTAS from the previous chunk, so the
    // running position is the decoder's state. The first chunk's delta is
    // measured from the origin.
    let (mut cx, mut cy, mut cz) = (0i32, 0i32, 0i32);

    while !cur.at_end() {
        chunk_count += 1;
        if chunk_count > MAX_CHUNKS {
            result.errors.push(TerrainDecodeError {
                cx: 0,
                cy: 0,
                cz: 0,
                reason: format!("chunk count exceeded {MAX_CHUNKS}; stopping decode"),
            });
            break;
        }

        // ── Chunk header: three i32 coordinate DELTAS, byte-plane
        //    interleaved (see `read_chunk_delta`). ──
        let hdr = match cur.read_bytes(CHUNK_HEADER_LEN) {
            Some(h) => h,
            None => {
                // Trailing partial header — not necessarily an error
                // (some writers pad). Only flag if there were leftover
                // bytes that looked like the start of a chunk.
                if cur.remaining() > 0 {
                    result.errors.push(TerrainDecodeError {
                        cx: 0,
                        cy: 0,
                        cz: 0,
                        reason: format!(
                            "truncated chunk header ({} trailing byte(s))",
                            cur.remaining()
                        ),
                    });
                }
                break;
            }
        };

        // Walk the sparse chunk list. The key is relative to the previous
        // chunk, so a raster run along +Z is a repeated `(0,0,1)` — which is
        // exactly why scanning for an ABSOLUTE coordinate field never found one.
        let (dx, dy, dz) = read_chunk_delta(hdr);
        cx += dx;
        cy += dy;
        cz += dz;

        // Sanity-bound the coordinates — a wildly out-of-range coord is a
        // sign we've lost framing. ±2^20 chunks = ±4 million studs, well
        // beyond any real place.
        const COORD_LIMIT: i32 = 1 << 20;
        if cx.abs() > COORD_LIMIT || cy.abs() > COORD_LIMIT || cz.abs() > COORD_LIMIT {
            result.errors.push(TerrainDecodeError {
                cx,
                cy,
                cz,
                reason: "implausible chunk coordinate — decode framing lost".to_string(),
            });
            break;
        }

        // ── Decode the RLE cell stream for this chunk. ──
        match decode_chunk_cells(&mut cur) {
            Ok((material, occupancy)) => {
                let chunk = VoxelChunk {
                    cx,
                    cy,
                    cz,
                    material,
                    occupancy,
                };
                if !chunk.is_empty() {
                    result.chunks.push(chunk);
                }
            }
            Err(reason) => {
                result
                    .errors
                    .push(TerrainDecodeError { cx, cy, cz, reason });
                // Framing is lost once a chunk overruns; stop rather than
                // emit garbage from misaligned reads.
                break;
            }
        }
    }

    result
}

/// Decode a 12-byte chunk header into its `(dx, dy, dz)` coordinate delta.
///
/// The three `i32`s are stored **byte-plane interleaved, big-endian** — the
/// same transform rbx-binary uses for integer arrays. For an array of three
/// values the bytes are grouped by significance: `[X₃ Y₃ Z₃][X₂ Y₂ Z₂]
/// [X₁ Y₁ Z₁][X₀ Y₀ Z₀]`, so each axis is `hdr[axis + 3*plane]` for
/// `plane = 0..4`, most-significant first. There is no zig-zag; the value is a
/// plain two's-complement `i32` (rbx-dom's `read_interleaved_i32_array`).
///
/// Small deltas leave the three high planes as pure sign extension, which is
/// why every real header looks like three identical 3-byte groups followed by
/// the one that differs — the low plane `[X₀ Y₀ Z₀]`.
fn read_chunk_delta(hdr: &[u8]) -> (i32, i32, i32) {
    let axis = |a: usize| i32::from_be_bytes([hdr[a], hdr[a + 3], hdr[a + 6], hdr[a + 9]]);
    (axis(0), axis(1), axis(2))
}

/// Decode exactly [`CELLS_PER_CHUNK`] cells of RLE data from `cur`.
///
/// Returns `(material, occupancy)` parallel arrays, or an `Err(reason)`
/// string on bounds overrun / framing loss. Never panics.
///
/// Grammar per record: a lead byte (`material` in bits 0..=5, bit 6 =
/// occupancy-present, bit 7 = count-present); then, if bit 6, a solid
/// occupancy byte; then, if bit 7, a count byte. The count byte carries the
/// **Shorelines water escape**: a count of `0` does not mean a run — it means
/// a single voxel that also holds water, and one more byte (the water
/// occupancy) follows. A non-zero count `c` is a run of `c + 1` voxels.
fn decode_chunk_cells(cur: &mut Cursor) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut material = Vec::with_capacity(CELLS_PER_CHUNK);
    let mut occupancy = Vec::with_capacity(CELLS_PER_CHUNK);

    while material.len() < CELLS_PER_CHUNK {
        let lead = cur
            .read_u8()
            .ok_or_else(|| format!("ran out of bytes after {} cells", material.len()))?;

        // bits 0..=5: material id; bit 6: occupancy present; bit 7: count present.
        let mat_id = lead & 0b0011_1111;
        let has_occupancy = (lead & 0b0100_0000) != 0;
        let has_count = (lead & 0b1000_0000) != 0;

        let occ = if has_occupancy {
            cur.read_u8()
                .ok_or_else(|| "ran out of bytes reading occupancy".to_string())?
        } else {
            // Default occupancy: solid materials fully occupied, air empty.
            if mat_id == 0 {
                0
            } else {
                255
            }
        };

        let run = if has_count {
            let count = cur
                .read_u8()
                .ok_or_else(|| "ran out of bytes reading count".to_string())?;
            if count == 0 {
                // Water escape: this voxel carries a separate water occupancy
                // (Roblox Shorelines). Consume that byte to hold framing. The
                // solid material + occupancy above stay authoritative for the
                // surface; the water overlay is not yet a separate channel in
                // the Eustress voxel record, so it is read and dropped here.
                let _water_occupancy = cur.read_u8().ok_or_else(|| {
                    "ran out of bytes reading water occupancy (count-0 escape)".to_string()
                })?;
                1
            } else {
                count as usize + 1
            }
        } else {
            1
        };

        for _ in 0..run {
            if material.len() >= CELLS_PER_CHUNK {
                return Err(format!(
                    "run length overruns chunk ({} > {CELLS_PER_CHUNK} cells)",
                    material.len() + 1
                ));
            }
            material.push(mat_id);
            occupancy.push(occ);
        }
    }

    Ok((material, occupancy))
}

// ---------------------------------------------------------------------------
// Eustress chunk-file encode (spec §6.6)
// ---------------------------------------------------------------------------

/// Encode one decoded chunk into the Eustress on-disk binary record
/// (spec §6.6), applying the Roblox→Eustress material mapping and
/// accumulating approximation counts.
///
/// Layout (before LZ4):
/// ```text
/// u8  version (== EUSTRESS_CHUNK_VERSION)
/// u8  material_count (informational; <= 8 Eustress materials + markers)
/// u8  flags (bit0 = contains water marker)
/// [ for each of CELLS_PER_CHUNK cells, YZX order (X inner): ]
///     u8 eustress_material_id  (255 = Air, 254 = Water)
///     u8 occupancy_q           (0..=255)
/// ```
fn encode_eustress_chunk(
    chunk: &VoxelChunk,
    material_tally: &mut std::collections::HashMap<u8, (String, usize)>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + CELLS_PER_CHUNK * 2);
    let mut distinct: std::collections::HashSet<u8> = std::collections::HashSet::new();
    let mut has_water = false;

    // Build the cell payload first so we can compute material_count.
    let mut cells = Vec::with_capacity(CELLS_PER_CHUNK * 2);
    for i in 0..CELLS_PER_CHUNK {
        let rbx_mat = *chunk.material.get(i).unwrap_or(&0);
        let occ = *chunk.occupancy.get(i).unwrap_or(&0);
        let mapped = map_roblox_material(rbx_mat);
        if mapped.eustress_id == WATER_MARKER {
            has_water = true;
        }
        if mapped.eustress_id != AIR_MARKER {
            distinct.insert(mapped.eustress_id);
        }
        if mapped.approximated {
            let entry = material_tally
                .entry(rbx_mat)
                .or_insert_with(|| (roblox_material_name(rbx_mat), 0));
            entry.1 += 1;
        }
        cells.push(mapped.eustress_id);
        cells.push(occ);
    }

    out.push(EUSTRESS_CHUNK_VERSION);
    out.push(distinct.len().min(255) as u8);
    out.push(if has_water { 0b0000_0001 } else { 0 });
    out.extend_from_slice(&cells);
    out
}

// ---------------------------------------------------------------------------
// Public entry point — import_terrain
// ---------------------------------------------------------------------------

/// Properties lifted off the Roblox `Terrain` instance that we want to
/// preserve on `Workspace/Terrain/_instance.toml`.
#[derive(Debug, Default, Clone)]
pub struct TerrainGlobals {
    /// `WaterColor` (Color3) as `[r, g, b]` in 0..1.
    pub water_color: Option<[f32; 3]>,
    /// `WaterTransparency` (Float32).
    pub water_transparency: Option<f32>,
    /// `WaterWaveSize` (Float32).
    pub water_wave_size: Option<f32>,
    /// `WaterWaveSpeed` (Float32).
    pub water_wave_speed: Option<f32>,
    /// `WaterReflectance` (Float32).
    pub water_reflectance: Option<f32>,
}

/// Decode + write a Roblox `Terrain` instance's voxel data into the
/// Eustress Space at `terrain_dir` (which is
/// `<space>/Workspace/Terrain/`).
///
/// - Writes `voxel_chunks/chunk_<cx>_<cy>_<cz>.bin` (LZ4) per non-empty chunk.
/// - Writes/extends `<terrain_dir>/_instance.toml` with the
///   `[material_colors]` table, `[terrain]` source flip, and global
///   water props.
/// - Updates `report.terrain_chunks_imported`,
///   `report.terrain_material_approximations`, and
///   `report.terrain_decode_errors`.
///
/// Returns the number of chunks written. Pure file I/O — no Bevy.
pub fn import_terrain(
    terrain_dir: &Path,
    smooth_grid: &[u8],
    material_colors: Option<&MaterialColors>,
    globals: &TerrainGlobals,
    report: &mut ImportReport,
) -> std::io::Result<usize> {
    let decoded = decode_smooth_grid(smooth_grid);

    // A decode error means the stream desynced and we stopped early, so
    // whatever decoded is a PREFIX of the real terrain, not all of it. That
    // distinction drives both the sidecar and the `source` label below.
    let decode_incomplete = !decoded.errors.is_empty();

    // Surface decode errors first (graceful degradation — spec §6.8).
    for err in decoded.errors {
        report.terrain_decode_errors.push(err);
    }

    // Keep the raw bytes whenever the decode did not complete — total failure
    // AND partial failure both need them. Persisting the payload next to the
    // Terrain instance lets a future decoder be developed and re-run against
    // real data without re-importing from the .rbxl. (The importer does not
    // hex-dump `SmoothGrid` into `[properties.extras]`; that inflated one
    // Terrain `_instance.toml` to 27 MB for 11.8 MB of voxels. This sidecar is
    // the compact, canonical home for it.)
    if (decode_incomplete || decoded.chunks.is_empty()) && smooth_grid.len() > 2 {
        std::fs::create_dir_all(terrain_dir)?;
        let raw_path = terrain_dir.join("smooth_grid.raw");
        std::fs::write(&raw_path, smooth_grid)?;
        report.terrain_decode_errors.push(TerrainDecodeError {
            cx: 0,
            cy: 0,
            cz: 0,
            reason: format!(
                "SmoothGrid decode INCOMPLETE — recovered {} of an unknown total from \
                 {} bytes; this terrain is a fragment, not the whole grid. Raw payload \
                 preserved at {}",
                decoded.chunks.len(),
                smooth_grid.len(),
                raw_path.display()
            ),
        });
    }

    // Write chunk files.
    let chunks_dir = terrain_dir.join("voxel_chunks");
    let mut written = 0usize;
    let mut material_tally: std::collections::HashMap<u8, (String, usize)> =
        std::collections::HashMap::new();

    if !decoded.chunks.is_empty() {
        std::fs::create_dir_all(&chunks_dir)?;
    }

    for chunk in &decoded.chunks {
        let raw = encode_eustress_chunk(chunk, &mut material_tally);
        let compressed = lz4_flex::compress_prepend_size(&raw);
        let file_name = format!("chunk_{}_{}_{}.bin", chunk.cx, chunk.cy, chunk.cz);
        std::fs::write(chunks_dir.join(file_name), compressed)?;
        written += 1;
    }

    // Fold the per-material approximation tally into the report.
    for (rbx_id, (rbx_name, count)) in material_tally {
        let mapped = map_roblox_material(rbx_id);
        let eustress_name = eustress_material_name(mapped.eustress_id);
        report
            .terrain_material_approximations
            .push(TerrainMaterialApproximation {
                roblox_material: rbx_name,
                eustress_material: eustress_name.to_string(),
                voxel_count: count,
            });
    }

    report.terrain_chunks_imported += written;

    // `imported` is reserved for a decode that ran clean to the end of the
    // grid. A desync leaves a fragment on disk, and calling that "imported"
    // would make 48-of-3450 chunks read as a faithful import — the failure has
    // to stay legible in the artifact, not just in the console.
    let source = match (written > 0, decode_incomplete) {
        (true, false) => "imported",
        (true, true) => "partial",
        (false, _) => "none",
    };

    // Patch the Terrain _instance.toml with material_colors + globals +
    // the source flip. Only do this if the TOML already exists (the
    // materializer creates it via create_instance before calling us).
    let toml_path = terrain_dir.join("_instance.toml");
    if toml_path.is_file() {
        if let Err(e) = patch_terrain_toml(&toml_path, material_colors, globals, source) {
            // A TOML patch failure is non-fatal for the voxel import —
            // the chunks are already on disk. Log it as a decode error
            // note so the user sees something went sideways.
            report.terrain_decode_errors.push(TerrainDecodeError {
                cx: 0,
                cy: 0,
                cz: 0,
                reason: format!("failed to patch Terrain _instance.toml: {e}"),
            });
        }
    }

    Ok(written)
}

/// Eustress material name for a mapped id (for reporting).
fn eustress_material_name(id: u8) -> &'static str {
    use eustress_material::*;
    match id {
        GRASS => "Grass",
        ROCK => "Rock",
        DIRT => "Dirt",
        SNOW => "Snow",
        SAND => "Sand",
        MUD => "Mud",
        CONCRETE => "Concrete",
        ASPHALT => "Asphalt",
        WATER_MARKER => "Water",
        AIR_MARKER => "Air",
        _ => "Rock",
    }
}

/// Layer `[material_colors]`, `[terrain]`, and water globals onto the
/// Terrain instance TOML.
fn patch_terrain_toml(
    toml_path: &Path,
    material_colors: Option<&MaterialColors>,
    globals: &TerrainGlobals,
    source: &str,
) -> std::io::Result<()> {
    let raw = std::fs::read_to_string(toml_path)?;
    let mut doc: toml::Value = raw
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))?;
    let root = match doc.as_table_mut() {
        Some(t) => t,
        None => return Ok(()),
    };

    // ── [material_colors] ──
    if let Some(mc) = material_colors {
        let mut colors = toml::value::Table::new();
        for material in ALL_TERRAIN_MATERIALS {
            let c = mc.get_color(material);
            let arr = toml::Value::Array(vec![
                toml::Value::Float(c.r as f64 / 255.0),
                toml::Value::Float(c.g as f64 / 255.0),
                toml::Value::Float(c.b as f64 / 255.0),
            ]);
            colors.insert(terrain_material_label(material).to_string(), arr);
        }
        root.insert("material_colors".to_string(), toml::Value::Table(colors));
    }

    // ── [terrain] source flip + water globals ──
    let terrain = root
        .entry("terrain".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(t) = terrain.as_table_mut() {
        t.insert("source".to_string(), toml::Value::String(source.to_string()));
        t.insert(
            "cell_size".to_string(),
            toml::Value::Float(ROBLOX_CELL_STUDS as f64),
        );
        if let Some(c) = globals.water_color {
            t.insert(
                "water_color".to_string(),
                toml::Value::Array(vec![
                    toml::Value::Float(c[0] as f64),
                    toml::Value::Float(c[1] as f64),
                    toml::Value::Float(c[2] as f64),
                ]),
            );
        }
        if let Some(v) = globals.water_transparency {
            t.insert(
                "water_transparency".to_string(),
                toml::Value::Float(v as f64),
            );
        }
        if let Some(v) = globals.water_wave_size {
            t.insert("water_wave_size".to_string(), toml::Value::Float(v as f64));
        }
        if let Some(v) = globals.water_wave_speed {
            t.insert("water_wave_speed".to_string(), toml::Value::Float(v as f64));
        }
        if let Some(v) = globals.water_reflectance {
            t.insert(
                "water_reflectance".to_string(),
                toml::Value::Float(v as f64),
            );
        }
    }

    let new_raw = toml::to_string_pretty(&doc).unwrap_or(raw);
    std::fs::write(toml_path, new_raw)?;
    Ok(())
}

/// The full Roblox `TerrainMaterials` set in a stable order, used to
/// serialise the `[material_colors]` table.
const ALL_TERRAIN_MATERIALS: [TerrainMaterials; 21] = [
    TerrainMaterials::Grass,
    TerrainMaterials::Slate,
    TerrainMaterials::Concrete,
    TerrainMaterials::Brick,
    TerrainMaterials::Sand,
    TerrainMaterials::WoodPlanks,
    TerrainMaterials::Rock,
    TerrainMaterials::Glacier,
    TerrainMaterials::Snow,
    TerrainMaterials::Sandstone,
    TerrainMaterials::Mud,
    TerrainMaterials::Basalt,
    TerrainMaterials::Ground,
    TerrainMaterials::CrackedLava,
    TerrainMaterials::Asphalt,
    TerrainMaterials::Cobblestone,
    TerrainMaterials::Ice,
    TerrainMaterials::LeafyGrass,
    TerrainMaterials::Salt,
    TerrainMaterials::Limestone,
    TerrainMaterials::Pavement,
];

/// Stable string label for a Roblox terrain material (for the TOML key).
fn terrain_material_label(m: TerrainMaterials) -> &'static str {
    match m {
        TerrainMaterials::Grass => "Grass",
        TerrainMaterials::Slate => "Slate",
        TerrainMaterials::Concrete => "Concrete",
        TerrainMaterials::Brick => "Brick",
        TerrainMaterials::Sand => "Sand",
        TerrainMaterials::WoodPlanks => "WoodPlanks",
        TerrainMaterials::Rock => "Rock",
        TerrainMaterials::Glacier => "Glacier",
        TerrainMaterials::Snow => "Snow",
        TerrainMaterials::Sandstone => "Sandstone",
        TerrainMaterials::Mud => "Mud",
        TerrainMaterials::Basalt => "Basalt",
        TerrainMaterials::Ground => "Ground",
        TerrainMaterials::CrackedLava => "CrackedLava",
        TerrainMaterials::Asphalt => "Asphalt",
        TerrainMaterials::Cobblestone => "Cobblestone",
        TerrainMaterials::Ice => "Ice",
        TerrainMaterials::LeafyGrass => "LeafyGrass",
        TerrainMaterials::Salt => "Salt",
        TerrainMaterials::Limestone => "Limestone",
        TerrainMaterials::Pavement => "Pavement",
        // rbx_types 3.x marks `TerrainMaterials` non-exhaustive (and Roblox
        // keeps adding materials); map anything unrecognized to the generic
        // Ground splat bucket so import never panics on a future material.
        _ => "Ground",
    }
}

// ---------------------------------------------------------------------------
// Variant helpers — pull terrain props off the rbx instance
// ---------------------------------------------------------------------------

/// Extract a byte-blob property's bytes by name.
///
/// Handles BOTH wire forms Roblox uses for a large binary payload:
/// - `BinaryString` — the blob stored inline on the instance.
/// - `SharedString` — the blob interned in the file's shared-string table,
///   where Roblox deduplicates large payloads (`csg.rs`'s `MeshData` read
///   accepts both variants for the same reason).
pub fn binary_string_bytes<'a>(
    props: &'a std::collections::HashMap<String, Variant>,
    name: &str,
) -> Option<&'a [u8]> {
    match props.get(name) {
        Some(Variant::BinaryString(bs)) => Some(bs.as_ref()),
        Some(Variant::SharedString(ss)) => Some(ss.data()),
        _ => None,
    }
}

/// Extract the `MaterialColors` property if present.
pub fn material_colors<'a>(
    props: &'a std::collections::HashMap<String, Variant>,
) -> Option<&'a MaterialColors> {
    match props.get("MaterialColors") {
        Some(Variant::MaterialColors(mc)) => Some(mc),
        _ => None,
    }
}

/// Collect the global water-related terrain properties.
pub fn collect_globals(props: &std::collections::HashMap<String, Variant>) -> TerrainGlobals {
    let f32_of = |name: &str| -> Option<f32> {
        match props.get(name) {
            Some(Variant::Float32(v)) => Some(*v),
            Some(Variant::Float64(v)) => Some(*v as f32),
            _ => None,
        }
    };
    let color_of = |name: &str| -> Option<[f32; 3]> {
        match props.get(name) {
            Some(Variant::Color3(c)) => Some([c.r, c.g, c.b]),
            Some(Variant::Color3uint8(c)) => {
                Some([c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0])
            }
            _ => None,
        }
    };
    TerrainGlobals {
        water_color: color_of("WaterColor"),
        water_transparency: f32_of("WaterTransparency"),
        water_wave_size: f32_of("WaterWaveSize"),
        water_wave_speed: f32_of("WaterWaveSpeed"),
        water_reflectance: f32_of("WaterReflectance"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2-byte file header every SmoothGrid opens with.
    fn grid_header() -> Vec<u8> {
        vec![SMOOTH_GRID_VERSION, 0x05]
    }

    /// Append a chunk header: three `i32` coordinate deltas, byte-plane
    /// interleaved big-endian — the inverse of [`read_chunk_delta`]. For an
    /// array of three ints the layout is `[X₃ Y₃ Z₃][X₂ Y₂ Z₂][X₁ Y₁ Z₁]
    /// [X₀ Y₀ Z₀]`, most-significant plane first.
    fn push_chunk_header(buf: &mut Vec<u8>, dx: i32, dy: i32, dz: i32) {
        let (bx, by, bz) = (dx.to_be_bytes(), dy.to_be_bytes(), dz.to_be_bytes());
        for plane in 0..4 {
            buf.push(bx[plane]);
            buf.push(by[plane]);
            buf.push(bz[plane]);
        }
    }

    /// Append `CELLS_PER_CHUNK` cells as a single repeated material/occupancy,
    /// in runs of 256 (the max a run byte can express).
    fn push_solid_cells(buf: &mut Vec<u8>, material: u8, occupancy: u8) {
        let mut emitted = 0;
        while emitted < CELLS_PER_CHUNK {
            let run = (CELLS_PER_CHUNK - emitted).min(256);
            // lead byte: material, occupancy-present, run-present
            buf.push((material & 0b0011_1111) | 0b0100_0000 | 0b1000_0000);
            buf.push(occupancy);
            buf.push((run - 1) as u8);
            emitted += run;
        }
    }

    /// Build a SmoothGrid blob: file header + one chunk whose entire 32³
    /// volume is a single RLE run of the given material/occupancy. Coordinates
    /// are deltas; for a lone chunk the delta *is* its absolute coordinate,
    /// since accumulation starts at the origin.
    fn single_run_chunk(dx: i32, dy: i32, dz: i32, material: u8, occupancy: u8) -> Vec<u8> {
        let mut buf = grid_header();
        push_chunk_header(&mut buf, dx, dy, dz);
        push_solid_cells(&mut buf, material, occupancy);
        buf
    }

    #[test]
    fn empty_grid_decodes_to_no_chunks() {
        // The canonical empty-terrain marker from the rbx_binary fixture.
        let buf = [0x01u8, 0x05u8];
        let res = decode_smooth_grid(&buf);
        assert!(res.chunks.is_empty());
        assert!(
            res.errors.is_empty(),
            "empty grid must not error: {:?}",
            res.errors
        );
    }

    #[test]
    fn truly_empty_buffer_is_graceful() {
        assert!(decode_smooth_grid(&[]).chunks.is_empty());
        assert!(decode_smooth_grid(&[0x01]).chunks.is_empty());
    }

    #[test]
    fn single_grass_chunk_decodes_full_volume() {
        // material 2 = Grass, occupancy 200.
        let buf = single_run_chunk(0, 0, 0, 2, 200);
        let res = decode_smooth_grid(&buf);
        assert_eq!(res.errors.len(), 0, "no decode errors: {:?}", res.errors);
        assert_eq!(res.chunks.len(), 1);
        let chunk = &res.chunks[0];
        assert_eq!(chunk.material.len(), CELLS_PER_CHUNK);
        assert_eq!(chunk.occupancy.len(), CELLS_PER_CHUNK);
        assert!(chunk.material.iter().all(|&m| m == 2));
        assert!(chunk.occupancy.iter().all(|&o| o == 200));
        assert!(!chunk.is_empty());
    }

    #[test]
    fn mixed_materials_chunk() {
        // Hand-build a chunk: first cell Grass(2), second Rock(8),
        // third Water(1), rest Air via a big run.
        let mut buf = grid_header();
        push_chunk_header(&mut buf, 0, 0, 0);
        // Grass, occupancy present (255), no run (single cell).
        buf.push(2 | 0b0100_0000);
        buf.push(255);
        // Rock, occupancy 128, single.
        buf.push(8 | 0b0100_0000);
        buf.push(128);
        // Water, default occupancy (no occ flag → 255 for non-air), single.
        buf.push(1);
        // Remaining 32765 cells: Air (material 0), run-length encoded.
        let mut remaining = CELLS_PER_CHUNK - 3;
        while remaining > 0 {
            let run = remaining.min(256);
            // Air: material 0, run present, no occupancy (defaults to 0).
            buf.push(0 | 0b1000_0000);
            buf.push((run - 1) as u8);
            remaining -= run;
        }

        let res = decode_smooth_grid(&buf);
        assert_eq!(res.errors.len(), 0, "errors: {:?}", res.errors);
        assert_eq!(res.chunks.len(), 1);
        let chunk = &res.chunks[0];
        // Cells fill linearly in stream order; with the Y-outer/Z-middle/
        // X-inner layout (`index = y*1024 + z*32 + x`), the Nth stream cell
        // lands at `index(N, 0, 0)` (X is contiguous).
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 0)], 2); // Grass (cell 0)
        assert_eq!(chunk.material[VoxelChunk::index(1, 0, 0)], 8); // Rock  (cell 1)
        assert_eq!(chunk.occupancy[VoxelChunk::index(1, 0, 0)], 128);
        assert_eq!(chunk.material[VoxelChunk::index(2, 0, 0)], 1); // Water (cell 2)
        assert_eq!(chunk.occupancy[VoxelChunk::index(2, 0, 0)], 255); // default solid
                                                                      // A later cell is Air.
        assert_eq!(chunk.material[VoxelChunk::index(5, 0, 0)], 0);
    }

    /// The Shorelines water escape (count byte == 0) is one voxel plus a
    /// water-occupancy byte — it must be consumed so framing survives, which
    /// is what lets dense terrain decode past its first shoreline.
    #[test]
    fn water_escape_consumes_its_byte_and_holds_framing() {
        let mut buf = grid_header();
        push_chunk_header(&mut buf, 0, 0, 0);
        // Cell 0: Rock, occupancy present (0xC0), count byte 0 → water escape,
        // then a water-occupancy byte. One voxel.
        buf.push(8 | 0b0100_0000 | 0b1000_0000);
        buf.push(200); // solid occupancy
        buf.push(0); // count 0 → water escape
        buf.push(255); // water occupancy (fully submerged)
                       // Fill the remaining 32767 cells with Air.
        let mut remaining = CELLS_PER_CHUNK - 1;
        while remaining > 0 {
            let run = remaining.min(256);
            buf.push(0 | 0b1000_0000);
            buf.push((run - 1) as u8);
            remaining -= run;
        }

        let res = decode_smooth_grid(&buf);
        assert!(res.errors.is_empty(), "water escape must not desync: {:?}", res.errors);
        assert_eq!(res.chunks.len(), 1);
        let chunk = &res.chunks[0];
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 0)], 8, "the water voxel is still Rock");
        assert_eq!(chunk.occupancy[VoxelChunk::index(0, 0, 0)], 200);
    }

    #[test]
    fn malformed_truncated_chunk_logs_error_no_panic() {
        // Valid header, then a cell stream that ends mid-chunk.
        let mut buf = grid_header();
        push_chunk_header(&mut buf, 0, 0, 0);
        // Only encode 10 cells then cut off.
        buf.push(2 | 0b1000_0000); // Grass, run present
        buf.push(9); // run = 10
                     // (no more bytes — chunk wants 32768 cells)
        let res = decode_smooth_grid(&buf);
        assert!(res.chunks.is_empty());
        assert_eq!(res.errors.len(), 1);
        assert!(res.errors[0].reason.contains("ran out of bytes"));
    }

    #[test]
    fn run_overrun_is_caught() {
        // A run that claims to exceed the chunk size mid-stream. We fill
        // 32768 - 1 cells, then a run of 256 that overruns by 255.
        let mut buf = grid_header();
        push_chunk_header(&mut buf, 0, 0, 0);
        // Fill all but one cell with one big sequence of runs.
        let mut remaining = CELLS_PER_CHUNK - 1;
        while remaining > 0 {
            let run = remaining.min(256);
            buf.push(2 | 0b1000_0000);
            buf.push((run - 1) as u8);
            remaining -= run;
        }
        // Now one more run of 256 — overruns the single remaining slot.
        buf.push(2 | 0b1000_0000);
        buf.push(255);
        let res = decode_smooth_grid(&buf);
        assert_eq!(res.errors.len(), 1, "expected one overrun error");
        assert!(res.errors[0].reason.contains("overrun"));
    }

    #[test]
    fn material_mapping_flags_approximations() {
        // Grass (2) → Grass is exact.
        let g = map_roblox_material(2);
        assert_eq!(g.eustress_id, eustress_material::GRASS);
        assert!(!g.approximated);
        // Air is never an approximation.
        assert_eq!(map_roblox_material(0).eustress_id, AIR_MARKER);
        assert!(!map_roblox_material(0).approximated);
        // Water maps to the marker, not an approximation.
        assert_eq!(map_roblox_material(1).eustress_id, WATER_MARKER);
        assert!(!map_roblox_material(1).approximated);
        // An id past the table (a material a newer Roblox release added)
        // still falls back to Rock and IS flagged — the only remaining
        // approximation path now that the table maps every known id exactly.
        let unknown = map_roblox_material(200);
        assert_eq!(unknown.eustress_id, eustress_material::ROCK);
        assert!(unknown.approximated);
    }

    /// Wave 9.E gave `TerrainMaterial` a Roblox-parity tail, so ids that
    /// used to collapse into Rock/Snow/Concrete now round-trip exactly.
    /// Mountain Ascension alone is ~11% Slate+Sandstone — material fidelity
    /// that was previously flattened into generic Rock.
    #[test]
    fn roblox_parity_materials_map_exactly() {
        for (rbx_id, expected) in [
            (3u8, eustress_material::SLATE),
            (5, eustress_material::BRICK),
            (7, eustress_material::WOOD_PLANKS),
            (9, eustress_material::GLACIER),
            (11, eustress_material::SANDSTONE),
            (13, eustress_material::BASALT),
            (14, eustress_material::GROUND),
            (15, eustress_material::CRACKED_LAVA),
            (17, eustress_material::COBBLESTONE),
            (18, eustress_material::ICE),
            (19, eustress_material::LEAFY_GRASS),
            (20, eustress_material::SALT),
            (21, eustress_material::LIMESTONE),
            (22, eustress_material::PAVEMENT),
        ] {
            let m = map_roblox_material(rbx_id);
            assert_eq!(m.eustress_id, expected, "roblox id {rbx_id}");
            assert!(!m.approximated, "roblox id {rbx_id} should be exact now");
        }
    }

    #[test]
    fn import_writes_chunk_files_and_counts() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_terrain_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let terrain_dir = dir.join("Workspace").join("Terrain");
        std::fs::create_dir_all(&terrain_dir).unwrap();
        // Minimal Terrain TOML so the patch path runs.
        std::fs::write(
            terrain_dir.join("_instance.toml"),
            "[metadata]\nclass = \"Terrain\"\n",
        )
        .unwrap();

        // Two chunks landing at absolute (0,0,0) and (3,0,0) — expressed as
        // the deltas 0 then +3, since coordinates accumulate.
        let mut buf = grid_header();
        for (dx, mat) in [(0i32, 2u8), (3i32, 8u8)] {
            push_chunk_header(&mut buf, dx, 0, 0);
            push_solid_cells(&mut buf, mat, 255);
        }

        let mut report = ImportReport::default();
        let globals = TerrainGlobals {
            water_transparency: Some(0.3),
            ..Default::default()
        };
        let written =
            import_terrain(&terrain_dir, &buf, None, &globals, &mut report).expect("import");
        assert_eq!(written, 2);
        assert_eq!(report.terrain_chunks_imported, 2);
        assert!(terrain_dir
            .join("voxel_chunks")
            .join("chunk_0_0_0.bin")
            .is_file());
        assert!(terrain_dir
            .join("voxel_chunks")
            .join("chunk_3_0_0.bin")
            .is_file());

        // Rock(8) is exact, Grass(2) is exact → no approximations.
        // (Both materials map exactly; tally should be empty.)
        assert!(
            report.terrain_material_approximations.is_empty(),
            "unexpected approximations: {:?}",
            report.terrain_material_approximations
        );

        // The TOML should now carry the [terrain] source flip.
        let toml = std::fs::read_to_string(terrain_dir.join("_instance.toml")).unwrap();
        assert!(toml.contains("source = \"imported\""), "toml: {toml}");
        assert!(toml.contains("water_transparency"));

        // Verify a chunk file round-trips through LZ4 + has the right shape.
        let compressed =
            std::fs::read(terrain_dir.join("voxel_chunks").join("chunk_0_0_0.bin")).unwrap();
        let raw = lz4_flex::decompress_size_prepended(&compressed).unwrap();
        assert_eq!(raw[0], EUSTRESS_CHUNK_VERSION);
        assert_eq!(raw.len(), 3 + CELLS_PER_CHUNK * 2);
        // First cell: Grass → eustress id 0, occupancy 255.
        assert_eq!(raw[3], eustress_material::GRASS);
        assert_eq!(raw[4], 255);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every id Roblox currently defines maps exactly, so the only material an
    /// import can approximate is one past the table — i.e. a material a newer
    /// Roblox release added that this build has never heard of.
    #[test]
    fn import_records_approximation_for_unknown_material() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_terrain_approx_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let terrain_dir = dir.join("Workspace").join("Terrain");
        std::fs::create_dir_all(&terrain_dir).unwrap();
        std::fs::write(
            terrain_dir.join("_instance.toml"),
            "[metadata]\nclass = \"Terrain\"\n",
        )
        .unwrap();

        // Id 30 is inside the 6-bit material field but past the 23 Roblox
        // defines, and clear of the 0x3f escape → falls back to Rock, flagged.
        let buf = single_run_chunk(0, 0, 0, 30, 255);
        let mut report = ImportReport::default();
        import_terrain(
            &terrain_dir,
            &buf,
            None,
            &TerrainGlobals::default(),
            &mut report,
        )
        .expect("import");
        assert_eq!(report.terrain_material_approximations.len(), 1);
        let approx = &report.terrain_material_approximations[0];
        assert_eq!(approx.roblox_material, "Material30");
        assert_eq!(approx.eustress_material, "Rock");
        assert_eq!(approx.voxel_count, CELLS_PER_CHUNK);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A grid that decodes some chunks and then desyncs must land on
    /// `source = "partial"`, keep its raw payload, and say the recovery was
    /// incomplete. Labelling a fragment `"imported"` would make a place
    /// carrying a sliver of its terrain look like a faithful import.
    #[test]
    fn partial_decode_is_not_labelled_imported() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_terrain_partial_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let terrain_dir = dir.join("Workspace").join("Terrain");
        std::fs::create_dir_all(&terrain_dir).unwrap();
        std::fs::write(
            terrain_dir.join("_instance.toml"),
            "[metadata]\nclass = \"Terrain\"\n",
        )
        .unwrap();

        // One good chunk, then a header whose cell stream is cut short.
        let mut buf = grid_header();
        push_chunk_header(&mut buf, 0, 0, 0);
        push_solid_cells(&mut buf, 8, 255);
        push_chunk_header(&mut buf, 0, 0, 1);
        buf.push(8 | 0b1000_0000); // Rock, run present
        buf.push(9); // ...only 10 cells, then the buffer ends

        let mut report = ImportReport::default();
        let written = import_terrain(
            &terrain_dir,
            &buf,
            None,
            &TerrainGlobals::default(),
            &mut report,
        )
        .expect("import");

        assert_eq!(written, 1, "the one complete chunk should still be written");
        assert!(
            !report.terrain_decode_errors.is_empty(),
            "a desync must be reported"
        );
        let toml = std::fs::read_to_string(terrain_dir.join("_instance.toml")).unwrap();
        assert!(
            toml.contains("source = \"partial\""),
            "a fragment must not read as a clean import: {toml}"
        );
        assert!(
            terrain_dir.join("smooth_grid.raw").is_file(),
            "raw payload must survive a partial decode so it can be re-decoded later"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Chunk coordinates accumulate from deltas, so a raster run along +Z is a
    /// repeated `(0,0,1)` — the reason no absolute coordinate field exists.
    #[test]
    fn chunk_coords_accumulate_from_deltas() {
        let mut buf = grid_header();
        // Deltas (2,0,0) → (0,0,1) → (0,0,1) land at (2,0,0), (2,0,1), (2,0,2).
        for (dx, dy, dz) in [(2i32, 0i32, 0i32), (0, 0, 1), (0, 0, 1)] {
            push_chunk_header(&mut buf, dx, dy, dz);
            push_solid_cells(&mut buf, 8, 255); // Rock
        }
        let res = decode_smooth_grid(&buf);
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let coords: Vec<_> = res.chunks.iter().map(|c| (c.cx, c.cy, c.cz)).collect();
        assert_eq!(coords, vec![(2, 0, 0), (2, 0, 1), (2, 0, 2)]);
    }

    /// A negative delta walks backwards; the first chunk's delta is measured
    /// from the origin, so it doubles as an absolute coordinate.
    #[test]
    fn chunk_coord_deltas_are_signed() {
        let mut buf = grid_header();
        for (dx, dy, dz) in [(-17i32, -1i32, -15i32), (1, 0, -2)] {
            push_chunk_header(&mut buf, dx, dy, dz);
            push_solid_cells(&mut buf, 8, 255);
        }
        let res = decode_smooth_grid(&buf);
        assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
        let coords: Vec<_> = res.chunks.iter().map(|c| (c.cx, c.cy, c.cz)).collect();
        assert_eq!(coords, vec![(-17, -1, -15), (-16, -1, -17)]);
    }
}
