//! The default terrain layers of a generated world: the `TerrainScatter` and
//! `TerrainWaterBody` instances [`super::export::export_to_space`] writes
//! into `Workspace/Terrain/Layers` beside the terrain while
//! `WorldSpec::default_layers` is on (the default), so a generated world
//! comes up with vegetation, scree and lakes that the user edits, moves or
//! deletes like any layer they inserted.
//!
//! ## Scatter
//! One layer per rule in `SCATTER_DEFAULTS`, each keyed to the material
//! the biome pass painted, since a scatter layer filters on one material:
//! meadow grass on Grass and on LeafyGrass, broadleaf forest on LeafyGrass at
//! moderate slopes below the conifer line, conifers above it (dense on
//! LeafyGrass, open woodland on Grass), scree rocks on steep Rock and Slate,
//! and beach grass on Sand just above sea level. The conifer line is a
//! quantile of the height of the world's Grass and LeafyGrass ground
//! ([`CONIFER_QUANTILE`]) rather than a fixed altitude: temperature here
//! follows latitude far more than height, so a fixed altitude would leave
//! one world without conifers and the next without broadleaf trees. Every
//! layer covers the whole terrain; its Seed comes from the world seed and its
//! name, so the same world places the same instances.
//!
//! ## Lakes
//! The depressions the hydrology pass would fill ([`fill_depressions`],
//! priority flood over the whole stitched world, whose edges drain) are the
//! lakes. Samples the fill raises by more than [`WET_MIN_M`], joined side to
//! side (`SIDE_NEIGHBOURS`), make one depression; it becomes a lake when it
//! covers at least [`MIN_LAKE_AREA_M2`] and the fill stands at least
//! [`MIN_LAKE_DEPTH_M`] over its lowest ground, the largest [`MAX_LAKES`]
//! kept. Its water body
//! stands on its lowest sample, where the flood starts, with Level 0 and the
//! water surface as its Position Y: [`LAKE_FREEBOARD_M`] below the height
//! the depression spills at. Its footprint is centred there, since a water
//! body's footprint turns and moves with its position, and reaches past the
//! depression's farthest sample on each axis.
//!
//! ## Files
//! Each layer is a folder holding one `_instance.toml` in the format the
//! class templates and Insert write: `[transform]`, the class's field-table
//! section written through [`FieldTable::to_toml_table`], and `[metadata]`,
//! read back by the Studio loader and the Client's
//! `layer_instances::read_layer_instances` alike. Its uuid is made of two
//! halves: a hash of the world seed and the folder name, and a hash of that
//! first half ([`generated_layer_uuid`]). It is minted when an export first
//! writes the folder; a later export writing the same folder keeps the uuid
//! it finds there, so the layer keeps its id, and the pattern placed from it,
//! in a Studio that has the layer open. Any later export can therefore tell
//! every layer an export wrote ([`is_generated_layer_uuid`]) without knowing
//! the seed it was written for, however it was edited or renamed since (every
//! writer keeps the uuid), and replaces them all: they belong to the ground
//! that export wrote. Any other layer is left alone, and a generated layer
//! whose name another layer already holds takes the first free `Name-2`,
//! `Name-3`.
//!
//! Determinism: the same [`WorldOutput`] and folder contents give
//! byte-identical files: fixed tables, fixed iteration orders, total-ordered
//! sorts, no RNG, no time.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;

use super::export::{stitch_world, ExportGrid, StitchedWorld};
use super::hydrology::fill_depressions;
use super::pipeline::WorldOutput;
use crate::classes::ClassName;
use crate::instance_create::{entity_name_is_available, uuid_bytes_to_hex, uuid_hex_to_bytes};
use crate::realism::particle_sim::class::FieldTable;
use crate::terrain::layer_instances::{layers_dir, LayerComponent, TerrainScatter, TerrainWaterBody};
use crate::terrain::material::TerrainMaterial;
use crate::terrain::scatter::{ScatterKind, TreeType};

/// How far the depression fill must raise a sample, metres, for it to count
/// as under a lake. The fill lifts filled flats by a millimetre per sample so
/// they drain; this keeps those staircases out.
pub const WET_MIN_M: f32 = 0.1;
/// Smallest lake written, square metres. A smaller hollow is a puddle.
pub const MIN_LAKE_AREA_M2: f64 = 2000.0;
/// Least depth, from the spill height down to the lowest ground, of a lake
/// written, metres.
pub const MIN_LAKE_DEPTH_M: f32 = 1.0;
/// Most lakes written, the largest first. Each is a flood the engine fills
/// and meshes.
pub const MAX_LAKES: usize = 32;
/// How far below its spill height a lake's water stands, metres. The export
/// resamples the world onto the terrain raster, which can bring a narrow rim
/// out a little lower than the source; water standing below the spill height
/// stays behind it, and the footprint holds back any that still gets past.
pub const LAKE_FREEBOARD_M: f32 = 0.25;
/// Samples a lake's footprint reaches past its farthest sample on each side.
const LAKE_FOOTPRINT_MARGIN_SAMPLES: f64 = 2.0;
/// The four samples beside one, `(di, dj)`. A lake's samples join through
/// these, as the engine's water-body flood joins raster cells: two hollows
/// that only touch corner to corner are two lakes, each at its own level,
/// rather than one flooded from the deeper at the level of the other.
const SIDE_NEIGHBOURS: [(i64, i64); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
/// Share of the Grass and LeafyGrass ground below the conifer line.
pub const CONIFER_QUANTILE: f64 = 0.6;
/// The conifer line of a world without Grass or LeafyGrass: this share of
/// its height scale above sea level. No layer places anything there anyway.
const CONIFER_FALLBACK_FRACTION: f64 = 0.5;
/// Most heights the conifer line is read from; a larger world is sampled
/// evenly down to this many.
const MAX_QUANTILE_SAMPLES: usize = 1 << 16;
/// Height above sea level, metres, up to which beach grass grows on Sand.
/// Higher Sand is desert.
const SHORE_BAND_M: f64 = 8.0;
/// Streaming radius of the tree layers, metres. Trees are entities; at the
/// Trees kind's own 1500 m a dense forest would stream tens of thousands.
const TREE_RADIUS_M: f64 = 600.0;
/// Folder names tried for one layer before giving up: `Name`, `Name-2`, ...
const MAX_NAME_TRIES: usize = 1000;

/// Hash domains of a generated layer's uuid halves and of its Seed.
const UUID_DOMAIN: &[u8] = b"eustress.worldgen.default-layer";
const UUID_CHECK_DOMAIN: &[u8] = b"eustress.worldgen.default-layer.check";
const SEED_DOMAIN: &[u8] = b"eustress.worldgen.default-layer.seed";

/// Written above each file's TOML. Gone once anything rewrites the file,
/// which changes nothing about the layer.
const FILE_HEADER: &str = "# A default layer of a generated world, written by the worldgen export.\n\
# The next export removes it, edited or not, with every other layer an export\n\
# wrote. Layers made any other way are left alone.\n\n";

// ============================================================================
// The layers
// ============================================================================

/// One default layer as the export writes it.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultLayer {
    /// Folder name, before a name another layer holds is avoided.
    pub name: String,
    pub class_name: ClassName,
    /// Engine world position (`[transform] position`).
    pub position: [f64; 3],
    /// The class component, written as its field-table section.
    pub component: LayerComponent,
}

