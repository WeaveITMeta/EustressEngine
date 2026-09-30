//! # Imported Roblox voxel terrain: the load window, the Terrain instance and the build
//!
//! The engine-free half of loading the voxel terrain a Roblox import carries.
//! Studio's loader (the engine's `terrain_voxel_load`) reads the chunk
//! records from the Space's world database; the Player reads the same records
//! from the files the importer writes beside it,
//! `Workspace/Terrain/voxel_chunks/chunk_<cx>_<cy>_<cz>.bin`
//! ([`read_voxel_chunk_files`]). Both hand them to [`build_from_chunks`],
//! which touches no Bevy world and so runs on a background task.
//!
//! ## The Terrain instance
//!
//! `Workspace/Terrain/_instance.toml` ([`parse_terrain_instance`]) gives the
//! unit the import was authored in (`[metadata] unit`), its water colour and
//! transparency (`[terrain]`), and whether it holds terrain at all: a Terrain
//! whose `[terrain] source` is `"none"` (the importer's mark for a place
//! without terrain, and what Clear leaves behind) loads nothing, whatever
//! voxels are still kept for it.
//!
//! ## The build
//!
//! [`build_from_chunks`]:
//!
//! 1. Sizes the voxel cell in the world from the unit
//!    ([`TerrainInstanceProps::unit`]): `voxel_extract::ROBLOX_CELL_STUDS`
//!    studs of it, 1.2192 m for an import
//!    in feet (1 stud = 1 ft), so the terrain lines up with the parts
//!    converted from the same unit. An import without the stamp stays in
//!    metres, as its parts do.
//! 2. Picks the load window from the distinct chunk columns `(cx, cz)`
//!    ([`load_window`]). Per axis, an import at most [`MAX_WINDOW_CHUNKS`]
//!    (255) chunks wide is centred on its columns' span; a wider one is
//!    centred on its median column, [`MAX_HALF_EXTENT_CHUNKS`] (127) chunks
//!    either side, unless the caller names a centre (see "Re-centring"
//!    below). 255 chunks of 32 cells is 8,160 raster texels, under the 8,192
//!    a material-map texture may be (`MAX_MATERIAL_MAP_SIDE`), so the
//!    imported ground keeps its textured material. Chunks outside the window
//!    are skipped and counted.
//! 3. Decodes each chunk in the window and runs the MULTI-SPAN column
//!    extractor ([`voxel_extract::fill_terrain_from_chunk`]): for every
//!    `(x, z)` column it writes the TOP surface height (raw world height, at
//!    the isosurface of the top solid cell and the cell above it) + the
//!    surface material's id (its material slot) into a `TerrainData`, and
//!    records each column's solid extent and water and each chunk's
//!    part-full bottom cells ([`voxel_extract::VoxelColumns`]).
//! 4. Once every chunk is in, finishes the surfaces whose cell above lives in
//!    the chunk stacked on top
//!    ([`voxel_extract::VoxelColumns::refine_surface_tops`]), builds the water
//!    level over every raster cell ([`voxel_extract::voxel_water_levels`]),
//!    gives the columns no chunk holds a height
//!    ([`voxel_extract::fill_hole_heights`]) and marks the raster sparse
//!    (`TerrainData::sparse_surface`), so meshes and colliders leave those
//!    columns out.
//! 5. Decodes the chunks around every cave a second time and carves the air
//!    under each column's top surface (caves, tunnels, the undersides of
//!    overhangs) into a `TerrainVolume` ([`voxel_extract::carve_voxel_caves`];
//!    its module docs cover the lattice alignment and what is left solid).
//!
//! The caller spawns one `TerrainRoot` with the built `TerrainConfig`,
//! `TerrainData` and `TerrainVolume` and, when the import holds water, the
//! [`TerrainVoxelWater`] with the levels and the import's water colour and
//! transparency; the terrain streaming chain meshes it as the camera moves.
//!
//! ## Re-centring
//!
//! An import wider than the window on some axis can have the window follow
//! the camera. [`recentre_target`] decides when: once the camera's chunk is
//! more than `MAX_HALF_EXTENT_CHUNKS - RECENTRE_MARGIN_CHUNKS` (111) chunks
//! from the window's centre along an axis wider than the window, the window
//! centres on the camera's chunk, held inside the import's chunk column
//! bounding box, along every such axis; an axis the window spans whole keeps
//! its centre. A rebuild with that centre ([`build_from_chunks`] with
//! `window_center`) replaces the terrain. Studio re-centres; the Player keeps
//! the window its Space opened with.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use bevy::math::{IVec2, IVec3, UVec2};
use tracing::{debug, warn};

use super::surface_material::MAX_MATERIAL_MAP_SIDE;
use super::voxel_extract;
use super::voxel_water::TerrainVoxelWater;
use super::{TerrainConfig, TerrainData, TerrainVolume};
use crate::units::Unit;

/// Most chunks either side of the load window's centre chunk: the widest
/// grid whose raster, `(2 * half + 1) * 32` texels, still fits a
/// material-map texture (`MAX_MATERIAL_MAP_SIDE`, 8,192 texels), so imported
/// ground keeps its textured material. 127 chunks. The raster (heights,
/// material cells and water levels, four bytes each per texel) is sized by
/// the window, so this also bounds what a load can allocate.
pub const MAX_HALF_EXTENT_CHUNKS: u32 = (MAX_MATERIAL_MAP_SIDE / voxel_extract::CHUNK_EDGE as u32 - 1) / 2;

/// Most chunks one axis of the load window spans: 255 chunks, 8,160 raster
/// texels.
pub const MAX_WINDOW_CHUNKS: u32 = 2 * MAX_HALF_EXTENT_CHUNKS + 1;

