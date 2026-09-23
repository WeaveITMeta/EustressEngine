//! Grass, shrubs, rocks and trees scattered over the finished ground by the
//! rules of each `TerrainScatter` layer instance.
//!
//! ## Placement
//! Placement is a pure function of the layer, the tile and the surface
//! ([`place_tile`]). Each chunk of the terrain's grid is cut into equal
//! square tiles, at most 32 m across for the merged kinds (the chunk itself
//! on grids of 32 m or finer) and at most 128 m for trees and custom meshes,
//! and each tile into square cells about one instance apart at the layer's
//! Density. A generator seeded from the layer's Seed, its id and the tile
//! draws the same numbers for each cell every time: whether the cell holds
//! an instance at all (so the count follows the density exactly on
//! average), where in the cell it stands, its scale, turn, variant and tint. Every draw is taken whatever
//! the rules decide, so one cell's outcome never shifts another's. A
//! candidate is kept when the ground passes the layer's rules there, read
//! from the SURFACE the meshers draw (`surface_data`): the footprint, the
//! height, the slope from the normal, the material weights, and, with
//! AvoidRoads, the corridors of the Road and Path splines the bake laid.
//! Nothing placed is stored: an edit, a bake or a layer change places the
//! affected tiles again, and the same inputs place the same instances on
//! every machine.
//!
//! ## Drawing
//! Small instances (grass, shrubs, and rocks up to [`LARGE_ROCK_SIZE`]) are
//! concatenated into one mesh per tile and layer, so a meadow of millions of
//! blades costs a draw call per tile. Trees, large rocks and custom meshes
//! are entities sharing one mesh and material handle per variant, which Bevy
//! batches into instanced draws; one batch stands at most
//! [`MAX_ENTITY_INSTANCES_PER_BATCH`] of them. With Collide on, trees get a
//! trunk capsule and large rocks a ball, all in one static compound collider
//! per batch.
//! The built-in meshes come from `scatter_meshes`; a Custom layer draws the
//! first mesh and material of its MeshAsset, through the `space://` asset
//! source every Space mesh loads through.
//!
//! ## Streaming
//! A layer keeps batches only near the view (the scene camera, see
//! `scene_camera_translation`): a tile is built once its nearest point comes
//! within the layer's radius and dropped once it is farther than the radius
//! plus a margin, so a camera on the edge does not build and drop the same
//! tile frame after frame. Builds are budgeted per frame, lowest Order first
//! and nearest first within an Order. A batch goes stale when the ground
//! under its chunk changes (the surface stamps of `TerrainDirtyChunks`, which
//! brush strokes, undo and bakes all leave) or when its layer changes; a
//! stale batch stays drawn until its replacement is built, so an edit never
//! blinks the scatter out, and the replacement waits until the ground and the
//! layer have gone `SCATTER_QUIET_SECS` without a change, so a stroke or a
//! drag is not placed again on every frame. A layer disabled or removed takes
//! its batches at once, as does one whose new Kind cuts its chunks into tiles
//! of another size.
//!
//! Batches are derived state, like a road's surface: not instances, never
//! saved, not in the Explorer. Only a terrain with a height raster is
//! scattered over; procedural terrain draws its ground from noise the
//! surface data does not hold.

use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::platform::time::Instant;
use bevy::prelude::*;

use super::height_query::{height_at_world, material_weights_at_world};
use super::layer_instances::{compose_world_pose, layer_id, TerrainScatter};
use super::layers::{rects_overlap, rotated_rect_bounds, surface_data, PreparedLayers, SplineMode, TerrainBaked};
use super::material::TerrainMaterial;
use super::scatter_meshes::{
    ScatterMesh, ScatterMeshData, BROADLEAF_VARIANTS, CONIFER_VARIANTS, GRASS_VARIANTS, ROCK_DIAMETER, ROCK_VARIANTS,
    SHRUB_VARIANTS,
};
use super::volume::{chunk_has_volume, sample_field, TerrainVolume};
use super::{
    apply_terrain_dirty_chunks, scene_camera_translation, SurfaceChanges, TerrainConfig, TerrainData,
    TerrainDirtyChunks, TerrainGridKey, TerrainRoot,
};
use crate::classes::Instance;
use crate::realism::particle_sim::rng::SimRng;

/// A rock wider than this (metres: its scale times `ROCK_DIAMETER`) stands as
/// its own entity, and collides when its layer does; smaller rocks join the
/// tile's merged mesh.
pub const LARGE_ROCK_SIZE: f32 = 1.5;
/// Most candidate cells along a tile's side. A Density that asks for more is
/// thinned to this many per side, which bounds a merged mesh's size. Over the
/// 32 m tiles of the merged kinds that is 2500 per 100 square metres, so
/// [`MAX_DENSITY`] is reachable.
pub const MAX_CELLS_PER_SIDE: u32 = 160;
/// Most instances one batch stands as their own entities (trees, custom
/// meshes, large rocks). Past it those candidates are thinned by the same
/// draw that thins cells, so the cap keeps a subset of the pattern rather
/// than a different one.
pub const MAX_ENTITY_INSTANCES_PER_BATCH: f32 = 2048.0;
/// Longest side of a tile, metres, for the kinds merged into one mesh
/// (Grass, Shrubs, Rocks) and for those standing as entities (Trees,
/// Custom). A chunk is cut into equal square tiles no longer than this, and
/// one batch covers one tile: tiles bound a merged mesh and a build's cost,
/// and they make the Density ceiling independent of the terrain's chunk
/// size.
const MERGED_TILE_TARGET: f32 = 32.0;
const ENTITY_TILE_TARGET: f32 = 128.0;
/// Most tiles along a chunk's side, so a chunk size typed absurdly large
/// cannot overflow tile coordinates.
const MAX_TILES_PER_CHUNK: f32 = 256.0;
/// Highest Density a layer takes, instances per 100 square metres.
pub const MAX_DENSITY: f64 = 1000.0;
/// Longest streaming radius a layer takes, metres.
pub const MAX_RADIUS: f64 = 20_000.0;
/// Scale range a layer's MinScale and MaxScale are held to.
pub const MIN_SCALE: f64 = 0.01;
pub const MAX_SCALE: f64 = 100.0;
/// Merged instances' colours are scaled by a tint within this range around 1.
const TINT_RANGE: f32 = 0.3;
/// How far below and above the surface a candidate probes a chunk's volume,
/// metres: ground carved away under it, or rock added over it, means no
/// instance there.
const VOLUME_PROBE: f32 = 0.05;
/// Batches built per frame, and the time after which a frame stops building.
/// The first build of a frame always runs, so streaming never stalls.
const MAX_BUILDS_PER_FRAME: usize = 8;
const BUILD_TIME_BUDGET: Duration = Duration::from_millis(3);
/// Seconds the ground and a layer must go unchanged before their stale
/// batches are rebuilt, as the terrain's colliders wait (`COLLIDER_QUIET_SECS`
/// in `dirty`): a stroke or a drag re-marks the same chunks every frame, and a
/// stale batch stays drawn meanwhile.
const SCATTER_QUIET_SECS: f64 = 0.15;
/// A batch is dropped only this fraction of its layer's radius past it, and
/// at least half a tile past it.
const DROP_MARGIN_FRACTION: f32 = 0.15;
/// The spline modes AvoidRoads keeps off.
const AVOIDED_SPLINES: [SplineMode; 2] = [SplineMode::Road, SplineMode::Path];

// ============================================================================
// Layer descriptions
// ============================================================================

/// What a scatter layer places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScatterKind {
    /// Tufts of curved blades, merged per tile, casting no shadow.
    #[default]
    Grass,
    /// Low rounded bushes, merged per tile.
    Shrubs,
    /// Faceted boulders: merged when small, entities past [`LARGE_ROCK_SIZE`].
    Rocks,
    /// Conifers and broadleaf trees, as entities.
    Trees,
    /// The layer's MeshAsset, as entities.
    Custom,
}

impl ScatterKind {
    pub const ALL: [ScatterKind; 5] = [Self::Grass, Self::Shrubs, Self::Rocks, Self::Trees, Self::Custom];

    /// The name a class property stores the kind under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Grass => "Grass",
            Self::Shrubs => "Shrubs",
            Self::Rocks => "Rocks",
            Self::Trees => "Trees",
            Self::Custom => "Custom",
        }
    }

    /// The kind [`Self::name`] names, ignoring case and surrounding spaces.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Streaming radius of a layer whose Radius is 0, metres: how far such
    /// objects still read on screen.
    pub fn default_radius(self) -> f32 {
        match self {
            Self::Grass => 120.0,
            Self::Shrubs => 250.0,
            Self::Rocks => 450.0,
            Self::Trees | Self::Custom => 1500.0,
        }
    }

    /// Density a layer of this kind starts at, instances per 100 square
    /// metres.
    pub fn default_density(self) -> f64 {
        match self {
            Self::Grass => 40.0,
            Self::Shrubs | Self::Rocks => 4.0,
            Self::Trees | Self::Custom => 1.0,
        }
    }

    /// Longest side of the tiles a layer of this kind cuts each chunk into,
    /// metres.
    pub fn tile_target(self) -> f32 {
        match self {
            Self::Grass | Self::Shrubs | Self::Rocks => MERGED_TILE_TARGET,
            Self::Trees | Self::Custom => ENTITY_TILE_TARGET,
        }
    }
}

/// Which trees a Trees layer places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TreeType {
    /// Conifers and broadleaf trees, about half each.
    #[default]
    Mixed,
    Conifer,
    Broadleaf,
}

impl TreeType {
    pub const ALL: [TreeType; 3] = [Self::Mixed, Self::Conifer, Self::Broadleaf];

