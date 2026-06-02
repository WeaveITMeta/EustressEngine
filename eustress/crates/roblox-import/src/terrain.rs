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
//! The on-disk format is documented by Roblox's own engineer Arseny
//! Kapoulkine (<https://zeux.io/2017/03/27/voxel-terrain-storage/>) and
//! corroborated against the empty-terrain fixture shipped in `rbx_binary`
//! (`SmoothGrid: AQU=` → bytes `[0x01, 0x05]`).
//!
//! ```text
//! SmoothGrid := u8 version (== 1)
//!            || ChunkRecord*            (until end of buffer)
//!
//! ChunkRecord := i32_le chunk_x         (chunk grid coordinate)
//!             || i32_le chunk_y
//!             || i32_le chunk_z
//!             || Cell* (run-length encoded; exactly 32^3 cells decoded)
//!
//! Cell (RLE):
//!   lead byte:  bits 0..=5  material id (0 = Air)
//!               bit  6      occupancy-present flag
//!               bit  7      run-length-present flag
//!   if occupancy-present:  u8 occupancy   (else default: 255 for solid, 0 for air)
//!   if run-length-present: u8 run_minus_1 (the run repeats run_minus_1 + 1 times;
//!                                          else the run length is 1)
//! ```
//!
//! Occupancy decodes to a fraction as `(occupancy + 1) / 256.0` for
//! non-air materials and `0.0` for air. We keep the quantised `u8`
//! on disk (the Eustress mesher applies the same `(q+1)/256` rule).
//!
//! Cells within a chunk are iterated **Y outer, X middle, Z inner**
//! (Roblox convention) so the linear index is `y*1024 + x*32 + z`.
//!
//! ### Defensive decoding
//!
//! Real places (e.g. Vehicle Simulator, ~60 MB) hit edge cases. Every
//! read is bounds-checked; a malformed chunk pushes a
//! [`TerrainDecodeError`] and decoding continues with the next chunk
//! rather than panicking. The exact chunk-header layout is the least
//! publicly-documented part of the format, so [`decode_smooth_grid`]
//! treats a chunk whose RLE stream would overrun the buffer as a decode
//! error (logged) and stops — it never reads out of bounds.

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
/// **Caveat (spec §6.3):** the exact integer Roblox writes into a
/// SmoothGrid cell is an internal enum that is not byte-for-byte
/// published. This table is the best-effort mapping; unknown / new ids
/// fall back to `Rock` (the spec's safe default, §6.8) and are flagged.
/// Mapping by *id* may misname an exotic material, but the
/// Roblox→Eustress *visual* bucket (and thus the rendered result) stays
/// reasonable because most ids collapse to Rock/Grass/Snow anyway.
const MATERIAL_TABLE: &[(&str, u8, bool)] = {
    use eustress_material::*;
    &[
        ("Air", AIR_MARKER, true),     // 0
        ("Water", WATER_MARKER, true), // 1
        ("Grass", GRASS, true),        // 2
        ("Slate", ROCK, false),        // 3
        ("Concrete", CONCRETE, true),  // 4
        ("Brick", CONCRETE, false),    // 5
        ("Sand", SAND, true),          // 6
        ("WoodPlanks", DIRT, false),   // 7
        ("Rock", ROCK, true),          // 8
        ("Glacier", SNOW, false),      // 9
        ("Snow", SNOW, true),          // 10
        ("Sandstone", ROCK, false),    // 11
        ("Mud", MUD, true),            // 12
        ("Basalt", ROCK, false),       // 13
        ("Ground", DIRT, true),        // 14
        ("CrackedLava", ROCK, false),  // 15 (color override carries the lava tint)
        ("Asphalt", ASPHALT, true),    // 16
        ("Cobblestone", ROCK, false),  // 17
        ("Ice", SNOW, false),          // 18
        ("LeafyGrass", GRASS, false),  // 19
        ("Salt", SNOW, false),         // 20
        ("Limestone", ROCK, false),    // 21
        ("Pavement", CONCRETE, false), // 22
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
/// `y*1024 + x*32 + z` order. `material[i]` is a **Roblox** material id
/// (mapping to Eustress is applied at write time so the
/// [`TerrainMaterialApproximation`] tally is accurate). `occupancy[i]`
/// is the quantised `u8` (decode with `(q + 1) / 256.0`).
#[derive(Debug, Clone)]
pub struct VoxelChunk {
    /// Chunk grid X coordinate.
    pub cx: i32,
    /// Chunk grid Y coordinate.
    pub cy: i32,
    /// Chunk grid Z coordinate.
    pub cz: i32,
    /// Per-cell Roblox material id (0 = Air), linear YXZ order.
    pub material: Vec<u8>,
    /// Per-cell quantised occupancy (0..=255), linear YXZ order.
    pub occupancy: Vec<u8>,
}

impl VoxelChunk {
    /// True when every cell is Air — such chunks are not written to disk.
    pub fn is_empty(&self) -> bool {
        self.material.iter().all(|&m| m == 0)
    }

    /// Linear index for `(x, y, z)` local cell coordinates (YXZ order).
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        y * (CHUNK_EDGE * CHUNK_EDGE) + x * CHUNK_EDGE + z
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

    #[inline]
    fn read_i32_le(&mut self) -> Option<i32> {
        if self.remaining() < 4 {
            return None;
        }
        let bytes: [u8; 4] = self.buf[self.pos..self.pos + 4].try_into().ok()?;
        self.pos += 4;
        Some(i32::from_le_bytes(bytes))
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

    // Empty terrain is the 2-byte `[0x01, 0x05]` marker (or shorter).
    // Anything < 5 bytes can't hold a chunk header → treat as empty.
    if buf.len() < 5 {
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

    // Cap the chunk count so a corrupt buffer can never spin forever.
    // 1 km³ at 4-stud cells is ~480 chunks; a million-instance world is
    // far larger, so allow generously but finite.
    const MAX_CHUNKS: usize = 1_000_000;
    let mut chunk_count = 0usize;

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

        // ── Chunk header: three i32 LE grid coordinates. ──
        let (cx, cy, cz) = match (cur.read_i32_le(), cur.read_i32_le(), cur.read_i32_le()) {
            (Some(x), Some(y), Some(z)) => (x, y, z),
            _ => {
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

/// Decode exactly [`CELLS_PER_CHUNK`] cells of RLE data from `cur`.
///
/// Returns `(material, occupancy)` parallel arrays, or an `Err(reason)`
/// string on bounds overrun / framing loss. Never panics.
fn decode_chunk_cells(cur: &mut Cursor) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut material = Vec::with_capacity(CELLS_PER_CHUNK);
    let mut occupancy = Vec::with_capacity(CELLS_PER_CHUNK);

    while material.len() < CELLS_PER_CHUNK {
        let lead = cur
            .read_u8()
            .ok_or_else(|| format!("ran out of bytes after {} cells", material.len()))?;

        // bits 0..=5: material id; bit 6: occupancy present; bit 7: run present.
        let mat_id = lead & 0b0011_1111;
        let has_occupancy = (lead & 0b0100_0000) != 0;
        let has_run = (lead & 0b1000_0000) != 0;

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

        let run = if has_run {
            let run_minus_1 = cur
                .read_u8()
                .ok_or_else(|| "ran out of bytes reading run length".to_string())?;
            run_minus_1 as usize + 1
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
/// [ for each of CELLS_PER_CHUNK cells, YXZ order: ]
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

    // Surface decode errors first (graceful degradation — spec §6.8).
    for err in decoded.errors {
        report.terrain_decode_errors.push(err);
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

    // Patch the Terrain _instance.toml with material_colors + globals +
    // the source flip. Only do this if the TOML already exists (the
    // materializer creates it via create_instance before calling us).
    let toml_path = terrain_dir.join("_instance.toml");
    if toml_path.is_file() {
        if let Err(e) = patch_terrain_toml(&toml_path, material_colors, globals, written > 0) {
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
    has_voxels: bool,
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
        t.insert(
            "source".to_string(),
            toml::Value::String(if has_voxels { "imported" } else { "none" }.to_string()),
        );
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
    }
}

// ---------------------------------------------------------------------------
// Variant helpers — pull terrain props off the rbx instance
// ---------------------------------------------------------------------------

/// Extract a `BinaryString` property's bytes by name.
pub fn binary_string_bytes<'a>(
    props: &'a std::collections::HashMap<String, Variant>,
    name: &str,
) -> Option<&'a [u8]> {
    match props.get(name) {
        Some(Variant::BinaryString(bs)) => Some(bs.as_ref()),
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

    /// Build a SmoothGrid blob: version byte + one chunk whose entire
    /// 32³ volume is a single RLE run of the given material/occupancy.
    fn single_run_chunk(cx: i32, cy: i32, cz: i32, material: u8, occupancy: u8) -> Vec<u8> {
        let mut buf = vec![SMOOTH_GRID_VERSION];
        buf.extend_from_slice(&cx.to_le_bytes());
        buf.extend_from_slice(&cy.to_le_bytes());
        buf.extend_from_slice(&cz.to_le_bytes());
        // 32768 cells in runs of 256 (max run = 255+1). 32768 / 256 = 128 runs.
        let total = CELLS_PER_CHUNK;
        let mut emitted = 0;
        while emitted < total {
            let run = (total - emitted).min(256);
            // lead byte: material, occupancy-present, run-present
            buf.push((material & 0b0011_1111) | 0b0100_0000 | 0b1000_0000);
            buf.push(occupancy);
            buf.push((run - 1) as u8);
            emitted += run;
        }
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
        let mut buf = vec![SMOOTH_GRID_VERSION];
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
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
        // Cells fill linearly in stream order; with the documented
        // Y-outer/X-middle/Z-inner layout (`index = y*1024 + x*32 + z`),
        // the Nth stream cell lands at `index(0, 0, N)` (Z is contiguous).
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 0)], 2); // Grass (cell 0)
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 1)], 8); // Rock  (cell 1)
        assert_eq!(chunk.occupancy[VoxelChunk::index(0, 0, 1)], 128);
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 2)], 1); // Water (cell 2)
        assert_eq!(chunk.occupancy[VoxelChunk::index(0, 0, 2)], 255); // default solid
                                                                      // A later cell is Air.
        assert_eq!(chunk.material[VoxelChunk::index(0, 0, 5)], 0);
    }

    #[test]
    fn malformed_truncated_chunk_logs_error_no_panic() {
        // Valid header, then a cell stream that ends mid-chunk.
        let mut buf = vec![SMOOTH_GRID_VERSION];
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
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
        let mut buf = vec![SMOOTH_GRID_VERSION];
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
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
        // Slate (id 3) → Rock is an approximation.
        let m = map_roblox_material(3);
        assert_eq!(m.eustress_id, eustress_material::ROCK);
        assert!(m.approximated);
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

        // Two chunks at different coords.
        let mut buf = vec![SMOOTH_GRID_VERSION];
        for (cx, mat) in [(0i32, 2u8), (3i32, 8u8)] {
            buf.extend_from_slice(&cx.to_le_bytes());
            buf.extend_from_slice(&0i32.to_le_bytes());
            buf.extend_from_slice(&0i32.to_le_bytes());
            let mut emitted = 0;
            while emitted < CELLS_PER_CHUNK {
                let run = (CELLS_PER_CHUNK - emitted).min(256);
                buf.push((mat & 0b0011_1111) | 0b0100_0000 | 0b1000_0000);
                buf.push(255);
                buf.push((run - 1) as u8);
                emitted += run;
            }
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

    #[test]
    fn import_records_approximation_for_slate() {
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

        // One chunk of Slate(3) → Rock approximation.
        let buf = single_run_chunk(0, 0, 0, 3, 255);
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
        assert_eq!(approx.roblox_material, "Slate"); // id 3 = Slate
        assert_eq!(approx.eustress_material, "Rock");
        assert_eq!(approx.voxel_count, CELLS_PER_CHUNK);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