/// The height band a scatter rule places in.
#[derive(Clone, Copy, Debug)]
enum Band {
    Any,
    /// Up to the conifer line.
    Lowland,
    /// From the conifer line up.
    Highland,
    /// Sea level to [`SHORE_BAND_M`] above it.
    Shore,
}

/// One biome-aware scatter rule.
#[derive(Clone, Copy, Debug)]
struct ScatterDefault {
    name: &'static str,
    kind: ScatterKind,
    tree_type: TreeType,
    material: TerrainMaterial,
    /// Instances per 100 square metres.
    density: f64,
    scale: (f64, f64),
    /// Degrees from horizontal.
    slope: (f64, f64),
    band: Band,
    align_to_normal: bool,
    collide: bool,
    /// Streaming radius, 0 for the kind's own.
    radius: f64,
}

/// The scatter every generated world gets. Trees stand upright and collide;
/// rocks follow the slope and the large ones collide.
const SCATTER_DEFAULTS: [ScatterDefault; 8] = [
    ScatterDefault {
        name: "MeadowGrass",
        kind: ScatterKind::Grass,
        tree_type: TreeType::Mixed,
        material: TerrainMaterial::Grass,
        density: 40.0,
        scale: (0.8, 1.3),
        slope: (0.0, 35.0),
        band: Band::Any,
        align_to_normal: true,
        collide: false,
        radius: 0.0,
    },
    ScatterDefault {
        name: "MeadowGrassLeafy",
        kind: ScatterKind::Grass,
        tree_type: TreeType::Mixed,
        material: TerrainMaterial::LeafyGrass,
        density: 30.0,
        scale: (0.8, 1.4),
        slope: (0.0, 35.0),
        band: Band::Any,
        align_to_normal: true,
        collide: false,
        radius: 0.0,
    },
    ScatterDefault {
        name: "BroadleafForest",
        kind: ScatterKind::Trees,
        tree_type: TreeType::Broadleaf,
        material: TerrainMaterial::LeafyGrass,
        density: 1.0,
        scale: (0.8, 1.4),
        slope: (0.0, 25.0),
        band: Band::Lowland,
        align_to_normal: false,
        collide: true,
        radius: TREE_RADIUS_M,
    },
    ScatterDefault {
        name: "ConiferForest",
        kind: ScatterKind::Trees,
        tree_type: TreeType::Conifer,
        material: TerrainMaterial::LeafyGrass,
        density: 1.2,
        scale: (0.8, 1.4),
        slope: (0.0, 30.0),
        band: Band::Highland,
        align_to_normal: false,
        collide: true,
        radius: TREE_RADIUS_M,
    },
    ScatterDefault {
        name: "ConiferWoodland",
        kind: ScatterKind::Trees,
        tree_type: TreeType::Conifer,
        material: TerrainMaterial::Grass,
        density: 0.3,
        scale: (0.7, 1.2),
        slope: (0.0, 30.0),
        band: Band::Highland,
        align_to_normal: false,
        collide: true,
        radius: TREE_RADIUS_M,
    },
    ScatterDefault {
        name: "ScreeRock",
        kind: ScatterKind::Rocks,
        tree_type: TreeType::Mixed,
        material: TerrainMaterial::Rock,
        density: 4.0,
        scale: (0.3, 2.5),
        slope: (30.0, 70.0),
        band: Band::Any,
        align_to_normal: true,
        collide: true,
        radius: 0.0,
    },
    ScatterDefault {
        name: "ScreeSlate",
        kind: ScatterKind::Rocks,
        tree_type: TreeType::Mixed,
        material: TerrainMaterial::Slate,
        density: 4.0,
        scale: (0.3, 2.5),
        slope: (30.0, 70.0),
        band: Band::Any,
        align_to_normal: true,
        collide: true,
        radius: 0.0,
    },
    ScatterDefault {
        name: "BeachGrass",
        kind: ScatterKind::Grass,
        tree_type: TreeType::Mixed,
        material: TerrainMaterial::Sand,
        density: 6.0,
        scale: (0.5, 1.0),
        slope: (0.0, 20.0),
        band: Band::Shore,
        align_to_normal: true,
        collide: false,
        radius: 0.0,
    },
];

