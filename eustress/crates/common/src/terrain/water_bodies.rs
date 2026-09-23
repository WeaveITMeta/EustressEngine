//! Lakes and rivers: the water surfaces `TerrainWaterBody` instances and
//! River-mode `TerrainSpline`s lay over the terrain. Both draw with the shared
//! water material (see `water`).
//!
//! ## Water bodies
//! A water body floods the ground from its position ([`flood_fill_water`]):
//! starting at the raster cell under it, every cell joined to that one
//! through cells whose finished height (the SURFACE data, `surface_data`) is
//! below the body's water level, inside its footprint, is under water.
//! Ground at or above the level (a ridge) stops the flood, and so does the
//! footprint's edge. The wet cells, and the dry cells beside them where the
//! shoreline runs, are meshed as a flat surface at the level, one mesh per
//! chunk ([`water_body_meshes`]). The water material drops every pixel whose
//! ground stands above the water, so the waterline follows the ground
//! between cells rather than their edges. Nothing is stored: a body fills
//! again whenever it changes, or the ground under its water or along its rim
//! does (the surface stamps of `TerrainDirtyChunks`, which brush strokes,
//! undo, volume edits and bakes all leave), at most every
//! [`WATER_REBUILD_INTERVAL_SECS`] while those keep changing.
//!
//! ## Rivers
//! A River spline with WaterSurface on carries a ribbon of water along the
//! stations its channel was carved along (`TerrainBaked::baked_spline`), at
//! the smoothed profile less Depth times (1 - WaterFill): the channel filled
//! to WaterFill of its depth ([`river_water_ribbon`]). The ribbon spans the
//! bed and as far up the banks as water that deep reaches where the ground
//! meets the profile, plus a raster cell; the material trims it to the real
//! waterline, so a layer ordered after the river that reshapes its channel
//! is trimmed there too. At each station the level is capped at the lowest
//! of its two banks at the waterline and the ribbon's two edges, on the
//! finished surface: water never stands higher than the ground beside it
//! can hold, and a reach whose banks cannot hold it (a dip the smoothed
//! profile bridges) shows dry. Its flow runs along the spline, downhill,
//! faster where the profile is steeper.
//!
//! Both are derived state, like a road's surface: not instances, never saved,
//! not in the Explorer, and without colliders.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::layer_instances::{compose_world_pose, layer_id, TerrainSpline, TerrainWaterBody};
use super::layers::{rects_overlap, surface_data, SplineLayer, SplineMode, TerrainBaked};
use super::road::RoadPath;
use super::scatter::ScatterFootprint;
use super::water::{WaterSurface, WaterSurfaceAssets, WaterSurfaceMaterial};
use super::{height_at_world, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainGridKey, TerrainRoot};
use crate::classes::Instance;

/// Water bodies fill again at most this often while their inputs keep
/// changing (a body dragged, a stroke along a shore), seconds. The final
/// state always fills once the interval has passed.
pub const WATER_REBUILD_INTERVAL_SECS: f64 = 0.1;
/// Flow speed of a level river, metres per second...
const RIVER_BASE_SPEED: f32 = 0.6;
/// ...plus this much per unit of the profile's grade...
const RIVER_GRADE_SPEED: f32 = 12.0;
/// ...up to this.
const RIVER_MAX_SPEED: f32 = 3.0;
/// A profile that rises less than this from its first station to its last,
/// metres, flows in path order.
const LEVEL_PROFILE: f32 = 0.01;

// ============================================================================
// Mesh data
// ============================================================================

/// A water surface's vertices before they become a `Mesh`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaterMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Flow velocity, world XZ metres per second: the shader's UV_1. Zero
    /// for still water.
    pub flows: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl WaterMeshData {
    /// A flat quad over world XZ `lo..hi`, its positions relative to
    /// `offset` at height 0, facing +Y, still. UV_0 is world XZ.
    fn push_flat_quad(&mut self, lo: Vec2, hi: Vec2, offset: Vec2) {
        let base = self.positions.len() as u32;
        for (x, z) in [(lo.x, lo.y), (hi.x, lo.y), (lo.x, hi.y), (hi.x, hi.y)] {
            self.positions.push([x - offset.x, 0.0, z - offset.y]);
            self.normals.push([0.0, 1.0, 0.0]);
            self.uvs.push([x, z]);
            self.flows.push([0.0, 0.0]);
        }
        // Counter-clockwise seen from above: (lo, lo), (lo, hi), (hi, lo),
        // then (hi, lo), (lo, hi), (hi, hi).
        self.indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }

    /// The mesh: positions, normals, UV_0 and the flow as UV_1.
    pub fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.flows);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

// ============================================================================
// Water bodies: the flood
// ============================================================================

/// One enabled water body as plain data in world units: what a
/// `TerrainWaterBody` instance maps onto (see `TerrainWaterBody::body`).
#[derive(Clone, Debug, PartialEq)]
pub struct WaterBodyDesc {
    /// Stable identity (the instance's).
    pub id: u64,
    /// Lower orders fill first.
    pub order: i32,
    /// World XZ the flood starts from.
    pub seed: Vec2,
    /// World Y of the water surface.
    pub level: f32,
    /// Where the water may spread, `None` for the whole terrain.
    pub footprint: Option<ScatterFootprint>,
}

/// Where the cells of a raster lie in the world: cell `(x, z)` at
/// `origin + (x, z) * step`, the inverse of `height_query::world_to_uv`
/// times `size - 1`, so a cell's centre is exactly where
/// `TerrainData::sample_height` reads it unblended.
#[derive(Clone, Copy, Debug)]
struct CellGrid {
    origin: Vec2,
    step: Vec2,
    width: usize,
    height: usize,
}

impl CellGrid {
    /// The grid of `data` over `config`'s chunk grid; `None` without a whole
    /// raster of at least two cells a side.
    fn of(config: &TerrainConfig, data: &TerrainData) -> Option<Self> {
        let (width, height) = (data.cache_width as usize, data.cache_height as usize);
        if width < 2 || height < 2 || data.height_cache.len() != width * height {
            return None;
        }
        let (min, max) = config.footprint_xz();
        let step = (max - min) / Vec2::new((width - 1) as f32, (height - 1) as f32);
        (step.is_finite() && step.x > 0.0 && step.y > 0.0).then_some(Self { origin: min, step, width, height })
    }

    fn world(&self, x: usize, z: usize) -> Vec2 {
        self.origin + Vec2::new(x as f32, z as f32) * self.step
    }

    /// The cell nearest world `p`, `None` off the raster.
    fn cell_at(&self, p: Vec2) -> Option<(usize, usize)> {
        let f = ((p - self.origin) / self.step).round();
        let last = Vec2::new((self.width - 1) as f32, (self.height - 1) as f32);
        (f.x >= 0.0 && f.y >= 0.0 && f.x <= last.x && f.y <= last.y).then(|| (f.x as usize, f.y as usize))
    }