    /// The name a class property stores the type under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mixed => "Mixed",
            Self::Conifer => "Conifer",
            Self::Broadleaf => "Broadleaf",
        }
    }

    /// The type [`Self::name`] names, ignoring case and surrounding spaces.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tree| tree.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// The rectangle a scatter layer places inside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScatterFootprint {
    /// World XZ of its centre.
    pub center: Vec2,
    /// Rotation about +Y, radians, as `Quat::from_rotation_y(yaw)`.
    pub yaw: f32,
    /// Half extents along its local X and Z; infinite along an axis the
    /// layer does not limit.
    pub half: Vec2,
}

impl ScatterFootprint {
    /// Whether world `p` lies inside, edges included.
    pub fn contains(&self, p: Vec2) -> bool {
        // The turn back into the footprint's frame, as the baked layers do
        // it: `Quat::from_rotation_y(yaw)` takes local +X to world
        // `(cos, -sin)`.
        let (sin, cos) = self.yaw.sin_cos();
        let d = p - self.center;
        let local = Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
        local.x.abs() <= self.half.x && local.y.abs() <= self.half.y
    }

    /// World XZ box `(min, max)` around it, `None` when an axis is unlimited.
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        if !self.half.is_finite() {
            return None;
        }
        Some(rotated_rect_bounds(Vec3::new(self.center.x, 0.0, self.center.y), self.yaw, self.half))
    }
}

/// One enabled scatter layer as plain data in world units: what a
/// `TerrainScatter` instance maps onto (see `TerrainScatter::layer`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScatterLayer {
    /// Stable identity (the instance's), which also seeds the pattern.
    pub id: u64,
    /// Lower orders build first.
    pub order: i32,
    pub kind: ScatterKind,
    pub tree_type: TreeType,
    /// Custom only: the mesh asset path as the instance holds it.
    pub mesh_asset: String,
    /// Instances per 100 square metres where every rule holds.
    pub density: f32,
    pub min_scale: f32,
    pub max_scale: f32,
    /// Material slot the ground must show, `None` for any.
    pub material: Option<u8>,
    /// Slope range, degrees from horizontal, both ends included.
    pub min_slope: f32,
    pub max_slope: f32,
    /// World height range, metres, both ends included.
    pub min_height: f32,
    pub max_height: f32,
    pub align_to_normal: bool,
    pub avoid_roads: bool,
    pub collide: bool,
    pub seed: u64,
    /// Where it places, `None` for the whole terrain.
    pub footprint: Option<ScatterFootprint>,
    /// Streaming radius, metres, the kind's default already applied.
    pub radius: f32,
}

impl Default for ScatterLayer {
    /// Grass over the whole terrain, on any ground.
    fn default() -> Self {
        Self {
            id: 1,
            order: 0,
            kind: ScatterKind::Grass,
            tree_type: TreeType::Mixed,
            mesh_asset: String::new(),
            density: 40.0,
            min_scale: 1.0,
            max_scale: 1.0,
            material: None,
            min_slope: 0.0,
            max_slope: 90.0,
            min_height: f32::MIN,
            max_height: f32::MAX,
            align_to_normal: true,
            avoid_roads: false,
            collide: false,
            seed: 1,
            footprint: None,
            radius: ScatterKind::Grass.default_radius(),
        }
    }
}

impl ScatterLayer {
    /// Whether chunk `chunk` can hold any instance of this layer: it is on
    /// `config`'s grid, it overlaps the footprint, and the layer places
    /// anything at all (a density, and a mesh when it is Custom).
    pub fn can_place(&self, config: &TerrainConfig, chunk: IVec2) -> bool {
        if !(self.density > 0.0) || (self.kind == ScatterKind::Custom && self.mesh_asset.trim().is_empty()) {
            return false;
        }
        let (extent_x, extent_z) = (config.chunks_x as i32, config.chunks_z as i32);
        if chunk.x < -extent_x || chunk.x > extent_x || chunk.y < -extent_z || chunk.y > extent_z {
            return false;
        }
        match self.footprint.and_then(|footprint| footprint.bounds()) {
            Some((lo, hi)) => {
                let chunk_lo = chunk.as_vec2() * config.chunk_size;
                let chunk_hi = chunk_lo + Vec2::splat(config.chunk_size);
                rects_overlap((chunk_lo, chunk_hi), (lo, hi))
            }
            None => true,
        }
    }
}

/// One placed instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScatterInstance {
    /// World position on the surface.
    pub position: Vec3,
    pub rotation: Quat,
    /// Uniform scale, 1 being the mesh's natural size.
    pub scale: f32,
    /// A draw from 0 to 255 each family picks its variant from.
    pub variant: u8,
    /// Brightness a merged instance's colours are scaled by.
    pub tint: f32,
}

// ============================================================================
// Placement
// ============================================================================

/// Candidate cells along a side of a tile `size` metres across at `density`
/// instances per 100 square metres, and the chance each cell holds one. The
/// cells are no larger than one instance's share of the ground, so the chance
/// never exceeds 1 until the side reaches [`MAX_CELLS_PER_SIDE`].
fn cell_grid(size: f32, density: f32) -> (u32, f32) {
    if !(density > 0.0) || !(size > 0.0) {
        return (0, 0.0);
    }
    let spacing = (100.0 / density).sqrt();
    let cells = ((size / spacing).ceil() as u32).clamp(1, MAX_CELLS_PER_SIDE);
    let cell = size / cells as f32;
    (cells, (density * cell * cell / 100.0).min(1.0))
}

/// The seed of one layer's generator over one tile (on a grid whose chunks
/// are one tile each, the tile is the chunk).
fn chunk_key(seed: u64, layer: u64, tile: IVec2) -> u64 {
    let packed = (u64::from(tile.x as u32) << 32) | u64::from(tile.y as u32);
    SimRng::new(seed ^ layer.rotate_left(32), packed).next_u64()
}

/// Tiles along a side of a `chunk_size` metre chunk for `layer`: the fewest
/// equal tiles no longer than its kind's [`ScatterKind::tile_target`], so
/// tile edges fall on chunk edges at any chunk size.
fn tiles_per_chunk(layer: &ScatterLayer, chunk_size: f32) -> i32 {
    // `max` before `min`, not `clamp`: a NaN size must come out as one tile,
    // and `clamp` would pass the NaN through to a zero divisor.
    (chunk_size / layer.kind.tile_target()).ceil().max(1.0).min(MAX_TILES_PER_CHUNK) as i32
}

/// The chunk tile `tile` lies in, at `tpc` tiles per chunk side.
fn tile_chunk(tile: IVec2, tpc: i32) -> IVec2 {
    IVec2::new(tile.x.div_euclid(tpc), tile.y.div_euclid(tpc))
}

/// World XZ of tile `tile`'s low corner, at `tpc` tiles per chunk side.
/// Measured from its chunk's corner, so tile edges land exactly on chunk
/// edges.
fn tile_origin(tile: IVec2, tpc: i32, config: &TerrainConfig) -> Vec2 {
    let chunk = tile_chunk(tile, tpc);
    chunk.as_vec2() * config.chunk_size + (tile - chunk * tpc).as_vec2() * (config.chunk_size / tpc as f32)
}

/// The surface normal at world `p`, by central differences `step` metres
/// either side (one raster cell, the finest the surface changes over).
fn surface_normal(config: &TerrainConfig, ground: &TerrainData, p: Vec2, step: f32) -> Vec3 {
    let h = |x: f32, z: f32| height_at_world(config, ground, x, z);
    let dx = h(p.x - step, p.y) - h(p.x + step, p.y);
    let dz = h(p.x, p.y - step) - h(p.x, p.y + step);
    Vec3::new(dx, 2.0 * step, dz).try_normalize().unwrap_or(Vec3::Y)
}

/// The share of material `slot` in the ground at `p`: its bilinear weight,
/// or, on ground without a material layer, all Grass, the material a new
/// material layer starts as.
fn material_share(config: &TerrainConfig, ground: &TerrainData, p: Vec2, slot: u8) -> f32 {
    if !ground.has_material_layer() {
        return if slot == TerrainMaterial::Grass.to_u8() { 1.0 } else { 0.0 };
    }
    material_weights_at_world(config, ground, p.x, p.y).weight_of(slot)
}

/// Whether the surface at `p`, height `h`, is carved away underneath or
/// buried under added rock.
fn buried_or_carved(config: &TerrainConfig, ground: &TerrainData, volume: &TerrainVolume, p: Vec2, h: f32) -> bool {
    sample_field(config, ground, volume, Vec3::new(p.x, h - VOLUME_PROBE, p.y)) > 0.0
        || sample_field(config, ground, volume, Vec3::new(p.x, h + VOLUME_PROBE, p.y)) < 0.0
}