/// Every default layer of `world`, exported on `grid`: the scatter layers in
/// `SCATTER_DEFAULTS` order, then one water body per lake, largest first.
/// Pure: the same world and grid give the same layers.
pub fn plan_default_layers(world: &WorldOutput, grid: &ExportGrid) -> Result<Vec<DefaultLayer>, String> {
    let stitched = stitch_world(world)?;
    let spec = &world.spec;
    let line = conifer_line(&stitched, spec.sea_level, spec.height_scale);
    let mut layers = scatter_layers(spec.seed, spec.sea_level, line);
    // The export lays generated-world metres `g` at engine `g - N * S`.
    let origin = -(f64::from(grid.half_extent) * f64::from(grid.chunk_size));
    let lakes = find_lakes(&stitched.heights, stitched.width, stitched.depth, stitched.cell);
    layers.extend(lakes.iter().enumerate().map(|(index, lake)| lake_layer(index, lake, stitched.cell, origin)));
    Ok(layers)
}

fn scatter_layers(seed: u64, sea_level: f64, conifer_line: f64) -> Vec<DefaultLayer> {
    let open = TerrainScatter::default();
    SCATTER_DEFAULTS
        .iter()
        .map(|rule| {
            let (min_height, max_height) = match rule.band {
                Band::Any => (open.min_height, open.max_height),
                Band::Lowland => (open.min_height, conifer_line),
                Band::Highland => (conifer_line, open.max_height),
                Band::Shore => (round_to(sea_level, 10.0), round_to(sea_level + SHORE_BAND_M, 10.0)),
            };
            DefaultLayer {
                name: rule.name.to_string(),
                class_name: ClassName::TerrainScatter,
                position: [0.0; 3],
                component: LayerComponent::Scatter(TerrainScatter {
                    kind: rule.kind,
                    tree_type: rule.tree_type,
                    density: rule.density,
                    min_scale: rule.scale.0,
                    max_scale: rule.scale.1,
                    align_to_normal: rule.align_to_normal,
                    collide: rule.collide,
                    seed: layer_seed(seed, rule.name),
                    material: Some(rule.material),
                    min_slope: rule.slope.0,
                    max_slope: rule.slope.1,
                    min_height,
                    max_height,
                    radius: rule.radius,
                    ..TerrainScatter::default()
                }),
            }
        })
        .collect()
}

/// The height between broadleaf and conifer country: the
/// [`CONIFER_QUANTILE`] of the height of the world's Grass and LeafyGrass
/// samples, to a tenth of a metre.
fn conifer_line(world: &StitchedWorld, sea_level: f64, height_scale: f64) -> f64 {
    let vegetated = [TerrainMaterial::Grass.to_u8(), TerrainMaterial::LeafyGrass.to_u8()];
    let mut heights: Vec<f32> = world
        .heights
        .iter()
        .zip(&world.materials)
        .filter(|&(h, m)| h.is_finite() && vegetated.contains(m))
        .map(|(h, _)| *h)
        .collect();
    if heights.is_empty() {
        return round_to(sea_level + CONIFER_FALLBACK_FRACTION * height_scale, 10.0);
    }
    let step = heights.len().div_ceil(MAX_QUANTILE_SAMPLES);
    if step > 1 {
        heights = heights.into_iter().step_by(step).collect();
    }
    heights.sort_unstable_by(f32::total_cmp);
    let index = ((heights.len() - 1) as f64 * CONIFER_QUANTILE).round() as usize;
    round_to(f64::from(heights[index]), 10.0)
}

/// A filled depression big and deep enough to hold a lake, on a sample grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Lake {
    /// Sample `(i, j)` of its lowest ground, the first in row order among
    /// equals: where its flood starts.
    pub deepest: (usize, usize),
    /// Height the depression fills to before it spills, metres.
    pub spill: f32,
    /// Height of its lowest ground, metres.
    pub floor: f32,
    /// Samples under it.
    pub samples: usize,
    /// Their area, square metres.
    pub area_m2: f64,
    /// Inclusive sample box around it, `(min_i, min_j, max_i, max_j)`.
    pub bounds: (usize, usize, usize, usize),
}

