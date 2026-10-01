//! Water imported with voxel terrain: the seas, lakes and pools a Roblox
//! place carries in its terrain.
//!
//! A Roblox place keeps its water as terrain cells beside the solid ones. The
//! voxel terrain build (`voxel_import`, run by the engine's
//! `terrain_voxel_load` and the Player's Space opener) reads them with the
//! rest of the place's terrain, works out how high the water stands over
//! every column of the raster, and the loader puts that on the terrain root
//! as [`TerrainVoxelWater`], with the place's water colour and transparency.
//! Water levels fall on fixed fractions of a cell, so a sea stands at one
//! level, give or take the part-filled cells along its shores.
//!
//! [`sync_voxel_water`] draws it. The columns are grouped into bodies of
//! water ([`voxel_water_fills`]): columns joined through the four beside each
//! whose water stands within [`VOXEL_WATER_JOIN_METRES`] of the level the body
//! was flooded from. A body is drawn flat at its most common level (to the
//! centimetre), so a sea is one surface however its shore cells are filled,
//! and water that steps down a slope splits into one body per step. Each body
//! becomes one `WaterFill`, the mask a lake's flood makes, meshed as a flat
//! surface at its level by `water_bodies::water_body_meshes`, and its chunk
//! meshes are merged into blocks of [`VOXEL_WATER_BLOCK_CHUNKS`] chunks a side
//! ([`merge_water_blocks`]): every translucent surface is a draw of its own,
//! and an imported sea covers thousands of chunks. The water material drops
//! every pixel whose ground stands above the water, so the waterline follows
//! the ground between cells. At most [`MAX_VOXEL_WATER_BODIES`] bodies are
//! drawn, the ones holding the most water. Water the place gives a colour or
//! a transparency draws in a variant of the material with them
//! ([`WaterSurfaceAssets::tinted_material`]); water it gives neither draws
//! like the rest of the world's water.
//!
//! A terrain filled with water in Studio (a Terrain API water fill) carries
//! the same component. It is not an instance, not in the Explorer, and has
//! no colliders. A terrain read from `Workspace/Terrain` keeps its water in
//! [`WATER_FILE_NAME`] beside `_terrain.toml`: Save writes it
//! ([`save_voxel_water`]) and `disk::hydrate_terrain_from_disk` reads it back
//! ([`load_voxel_water`]). An imported place's water is built again from its
//! voxel chunks on every open. The surfaces are built when the component
//! arrives, and again only when it changes, when the terrain's grid or
//! raster changes, or when something else despawns one of them. An edit of
//! the ground leaves the water where it stands; the material trims it
//! wherever the ground stands above it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use bevy::light::NotShadowCaster;
use bevy::platform::time::Instant;
use bevy::prelude::*;
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{info, warn};

use super::layers::{surface_data, TerrainBaked};
use super::material::{material_cell_weights, TerrainMaterial};
use super::water::{WaterSurface, WaterSurfaceAssets, WaterSurfaceMaterial};
use super::water_bodies::{water_body_meshes, CellGrid, WaterFill, WaterMeshData};
use super::{TerrainConfig, TerrainData, TerrainGridKey, TerrainRoot};

/// At most this many bodies of water are drawn: the ones holding the most
/// water. The rest are left out, with a warning.
pub const MAX_VOXEL_WATER_BODIES: usize = 256;
/// Neighbouring columns whose water stands within this many metres of the
/// level a body was flooded from belong to that body. A part-filled cell
/// moves its column's level by less than a cell, so a quarter of a Roblox
/// cell's height keeps a sea whole while a waterfall's steps stay apart.
pub const VOXEL_WATER_JOIN_METRES: f32 = 0.3;
/// Chunks per side of the blocks imported water is meshed in.
pub const VOXEL_WATER_BLOCK_CHUNKS: i32 = 16;
/// Opacity of the clearest imported water, so water a place makes fully
/// transparent still shows its surface.
pub const VOXEL_WATER_MIN_ALPHA: f32 = 0.15;
/// A body's level is its most common level, counted in bins of this many
/// steps per metre, the centimetre, which also merges the float noise
/// between columns that stand for the same water.
const LEVEL_STEPS_PER_METRE: f64 = 100.0;

// ============================================================================
// The component
// ============================================================================

/// Water surfaces carried by imported voxel terrain (see the module docs).
#[derive(Component, Clone, Debug, Default)]
pub struct TerrainVoxelWater {
    /// World Y of the water surface over each raster cell of the root's TerrainData,
    /// row-major like `height_cache`, `f32::NAN` where the column holds no water.
    pub levels: Vec<f32>,
    /// Raster width the levels were built for (TerrainData::cache_width).
    pub width: u32,
    /// Raster height the levels were built for (TerrainData::cache_height).
    pub height: u32,
    /// Water colour, sRGB 0..1 (the import's `[terrain] water_color`); None keeps the material default.
    pub color: Option<[f32; 3]>,
    /// 0 opaque to 1 clear (the import's `[terrain] water_transparency`); None keeps the default.
    pub transparency: Option<f32>,
}

// ============================================================================
// Levels
// ============================================================================

/// One body of water while [`voxel_water_fills`] gathers it.
#[derive(Clone, Debug)]
struct Body {
    /// Row-major raster index of the column it was flooded from.
    seed: usize,
    /// How many columns it holds.
    count: usize,
    /// Inclusive raster cell box of those columns.
    lo_x: usize,
    lo_z: usize,
    hi_x: usize,
    hi_z: usize,
    /// Its columns by level, in [`LEVEL_STEPS_PER_METRE`] bins: how many fall
    /// in each bin, and the sum of their levels.
    bins: HashMap<i64, (usize, f64)>,
}

impl Body {
    fn new(seed: usize) -> Self {
        Self { seed, count: 0, lo_x: usize::MAX, lo_z: usize::MAX, hi_x: 0, hi_z: 0, bins: HashMap::new() }
    }

    fn add(&mut self, x: usize, z: usize, level: f32) {
        self.count += 1;
        (self.lo_x, self.lo_z) = (self.lo_x.min(x), self.lo_z.min(z));
        (self.hi_x, self.hi_z) = (self.hi_x.max(x), self.hi_z.max(z));
        let bin = self.bins.entry((f64::from(level) * LEVEL_STEPS_PER_METRE).round() as i64).or_insert((0, 0.0));
        bin.0 += 1;
        bin.1 += f64::from(level);
    }

    /// The level the body is drawn at: the mean of the columns in its
    /// fullest bin, ties going to the higher bin.
    fn level(&self) -> f32 {
        self.bins
            .iter()
            .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then(a.0.cmp(b.0)))
            .map_or(f32::NAN, |(_, &(count, sum))| (sum / count as f64) as f32)
    }

    /// Columns of the box.
    fn columns(&self) -> usize {
        self.hi_x - self.lo_x + 1
    }

    /// Rows of the box.
    fn rows(&self) -> usize {
        self.hi_z - self.lo_z + 1
    }
}

/// Whether a column standing at `level` joins a body flooded from a column
/// at `seed_level`: it holds water (a finite level) within
/// [`VOXEL_WATER_JOIN_METRES`] of it.
fn joins(level: f32, seed_level: f32) -> bool {
    level.is_finite() && (level - seed_level).abs() <= VOXEL_WATER_JOIN_METRES
}