/// How near, in chunks, the camera may come to the edge of what the last
/// build read before the window moves to follow it.
pub const RECENTRE_MARGIN_CHUNKS: u32 = 16;

/// The folder of a terrain directory the Roblox importer writes one file per
/// voxel chunk record into.
pub const VOXEL_CHUNKS_DIR: &str = "voxel_chunks";

/// The Terrain instance's file in a terrain directory.
pub const TERRAIN_INSTANCE_FILE: &str = "_instance.toml";

/// Log target of the voxel terrain build and its readers.
const LOG_TARGET: &str = "eustress::terrain::voxel";

/// The chunk grid a load covers (the grid of
/// [`voxel_extract::voxel_terrain_config`]), and the span of the chunk
/// columns it was chosen from, for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadWindow {
    /// Chunk the grid is centred on.
    pub center: IVec2,
    /// Chunks either side of `center`, along X and Z.
    pub half: UVec2,
    /// Smallest chunk column coordinate on each axis.
    pub columns_min: IVec2,
    /// Largest chunk column coordinate on each axis.
    pub columns_max: IVec2,
}

/// The load window over the distinct chunk columns `columns` of an import
/// (see the module docs), centred on `center` when one is given (moved as
/// little as [`clamp_window_center`] needs) and by the median rule otherwise;
/// `None` without a column.
pub fn load_window(columns: &HashSet<IVec2>, center: Option<IVec2>) -> Option<LoadWindow> {
    let columns_min = columns.iter().copied().reduce(IVec2::min)?;
    let columns_max = columns.iter().copied().reduce(IVec2::max)?;
    let mut xs: Vec<i32> = columns.iter().map(|column| column.x).collect();
    let mut zs: Vec<i32> = columns.iter().map(|column| column.y).collect();
    let (center_x, half_x) = window_axis(&mut xs, center.map(|center| center.x));
    let (center_z, half_z) = window_axis(&mut zs, center.map(|center| center.y));
    Some(LoadWindow {
        center: IVec2::new(center_x, center_z),
        half: UVec2::new(half_x, half_z),
        columns_min,
        columns_max,
    })
}

/// Centre and half extent, in chunks, of one axis of the load window, from
/// that axis's coordinate of every chunk column (`coords`, not empty).
/// Without a `center`, a span of at most [`MAX_WINDOW_CHUNKS`] is covered
/// whole, centred on it (rounding toward the smaller coordinate), and a
/// wider one gets the widest window, centred on its median column. A given
/// `center` is kept as far as [`clamp_window_center`] allows, with the widest
/// window over a wider span and the narrowest that covers a span it fits.
pub fn window_axis(coords: &mut [i32], center: Option<i32>) -> (i32, u32) {
    coords.sort_unstable();
    let (min, max) = (coords[0], coords[coords.len() - 1]);
    let wide = wider_than_window(min, max);
    match center {
        Some(center) => {
            let center = clamp_window_center(center, min, max);
            let reach = (center as i64 - min as i64).max(max as i64 - center as i64);
            let half = if wide { MAX_HALF_EXTENT_CHUNKS } else { reach.clamp(0, MAX_HALF_EXTENT_CHUNKS as i64) as u32 };
            (center, half)
        }
        None if !wide => {
            let center = (min as i64 + max as i64).div_euclid(2) as i32;
            (center, (center - min).max(max - center) as u32)
        }
        None => (coords[coords.len() / 2], MAX_HALF_EXTENT_CHUNKS),
    }
}

/// Whether the chunk columns `min..=max` of an axis span more than a load
/// window holds.
pub fn wider_than_window(min: i32, max: i32) -> bool {
    max as i64 - min as i64 >= MAX_WINDOW_CHUNKS as i64
}

/// `center` moved as little as keeps a window of [`MAX_HALF_EXTENT_CHUNKS`]
/// either side of it inside the chunk columns `min..=max` of an axis wider
/// than the window, or covering all of them on an axis the window spans
/// whole.
pub fn clamp_window_center(center: i32, min: i32, max: i32) -> i32 {
    let half = MAX_HALF_EXTENT_CHUNKS as i64;
    let (from_min, from_max) = (min as i64 + half, max as i64 - half);
    (center as i64).clamp(from_min.min(from_max), from_min.max(from_max)) as i32
}

/// Where the load window moves to follow a camera in chunk column `camera`,
/// or `None` to leave it at `center`. `half` is how far the window reaches
/// either side of `center` on each axis, and `bbox_min..=bbox_max` holds the
/// import's chunk columns. The window moves once the camera comes within
/// [`RECENTRE_MARGIN_CHUNKS`] of its reach along an axis wider than the
/// window; it then centres on the camera along every such axis, held inside
/// the import ([`clamp_window_center`]), and keeps its centre along an axis
/// it spans whole.
pub fn recentre_target(camera: IVec2, center: IVec2, half: UVec2, bbox_min: IVec2, bbox_max: IVec2) -> Option<IVec2> {
    let near_edge = |camera: i32, center: i32, half: u32| {
        (camera as i64 - center as i64).abs() > half as i64 - RECENTRE_MARGIN_CHUNKS as i64
    };
    let wide_x = wider_than_window(bbox_min.x, bbox_max.x);
    let wide_z = wider_than_window(bbox_min.y, bbox_max.y);
    let moved = (wide_x && near_edge(camera.x, center.x, half.x)) || (wide_z && near_edge(camera.y, center.y, half.y));
    if !moved {
        return None;
    }
    let target = IVec2::new(
        if wide_x { clamp_window_center(camera.x, bbox_min.x, bbox_max.x) } else { center.x },
        if wide_z { clamp_window_center(camera.y, bbox_min.y, bbox_max.y) } else { center.y },
    );
    (target != center).then_some(target)
}