/// The lakes of a `width x depth` row-major heightfield with samples
/// `cell_m` metres apart (see the module docs), largest first, then in the
/// row order of their lowest sample; at most [`MAX_LAKES`].
pub fn find_lakes(heights: &[f32], width: usize, depth: usize, cell_m: f64) -> Vec<Lake> {
    let n = width * depth;
    if width < 3 || depth < 3 || heights.len() != n || !(cell_m > 0.0) {
        return Vec::new();
    }
    let filled = fill_depressions(heights, width as u32, depth as u32);
    let wet: Vec<bool> = heights.iter().zip(&filled).map(|(h, f)| f - h > WET_MIN_M).collect();
    let mut seen = vec![false; n];
    let mut queue = VecDeque::new();
    let mut lakes = Vec::new();
    for start in 0..n {
        if !wet[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        queue.push_back(start);
        let mut samples = 0usize;
        // The fill climbs a millimetre per sample away from the spill point
        // across the flat it makes, so its lowest value is the spill height.
        let mut spill = f32::INFINITY;
        let (mut deepest, mut floor) = (start, f32::INFINITY);
        let (mut lo_i, mut lo_j, mut hi_i, mut hi_j) = (usize::MAX, usize::MAX, 0usize, 0usize);
        while let Some(c) = queue.pop_front() {
            let (i, j) = (c % width, c / width);
            samples += 1;
            spill = spill.min(filled[c]);
            if heights[c] < floor || (heights[c] == floor && c < deepest) {
                floor = heights[c];
                deepest = c;
            }
            (lo_i, lo_j, hi_i, hi_j) = (lo_i.min(i), lo_j.min(j), hi_i.max(i), hi_j.max(j));
            for (di, dj) in SIDE_NEIGHBOURS {
                let (ni, nj) = (i as i64 + di, j as i64 + dj);
                if ni < 0 || nj < 0 || ni >= width as i64 || nj >= depth as i64 {
                    continue;
                }
                let nc = nj as usize * width + ni as usize;
                if wet[nc] && !seen[nc] {
                    seen[nc] = true;
                    queue.push_back(nc);
                }
            }
        }
        let area_m2 = samples as f64 * cell_m * cell_m;
        if area_m2 >= MIN_LAKE_AREA_M2 && spill - floor >= MIN_LAKE_DEPTH_M {
            lakes.push(Lake {
                deepest: (deepest % width, deepest / width),
                spill,
                floor,
                samples,
                area_m2,
                bounds: (lo_i, lo_j, hi_i, hi_j),
            });
        }
    }
    lakes.sort_by(|a, b| {
        b.samples.cmp(&a.samples).then((a.deepest.1, a.deepest.0).cmp(&(b.deepest.1, b.deepest.0)))
    });
    lakes.truncate(MAX_LAKES);
    lakes
}

/// The water body of lake number `index` (from 0) on a grid of samples
/// `cell` metres apart whose first sample sits at engine `(origin, origin)`.
fn lake_layer(index: usize, lake: &Lake, cell: f64, origin: f64) -> DefaultLayer {
    let (di, dj) = lake.deepest;
    let (lo_i, lo_j, hi_i, hi_j) = lake.bounds;
    let margin = LAKE_FOOTPRINT_MARGIN_SAMPLES * cell;
    let half_x = (di - lo_i).max(hi_i - di) as f64 * cell + margin;
    let half_z = (dj - lo_j).max(hi_j - dj) as f64 * cell + margin;
    // Down to the centimetre, so rounding never lifts the water toward its
    // rim; the footprint is rounded up, so it never shrinks.
    let level = floor_to(f64::from(lake.spill) - f64::from(LAKE_FREEBOARD_M), 100.0);
    DefaultLayer {
        name: format!("Lake{:02}", index + 1),
        class_name: ClassName::TerrainWaterBody,
        position: [round_to(origin + di as f64 * cell, 100.0), level, round_to(origin + dj as f64 * cell, 100.0)],
        component: LayerComponent::WaterBody(TerrainWaterBody {
            enabled: true,
            order: 0,
            level: 0.0,
            size_x: ceil_to(2.0 * half_x, 100.0),
            size_z: ceil_to(2.0 * half_z, 100.0),
        }),
    }
}

/// `x` to the nearest `1 / per`, e.g. `per = 10` for tenths. Dividing by
/// `per` rather than multiplying by its inverse keeps the result the closest
/// double to the decimal, so the file reads `63.3`, not `63.300000000000004`.
fn round_to(x: f64, per: f64) -> f64 {
    (x * per).round() / per
}

fn floor_to(x: f64, per: f64) -> f64 {
    (x * per).floor() / per
}

fn ceil_to(x: f64, per: f64) -> f64 {
    (x * per).ceil() / per
}

// ============================================================================
// Identity
// ============================================================================

fn hash_parts(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(&[0x1f]);
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// The second half of a generated layer's uuid, from its first.
fn uuid_check(first: &[u8]) -> [u8; 8] {
    let hash = hash_parts(UUID_CHECK_DOMAIN, &[first]);
    let mut check = [0u8; 8];
    check.copy_from_slice(&hash[..8]);
    check
}

/// The uuid minted for a default layer first written as folder `name` for a
/// world of seed `seed`: a hash of both, then a hash of that. The first 16
/// hex digits are the layer's id (`layer_instances::layer_id`).
pub fn generated_layer_uuid(seed: u64, name: &str) -> String {
    let hash = hash_parts(UUID_DOMAIN, &[seed.to_le_bytes().as_slice(), name.as_bytes()]);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hash[..8]);
    bytes[8..].copy_from_slice(&uuid_check(&hash[..8]));
    uuid_bytes_to_hex(&bytes)
}

/// Whether `uuid` is one [`generated_layer_uuid`] makes, for any seed and
/// name. Another uuid passes by chance once in 2^64.
pub fn is_generated_layer_uuid(uuid: &str) -> bool {
    uuid_hex_to_bytes(uuid).is_some_and(|bytes| bytes[8..] == uuid_check(&bytes[..8]))
}

/// A scatter rule's Seed in a world of seed `seed`: 32 bits, as the class
/// holds it.
fn layer_seed(seed: u64, name: &str) -> i64 {
    let hash = hash_parts(SEED_DOMAIN, &[seed.to_le_bytes().as_slice(), name.as_bytes()]);
    i64::from(u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]))
}

// ============================================================================
// Writing
// ============================================================================

/// What a write or clear of the default layers did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DefaultLayersSummary {
    /// Layer files written.
    pub written: usize,
    /// Folders of layers an earlier export wrote that were removed.
    pub removed: usize,
    /// Bytes of the files written.
    pub bytes_written: u64,
}

/// Replace the default layers in `<space_root>/Workspace/Terrain/Layers`
/// with `world`'s (see the module docs): every layer an earlier export wrote
/// goes, and `world`'s are written when `world.spec.default_layers` is on.
/// `grid` is the grid `world` was exported on.
pub fn write_default_layers(
    world: &WorldOutput,
    grid: &ExportGrid,
    space_root: &Path,
) -> Result<DefaultLayersSummary, String> {
    let planned = if world.spec.default_layers { plan_default_layers(world, grid)? } else { Vec::new() };
    replace_generated_layers(&layers_dir(space_root), world.spec.seed, &planned)
}