    /// The inclusive cell box `(x0, z0, x1, z1)` whose centres lie in world
    /// box `lo..hi`, `None` when none do.
    fn cells_in(&self, lo: Vec2, hi: Vec2) -> Option<(usize, usize, usize, usize)> {
        if !(lo.is_finite() && hi.is_finite()) {
            return None;
        }
        let last = Vec2::new((self.width - 1) as f32, (self.height - 1) as f32);
        let a = ((lo - self.origin) / self.step).ceil().max(Vec2::ZERO);
        let b = ((hi - self.origin) / self.step).floor().min(last);
        (a.x <= b.x && a.y <= b.y).then(|| (a.x as usize, a.y as usize, b.x as usize, b.y as usize))
    }

    /// World height of cell `(x, z)`.
    fn height(&self, config: &TerrainConfig, data: &TerrainData, x: usize, z: usize) -> f32 {
        config.world_height(data.height_cache[z * self.width + x])
    }
}

/// The raster cells a water body floods (see the module docs), as a mask
/// over the box around them.
#[derive(Clone, Debug, PartialEq)]
pub struct WaterFill {
    /// World Y of the water surface.
    pub level: f32,
    /// Raster cell (column, row) of the mask's first cell.
    pub origin: UVec2,
    /// The mask's size in cells.
    pub size: UVec2,
    /// One flag per cell of the box, row by row: the cell is under water.
    pub wet: Vec<bool>,
    /// How many cells are wet.
    pub wet_count: usize,
    /// World XZ box of the wet cells' centres.
    pub bounds: (Vec2, Vec2),
    /// World XZ spacing of the raster's cells.
    pub cell: Vec2,
}

impl WaterFill {
    /// Whether raster cell `(x, z)` is under water.
    pub fn is_wet(&self, x: u32, z: u32) -> bool {
        let (lx, lz) = (x.wrapping_sub(self.origin.x), z.wrapping_sub(self.origin.y));
        lx < self.size.x && lz < self.size.y && self.wet[lz as usize * self.size.x as usize + lx as usize]
    }

    /// World box a change of the ground must touch to change this flood: the
    /// wet cells, grown by a cell for the rim that holds the water in.
    pub fn reach(&self) -> (Vec2, Vec2) {
        (self.bounds.0 - self.cell, self.bounds.1 + self.cell)
    }
}

/// Flood `ground` (the surface data of `config`'s terrain) from `body`'s seed:
/// every raster cell joined to the seed's cell, through the four cells beside
/// each, by cells below `body.level` inside the footprint. `None` when the
/// seed's own cell is dry, off the raster or outside the footprint, or the
/// terrain has no raster.
pub fn flood_fill_water(config: &TerrainConfig, ground: &TerrainData, body: &WaterBodyDesc) -> Option<WaterFill> {
    let grid = CellGrid::of(config, ground)?;
    if !body.level.is_finite() {
        return None;
    }
    let (seed_x, seed_z) = grid.cell_at(body.seed)?;
    // The box the flood may reach: the footprint's, or the whole raster when
    // an axis is unlimited (the footprint test below still holds the other).
    let (x0, z0, x1, z1) = match body.footprint.and_then(|footprint| footprint.bounds()) {
        Some((lo, hi)) => grid.cells_in(lo, hi)?,
        None => (0, 0, grid.width - 1, grid.height - 1),
    };
    // A height that is not finite never counts as below the level.
    let floods = |x: usize, z: usize| {
        grid.height(config, ground, x, z) < body.level
            && body.footprint.is_none_or(|footprint| footprint.contains(grid.world(x, z)))
    };
    if seed_x < x0 || seed_x > x1 || seed_z < z0 || seed_z > z1 || !floods(seed_x, seed_z) {
        return None;
    }

    let (box_w, box_h) = (x1 - x0 + 1, z1 - z0 + 1);
    let index = |x: usize, z: usize| (z - z0) * box_w + (x - x0);
    let mut seen = vec![false; box_w * box_h];
    let mut wet = vec![false; box_w * box_h];
    let mut queue = VecDeque::new();
    seen[index(seed_x, seed_z)] = true;
    queue.push_back((seed_x, seed_z));
    let (mut lo_x, mut lo_z, mut hi_x, mut hi_z) = (seed_x, seed_z, seed_x, seed_z);
    let mut wet_count = 0usize;
    while let Some((x, z)) = queue.pop_front() {
        wet[index(x, z)] = true;
        wet_count += 1;
        (lo_x, lo_z, hi_x, hi_z) = (lo_x.min(x), lo_z.min(z), hi_x.max(x), hi_z.max(z));
        let neighbours = [
            (x > x0).then(|| (x - 1, z)),
            (x < x1).then(|| (x + 1, z)),
            (z > z0).then(|| (x, z - 1)),
            (z < z1).then(|| (x, z + 1)),
        ];
        for (nx, nz) in neighbours.into_iter().flatten() {
            let i = index(nx, nz);
            if seen[i] {
                continue;
            }
            seen[i] = true;
            if floods(nx, nz) {
                queue.push_back((nx, nz));
            }
        }
    }

    // Crop the mask to the wet cells.
    let (crop_w, crop_h) = (hi_x - lo_x + 1, hi_z - lo_z + 1);
    let mut cropped = Vec::with_capacity(crop_w * crop_h);
    for z in lo_z..=hi_z {
        let row = index(lo_x, z);
        cropped.extend_from_slice(&wet[row..row + crop_w]);
    }
    Some(WaterFill {
        level: body.level,
        origin: UVec2::new(lo_x as u32, lo_z as u32),
        size: UVec2::new(crop_w as u32, crop_h as u32),
        wet: cropped,
        wet_count,
        bounds: (grid.world(lo_x, lo_z), grid.world(hi_x, hi_z)),
        cell: grid.step,
    })
}