/// Flood the body of water whose seed column is raster index `seed` (a
/// finite level) through the four columns beside each, over the columns
/// not yet `taken` that join it, marking each taken and handing its index
/// and level to `visit`. A column goes to the first body that reaches it,
/// so flooding the same seeds in the same order hands out the same bodies.
fn flood_body(
    levels: &[f32],
    width: usize,
    height: usize,
    seed: usize,
    taken: &mut [bool],
    stack: &mut Vec<usize>,
    mut visit: impl FnMut(usize, f32),
) {
    let seed_level = levels[seed];
    taken[seed] = true;
    stack.clear();
    stack.push(seed);
    while let Some(i) = stack.pop() {
        visit(i, levels[i]);
        let (x, z) = (i % width, i / width);
        let neighbours = [
            (x > 0).then(|| i - 1),
            (x + 1 < width).then(|| i + 1),
            (z > 0).then(|| i - width),
            (z + 1 < height).then(|| i + width),
        ];
        for n in neighbours.into_iter().flatten() {
            if !taken[n] && joins(levels[n], seed_level) {
                taken[n] = true;
                stack.push(n);
            }
        }
    }
}

/// The water of `water` over the raster of `data` (the terrain of `config`)
/// as one [`WaterFill`] per body of water (see the module docs), lowest
/// first and then in raster order of the column each was flooded from: its
/// columns as a mask cropped to them, with the world box and cell spacing a
/// lake's flood has (see `water_bodies::flood_fill_water`), at the body's
/// most common level. Past [`MAX_VOXEL_WATER_BODIES`] bodies, only the ones
/// holding the most water are kept. Empty when the levels were built for
/// another raster than `data`'s, or `data` has no raster of at least two
/// cells a side.
pub fn voxel_water_fills(config: &TerrainConfig, data: &TerrainData, water: &TerrainVoxelWater) -> Vec<WaterFill> {
    let Some(grid) = CellGrid::of(config, data) else {
        return Vec::new();
    };
    let (width, height) = (grid.width, grid.height);
    let cells = width * height;
    if water.width as usize != width || water.height as usize != height || water.levels.len() != cells {
        return Vec::new();
    }

    // Every body: how many columns, the box around them and their levels.
    let mut taken = vec![false; cells];
    let mut stack = Vec::new();
    let mut bodies: Vec<Body> = Vec::new();
    for seed in 0..cells {
        if taken[seed] || !water.levels[seed].is_finite() {
            continue;
        }
        let mut body = Body::new(seed);
        flood_body(&water.levels, width, height, seed, &mut taken, &mut stack, |i, level| {
            body.add(i % width, i / width, level)
        });
        bodies.push(body);
    }

    // Past the cap the bodies holding the most water win, ties going to the
    // one flooded first, so the same ones always do.
    let mut keep = vec![true; bodies.len()];
    if bodies.len() > MAX_VOXEL_WATER_BODIES {
        let mut order: Vec<usize> = (0..bodies.len()).collect();
        order.sort_by(|&a, &b| bodies[b].count.cmp(&bodies[a].count).then(bodies[a].seed.cmp(&bodies[b].seed)));
        let left_out: usize = order[MAX_VOXEL_WATER_BODIES..].iter().map(|&i| bodies[i].count).sum();
        warn!(
            target: "eustress::terrain::water",
            bodies = bodies.len(),
            drawn = MAX_VOXEL_WATER_BODIES,
            columns_left_out = left_out,
            "imported water has more bodies than are drawn; the ones holding the least water are left out"
        );
        for &i in &order[MAX_VOXEL_WATER_BODIES..] {
            keep[i] = false;
        }
    }

    // Each kept body's mask over its box. Flooding every body again from the
    // same seeds in the same order hands each exactly the columns it had.
    taken.fill(false);
    let mut fills: Vec<(usize, WaterFill)> = Vec::new();
    for (body, &kept) in bodies.iter().zip(&keep) {
        let columns = body.columns();
        let mut wet = if kept { vec![false; columns * body.rows()] } else { Vec::new() };
        flood_body(&water.levels, width, height, body.seed, &mut taken, &mut stack, |i, _| {
            if kept {
                wet[(i / width - body.lo_z) * columns + (i % width - body.lo_x)] = true;
            }
        });
        if !kept {
            continue;
        }
        fills.push((
            body.seed,
            WaterFill {
                level: body.level(),
                origin: UVec2::new(body.lo_x as u32, body.lo_z as u32),
                size: UVec2::new(columns as u32, body.rows() as u32),
                wet,
                wet_count: body.count,
                bounds: (grid.world(body.lo_x, body.lo_z), grid.world(body.hi_x, body.hi_z)),
                cell: grid.step,
            },
        ));
    }
    fills.sort_by(|(seed_a, a), (seed_b, b)| a.level.total_cmp(&b.level).then(seed_a.cmp(seed_b)));
    fills.into_iter().map(|(_, fill)| fill).collect()
}

/// `chunks` (one mesh per chunk, each relative to its chunk's corner, as
/// `water_body_meshes` returns them) merged into one mesh per block of
/// [`VOXEL_WATER_BLOCK_CHUNKS`] chunks a side, each relative to its block's
/// corner (`block * VOXEL_WATER_BLOCK_CHUNKS * chunk_size`), in block order.
/// No quad moves in the world.
pub fn merge_water_blocks(chunks: Vec<(IVec2, WaterMeshData)>, chunk_size: f32) -> Vec<(IVec2, WaterMeshData)> {
    let mut blocks: BTreeMap<(i32, i32), WaterMeshData> = BTreeMap::new();
    for (chunk, data) in chunks {
        let block = IVec2::new(chunk.x.div_euclid(VOXEL_WATER_BLOCK_CHUNKS), chunk.y.div_euclid(VOXEL_WATER_BLOCK_CHUNKS));
        let shift = (chunk - block * VOXEL_WATER_BLOCK_CHUNKS).as_vec2() * chunk_size;
        let merged = blocks.entry((block.x, block.y)).or_default();
        let base = merged.positions.len() as u32;
        merged.positions.extend(data.positions.iter().map(|p| [p[0] + shift.x, p[1], p[2] + shift.y]));
        merged.normals.extend_from_slice(&data.normals);
        merged.uvs.extend_from_slice(&data.uvs);
        merged.flows.extend_from_slice(&data.flows);
        merged.indices.extend(data.indices.iter().map(|index| index + base));
    }
    blocks.into_iter().map(|((x, z), data)| (IVec2::new(x, z), data)).collect()
}

/// The colour and opacity `water` gives its surfaces, each `None` where it
/// keeps the material's: its colour, and its transparency as an opacity no
/// lower than [`VOXEL_WATER_MIN_ALPHA`].
fn voxel_water_tint(water: &TerrainVoxelWater) -> (Option<[f32; 3]>, Option<f32>) {
    let alpha = water
        .transparency
        .filter(|transparency| transparency.is_finite())
        .map(|transparency| (1.0 - transparency).clamp(VOXEL_WATER_MIN_ALPHA, 1.0));
    (water.color, alpha)
}

// ============================================================================
// Water paint
// ============================================================================

/// Seabeds are Sand this many metres under water, then Mud, and Slate past
/// [`SEABED_MUD_DEPTH_M`].
pub const SEABED_SAND_DEPTH_M: f32 = 4.0;
pub const SEABED_MUD_DEPTH_M: f32 = 20.0;