/// What a load reads off the Terrain instance.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TerrainInstanceProps {
    /// The unit the import was authored in (`[metadata] unit`); metres when
    /// the file has no stamp or an unknown one. The build lays the cells out
    /// in it, so a host that reads parts without converting them from their
    /// authored unit sets metres here before building.
    pub unit: Unit,
    /// `[terrain] water_color`: sRGB, 0 to 1.
    pub water_color: Option<[f32; 3]>,
    /// `[terrain] water_transparency`: 0 opaque to 1 clear.
    pub water_transparency: Option<f32>,
    /// `[terrain] source` is `"none"`: the Terrain holds no terrain, whatever
    /// voxels are still kept for it.
    pub cleared: bool,
}

/// [`TerrainInstanceProps`] of the Terrain instance's TOML `text`. Text that
/// does not parse, and a missing table or key, leave the default.
pub fn parse_terrain_instance(text: &str) -> TerrainInstanceProps {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return TerrainInstanceProps::default();
    };
    let unit = match doc.get("metadata").and_then(|metadata| metadata.get("unit")).and_then(toml::Value::as_str) {
        Some(symbol) => Unit::from_symbol(symbol).unwrap_or_else(|| {
            warn!(
                target: LOG_TARGET,
                unit = symbol,
                "voxel-terrain load: unknown unit on the Terrain instance; the terrain loads in metres"
            );
            Unit::Meter
        }),
        None => Unit::Meter,
    };
    let terrain = doc.get("terrain");
    TerrainInstanceProps {
        unit,
        water_color: terrain.and_then(|terrain| terrain.get("water_color")).and_then(toml_color),
        water_transparency: terrain
            .and_then(|terrain| terrain.get("water_transparency"))
            .and_then(toml_number)
            .filter(|transparency| transparency.is_finite())
            .map(|transparency| transparency.clamp(0.0, 1.0)),
        cleared: terrain.and_then(|terrain| terrain.get("source")).and_then(toml::Value::as_str) == Some("none"),
    }
}

/// A TOML number, float or integer, as `f32`.
pub fn toml_number(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Float(float) => Some(*float as f32),
        toml::Value::Integer(integer) => Some(*integer as f32),
        _ => None,
    }
}

/// An sRGB colour from a TOML array of three numbers: floats are 0 to 1 (as
/// the importer writes them), integers 0 to 255 (the Terrain class default).
pub fn toml_color(value: &toml::Value) -> Option<[f32; 3]> {
    let [r, g, b] = value.as_array()?.as_slice() else {
        return None;
    };
    let channel = |component: &toml::Value| {
        let value = match component {
            toml::Value::Float(float) => *float as f32,
            toml::Value::Integer(integer) => *integer as f32 / 255.0,
            _ => return None,
        };
        value.is_finite().then(|| value.clamp(0.0, 1.0))
    };
    Some([channel(r)?, channel(g)?, channel(b)?])
}

/// The Terrain instance of terrain directory `terrain_dir`, read from its
/// `_instance.toml`; `Ok(None)` when there is no such file.
pub fn read_terrain_instance_file(terrain_dir: &Path) -> std::io::Result<Option<TerrainInstanceProps>> {
    match std::fs::read_to_string(terrain_dir.join(TERRAIN_INSTANCE_FILE)) {
        Ok(text) => Ok(Some(parse_terrain_instance(&text))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// The chunk coordinates in a voxel chunk file's name,
/// `chunk_<cx>_<cy>_<cz>.bin` with SIGNED decimal coordinates (the
/// importer's `format!("chunk_{}_{}_{}.bin", cx, cy, cz)`, so
/// `chunk_-4_0_-8.bin` is chunk `(-4, 0, -8)`); `None` for any other name.
pub fn voxel_chunk_file_coords(name: &str) -> Option<(i32, i32, i32)> {
    let coords = name.strip_prefix("chunk_")?.strip_suffix(".bin")?;
    // The '-' sign is part of a number, never a separator.
    let mut parts = coords.split('_');
    let (x, y, z) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    Some((x.parse().ok()?, y.parse().ok()?, z.parse().ok()?))
}

/// Whether the `voxel_chunks` folder of terrain directory `terrain_dir`
/// holds a voxel chunk file. Lists the folder as far as the first one and
/// reads no file.
pub fn has_voxel_chunk_files(terrain_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(terrain_dir.join(VOXEL_CHUNKS_DIR)) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_name().to_str().and_then(voxel_chunk_file_coords).is_some()
            && entry.file_type().is_ok_and(|kind| kind.is_file())
    })
}

/// Every voxel chunk record in the `voxel_chunks` folder of terrain
/// directory `terrain_dir`, as the `((cx, cy, cz), bytes)` pairs
/// [`build_from_chunks`] takes, sorted by coordinates. The bytes are the
/// importer's LZ4 records, the same ones Studio's world database holds.
///
/// `Ok` with no records when the folder is missing (most Spaces have no
/// imported terrain). Anything in the folder that is not a
/// `chunk_<cx>_<cy>_<cz>.bin` file is skipped with a debug note, and a chunk
/// file that cannot be read with a warning. `Err` when the folder is there
/// but cannot be listed.
pub fn read_voxel_chunk_files(terrain_dir: &Path) -> Result<Vec<((i32, i32, i32), Vec<u8>)>, String> {
    read_voxel_chunk_files_where(terrain_dir, |_| true)
}

/// [`read_voxel_chunk_files`] for the chunks whose coordinates `keep`
/// accepts: a file it refuses is never read, so a load window re-centred
/// over a wide import reads only the chunks it can hold.
pub fn read_voxel_chunk_files_where(
    terrain_dir: &Path,
    keep: impl Fn((i32, i32, i32)) -> bool,
) -> Result<Vec<((i32, i32, i32), Vec<u8>)>, String> {
    let dir = terrain_dir.join(VOXEL_CHUNKS_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("failed to list {}: {error}", dir.display())),
    };
    let mut chunks = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(
                    target: LOG_TARGET,
                    folder = %dir.display(),
                    error = %error,
                    "voxel chunk files: an entry of the folder could not be read; skipped"
                );
                continue;
            }
        };
        let path = entry.path();
        let coords = entry
            .file_name()
            .to_str()
            .and_then(voxel_chunk_file_coords)
            .filter(|_| entry.file_type().is_ok_and(|kind| kind.is_file()));
        let Some(coords) = coords else {
            debug!(
                target: LOG_TARGET,
                file = %path.display(),
                "voxel chunk files: not a chunk_<cx>_<cy>_<cz>.bin file; skipped"
            );
            continue;
        };
        if !keep(coords) {
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => chunks.push((coords, bytes)),
            Err(error) => warn!(
                target: LOG_TARGET,
                file = %path.display(),
                error = %error,
                "voxel chunk files: the chunk file could not be read; skipped"
            ),
        }
    }
    chunks.sort_unstable_by_key(|&(coords, _)| coords);
    Ok(chunks)
}