/// A range typed backwards still means the range between its ends.
fn ordered(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Every instance `layer` places on chunk `chunk` of `config`'s grid: its
/// tiles' ([`place_tile`]) in row order. Deterministic: the same inputs give
/// the same instances, in the same order.
pub fn place_chunk(
    layer: &ScatterLayer,
    chunk: IVec2,
    config: &TerrainConfig,
    ground: &TerrainData,
    volume: &TerrainVolume,
    corridors: Option<&PreparedLayers>,
) -> Vec<ScatterInstance> {
    let tpc = tiles_per_chunk(layer, config.chunk_size);
    let mut placed = Vec::new();
    for z in 0..tpc {
        for x in 0..tpc {
            let tile = chunk * tpc + IVec2::new(x, z);
            placed.extend(place_tile(layer, tile, config, ground, volume, corridors));
        }
    }
    placed
}

/// Every instance `layer` places on tile `tile` (see `tiles_per_chunk`) of
/// `config`'s grid, over `ground` (the surface data) and `volume`, keeping
/// off the Road and Path corridors of `corridors` when the layer avoids
/// roads. Deterministic: the same inputs give the same instances, in the same
/// order.
pub fn place_tile(
    layer: &ScatterLayer,
    tile: IVec2,
    config: &TerrainConfig,
    ground: &TerrainData,
    volume: &TerrainVolume,
    corridors: Option<&PreparedLayers>,
) -> Vec<ScatterInstance> {
    let mut placed = Vec::new();
    let tpc = tiles_per_chunk(layer, config.chunk_size);
    let chunk = tile_chunk(tile, tpc);
    let size = config.chunk_size / tpc as f32;
    let (cells, keep) = cell_grid(size, layer.density);
    let has_raster = ground.cache_width >= 2
        && ground.cache_height >= 2
        && ground.height_cache.len() == ground.cache_width as usize * ground.cache_height as usize;
    if cells == 0 || !has_raster || !layer.can_place(config, chunk) {
        return placed;
    }
    let (lo, hi) = config.footprint_xz();
    let step = ((hi.x - lo.x) / (ground.cache_width - 1) as f32).max(1e-3);
    let origin = tile_origin(tile, tpc, config);
    let cell = size / cells as f32;
    let (min_slope, max_slope) = ordered(layer.min_slope, layer.max_slope);
    let (min_height, max_height) = ordered(layer.min_height, layer.max_height);
    let (min_scale, max_scale) = ordered(layer.min_scale, layer.max_scale);
    // The share of kept candidates that stand alone (the scale draw is
    // uniform), and the lower keep that holds a batch's expected entities to
    // the cap.
    let alone_share = match layer.kind {
        ScatterKind::Grass | ScatterKind::Shrubs => 0.0,
        ScatterKind::Trees | ScatterKind::Custom => 1.0,
        ScatterKind::Rocks if max_scale > min_scale => {
            ((max_scale - LARGE_ROCK_SIZE / ROCK_DIAMETER) / (max_scale - min_scale)).clamp(0.0, 1.0)
        }
        ScatterKind::Rocks => {
            if stands_alone(ScatterKind::Rocks, min_scale) { 1.0 } else { 0.0 }
        }
    };
    let expected_alone = (cells * cells) as f32 * keep * alone_share;
    let alone_keep = if expected_alone > MAX_ENTITY_INSTANCES_PER_BATCH {
        keep * MAX_ENTITY_INSTANCES_PER_BATCH / expected_alone
    } else {
        keep
    };
    let probe_volume = !volume.is_empty() && chunk_has_volume(chunk, config, volume);
    let corridors = corridors.filter(|_| layer.avoid_roads);
    let key = chunk_key(layer.seed, layer.id, tile);

    for row in 0..cells {
        for column in 0..cells {
            let mut rng = SimRng::new(key, u64::from(row * cells + column));
            let [keep_draw, x_draw, z_draw, scale_draw, yaw_draw, variant_draw, tint_draw, material_draw] =
                [(); 8].map(|_| rng.uniform());
            if keep_draw >= keep {
                continue;
            }
            let scale = min_scale + (max_scale - min_scale) * scale_draw;
            if keep_draw >= alone_keep && stands_alone(layer.kind, scale) {
                continue;
            }
            let p = origin + Vec2::new((column as f32 + x_draw) * cell, (row as f32 + z_draw) * cell);
            if layer.footprint.is_some_and(|footprint| !footprint.contains(p)) {
                continue;
            }
            let h = height_at_world(config, ground, p.x, p.y);
            if !(h >= min_height && h <= max_height) {
                continue;
            }
            let normal = surface_normal(config, ground, p, step);
            let slope = normal.y.clamp(-1.0, 1.0).acos().to_degrees();
            if !(slope >= min_slope && slope <= max_slope) {
                continue;
            }
            // Kept with the chance the material's share gives, so a blend
            // into another material thins the scatter out rather than
            // cutting it off at a line.
            if layer.material.is_some_and(|slot| material_draw >= material_share(config, ground, p, slot)) {
                continue;
            }
            if corridors.is_some_and(|corridors| corridors.in_corridor(p, &AVOIDED_SPLINES)) {
                continue;
            }
            if probe_volume && buried_or_carved(config, ground, volume, p, h) {
                continue;
            }
            let yaw = Quat::from_rotation_y(yaw_draw * TAU);
            let rotation = if layer.align_to_normal { Quat::from_rotation_arc(Vec3::Y, normal) * yaw } else { yaw };
            placed.push(ScatterInstance {
                position: Vec3::new(p.x, h, p.y),
                rotation,
                scale,
                variant: (variant_draw * 256.0) as u8,
                tint: 1.0 + (tint_draw - 0.5) * TINT_RANGE,
            });
        }
    }
    placed
}

// ============================================================================
// Batches
// ============================================================================

/// The built-in meshes as mesh data, each built once on first use.
#[derive(Debug, Default)]
pub struct ScatterMeshLibrary {
    data: HashMap<ScatterMesh, ScatterMeshData>,
}

impl ScatterMeshLibrary {
    /// Mesh `mesh`'s data.
    pub fn get(&mut self, mesh: ScatterMesh) -> &ScatterMeshData {
        self.data.entry(mesh).or_insert_with(|| mesh.build())
    }

    /// A ball around rock `mesh` at scale 1: centred in the rock's bounds,
    /// as wide as their mean half extent, so a body meets roughly the rock
    /// it sees.
    pub fn rock_ball(&mut self, mesh: ScatterMesh) -> (Vec3, f32) {
        let data = self.get(mesh);
        let (lo, hi) = data.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
        if !lo.cmple(hi).all() {
            return (Vec3::ZERO, ROCK_DIAMETER * 0.5);
        }
        let half = (hi - lo) * 0.5;
        ((lo + hi) * 0.5, (half.x + half.y + half.z) / 3.0)
    }
}

/// The built-in mesh an instance of `layer` draws, `None` for a Custom layer.
pub fn instance_mesh(layer: &ScatterLayer, instance: &ScatterInstance) -> Option<ScatterMesh> {
    let v = instance.variant;
    Some(match layer.kind {
        ScatterKind::Grass => ScatterMesh::Grass(v % GRASS_VARIANTS),
        ScatterKind::Shrubs => ScatterMesh::Shrub(v % SHRUB_VARIANTS),
        ScatterKind::Rocks => ScatterMesh::Rock(v % ROCK_VARIANTS),
        ScatterKind::Trees => match layer.tree_type {
            TreeType::Conifer => ScatterMesh::Conifer(v % CONIFER_VARIANTS),
            TreeType::Broadleaf => ScatterMesh::Broadleaf(v % BROADLEAF_VARIANTS),
            // The low bit picks the family, the rest the variant, so a mixed
            // stand still shows every variant of both.
            TreeType::Mixed if v & 1 == 0 => ScatterMesh::Conifer((v >> 1) % CONIFER_VARIANTS),
            TreeType::Mixed => ScatterMesh::Broadleaf((v >> 1) % BROADLEAF_VARIANTS),
        },
        ScatterKind::Custom => return None,
    })
}

/// Whether an instance of `mesh` at `scale` joins its tile's merged mesh
/// rather than standing as its own entity.
fn merges(mesh: ScatterMesh, scale: f32) -> bool {
    match mesh {
        ScatterMesh::Grass(_) | ScatterMesh::Shrub(_) => true,
        ScatterMesh::Rock(_) => scale * ROCK_DIAMETER <= LARGE_ROCK_SIZE,
        ScatterMesh::Conifer(_) | ScatterMesh::Broadleaf(_) => false,
    }
}

/// Whether an instance of `kind` at `scale` stands as its own entity; the
/// converse of [`merges`] for the built-in meshes.
fn stands_alone(kind: ScatterKind, scale: f32) -> bool {
    match kind {
        ScatterKind::Grass | ScatterKind::Shrubs => false,
        ScatterKind::Rocks => scale * ROCK_DIAMETER > LARGE_ROCK_SIZE,
        ScatterKind::Trees | ScatterKind::Custom => true,
    }
}

/// What one entity of a batch draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScatterDraw {
    Builtin(ScatterMesh),
    /// The layer's MeshAsset.
    Custom,
}

/// A static collision shape of a batch, in the batch's frame (its tile's
/// origin, unscaled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScatterCollider {
    /// A tree trunk: a capsule whose end hemispheres are centred on `a` and
    /// `b`.
    Capsule { a: Vec3, b: Vec3, radius: f32 },
    /// A large rock.
    Ball { center: Vec3, radius: f32 },
}

/// One tile's scatter of one layer, ready to spawn. Everything is relative
/// to the tile's origin, which keeps merged vertex positions small.
#[derive(Clone, Debug, Default)]
pub struct ScatterBatchParts {
    /// The small instances concatenated, `None` when there are none.
    pub merged: Option<ScatterMeshData>,
    /// The instances that stand as entities, and where.
    pub entities: Vec<(ScatterDraw, Transform)>,
    /// Static collision shapes, when the layer collides.
    pub colliders: Vec<ScatterCollider>,
}

impl ScatterBatchParts {
    pub fn is_empty(&self) -> bool {
        self.merged.is_none() && self.entities.is_empty() && self.colliders.is_empty()
    }
}