/// The flat water surface over `fill`, one mesh per chunk of `config`'s grid,
/// in chunk order. Each mesh's positions are relative to its chunk's corner
/// at the water's level (the entity stands there). A cell's quad spans the
/// half cell around its centre, meshed a quarter (a half cell square) at a
/// time, so no two quads overlap (overlapping translucent quads would draw
/// darker). The wet cells are drawn whole. A dry shore cell holds the
/// shoreline, where the water material drops the pixels whose ground stands
/// above the water, and is drawn only in the quarters that touch a wet cell,
/// so the far side of a one-cell ridge draws no water. Cells beside the
/// water that are below its level but not flooded (outside the footprint, or
/// a hollow the ridge cuts off) are left out, or they would show water the
/// flood did not reach.
pub fn water_body_meshes(config: &TerrainConfig, ground: &TerrainData, fill: &WaterFill) -> Vec<(IVec2, WaterMeshData)> {
    let Some(grid) = CellGrid::of(config, ground) else {
        return Vec::new();
    };
    let size = config.chunk_size.max(1e-3);
    let wet = |x: usize, z: usize| fill.is_wet(x as u32, z as u32);
    // A wet cell is drawn whole. A dry cell at or above the level is drawn a
    // quarter at a time, and only where one of the three cells on that corner
    // is wet: the ground under a quarter blends the cell with exactly those
    // three, so a quarter touching no wet cell can only dip below the level
    // toward a hollow the flood did not reach (the far side of a one-cell
    // ridge), where it would draw water on the wrong side. `qx` and `qz` are
    // 0 for the quarter on the cell's low side, 1 for the high side.
    let quarter_drawn = |x: usize, z: usize, qx: usize, qz: usize| {
        if wet(x, z) {
            return true;
        }
        if grid.height(config, ground, x, z) < fill.level {
            return false;
        }
        let nx = if qx == 0 { x.checked_sub(1) } else { (x + 1 < grid.width).then_some(x + 1) };
        let nz = if qz == 0 { z.checked_sub(1) } else { (z + 1 < grid.height).then_some(z + 1) };
        nx.is_some_and(|nx| wet(nx, z))
            || nz.is_some_and(|nz| wet(x, nz))
            || nx.zip(nz).is_some_and(|(nx, nz)| wet(nx, nz))
    };
    let chunk_of = |x: usize, z: usize| (grid.world(x, z) / size).floor().as_ivec2();

    // The mask's box grown by the shore ring, clipped to the raster.
    let x0 = (fill.origin.x as usize).saturating_sub(1);
    let z0 = (fill.origin.y as usize).saturating_sub(1);
    let x1 = (fill.origin.x as usize + fill.size.x as usize).min(grid.width - 1);
    let z1 = (fill.origin.y as usize + fill.size.y as usize).min(grid.height - 1);
    let half = grid.step * 0.5;
    let mut chunks: BTreeMap<(i32, i32), WaterMeshData> = BTreeMap::new();
    // Each row of cells is walked as two half rows, and each half row in
    // half-cell columns `s`: cell `s / 2`, quarter `s % 2`. Every quarter
    // belongs to its own cell's chunk.
    for z in z0..=z1 {
        for qz in 0..2usize {
            let mut s = 2 * x0;
            let last_s = 2 * x1 + 1;
            while s <= last_s {
                if !quarter_drawn(s / 2, z, s % 2, qz) {
                    s += 1;
                    continue;
                }
                // A run of drawn quarters in one chunk becomes one quad.
                let chunk = chunk_of(s / 2, z);
                let first = s;
                while s < last_s
                    && quarter_drawn((s + 1) / 2, z, (s + 1) % 2, qz)
                    && chunk_of((s + 1) / 2, z) == chunk
                {
                    s += 1;
                }
                let lo = grid.world(first / 2, z) - half + half * Vec2::new((first % 2) as f32, qz as f32);
                let hi = grid.world(s / 2, z) - half + half * Vec2::new((s % 2 + 1) as f32, (qz + 1) as f32);
                let offset = chunk.as_vec2() * size;
                chunks.entry((chunk.x, chunk.y)).or_default().push_flat_quad(lo, hi, offset);
                s += 1;
            }
        }
    }
    chunks.into_iter().map(|((cx, cz), mesh)| (IVec2::new(cx, cz), mesh)).collect()
}

// ============================================================================
// Rivers: the ribbon
// ============================================================================

/// The `t` in 0..=1 at which `smoothstep(t)` (the bank's blend across a
/// spline's shoulder) is `y`.
fn inverse_smoothstep(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    0.5 - ((1.0 - 2.0 * y).asin() / 3.0).sin()
}

/// The water ribbon of river `spline` along `stations` (world XZ along the
/// carved channel, the smoothed profile as Y), filled to `fill` of its depth
/// (see the module docs), `margin` metres wider on each side than the
/// waterline where the ground meets the profile. `ground` is the finished
/// surface height at a world XZ; each station's level is held down to the
/// lowest ground at its two waterline points and its two edges. World-space
/// positions, flat across, sloping with the profile along; flow along the
/// tangent, downhill. `None` for a spline that is not a River, a fill of 0,
/// no depth, or fewer than two stations with any length between them.
pub fn river_water_ribbon(
    stations: &[Vec3],
    spline: &SplineLayer,
    fill: f32,
    margin: f32,
    ground: impl Fn(Vec2) -> f32,
) -> Option<WaterMeshData> {
    if spline.mode != SplineMode::River || !(fill > 0.0) {
        return None;
    }
    let fill = fill.min(1.0);
    let depth = spline.depth.abs();
    if !(depth > 0.0) {
        return None;
    }
    let path = RoadPath::from_positions(stations)?;
    // The bank carves `depth * (1 - smoothstep(t))` below the profile at `t`
    // across the shoulder, so water `fill * depth` deep reaches where
    // `smoothstep(t)` is `fill`.
    let waterline = (spline.width * 0.5).max(0.0) + spline.shoulder_width.max(0.0) * inverse_smoothstep(fill);
    let half_width = waterline + margin.max(0.0);
    if !(half_width > 0.0) {
        return None;
    }
    let drop = depth * (1.0 - fill);
    let count = path.stations.len();
    let (first, last) = (path.stations[0].pos.y, path.stations[count - 1].pos.y);
    // Water runs downhill, whichever way the points were laid.
    let downstream = if last > first + LEVEL_PROFILE { -1.0 } else { 1.0 };

    let mut mesh = WaterMeshData::default();
    for (i, station) in path.stations.iter().enumerate() {
        let (before, after) = (&path.stations[i.saturating_sub(1)], &path.stations[(i + 1).min(count - 1)]);
        let run = after.s - before.s;
        let grade = if run > 1e-4 { (after.pos.y - before.pos.y) / run } else { 0.0 };
        let speed = (RIVER_BASE_SPEED + RIVER_GRADE_SPEED * grade.abs()).min(RIVER_MAX_SPEED);
        let flow = station.tangent * (downstream * speed);
        // The surface rises `grade` per metre along the tangent.
        let normal = Vec3::new(-station.tangent.x * grade, 1.0, -station.tangent.y * grade)
            .try_normalize()
            .unwrap_or(Vec3::Y);
        let across = Vec2::new(-station.tangent.y, station.tangent.x);
        let centre = Vec2::new(station.pos.x, station.pos.z);
        // The carve only lowers ground, so nothing holds water above ground
        // that stands below the level beside the channel (a dip the smoothed
        // profile bridges, a hillside's low bank, a point raised off the
        // ground). Drop it to the lowest of the two waterlines and the
        // ribbon's two edges, and the material shows the reach dry where that
        // sinks under the bed. `f32::min` skips a NaN sample.
        let y = [waterline, half_width]
            .into_iter()
            .flat_map(|d| [centre + across * d, centre - across * d])
            .map(&ground)
            .fold(station.pos.y - drop, f32::min);
        // Left (the tangent's anticlockwise side) first, as the road ribbon.
        for (side, u) in [(1.0f32, 0.0f32), (-1.0, 1.0)] {
            let p = centre + across * (half_width * side);
            mesh.positions.push([p.x, y, p.y]);
            mesh.normals.push(normal.to_array());
            mesh.uvs.push([u, station.s]);
            mesh.flows.push(flow.to_array());
        }
    }
    for i in 0..(count - 1) as u32 {
        let base = i * 2;
        // Counter-clockwise seen from above.
        mesh.indices.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
    }
    Some(mesh)
}