/// A voxel terrain built off the main thread, ready to spawn.
pub struct BuiltVoxelTerrain {
    pub config: TerrainConfig,
    pub data: TerrainData,
    pub volume: TerrainVolume,
    /// The water levels with the import's water style, when it holds water.
    pub water: Option<TerrainVoxelWater>,
    /// The chunk grid, and the chunk column bounding box of the chunks read:
    /// the whole import for a first build, the region for a re-centre.
    pub window: LoadWindow,
    pub stats: VoxelBuildStats,
}

/// What a build did, for the summary logged when it lands.
#[derive(Clone, Copy, Debug)]
pub struct VoxelBuildStats {
    /// Chunks handed to the build.
    pub chunks_read: usize,
    /// Chunks decoded into the terrain.
    pub chunks_filled: usize,
    /// Chunks whose bytes did not decode.
    pub decode_errors: usize,
    /// Chunks outside the load window.
    pub skipped_off_grid: usize,
    /// The unit the cells are laid out in.
    pub unit: Unit,
    /// One voxel cell's edge, in metres.
    pub cell_size: f32,
    /// Raster cells under water.
    pub water_cells: usize,
    /// What the cave carve wrote.
    pub caves: voxel_extract::VoxelCaveReport,
    /// How long the build took, the read included. [`build_from_chunks`]
    /// leaves it zero; the caller that timed the read sets it.
    pub elapsed: Duration,
}