/// The ground under `depth` metres of water: sand in the shallows, mud
/// further out, slate in the deep.
pub fn seabed_material(depth: f32) -> TerrainMaterial {
    if depth < SEABED_SAND_DEPTH_M {
        TerrainMaterial::Sand
    } else if depth < SEABED_MUD_DEPTH_M {
        TerrainMaterial::Mud
    } else {
        TerrainMaterial::Slate
    }
}

/// What [`convert_water_paint`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterPaintConversion {
    /// Cells that were painted Water and are now seabed under real water.
    pub cells: usize,
    /// Bodies of painted cells that took water.
    pub bodies: usize,
    /// Painted cells left painted: they stand above the level their body
    /// holds (a river running downhill).
    pub left_painted: usize,
    /// World XZ box of the converted cells.
    pub bounds: Option<(Vec2, Vec2)>,
}

/// Turn ground painted Water (older Spaces made it a ground colour) into
/// real water over seabed. The cells whose strongest material is Water are
/// grouped into bodies (joined through the four cells beside each); a body
/// holds water up to its lowest rim, the lowest ground beside it that is not
/// painted, where the water would spill (its highest cell when nothing
/// unpainted borders it). Each cell of a body whose ground lies below that
/// level takes water up to it and the seabed material its depth gives
/// ([`seabed_material`]); a cell at or above it keeps its paint. `data` is
/// the base raster, `water` the root's water, sized to it first. `recorder`
/// is told about every tile before it changes.
pub fn convert_water_paint(
    config: &TerrainConfig,
    data: &mut TerrainData,
    water: &mut TerrainVoxelWater,
    mut recorder: Option<&mut super::TerrainEditRecorder>,
) -> WaterPaintConversion {
    let mut out = WaterPaintConversion::default();
    let Some(grid) = CellGrid::of(config, data) else { return out };
    if !data.has_material_layer() {
        return out;
    }
    let (width, height) = (grid.width, grid.height);
    let water_slot = TerrainMaterial::Water.to_u8();
    let painted: Vec<bool> = data
        .material_cache
        .iter()
        .map(|cell| {
            material_cell_weights(*cell)
                .into_iter()
                .filter(|(_, weight)| *weight > 0.0)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .is_some_and(|(slot, _)| slot == water_slot)
        })
        .collect();
    if !painted.iter().any(|p| *p) {
        return out;
    }
    super::api::size_water(water, data);
    let ground = |data: &TerrainData, i: usize| config.world_height(data.height_cache[i]);
    let neighbours = |i: usize| {
        let (x, z) = (i % width, i / width);
        [
            (x > 0).then(|| i - 1),
            (x + 1 < width).then(|| i + 1),
            (z > 0).then(|| i - width),
            (z + 1 < height).then(|| i + width),
        ]
    };

    let mut seen = vec![false; painted.len()];
    let mut body = Vec::new();
    let mut stack = Vec::new();
    for start in 0..painted.len() {
        if !painted[start] || seen[start] || data.cell_is_hole(start) {
            continue;
        }
        // Gather the body and its lowest rim.
        body.clear();
        stack.push(start);
        seen[start] = true;
        let mut rim = f32::INFINITY;
        while let Some(i) = stack.pop() {
            body.push(i);
            for n in neighbours(i).into_iter().flatten() {
                if data.cell_is_hole(n) {
                    continue;
                }
                if painted[n] {
                    if !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                } else {
                    rim = rim.min(ground(data, n));
                }
            }
        }
        let level = if rim.is_finite() { rim } else { body.iter().map(|&i| ground(data, i)).fold(f32::NEG_INFINITY, f32::max) };
        let mut took = false;
        for &i in &body {
            let depth = level - ground(data, i);
            if !(depth > 0.0) {
                out.left_painted += 1;
                continue;
            }
            let p = grid.world(i % width, i / width);
            if let Some(recorder) = recorder.as_deref_mut() {
                recorder.record_world_point(config, data, p.x, p.y);
                recorder.record_water_rect(config, data, Some(&*water), p, p);
            }
            data.material_cache[i] = super::material::material_cell(seabed_material(depth).to_u8());
            if !(water.levels[i] >= level) {
                water.levels[i] = level;
            }
            out.cells += 1;
            out.bounds = Some(match out.bounds {
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
                None => (p, p),
            });
            took = true;
        }
        if took {
            out.bodies += 1;
        }
    }
    if out.cells > 0 {
        data.material_dirty = true;
    }
    out
}

// ============================================================================
// Saved water
// ============================================================================

/// The file beside `_terrain.toml` that keeps a disk terrain's water.
///
/// Layout, little-endian: the magic `EWTR`, a u16 format version, u16 flags
/// (bit 0: a colour follows, bit 1: a transparency follows), the raster
/// width and height as u32, the colour as three f32 and the transparency as
/// an f32 (zero when their flag is clear), then the levels: one f32 per
/// raster cell, row-major, NaN where the column is dry, lz4-compressed with
/// the byte length prepended.
pub const WATER_FILE_NAME: &str = "water.bin";
const WATER_MAGIC: [u8; 4] = *b"EWTR";
const WATER_VERSION: u16 = 1;
const WATER_HEADER_LEN: usize = 4 + 2 + 2 + 4 + 4 + 12 + 4;
const WATER_HAS_COLOR: u16 = 1;
const WATER_HAS_TRANSPARENCY: u16 = 2;

/// `Workspace/Terrain/water.bin` for the Space's `Workspace/Terrain` folder.
pub fn water_file_path(terrain_dir: &Path) -> PathBuf {
    terrain_dir.join(WATER_FILE_NAME)
}

/// Marks a terrain root whose `water.bin` was there on load but could not be
/// used (unreadable, damaged, a newer format, or built for another raster).
/// Save leaves the file alone while the terrain holds no water of its own,
/// so a file this build cannot read is never deleted by a save that had
/// nothing to put in its place.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct UnreadWaterFile;

impl TerrainVoxelWater {
    /// True when no column holds water and no colour or transparency is set:
    /// nothing a save needs to keep.
    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.transparency.is_none() && self.levels.iter().all(|level| level.is_nan())
    }

    /// World Y of the water over the raster cell of `data` (the terrain of
    /// `config`) nearest world `p`, `None` where that column holds none, off
    /// the raster, or when the levels were built for another raster.
    pub fn level_at(&self, config: &TerrainConfig, data: &TerrainData, p: Vec2) -> Option<f32> {
        if self.width != data.cache_width || self.height != data.cache_height {
            return None;
        }
        let (x, z) = CellGrid::of(config, data)?.cell_at(p)?;
        self.levels.get(z * self.width as usize + x).copied().filter(|level| level.is_finite())
    }
}