/// Sort `instances` of `layer`, placed on the tile whose origin is `origin`,
/// into a merged mesh, entities and colliders.
pub fn build_batch_parts(
    layer: &ScatterLayer,
    origin: Vec3,
    instances: &[ScatterInstance],
    library: &mut ScatterMeshLibrary,
) -> ScatterBatchParts {
    let mut parts = ScatterBatchParts::default();
    let mut merged = ScatterMeshData::default();
    // Sized up front: a merged tile holds hundreds of tufts, and buffers grown
    // by doubling would copy their vertices over and over.
    let (mut vertices, mut indices) = (0usize, 0usize);
    for instance in instances {
        if let Some(mesh) = instance_mesh(layer, instance).filter(|mesh| merges(*mesh, instance.scale)) {
            let data = library.get(mesh);
            vertices += data.vertex_count();
            indices += data.indices.len();
        }
    }
    merged.positions.reserve(vertices);
    merged.normals.reserve(vertices);
    merged.colors.reserve(vertices);
    merged.indices.reserve(indices);
    for instance in instances {
        let local = Transform {
            translation: instance.position - origin,
            rotation: instance.rotation,
            scale: Vec3::splat(instance.scale),
        };
        let Some(mesh) = instance_mesh(layer, instance) else {
            parts.entities.push((ScatterDraw::Custom, local));
            continue;
        };
        if merges(mesh, instance.scale) {
            merged.append_transformed(library.get(mesh), &local, instance.tint);
            continue;
        }
        parts.entities.push((ScatterDraw::Builtin(mesh), local));
        if !layer.collide {
            continue;
        }
        // Shapes carry the instance's scale in their sizes, since the batch
        // they belong to is unscaled.
        if let Some(trunk) = mesh.trunk() {
            let radius = trunk.radius * instance.scale;
            let up = instance.rotation * Vec3::Y;
            let top = (trunk.height * instance.scale - radius).max(radius);
            parts.colliders.push(ScatterCollider::Capsule {
                a: local.translation + up * radius,
                b: local.translation + up * top,
                radius,
            });
        } else if matches!(mesh, ScatterMesh::Rock(_)) {
            let (center, radius) = library.rock_ball(mesh);
            parts.colliders.push(ScatterCollider::Ball {
                center: local.transform_point(center),
                radius: radius * instance.scale,
            });
        }
    }
    if merged.vertex_count() > 0 {
        parts.merged = Some(merged);
    }
    parts
}

/// The `space://` asset URLs of a Custom layer's mesh and material: the first
/// primitive of the first mesh and the first material of `path`, a file
/// inside the Space folder named relative to it, as every Space mesh is
/// loaded. A path that names its own asset source (`bundled://...`) keeps
/// it, and a label after `#` is replaced. `None` for an empty path.
pub fn custom_asset_urls(path: &str) -> Option<(String, String)> {
    let path = path.trim().replace('\\', "/");
    let file = path.split('#').next().unwrap_or("").trim();
    if file.is_empty() {
        return None;
    }
    let source = if file.contains("://") { file.to_string() } else { format!("space://{}", file.trim_start_matches('/')) };
    Some((format!("{source}#Mesh0/Primitive0"), format!("{source}#Material0/std")))
}

// ============================================================================
// ECS
// ============================================================================

/// Marks the entity holding one tile's scatter of one layer: its merged
/// mesh, its entity instances as children, and its collider.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScatterBatch {
    /// The layer's id (`layer_instances::layer_id`).
    pub layer: u64,
    /// The chunk the tile lies in.
    pub chunk: IVec2,
    /// The tile, in the layer's tiles (the chunk itself when a chunk is one
    /// tile).
    pub tile: IVec2,
}

/// The two shared scatter materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ScatterMaterial {
    /// Grass: drawn from both sides.
    Foliage,
    /// Everything else.
    Solid,
}

/// A white, matte material that shows the meshes' vertex colours unmodified
/// (`StandardMaterial` multiplies its base colour by them).
fn scatter_material(material: ScatterMaterial) -> StandardMaterial {
    match material {
        ScatterMaterial::Foliage => StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            reflectance: 0.25,
            // A blade is one strip seen from both sides. Culling is off
            // rather than `double_sided` set: that flips a back face's
            // normal, and the blades' normals lean up so that both faces
            // light like the ground they stand on.
            cull_mode: None,
            ..default()
        },
        ScatterMaterial::Solid => StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            reflectance: 0.3,
            ..default()
        },
    }
}

/// Meshes and materials every batch shares: each built-in mesh once as data
/// for merging and once as a mesh asset for entities, the two materials, and
/// each custom mesh's handles.
#[derive(Resource, Default)]
pub struct ScatterAssets {
    library: ScatterMeshLibrary,
    meshes: HashMap<ScatterMesh, Handle<Mesh>>,
    materials: HashMap<ScatterMaterial, Handle<StandardMaterial>>,
    custom: HashMap<String, (Handle<Mesh>, Handle<StandardMaterial>)>,
}

impl ScatterAssets {
    fn mesh_handle(&mut self, mesh: ScatterMesh, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        if let Some(handle) = self.meshes.get(&mesh) {
            return handle.clone();
        }
        let handle = meshes.add(self.library.get(mesh).clone().into_mesh());
        self.meshes.insert(mesh, handle.clone());
        handle
    }

    fn material(&mut self, material: ScatterMaterial, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        self.materials.entry(material).or_insert_with(|| materials.add(scatter_material(material))).clone()
    }

    /// The handles of the mesh and material a Custom layer with MeshAsset
    /// `path` draws, loaded on first use. `None` for an empty path, with no
    /// asset server to load through, or for a Space path on a host without
    /// the `space` asset source.
    fn custom_handles(
        &mut self,
        path: &str,
        asset_server: Option<&AssetServer>,
    ) -> Option<(Handle<Mesh>, Handle<StandardMaterial>)> {
        let (mesh_url, material_url) = custom_asset_urls(path)?;
        if let Some(handles) = self.custom.get(&mesh_url) {
            return Some(handles.clone());
        }
        let server = asset_server?;
        // A host that never registered `space://` would log a load error per
        // batch; draw nothing instead. Other sources (`bundled://`) still load.
        if mesh_url.starts_with("space://") && server.get_source("space").is_err() {
            return None;
        }
        let handles: (Handle<Mesh>, Handle<StandardMaterial>) = (server.load(mesh_url.clone()), server.load(material_url));
        self.custom.insert(mesh_url, handles.clone());
        Some(handles)
    }
}

/// What a built batch is.
#[derive(Clone, Copy, Debug)]
struct ScatterBatchRecord {
    /// Its entity; `None` when the tile holds nothing of the layer.
    entity: Option<Entity>,
    /// The ground or the layer changed since it was built.
    stale: bool,
    /// The chunk its tile lies in, whose surface marks make it stale.
    chunk: IVec2,
}

/// Bookkeeping of [`update_terrain_scatter`].
#[derive(Resource, Debug, Default)]
pub struct TerrainScatterState {
    /// The enabled layers as last seen, by id.
    layers: HashMap<u64, ScatterLayer>,
    /// The batch built for each layer and tile.
    batches: HashMap<(u64, IVec2), ScatterBatchRecord>,
    /// `TerrainDirtyChunks::surface_seq` caught up to.
    seen_surface: u64,
    /// The terrain root and grid the batches stand on.
    terrain: Option<(Entity, TerrainGridKey)>,
    /// Clock seconds (`Time`, the clock `TerrainDirtyChunks::last_mark_secs`
    /// is stamped with) of each layer's latest change.
    layer_changed: HashMap<u64, f64>,
}

impl TerrainScatterState {
    /// The entity drawing layer `layer`'s scatter over tile `tile` (the
    /// chunk itself when a chunk is one tile), when one is built and the
    /// tile holds anything.
    pub fn batch(&self, layer: u64, tile: IVec2) -> Option<Entity> {
        self.batches.get(&(layer, tile)).and_then(|record| record.entity)
    }

    /// How many batches are built, the empty ones included.
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }
}

/// Read access to the scatter layer instances.
#[derive(SystemParam)]
pub struct ScatterLayerQueries<'w, 's> {
    scatters: Query<'w, 's, (Entity, Option<&'static Instance>, &'static TerrainScatter)>,
    transforms: Query<'w, 's, &'static Transform>,
    parents: Query<'w, 's, &'static ChildOf>,
}

impl ScatterLayerQueries<'_, '_> {
    /// Every enabled scatter layer. Poses are composed from the `Transform`s
    /// of the layer and its ancestors, as the baked layers' are, since
    /// `GlobalTransform` lags a frame.
    pub fn gather(&self) -> Vec<ScatterLayer> {
        self.scatters
            .iter()
            .filter(|(_, _, scatter)| scatter.enabled)
            .map(|(entity, instance, scatter)| {
                let pose = compose_world_pose(
                    entity,
                    |e| self.transforms.get(e).ok().copied(),
                    |e| self.parents.get(e).ok().map(ChildOf::parent),
                );
                scatter.layer(layer_id(instance.map(|i| i.uuid.as_str()), entity), &pose)
            })
            .collect()
    }
}

/// Horizontal distance from `viewer` to the nearest point of square `chunk`
/// of a grid of squares `size` metres across (chunks, or a layer's tiles).
fn chunk_distance(chunk: IVec2, size: f32, viewer: Vec2) -> f32 {
    let lo = chunk.as_vec2() * size;
    viewer.clamp(lo, lo + Vec2::splat(size)).distance(viewer)
}

/// How far past its radius a layer's batch is kept, on tiles `tile_size`
/// metres across.
fn drop_margin(radius: f32, tile_size: f32) -> f32 {
    (radius * DROP_MARGIN_FRACTION).max(tile_size * 0.5)
}

fn despawn_record(commands: &mut Commands, record: &mut ScatterBatchRecord) {
    if let Some(entity) = record.entity.take() {
        commands.entity(entity).try_despawn();
    }
}

fn drop_batches(commands: &mut Commands, batches: &mut HashMap<(u64, IVec2), ScatterBatchRecord>) {
    for (_, mut record) in batches.drain() {
        despawn_record(commands, &mut record);
    }
}

/// Mark the batches on the chunks whose ground `changes` names stale. An
/// empty batch goes stale too: the new ground may pass the rules.
fn mark_surface_changes(batches: &mut HashMap<(u64, IVec2), ScatterBatchRecord>, changes: &SurfaceChanges) {
    if changes.all {
        batches.values_mut().for_each(|record| record.stale = true);
        return;
    }
    if changes.chunks.is_empty() {
        return;
    }
    let changed: HashSet<IVec2> = changes.chunks.iter().copied().collect();
    for record in batches.values_mut() {
        if changed.contains(&record.chunk) {
            record.stale = true;
        }
    }
}

/// A batch waiting to be built.
#[derive(Clone, Copy, Debug)]
struct Wanted {
    order: i32,
    distance: f32,
    layer: u64,
    tile: IVec2,
}