/// Everything a build does once its chunks are read (see "The build" in the
/// module docs), from `chunks`, the `((cx, cy, cz), bytes)` records read,
/// with the window centred on `window_center` when one is given.
pub fn build_from_chunks(
    chunks: Vec<((i32, i32, i32), Vec<u8>)>,
    instance: TerrainInstanceProps,
    window_center: Option<IVec2>,
) -> Result<BuiltVoxelTerrain, String> {
    if chunks.is_empty() {
        return Err(match window_center {
            None => "no voxel chunk was read".to_string(),
            Some(center) => format!("no voxel chunk was read within reach of chunk column {center}"),
        });
    }

    // The host sets the unit its parts convert from (see
    // `TerrainInstanceProps::unit`), so the terrain lines up with them.
    let unit = instance.unit;
    let cell_size = voxel_extract::ROBLOX_CELL_STUDS * unit.to_meters() as f32;

    // ── Size the terrain to the load window ───────────────────────────
    // The grid is centred on the import's chunk columns rather than the
    // origin, so a map far from the origin still fits (see the module docs).
    let chunk_columns: HashSet<IVec2> = chunks.iter().map(|&((cx, _, cz), _)| IVec2::new(cx, cz)).collect();
    let Some(window) = load_window(&chunk_columns, window_center) else {
        return Err("the chunks read hold no chunk column".to_string());
    };
    let config = voxel_extract::voxel_terrain_config(window.center, window.half.x, window.half.y, cell_size);

    // ── Decode + multi-span fill, per chunk ───────────────────────────
    let mut data = TerrainData::procedural();
    let mut columns = voxel_extract::VoxelColumns::default();
    let mut filled = 0usize;
    let mut decode_errors = 0usize;
    let mut skipped_off_grid = 0usize;
    for &((cx, cy, cz), ref bytes) in &chunks {
        // Only an import wider than the window has chunks outside it.
        if !config.contains_chunk(IVec2::new(cx, cz)) {
            skipped_off_grid += 1;
            continue;
        }
        match voxel_extract::decode_voxel_chunk(bytes) {
            Ok(chunk) => {
                voxel_extract::fill_terrain_from_chunk(&mut data, &config, cx, cy, cz, &chunk);
                columns.record_chunk(cx, cy, cz, &chunk);
                filled += 1;
            }
            Err(e) => {
                decode_errors += 1;
                // Per-chunk, so DEBUG to avoid log spam on a big terrain; the
                // aggregate count is in the summary logged when the build lands.
                debug!(
                    target: LOG_TARGET,
                    cx, cy, cz, error = %e,
                    "voxel-terrain load: chunk decode failed; skipping"
                );
            }
        }
    }

    if filled == 0 {
        return Err(format!(
            "no chunk filled the terrain: {} read, {decode_errors} failed to decode, {skipped_off_grid} outside the load window",
            chunks.len()
        ));
    }

    // ── Surfaces over chunk seams, water, holes ───────────────────────
    // A surface whose cell above lives in the chunk stacked on its own is
    // finished only now: that chunk may have arrived after it (the world
    // database iterates Morton order). The holes take heights after the
    // water, which sets the ground under it.
    columns.refine_surface_tops(&mut data, &config);
    let water_levels = columns
        .has_water()
        .then(|| voxel_extract::voxel_water_levels(&config, &data, &columns));
    let water_cells = water_levels
        .as_ref()
        .map_or(0, |levels| levels.iter().filter(|level| level.is_finite()).count());
    let water_levels = water_levels.filter(|_| water_cells > 0);
    voxel_extract::fill_hole_heights(&config, &mut data, water_levels.as_deref());
    // A column without voxels is a hole: no ground is meshed or collided
    // there.
    data.sparse_surface = true;

    // ── Caves: carve the air under each column's top surface ──────────
    // Only the chunks around a cave are decoded a second time, and only
    // once every chunk is in, since a column's top surface can come from
    // any chunk stacked on it.
    let mut volume = TerrainVolume::default();
    let mut caves = voxel_extract::VoxelCaveReport::default();
    let cave_chunks = columns.cave_chunks();
    if !cave_chunks.is_empty() {
        let mut grid = voxel_extract::VoxelGrid::default();
        for &((cx, cy, cz), ref bytes) in &chunks {
            let coord = IVec3::new(cx, cy, cz);
            if !cave_chunks.contains(&coord) {
                continue;
            }
            // A chunk that failed to decode above contributed no surface
            // or extent either; its cells read as air here too.
            if let Ok(chunk) = voxel_extract::decode_voxel_chunk(bytes) {
                grid.insert(coord, chunk);
            }
        }
        caves = voxel_extract::carve_voxel_caves(&config, &data, &columns, &grid, &mut volume);
    }

    // ── Water: the levels, with the import's colour and transparency ──
    let water = water_levels.map(|levels| TerrainVoxelWater {
        levels,
        width: data.cache_width,
        height: data.cache_height,
        color: instance.water_color,
        transparency: instance.water_transparency,
    });

    Ok(BuiltVoxelTerrain {
        config,
        data,
        volume,
        water,
        window,
        stats: VoxelBuildStats {
            chunks_read: chunks.len(),
            chunks_filled: filled,
            decode_errors,
            skipped_off_grid,
            unit,
            cell_size,
            water_cells,
            caves,
            elapsed: Duration::ZERO,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::MATERIAL_SLOT_NONE;

    /// Every chunk column in `xs` by `zs`.
    fn columns(xs: std::ops::RangeInclusive<i32>, zs: std::ops::RangeInclusive<i32>) -> HashSet<IVec2> {
        xs.flat_map(|x| zs.clone().map(move |z| IVec2::new(x, z))).collect()
    }

    /// Vehicle Simulator's chunk columns span X -93..=160 and Z -38..=37: one
    /// window holds every one of them, and its raster fits a material-map
    /// texture.
    #[test]
    fn an_import_up_to_the_window_width_is_centred_on_its_span() {
        let imported = columns(-93..=160, -38..=37);
        let window = load_window(&imported, None).expect("the import has columns");
        assert_eq!(window.center, IVec2::new(33, -1));
        assert_eq!(window.half, UVec2::new(127, 38));
        assert_eq!((window.columns_min, window.columns_max), (IVec2::new(-93, -38), IVec2::new(160, 37)));
        let config = voxel_extract::voxel_terrain_config(
            window.center,
            window.half.x,
            window.half.y,
            voxel_extract::ROBLOX_CELL_STUDS,
        );
        assert!(imported.iter().all(|&column| config.contains_chunk(column)), "nothing is skipped");
        let raster_width = (2 * window.half.x + 1) * voxel_extract::CHUNK_EDGE as u32;
        assert!(raster_width <= MAX_MATERIAL_MAP_SIDE, "a {raster_width}-texel raster is too wide");
        // 255 columns fit the window; one more does not.
        assert!(!wider_than_window(0, 254) && wider_than_window(0, 255));
    }

    #[test]
    fn a_wider_import_centres_on_its_median_column() {
        // Columns X 0..=199 in one row, and a stray far out at X 1000.
        let mut imported = columns(0..=199, 0..=0);
        imported.insert(IVec2::new(1000, 0));
        let window = load_window(&imported, None).expect("the import has columns");
        assert_eq!(window.center, IVec2::new(100, 0), "201 columns: the median is the 101st");
        assert_eq!(window.half, UVec2::new(MAX_HALF_EXTENT_CHUNKS, 0));
        assert_eq!(MAX_WINDOW_CHUNKS, 255);
        assert!(load_window(&HashSet::new(), None).is_none());
    }

    /// Chunk columns X 0..=399 in one row, wider than the window: a given
    /// centre is pulled in until the window stays inside the import at either
    /// end, and without one the median rule decides.
    #[test]
    fn an_explicit_centre_keeps_the_window_inside_a_wide_import() {
        let imported = columns(0..=399, 0..=0);
        let half = MAX_HALF_EXTENT_CHUNKS as i32;
        for (asked, kept) in [(200, 200), (0, half), (-5_000, half), (399, 399 - half), (i32::MAX, 399 - half)] {
            let window = load_window(&imported, Some(IVec2::new(asked, 0))).expect("the import has columns");
            assert_eq!(window.center, IVec2::new(kept, 0), "a centre of {asked}");
            assert_eq!(window.half, UVec2::new(MAX_HALF_EXTENT_CHUNKS, 0), "a centre of {asked}");
            let config = voxel_extract::voxel_terrain_config(
                window.center,
                window.half.x,
                window.half.y,
                voxel_extract::ROBLOX_CELL_STUDS,
            );
            assert!(config.chunk_min().x >= 0 && config.chunk_max().x <= 399, "a centre of {asked} leaves the import");
        }
        let median = load_window(&imported, None).expect("the import has columns");
        assert_eq!(median.center, IVec2::new(200, 0), "400 columns: the median is the 201st");
        assert_eq!(median.half, UVec2::new(MAX_HALF_EXTENT_CHUNKS, 0));
    }

    /// Chunk columns Z 0..=99, which the window spans: a given centre is kept
    /// where the window still covers every column, and pulled in where it
    /// would not.
    #[test]
    fn an_explicit_centre_on_an_axis_the_window_spans_still_covers_it() {
        let imported = columns(0..=0, 0..=99);
        let half = MAX_HALF_EXTENT_CHUNKS as i32;
        for (asked, kept) in [(49, 49), (10, 10), (-500, 99 - half), (500, half)] {
            let window = load_window(&imported, Some(IVec2::new(0, asked))).expect("the import has columns");
            assert_eq!(window.center, IVec2::new(0, kept), "a centre of {asked}");
            assert!(window.half.y <= MAX_HALF_EXTENT_CHUNKS, "a centre of {asked}");
            let config = voxel_extract::voxel_terrain_config(
                window.center,
                window.half.x,
                window.half.y,
                voxel_extract::ROBLOX_CELL_STUDS,
            );
            assert!(imported.iter().all(|&column| config.contains_chunk(column)), "a centre of {asked} leaves columns out");
        }
    }

    #[test]
    fn the_window_follows_the_camera_along_an_axis_wider_than_it() {
        let reach = UVec2::splat(MAX_HALF_EXTENT_CHUNKS);
        // X 0..=999 is wider than the window, Z 0..=9 is not.
        let (bbox_min, bbox_max) = (IVec2::new(0, 0), IVec2::new(999, 9));
        let center = IVec2::new(500, 4);
        let edge = (MAX_HALF_EXTENT_CHUNKS - RECENTRE_MARGIN_CHUNKS) as i32;
        let target = |camera: IVec2, center: IVec2| recentre_target(camera, center, reach, bbox_min, bbox_max);

        // Up to the margin the window stays; one chunk past it, it centres on
        // the camera.
        assert_eq!(target(IVec2::new(500 + edge, 4), center), None);
        assert_eq!(target(IVec2::new(500 - edge, 4), center), None);
        assert_eq!(target(IVec2::new(501 + edge, 4), center), Some(IVec2::new(501 + edge, 4)));
        // Z fits the window: it never moves the window and keeps its centre.
        assert_eq!(target(IVec2::new(500, 5_000), center), None);
        assert_eq!(target(IVec2::new(300, 5_000), center), Some(IVec2::new(300, 4)));

        // Held inside the import at both ends, and left alone once there.
        let half = MAX_HALF_EXTENT_CHUNKS as i32;
        assert_eq!(target(IVec2::new(5_000, 4), center), Some(IVec2::new(999 - half, 4)));
        assert_eq!(target(IVec2::new(-5_000, 4), center), Some(IVec2::new(half, 4)));
        assert_eq!(target(IVec2::new(5_000, 4), IVec2::new(999 - half, 4)), None);
        assert_eq!(target(IVec2::new(-5_000, 4), IVec2::new(half, 4)), None);
    }

    #[test]
    fn a_window_that_holds_its_import_never_moves() {
        let reach = UVec2::splat(MAX_HALF_EXTENT_CHUNKS);
        // Vehicle Simulator's columns, X -93..=160 and Z -38..=37.
        let (bbox_min, bbox_max) = (IVec2::new(-93, -38), IVec2::new(160, 37));
        for camera in [IVec2::new(33, -1), IVec2::new(10_000, -1), IVec2::new(-10_000, 10_000), IVec2::new(160, 37)] {
            assert_eq!(recentre_target(camera, IVec2::new(33, -1), reach, bbox_min, bbox_max), None, "a camera at {camera}");
        }

        // Wide on both axes: a camera past the margin on X alone centres the
        // window on it along both.
        let (bbox_min, bbox_max) = (IVec2::new(0, 0), IVec2::new(999, 999));
        assert_eq!(
            recentre_target(IVec2::new(650, 520), IVec2::new(500, 500), reach, bbox_min, bbox_max),
            Some(IVec2::new(650, 520))
        );
    }

    /// `raw` as the size-prepended LZ4 record `decode_voxel_chunk` reads: its
    /// length, then one LZ4 block carrying every byte as literals, a valid
    /// block (the one an encoder writes for bytes it cannot shrink).
    fn lz4_literals(raw: &[u8]) -> Vec<u8> {
        let mut out = (raw.len() as u32).to_le_bytes().to_vec();
        if raw.len() < 15 {
            out.push((raw.len() as u8) << 4);
        } else {
            out.push(0xF0);
            let mut rest = raw.len() - 15;
            while rest >= 255 {
                out.push(255);
                rest -= 255;
            }
            out.push(rest as u8);
        }
        out.extend_from_slice(raw);
        out
    }

    /// A chunk record (version 1, no water) whose lowest `solid` layers are
    /// full Rock and the rest air, as the importer writes it.
    fn chunk_record(solid: usize) -> Vec<u8> {
        const ROCK: u8 = 1;
        let layer = voxel_extract::CHUNK_EDGE * voxel_extract::CHUNK_EDGE;
        let mut raw = vec![1, 1, 0];
        for cell in 0..voxel_extract::CELLS_PER_CHUNK {
            let pair = if cell / layer < solid { [ROCK, 255] } else { [voxel_extract::AIR_MARKER, 0] };
            raw.extend_from_slice(&pair);
        }
        lz4_literals(&raw)
    }

    /// Raster cell of local column `(x, z)` of chunk `chunk` in a built
    /// terrain.
    fn raster_cell(built: &BuiltVoxelTerrain, chunk: IVec2, x: usize, z: usize) -> usize {
        let grid = built.config.chunk_grid_index(chunk).expect("the chunk is on the grid");
        let edge = voxel_extract::CHUNK_EDGE;
        (grid.y as usize * edge + z) * built.data.cache_width as usize + grid.x as usize * edge + x
    }

    #[test]
    fn a_build_fills_the_top_surface_of_every_chunk_in_its_window() {
        assert!(voxel_extract::decode_voxel_chunk(&chunk_record(10)).is_ok(), "the test record decodes");
        // A full chunk with a five-layer chunk stacked on it, and two
        // ten-layer chunks beside it.
        let chunks = vec![
            ((0, 0, 0), chunk_record(32)),
            ((0, 1, 0), chunk_record(5)),
            ((1, 0, 0), chunk_record(10)),
            ((1, 0, 1), chunk_record(10)),
        ];
        let built = build_from_chunks(chunks, TerrainInstanceProps::default(), None).expect("the chunks build");
        assert_eq!((built.window.center, built.window.half), (IVec2::ZERO, UVec2::ONE));
        assert_eq!((built.window.columns_min, built.window.columns_max), (IVec2::ZERO, IVec2::ONE));
        assert_eq!(built.config.center_chunk, IVec2::ZERO);
        let stats = built.stats;
        assert_eq!((stats.chunks_read, stats.chunks_filled, stats.decode_errors, stats.skipped_off_grid), (4, 4, 0, 0));
        assert_eq!(stats.unit, Unit::Meter);
        assert!(built.water.is_none() && built.data.sparse_surface);

        // Metre cells are 4 m: the stacked column tops out 37 cells up, the
        // others 10.
        let cell = voxel_extract::ROBLOX_CELL_STUDS;
        for (chunk, cells) in [(IVec2::new(0, 0), 37.0), (IVec2::new(1, 0), 10.0), (IVec2::new(1, 1), 10.0)] {
            for (x, z) in [(0, 0), (31, 31), (7, 20)] {
                let i = raster_cell(&built, chunk, x, z);
                assert_eq!(built.data.height_cache[i], cells * cell, "chunk {chunk} column ({x}, {z})");
                assert_ne!(built.data.material_cache[i][0], MATERIAL_SLOT_NONE, "chunk {chunk} column ({x}, {z})");
            }
        }
        // No chunk holds column (0, 1): a hole.
        let hole = raster_cell(&built, IVec2::new(0, 1), 5, 5);
        assert_eq!(built.data.material_cache[hole][0], MATERIAL_SLOT_NONE);
    }

    #[test]
    fn a_build_centred_off_the_median_fills_the_other_end_of_a_wide_import() {
        let chunks = || vec![((0, 0, 0), chunk_record(10)), ((400, 0, 0), chunk_record(10))];
        let half = MAX_HALF_EXTENT_CHUNKS as i32;

        // The median rule centres on the second of the two columns.
        let median = build_from_chunks(chunks(), TerrainInstanceProps::default(), None).expect("the chunks build");
        assert_eq!(median.window.center, IVec2::new(400, 0));
        assert_eq!((median.stats.chunks_filled, median.stats.skipped_off_grid), (1, 1));

        // Centred toward the first, held inside the import: the first fills.
        let low = build_from_chunks(chunks(), TerrainInstanceProps::default(), Some(IVec2::ZERO)).expect("the chunks build");
        assert_eq!(low.window.center, IVec2::new(half, 0));
        assert_eq!((low.stats.chunks_filled, low.stats.skipped_off_grid), (1, 1));
        let first = raster_cell(&low, IVec2::ZERO, 3, 3);
        assert_eq!(low.data.height_cache[first], 10.0 * voxel_extract::ROBLOX_CELL_STUDS);

        let high = build_from_chunks(chunks(), TerrainInstanceProps::default(), Some(IVec2::new(5_000, 0)))
            .expect("the chunks build");
        assert_eq!(high.window.center, IVec2::new(400 - half, 0));
        assert!(high.config.contains_chunk(IVec2::new(400, 0)) && !high.config.contains_chunk(IVec2::ZERO));
    }

    #[test]
    fn a_build_with_nothing_to_fill_says_why() {
        assert!(build_from_chunks(Vec::new(), TerrainInstanceProps::default(), None).is_err());
        let garbage = build_from_chunks(vec![((0, 0, 0), vec![0, 1, 2])], TerrainInstanceProps::default(), None)
            .err()
            .expect("undecodable bytes build nothing");
        assert!(garbage.contains("1 failed to decode"), "{garbage}");
    }

    #[test]
    fn the_terrain_instance_gives_the_unit_and_the_water_style() {
        let imported = parse_terrain_instance(
            "[metadata]\nclass_name = \"Terrain\"\nunit = \"ft\"\n\n\
             [terrain]\nsource = \"imported\"\nwater_color = [0.05, 0.33, 0.36]\nwater_transparency = 0.3\n",
        );
        assert_eq!(imported.unit, Unit::Foot);
        let [r, g, b] = imported.water_color.expect("a water colour");
        assert!((r - 0.05).abs() < 1e-6 && (g - 0.33).abs() < 1e-6 && (b - 0.36).abs() < 1e-6);
        assert!((imported.water_transparency.expect("a transparency") - 0.3).abs() < 1e-6);

        // The Terrain class default stores the colour as bytes, and a file
        // without a unit stamp is in metres.
        let class_default = parse_terrain_instance("[terrain]\nwater_color = [88, 147, 172]\n");
        assert_eq!(class_default.unit, Unit::Meter);
        let [r, g, b] = class_default.water_color.expect("a water colour");
        assert!((r - 88.0 / 255.0).abs() < 1e-6 && (g - 147.0 / 255.0).abs() < 1e-6 && (b - 172.0 / 255.0).abs() < 1e-6);
        assert_eq!(class_default.water_transparency, None);

        assert_eq!(parse_terrain_instance("[metadata]\nunit = \"furlong\"\n").unit, Unit::Meter);
        assert_eq!(parse_terrain_instance("[terrain]\nwater_color = [1.0, 0.5]\n").water_color, None);
        assert_eq!(parse_terrain_instance("not toml ["), TerrainInstanceProps::default());

        // Only a source of "none" means the Terrain holds nothing.
        assert!(!imported.cleared);
        assert!(!parse_terrain_instance("[terrain]\nsource = \"partial\"\n").cleared);
        assert!(parse_terrain_instance("[metadata]\nunit = \"ft\"\n\n[terrain]\nsource = \"none\"\n").cleared);
    }

    /// A fresh, empty scratch directory for one test.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_voxel_import_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        dir
    }

    #[test]
    fn chunk_file_names_carry_signed_coordinates() {
        assert_eq!(voxel_chunk_file_coords("chunk_0_0_0.bin"), Some((0, 0, 0)));
        assert_eq!(voxel_chunk_file_coords("chunk_-4_0_-8.bin"), Some((-4, 0, -8)));
        assert_eq!(voxel_chunk_file_coords("chunk_12_-3_7.bin"), Some((12, -3, 7)));
        for name in [
            "chunk_1_2.bin",
            "chunk_1_2_3_4.bin",
            "chunk_1_2_3.bin.tmp",
            "chunk_a_2_3.bin",
            "chunk_1__3.bin",
            "block_1_2_3.bin",
            "notes.txt",
        ] {
            assert_eq!(voxel_chunk_file_coords(name), None, "{name}");
        }
    }

    /// The importer's chunk files read back with their coordinates, negative
    /// ones included, sorted; a stray file and a folder in their midst are
    /// skipped, and a terrain without the folder has no chunks.
    #[test]
    fn voxel_chunk_files_read_back_with_their_coordinates() {
        let terrain_dir = scratch_dir("read");
        assert_eq!(read_voxel_chunk_files(&terrain_dir), Ok(Vec::new()), "no voxel_chunks folder");
        assert!(!has_voxel_chunk_files(&terrain_dir));

        let folder = terrain_dir.join(VOXEL_CHUNKS_DIR);
        std::fs::create_dir_all(&folder).expect("the folder is creatable");
        std::fs::write(folder.join("notes.txt"), b"not a chunk").expect("writes");
        std::fs::create_dir_all(folder.join("chunk_9_9_9.bin")).expect("a folder with a chunk's name");
        assert!(!has_voxel_chunk_files(&terrain_dir), "neither a stray file nor a folder is a chunk file");
        assert_eq!(read_voxel_chunk_files(&terrain_dir), Ok(Vec::new()));

        std::fs::write(folder.join("chunk_3_0_1.bin"), b"third").expect("writes");
        std::fs::write(folder.join("chunk_-4_0_-8.bin"), b"first").expect("writes");
        std::fs::write(folder.join("chunk_-4_1_-8.bin"), b"second").expect("writes");
        assert!(has_voxel_chunk_files(&terrain_dir));
        let chunks = read_voxel_chunk_files(&terrain_dir).expect("the folder lists");
        assert_eq!(
            chunks,
            vec![
                ((-4, 0, -8), b"first".to_vec()),
                ((-4, 1, -8), b"second".to_vec()),
                ((3, 0, 1), b"third".to_vec()),
            ]
        );
        std::fs::remove_dir_all(&terrain_dir).ok();
    }

    /// Records read from the chunk files build the terrain the same records
    /// build from the world database.
    #[test]
    fn voxel_chunk_files_build_a_terrain() {
        let terrain_dir = scratch_dir("build");
        let folder = terrain_dir.join(VOXEL_CHUNKS_DIR);
        std::fs::create_dir_all(&folder).expect("the folder is creatable");
        std::fs::write(folder.join("chunk_-1_0_0.bin"), chunk_record(10)).expect("writes");
        std::fs::write(folder.join("chunk_0_0_0.bin"), chunk_record(20)).expect("writes");
        std::fs::write(
            terrain_dir.join(TERRAIN_INSTANCE_FILE),
            "[metadata]\nclass_name = \"Terrain\"\n\n[terrain]\nsource = \"imported\"\n",
        )
        .expect("writes");

        let instance = read_terrain_instance_file(&terrain_dir).expect("reads").expect("the instance is there");
        assert!(!instance.cleared);
        let chunks = read_voxel_chunk_files(&terrain_dir).expect("the folder lists");
        let built = build_from_chunks(chunks, instance, None).expect("the chunks build");
        assert_eq!(built.stats.chunks_filled, 2);
        let cell = voxel_extract::ROBLOX_CELL_STUDS;
        assert_eq!(built.data.height_cache[raster_cell(&built, IVec2::new(-1, 0), 4, 4)], 10.0 * cell);
        assert_eq!(built.data.height_cache[raster_cell(&built, IVec2::ZERO, 4, 4)], 20.0 * cell);

        std::fs::remove_file(terrain_dir.join(TERRAIN_INSTANCE_FILE)).expect("the instance is removable");
        assert_eq!(read_terrain_instance_file(&terrain_dir).expect("reads"), None);
        std::fs::remove_dir_all(&terrain_dir).ok();
    }
}