/// The `water.bin` bytes of `water`. Fails when its levels do not cover its
/// raster exactly.
pub fn encode_voxel_water(water: &TerrainVoxelWater) -> Result<Vec<u8>, String> {
    let cells = water.width as usize * water.height as usize;
    if water.levels.len() != cells {
        return Err(format!(
            "water holds {} levels for a {} x {} raster",
            water.levels.len(),
            water.width,
            water.height
        ));
    }
    let mut raw = Vec::with_capacity(cells * 4);
    for &level in &water.levels {
        // One NaN pattern, so dry runs compress to almost nothing.
        let level = if level.is_nan() { f32::NAN } else { level };
        raw.extend_from_slice(&level.to_le_bytes());
    }
    let packed = lz4_flex::compress_prepend_size(&raw);

    let mut flags = 0u16;
    if water.color.is_some() {
        flags |= WATER_HAS_COLOR;
    }
    if water.transparency.is_some() {
        flags |= WATER_HAS_TRANSPARENCY;
    }
    let mut out = Vec::with_capacity(WATER_HEADER_LEN + packed.len());
    out.extend_from_slice(&WATER_MAGIC);
    out.extend_from_slice(&WATER_VERSION.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&water.width.to_le_bytes());
    out.extend_from_slice(&water.height.to_le_bytes());
    for channel in water.color.unwrap_or([0.0; 3]) {
        out.extend_from_slice(&channel.to_le_bytes());
    }
    out.extend_from_slice(&water.transparency.unwrap_or(0.0).to_le_bytes());
    out.extend_from_slice(&packed);
    Ok(out)
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_f32(bytes: &[u8], at: usize) -> f32 {
    f32::from_bits(read_u32(bytes, at))
}

/// The water `water.bin` bytes hold. `raster`, when given, is the width and
/// height the levels must have: a file for another raster is refused before
/// its levels are decompressed, so a damaged size can never drive a huge
/// allocation.
pub fn decode_voxel_water(bytes: &[u8], raster: Option<(u32, u32)>) -> Result<TerrainVoxelWater, String> {
    if bytes.len() < WATER_HEADER_LEN + 4 {
        return Err(format!("the water file is {} bytes, shorter than its header", bytes.len()));
    }
    if bytes[..4] != WATER_MAGIC[..] {
        return Err("the water file does not start with the EWTR magic".to_string());
    }
    let version = read_u16(bytes, 4);
    if version != WATER_VERSION {
        return Err(format!("the water file is format version {version}, this build reads {WATER_VERSION}"));
    }
    let flags = read_u16(bytes, 6);
    let (width, height) = (read_u32(bytes, 8), read_u32(bytes, 12));
    if let Some((raster_width, raster_height)) = raster {
        if (width, height) != (raster_width, raster_height) {
            return Err(format!(
                "the water was saved for a {width} x {height} raster, the terrain's is {raster_width} x {raster_height}"
            ));
        }
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|cells| cells.checked_mul(4))
        .ok_or_else(|| format!("a {width} x {height} water raster is too large"))?;
    let declared = read_u32(bytes, WATER_HEADER_LEN) as usize;
    if declared != expected {
        return Err(format!("the water levels declare {declared} bytes, expected {expected}"));
    }
    let raw = lz4_flex::decompress_size_prepended(&bytes[WATER_HEADER_LEN..])
        .map_err(|error| format!("the water levels failed to decompress: {error}"))?;
    if raw.len() != expected {
        return Err(format!("the water levels are {} bytes, expected {expected}", raw.len()));
    }
    let levels = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let color = (flags & WATER_HAS_COLOR != 0).then(|| [read_f32(bytes, 16), read_f32(bytes, 20), read_f32(bytes, 24)]);
    let transparency = (flags & WATER_HAS_TRANSPARENCY != 0).then(|| read_f32(bytes, 28));
    Ok(TerrainVoxelWater { levels, width, height, color, transparency })
}

/// Read `water.bin` from a Space's `Workspace/Terrain` folder for a terrain
/// whose raster is `data`'s. `Ok(None)` when there is no file; `Err` when
/// one is there but cannot be used, which the caller reports and marks with
/// [`UnreadWaterFile`].
pub fn load_voxel_water(terrain_dir: &Path, data: &TerrainData) -> Result<Option<TerrainVoxelWater>, String> {
    let path = water_file_path(terrain_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {path:?}: {error}")),
    };
    decode_voxel_water(&bytes, Some((data.cache_width, data.cache_height)))
        .map(Some)
        .map_err(|error| format!("{path:?}: {error}"))
}

/// What [`save_voxel_water`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterSave {
    /// The water was written.
    Written,
    /// The terrain holds no water, so the file was deleted (or was never there).
    Removed,
    /// The terrain holds no water and the file is one the load could not
    /// use, so it was left alone.
    Kept,
}

/// Write the terrain's water to `water.bin` in its `Workspace/Terrain`
/// folder, or delete the file when there is no water to keep, so a reload
/// cannot bring back water that was undone or drained. `unread_file` is
/// whether the root carries [`UnreadWaterFile`]: a file the load could not
/// use is then kept while there is no water to replace it with.
///
/// The file is written beside its target and renamed over it, so an
/// interrupted save leaves the old water or the new, never a torn file.
pub fn save_voxel_water(
    terrain_dir: &Path,
    water: Option<&TerrainVoxelWater>,
    unread_file: bool,
) -> Result<WaterSave, String> {
    let path = water_file_path(terrain_dir);
    let Some(water) = water.filter(|water| !water.is_empty()) else {
        if unread_file {
            return Ok(WaterSave::Kept);
        }
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(WaterSave::Removed),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(WaterSave::Removed),
            Err(error) => Err(format!("Failed to delete the drained water file {path:?}: {error}")),
        };
    };
    let bytes = encode_voxel_water(water)?;
    let staging = terrain_dir.join(format!("{WATER_FILE_NAME}.tmp"));
    std::fs::write(&staging, bytes).map_err(|error| format!("Failed to write the water file {staging:?}: {error}"))?;
    if let Err(error) = std::fs::rename(&staging, &path) {
        let _ = std::fs::remove_file(&staging);
        return Err(format!("Failed to move the water file into place at {path:?}: {error}"));
    }
    Ok(WaterSave::Written)
}

// ============================================================================
// ECS
// ============================================================================

/// Marks one block's surface of one body of the water imported with the
/// voxel terrain of the terrain root `root`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelWaterSurface {
    pub root: Entity,
}

/// What one root's imported water was last built from.
#[derive(Debug)]
struct BuiltVoxelWater {
    /// The grid the surfaces stand on.
    grid: TerrainGridKey,
    /// The raster size the levels were matched against.
    raster: UVec2,
    surfaces: Vec<Entity>,
}

/// Bookkeeping of [`sync_voxel_water`]: the surfaces built for each terrain
/// root.
#[derive(Resource, Debug, Default)]
pub struct VoxelWaterState {
    built: HashMap<Entity, BuiltVoxelWater>,
    /// Roots whose water changed while a rebuild was held back.
    pending: std::collections::HashSet<Entity>,
    /// `Time<Real>` seconds of the last build, `None` before the first.
    last_build: Option<f64>,
}

/// Seconds between rebuilds of water that keeps changing (a water brush
/// stroke re-marks it every dab); the surfaces standing keep drawing.
const VOXEL_WATER_REBUILD_SECS: f64 = 0.1;

impl VoxelWaterState {
    /// The surfaces built for the imported water of terrain root `root`.
    pub fn surfaces_of(&self, root: Entity) -> &[Entity] {
        self.built.get(&root).map(|built| built.surfaces.as_slice()).unwrap_or(&[])
    }
}

fn despawn_surfaces(commands: &mut Commands, surfaces: &[Entity]) {
    for surface in surfaces {
        commands.entity(*surface).try_despawn();
    }
}