/// Spawn one batch from `parts`, returning its entity, or `None` when there
/// is nothing to draw or collide with.
#[allow(clippy::too_many_arguments)]
fn spawn_batch(
    commands: &mut Commands,
    layer: &ScatterLayer,
    chunk: IVec2,
    tile: IVec2,
    origin: Vec3,
    parts: ScatterBatchParts,
    assets: &mut ScatterAssets,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: Option<&AssetServer>,
) -> Option<Entity> {
    // Resolved first: a custom mesh may have nothing to load from.
    let children: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)> = parts
        .entities
        .iter()
        .filter_map(|(draw, transform)| {
            let (mesh, material) = match draw {
                ScatterDraw::Builtin(mesh) => {
                    (assets.mesh_handle(*mesh, meshes), assets.material(ScatterMaterial::Solid, materials))
                }
                ScatterDraw::Custom => assets.custom_handles(&layer.mesh_asset, asset_server)?,
            };
            Some((mesh, material, *transform))
        })
        .collect();
    if parts.merged.is_none() && children.is_empty() && parts.colliders.is_empty() {
        return None;
    }
    let foliage = layer.kind == ScatterKind::Grass;
    let merged_material = parts
        .merged
        .as_ref()
        .map(|_| assets.material(if foliage { ScatterMaterial::Foliage } else { ScatterMaterial::Solid }, materials));
    let mut batch = commands.spawn((
        Name::new(format!("Scatter_{}_{}", tile.x, tile.y)),
        ScatterBatch { layer: layer.id, chunk, tile },
        Transform::from_translation(origin),
        Visibility::default(),
    ));
    if let (Some(merged), Some(material)) = (parts.merged, merged_material) {
        batch.insert((Mesh3d(meshes.add(merged.into_mesh())), MeshMaterial3d(material)));
        if foliage {
            // Thousands of thin blades per tile: their shadows cost far
            // more than they show.
            batch.insert(NotShadowCaster);
        }
    }
    #[cfg(feature = "physics")]
    {
        use avian3d::prelude::{Collider, RigidBody};
        if !parts.colliders.is_empty() {
            let shapes: Vec<(Vec3, Quat, Collider)> = parts
                .colliders
                .iter()
                .map(|shape| match *shape {
                    ScatterCollider::Capsule { a, b, radius } => {
                        (Vec3::ZERO, Quat::IDENTITY, Collider::capsule_endpoints(radius, a, b))
                    }
                    ScatterCollider::Ball { center, radius } => (center, Quat::IDENTITY, Collider::sphere(radius)),
                })
                .collect();
            batch.insert((RigidBody::Static, Collider::compound(shapes)));
        }
    }
    let batch = batch.id();
    for (mesh, material, transform) in children {
        commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform, Visibility::default(), ChildOf(batch)));
    }
    Some(batch)
}

/// Keep every enabled scatter layer's batches built near the view and in
/// step with the ground and the layer (see the module docs). Runs after
/// `apply_terrain_dirty_chunks`, so a chunk edited or re-baked this frame is
/// placed from its finished ground, around the corridors that bake laid.
#[allow(clippy::too_many_arguments)]
pub fn update_terrain_scatter(
    mut commands: Commands,
    mut state: ResMut<TerrainScatterState>,
    mut assets: ResMut<ScatterAssets>,
    dirty: Option<Res<TerrainDirtyChunks>>,
    time: Option<Res<Time>>,
    roots: Query<
        (Entity, &TerrainConfig, &TerrainData, Option<&TerrainBaked>, Option<&TerrainVolume>),
        With<TerrainRoot>,
    >,
    layer_queries: ScatterLayerQueries,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    live: Query<(), With<ScatterBatch>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    asset_server: Option<Res<AssetServer>>,
) {
    let TerrainScatterState { layers, batches, seen_surface, terrain, layer_changed } = &mut *state;
    let (Some((root, config, base, baked, volume)), Some(mut meshes), Some(mut materials)) =
        (roots.iter().next(), meshes, materials)
    else {
        // No terrain to scatter over, or a host that draws nothing.
        drop_batches(&mut commands, batches);
        layers.clear();
        layer_changed.clear();
        *terrain = None;
        return;
    };

    // A new terrain (regenerated, imported, another Space) or a new grid:
    // nothing built fits it, and its own history of marks starts now.
    let key = TerrainGridKey::of(config);
    if *terrain != Some((root, key)) {
        drop_batches(&mut commands, batches);
        *terrain = Some((root, key));
        *seen_surface = dirty.as_ref().map_or(0, |dirty| dirty.surface_seq());
    }
    if let Some(dirty) = dirty.as_deref() {
        mark_surface_changes(batches, &dirty.surface_changes_since(*seen_surface));
        *seen_surface = dirty.surface_seq();
    }

    // A layer gone or disabled takes its batches at once, as does one whose
    // new Kind cuts its chunks into tiles of another size (their keys name
    // tiles that no longer exist); a changed one's go stale and stay drawn
    // until their replacements are built.
    let now_secs = time.as_deref().map(|time| time.elapsed_secs_f64());
    let current: HashMap<u64, ScatterLayer> =
        layer_queries.gather().into_iter().map(|layer| (layer.id, layer)).collect();
    // What each changed layer makes stale: `Some` boxes when only its
    // footprint moved (a tile outside both could hold nothing before and
    // still holds nothing), `None` for every batch.
    let mut changed: HashMap<u64, Option<[(Vec2, Vec2); 2]>> = HashMap::new();
    let mut retiled: HashSet<u64> = HashSet::new();
    for (id, layer) in &current {
        let Some(before) = layers.get(id).filter(|before| *before != layer) else { continue };
        if let Some(now) = now_secs {
            layer_changed.insert(*id, now);
        }
        if tiles_per_chunk(before, config.chunk_size) != tiles_per_chunk(layer, config.chunk_size) {
            retiled.insert(*id);
            continue;
        }
        let same_but_footprint =
            ScatterLayer { footprint: None, ..before.clone() } == ScatterLayer { footprint: None, ..layer.clone() };
        let region = same_but_footprint
            .then(|| Some([before.footprint?.bounds()?, layer.footprint?.bounds()?]))
            .flatten();
        changed.insert(*id, region);
    }
    batches.retain(|(id, tile), record| {
        let Some(layer) = current.get(id).filter(|_| !retiled.contains(id)) else {
            despawn_record(&mut commands, record);
            return false;
        };
        match changed.get(id) {
            Some(None) => record.stale = true,
            Some(Some(boxes)) => {
                let tpc = tiles_per_chunk(layer, config.chunk_size);
                let lo = tile_origin(*tile, tpc, config);
                let hi = lo + Vec2::splat(config.chunk_size / tpc as f32);
                if boxes.iter().any(|footprint| rects_overlap((lo, hi), *footprint)) {
                    record.stale = true;
                }
            }
            None => {}
        }
        true
    });
    layer_changed.retain(|id, _| current.contains_key(id));
    *layers = current;
    if layers.is_empty() {
        return;
    }

    let size = config.chunk_size.max(1e-3);
    let viewer = scene_camera_translation(&cameras).unwrap_or(Vec3::ZERO);
    let viewer = Vec2::new(viewer.x, viewer.z);

    // Out of range, or despawned by something else (a scene cleared under
    // it): gone, and built again if it comes back into range.
    batches.retain(|(id, tile), record| {
        let alive = record.entity.is_none_or(|entity| live.contains(entity));
        let in_reach = layers.get(id).is_some_and(|layer| {
            let tile_size = size / tiles_per_chunk(layer, config.chunk_size) as f32;
            chunk_distance(*tile, tile_size, viewer) <= layer.radius + drop_margin(layer.radius, tile_size)
        });
        if alive && in_reach {
            return true;
        }
        despawn_record(&mut commands, record);
        false
    });

    // Tiles come into range without a batch, and stale batches. A tile whose
    // chunk the layer cannot place on is settled here without a build.
    let mut wanted: Vec<Wanted> = Vec::new();
    let (extent_x, extent_z) = (config.chunks_x as i32, config.chunks_z as i32);
    for layer in layers.values() {
        let tpc = tiles_per_chunk(layer, config.chunk_size);
        let tile_size = size / tpc as f32;
        let lo = ((viewer - Vec2::splat(layer.radius)) / tile_size).floor();
        let hi = ((viewer + Vec2::splat(layer.radius)) / tile_size).floor();
        let x_range = (lo.x as i32).max(-extent_x * tpc)..=(hi.x as i32).min(extent_x * tpc + tpc - 1);
        for x in x_range {
            for z in (lo.y as i32).max(-extent_z * tpc)..=(hi.y as i32).min(extent_z * tpc + tpc - 1) {
                let tile = IVec2::new(x, z);
                if batches.contains_key(&(layer.id, tile)) {
                    continue;
                }
                let distance = chunk_distance(tile, tile_size, viewer);
                if distance > layer.radius {
                    continue;
                }
                let chunk = tile_chunk(tile, tpc);
                if !layer.can_place(config, chunk) {
                    batches.insert((layer.id, tile), ScatterBatchRecord { entity: None, stale: false, chunk });
                    continue;
                }
                wanted.push(Wanted { order: layer.order, distance, layer: layer.id, tile });
            }
        }
    }
    // Wait for the stroke or drag to settle; the stale batch keeps drawing.
    // A tile with no batch yet is not held back, so streaming never stalls.
    let quiet = |since: f64| now_secs.is_none_or(|now| now - since >= SCATTER_QUIET_SECS);
    let ground_quiet = dirty.as_deref().is_none_or(|dirty| quiet(dirty.last_mark_secs));
    for (&(id, tile), record) in batches.iter_mut() {
        if !record.stale {
            continue;
        }
        let Some(layer) = layers.get(&id) else { continue };
        if !layer.can_place(config, record.chunk) {
            despawn_record(&mut commands, record);
            record.stale = false;
            continue;
        }
        if !(ground_quiet && layer_changed.get(&id).is_none_or(|&since| quiet(since))) {
            continue;
        }
        let tile_size = size / tiles_per_chunk(layer, config.chunk_size) as f32;
        wanted.push(Wanted { order: layer.order, distance: chunk_distance(tile, tile_size, viewer), layer: id, tile });
    }
    if wanted.is_empty() {
        return;
    }
    wanted.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then(a.distance.total_cmp(&b.distance))
            .then(a.layer.cmp(&b.layer))
            .then((a.tile.x, a.tile.y).cmp(&(b.tile.x, b.tile.y)))
    });

    let ground = surface_data(base, baked);
    let volume = volume.unwrap_or(TerrainVolume::empty());
    let corridors = baked.and_then(TerrainBaked::prepared);
    let started = Instant::now();
    for (built, want) in wanted.iter().enumerate() {
        if built >= MAX_BUILDS_PER_FRAME || (built > 0 && started.elapsed() >= BUILD_TIME_BUDGET) {
            break;
        }
        let Some(layer) = layers.get(&want.layer) else { continue };
        let tpc = tiles_per_chunk(layer, config.chunk_size);
        let chunk = tile_chunk(want.tile, tpc);
        let instances = place_tile(layer, want.tile, config, ground, volume, corridors);
        let corner = tile_origin(want.tile, tpc, config);
        let origin = Vec3::new(corner.x, 0.0, corner.y);
        let parts = build_batch_parts(layer, origin, &instances, &mut assets.library);
        let entity = spawn_batch(
            &mut commands,
            layer,
            chunk,
            want.tile,
            origin,
            parts,
            &mut assets,
            &mut meshes,
            &mut materials,
            asset_server.as_deref(),
        );
        // The old batch goes only now, so the scatter never blinks out.
        let previous = batches.insert((want.layer, want.tile), ScatterBatchRecord { entity, stale: false, chunk });
        if let Some(old) = previous.and_then(|record| record.entity) {
            commands.entity(old).try_despawn();
        }
    }
}