// ============================================================================
// Water bodies: ECS
// ============================================================================

/// Marks one chunk's water surface of the water body `body`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaterBodySurface {
    pub body: Entity,
    pub chunk: IVec2,
}

/// What a water body's surfaces were last built from.
#[derive(Debug)]
struct BuiltBody {
    desc: WaterBodyDesc,
    /// World box a change of the ground must touch to change the flood (see
    /// [`WaterFill::reach`]), or the cells around the seed while nothing is
    /// wet, since only the seed's cell going under water starts a flood.
    reach: (Vec2, Vec2),
    surfaces: Vec<Entity>,
}

/// Bookkeeping of [`sync_water_bodies`].
#[derive(Resource, Debug, Default)]
pub struct WaterBodyState {
    built: HashMap<Entity, BuiltBody>,
    /// Bodies waiting to fill again.
    pending: HashSet<Entity>,
    /// `TerrainDirtyChunks::surface_seq` caught up to.
    seen_surface: u64,
    /// The terrain root and grid the surfaces stand on.
    terrain: Option<(Entity, TerrainGridKey)>,
    /// `Time<Real>` seconds of the last fill, `None` before the first.
    last_build: Option<f64>,
}

fn despawn_surfaces(commands: &mut Commands, surfaces: &[Entity]) {
    for surface in surfaces {
        commands.entity(*surface).try_despawn();
    }
}