/// Keep the water of every terrain root's [`TerrainVoxelWater`] drawn (see
/// the module docs): built when the component arrives, built again when it
/// changes, when the root's grid or raster changes, or when something else
/// despawned one of its surfaces, and despawned with the component or the
/// root. Runs after `apply_terrain_dirty_chunks`, as the lakes do, so a build
/// reads the ground that pass just edited or baked.
#[allow(clippy::too_many_arguments)]
pub fn sync_voxel_water(
    mut commands: Commands,
    time: Option<Res<Time<Real>>>,
    mut state: ResMut<VoxelWaterState>,
    roots: Query<(Entity, &TerrainConfig, &TerrainData, Option<&TerrainBaked>, Ref<TerrainVoxelWater>), With<TerrainRoot>>,
    live: Query<(), With<VoxelWaterSurface>>,
    mut removed: RemovedComponents<VoxelWaterSurface>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<WaterSurfaceMaterial>>>,
    mut water_assets: ResMut<WaterSurfaceAssets>,
) {
    // Only a surface despawned since the last run (this system's own old
    // ones included) can leave a build short, so only then is one looked
    // for.
    let lost_any = !removed.is_empty();
    removed.clear();

    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        // A host that draws nothing.
        if !state.built.is_empty() {
            for (_, built) in state.built.drain() {
                despawn_surfaces(&mut commands, &built.surfaces);
            }
        }
        return;
    };

    // A root gone, or its water: its surfaces go at once. Read through
    // `Deref` first so an idle frame leaves the resource alone.
    if state.built.keys().any(|root| !roots.contains(*root)) {
        state.built.retain(|root, built| {
            let keep = roots.contains(*root);
            if !keep {
                despawn_surfaces(&mut commands, &built.surfaces);
            }
            keep
        });
    }

    let now = time.map(|time| time.elapsed_secs_f64());
    for (root, config, base, baked, water) in &roots {
        let grid = TerrainGridKey::of(config);
        let raster = UVec2::new(base.cache_width, base.cache_height);
        let changed = water.is_changed() || state.pending.contains(&root);
        let current = state.built.get(&root).is_some_and(|built| {
            built.grid == grid
                && built.raster == raster
                && (!lost_any || built.surfaces.iter().all(|surface| live.contains(*surface)))
        });
        if !changed && current {
            continue;
        }
        // Changed water over surfaces that still fit waits out the interval.
        if current && now.zip(state.last_build).is_some_and(|(now, last)| now - last < VOXEL_WATER_REBUILD_SECS) {
            state.pending.insert(root);
            continue;
        }
        state.pending.remove(&root);
        state.last_build = now;

        let started = Instant::now();
        let ground = surface_data(base, baked);
        let fills = voxel_water_fills(config, ground, &water);
        if fills.is_empty() && !water.levels.is_empty() && (water.width, water.height) != (raster.x, raster.y) {
            warn!(
                target: "eustress::terrain::water",
                width = water.width,
                height = water.height,
                raster_width = raster.x,
                raster_height = raster.y,
                "imported water was built for another raster than the terrain's; none is drawn"
            );
        }
        let mut surfaces = Vec::new();
        if !fills.is_empty() {
            let (color, alpha) = voxel_water_tint(&water);
            let material = water_assets.tinted_material(&mut materials, color, alpha);
            let block_size = VOXEL_WATER_BLOCK_CHUNKS as f32 * config.chunk_size;
            for fill in &fills {
                for (block, data) in merge_water_blocks(water_body_meshes(config, ground, fill), config.chunk_size) {
                    let corner = block.as_vec2() * block_size;
                    let surface = commands
                        .spawn((
                            Name::new(format!("ImportedWater_{}_{}", block.x, block.y)),
                            VoxelWaterSurface { root },
                            WaterSurface,
                            // Translucent water casts no shadow onto the
                            // ground it shows.
                            NotShadowCaster,
                            Mesh3d(meshes.add(data.into_mesh())),
                            MeshMaterial3d(material.clone()),
                            Transform::from_xyz(corner.x, fill.level, corner.y),
                            Visibility::default(),
                        ))
                        .id();
                    surfaces.push(surface);
                }
            }
            info!(
                target: "eustress::terrain::water",
                "Imported water: {} bodies in {} surfaces, built in {:.0} ms",
                fills.len(),
                surfaces.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        // The old surfaces go in the same command flush the new ones spawn
        // in, so the water never blinks out.
        if let Some(old) = state.built.insert(root, BuiltVoxelWater { grid, raster, surfaces }) {
            despawn_surfaces(&mut commands, &old.surfaces);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 3 x 3 chunks of 32 m at `resolution` cells: a raster `3 * resolution`
    /// cells a side spanning world -32..64 on both axes, heights 0..100 m.
    fn config(resolution: u32) -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: resolution,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            height_offset: 0.0,
            ..TerrainConfig::default()
        }
    }

    /// A raster standing `height` m high everywhere.
    fn flat_ground(config: &TerrainConfig, height: f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let normalized = config.normalized_height(height);
        data.height_cache.iter_mut().for_each(|h| *h = normalized);
        data
    }

    /// Water over the raster of `data` at `level(x, z)` in each column.
    fn water_with(data: &TerrainData, level: impl Fn(usize, usize) -> f32) -> TerrainVoxelWater {
        let (width, height) = (data.cache_width as usize, data.cache_height as usize);
        let mut levels = Vec::with_capacity(width * height);
        for z in 0..height {
            for x in 0..width {
                levels.push(level(x, z));
            }
        }
        TerrainVoxelWater { levels, width: data.cache_width, height: data.cache_height, color: None, transparency: None }
    }

    fn same_water(a: &TerrainVoxelWater, b: &TerrainVoxelWater) -> bool {
        (a.width, a.height, a.color, a.transparency) == (b.width, b.height, b.color, b.transparency)
            && a.levels.len() == b.levels.len()
            && a.levels.iter().zip(&b.levels).all(|(x, y)| (x.is_nan() && y.is_nan()) || x == y)
    }

    #[test]
    fn water_paint_becomes_seabed_under_real_water_up_to_its_rim() {
        use crate::terrain::material::material_cell;
        let config = config(8);
        // Ground at 5 m, a painted pond 2 m deep in the middle, and a painted
        // strip climbing out of it to the east, whose top stands above the
        // pond's rim.
        let mut data = flat_ground(&config, 5.0);
        let (w, h) = (data.cache_width as usize, data.cache_height as usize);
        data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); w * h];
        let water_paint = material_cell(TerrainMaterial::Water.to_u8());
        let (cx, cz) = (w / 2, h / 2);
        for z in cz - 2..=cz + 2 {
            for x in cx - 2..=cx + 2 {
                data.height_cache[z * w + x] = config.normalized_height(3.0);
                data.material_cache[z * w + x] = water_paint;
            }
        }
        for (step, x) in (cx + 3..cx + 8).enumerate() {
            data.height_cache[cz * w + x] = config.normalized_height(3.5 + step as f32);
            data.material_cache[cz * w + x] = water_paint;
        }
        let mut water = TerrainVoxelWater::default();
        let done = convert_water_paint(&config, &mut data, &mut water, None);

        assert_eq!(done.bodies, 1, "the pond and its strip are one body");
        assert_eq!((water.width, water.height), (data.cache_width, data.cache_height), "the water is sized to the raster");
        let pond = cz * w + cx;
        assert!((water.levels[pond] - 5.0).abs() < 1e-4, "the pond fills to its rim, the ground around it");
        assert_eq!(data.material_cache[pond], material_cell(TerrainMaterial::Sand.to_u8()), "2 m down is sand");
        // Strip cells below 5 m take water; those at or above keep the paint.
        let low = cz * w + cx + 3;
        assert!((water.levels[low] - 5.0).abs() < 1e-4);
        let high = cz * w + cx + 7;
        assert!(water.levels[high].is_nan(), "7.5 m of ground stands above the pond");
        assert_eq!(data.material_cache[high], water_paint, "and keeps its paint");
        assert_eq!(done.cells + done.left_painted, 30);
        assert!(done.left_painted >= 2);
        assert!(data.material_dirty);

        // Nothing painted, nothing done.
        let mut plain = flat_ground(&config, 5.0);
        plain.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); w * h];
        assert_eq!(convert_water_paint(&config, &mut plain, &mut TerrainVoxelWater::default(), None).cells, 0);
    }

    #[test]
    fn saved_water_reads_back_exactly_and_damage_is_refused() {
        let data = flat_ground(&config(8), 2.0);
        let mut water = water_with(&data, |x, z| if x < 3 && z > 1 { 4.5 + x as f32 * 0.25 } else { f32::NAN });
        water.color = Some([0.1, 0.4, 0.6]);
        water.transparency = Some(0.3);
        let bytes = encode_voxel_water(&water).expect("the water encodes");
        let raster = Some((data.cache_width, data.cache_height));
        let back = decode_voxel_water(&bytes, raster).expect("the water decodes");
        assert!(same_water(&water, &back), "the water reads back as it was saved");

        let plain = water_with(&data, |_, _| 3.0);
        let back = decode_voxel_water(&encode_voxel_water(&plain).unwrap(), raster).unwrap();
        assert_eq!((back.color, back.transparency), (None, None), "no tint stays no tint");
        assert!(same_water(&plain, &back));

        // Another raster, a wrong magic or version, a damaged size and a
        // cut-off file are all refused.
        assert!(decode_voxel_water(&bytes, Some((data.cache_width + 1, data.cache_height))).is_err());
        let mut magic = bytes.clone();
        magic[0] = b'X';
        assert!(decode_voxel_water(&magic, raster).is_err());
        let mut version = bytes.clone();
        version[4] = 9;
        assert!(decode_voxel_water(&version, raster).is_err());
        let mut size = bytes.clone();
        size[WATER_HEADER_LEN] ^= 0x40;
        assert!(decode_voxel_water(&size, raster).is_err());
        assert!(decode_voxel_water(&bytes[..bytes.len() - 3], raster).is_err());
        assert!(decode_voxel_water(&bytes[..10], raster).is_err());

        // Levels that do not cover the raster are never written.
        let short = TerrainVoxelWater { levels: vec![1.0; 3], ..water.clone() };
        assert!(encode_voxel_water(&short).is_err());
    }

    #[test]
    fn a_save_writes_the_water_and_deletes_it_once_drained() {
        let dir = std::env::temp_dir().join(format!("eustress_voxel_water_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let data = flat_ground(&config(8), 2.0);
        assert!(matches!(load_voxel_water(&dir, &data), Ok(None)), "no file, no water");

        let water = water_with(&data, |x, _| if x > 4 { 6.0 } else { f32::NAN });
        assert_eq!(save_voxel_water(&dir, Some(&water), false), Ok(WaterSave::Written));
        let loaded = load_voxel_water(&dir, &data).expect("the file loads").expect("it holds water");
        assert!(same_water(&water, &loaded));
        assert!(!dir.join(format!("{WATER_FILE_NAME}.tmp")).exists(), "no staging file is left behind");

        // Drained water, or none at all, deletes the file.
        let dry = water_with(&data, |_, _| f32::NAN);
        assert!(dry.is_empty());
        assert_eq!(save_voxel_water(&dir, Some(&dry), false), Ok(WaterSave::Removed));
        assert!(!water_file_path(&dir).exists());
        assert_eq!(save_voxel_water(&dir, None, false), Ok(WaterSave::Removed), "deleting nothing is fine");

        // A tint on dry ground is still worth keeping.
        let tinted = TerrainVoxelWater { color: Some([1.0, 0.0, 0.0]), ..dry.clone() };
        assert!(!tinted.is_empty());
        assert_eq!(save_voxel_water(&dir, Some(&tinted), false), Ok(WaterSave::Written));

        // A file the load could not use survives a save with no water, and
        // is replaced by one with water.
        std::fs::write(water_file_path(&dir), b"EWTR from a future build").unwrap();
        assert!(load_voxel_water(&dir, &data).is_err(), "a damaged file is reported");
        assert_eq!(save_voxel_water(&dir, None, true), Ok(WaterSave::Kept));
        assert!(water_file_path(&dir).exists());
        assert_eq!(save_voxel_water(&dir, Some(&water), true), Ok(WaterSave::Written));
        assert!(load_voxel_water(&dir, &data).unwrap().is_some());

        // Water saved for another raster is reported, not loaded.
        let wider = flat_ground(&config(16), 2.0);
        assert!(load_voxel_water(&dir, &wider).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn each_body_of_water_becomes_one_fill_over_its_own_columns() {
        // A 6 x 6 raster: a pond at 2 m in the north-west, one more column at
        // the same level far to the south (a body of its own), a pool at 5 m
        // in the east, one column of infinite water, and dry columns
        // everywhere else.
        let config = config(2);
        let ground = flat_ground(&config, 0.0);
        assert_eq!((ground.cache_width, ground.cache_height), (6, 6));
        let pond: [(usize, usize); 4] = [(1, 1), (2, 1), (1, 2), (2, 2)];
        let lone = (0usize, 5usize);
        let pool: [(usize, usize); 3] = [(4, 2), (4, 3), (5, 3)];
        let water = water_with(&ground, |x, z| {
            if pond.contains(&(x, z)) || (x, z) == lone {
                2.0
            } else if pool.contains(&(x, z)) {
                5.0
            } else if (x, z) == (3, 0) {
                f32::INFINITY
            } else {
                f32::NAN
            }
        });
        let fills = voxel_water_fills(&config, &ground, &water);
        assert_eq!(fills.len(), 3, "one fill per body");
        let (pond_fill, lone_fill, pool_fill) = (&fills[0], &fills[1], &fills[2]);
        assert_eq!(
            (pond_fill.level, lone_fill.level, pool_fill.level),
            (2.0, 2.0, 5.0),
            "lowest first, then in raster order"
        );
        assert_eq!((pond_fill.wet_count, lone_fill.wet_count, pool_fill.wet_count), (4, 1, 3));

        // Each mask is cropped to its own columns.
        assert_eq!((pond_fill.origin, pond_fill.size), (UVec2::new(1, 1), UVec2::new(2, 2)));
        assert_eq!(pond_fill.wet, vec![true; 4]);
        assert_eq!((lone_fill.origin, lone_fill.size), (UVec2::new(0, 5), UVec2::new(1, 1)));
        assert_eq!(lone_fill.wet, vec![true]);
        assert_eq!((pool_fill.origin, pool_fill.size), (UVec2::new(4, 2), UVec2::new(2, 2)));
        assert_eq!(pool_fill.wet, vec![true, false, true, true]);
        for z in 0..6u32 {
            for x in 0..6u32 {
                let column = (x as usize, z as usize);
                assert_eq!(pond_fill.is_wet(x, z), pond.contains(&column), "({x}, {z}) in the pond");
                assert_eq!(lone_fill.is_wet(x, z), column == lone, "({x}, {z}) in the lone column");
                assert_eq!(pool_fill.is_wet(x, z), pool.contains(&column), "({x}, {z}) in the pool");
            }
        }

        // The world mapping of a lake's flood.
        let grid = CellGrid::of(&config, &ground).expect("a whole raster");
        assert_eq!(pond_fill.bounds, (grid.world(1, 1), grid.world(2, 2)));
        assert_eq!(pool_fill.bounds, (grid.world(4, 2), grid.world(5, 3)));
        assert_eq!((pond_fill.cell, pool_fill.cell), (grid.step, grid.step));

        // Levels built for another raster draw nothing.
        let narrower = TerrainVoxelWater { width: 5, ..water.clone() };
        assert!(voxel_water_fills(&config, &ground, &narrower).is_empty());
        let taller = TerrainVoxelWater { height: 7, ..water.clone() };
        assert!(voxel_water_fills(&config, &ground, &taller).is_empty());
        let short = TerrainVoxelWater { levels: water.levels[..30].to_vec(), ..water.clone() };
        assert!(voxel_water_fills(&config, &ground, &short).is_empty());
        // Nor does terrain without a raster.
        let none = TerrainVoxelWater::default();
        assert!(voxel_water_fills(&config, &TerrainData::procedural(), &none).is_empty());
    }

    #[test]
    fn neighbouring_columns_within_the_join_distance_are_one_body() {
        let config = config(2);
        let ground = flat_ground(&config, 0.0);
        let pair = |a: f32, b: f32| {
            water_with(&ground, move |x, z| match (x, z) {
                (1, 1) => a,
                (2, 1) => b,
                _ => f32::NAN,
            })
        };

        let joined = voxel_water_fills(&config, &ground, &pair(1.0, 1.004));
        assert_eq!(joined.len(), 1, "4 mm apart: one body");
        assert_eq!(joined[0].wet_count, 2);
        assert!(joined[0].is_wet(1, 1) && joined[0].is_wet(2, 1));
        assert!((1.0f32..=1.004).contains(&joined[0].level), "the level {} is between its columns'", joined[0].level);

        // A step of half a metre is two bodies, each at its own level.
        let stepped = voxel_water_fills(&config, &ground, &pair(1.0, 1.5));
        assert_eq!(stepped.len(), 2, "half a metre apart: two bodies");
        assert_eq!((stepped[0].level, stepped[1].level), (1.0, 1.5));
        assert!(stepped.iter().all(|fill| fill.wet_count == 1));
        assert!(stepped[0].is_wet(1, 1) && stepped[1].is_wet(2, 1));

        // Shore columns a few centimetres low join the sea without moving it:
        // it stands at its most common level.
        let sea = water_with(&ground, |x, z| match (x, z) {
            (0, _) if z < 3 => 1.95,
            (_, _) if z < 3 => 2.0,
            _ => f32::NAN,
        });
        let fills = voxel_water_fills(&config, &ground, &sea);
        assert_eq!(fills.len(), 1, "one sea");
        assert_eq!((fills[0].level, fills[0].wet_count), (2.0, 18));
    }

    #[test]
    fn past_the_cap_the_bodies_holding_the_most_water_are_drawn() {
        // Body k: row k, columns 0 to k, at level k, for six more bodies than
        // are drawn. Rows a metre apart never join.
        let config = config(96);
        let ground = flat_ground(&config, 0.0);
        let bodies = MAX_VOXEL_WATER_BODIES + 6;
        assert!(bodies <= ground.cache_width as usize && bodies <= ground.cache_height as usize);
        let water = water_with(&ground, |x, z| if z < bodies && x <= z { z as f32 } else { f32::NAN });

        let fills = voxel_water_fills(&config, &ground, &water);
        assert_eq!(fills.len(), MAX_VOXEL_WATER_BODIES);
        // Bodies 0 to 5, holding one to six columns, are left out.
        assert_eq!((fills[0].level, fills[0].wet_count), (6.0, 7));
        assert!(fills.windows(2).all(|pair| pair[0].level < pair[1].level), "sorted by level");
        assert!(fills.iter().all(|fill| fill.wet_count == fill.level as usize + 1));
    }

    #[test]
    fn chunk_meshes_merge_into_blocks_without_moving_a_quad() {
        // A pool at 3 m straddling the chunk border at x = 0.
        let config = config(32);
        let ground = flat_ground(&config, 1.0);
        let grid = CellGrid::of(&config, &ground).expect("a whole raster");
        let water = water_with(&ground, |x, z| {
            let p = grid.world(x, z);
            if p.x.abs() < 10.0 && (p.y - 16.0).abs() < 6.0 { 3.0 } else { f32::NAN }
        });
        let fill = voxel_water_fills(&config, &ground, &water).remove(0);
        let chunks = water_body_meshes(&config, &ground, &fill);
        assert!(chunks.len() >= 2);

        // Every quad as its world box, in millimetres.
        let world_quads = |corner: Vec2, mesh: &WaterMeshData| -> Vec<[i64; 4]> {
            mesh.positions
                .chunks(4)
                .map(|quad| {
                    let (lo, hi) = (Vec2::new(quad[0][0], quad[0][2]) + corner, Vec2::new(quad[3][0], quad[3][2]) + corner);
                    [lo.x, lo.y, hi.x, hi.y].map(|v| (v * 1000.0).round() as i64)
                })
                .collect()
        };
        let mut before: Vec<[i64; 4]> =
            chunks.iter().flat_map(|(chunk, mesh)| world_quads(chunk.as_vec2() * config.chunk_size, mesh)).collect();
        let blocks = merge_water_blocks(chunks.clone(), config.chunk_size);
        let block_size = VOXEL_WATER_BLOCK_CHUNKS as f32 * config.chunk_size;
        let mut after: Vec<[i64; 4]> =
            blocks.iter().flat_map(|(block, mesh)| world_quads(block.as_vec2() * block_size, mesh)).collect();
        before.sort_unstable();
        after.sort_unstable();
        assert_eq!(before, after, "the same quads in the same places");
        // Chunks -1 and 0 fall in blocks -1 and 0.
        assert_eq!(blocks.iter().map(|(block, _)| *block).collect::<Vec<_>>(), vec![IVec2::new(-1, 0), IVec2::new(0, 0)]);
        for (_, mesh) in &blocks {
            assert_eq!(mesh.indices.len() / 6, mesh.positions.len() / 4, "six indices per four-vertex quad");
            assert!(mesh.indices.iter().all(|&index| (index as usize) < mesh.positions.len()));
        }
    }

    #[test]
    fn a_level_meshes_flat_per_chunk_through_the_lake_mesher() {
        // Flat ground at 1 m under a pool at 3 m, 20 x 12 columns across the
        // chunk border at x = 0.
        let config = config(32);
        let ground = flat_ground(&config, 1.0);
        let grid = CellGrid::of(&config, &ground).expect("a whole raster");
        let water = water_with(&ground, |x, z| {
            let p = grid.world(x, z);
            if p.x.abs() < 10.0 && (p.y - 16.0).abs() < 6.0 { 3.0 } else { f32::NAN }
        });
        let fills = voxel_water_fills(&config, &ground, &water);
        assert_eq!(fills.len(), 1);
        let fill = &fills[0];
        assert_eq!((fill.level, fill.wet_count), (3.0, 240));

        let meshes = water_body_meshes(&config, &ground, fill);
        assert!(meshes.len() >= 2, "the pool straddles a chunk border: {:?}", meshes.iter().map(|(c, _)| *c).collect::<Vec<_>>());
        let mut covered = 0.0f32;
        for (chunk, mesh) in &meshes {
            assert!(!mesh.indices.is_empty());
            let corner = chunk.as_vec2() * config.chunk_size;
            for quad in mesh.positions.chunks(4) {
                assert!(quad.iter().all(|p| p[1] == 0.0), "flat: the entity stands at the level");
                let (lo, hi) = (Vec2::new(quad[0][0], quad[0][2]) + corner, Vec2::new(quad[3][0], quad[3][2]) + corner);
                covered += (hi - lo).x * (hi - lo).y;
            }
        }
        // The ground is below the water everywhere, so no dry shore is drawn:
        // exactly the wet columns, one cell's area each.
        let wet_area = fill.wet_count as f32 * grid.step.x * grid.step.y;
        assert!((covered - wet_area).abs() < 0.05, "{covered} m2 drawn over {wet_area} m2 of water");
    }

    #[test]
    fn a_tint_is_one_material_however_often_it_is_asked_for() {
        let mut assets = WaterSurfaceAssets::default();
        let mut materials = Assets::<WaterSurfaceMaterial>::default();
        let shared = assets.material(&mut materials);
        assert_eq!(assets.tinted_material(&mut materials, None, None), shared, "no tint is the shared material");

        let red = assets.tinted_material(&mut materials, Some([1.0, 0.0, 0.0]), None);
        assert_ne!(red, shared);
        assert_eq!(assets.tinted_material(&mut materials, Some([1.0, 0.0, 0.0]), None), red, "cached by value");
        let clear = assets.tinted_material(&mut materials, None, Some(0.2));
        assert_ne!(clear, red);
        // A part that is not a number follows the shared material.
        assert_eq!(assets.tinted_material(&mut materials, Some([f32::NAN, 0.0, 0.0]), Some(0.2)), clear);
        assert_eq!(materials.len(), 3, "the shared material and two tints");

        let params = |handle: &Handle<WaterSurfaceMaterial>| materials.get(handle).expect("the material").extension.params;
        let (red, clear) = (params(&red), params(&clear));
        assert!((red.deep_color.x - 1.0).abs() < 1e-6 && red.deep_color.y.abs() < 1e-6 && red.deep_color.z.abs() < 1e-6);
        assert!((red.deep_color.w - params(&shared).deep_color.w).abs() < 1e-6, "a colour alone keeps the opacity");
        assert!((clear.deep_color.w - 0.2).abs() < 1e-6);
        assert_eq!(clear.deep_color.truncate(), params(&shared).deep_color.truncate(), "an opacity alone keeps the colour");

        // Imported transparency maps to an opacity, never below the least.
        let tint = |transparency: Option<f32>| voxel_water_tint(&TerrainVoxelWater { transparency, ..default() }).1;
        assert_eq!(tint(None), None);
        assert_eq!(tint(Some(f32::NAN)), None);
        assert!((tint(Some(0.3)).unwrap() - 0.7).abs() < 1e-6);
        assert_eq!(tint(Some(1.0)), Some(VOXEL_WATER_MIN_ALPHA));
        assert_eq!(tint(Some(-1.0)), Some(1.0));
    }

    #[test]
    fn imported_water_is_built_with_its_root_and_goes_with_it() {
        let config = config(32);
        let ground = flat_ground(&config, 1.0);
        let grid = CellGrid::of(&config, &ground).expect("a whole raster");
        let pool = |level: f32| {
            water_with(&ground, move |x, z| {
                let p = grid.world(x, z);
                if p.x.abs() < 10.0 && (p.y - 16.0).abs() < 6.0 { level } else { f32::NAN }
            })
        };

        let mut world = World::new();
        world.init_resource::<VoxelWaterState>();
        world.init_resource::<WaterSurfaceAssets>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<WaterSurfaceMaterial>>();
        let root = world.spawn((TerrainRoot, config.clone(), ground.clone(), pool(3.0))).id();
        // Registered once, so change detection and the removal reader carry
        // over from run to run as they do in a schedule.
        let system = world.register_system(sync_voxel_water);
        let run = |world: &mut World| assert!(world.run_system(system).is_ok(), "the imported water system runs");
        let surfaces = |world: &mut World| {
            let mut query = world.query::<(Entity, &VoxelWaterSurface, &Transform)>();
            let mut found: Vec<(Entity, Entity, f32)> =
                query.iter(world).map(|(entity, surface, transform)| (entity, surface.root, transform.translation.y)).collect();
            found.sort_by_key(|(entity, ..)| entity.to_bits());
            found
        };
        let material_of = |world: &World, surface: Entity| {
            world.get::<MeshMaterial3d<WaterSurfaceMaterial>>(surface).expect("a water material").0.clone()
        };

        run(&mut world);
        let first = surfaces(&mut world);
        assert!(first.len() >= 2, "one surface per block the pool covers: {first:?}");
        assert!(first.iter().all(|(_, owner, y)| *owner == root && *y == 3.0), "{first:?}");
        assert_eq!(world.resource::<VoxelWaterState>().surfaces_of(root).len(), first.len());
        for (surface, ..) in &first {
            assert!(world.get::<WaterSurface>(*surface).is_some(), "drawn with the water material");
        }
        // Neither a colour nor a transparency: the shared material.
        let shared = world.resource_scope(|world, mut assets: Mut<WaterSurfaceAssets>| {
            assets.material(&mut world.resource_mut::<Assets<WaterSurfaceMaterial>>())
        });
        assert!(first.iter().all(|(surface, ..)| material_of(&world, *surface) == shared));

        // Nothing changed: nothing is built.
        run(&mut world);
        assert_eq!(surfaces(&mut world), first);

        // New water: new surfaces at its level, in its tint.
        *world.get_mut::<TerrainVoxelWater>(root).expect("the water") =
            TerrainVoxelWater { color: Some([1.0, 0.0, 0.0]), transparency: Some(1.0), ..pool(4.0) };
        run(&mut world);
        let second = surfaces(&mut world);
        assert_eq!(second.len(), first.len());
        assert!(second.iter().all(|(entity, owner, y)| *owner == root && *y == 4.0 && first.iter().all(|(old, ..)| old != entity)));
        let tinted = material_of(&world, second[0].0);
        assert_ne!(tinted, shared, "a tinted variant");
        let params = world.resource::<Assets<WaterSurfaceMaterial>>().get(&tinted).expect("the tint").extension.params;
        assert!((params.deep_color.w - VOXEL_WATER_MIN_ALPHA).abs() < 1e-6, "fully clear water still shows");
        assert!((params.deep_color.x - 1.0).abs() < 1e-6 && params.deep_color.y.abs() < 1e-6, "red water");

        // A surface despawned by something else is built again.
        world.despawn(second[0].0);
        run(&mut world);
        assert_eq!(surfaces(&mut world).len(), second.len());

        // The water goes with its component, and with its root.
        world.entity_mut(root).remove::<TerrainVoxelWater>();
        run(&mut world);
        assert!(surfaces(&mut world).is_empty(), "no water without the component");
        world.entity_mut(root).insert(pool(3.0));
        run(&mut world);
        assert_eq!(surfaces(&mut world).len(), first.len());
        world.despawn(root);
        run(&mut world);
        assert!(surfaces(&mut world).is_empty(), "the root's water goes with it");
        assert!(world.resource::<VoxelWaterState>().surfaces_of(root).is_empty());
    }
}