/// Places, draws and streams every `TerrainScatter` layer. Added by the
/// shared `TerrainPlugin` (the Client) and by the engine's terrain plugin.
pub struct TerrainScatterPlugin;

impl Plugin for TerrainScatterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainScatterState>()
            .init_resource::<ScatterAssets>()
            .init_resource::<TerrainDirtyChunks>()
            .add_systems(Update, update_terrain_scatter.after(apply_terrain_dirty_chunks));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::ClassName;
    use crate::terrain::layers::{LayerDesc, LayerKind, SplineLayer};
    use crate::terrain::material::material_cell;

    /// 5 x 5 chunks of 32 m at 16 cells: an 80 x 80 raster spanning world
    /// -64..96 on both axes, about 2 m a cell, heights 0..100 m.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 2,
            chunks_z: 2,
            height_scale: 100.0,
            ..TerrainConfig::default()
        }
    }

    /// The raster's cell spacing, metres.
    fn cell(config: &TerrainConfig) -> f32 {
        let (lo, hi) = config.footprint_xz();
        (hi.x - lo.x) / ((config.chunks_x * 2 + 1) * config.chunk_resolution - 1) as f32
    }

    /// Ground whose height and material at each raster cell come from
    /// `height` and `material` (world XZ in, `None` for no material layer).
    fn ground(
        config: &TerrainConfig,
        height: impl Fn(Vec2) -> f32,
        material: Option<&dyn Fn(Vec2) -> TerrainMaterial>,
    ) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let (lo, _) = config.footprint_xz();
        let step = cell(config);
        let (w, h) = (data.cache_width as usize, data.cache_height as usize);
        let at = |i: usize| lo + Vec2::new((i % w) as f32, (i / w) as f32) * step;
        for i in 0..w * h {
            data.height_cache[i] = config.normalized_height(height(at(i)));
        }
        if let Some(material) = material {
            data.material_cache = (0..w * h).map(|i| material_cell(material(at(i)).to_u8())).collect();
        }
        data
    }

    fn flat(config: &TerrainConfig) -> TerrainData {
        ground(config, |_| 0.0, None)
    }

    fn place(layer: &ScatterLayer, chunk: IVec2, config: &TerrainConfig, data: &TerrainData) -> Vec<ScatterInstance> {
        place_chunk(layer, chunk, config, data, TerrainVolume::empty(), None)
    }

    fn every_chunk(config: &TerrainConfig) -> impl Iterator<Item = IVec2> {
        let (x, z) = (config.chunks_x as i32, config.chunks_z as i32);
        (-x..=x).flat_map(move |cx| (-z..=z).map(move |cz| IVec2::new(cx, cz)))
    }

    #[test]
    fn placement_is_deterministic_for_a_seed_and_chunk() {
        let config = config();
        let data = flat(&config);
        let layer = ScatterLayer { density: 20.0, min_scale: 0.5, max_scale: 2.0, ..ScatterLayer::default() };
        let chunk = IVec2::new(1, -1);
        let first = place(&layer, chunk, &config, &data);
        assert!(!first.is_empty());
        assert_eq!(first, place(&layer, chunk, &config, &data), "the same inputs place the same instances");
        for instance in &first {
            let p = instance.position;
            assert!(p.x >= 32.0 && p.x < 64.0 && p.z >= -32.0 && p.z < 0.0, "{p} outside chunk {chunk}");
            assert!(instance.scale >= 0.5 && instance.scale <= 2.0);
            assert!(instance.tint > 0.8 && instance.tint < 1.2);
        }
        assert_ne!(first, place(&layer, IVec2::new(0, -1), &config, &data), "another chunk, another pattern");
        assert_ne!(first, place(&ScatterLayer { seed: 2, ..layer.clone() }, chunk, &config, &data), "another seed");
        assert_ne!(first, place(&ScatterLayer { id: 7, ..layer.clone() }, chunk, &config, &data), "another layer");
    }

    #[test]
    fn density_scales_the_count() {
        let config = config();
        let data = flat(&config);
        let count = |density: f32| -> usize {
            let layer = ScatterLayer { density, ..ScatterLayer::default() };
            every_chunk(&config).map(|chunk| place(&layer, chunk, &config, &data).len()).sum()
        };
        let area = 25.0 * 32.0 * 32.0;
        for density in [10.0f32, 40.0] {
            let expected = density * area / 100.0;
            let placed = count(density) as f32;
            assert!((placed - expected).abs() < expected * 0.1, "density {density}: {placed} placed, {expected} expected");
        }
        let ratio = count(40.0) as f32 / count(10.0) as f32;
        assert!((ratio - 4.0).abs() < 0.4, "four times the density gave {ratio} times the count");
        assert_eq!(count(0.0), 0);
    }

    #[test]
    fn big_chunks_are_cut_into_tiles_that_bound_a_batch() {
        // 3 x 3 chunks of 256 m: the merged kinds' tiles are 32 m, eight to
        // a chunk's side, and the entity kinds' 128 m, two to a side.
        let config = TerrainConfig { chunk_size: 256.0, chunks_x: 1, chunks_z: 1, ..config() };
        let data = flat(&config);
        let meadow = ScatterLayer { density: 40.0, ..ScatterLayer::default() };
        let tpc = tiles_per_chunk(&meadow, config.chunk_size);
        assert_eq!(tpc, 8);
        assert_eq!(tiles_per_chunk(&ScatterLayer { kind: ScatterKind::Trees, ..meadow.clone() }, config.chunk_size), 2);
        // About 1.6 m apart at 40 per 100 square metres: 21 cells a side.
        assert_eq!(cell_grid(config.chunk_size / tpc as f32, meadow.density).0, 21);

        let chunk = IVec2::new(0, -1);
        let mut tiles = Vec::new();
        for z in 0..tpc {
            for x in 0..tpc {
                let tile = chunk * tpc + IVec2::new(x, z);
                assert_eq!(tile_chunk(tile, tpc), chunk);
                let placed = place_tile(&meadow, tile, &config, &data, TerrainVolume::empty(), None);
                assert!(!placed.is_empty() && placed.len() <= 21 * 21, "tile {tile}: {} placed", placed.len());
                let lo = tile_origin(tile, tpc, &config);
                assert!(
                    placed.iter().all(|i| {
                        let p = Vec2::new(i.position.x, i.position.z);
                        p.cmpge(lo).all() && p.cmple(lo + Vec2::splat(32.0)).all()
                    }),
                    "tile {tile} placed outside its 32 m from {lo}"
                );
                tiles.extend(placed);
            }
        }
        assert_eq!(place(&meadow, chunk, &config, &data), tiles, "a chunk is its 64 tiles in row order");
    }

    #[test]
    fn an_entity_batch_holds_a_bounded_subset_of_its_pattern() {
        let config = config();
        let data = flat(&config);
        let trees = ScatterLayer { kind: ScatterKind::Trees, density: MAX_DENSITY as f32, ..ScatterLayer::default() };
        // Grass on this grid cuts a chunk into the same single tile, so it
        // draws the same numbers for the same cells, and nothing caps it.
        let grass = ScatterLayer { kind: ScatterKind::Grass, ..trees.clone() };
        assert_eq!(tiles_per_chunk(&trees, config.chunk_size), tiles_per_chunk(&grass, config.chunk_size));
        let capped = place(&trees, IVec2::ZERO, &config, &data);
        let uncapped = place(&grass, IVec2::ZERO, &config, &data);
        // A binomial count about the cap, within five spreads of it.
        let cap = MAX_ENTITY_INSTANCES_PER_BATCH;
        assert!((capped.len() as f32 - cap).abs() <= 5.0 * cap.sqrt(), "{} trees on one batch", capped.len());
        assert!(uncapped.len() > 4 * capped.len(), "{} candidates, {} kept", uncapped.len(), capped.len());
        let pattern: HashSet<(u32, u32)> =
            uncapped.iter().map(|i| (i.position.x.to_bits(), i.position.z.to_bits())).collect();
        assert!(
            capped.iter().all(|i| pattern.contains(&(i.position.x.to_bits(), i.position.z.to_bits()))),
            "the cap keeps a subset of the pattern"
        );
    }

    /// Flat at 0 m west of x = -8, a 45 degree ramp up to 80 m at x = 72,
    /// flat beyond; Rock south of z = 0, Grass north of it.
    fn ramp(config: &TerrainConfig) -> TerrainData {
        let rock_south = |p: Vec2| if p.y < 0.0 { TerrainMaterial::Rock } else { TerrainMaterial::Grass };
        ground(config, |p| (p.x + 8.0).clamp(0.0, 80.0), Some(&rock_south))
    }

    #[test]
    fn slope_and_height_rules_filter_candidates() {
        let config = config();
        let data = ramp(&config);
        let flat_chunk = IVec2::new(-2, 0);
        let ramp_chunk = IVec2::new(1, 0);
        let gentle = ScatterLayer { max_slope: 30.0, ..ScatterLayer::default() };
        assert!(!place(&gentle, flat_chunk, &config, &data).is_empty());
        assert!(place(&gentle, ramp_chunk, &config, &data).is_empty(), "the 45 degree ramp is too steep");
        let steep = ScatterLayer { min_slope: 40.0, ..ScatterLayer::default() };
        assert!(place(&steep, flat_chunk, &config, &data).is_empty(), "flat ground is too gentle");
        let on_ramp = place(&steep, ramp_chunk, &config, &data);
        assert!(!on_ramp.is_empty());
        for instance in &on_ramp {
            // Aligned to the 45 degree slope.
            let up = instance.rotation * Vec3::Y;
            assert!((up.y - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02, "tilted to {up}");
        }
        let upright = ScatterLayer { align_to_normal: false, ..steep.clone() };
        assert!(place(&upright, ramp_chunk, &config, &data).iter().all(|i| (i.rotation * Vec3::Y).y > 0.9999));

        // The ramp chunk climbs from 40 m to 72 m.
        let band = ScatterLayer { min_height: 60.0, max_height: 50.0, ..ScatterLayer::default() };
        let placed = place(&band, ramp_chunk, &config, &data);
        assert!(!placed.is_empty());
        assert!(placed.iter().all(|i| i.position.y >= 50.0 && i.position.y <= 60.0), "a backwards range still holds");
    }

    #[test]
    fn material_and_footprint_rules_filter_candidates() {
        let config = config();
        let data = ramp(&config);
        let step = cell(&config);
        let rock = ScatterLayer { material: Some(TerrainMaterial::Rock.to_u8()), ..ScatterLayer::default() };
        let south = place(&rock, IVec2::new(-2, -1), &config, &data);
        assert!(!south.is_empty(), "Rock ground takes a Rock scatter");
        let north = place(&rock, IVec2::new(-2, 1), &config, &data);
        assert!(north.is_empty(), "Grass ground refuses it");
        // At the boundary only the one-cell blend lets any through.
        let edge = place(&rock, IVec2::new(-2, 0), &config, &data);
        assert!(edge.iter().all(|i| i.position.z < step), "Rock scatter at z = {:?}", edge.iter().map(|i| i.position.z).collect::<Vec<_>>());

        // Ground without a material layer counts as Grass.
        let bare = flat(&config);
        let grass = ScatterLayer { material: Some(TerrainMaterial::Grass.to_u8()), ..ScatterLayer::default() };
        assert!(!place(&grass, IVec2::ZERO, &config, &bare).is_empty());
        assert!(place(&rock, IVec2::ZERO, &config, &bare).is_empty());

        // A 10 m square turned half a radian about its centre at (-48, -48).
        let footprint = ScatterFootprint { center: Vec2::splat(-48.0), yaw: 0.5, half: Vec2::splat(5.0) };
        let patch = ScatterLayer { footprint: Some(footprint), ..ScatterLayer::default() };
        let inside = place(&patch, IVec2::new(-2, -2), &config, &bare);
        assert!(!inside.is_empty());
        assert!(inside.iter().all(|i| footprint.contains(Vec2::new(i.position.x, i.position.z))));
        assert!(inside.iter().all(|i| Vec2::new(i.position.x, i.position.z).distance(footprint.center) <= 5.0 * 2f32.sqrt() + 1e-3));
        assert!(!patch.can_place(&config, IVec2::new(1, 1)), "a chunk clear of the footprint holds nothing");
        assert!(place(&patch, IVec2::new(1, 1), &config, &bare).is_empty());

        // An unlimited Z: a band 10 m wide in X across the whole terrain.
        let band = ScatterFootprint { center: Vec2::new(-48.0, 0.0), yaw: 0.0, half: Vec2::new(5.0, f32::INFINITY) };
        let strip = ScatterLayer { footprint: Some(band), ..ScatterLayer::default() };
        assert!(strip.can_place(&config, IVec2::new(-2, 2)));
        let placed = place(&strip, IVec2::new(-2, 2), &config, &bare);
        assert!(!placed.is_empty());
        assert!(placed.iter().all(|i| (i.position.x + 48.0).abs() <= 5.0));
    }

    fn road(mode: SplineMode) -> LayerDesc {
        LayerDesc {
            id: 50,
            order: 0,
            kind: LayerKind::Spline(SplineLayer {
                mode,
                points: vec![Vec3::new(-60.0, 0.0, 16.0), Vec3::new(90.0, 0.0, 16.0)],
                width: 8.0,
                shoulder_width: 4.0,
                depth: 2.0,
                smoothing: 0.5,
                bed_material: None,
                shoulder_material: None,
            }),
        }
    }

    #[test]
    fn avoid_roads_keeps_off_road_and_path_corridors_only() {
        let config = config();
        let data = flat(&config);
        let layer = ScatterLayer { density: 40.0, avoid_roads: true, ..ScatterLayer::default() };
        let chunk = IVec2::ZERO;
        // Along z = 16 through the chunk; the corridor reaches 8 m either side.
        let near_road = |instances: &[ScatterInstance]| instances.iter().filter(|i| (i.position.z - 16.0).abs() <= 7.9).count();

        let roads = PreparedLayers::new(&config, &data, &[road(SplineMode::Road)]);
        let avoided = place_chunk(&layer, chunk, &config, &data, TerrainVolume::empty(), Some(&roads));
        assert!(!avoided.is_empty(), "the ground either side still holds scatter");
        assert_eq!(near_road(&avoided), 0, "nothing on the road or its shoulders");
        assert!(avoided.iter().all(|i| (i.position.z - 16.0).abs() > 7.99));

        let ignored = ScatterLayer { avoid_roads: false, ..layer.clone() };
        let over = place_chunk(&ignored, chunk, &config, &data, TerrainVolume::empty(), Some(&roads));
        assert!(near_road(&over) > 0, "without AvoidRoads the road is scattered over");

        let paths = PreparedLayers::new(&config, &data, &[road(SplineMode::Path)]);
        assert_eq!(near_road(&place_chunk(&layer, chunk, &config, &data, TerrainVolume::empty(), Some(&paths))), 0);
        let river = PreparedLayers::new(&config, &data, &[road(SplineMode::River)]);
        let banks = place_chunk(&layer, chunk, &config, &data, TerrainVolume::empty(), Some(&river));
        assert!(near_road(&banks) > 0, "a river is not a road");
    }

    fn instance(position: Vec3, scale: f32, variant: u8) -> ScatterInstance {
        ScatterInstance { position, rotation: Quat::from_rotation_y(0.3), scale, variant, tint: 1.0 }
    }

    #[test]
    fn merged_batches_hold_every_small_instance() {
        let mut library = ScatterMeshLibrary::default();
        let origin = Vec3::new(32.0, 0.0, -32.0);
        let grass = ScatterLayer::default();
        let tufts: Vec<ScatterInstance> =
            (0..20u8).map(|i| instance(origin + Vec3::new(i as f32, 1.0, 3.0), 1.0, i.wrapping_mul(37))).collect();
        let parts = build_batch_parts(&grass, origin, &tufts, &mut library);
        let merged = parts.merged.as_ref().expect("grass merges");
        let expected: usize =
            tufts.iter().map(|t| library.get(instance_mesh(&grass, t).expect("built in")).vertex_count()).sum();
        assert_eq!(merged.vertex_count(), expected, "one copy of each tuft's vertices");
        let triangles: usize =
            tufts.iter().map(|t| library.get(instance_mesh(&grass, t).expect("built in")).triangle_count()).sum();
        assert_eq!(merged.triangle_count(), triangles);
        assert!(merged.indices.iter().all(|&i| (i as usize) < merged.vertex_count()));
        assert!(merged.positions.iter().all(|p| p[0] >= -1.0 && p[0] < 21.0), "positions are relative to the chunk");
        assert!(parts.entities.is_empty() && parts.colliders.is_empty());

        // Rocks straddling the large-rock size: small ones merge, large ones
        // stand alone and collide.
        let rocks = ScatterLayer { kind: ScatterKind::Rocks, collide: true, ..ScatterLayer::default() };
        let small = instance(origin, LARGE_ROCK_SIZE / ROCK_DIAMETER * 0.5, 0);
        let large = instance(origin + Vec3::X * 5.0, LARGE_ROCK_SIZE / ROCK_DIAMETER * 2.0, 1);
        let parts = build_batch_parts(&rocks, origin, &[small, large], &mut library);
        assert_eq!(parts.merged.as_ref().map(ScatterMeshData::vertex_count), Some(library.get(ScatterMesh::Rock(0)).vertex_count()));
        assert_eq!(parts.entities.len(), 1);
        assert_eq!(parts.entities[0].0, ScatterDraw::Builtin(ScatterMesh::Rock(1)));
        assert_eq!(parts.entities[0].1.translation, Vec3::X * 5.0);
        let [ScatterCollider::Ball { center, radius }] = parts.colliders.as_slice() else { panic!("{:?}", parts.colliders) };
        assert!((*center - Vec3::X * 5.0).length() < LARGE_ROCK_SIZE * 2.0 && *radius > 0.5);

        // Trees always stand alone; each collides as a trunk up its axis.
        let trees = ScatterLayer { kind: ScatterKind::Trees, collide: true, ..ScatterLayer::default() };
        let stand: Vec<ScatterInstance> = (0..6u8).map(|i| instance(origin + Vec3::Z * i as f32 * 4.0, 1.2, i)).collect();
        let parts = build_batch_parts(&trees, origin, &stand, &mut library);
        assert!(parts.merged.is_none());
        assert_eq!(parts.entities.len(), 6);
        assert_eq!(parts.colliders.len(), 6);
        let families: HashSet<bool> =
            parts.entities.iter().map(|(draw, _)| matches!(draw, ScatterDraw::Builtin(ScatterMesh::Conifer(_)))).collect();
        assert_eq!(families.len(), 2, "a mixed stand has conifers and broadleaf trees");
        for shape in &parts.colliders {
            let ScatterCollider::Capsule { a, b, radius } = *shape else { panic!("{shape:?}") };
            assert!(b.y > a.y && radius > 0.2 && (b.x - a.x).abs() < 1e-4, "an upright trunk, {a} .. {b}");
        }
        let quiet = ScatterLayer { collide: false, ..trees.clone() };
        assert!(build_batch_parts(&quiet, origin, &stand, &mut library).colliders.is_empty());

        // A custom layer draws its asset for every instance.
        let custom = ScatterLayer { kind: ScatterKind::Custom, mesh_asset: "Assets/pine.glb".into(), ..ScatterLayer::default() };
        let parts = build_batch_parts(&custom, origin, &stand, &mut library);
        assert!(parts.entities.iter().all(|(draw, _)| *draw == ScatterDraw::Custom) && parts.entities.len() == 6);
    }

    #[test]
    fn custom_meshes_load_through_the_space_source() {
        assert_eq!(
            custom_asset_urls(" Assets\\Trees\\pine.glb "),
            Some(("space://Assets/Trees/pine.glb#Mesh0/Primitive0".into(), "space://Assets/Trees/pine.glb#Material0/std".into()))
        );
        assert_eq!(
            custom_asset_urls("bundled://props/rock.glb#Scene0").map(|(mesh, _)| mesh),
            Some("bundled://props/rock.glb#Mesh0/Primitive0".into())
        );
        assert_eq!(custom_asset_urls("   "), None);
        let custom = ScatterLayer { kind: ScatterKind::Custom, ..ScatterLayer::default() };
        assert!(!custom.can_place(&config(), IVec2::ZERO), "a custom layer without a mesh places nothing");
    }

    // ── The system ───────────────────────────────────────────────────────

    struct Rig {
        world: World,
        system: bevy::ecs::system::SystemId,
        camera: Entity,
        scatter: Entity,
    }

    /// The id of the scatter instance, from its uuid.
    const LAYER_ID: u64 = 3;

    impl Rig {
        /// A 3 x 3 chunk terrain of 32 m, a camera above chunk (0, 0), and a
        /// grass layer drawn 40 m out: every chunk is in range.
        fn new() -> Self {
            let mut world = World::new();
            world.init_resource::<TerrainScatterState>();
            world.init_resource::<ScatterAssets>();
            world.init_resource::<TerrainDirtyChunks>();
            world.init_resource::<Assets<Mesh>>();
            world.init_resource::<Assets<StandardMaterial>>();
            let config = TerrainConfig { chunks_x: 1, chunks_z: 1, ..config() };
            let data = flat(&config);
            world.spawn((TerrainRoot, config, data));
            let camera = world
                .spawn((Camera3d::default(), Camera::default(), GlobalTransform::from_translation(Vec3::new(16.0, 30.0, 16.0))))
                .id();
            let scatter = world
                .spawn((
                    Instance {
                        name: "Meadow".into(),
                        class_name: ClassName::TerrainScatter,
                        archivable: true,
                        id: 0,
                        uuid: format!("{LAYER_ID:016x}{}", "0".repeat(16)),
                        ai: false,
                    },
                    TerrainScatter { density: 10.0, radius: 40.0, ..TerrainScatter::default() },
                    Transform::default(),
                ))
                .id();
            // Registered once, so its state carries over from run to run as
            // it does in a schedule.
            let system = world.register_system(update_terrain_scatter);
            Self { world, system, camera, scatter }
        }

        fn run(&mut self) {
            assert!(self.world.run_system(self.system).is_ok(), "the scatter system runs");
        }

        /// Run until nothing is left to build, as many frames as that takes.
        fn settle(&mut self) {
            for _ in 0..32 {
                self.run();
            }
        }

        fn batches(&mut self) -> Vec<(Entity, ScatterBatch)> {
            let mut query = self.world.query::<(Entity, &ScatterBatch)>();
            let mut batches: Vec<(Entity, ScatterBatch)> = query.iter(&self.world).map(|(e, b)| (e, *b)).collect();
            batches.sort_by_key(|(_, b)| (b.chunk.x, b.chunk.y));
            batches
        }

        fn entity_of(&self, chunk: IVec2) -> Option<Entity> {
            self.world.resource::<TerrainScatterState>().batch(LAYER_ID, chunk)
        }
    }

    #[test]
    fn batches_stream_in_rebuild_on_edits_and_go_with_their_layer() {
        let mut rig = Rig::new();
        rig.run();
        let first_frame = rig.batches().len();
        assert!(first_frame >= 1 && first_frame <= MAX_BUILDS_PER_FRAME, "{first_frame} built in one frame");
        rig.settle();
        let batches = rig.batches();
        assert_eq!(batches.len(), 9, "every chunk in range has its grass");
        assert!(batches.iter().all(|(_, b)| b.layer == LAYER_ID));
        let centre = rig.entity_of(IVec2::ZERO).expect("the chunk under the camera");
        assert!(rig.world.get::<Mesh3d>(centre).is_some(), "grass is one merged mesh");
        assert!(rig.world.get::<NotShadowCaster>(centre).is_some());
        let east = rig.entity_of(IVec2::X).expect("a neighbour");

        // Nothing changed: nothing rebuilt.
        rig.settle();
        assert_eq!(rig.entity_of(IVec2::ZERO), Some(centre));

        // An edit on chunk (0, 0) rebuilds its batch and only its batch.
        rig.world.resource_mut::<TerrainDirtyChunks>().mark(IVec2::ZERO);
        rig.settle();
        let rebuilt = rig.entity_of(IVec2::ZERO).expect("rebuilt");
        assert_ne!(rebuilt, centre, "the edited chunk was placed again");
        assert!(rig.world.get_entity(centre).is_err(), "and its old batch went");
        assert_eq!(rig.entity_of(IVec2::X), Some(east), "its neighbours kept theirs");

        // A property edit rebuilds every batch of the layer.
        rig.world.get_mut::<TerrainScatter>(rig.scatter).unwrap().density = 20.0;
        rig.settle();
        assert_eq!(rig.batches().len(), 9);
        assert_ne!(rig.entity_of(IVec2::X), Some(east));

        // Far from the camera nothing stays.
        *rig.world.get_mut::<GlobalTransform>(rig.camera).unwrap() = GlobalTransform::from_translation(Vec3::new(600.0, 30.0, 0.0));
        rig.settle();
        assert!(rig.batches().is_empty(), "out of range, out of the world");
        *rig.world.get_mut::<GlobalTransform>(rig.camera).unwrap() = GlobalTransform::from_translation(Vec3::new(16.0, 30.0, 16.0));
        rig.settle();
        assert_eq!(rig.batches().len(), 9, "back in range, built again");

        // Disabled: gone at once.
        rig.world.get_mut::<TerrainScatter>(rig.scatter).unwrap().enabled = false;
        rig.run();
        assert!(rig.batches().is_empty(), "a disabled layer takes its batches");
        assert_eq!(rig.world.resource::<TerrainScatterState>().batch_count(), 0);
    }

    #[test]
    fn stale_batches_wait_for_the_ground_and_the_layer_to_go_quiet() {
        let mut rig = Rig::new();
        rig.world.init_resource::<Time>();
        rig.settle();
        assert_eq!(rig.batches().len(), 9, "new tiles are never held back");
        let centre = rig.entity_of(IVec2::ZERO).expect("built");

        // A stroke still under way: marked this moment, as the dirty
        // pipeline stamps it.
        rig.world.resource_mut::<Time>().advance_by(Duration::from_secs(1));
        let now = rig.world.resource::<Time>().elapsed_secs_f64();
        {
            let mut dirty = rig.world.resource_mut::<TerrainDirtyChunks>();
            dirty.mark(IVec2::ZERO);
            dirty.last_mark_secs = now;
        }
        rig.settle();
        assert_eq!(rig.entity_of(IVec2::ZERO), Some(centre), "the stale batch keeps drawing while the ground moves");
        rig.world.resource_mut::<Time>().advance_by(Duration::from_secs_f64(SCATTER_QUIET_SECS * 2.0));
        rig.run();
        let rebuilt = rig.entity_of(IVec2::ZERO).expect("rebuilt");
        assert_ne!(rebuilt, centre, "placed again once the ground went quiet");

        // A layer edit waits the same way.
        let east = rig.entity_of(IVec2::X).expect("a neighbour");
        rig.world.get_mut::<TerrainScatter>(rig.scatter).unwrap().density = 20.0;
        rig.settle();
        assert_eq!(rig.entity_of(IVec2::X), Some(east), "held while the layer may still be changing");
        rig.world.resource_mut::<Time>().advance_by(Duration::from_secs_f64(SCATTER_QUIET_SECS * 2.0));
        rig.settle();
        assert_ne!(rig.entity_of(IVec2::X), Some(east), "placed again once the layer went quiet");
    }
}