/// Keep every enabled water body's surfaces filled over the finished ground
/// (see the module docs). Runs after `apply_terrain_dirty_chunks`, so a flood
/// reads the ground that pass just edited or baked.
#[allow(clippy::too_many_arguments)]
pub fn sync_water_bodies(
    mut commands: Commands,
    time: Option<Res<Time<Real>>>,
    mut state: ResMut<WaterBodyState>,
    dirty: Option<Res<TerrainDirtyChunks>>,
    roots: Query<(Entity, &TerrainConfig, &TerrainData, Option<&TerrainBaked>), With<TerrainRoot>>,
    bodies: Query<(Entity, Option<&Instance>, &TerrainWaterBody)>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    live: Query<(), With<WaterBodySurface>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<WaterSurfaceMaterial>>>,
    mut water: ResMut<WaterSurfaceAssets>,
) {
    let WaterBodyState { built, pending, seen_surface, terrain, last_build } = &mut *state;
    let (Some((root, config, base, baked)), Some(mut meshes), Some(mut materials)) =
        (roots.iter().next(), meshes, materials)
    else {
        // No terrain to hold water, or a host that draws nothing.
        for (_, body) in built.drain() {
            despawn_surfaces(&mut commands, &body.surfaces);
        }
        pending.clear();
        *terrain = None;
        return;
    };

    // A new terrain (regenerated, imported, another Space) or a new grid:
    // nothing built fits it, and its own history of marks starts now.
    let key = TerrainGridKey::of(config);
    if *terrain != Some((root, key)) {
        for (_, body) in built.drain() {
            despawn_surfaces(&mut commands, &body.surfaces);
        }
        pending.clear();
        *terrain = Some((root, key));
        *seen_surface = dirty.as_ref().map_or(0, |dirty| dirty.surface_seq());
    }

    // Poses are composed from the `Transform`s of the body and its
    // ancestors, as the baked layers' are, since `GlobalTransform` lags a
    // frame.
    let current: HashMap<Entity, WaterBodyDesc> = bodies
        .iter()
        .filter(|(_, _, body)| body.enabled)
        .map(|(entity, instance, body)| {
            let pose = compose_world_pose(
                entity,
                |e| transforms.get(e).ok().copied(),
                |e| parents.get(e).ok().map(ChildOf::parent),
            );
            (entity, body.body(layer_id(instance.map(|i| i.uuid.as_str()), entity), &pose))
        })
        .collect();

    // A body gone or disabled takes its water at once.
    built.retain(|entity, body| {
        let keep = current.contains_key(entity);
        if !keep {
            despawn_surfaces(&mut commands, &body.surfaces);
        }
        keep
    });
    pending.retain(|entity| current.contains_key(entity));

    // New, changed, or a surface despawned by something else (a scene
    // cleared under it).
    for (entity, desc) in &current {
        let fresh = built
            .get(entity)
            .is_some_and(|body| body.desc == *desc && body.surfaces.iter().all(|surface| live.contains(*surface)));
        if !fresh {
            pending.insert(*entity);
        }
    }

    // Ground changed under the water or along its rim.
    if let Some(dirty) = dirty.as_deref() {
        let changes = dirty.surface_changes_since(*seen_surface);
        *seen_surface = dirty.surface_seq();
        if changes.all {
            pending.extend(built.keys().copied());
        } else if !changes.chunks.is_empty() {
            let size = config.chunk_size;
            for (entity, body) in built.iter() {
                let touched = changes.chunks.iter().any(|chunk| {
                    let lo = chunk.as_vec2() * size;
                    rects_overlap((lo, lo + Vec2::splat(size)), body.reach)
                });
                if touched {
                    pending.insert(*entity);
                }
            }
        }
    }

    if pending.is_empty() {
        return;
    }
    let now = time.map(|time| time.elapsed_secs_f64());
    if let (Some(now), Some(last)) = (now, *last_build) {
        if now - last < WATER_REBUILD_INTERVAL_SECS {
            return;
        }
    }
    *last_build = now;

    let ground = surface_data(base, baked);
    let material = water.material(&mut materials);
    let cell = Vec2::splat(config.chunk_size / config.chunk_resolution.max(1) as f32);
    let mut order: Vec<Entity> = pending.drain().collect();
    order.sort_by_key(|entity| current.get(entity).map(|desc| (desc.order, desc.id, entity.to_bits())));
    for entity in order {
        let Some(desc) = current.get(&entity) else { continue };
        let fill = flood_fill_water(config, ground, desc);
        let reach = fill.as_ref().map_or((desc.seed - cell, desc.seed + cell), WaterFill::reach);
        let mut surfaces = Vec::new();
        if let Some(fill) = &fill {
            for (chunk, data) in water_body_meshes(config, ground, fill) {
                let corner = chunk.as_vec2() * config.chunk_size;
                let surface = commands
                    .spawn((
                        Name::new(format!("Water_{}_{}", chunk.x, chunk.y)),
                        WaterBodySurface { body: entity, chunk },
                        WaterSurface,
                        // Translucent water casts no shadow onto the ground
                        // it shows.
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
        // The old surfaces go in the same command flush the new ones spawn
        // in, so the water never blinks out.
        if let Some(old) = built.insert(entity, BuiltBody { desc: desc.clone(), reach, surfaces }) {
            despawn_surfaces(&mut commands, &old.surfaces);
        }
    }
}

// ============================================================================
// Rivers: ECS
// ============================================================================

/// Marks the water ribbon built for the river spline `spline`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiverWaterSurface {
    pub spline: Entity,
}

/// What a river's ribbon was last built from.
#[derive(Debug)]
struct BuiltRiver {
    surface: Entity,
    stations: Vec<Vec3>,
    layer: SplineLayer,
    fill: f32,
    margin: f32,
    /// The ribbon's vertex heights: a bank reshaped under an unchanged
    /// profile shows only here.
    heights: Vec<f32>,
}

/// The ribbon built for each river spline, keyed by the spline's entity.
#[derive(Resource, Debug, Default)]
pub struct RiverWaterState {
    built: HashMap<Entity, BuiltRiver>,
    /// Something changed that has not been looked at yet: the bake was
    /// waiting on a replaced layer list when it was noticed.
    pending: bool,
}

impl RiverWaterState {
    /// The water ribbon of river spline `spline`, if one is built.
    pub fn surface_of(&self, spline: Entity) -> Option<Entity> {
        self.built.get(&spline).map(|built| built.surface)
    }
}

/// Keep one water ribbon per enabled River spline with WaterSurface on,
/// matching the stations its channel was baked along and its WaterFill (see
/// the module docs). Runs after `apply_terrain_dirty_chunks`, which re-bakes,
/// so a moved point's water follows in the frame its channel does.
#[allow(clippy::too_many_arguments)]
pub fn sync_river_water(
    mut commands: Commands,
    mut state: ResMut<RiverWaterState>,
    roots: Query<(&TerrainConfig, Ref<TerrainBaked>), With<TerrainRoot>>,
    splines: Query<(Entity, Option<&Instance>, &TerrainSpline)>,
    // WaterSurface and WaterFill bake nothing, so an edit of either leaves
    // the bake alone; the spline's own change is what shows it.
    touched: Query<(), Changed<TerrainSpline>>,
    live: Query<(), With<RiverWaterSurface>>,
    mut removed: RemovedComponents<TerrainSpline>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<WaterSurfaceMaterial>>>,
    mut water: ResMut<WaterSurfaceAssets>,
) {
    let spline_removed = !removed.is_empty();
    removed.clear();
    let root = roots.iter().next();
    // Read through `Deref` first so an idle frame leaves the resource alone.
    let lost = state.built.values().any(|built| !live.contains(built.surface));
    let noticed = spline_removed
        || lost
        || !touched.is_empty()
        || root.as_ref().is_some_and(|(_, baked)| baked.is_changed())
        || (root.is_none() && !state.built.is_empty());
    if !noticed && !state.pending {
        return;
    }
    let state = &mut *state;

    // A ribbon whose spline is gone goes at once, bake or no bake.
    state.built.retain(|spline, built| {
        let keep = splines.contains(*spline);
        if !keep {
            commands.entity(built.surface).try_despawn();
        }
        keep
    });

    let (Some((config, baked)), Some(mut meshes), Some(mut materials)) = (root, meshes, materials) else {
        // No bake means no layers, so no rivers; or a host that draws
        // nothing.
        for (_, built) in state.built.drain() {
            commands.entity(built.surface).try_despawn();
        }
        state.pending = false;
        return;
    };
    // A replaced layer list is laid only by the next bake; until then the
    // stations are not there to read, which is not the same as no river.
    if baked.wants_bake() {
        state.pending = true;
        return;
    }
    state.pending = false;

    let margin = config.chunk_size / config.chunk_resolution.max(1) as f32;
    // Procedural terrain has no raster for the bake to carve a channel into,
    // so a river there would be water standing on nothing.
    let carved = !baked.data.height_cache.is_empty();
    let mut kept: HashMap<Entity, BuiltRiver> = HashMap::with_capacity(state.built.len());
    for (spline_entity, instance, spline) in &splines {
        if !carved || spline.mode != SplineMode::River || !spline.water_surface {
            continue;
        }
        let id = layer_id(instance.map(|i| i.uuid.as_str()), spline_entity);
        let Some((layer, stations)) = baked.baked_spline(id) else { continue };
        let fill = spline.water_fill.clamp(0.0, 1.0) as f32;
        let previous = state.built.remove(&spline_entity);
        let reuse = previous.as_ref().map(|built| built.surface).filter(|surface| live.contains(*surface));
        let ground = |p: Vec2| height_at_world(config, &baked.data, p.x, p.y);
        let Some(data) = river_water_ribbon(stations, layer, fill, margin, ground) else {
            if let Some(surface) = reuse {
                commands.entity(surface).try_despawn();
            }
            continue;
        };
        let heights: Vec<f32> = data.positions.iter().map(|p| p[1]).collect();
        // A bank reshaped under an unchanged profile re-bakes (which is what
        // noticed it) but keeps the same stations, so only the heights the
        // ground now caps the water at can show it.
        let unchanged = previous.as_ref().is_some_and(|built| {
            built.fill == fill
                && built.margin == margin
                && built.layer == *layer
                && built.stations.as_slice() == stations
                && built.heights == heights
        });
        if unchanged && reuse.is_some() {
            kept.extend(previous.map(|built| (spline_entity, built)));
            continue;
        }
        // The old mesh asset goes with the handle this replaces.
        let mesh = Mesh3d(meshes.add(data.into_mesh()));
        let surface = match reuse {
            Some(surface) => {
                commands.entity(surface).try_insert(mesh);
                surface
            }
            None => commands
                .spawn((
                    Name::new("RiverWater"),
                    RiverWaterSurface { spline: spline_entity },
                    WaterSurface,
                    NotShadowCaster,
                    mesh,
                    MeshMaterial3d(water.material(&mut materials)),
                    // The ribbon is laid in world space.
                    Transform::IDENTITY,
                    Visibility::default(),
                ))
                .id(),
        };
        kept.insert(
            spline_entity,
            BuiltRiver { surface, stations: stations.to_vec(), layer: layer.clone(), fill, margin, heights },
        );
    }

    // Whatever was built and is not wanted any more: a spline disabled,
    // switched out of River mode, its water turned off or emptied.
    for (_, built) in state.built.drain() {
        commands.entity(built.surface).try_despawn();
    }
    state.built = kept;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::height_query::height_at_world;
    use crate::terrain::road::smoothstep;

    /// 3 x 3 chunks of 32 m at 32 cells: a 96 x 96 raster with cells about
    /// 1 m apart, spanning world -32..64 on both axes, heights 0..100 m.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 32,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            height_offset: 0.0,
            ..TerrainConfig::default()
        }
    }

    /// A raster whose world height is `height(x, z)` at every cell centre.
    fn ground_with(config: &TerrainConfig, height: impl Fn(Vec2) -> f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let grid = CellGrid::of(config, &data).expect("a whole raster");
        for z in 0..grid.height {
            for x in 0..grid.width {
                data.height_cache[z * grid.width + x] = config.normalized_height(height(grid.world(x, z)));
            }
        }
        data
    }

    /// Two round basins 10 m deep and 20 m in radius, centred at x = 0 and
    /// x = 32 on z = 16, overlapping so that the lowest point of the ridge
    /// between them, at (16, 16), stands 6.4 m high; 10 m ground elsewhere.
    fn two_basins(p: Vec2) -> f32 {
        let bowl = |centre: Vec2| {
            let r = p.distance(centre);
            if r < 20.0 { 10.0 * (r / 20.0).powi(2) } else { 10.0 }
        };
        bowl(Vec2::new(0.0, 16.0)).min(bowl(Vec2::new(32.0, 16.0)))
    }

    fn body(seed: Vec2, level: f32, footprint: Option<ScatterFootprint>) -> WaterBodyDesc {
        WaterBodyDesc { id: 1, order: 0, seed, level, footprint }
    }

    #[test]
    fn the_flood_stays_below_its_level_inside_its_footprint_and_stops_at_ridges() {
        let config = config();
        let ground = ground_with(&config, two_basins);
        let grid = CellGrid::of(&config, &ground).unwrap();
        let height = |x: usize, z: usize| grid.height(&config, &ground, x, z);

        // Level 5 m from the west basin's bottom: the ridge (6.4 m) holds it.
        let fill = flood_fill_water(&config, &ground, &body(Vec2::new(0.0, 16.0), 5.0, None)).expect("wet");
        assert!(fill.wet_count > 0);
        let mut east_wet = 0;
        for z in 0..grid.height {
            for x in 0..grid.width {
                if !fill.is_wet(x as u32, z as u32) {
                    continue;
                }
                assert!(height(x, z) < 5.0, "cell ({x}, {z}) at {} m is above the level", height(x, z));
                if grid.world(x, z).x > 16.0 {
                    east_wet += 1;
                }
            }
        }
        assert_eq!(east_wet, 0, "the ridge stops the flood; the east basin stays dry");
        // Every cell below the level in the west basin is wet: the flood
        // reached all of it.
        for z in 0..grid.height {
            for x in 0..grid.width {
                let p = grid.world(x, z);
                if p.x < 15.0 && height(x, z) < 5.0 {
                    assert!(fill.is_wet(x as u32, z as u32), "({x}, {z}) at {} m was not reached", height(x, z));
                }
            }
        }

        // Level 7 m tops the ridge's saddle: both basins fill as one.
        let over = flood_fill_water(&config, &ground, &body(Vec2::new(0.0, 16.0), 7.0, None)).expect("wet");
        let (east_x, east_z) = grid.cell_at(Vec2::new(32.0, 16.0)).unwrap();
        assert!(over.is_wet(east_x as u32, east_z as u32), "over the ridge the east basin fills too");

        // A footprint 8 m across holds the same flood inside it.
        let footprint = ScatterFootprint { center: Vec2::new(0.0, 16.0), yaw: 0.3, half: Vec2::splat(4.0) };
        let held = flood_fill_water(&config, &ground, &body(Vec2::new(0.0, 16.0), 5.0, Some(footprint))).expect("wet");
        assert!(held.wet_count < fill.wet_count);
        for z in 0..grid.height {
            for x in 0..grid.width {
                if held.is_wet(x as u32, z as u32) {
                    assert!(footprint.contains(grid.world(x, z)), "({x}, {z}) is outside the footprint");
                    assert!(height(x, z) < 5.0);
                }
            }
        }

        // A seed on dry ground, or off the raster, floods nothing.
        assert!(flood_fill_water(&config, &ground, &body(Vec2::new(16.0, 16.0), 5.0, None)).is_none(), "on the ridge");
        assert!(flood_fill_water(&config, &ground, &body(Vec2::new(500.0, 16.0), 5.0, None)).is_none());
        assert!(flood_fill_water(&config, &TerrainData::procedural(), &body(Vec2::ZERO, 5.0, None)).is_none());
    }

    #[test]
    fn a_lake_is_meshed_flat_per_chunk_over_its_wet_cells_and_their_shore() {
        let config = config();
        let ground = ground_with(&config, two_basins);
        let grid = CellGrid::of(&config, &ground).unwrap();
        let fill = flood_fill_water(&config, &ground, &body(Vec2::new(0.0, 16.0), 5.0, None)).expect("wet");
        let meshes = water_body_meshes(&config, &ground, &fill);
        assert!(meshes.len() >= 2, "the west basin straddles chunks: {:?}", meshes.iter().map(|(c, _)| *c).collect::<Vec<_>>());

        let mut covered = 0.0f32;
        for (chunk, mesh) in &meshes {
            let corner = chunk.as_vec2() * config.chunk_size;
            assert_eq!(mesh.positions.len() % 4, 0);
            assert_eq!(mesh.indices.len(), mesh.positions.len() / 4 * 6);
            assert!(mesh.flows.iter().all(|flow| *flow == [0.0, 0.0]), "a lake is still");
            for quad in mesh.positions.chunks(4) {
                assert!(quad.iter().all(|p| p[1] == 0.0), "flat: the entity stands at the level");
                let (lo, hi) = (Vec2::new(quad[0][0], quad[0][2]) + corner, Vec2::new(quad[3][0], quad[3][2]) + corner);
                covered += (hi - lo).x * (hi - lo).y;
            }
            for triangle in mesh.indices.chunks(3) {
                let at = |i: u32| Vec3::from(mesh.positions[i as usize]);
                let normal = (at(triangle[1]) - at(triangle[0])).cross(at(triangle[2]) - at(triangle[0]));
                assert!(normal.y > 0.0, "every triangle faces up");
            }
        }
        // At least the wet cells are covered, one cell's area each, and no
        // cell twice: the shore ring adds less than the wet area again.
        let cell_area = grid.step.x * grid.step.y;
        let wet_area = fill.wet_count as f32 * cell_area;
        assert!(covered >= wet_area - 1e-2, "{covered} m2 drawn over {wet_area} m2 of water");
        assert!(covered < wet_area * 2.0, "{covered} m2 drawn over {wet_area} m2 of water");
    }

    fn river(depth: f32) -> SplineLayer {
        SplineLayer {
            mode: SplineMode::River,
            points: Vec::new(),
            width: 8.0,
            shoulder_width: 4.0,
            depth,
            smoothing: 0.5,
            bed_material: None,
            shoulder_material: None,
        }
    }

    #[test]
    fn a_river_ribbon_follows_the_profile_filled_to_its_fraction() {
        // A profile falling 3 m over 40 m along +X, then rising 1 m.
        let stations: Vec<Vec3> = (0..=20)
            .map(|i| {
                let x = i as f32 * 2.5;
                let y = if x <= 40.0 { 10.0 - x * 0.075 } else { 7.0 + (x - 40.0) * 0.1 };
                Vec3::new(x, y, 5.0)
            })
            .collect();
        let margin = 0.5;
        // Ground that never caps the level: the profile's own geometry.
        let open = |_: Vec2| f32::INFINITY;
        for fill in [0.25f32, 0.5, 1.0] {
            let mesh = river_water_ribbon(&stations, &river(2.0), fill, margin, open).expect("a ribbon");
            assert_eq!(mesh.positions.len(), stations.len() * 2, "two edges per station");
            assert_eq!(mesh.indices.len(), (stations.len() - 1) * 6);
            let half = 4.0 + 4.0 * inverse_smoothstep(fill) + margin;
            for (i, station) in stations.iter().enumerate() {
                for edge in &mesh.positions[i * 2..i * 2 + 2] {
                    let expected = station.y - 2.0 * (1.0 - fill);
                    assert!((edge[1] - expected).abs() < 1e-5, "fill {fill}: station {i} water at {} for {expected}", edge[1]);
                    let lateral = Vec2::new(edge[0] - station.x, edge[2] - station.z).length();
                    assert!((lateral - half).abs() < 1e-3, "fill {fill}: half width {lateral} for {half}");
                }
            }
            for triangle in mesh.indices.chunks(3) {
                let at = |i: u32| Vec3::from(mesh.positions[i as usize]);
                let normal = (at(triangle[1]) - at(triangle[0])).cross(at(triangle[2]) - at(triangle[0]));
                assert!(normal.y > 0.0, "every triangle faces up");
            }
            // The profile ends higher than it starts less than it falls, so
            // the water runs along +X, and fastest where it is steepest.
            let flow = |i: usize| Vec2::from(mesh.flows[i * 2]);
            assert!(flow(2).x > 0.0 && flow(2).y.abs() < 1e-5, "downstream along the spline: {:?}", flow(2));
            assert!(flow(18).x > 0.0, "the rise at the end still flows the river's way");
        }
        // The waterline where the ground meets the profile: the bank's depth
        // there is exactly the water's.
        for fill in [0.1f32, 0.5, 0.9] {
            let t = inverse_smoothstep(fill);
            assert!((smoothstep(t) - fill).abs() < 1e-4, "inverse of {fill}");
        }

        // A profile that rises along the points flows back toward the first.
        let rising: Vec<Vec3> = stations.iter().map(|s| Vec3::new(s.x, 20.0 - s.y, s.z)).collect();
        let mesh = river_water_ribbon(&rising, &river(2.0), 0.5, margin, open).expect("a ribbon");
        assert!(mesh.flows[4][0] < 0.0, "water runs downhill: {:?}", mesh.flows[4]);

        // Nothing to fill.
        assert!(river_water_ribbon(&stations, &river(2.0), 0.0, margin, open).is_none());
        assert!(river_water_ribbon(&stations, &river(0.0), 0.5, margin, open).is_none());
        assert!(river_water_ribbon(&stations[..1], &river(2.0), 0.5, margin, open).is_none());
        let road = SplineLayer { mode: SplineMode::Road, ..river(2.0) };
        assert!(river_water_ribbon(&stations, &road, 0.5, margin, open).is_none(), "only a river holds water");
    }

    #[test]
    fn a_river_spline_gets_water_that_follows_its_fill_and_goes_with_it() {
        use crate::classes::ClassName;
        use crate::terrain::layers::{LayerDesc, LayerKind};
        use crate::terrain::TerrainRebake;
        use bevy::mesh::VertexAttributeValues;

        let config = config();
        let ground = ground_with(&config, |_| 10.0);
        let layer = SplineLayer {
            points: vec![Vec3::new(-20.0, 10.0, 16.0), Vec3::new(40.0, 10.0, 16.0)],
            ..river(2.0)
        };
        let mut baked = TerrainBaked::new(&ground, vec![LayerDesc { id: 7, order: 0, kind: LayerKind::Spline(layer) }]);
        baked.rebake(&config, &ground, &TerrainRebake::default());
        let stations = baked.baked_spline(7).expect("the river is baked").1.to_vec();

        let mut world = World::new();
        world.init_resource::<RiverWaterState>();
        world.init_resource::<WaterSurfaceAssets>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<WaterSurfaceMaterial>>();
        world.spawn((TerrainRoot, config.clone(), ground.clone(), baked));
        let spline = world
            .spawn((
                Instance {
                    name: "River".into(),
                    class_name: ClassName::TerrainSpline,
                    archivable: true,
                    id: 0,
                    uuid: format!("{:016x}{}", 7, "0".repeat(16)),
                    ai: false,
                },
                TerrainSpline { mode: SplineMode::River, depth: 2.0, ..TerrainSpline::default() },
            ))
            .id();
        // Registered once, so change detection and the removal reader carry
        // over from run to run as they do in a schedule.
        let system = world.register_system(sync_river_water);
        let run = |world: &mut World| assert!(world.run_system(system).is_ok(), "the river system runs");
        let surfaces = |world: &mut World| {
            let mut query = world.query::<(Entity, &RiverWaterSurface)>();
            query.iter(world).map(|(entity, surface)| (entity, *surface)).collect::<Vec<_>>()
        };
        let heights = |world: &World, surface: Entity| {
            let handle = world.get::<Mesh3d>(surface).expect("a ribbon").0.clone();
            let mesh = world.resource::<Assets<Mesh>>().get(&handle).expect("the ribbon mesh");
            match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(positions)) => positions.iter().map(|p| p[1]).collect::<Vec<_>>(),
                other => panic!("ribbon positions {other:?}"),
            }
        };

        run(&mut world);
        let found = surfaces(&mut world);
        assert_eq!(found.len(), 1, "one ribbon per river");
        let (surface, marker) = found[0];
        assert_eq!(marker.spline, spline);
        assert_eq!(world.resource::<RiverWaterState>().surface_of(spline), Some(surface));
        assert!(world.get::<WaterSurface>(surface).is_some(), "drawn with the water material");
        // At most the fill's level, and only a few centimetres under it:
        // the bilinear ground at the waterline, on the carved bank, reads a
        // little low.
        let full = TerrainSpline::default().water_fill as f32;
        for (i, y) in heights(&world, surface).iter().enumerate() {
            let expected = stations[i / 2].y - 2.0 * (1.0 - full);
            assert!(*y <= expected + 1e-5 && *y > expected - 0.05, "vertex {i} at {y} for {expected}");
        }

        // A fuller channel: the same entity, its water at the profile.
        world.get_mut::<TerrainSpline>(spline).unwrap().water_fill = 1.0;
        run(&mut world);
        assert_eq!(surfaces(&mut world), vec![(surface, marker)]);
        for (i, y) in heights(&world, surface).iter().enumerate() {
            let expected = stations[i / 2].y;
            assert!(*y <= expected + 1e-5 && *y > expected - 0.05, "full to the profile at vertex {i}: {y}");
        }

        // Water off, then on again, then the spline goes.
        world.get_mut::<TerrainSpline>(spline).unwrap().water_surface = false;
        run(&mut world);
        assert!(surfaces(&mut world).is_empty(), "no water without WaterSurface");
        world.get_mut::<TerrainSpline>(spline).unwrap().water_surface = true;
        run(&mut world);
        assert_eq!(surfaces(&mut world).len(), 1);
        world.despawn(spline);
        run(&mut world);
        assert!(surfaces(&mut world).is_empty(), "the removed river's water is gone");
        assert!(world.resource::<RiverWaterState>().surface_of(spline).is_none());
    }

    #[test]
    fn a_baked_river_carries_its_water_on_the_carved_channel() {
        use crate::terrain::layers::{LayerDesc, LayerKind};
        use crate::terrain::TerrainRebake;

        // A river carved 2 m into flat 10 m ground, its points on the
        // ground: the water sits on the channel's bed plus the fill.
        let config = config();
        let ground = ground_with(&config, |_| 10.0);
        let layer = SplineLayer {
            points: vec![Vec3::new(-20.0, 10.0, 16.0), Vec3::new(40.0, 10.0, 16.0)],
            ..river(2.0)
        };
        let mut baked = TerrainBaked::new(&ground, vec![LayerDesc { id: 7, order: 0, kind: LayerKind::Spline(layer) }]);
        baked.rebake(&config, &ground, &TerrainRebake::default());
        let (layer, stations) = baked.baked_spline(7).expect("the river is baked");
        let surface = surface_data(&ground, Some(&baked));
        let mesh = river_water_ribbon(stations, layer, 0.5, 1.0, |p| height_at_world(&config, surface, p.x, p.y))
            .expect("a ribbon");
        for (i, station) in stations.iter().enumerate() {
            let bed = height_at_world(&config, surface, station.x, station.z);
            assert!((bed - 8.0).abs() < 0.05, "the bed under station {i} is at {bed}");
            assert!((mesh.positions[i * 2][1] - 9.0).abs() < 0.05, "half full: 1 m of water over the bed");
        }
    }

    #[test]
    fn river_water_never_stands_above_the_ground_beside_it() {
        use crate::terrain::layers::{LayerDesc, LayerKind};
        use crate::terrain::TerrainRebake;

        // Flat 10 m ground under a river whose middle point is raised 4 m off
        // it. The carve only lowers ground, so along the rise the smoothed
        // profile runs over ground it never cut, and water laid by the
        // profile alone would hang there in the air.
        let config = config();
        let ground = ground_with(&config, |_| 10.0);
        let layer = SplineLayer {
            points: vec![Vec3::new(-20.0, 10.0, 16.0), Vec3::new(10.0, 14.0, 16.0), Vec3::new(40.0, 10.0, 16.0)],
            ..river(2.0)
        };
        let mut baked = TerrainBaked::new(&ground, vec![LayerDesc { id: 7, order: 0, kind: LayerKind::Spline(layer) }]);
        baked.rebake(&config, &ground, &TerrainRebake::default());
        let (layer, stations) = baked.baked_spline(7).expect("the river is baked");
        let surface = surface_data(&ground, Some(&baked));
        let height = |p: Vec2| height_at_world(&config, surface, p.x, p.y);
        let (fill, margin) = (0.8f32, 1.0f32);
        let mesh = river_water_ribbon(stations, layer, fill, margin, height).expect("a ribbon");
        let uncapped = river_water_ribbon(stations, layer, fill, margin, |_| f32::INFINITY).expect("a ribbon");
        assert!(
            uncapped.positions.iter().zip(&mesh.positions).any(|(u, c)| u[1] > c[1] + 1.0),
            "the rise lifts the profile clear of the ground"
        );

        let waterline = layer.width * 0.5 + layer.shoulder_width * inverse_smoothstep(fill);
        let path = RoadPath::from_positions(stations).expect("a path");
        assert_eq!(mesh.positions.len(), path.stations.len() * 2);
        for (i, station) in path.stations.iter().enumerate() {
            let across = Vec2::new(-station.tangent.y, station.tangent.x);
            let centre = Vec2::new(station.pos.x, station.pos.z);
            for (k, side) in [1.0f32, -1.0].into_iter().enumerate() {
                let vertex = mesh.positions[i * 2 + k];
                let at_vertex = height(Vec2::new(vertex[0], vertex[2]));
                let at_waterline = height(centre + across * (waterline * side));
                assert!(
                    vertex[1] <= at_vertex.min(at_waterline) + 1e-4,
                    "station {i}: water at {} over ground {at_vertex} at its edge, {at_waterline} at its waterline",
                    vertex[1]
                );
            }
        }
    }

    #[test]
    fn a_dry_shore_cell_draws_water_only_on_its_wet_side() {
        let config = config();
        let grid = CellGrid::of(&config, &ground_with(&config, |_| 0.0)).unwrap();
        // A one-cell ridge at column 48 standing just above the 5 m level, a
        // lake west of it, and east of it a hollow below the level that the
        // ridge keeps the flood out of.
        let ridge_x = grid.world(48, 0).x;
        let half = grid.step.x * 0.5;
        let ground = ground_with(&config, |p| {
            if (p.x - ridge_x).abs() < half {
                5.5
            } else if p.x < ridge_x {
                2.0
            } else {
                1.0
            }
        });
        let fill = flood_fill_water(&config, &ground, &body(Vec2::new(0.0, 16.0), 5.0, None)).expect("wet");
        let (_, row) = grid.cell_at(Vec2::new(0.0, 16.0)).unwrap();
        assert!(fill.is_wet(47, row as u32), "the lake reaches the ridge");
        assert!(!fill.is_wet(48, row as u32) && !fill.is_wet(49, row as u32), "the ridge holds the hollow dry");
        assert!(grid.height(&config, &ground, 49, row) < fill.level, "the hollow is below the level");
        // The case is real: the ground under the ridge cell's outer half
        // dips below the water, so drawing that half would show water on the
        // hollow's side of the ridge.
        assert!(height_at_world(&config, &ground, ridge_x + 0.4 * grid.step.x, 16.0) < fill.level);

        let mut reach = f32::MIN;
        for (chunk, mesh) in water_body_meshes(&config, &ground, &fill) {
            let corner = chunk.as_vec2() * config.chunk_size;
            for quad in mesh.positions.chunks(4) {
                reach = reach.max(quad[3][0] + corner.x);
            }
        }
        assert!(reach <= ridge_x + 1e-4, "water drawn to x = {reach}, past the ridge's centre at {ridge_x}");
        assert!(reach >= ridge_x - 1e-4, "the ridge's lake side still holds the shoreline; water reached {reach}");
    }
}