/// Remove every layer an export wrote from `<space_root>/Workspace/Terrain/Layers`,
/// for a writer that replaces the terrain with ground of its own (the flat
/// plate, a heightmap import). Best effort: a folder that cannot be removed
/// stays.
pub fn clear_generated_layers(space_root: &Path) -> DefaultLayersSummary {
    replace_generated_layers(&layers_dir(space_root), 0, &[]).unwrap_or_default()
}

fn replace_generated_layers(dir: &Path, seed: u64, planned: &[DefaultLayer]) -> Result<DefaultLayersSummary, String> {
    let generated = generated_layer_folders(dir);
    let mut names: Vec<String> = Vec::with_capacity(planned.len());
    for layer in planned {
        let name = free_layer_name(dir, &layer.name, &generated, &names)?;
        names.push(name);
    }

    // A layer written over a folder an earlier export wrote keeps that folder's
    // uuid: Studio's live entity for it hot-reloads the file but keeps the uuid
    // it spawned with, and the layer id scatter and water place from is that
    // uuid's first half, so a new one would part Studio's placement from the
    // Client's until a reload. A kept uuid that another layer of this write
    // mints or keeps (a generated layer renamed onto a planned name) gives way
    // to a fresh one, so no two layers share an id.
    let minted: Vec<String> = names.iter().map(|name| generated_layer_uuid(seed, name)).collect();
    let mut uuids: Vec<String> = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let kept = generated.get(name).map(|folder| folder.uuid.as_str()).filter(|kept| {
            !uuids.iter().any(|u| u == kept) && !minted.iter().enumerate().any(|(j, m)| j != i && m == kept)
        });
        uuids.push(kept.map_or_else(|| minted[i].clone(), str::to_string));
    }

    let mut summary = DefaultLayersSummary::default();
    for (name, folder) in &generated {
        if !names.contains(name) && folder.alone && remove_generated_folder(&dir.join(name)) {
            summary.removed += 1;
        }
    }
    for ((layer, name), uuid) in planned.iter().zip(&names).zip(&uuids) {
        let folder = dir.join(name);
        fs::create_dir_all(&folder).map_err(|e| format!("default layers: failed to create {:?}: {}", folder, e))?;
        let text = layer_instance_text(layer, name, uuid)?;
        let path = folder.join("_instance.toml");
        fs::write(&path, text.as_bytes()).map_err(|e| format!("default layers: failed to write {:?}: {}", path, e))?;
        summary.written += 1;
        summary.bytes_written += text.len() as u64;
    }
    Ok(summary)
}

/// A folder an earlier export wrote (see [`generated_layer_folders`]).
struct GeneratedFolder {
    /// It holds nothing but its `_instance.toml`.
    alone: bool,
    /// The generated uuid its file carries.
    uuid: String,
}

/// Folders directly under `dir` whose `_instance.toml` carries a generated
/// layer's uuid, by name, each with that uuid and whether the folder holds
/// nothing else. Only such a folder is ever removed: anything put beside the
/// file means someone is using it.
fn generated_layer_folders(dir: &Path) -> BTreeMap<String, GeneratedFolder> {
    let mut found = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Ok(text) = fs::read_to_string(folder.join("_instance.toml")) else { continue };
        let uuid = text
            .parse::<toml::Value>()
            .ok()
            .and_then(|doc| doc.get("metadata")?.get("uuid")?.as_str().map(str::to_string));
        let Some(uuid) = uuid.filter(|uuid| is_generated_layer_uuid(uuid)) else { continue };
        let alone = fs::read_dir(&folder)
            .map(|inner| inner.flatten().all(|item| item.file_name().to_str() == Some("_instance.toml")))
            .unwrap_or(false);
        found.insert(name, GeneratedFolder { alone, uuid });
    }
    found
}

/// The folder a default layer named `base` is written as: `base`, else
/// `base-2`, `base-3`, ..., the first that a generated layer holds (and is
/// overwritten) or that nothing holds, and that no earlier planned layer
/// took.
fn free_layer_name(
    dir: &Path,
    base: &str,
    generated: &BTreeMap<String, GeneratedFolder>,
    taken: &[String],
) -> Result<String, String> {
    for n in 1..=MAX_NAME_TRIES {
        let candidate = if n == 1 { base.to_string() } else { format!("{base}-{n}") };
        if taken.contains(&candidate) {
            continue;
        }
        if generated.contains_key(&candidate) || entity_name_is_available(dir, &candidate) {
            return Ok(candidate);
        }
    }
    Err(format!("default layers: no free folder name for {base} in {:?}", dir))
}

/// Remove a generated layer's folder: its file, then the folder.
fn remove_generated_folder(folder: &Path) -> bool {
    fs::remove_file(folder.join("_instance.toml")).is_ok() && fs::remove_dir(folder).is_ok()
}

/// The field-table section of a layer component and its TOML table.
fn component_section(component: &LayerComponent) -> (&'static str, toml::value::Table) {
    fn of<T: FieldTable>(component: &T) -> (&'static str, toml::value::Table) {
        (T::SECTION, component.to_toml_table())
    }
    match component {
        LayerComponent::Spline(c) => of(c),
        LayerComponent::Point(c) => of(c),
        LayerComponent::Stamp(c) => of(c),
        LayerComponent::FlattenPad(c) => of(c),
        LayerComponent::Noise(c) => of(c),
        LayerComponent::MaterialFill(c) => of(c),
        LayerComponent::Scatter(c) => of(c),
        LayerComponent::WaterBody(c) => of(c),
    }
}

/// The `_instance.toml` of `layer` written as folder `name` with `uuid`.
fn layer_instance_text(layer: &DefaultLayer, name: &str, uuid: &str) -> Result<String, String> {
    let floats = |values: &[f64]| toml::Value::Array(values.iter().map(|v| toml::Value::Float(*v)).collect());
    let mut transform = toml::value::Table::new();
    transform.insert("position".into(), floats(&layer.position));
    transform.insert("rotation".into(), floats(&[0.0, 0.0, 0.0, 1.0]));
    transform.insert("scale".into(), floats(&[1.0, 1.0, 1.0]));

    let mut metadata = toml::value::Table::new();
    metadata.insert("class_name".into(), toml::Value::String(layer.class_name.as_str().to_string()));
    metadata.insert("archivable".into(), toml::Value::Boolean(true));
    metadata.insert("name".into(), toml::Value::String(name.to_string()));
    metadata.insert("uuid".into(), toml::Value::String(uuid.to_string()));

    let (section, table) = component_section(&layer.component);
    let mut doc = toml::value::Table::new();
    doc.insert("transform".into(), toml::Value::Table(transform));
    doc.insert(section.to_string(), toml::Value::Table(table));
    doc.insert("metadata".into(), toml::Value::Table(metadata));
    let body = toml::to_string_pretty(&toml::Value::Table(doc))
        .map_err(|e| format!("default layers: failed to serialize {name}: {e}"))?;
    Ok(format!("{FILE_HEADER}{body}"))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::prelude::*;

    use super::*;
    use crate::instance_create::{fresh_uuid_for_create, is_valid_uuid};
    use crate::terrain::layer_instances::{layer_id, read_layer_instances, LayerInstanceFile};
    use crate::terrain::scatter::place_chunk;
    use crate::terrain::toml_loader;
    use crate::terrain::volume::TerrainVolume;
    use crate::terrain::water_bodies::flood_fill_water;
    use crate::terrain::worldgen::export::{export_flat_to_space, export_to_space, plan_export_grid, FlatSpec};
    use crate::terrain::worldgen::pipeline::WorldSpec;
    use crate::terrain::worldgen::GeneratedRegion;
    use crate::terrain::{TerrainConfig, TerrainData};

    // ── Lakes on a plain heightfield ─────────────────────────────────────

    /// A 48 x 40 plateau at 10 m, 5 m samples, dropping to 9 m on its edge
    /// so it drains, holding the hollow `(i, j, radius in samples, depth)`
    /// of each entry of `hollows` as a paraboloid.
    fn plateau(hollows: &[(f32, f32, f32, f32)]) -> (Vec<f32>, usize, usize) {
        let (width, depth) = (48usize, 40usize);
        let mut heights = vec![10.0f32; width * depth];
        for j in 0..depth {
            for i in 0..width {
                let h = &mut heights[j * width + i];
                if i == 0 || j == 0 || i == width - 1 || j == depth - 1 {
                    *h = 9.0;
                    continue;
                }
                for &(ci, cj, radius, deep) in hollows {
                    let r2 = ((i as f32 - ci).powi(2) + (j as f32 - cj).powi(2)) / (radius * radius);
                    if r2 < 1.0 {
                        *h -= deep * (1.0 - r2);
                    }
                }
            }
        }
        (heights, width, depth)
    }

    #[test]
    fn only_large_deep_hollows_are_lakes_largest_first() {
        let (heights, width, depth) = plateau(&[
            (12.0, 20.0, 7.0, 5.0), // a lake
            (34.0, 12.0, 7.0, 0.8), // as large, too shallow
            (24.0, 8.0, 1.5, 5.0),  // as deep, too small
            (34.0, 30.0, 6.0, 3.0), // a smaller lake
        ]);
        let lakes = find_lakes(&heights, width, depth, 5.0);
        assert_eq!(lakes.len(), 2, "{lakes:?}");
        let (big, small) = (&lakes[0], &lakes[1]);
        assert_eq!(big.deepest, (12, 20), "the flood starts on the lowest ground");
        assert_eq!(small.deepest, (34, 30));
        assert!(big.samples > small.samples, "largest first");
        assert_eq!(big.floor, 5.0);
        // The plateau it spills onto, lifted by the fill's millimetre
        // staircase at most.
        assert!(big.spill >= 10.0 && big.spill < 10.05, "spill {}", big.spill);
        assert!(big.area_m2 >= MIN_LAKE_AREA_M2);
        let (lo_i, lo_j, hi_i, hi_j) = big.bounds;
        assert!(lo_i >= 5 && hi_i <= 19 && lo_j >= 13 && hi_j <= 27, "bounds {:?}", big.bounds);
        assert!(hi_i - lo_i >= 10 && hi_j - lo_j >= 10, "bounds {:?}", big.bounds);

        // Nothing to fill, nothing found; and the same input, the same lakes.
        let (flat, width, depth) = plateau(&[]);
        assert!(find_lakes(&flat, width, depth, 5.0).is_empty());
        assert_eq!(find_lakes(&heights, width, depth, 5.0), lakes);
    }

    #[test]
    fn generated_uuids_are_recognised_whatever_the_seed_or_name() {
        let a = generated_layer_uuid(7, "MeadowGrass");
        assert!(is_valid_uuid(&a));
        assert!(is_generated_layer_uuid(&a));
        assert_eq!(a, generated_layer_uuid(7, "MeadowGrass"), "deterministic");
        for other in [generated_layer_uuid(8, "MeadowGrass"), generated_layer_uuid(7, "Lake01")] {
            assert_ne!(other, a);
            assert!(is_generated_layer_uuid(&other));
        }
        assert!(!is_generated_layer_uuid(&fresh_uuid_for_create()), "a uuid Insert mints is not generated");
        assert!(!is_generated_layer_uuid("not a uuid"));
        let mut tampered = a.clone();
        tampered.replace_range(31..32, if a.ends_with('0') { "1" } else { "0" });
        assert!(!is_generated_layer_uuid(&tampered));
    }

    // ── A generated world, exported ───────────────────────────────────────

    /// Ground 20 m up, rising 2 cm per metre toward +X so it drains off the
    /// world's low edge, with a bowl 8 m deep and 96 m across at generated
    /// (128, 128) and a pit as deep but only 12 m across at (384, 128), too
    /// small for a lake. The tilt puts the bowl's lowest ground 2.9 m
    /// downhill of its centre, nearest the sample at X = 124.
    fn ground(gx: f64, gz: f64) -> f32 {
        let mut h = 20.0 + 0.02 * gx;
        let bowl = ((gx - 128.0).powi(2) + (gz - 128.0).powi(2)).sqrt();
        if bowl < 48.0 {
            h -= 8.0 * (1.0 - (bowl / 48.0).powi(2));
        }
        let pit = ((gx - 384.0).powi(2) + (gz - 128.0).powi(2)).sqrt();
        if pit < 6.0 {
            h -= 6.0 * (1.0 - (pit / 6.0).powi(2));
        }
        h as f32
    }

    /// Two 256 m regions side by side, 4 m samples: LeafyGrass on the first
    /// (the bowl's), Grass on the second.
    fn test_world(default_layers: bool) -> WorldOutput {
        let spec = WorldSpec {
            seed: 7,
            regions_x: 2,
            regions_z: 1,
            region_size_m: 256.0,
            region_res: 65,
            sea_level: 0.0,
            height_scale: 120.0,
            default_layers,
            ..WorldSpec::default()
        };
        let cell = spec.region_size_m / f64::from(spec.region_res - 1);
        let mut regions = Vec::new();
        for rx in 0..spec.regions_x {
            let (ox, oz) = spec.region_origin(rx, 0);
            let mut region = GeneratedRegion::new(spec.region_res, spec.region_res);
            for iz in 0..spec.region_res {
                for ix in 0..spec.region_res {
                    let i = region.idx(ix, iz);
                    region.heights[i] = ground(ox + f64::from(ix) * cell, oz + f64::from(iz) * cell);
                }
            }
            let material = if rx == 0 { TerrainMaterial::LeafyGrass } else { TerrainMaterial::Grass };
            region.materials.fill(material.to_u8());
            regions.push(region);
        }
        WorldOutput { spec, regions, recipes: Vec::new() }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_default_layers_{}_{tag}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("a stale test dir is removable");
        }
        dir
    }

    /// Every file under `dir`, by path relative to it, with its bytes.
    fn files_under(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(path.strip_prefix(root).unwrap().to_path_buf(), fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    fn by_name(layers: &[LayerInstanceFile]) -> BTreeMap<String, &LayerInstanceFile> {
        layers.iter().map(|layer| (layer.name.clone(), layer)).collect()
    }

    fn load_terrain(root: &Path) -> (TerrainConfig, TerrainData) {
        let terrain = root.join("Workspace").join("Terrain");
        let config = toml_loader::load_terrain_toml(&terrain.join("_terrain.toml")).unwrap().to_terrain_config();
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        toml_loader::load_chunks_from_disk(&terrain, &config, &mut data);
        (config, data)
    }

    #[test]
    fn a_generated_world_writes_its_default_layers_deterministically() {
        let world = test_world(true);
        let grid = plan_export_grid(&world.spec).unwrap();
        assert_eq!((grid.half_extent, grid.chunk_size), (1, 256.0), "generated (g) lands at engine g - 256");
        let plan = plan_default_layers(&world, &grid).unwrap();

        let root_a = temp_root("det_a");
        let root_b = temp_root("det_b");
        let summary = export_to_space(&world, &root_a).unwrap();
        export_to_space(&world, &root_b).unwrap();
        assert_eq!(summary.layers_written, plan.len());
        let files_a = files_under(&layers_dir(&root_a));
        let files_b = files_under(&layers_dir(&root_b));
        assert_eq!(files_a.len(), plan.len(), "one _instance.toml per layer");
        assert_eq!(files_a, files_b, "two exports of one world write the same bytes");
        let layer_bytes: u64 = files_a.values().map(|bytes| bytes.len() as u64).sum();
        assert!(summary.bytes_written > layer_bytes, "the layer files count toward the bytes written");

        // Eight scatter layers and exactly one lake: the pit is too small.
        let (layers, problems) = read_layer_instances(&root_a);
        assert!(problems.is_empty(), "{problems:?}");
        let layers = by_name(&layers);
        let expected: Vec<&str> = SCATTER_DEFAULTS.iter().map(|rule| rule.name).chain(["Lake01"]).collect();
        let mut names: Vec<&str> = layers.keys().map(String::as_str).collect();
        let mut sorted = expected.clone();
        sorted.sort_unstable();
        names.sort_unstable();
        assert_eq!(names, sorted);

        // Each file reads back as the layer planned, under a generated uuid.
        for planned in &plan {
            let file = layers[planned.name.as_str()];
            assert_eq!(file.class_name, planned.class_name);
            assert_eq!(file.component, planned.component, "{}", planned.name);
            assert_eq!(file.uuid, generated_layer_uuid(7, &planned.name));
            assert!(is_generated_layer_uuid(&file.uuid));
            let at = planned.position.map(|v| v as f32);
            assert_eq!(file.transform.translation, Vec3::from_array(at), "{}", planned.name);
        }

        // The forest splits at the conifer line, inside the vegetated heights.
        let scatter = |name: &str| match &layers[name].component {
            LayerComponent::Scatter(scatter) => scatter.clone(),
            other => panic!("{name} is {other:?}"),
        };
        let (broadleaf, conifer) = (scatter("BroadleafForest"), scatter("ConiferForest"));
        assert_eq!(broadleaf.max_height, conifer.min_height);
        assert!(conifer.min_height > 14.0 && conifer.min_height < 31.0, "line {}", conifer.min_height);
        assert_eq!((broadleaf.kind, broadleaf.tree_type), (ScatterKind::Trees, TreeType::Broadleaf));
        assert_eq!(conifer.tree_type, TreeType::Conifer);
        assert_eq!(scatter("ScreeRock").material, Some(TerrainMaterial::Rock));
        assert_eq!(scatter("BeachGrass").max_height, SHORE_BAND_M);

        // The lake stands on the bowl's lowest sample (generated (124, 128)),
        // its water below the rim and over the floor.
        let lake = layers["Lake01"];
        let at = lake.transform.translation;
        assert_eq!((at.x, at.z), (124.0 - 256.0, 128.0 - 256.0));
        let floor = ground(124.0, 128.0);
        assert!(at.y - floor >= MIN_LAKE_DEPTH_M - LAKE_FREEBOARD_M - 0.01, "water {} over floor {floor}", at.y);
        assert!(at.y < ground(128.0 - 48.0, 128.0), "water {} under the rim", at.y);
        let LayerComponent::WaterBody(body) = &lake.component else { panic!("Lake01 is {:?}", lake.component) };
        assert_eq!(body.level, 0.0, "the water surface is the body's Position Y");
        assert!(body.size_x >= 96.0 && body.size_z >= 96.0, "footprint {} x {}", body.size_x, body.size_z);

        // On the terrain the export wrote, the lake floods the bowl and stays
        // in it, and the meadow grass grows on the Grass region while no
        // beach grass does.
        let (config, data) = load_terrain(&root_a);
        let desc = body.body(layer_id(Some(lake.uuid.as_str()), Entity::PLACEHOLDER), &lake.transform);
        let fill = flood_fill_water(&config, &data, &desc).expect("the lake's position stands under its water");
        assert!(fill.wet_count > 150, "{} wet cells", fill.wet_count);
        let bowl = Vec2::new(128.0 - 256.0, 128.0 - 256.0);
        for corner in [fill.bounds.0, fill.bounds.1] {
            assert!(corner.distance(bowl) < 70.0, "water reaches {corner}, outside the bowl");
        }
        let place = |name: &str| {
            let file = layers[name];
            let layer = scatter(name).layer(layer_id(Some(file.uuid.as_str()), Entity::PLACEHOLDER), &file.transform);
            place_chunk(&layer, IVec2::new(0, -1), &config, &data, &TerrainVolume::default(), None)
        };
        assert!(!place("MeadowGrass").is_empty(), "meadow grass on the Grass region");
        assert!(place("BeachGrass").is_empty(), "no beach 14 m and more above the sea");

        fs::remove_dir_all(&root_a).ok();
        fs::remove_dir_all(&root_b).ok();
    }

    #[test]
    fn a_re_export_with_another_seed_keeps_each_folders_uuid() {
        let root = temp_root("reseed");
        let world = test_world(true);
        export_to_space(&world, &root).unwrap();
        let uuids = |root: &Path| -> BTreeMap<String, String> {
            let (layers, problems) = read_layer_instances(root);
            assert!(problems.is_empty(), "{problems:?}");
            layers.into_iter().map(|layer| (layer.name, layer.uuid)).collect()
        };
        let before = uuids(&root);

        // The same folders written again for a world of another seed.
        let grid = plan_export_grid(&world.spec).unwrap();
        let plan = plan_default_layers(&world, &grid).unwrap();
        replace_generated_layers(&layers_dir(&root), 8, &plan).unwrap();
        let after = uuids(&root);
        assert_eq!(after, before, "every overwritten folder keeps its uuid");
        assert!(after.values().all(|uuid| is_generated_layer_uuid(uuid)));
        assert!(after.iter().all(|(name, uuid)| *uuid != generated_layer_uuid(8, name)), "none took the new seed's");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_flag_turns_the_default_layers_off() {
        let root = temp_root("flag_off");
        let summary = export_to_space(&test_world(false), &root).unwrap();
        assert_eq!(summary.layers_written, 0);
        let (layers, _) = read_layer_instances(&root);
        assert!(layers.is_empty(), "{layers:?}");
        assert!(files_under(&layers_dir(&root)).is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exports_replace_only_the_layers_an_export_wrote() {
        let root = temp_root("replace");
        let dir = layers_dir(&root);
        // A layer the user made, under a name a default layer wants.
        let own = dir.join("MeadowGrass");
        fs::create_dir_all(&own).unwrap();
        let own_text = "[metadata]\nclass_name = \"TerrainStamp\"\narchivable = true\nuuid = \"0123456789abcdef0123456789abcdef\"\n";
        fs::write(own.join("_instance.toml"), own_text).unwrap();

        let world = test_world(true);
        let first = export_to_space(&world, &root).unwrap();
        let written = files_under(&dir);
        assert_eq!(fs::read_to_string(own.join("_instance.toml")).unwrap(), own_text, "the user's layer is untouched");
        assert!(dir.join("MeadowGrass-2").join("_instance.toml").is_file(), "the default takes the next free name");

        // Again: the same folders are overwritten, none added.
        let second = export_to_space(&world, &root).unwrap();
        assert_eq!(second.layers_written, first.layers_written);
        assert_eq!(files_under(&dir), written);

        // Off: every generated layer goes, the user's stays.
        export_to_space(&test_world(false), &root).unwrap();
        let left: Vec<PathBuf> = files_under(&dir).into_keys().collect();
        assert_eq!(left, vec![PathBuf::from("MeadowGrass").join("_instance.toml")]);

        // A flat plate clears them too.
        export_to_space(&world, &root).unwrap();
        assert!(dir.join("Lake01").is_dir());
        export_flat_to_space(&FlatSpec { half_extent: 1, chunk_resolution: 16, ..Default::default() }, &root).unwrap();
        let left: Vec<PathBuf> = files_under(&dir).into_keys().collect();
        assert_eq!(left, vec![PathBuf::from("MeadowGrass").join("_instance.toml")]);
        fs::remove_dir_all(&root).ok();
    }
}
