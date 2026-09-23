//! Sparse volumetric edits on top of the terrain heightfield: the caves,
//! overhangs, arches and tunnels that one height per column cannot hold.
//!
//! ## The field
//!
//! Every consumer (the marching-cubes mesher, the trimesh collider, the field
//! raycast) reads one scalar field, solid where it is negative:
//!
//! ```text
//! f  = max(min(fh, A), -C)
//! fh = p.y - H(p.x, p.z)
//! ```
//!
//! `H` is [`height_at_world`], the edited heightfield surface. `A` is the ADD
//! field (union: solid where `A < 0`) and `C` the CARVE field (subtraction:
//! air where `C < 0`), both signed distances stored sparsely in bricks.
//! Where no brick reaches, `A = C = +inf` and `f` is exactly `fh`, so a
//! terrain without edits samples bit for bit as the heightfield it always was.
//!
//! `fh` is deliberately not divided by `sqrt(1 + |grad H|^2)`. That would make
//! it a better distance estimate on slopes, but the divisor is positive, so
//! the solid set does not change, while the crossing marching cubes
//! interpolates on a horizontal lattice edge would then depend on the slope
//! at both ends of the edge and leave the straight edge the neighbouring
//! heightfield chunk draws, opening a crack along every border between a
//! volumetric chunk and a heightfield chunk. Unnormalized, a vertical lattice
//! edge crosses exactly at `H` and a horizontal one exactly on the straight
//! segment between the two lattice heights. Code that steps along `f` as a
//! distance (sphere tracing) bounds the heightfield term by the local slope
//! first (see [`heightfield_slope_factor`]).
//!
//! ## Edit order
//!
//! Two fields represent any sequence of adds and carves exactly. A carve of
//! shape `S` (distance `d`) sets `C = min(C, d)`. An add sets `A = min(A, d)`
//! and also `C = max(C, s - d)`, taking its own volume (grown by one stored
//! step `s`, so the add wins the tie on its own surface) out of earlier
//! carves, so filling a tunnel back in works: `(X - C) + S == (X + S) - (C - S)`
//! with `+` union and `-` subtraction.
//!
//! ## Bricks
//!
//! Distances live on the heightfield's LOD-0 vertex lattice: spacing
//! [`lattice_cell_size`] on all three axes with a lattice point at the world
//! origin, so volumetric and heightfield meshes share their boundary vertices.
//! The lattice is cut into bricks of [`BRICK_EDGE`] points per axis; brick `b`
//! owns lattice points `16 b ..= 16 b + 15` and a sample between two bricks
//! interpolates across both. Distances are `i8` steps of 1/32 cell (about 4
//! cells either way); [`Q_NONE`] means "no edit", the value a distance
//! saturates at once it is too far from any edited surface to matter. A brick
//! whose add and carve cells are all [`Q_NONE`] is dropped, so an empty
//! [`TerrainVolume`] means an untouched terrain.
//!
//! ## Files
//!
//! One file per brick, `Workspace/Terrain/volume/b{x}_{y}_{z}.vbk`,
//! little-endian:
//!
//! | bytes  | contents                                               |
//! |--------|--------------------------------------------------------|
//! | 0..4   | magic `EVBK`                                           |
//! | 4..6   | format version, `u16` = 1                              |
//! | 6..8   | brick edge, `u16` = 16                                 |
//! | 8..12  | lattice cell size in world units, `f32`                |
//! | 12..   | `lz4_flex::compress_prepend_size` of `add`, `carve`, `material`, 4096 bytes each |
//!
//! Cell `(i, j, k)` of a brick is byte `i + 16 j + 256 k` of each array.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use super::{height_at_world, TerrainConfig, TerrainData, TerrainMaterial};

/// Lattice points along each edge of a brick.
pub const BRICK_EDGE: i32 = 16;
/// Lattice points in one brick.
pub const BRICK_CELLS: usize = (BRICK_EDGE * BRICK_EDGE * BRICK_EDGE) as usize;
/// Quantization steps per lattice cell: stored distances resolve 1/32 cell.
pub const Q_STEPS_PER_CELL: f32 = 32.0;
/// Stored distance meaning "no edit here".
pub const Q_NONE: i8 = i8::MAX;
/// Stored material meaning "none".
pub const MATERIAL_NONE: u8 = u8::MAX;
/// Distance in cells at which a stored value saturates at [`Q_NONE`]. Edits
/// only visit lattice points this close to their shape.
pub const EDIT_BAND_CELLS: f32 = Q_NONE as f32 / Q_STEPS_PER_CELL;
/// File magic of a `.vbk` brick.
pub const VBK_MAGIC: [u8; 4] = *b"EVBK";
/// Current `.vbk` format version.
pub const VBK_VERSION: u16 = 1;
/// Folder under `Workspace/Terrain` holding the `.vbk` bricks.
pub const VOLUME_DIR_NAME: &str = "volume";

const VBK_HEADER_LEN: usize = 12;
/// Bytes of one brick's uncompressed `add | carve | material` payload.
pub(crate) const VBK_PAYLOAD_LEN: usize = BRICK_CELLS * 3;
/// Largest lattice region one CSG edit visits (256^3 points). A shape bigger
/// than this is refused rather than stalling the frame for seconds.
const MAX_EDIT_LATTICE_POINTS: i64 = 256 * 256 * 256;
/// Smoothing copies its region before blurring it, so it gets a smaller cap.
const MAX_SMOOTH_LATTICE_POINTS: i64 = 128 * 128 * 128;
/// Lattice coordinates are clamped to +/- 2^28 before integer conversion so
/// neighbour arithmetic on an absurd sample position cannot overflow `i32`.
const LATTICE_LIMIT: f32 = 268_435_456.0;

// ============================================================================
// Lattice and quantization
// ============================================================================

/// World spacing of the volume lattice: the heightfield's LOD-0 vertex
/// spacing, `chunk_size / chunk_resolution`.
///
/// Divides by `resolution_for_lod(0)` rather than `chunk_resolution` because
/// that is what the LOD-0 mesh and collider are built at (it floors tiny
/// resolutions at 4); the two agree for every real config.
pub fn lattice_cell_size(config: &TerrainConfig) -> f32 {
    (config.chunk_size / config.resolution_for_lod(0) as f32).max(1e-4)
}

/// Signed distance `distance` (world units) as a stored step count:
/// `round(distance / cell * 32)`, saturating at +/-127. A NaN distance
/// stores as [`Q_NONE`] so garbage input can never make solid.
pub fn quantize_distance(distance: f32, cell: f32) -> i8 {
    let steps = (distance / cell * Q_STEPS_PER_CELL).round();
    if steps.is_nan() {
        return Q_NONE;
    }
    steps.clamp(-(Q_NONE as f32), Q_NONE as f32) as i8
}

/// World distance of a stored step count. [`Q_NONE`] decodes to its
/// saturated distance (about 4 cells); samplers treat it as "no edit".
#[inline]
pub fn dequantize_distance(q: i8, cell: f32) -> f32 {
    q as f32 * cell / Q_STEPS_PER_CELL
}

/// Index of brick-local cell `(i, j, k)` in a brick's arrays.
#[inline]
pub fn brick_cell_index(i: i32, j: i32, k: i32) -> usize {
    (i + BRICK_EDGE * (j + BRICK_EDGE * k)) as usize
}

/// The brick owning global lattice point `n`, and the point's index in it.
#[inline]
pub fn lattice_to_brick(n: IVec3) -> (IVec3, usize) {
    let brick = IVec3::new(
        n.x.div_euclid(BRICK_EDGE),
        n.y.div_euclid(BRICK_EDGE),
        n.z.div_euclid(BRICK_EDGE),
    );
    let local = n - brick * BRICK_EDGE;
    (brick, brick_cell_index(local.x, local.y, local.z))
}

/// World position of global lattice point `n`.
#[inline]
pub fn lattice_point_world(n: IVec3, cell: f32) -> Vec3 {
    n.as_vec3() * cell
}

/// Global lattice point at or below `p` on every axis (clamped far out).
pub fn lattice_floor(p: Vec3, cell: f32) -> IVec3 {
    let limit = Vec3::splat(LATTICE_LIMIT);
    let g = p / cell;
    if !g.is_finite() {
        return IVec3::ZERO;
    }
    g.floor().clamp(-limit, limit).as_ivec3()
}

/// Brick holding the lattice cell that contains world point `p`.
pub fn brick_coord_at_world(config: &TerrainConfig, p: Vec3) -> IVec3 {
    lattice_to_brick(lattice_floor(p, lattice_cell_size(config))).0
}

/// Global lattice index of brick `coord`'s first point on each axis.
#[inline]
pub fn brick_lattice_origin(coord: IVec3) -> IVec3 {
    coord * BRICK_EDGE
}

/// World AABB of the lattice points brick `coord` owns.
pub fn brick_world_bounds(coord: IVec3, cell: f32) -> (Vec3, Vec3) {
    let lo = lattice_point_world(brick_lattice_origin(coord), cell);
    (lo, lo + Vec3::splat((BRICK_EDGE - 1) as f32 * cell))
}

/// World AABB over which brick `coord` can change the field: its own points
/// plus the cell on each side, where samples interpolate into them.
pub fn brick_influence_bounds(coord: IVec3, cell: f32) -> (Vec3, Vec3) {
    let (lo, hi) = brick_world_bounds(coord, cell);
    (lo - Vec3::splat(cell), hi + Vec3::splat(cell))
}

/// Whether brick `coord` owns any lattice point in `lo ..= hi`.
fn brick_overlaps_lattice(coord: IVec3, lo: IVec3, hi: IVec3) -> bool {
    let b0 = brick_lattice_origin(coord);
    let b1 = b0 + IVec3::splat(BRICK_EDGE - 1);
    b0.cmple(hi).all() && b1.cmpge(lo).all()
}

/// Lattice points covering world box `lo..hi` (outward rounded), or `None`
/// when the box is not finite or holds more than `max_points` points.
fn lattice_range(lo: Vec3, hi: Vec3, cell: f32, max_points: i64) -> Option<(IVec3, IVec3)> {
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let limit = Vec3::splat(LATTICE_LIMIT);
    let n0 = (lo.min(hi) / cell).floor().clamp(-limit, limit).as_ivec3();
    let n1 = (lo.max(hi) / cell).ceil().clamp(-limit, limit).as_ivec3();
    let extent = n1 - n0 + IVec3::ONE;
    let points = extent.x as i64 * extent.y as i64 * extent.z as i64;
    if points > max_points {
        tracing::warn!(
            points,
            max_points,
            "terrain volume edit skipped: its region holds more lattice points than one edit may visit"
        );
        return None;
    }
    Some((n0, n1))
}

/// Every brick coordinate owning a lattice point inside world box
/// `min..max`, whether or not the brick exists yet. An undoable writer
/// snapshots these (see [`TerrainVolume::snapshot_bricks`]) before an edit
/// over the same box, e.g. [`CsgShape::edit_bounds`].
pub fn brick_coords_in_aabb(config: &TerrainConfig, min: Vec3, max: Vec3) -> Vec<IVec3> {
    let cell = lattice_cell_size(config);
    let Some((n0, n1)) = lattice_range(min, max, cell, MAX_EDIT_LATTICE_POINTS) else {
        return Vec::new();
    };
    let b0 = lattice_to_brick(n0).0;
    let b1 = lattice_to_brick(n1).0;
    let mut coords = Vec::new();
    for bz in b0.z..=b1.z {
        for by in b0.y..=b1.y {
            for bx in b0.x..=b1.x {
                coords.push(IVec3::new(bx, by, bz));
            }
        }
    }
    coords
}

// ============================================================================
// Bricks and the volume component
// ============================================================================

/// One lattice point's stored edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeCell {
    /// Quantized ADD distance, [`Q_NONE`] when unedited.
    pub add: i8,
    /// Quantized CARVE distance, [`Q_NONE`] when unedited.
    pub carve: i8,
    /// [`TerrainMaterial`] id of the surface nearest this point, or
    /// [`MATERIAL_NONE`].
    pub material: u8,
}

impl VolumeCell {
    /// An unedited lattice point.
    pub const NONE: Self = Self { add: Q_NONE, carve: Q_NONE, material: MATERIAL_NONE };

    /// Whether either distance carries an edit. Material on a point with no
    /// edit is never read, which is what lets such bricks be dropped.
    #[inline]
    pub fn is_edited(&self) -> bool {
        self.add != Q_NONE || self.carve != Q_NONE
    }
}

/// `BRICK_EDGE^3` lattice points of ADD and CARVE distances plus a material
/// id each. Boxed so a brick costs a pointer in the map and moves cheaply.
#[derive(Clone, PartialEq, Eq)]
pub struct VolumeBrick {
    /// Quantized ADD distances, indexed by [`brick_cell_index`].
    pub add: Box<[i8; BRICK_CELLS]>,
    /// Quantized CARVE distances, indexed by [`brick_cell_index`].
    pub carve: Box<[i8; BRICK_CELLS]>,
    /// Material ids, indexed by [`brick_cell_index`].
    pub material: Box<[u8; BRICK_CELLS]>,
}

impl VolumeBrick {
    /// A brick with no edits.
    pub fn new() -> Self {
        Self {
            add: Box::new([Q_NONE; BRICK_CELLS]),
            carve: Box::new([Q_NONE; BRICK_CELLS]),
            material: Box::new([MATERIAL_NONE; BRICK_CELLS]),
        }
    }

    /// Whether no cell carries an edit, so the brick does not change the
    /// field and need not be stored.
    pub fn is_default(&self) -> bool {
        self.add.iter().all(|&q| q == Q_NONE) && self.carve.iter().all(|&q| q == Q_NONE)
    }

    /// The cell at `index` (see [`brick_cell_index`]).
    #[inline]
    pub fn cell(&self, index: usize) -> VolumeCell {
        VolumeCell { add: self.add[index], carve: self.carve[index], material: self.material[index] }
    }

    /// Number of cells carrying an edit.
    pub fn edited_cells(&self) -> usize {
        (0..BRICK_CELLS).filter(|&i| self.add[i] != Q_NONE || self.carve[i] != Q_NONE).count()
    }
}

impl Default for VolumeBrick {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for VolumeBrick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VolumeBrick").field("edited_cells", &self.edited_cells()).finish()
    }
}

/// Volumetric edits of one terrain, on its `TerrainRoot` next to
/// `TerrainData`. Every root is spawned with one; a root without it reads as
/// an empty volume. Holds only bricks with at least one edited cell.
#[derive(Component, Clone, Debug, Default)]
pub struct TerrainVolume {
    bricks: HashMap<IVec3, VolumeBrick>,
    // Brick Y coordinates stored per brick XZ column, so chunk-column
    // queries visit only the columns under the chunk.
    columns: HashMap<IVec2, BTreeSet<i32>>,
    /// Brick files on disk that the last load could not use (unreadable,
    /// undecodable, or saved at another lattice spacing). Save must not
    /// delete them as stale: they are data this session never saw.
    unloaded: HashSet<IVec3>,
    /// The load could not list the volume folder at all, so no file there
    /// is known to be stale.
    listing_failed: bool,
}

impl TerrainVolume {
    /// A volume with no edits.
    pub fn new() -> Self {
        Self::default()
    }

    /// A shared empty volume, for reading a root spawned without one:
    /// `volume.unwrap_or(TerrainVolume::empty())`.
    pub fn empty() -> &'static TerrainVolume {
        static EMPTY: std::sync::OnceLock<TerrainVolume> = std::sync::OnceLock::new();
        EMPTY.get_or_init(TerrainVolume::default)
    }

    /// No brick is stored, so the field is exactly the heightfield.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bricks.is_empty()
    }

    /// Number of stored bricks.
    pub fn brick_count(&self) -> usize {
        self.bricks.len()
    }

    /// The brick at `coord`, if it has edits.
    pub fn brick(&self, coord: IVec3) -> Option<&VolumeBrick> {
        self.bricks.get(&coord)
    }

    /// Every stored brick, in no particular order.
    pub fn bricks(&self) -> impl Iterator<Item = (IVec3, &VolumeBrick)> + '_ {
        self.bricks.iter().map(|(coord, brick)| (*coord, brick))
    }

    /// Store `brick` at `coord`, or remove it for `None`. A brick without
    /// edits is removed rather than stored.
    pub fn set_brick(&mut self, coord: IVec3, brick: Option<VolumeBrick>) {
        match brick {
            Some(brick) if !brick.is_default() => {
                self.insert_brick(coord, brick);
            }
            _ => {
                self.remove_brick(coord);
            }
        }
    }

    /// Remove every brick, and forget the brick files the last load could
    /// not use, so the next save deletes those too.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Brick files the last load skipped (see [`load_volume_bricks`]).
    pub fn unloaded_bricks(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.unloaded.iter().copied()
    }

    /// Whether save must keep the file of brick `coord` although the volume
    /// holds no brick there: the load skipped it, or could not list the
    /// folder at all.
    pub fn keeps_unloaded_file(&self, coord: IVec3) -> bool {
        self.listing_failed || self.unloaded.contains(&coord)
    }

    /// World box every stored brick can change the field in (see
    /// [`brick_influence_bounds`]), or `None` for an empty volume.
    pub fn influence_bounds(&self, cell: f32) -> Option<(Vec3, Vec3)> {
        self.columns
            .iter()
            .filter_map(|(column, ys)| {
                let (lowest, highest) = (*ys.first()?, *ys.last()?);
                let lo = brick_influence_bounds(IVec3::new(column.x, lowest, column.y), cell).0;
                let hi = brick_influence_bounds(IVec3::new(column.x, highest, column.y), cell).1;
                Some((lo, hi))
            })
            .reduce(|(a, b), (lo, hi)| (a.min(lo), b.max(hi)))
    }

    // The only two writers of `bricks`, so `columns` never disagrees with it.
    fn insert_brick(&mut self, coord: IVec3, brick: VolumeBrick) {
        self.bricks.insert(coord, brick);
        self.columns.entry(coord.xz()).or_default().insert(coord.y);
    }

    fn remove_brick(&mut self, coord: IVec3) -> Option<VolumeBrick> {
        let removed = self.bricks.remove(&coord);
        if removed.is_some() {
            if let Some(ys) = self.columns.get_mut(&coord.xz()) {
                ys.remove(&coord.y);
                if ys.is_empty() {
                    self.columns.remove(&coord.xz());
                }
            }
        }
        removed
    }

    /// Copies of the bricks at `coords` (`None` where there is none), for
    /// [`Self::restore_bricks`] to put back.
    pub fn snapshot_bricks(&self, coords: &[IVec3]) -> Vec<(IVec3, Option<VolumeBrick>)> {
        coords.iter().map(|coord| (*coord, self.bricks.get(coord).cloned())).collect()
    }

    /// Put back bricks from [`Self::snapshot_bricks`]. Returns the bricks
    /// that actually changed and the world box their field covers.
    pub fn restore_bricks(
        &mut self,
        config: &TerrainConfig,
        snapshot: &[(IVec3, Option<VolumeBrick>)],
    ) -> VolumeEdit {
        let mut tracker = EditTracker::new();
        for (coord, brick) in snapshot {
            let wanted = brick.as_ref().filter(|brick| !brick.is_default());
            let unchanged = self.bricks.get(coord) == wanted;
            if unchanged {
                continue;
            }
            match wanted {
                Some(brick) => {
                    self.insert_brick(*coord, brick.clone());
                }
                None => {
                    self.remove_brick(*coord);
                }
            }
            let origin = brick_lattice_origin(*coord);
            tracker.include(origin);
            tracker.include(origin + IVec3::splat(BRICK_EDGE - 1));
            tracker.bricks.push(*coord);
        }
        tracker.finish(lattice_cell_size(config))
    }

    /// Remove bricks left without edits. Returns their coordinates.
    pub fn prune(&mut self) -> Vec<IVec3> {
        let empty: Vec<IVec3> = self
            .bricks
            .iter()
            .filter(|(_, brick)| brick.is_default())
            .map(|(coord, _)| *coord)
            .collect();
        for coord in &empty {
            self.remove_brick(*coord);
        }
        empty
    }

    /// The stored edit at global lattice point `n`.
    pub fn cell_at_lattice(&self, n: IVec3) -> VolumeCell {
        let (coord, index) = lattice_to_brick(n);
        self.bricks.get(&coord).map_or(VolumeCell::NONE, |brick| brick.cell(index))
    }

    /// Trilinear ADD and CARVE distances (world units) at `p`. Each is
    /// `+inf` when none of the lattice points that carry weight at `p` has
    /// an edit in that field, so far from edits the field is exactly `fh`.
    /// Near an edit, unedited corners count as their saturated distance
    /// (about 4 cells), which keeps the surface where the edits put it.
    pub fn edit_distances(&self, cell: f32, p: Vec3) -> (f32, f32) {
        if self.bricks.is_empty() {
            return (f32::INFINITY, f32::INFINITY);
        }
        let (mut add, mut carve) = (0.0f32, 0.0f32);
        let (mut add_edited, mut carve_edited) = (false, false);
        self.for_each_corner(cell, p, |weight, value| {
            add += weight * value.add as f32;
            carve += weight * value.carve as f32;
            add_edited |= value.add != Q_NONE;
            carve_edited |= value.carve != Q_NONE;
        });
        let scale = cell / Q_STEPS_PER_CELL;
        (
            if add_edited { add * scale } else { f32::INFINITY },
            if carve_edited { carve * scale } else { f32::INFINITY },
        )
    }

    /// Visit the lattice points around `p` that carry trilinear weight, with
    /// their weight and stored cell. Corners in one brick share a lookup.
    fn for_each_corner(&self, cell: f32, p: Vec3, mut visit: impl FnMut(f32, VolumeCell)) {
        let g = p / cell;
        if !g.is_finite() {
            return;
        }
        let limit = Vec3::splat(LATTICE_LIMIT);
        let floor = g.floor().clamp(-limit, limit);
        let t = (g - floor).clamp(Vec3::ZERO, Vec3::ONE);
        let base = floor.as_ivec3();
        let mut cached: Option<(IVec3, Option<&VolumeBrick>)> = None;
        for corner in 0..8i32 {
            let offset = IVec3::new(corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let wx = if offset.x == 1 { t.x } else { 1.0 - t.x };
            let wy = if offset.y == 1 { t.y } else { 1.0 - t.y };
            let wz = if offset.z == 1 { t.z } else { 1.0 - t.z };
            let weight = wx * wy * wz;
            if weight <= 0.0 {
                continue;
            }
            let (coord, index) = lattice_to_brick(base + offset);
            let brick = match cached {
                Some((cached_coord, brick)) if cached_coord == coord => brick,
                _ => {
                    let brick = self.bricks.get(&coord);
                    cached = Some((coord, brick));
                    brick
                }
            };
            visit(weight, brick.map_or(VolumeCell::NONE, |brick| brick.cell(index)));
        }
    }

    /// Stored bricks that can change the field inside world box `min..max`.
    pub fn bricks_in_aabb<'a>(
        &'a self,
        config: &TerrainConfig,
        min: Vec3,
        max: Vec3,
    ) -> impl Iterator<Item = (IVec3, &'a VolumeBrick)> + 'a {
        let cell = lattice_cell_size(config);
        let (lo, hi) = (min.min(max), min.max(max));
        self.bricks
            .iter()
            .filter(move |(coord, _)| {
                let (brick_lo, brick_hi) = brick_influence_bounds(**coord, cell);
                brick_lo.cmple(hi).all() && brick_hi.cmpge(lo).all()
            })
            .map(|(coord, brick)| (*coord, brick))
    }

    /// Stored bricks owning a lattice point of `chunk_pos`'s LOD-0 vertex
    /// columns, border columns included (they are shared with the
    /// neighbouring chunk, which then counts the brick too).
    pub fn bricks_in_chunk_column<'a>(
        &'a self,
        config: &TerrainConfig,
        chunk_pos: IVec2,
    ) -> impl Iterator<Item = (IVec3, &'a VolumeBrick)> + 'a {
        self.chunk_brick_columns(config, chunk_pos).flat_map(move |(column, ys)| {
            ys.iter().map(move |&y| {
                let coord = IVec3::new(column.x, y, column.y);
                (coord, &self.bricks[&coord])
            })
        })
    }

    /// The brick XZ columns holding a brick that owns a lattice point of
    /// `chunk_pos`'s LOD-0 vertex columns, with their brick Y coordinates.
    /// The chunk's inclusive lattice span `x0 ..= x1` meets exactly the
    /// bricks `x0 / 16 ..= x1 / 16` (flooring), and the same on Z.
    fn chunk_brick_columns<'a>(
        &'a self,
        config: &TerrainConfig,
        chunk_pos: IVec2,
    ) -> impl Iterator<Item = (IVec2, &'a BTreeSet<i32>)> + 'a {
        let resolution = config.resolution_for_lod(0).max(1) as i32;
        let (x0, x1) = (chunk_pos.x * resolution, (chunk_pos.x + 1) * resolution);
        let (z0, z1) = (chunk_pos.y * resolution, (chunk_pos.y + 1) * resolution);
        let (bx0, bx1) = (x0.div_euclid(BRICK_EDGE), x1.div_euclid(BRICK_EDGE));
        let (bz0, bz1) = (z0.div_euclid(BRICK_EDGE), z1.div_euclid(BRICK_EDGE));
        (bz0..=bz1)
            .flat_map(move |bz| (bx0..=bx1).map(move |bx| IVec2::new(bx, bz)))
            .filter_map(move |column| self.columns.get(&column).map(|ys| (column, ys)))
    }

    /// Write `changes` (global lattice point, new cell), creating bricks as
    /// needed and dropping any the changes leave without edits.
    fn write_cells(&mut self, changes: &[(IVec3, VolumeCell)], cell: f32) -> VolumeEdit {
        let mut tracker = EditTracker::new();
        let mut touched = HashSet::new();
        for &(n, value) in changes {
            let (coord, index) = lattice_to_brick(n);
            if !self.bricks.contains_key(&coord) {
                self.insert_brick(coord, VolumeBrick::new());
            }
            let Some(brick) = self.bricks.get_mut(&coord) else {
                continue;
            };
            brick.add[index] = value.add;
            brick.carve[index] = value.carve;
            brick.material[index] = value.material;
            tracker.include(n);
            touched.insert(coord);
        }
        for coord in touched {
            if self.bricks.get(&coord).is_some_and(VolumeBrick::is_default) {
                self.remove_brick(coord);
            }
            tracker.bricks.push(coord);
        }
        tracker.finish(cell)
    }
}

/// Whether any brick owns a lattice point of chunk `chunk_pos`'s LOD-0
/// vertex columns, i.e. the chunk's surface can differ from its heightfield.
pub fn chunk_has_volume(chunk_pos: IVec2, config: &TerrainConfig, volume: &TerrainVolume) -> bool {
    volume.bricks_in_chunk_column(config, chunk_pos).next().is_some()
}

/// World Y extent `(min, max)` of the lattice points owned by the bricks in
/// chunk `chunk_pos`'s column, or `None` when the chunk has no volume.
pub fn chunk_volume_y_range(chunk_pos: IVec2, config: &TerrainConfig, volume: &TerrainVolume) -> Option<(f32, f32)> {
    let cell = lattice_cell_size(config);
    volume
        .chunk_brick_columns(config, chunk_pos)
        .filter_map(|(column, ys)| {
            let (lowest, highest) = (*ys.first()?, *ys.last()?);
            Some((
                brick_world_bounds(IVec3::new(column.x, lowest, column.y), cell).0.y,
                brick_world_bounds(IVec3::new(column.x, highest, column.y), cell).1.y,
            ))
        })
        .reduce(|(lo_a, hi_a), (lo_b, hi_b)| (lo_a.min(lo_b), hi_a.max(hi_b)))
}

// ============================================================================
// The field
// ============================================================================

/// Which term of `max(min(fh, A), -C)` produced a field value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldTerm {
    /// The heightfield surface.
    Heightfield,
    /// An ADD edit.
    Add,
    /// A CARVE edit.
    Carve,
}

/// The field at one point together with the terms it was composed from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldSample {
    /// `max(min(heightfield, add), -carve)`: solid where negative.
    pub value: f32,
    /// `p.y - H`.
    pub heightfield: f32,
    /// ADD distance, `+inf` without an edit.
    pub add: f32,
    /// CARVE distance, `+inf` without an edit.
    pub carve: f32,
}

impl FieldSample {
    /// Compose the three terms.
    #[inline]
    pub fn compose(heightfield: f32, add: f32, carve: f32) -> Self {
        Self { value: heightfield.min(add).max(-carve), heightfield, add, carve }
    }

    /// The term that set [`Self::value`]. Ties go to the carve, then the
    /// heightfield, so an unedited point always reports the heightfield.
    pub fn term(&self) -> FieldTerm {
        if self.carve.is_finite() && -self.carve >= self.heightfield.min(self.add) {
            FieldTerm::Carve
        } else if self.add < self.heightfield {
            FieldTerm::Add
        } else {
            FieldTerm::Heightfield
        }
    }

    /// Whether the point is inside the terrain.
    #[inline]
    pub fn is_solid(&self) -> bool {
        self.value < 0.0
    }
}

/// Heightfield term `fh = p.y - H(p.x, p.z)`, unnormalized (see the module
/// docs for why).
#[inline]
pub fn heightfield_term(config: &TerrainConfig, data: &TerrainData, p: Vec3) -> f32 {
    p.y - height_at_world(config, data, p.x, p.z)
}

/// `sqrt(1 + |grad H|^2)` at `x, z` from central differences one lattice
/// cell apart: how much `fh` overstates the distance to the heightfield
/// surface there. Sphere tracing divides `f` by it for a safe step.
pub fn heightfield_slope_factor(config: &TerrainConfig, data: &TerrainData, x: f32, z: f32) -> f32 {
    let c = lattice_cell_size(config);
    let gx = (height_at_world(config, data, x + c, z) - height_at_world(config, data, x - c, z)) / (2.0 * c);
    let gz = (height_at_world(config, data, x, z + c) - height_at_world(config, data, x, z - c)) / (2.0 * c);
    (1.0 + gx * gx + gz * gz).sqrt()
}

/// The terrain field at world point `p` with its terms.
pub fn sample_field_parts(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, p: Vec3) -> FieldSample {
    let heightfield = heightfield_term(config, data, p);
    if volume.is_empty() {
        return FieldSample::compose(heightfield, f32::INFINITY, f32::INFINITY);
    }
    let (add, carve) = volume.edit_distances(lattice_cell_size(config), p);
    FieldSample::compose(heightfield, add, carve)
}

/// The terrain field at world point `p`: solid where negative.
#[inline]
pub fn sample_field(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, p: Vec3) -> f32 {
    sample_field_parts(config, data, volume, p).value
}

/// World height of the heightfield at global lattice column `nx, nz`,
/// computed with exactly the arithmetic `chunk_height_grid` (mesh.rs) uses
/// for that vertex, so a lattice sample equals the heightfield mesh's vertex
/// bit for bit from either chunk sharing a border column.
///
/// `height_at_world` at the same point can differ in the last bit (it
/// divides a world coordinate back down). A cache-less procedural terrain
/// has no raster to share and reads its band floor here, like
/// `height_at_world`; its chunk meshes come from noise instead.
pub fn lattice_surface_height(config: &TerrainConfig, data: &TerrainData, nx: i32, nz: i32) -> f32 {
    let resolution = config.resolution_for_lod(0).max(1);
    let r = resolution as i32;
    let (chunk_x, i) = (nx.div_euclid(r), nx.rem_euclid(r));
    let (chunk_z, k) = (nz.div_euclid(r), nz.rem_euclid(r));
    let total_x = (config.chunks_x * 2 + 1) as f32;
    let total_z = (config.chunks_z * 2 + 1) as f32;
    let u = i as f32 / resolution as f32;
    let v = k as f32 / resolution as f32;
    let world_u = ((chunk_x as f32 + u + config.chunks_x as f32) / total_x).clamp(0.0, 1.0);
    let world_v = ((chunk_z as f32 + v + config.chunks_z as f32) / total_z).clamp(0.0, 1.0);
    config.world_height(data.sample_height(world_u, world_v))
}

/// The field at global lattice point `n`, reading the stored cell directly
/// and the heightfield through [`lattice_surface_height`]. Meshers evaluate
/// lattice points through this so a volumetric chunk's border vertices match
/// the neighbouring heightfield chunk exactly.
pub fn sample_field_lattice(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, n: IVec3) -> FieldSample {
    let cell = lattice_cell_size(config);
    let heightfield = n.y as f32 * cell - lattice_surface_height(config, data, n.x, n.z);
    lattice_field_sample(heightfield, volume.cell_at_lattice(n), cell)
}

/// The field at a lattice point from its heightfield term (`n.y * cell`
/// minus [`lattice_surface_height`]) and its stored cell. The mesher samples
/// whole chunks through this with each column's height and brick looked up
/// once, which keeps its values bit for bit those of [`sample_field_lattice`].
#[inline]
pub fn lattice_field_sample(heightfield: f32, stored: VolumeCell, cell: f32) -> FieldSample {
    let add = if stored.add == Q_NONE { f32::INFINITY } else { dequantize_distance(stored.add, cell) };
    let carve = if stored.carve == Q_NONE { f32::INFINITY } else { dequantize_distance(stored.carve, cell) };
    FieldSample::compose(heightfield, add, carve)
}

/// Gradient of the field at `p` by central differences one lattice cell
/// apart, the same span the heightfield mesh takes its normals over, so
/// normals agree across the border between a volumetric chunk and a
/// heightfield chunk. Points from solid toward air.
pub fn field_gradient(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, p: Vec3) -> Vec3 {
    let h = lattice_cell_size(config);
    let sample = |q: Vec3| sample_field(config, data, volume, q);
    Vec3::new(
        sample(p + Vec3::X * h) - sample(p - Vec3::X * h),
        sample(p + Vec3::Y * h) - sample(p - Vec3::Y * h),
        sample(p + Vec3::Z * h) - sample(p - Vec3::Z * h),
    ) / (2.0 * h)
}

/// Unit outward surface normal at `p` (the normalized [`field_gradient`]),
/// straight up where the gradient vanishes.
pub fn field_normal(config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume, p: Vec3) -> Vec3 {
    let normal = field_gradient(config, data, volume, p).normalize_or_zero();
    if normal == Vec3::ZERO {
        Vec3::Y
    } else {
        normal
    }
}

/// Material an edit wrote nearest `p`: among the lattice points carrying
/// trilinear weight at `p` that hold an edit and a material, the one with
/// the largest weight. `None` away from edits.
pub fn material_at(config: &TerrainConfig, volume: &TerrainVolume, p: Vec3) -> Option<TerrainMaterial> {
    if volume.is_empty() {
        return None;
    }
    let mut best: Option<(f32, u8)> = None;
    volume.for_each_corner(lattice_cell_size(config), p, |weight, value| {
        if value.is_edited() && value.material != MATERIAL_NONE && best.map_or(true, |(w, _)| weight > w) {
            best = Some((weight, value.material));
        }
    });
    best.and_then(|(_, id)| TerrainMaterial::from_u8(id))
}

// ============================================================================
// CSG edits
// ============================================================================

/// How a shape combines with the terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsgOp {
    /// Union: the shape becomes solid.
    Add,
    /// Subtraction: the shape becomes air.
    Carve,
}

/// A shape a CSG edit applies, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CsgShape {
    /// Ball of `radius` around `center`.
    Sphere { center: Vec3, radius: f32 },
    /// World-axis-aligned box.
    AxisBox { center: Vec3, half_extents: Vec3 },
    /// Upright cylinder around the vertical line through `center`, reaching
    /// `half_height` above and below it.
    Cylinder { center: Vec3, radius: f32, half_height: f32 },
}

impl CsgShape {
    /// Exact signed distance from `p` to the shape's surface, negative inside.
    pub fn distance(&self, p: Vec3) -> f32 {
        match *self {
            Self::Sphere { center, radius } => (p - center).length() - radius,
            Self::AxisBox { center, half_extents } => {
                let q = (p - center).abs() - half_extents;
                q.max(Vec3::ZERO).length() + q.max_element().min(0.0)
            }
            Self::Cylinder { center, radius, half_height } => {
                let d = p - center;
                let radial = Vec2::new(d.x, d.z).length() - radius;
                let vertical = d.y.abs() - half_height;
                Vec2::new(radial.max(0.0), vertical.max(0.0)).length() + radial.max(vertical).min(0.0)
            }
        }
    }

    /// World AABB of the shape itself.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        match *self {
            Self::Sphere { center, radius } => (center - Vec3::splat(radius), center + Vec3::splat(radius)),
            Self::AxisBox { center, half_extents } => (center - half_extents, center + half_extents),
            Self::Cylinder { center, radius, half_height } => {
                let reach = Vec3::new(radius, half_height, radius);
                (center - reach, center + reach)
            }
        }
    }

    /// World AABB an edit with this shape visits: [`Self::bounds`] grown by
    /// the band within which a distance still quantizes below [`Q_NONE`].
    pub fn edit_bounds(&self, config: &TerrainConfig) -> (Vec3, Vec3) {
        let band = Vec3::splat(EDIT_BAND_CELLS * lattice_cell_size(config));
        let (lo, hi) = self.bounds();
        (lo - band, hi + band)
    }

    /// Finite position and strictly positive size.
    fn is_valid(&self) -> bool {
        match *self {
            Self::Sphere { center, radius } => center.is_finite() && radius.is_finite() && radius > 0.0,
            Self::AxisBox { center, half_extents } => {
                center.is_finite() && half_extents.is_finite() && half_extents.cmpgt(Vec3::ZERO).all()
            }
            Self::Cylinder { center, radius, half_height } => {
                center.is_finite() && radius.is_finite() && half_height.is_finite() && radius > 0.0 && half_height > 0.0
            }
        }
    }
}

/// What a volume edit changed: the world box whose field may differ now,
/// and every brick it created, changed or removed. Hosts mark the box dirty
/// (remesh and recollide) and snapshot the bricks for undo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VolumeEdit {
    /// Lower corner of the changed box (meaningless when [`Self::is_empty`]).
    pub min: Vec3,
    /// Upper corner of the changed box (meaningless when [`Self::is_empty`]).
    pub max: Vec3,
    /// Bricks created, changed or removed, sorted by `(x, y, z)`.
    pub bricks: Vec<IVec3>,
}

impl VolumeEdit {
    /// The edit changed nothing.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bricks.is_empty()
    }

    /// Fold `other` into this edit (a stroke of several dabs).
    pub fn merge(&mut self, other: &VolumeEdit) {
        if other.is_empty() {
            return;
        }
        if self.is_empty() {
            *self = other.clone();
            return;
        }
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        self.bricks.extend(&other.bricks);
        sort_brick_coords(&mut self.bricks);
    }
}

fn sort_brick_coords(coords: &mut Vec<IVec3>) {
    coords.sort_unstable_by_key(|c| (c.x, c.y, c.z));
    coords.dedup();
}

/// Bounds of the lattice points an edit changed, plus the bricks it touched.
struct EditTracker {
    lo: IVec3,
    hi: IVec3,
    bricks: Vec<IVec3>,
}

impl EditTracker {
    fn new() -> Self {
        Self { lo: IVec3::MAX, hi: IVec3::MIN, bricks: Vec::new() }
    }

    #[inline]
    fn include(&mut self, n: IVec3) {
        self.lo = self.lo.min(n);
        self.hi = self.hi.max(n);
    }

    /// The changed box grows one cell past the changed points: a sample
    /// anywhere in the cells around a point interpolates it.
    fn finish(mut self, cell: f32) -> VolumeEdit {
        if self.bricks.is_empty() {
            return VolumeEdit::default();
        }
        sort_brick_coords(&mut self.bricks);
        VolumeEdit {
            min: lattice_point_world(self.lo - IVec3::ONE, cell),
            max: lattice_point_world(self.hi + IVec3::ONE, cell),
            bricks: self.bricks,
        }
    }
}

/// Apply `shape` with `op`. Add writes `material` on the points where it
/// becomes the nearest surface; carve writes it on the walls it exposes.
/// Either paints inside its shape, and up to one cell outside it where the
/// other field's surface is not nearer. `None` means [`TerrainMaterial::Rock`].
pub fn apply_shape(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    shape: CsgShape,
    op: CsgOp,
    material: Option<TerrainMaterial>,
) -> VolumeEdit {
    if !shape.is_valid() {
        return VolumeEdit::default();
    }
    let cell = lattice_cell_size(config);
    let (lo, hi) = shape.edit_bounds(config);
    let Some((n0, n1)) = lattice_range(lo, hi, cell, MAX_EDIT_LATTICE_POINTS) else {
        return VolumeEdit::default();
    };
    let material = material.unwrap_or(TerrainMaterial::Rock).to_u8();
    let b0 = lattice_to_brick(n0).0;
    let b1 = lattice_to_brick(n1).0;
    let mut tracker = EditTracker::new();
    // A surface vertex takes its material from the two lattice points of the
    // edge it sits on, each within one cell of the surface. Painting farther
    // out only recolours the other field's surfaces, which share this byte.
    let paint_band = Q_STEPS_PER_CELL as i8;

    for bz in b0.z..=b1.z {
        for by in b0.y..=b1.y {
            for bx in b0.x..=b1.x {
                let coord = IVec3::new(bx, by, bz);
                let origin = brick_lattice_origin(coord);
                let local_lo = (n0 - origin).max(IVec3::ZERO);
                let local_hi = (n1 - origin).min(IVec3::splat(BRICK_EDGE - 1));
                let existing = volume.remove_brick(coord);
                let existed = existing.is_some();
                let mut brick = existing.unwrap_or_default();
                let mut changed = false;

                for k in local_lo.z..=local_hi.z {
                    for j in local_lo.y..=local_hi.y {
                        for i in local_lo.x..=local_hi.x {
                            let n = origin + IVec3::new(i, j, k);
                            let q = quantize_distance(shape.distance(lattice_point_world(n, cell)), cell);
                            // Saturated: the shape is too far away to lower
                            // A or C, or to matter to a carve it would lift.
                            if q == Q_NONE {
                                continue;
                            }
                            let index = brick_cell_index(i, j, k);
                            let mut hit = false;
                            match op {
                                CsgOp::Add => {
                                    if q < brick.add[index] {
                                        brick.add[index] = q;
                                        // Outside the shape, a carve wall nearer
                                        // than this surface keeps its colour.
                                        if q <= paint_band
                                            && (q <= 0 || (brick.carve[index] as i16).abs() >= q as i16)
                                        {
                                            brick.material[index] = material;
                                        }
                                        hit = true;
                                    }
                                    // Take the shape out of earlier carves,
                                    // one step past its own surface: at
                                    // exactly `-q` an add over an identical
                                    // carve would leave `f = 0` (air) on
                                    // every lattice point of that surface.
                                    let lift = (1 - q as i16).min(Q_NONE as i16) as i8;
                                    if lift > brick.carve[index] {
                                        brick.carve[index] = lift;
                                        hit = true;
                                    }
                                }
                                CsgOp::Carve => {
                                    if q < brick.carve[index] {
                                        brick.carve[index] = q;
                                        // On the solid side of the wall, an add
                                        // surface nearer than this wall keeps
                                        // its colour.
                                        if q <= paint_band
                                            && (q <= 0 || (brick.add[index] as i16).abs() >= q as i16)
                                        {
                                            brick.material[index] = material;
                                        }
                                        hit = true;
                                    }
                                }
                            }
                            if hit {
                                changed = true;
                                tracker.include(n);
                            }
                        }
                    }
                }

                if changed {
                    tracker.bricks.push(coord);
                    if !brick.is_default() {
                        volume.insert_brick(coord, brick);
                    }
                } else if existed {
                    volume.insert_brick(coord, brick);
                }
            }
        }
    }
    tracker.finish(cell)
}

/// Add or carve a sphere. See [`apply_shape`].
pub fn apply_sphere(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    center: Vec3,
    radius: f32,
    op: CsgOp,
    material: Option<TerrainMaterial>,
) -> VolumeEdit {
    apply_shape(config, volume, CsgShape::Sphere { center, radius }, op, material)
}

/// Add or carve a world-axis-aligned box. See [`apply_shape`].
pub fn apply_box(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    center: Vec3,
    half_extents: Vec3,
    op: CsgOp,
    material: Option<TerrainMaterial>,
) -> VolumeEdit {
    apply_shape(config, volume, CsgShape::AxisBox { center, half_extents }, op, material)
}

/// Add or carve an upright cylinder. See [`apply_shape`].
pub fn apply_cylinder(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    center: Vec3,
    radius: f32,
    half_height: f32,
    op: CsgOp,
    material: Option<TerrainMaterial>,
) -> VolumeEdit {
    apply_shape(config, volume, CsgShape::Cylinder { center, radius, half_height }, op, material)
}

/// Blur the ADD and CARVE fields inside the ball of `radius` around
/// `center`, rounding the edges of earlier edits. Each lattice point moves
/// toward the mean of its 27-point neighbourhood by `strength` (clamped to
/// `0..=1`), fading to nothing at the rim. Only points near an edit change;
/// the heightfield is left to the heightfield Smooth brush.
pub fn apply_smooth(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    center: Vec3,
    radius: f32,
    strength: f32,
) -> VolumeEdit {
    if volume.is_empty()
        || !center.is_finite()
        || !(radius.is_finite() && radius > 0.0)
        || !(strength.is_finite() && strength > 0.0)
    {
        return VolumeEdit::default();
    }
    let strength = strength.min(1.0);
    let cell = lattice_cell_size(config);
    let reach = Vec3::splat(radius);
    let Some((n0, n1)) = lattice_range(center - reach, center + reach, cell, MAX_SMOOTH_LATTICE_POINTS) else {
        return VolumeEdit::default();
    };
    // Every output reads the pre-smooth values of its neighbourhood, so the
    // region plus a one-point margin is copied out before anything changes.
    let r0 = n0 - IVec3::ONE;
    let r1 = n1 + IVec3::ONE;
    if !volume.bricks.keys().any(|coord| brick_overlaps_lattice(*coord, r0, r1)) {
        return VolumeEdit::default();
    }
    let dims = r1 - r0 + IVec3::ONE;
    let index_of = |n: IVec3| {
        let l = n - r0;
        (l.x + dims.x * (l.y + dims.y * l.z)) as usize
    };
    let mut source = Vec::with_capacity((dims.x * dims.y * dims.z) as usize);
    for z in r0.z..=r1.z {
        for y in r0.y..=r1.y {
            for x in r0.x..=r1.x {
                source.push(volume.cell_at_lattice(IVec3::new(x, y, z)));
            }
        }
    }

    let radius_sq = radius * radius;
    let mut changes = Vec::new();
    for z in n0.z..=n1.z {
        for y in n0.y..=n1.y {
            for x in n0.x..=n1.x {
                let n = IVec3::new(x, y, z);
                let dist_sq = lattice_point_world(n, cell).distance_squared(center);
                if dist_sq > radius_sq {
                    continue;
                }
                let weight = strength * (1.0 - dist_sq / radius_sq);
                let old = source[index_of(n)];
                let (mut sum_add, mut sum_carve) = (0.0f32, 0.0f32);
                let mut any_edit = false;
                // A point the blur newly reaches takes the material of its
                // most deeply edited neighbour.
                let mut donor: Option<(i8, u8)> = None;
                for dz in -1..=1 {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let near = source[index_of(n + IVec3::new(dx, dy, dz))];
                            sum_add += near.add as f32;
                            sum_carve += near.carve as f32;
                            if near.is_edited() {
                                any_edit = true;
                                let depth = near.add.min(near.carve);
                                if near.material != MATERIAL_NONE && donor.map_or(true, |(d, _)| depth < d) {
                                    donor = Some((depth, near.material));
                                }
                            }
                        }
                    }
                }
                if !any_edit {
                    continue;
                }
                let add = blend_quantized(old.add, sum_add / 27.0, weight);
                let carve = blend_quantized(old.carve, sum_carve / 27.0, weight);
                if add == old.add && carve == old.carve {
                    continue;
                }
                let material = if old.material == MATERIAL_NONE {
                    donor.map_or(MATERIAL_NONE, |(_, id)| id)
                } else {
                    old.material
                };
                changes.push((n, VolumeCell { add, carve, material }));
            }
        }
    }
    volume.write_cells(&changes, cell)
}

/// `old` moved toward `target` (both in quantized steps) by `weight`.
fn blend_quantized(old: i8, target: f32, weight: f32) -> i8 {
    let value = old as f32 + (target - old as f32) * weight;
    value.round().clamp(-(Q_NONE as f32), Q_NONE as f32) as i8
}

// ============================================================================
// .vbk persistence
// ============================================================================

/// `Workspace/Terrain/volume` for the Space's `Workspace/Terrain` folder.
pub fn volume_dir(terrain_dir: &Path) -> PathBuf {
    terrain_dir.join(VOLUME_DIR_NAME)
}

/// File name of brick `coord`: `b{x}_{y}_{z}.vbk`.
pub fn brick_file_name(coord: IVec3) -> String {
    format!("b{}_{}_{}.vbk", coord.x, coord.y, coord.z)
}

/// Path of brick `coord` under the Space's `Workspace/Terrain` folder.
pub fn brick_file_path(terrain_dir: &Path, coord: IVec3) -> PathBuf {
    volume_dir(terrain_dir).join(brick_file_name(coord))
}

/// Brick coordinate named by a `b{x}_{y}_{z}.vbk` file name.
pub fn parse_brick_file_name(name: &str) -> Option<IVec3> {
    let stem = name.strip_prefix('b')?.strip_suffix(".vbk")?;
    let mut parts = stem.split('_');
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let z = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(IVec3::new(x, y, z))
}

/// The lz4 `add | carve | material` payload of one brick, shared by `.vbk`
/// files and the undo history.
pub(crate) fn pack_brick_payload(brick: &VolumeBrick) -> Vec<u8> {
    let mut raw = Vec::with_capacity(VBK_PAYLOAD_LEN);
    raw.extend(brick.add.iter().map(|&q| q as u8));
    raw.extend(brick.carve.iter().map(|&q| q as u8));
    raw.extend_from_slice(&brick.material[..]);
    lz4_flex::compress_prepend_size(&raw)
}

/// The brick a [`pack_brick_payload`] payload holds.
pub(crate) fn unpack_brick_payload(bytes: &[u8]) -> Result<VolumeBrick, String> {
    // Checked before decompressing: a corrupt size prefix must not drive a
    // multi-gigabyte allocation.
    let declared = bytes.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if declared != Some(VBK_PAYLOAD_LEN) {
        return Err(format!("brick payload declares {declared:?} bytes, expected {VBK_PAYLOAD_LEN}"));
    }
    let raw = lz4_flex::decompress_size_prepended(bytes)
        .map_err(|error| format!("brick payload failed to decompress: {error}"))?;
    if raw.len() != VBK_PAYLOAD_LEN {
        return Err(format!("brick payload is {} bytes, expected {VBK_PAYLOAD_LEN}", raw.len()));
    }
    let mut brick = VolumeBrick::new();
    for i in 0..BRICK_CELLS {
        // -128 is outside the stored range; keep distances symmetric.
        brick.add[i] = (raw[i] as i8).max(-Q_NONE);
        brick.carve[i] = (raw[BRICK_CELLS + i] as i8).max(-Q_NONE);
        brick.material[i] = raw[2 * BRICK_CELLS + i];
    }
    Ok(brick)
}

/// Serialize a brick authored at lattice spacing `cell_size` as `.vbk`
/// bytes (layout in the module docs).
pub fn encode_brick(brick: &VolumeBrick, cell_size: f32) -> Vec<u8> {
    let packed = pack_brick_payload(brick);

    let mut out = Vec::with_capacity(VBK_HEADER_LEN + packed.len());
    out.extend_from_slice(&VBK_MAGIC);
    out.extend_from_slice(&VBK_VERSION.to_le_bytes());
    out.extend_from_slice(&(BRICK_EDGE as u16).to_le_bytes());
    out.extend_from_slice(&cell_size.to_le_bytes());
    out.extend_from_slice(&packed);
    out
}

/// Parse `.vbk` bytes into the brick and the lattice spacing it was saved
/// at. Rejects a wrong magic, version, edge, cell size or payload length.
pub fn decode_brick(bytes: &[u8]) -> Result<(VolumeBrick, f32), String> {
    if bytes.len() < VBK_HEADER_LEN + 4 {
        return Err(format!("brick file is {} bytes, shorter than its header", bytes.len()));
    }
    if bytes[..4] != VBK_MAGIC[..] {
        return Err("brick file does not start with the EVBK magic".to_string());
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VBK_VERSION {
        return Err(format!("brick file is format version {version}, this build reads {VBK_VERSION}"));
    }
    let edge = u16::from_le_bytes([bytes[6], bytes[7]]);
    if edge as i32 != BRICK_EDGE {
        return Err(format!("brick edge is {edge}, expected {BRICK_EDGE}"));
    }
    let cell_size = f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if !(cell_size.is_finite() && cell_size > 0.0) {
        return Err(format!("brick cell size {cell_size} is not a positive length"));
    }
    let brick = unpack_brick_payload(&bytes[VBK_HEADER_LEN..])?;
    Ok((brick, cell_size))
}

/// What [`save_volume_bricks`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VolumeSaveReport {
    /// Brick files written.
    pub written: usize,
    /// Stale brick files deleted (bricks no longer in the volume, and
    /// leftovers of an interrupted save).
    pub removed: usize,
}

/// Write every brick of `volume` to `terrain_dir/volume/b{x}_{y}_{z}.vbk`,
/// then delete the brick files the volume no longer has, so a reload
/// cannot bring back an edit that was undone or cleared.
///
/// Takes the config for the lattice spacing written into each header. Each
/// file is written beside its target and renamed over it, so an interrupted
/// save leaves every brick either old or new, never torn. Stale files are
/// only deleted after every write succeeded, and only files named like
/// bricks are ever touched. Brick files the load could not use are kept
/// (see [`TerrainVolume::keeps_unloaded_file`]): they hold edits this
/// session never saw.
pub fn save_volume_bricks(
    terrain_dir: &Path,
    config: &TerrainConfig,
    volume: &TerrainVolume,
) -> Result<VolumeSaveReport, String> {
    let dir = volume_dir(terrain_dir);
    let cell = lattice_cell_size(config);
    let mut report = VolumeSaveReport::default();

    if !volume.is_empty() {
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("Failed to create terrain volume directory {:?}: {}", dir, error))?;
    }
    for (coord, brick) in volume.bricks() {
        let name = brick_file_name(coord);
        let path = dir.join(&name);
        let staging = dir.join(format!("{name}.tmp"));
        std::fs::write(&staging, encode_brick(brick, cell))
            .map_err(|error| format!("Failed to write terrain brick {:?}: {}", staging, error))?;
        if let Err(error) = std::fs::rename(&staging, &path) {
            let _ = std::fs::remove_file(&staging);
            return Err(format!("Failed to move terrain brick into place at {:?}: {}", path, error));
        }
        report.written += 1;
    }

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => {
            return Err(format!("Failed to list terrain volume directory {:?}: {}", dir, error));
        }
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let stale = match parse_brick_file_name(name) {
            Some(coord) => volume.brick(coord).is_none() && !volume.keeps_unloaded_file(coord),
            None => name
                .strip_suffix(".tmp")
                .is_some_and(|brick_name| parse_brick_file_name(brick_name).is_some()),
        };
        if stale {
            let path = entry.path();
            std::fs::remove_file(&path)
                .map_err(|error| format!("Failed to delete stale terrain brick {:?}: {}", path, error))?;
            report.removed += 1;
        }
    }
    Ok(report)
}

/// Read every `b{x}_{y}_{z}.vbk` under `terrain_dir/volume` into a volume.
///
/// A Space without the folder has an empty volume. A file that cannot be
/// read or decoded, or that was saved at a lattice spacing other than this
/// config's (its bricks would land in the wrong place), is skipped with a
/// warning rather than failing the whole terrain load. Skipped files stay on
/// disk, and later saves of this volume leave them alone, as they do every
/// file when the folder could not be listed at all.
pub fn load_volume_bricks(terrain_dir: &Path, config: &TerrainConfig) -> TerrainVolume {
    let dir = volume_dir(terrain_dir);
    let mut volume = TerrainVolume::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return volume,
        Err(error) => {
            tracing::warn!("terrain volume: cannot list {:?}: {}; loading without volumetric edits", dir, error);
            volume.listing_failed = true;
            return volume;
        }
    };
    let cell = lattice_cell_size(config);
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(coord) = file_name.to_str().and_then(parse_brick_file_name) else {
            continue;
        };
        let path = entry.path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                tracing::warn!("terrain volume: cannot read {:?}: {}; skipping the brick", path, error);
                volume.unloaded.insert(coord);
                continue;
            }
        };
        match decode_brick(&bytes) {
            Ok((brick, saved_cell)) => {
                if (saved_cell - cell).abs() > cell * 1e-4 {
                    tracing::warn!(
                        "terrain volume: {:?} was saved at cell size {} but the terrain lattice is {}; skipping the brick",
                        path,
                        saved_cell,
                        cell
                    );
                    volume.unloaded.insert(coord);
                    continue;
                }
                volume.set_brick(coord, Some(brick));
            }
            Err(error) => {
                tracing::warn!("terrain volume: {:?}: {}; skipping the brick", path, error);
                volume.unloaded.insert(coord);
            }
        }
    }
    volume
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::chunk_height_grid;

    /// 32 m chunks at 16 cells: a 2 m lattice. The 64 m band from Y 0 keeps
    /// whole-metre heights exact through the normalized raster.
    fn test_config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 2,
            chunks_z: 2,
            lod_levels: 1,
            lod_distances: vec![64.0],
            view_distance: 512.0,
            height_scale: 64.0,
            height_offset: 0.0,
            seed: 1,
        }
    }

    fn flat_data(config: &TerrainConfig, world_y: f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let normalized = config.normalized_height(world_y);
        data.height_cache.iter_mut().for_each(|h| *h = normalized);
        data
    }

    fn rolling_data(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = 0.2 + 0.1 * ((i % 37) as f32 / 37.0) + 0.05 * ((i / 11 % 13) as f32 / 13.0);
        }
        data
    }

    fn temp_terrain_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress_terrain_volume_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn quantized_distances_round_trip_within_half_a_step() {
        for cell in [0.5f32, 2.0] {
            let mut d = -3.9 * cell;
            while d <= 3.9 * cell {
                let back = dequantize_distance(quantize_distance(d, cell), cell);
                assert!((back - d).abs() <= cell / 64.0 + 1e-6, "{d} at cell {cell} came back as {back}");
                d += cell * 0.037;
            }
            assert_eq!(quantize_distance(10.0 * cell, cell), Q_NONE);
            assert_eq!(quantize_distance(-10.0 * cell, cell), -Q_NONE);
            assert_eq!(quantize_distance(f32::INFINITY, cell), Q_NONE);
            assert_eq!(quantize_distance(f32::NEG_INFINITY, cell), -Q_NONE);
            assert_eq!(quantize_distance(f32::NAN, cell), Q_NONE);
            assert_eq!(quantize_distance(0.0, cell), 0);
            assert_eq!(quantize_distance(cell, cell), 32);
        }
    }

    #[test]
    fn lattice_points_split_into_bricks_on_both_sides_of_zero() {
        assert_eq!(lattice_to_brick(IVec3::new(0, 0, 0)), (IVec3::ZERO, 0));
        assert_eq!(lattice_to_brick(IVec3::new(15, 0, 0)), (IVec3::ZERO, 15));
        assert_eq!(lattice_to_brick(IVec3::new(16, 0, 0)), (IVec3::new(1, 0, 0), 0));
        assert_eq!(lattice_to_brick(IVec3::new(-1, 0, 0)), (IVec3::new(-1, 0, 0), 15));
        assert_eq!(lattice_to_brick(IVec3::new(0, -16, 0)), (IVec3::new(0, -1, 0), 0));
        assert_eq!(lattice_to_brick(IVec3::new(1, 2, 3)).1, 1 + 16 * 2 + 256 * 3);
        let config = test_config();
        assert_eq!(lattice_cell_size(&config), 2.0);
        assert_eq!(brick_coord_at_world(&config, Vec3::new(31.9, 0.0, -0.1)), IVec3::new(0, 0, -1));
        assert_eq!(brick_coord_at_world(&config, Vec3::new(32.0, 0.0, 0.0)), IVec3::new(1, 0, 0));
    }

    #[test]
    fn an_unedited_volume_samples_exactly_the_heightfield() {
        let config = test_config();
        let data = rolling_data(&config);
        let empty = TerrainVolume::empty();
        assert!(empty.is_empty());
        let mut far_edit = TerrainVolume::new();
        apply_sphere(&config, &mut far_edit, Vec3::new(80.0, 20.0, 80.0), 3.0, CsgOp::Carve, None);
        assert!(!far_edit.is_empty());
        for x in -20..20 {
            for z in -20..20 {
                for y in [-5.0f32, 3.3, 12.7, 20.0] {
                    let p = Vec3::new(x as f32 * 1.37, y, z as f32 * 1.91);
                    let fh = heightfield_term(&config, &data, p);
                    assert_eq!(sample_field(&config, &data, empty, p).to_bits(), fh.to_bits(), "empty volume at {p}");
                    assert_eq!(sample_field(&config, &data, &far_edit, p).to_bits(), fh.to_bits(), "far edit at {p}");
                    assert_eq!(sample_field_parts(&config, &data, &far_edit, p).term(), FieldTerm::Heightfield);
                }
            }
        }
        assert_eq!(material_at(&config, empty, Vec3::ZERO), None);
    }

    #[test]
    fn a_carved_sphere_under_flat_ground_opens_air_at_its_centre() {
        let config = test_config();
        let data = flat_data(&config, 10.0);
        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::ZERO, 4.0, CsgOp::Carve, None);
        assert!(!edit.is_empty());
        assert!(edit.min.cmple(Vec3::splat(-4.0)).all() && edit.max.cmpge(Vec3::splat(4.0)).all());

        let centre = sample_field_parts(&config, &data, &volume, Vec3::ZERO);
        assert!(centre.heightfield < 0.0, "the centre is under the ground");
        assert!((centre.value - 4.0).abs() < 1e-5, "the centre is 4 m inside the cave, got {}", centre.value);
        assert_eq!(centre.term(), FieldTerm::Carve);
        assert_eq!(material_at(&config, &volume, Vec3::new(0.0, 0.0, 4.0)), Some(TerrainMaterial::Rock));

        // One cell outside the cave wall is still rock.
        assert!(sample_field(&config, &data, &volume, Vec3::new(0.0, 0.0, 6.0)) < 0.0);
        // The ground surface above is untouched.
        let surface = Vec3::new(0.0, 10.0, 0.0);
        assert_eq!(
            sample_field(&config, &data, &volume, surface).to_bits(),
            heightfield_term(&config, &data, surface).to_bits()
        );
        // Far away the field is exactly the heightfield.
        let far = Vec3::new(60.0, 5.0, 60.0);
        assert_eq!(
            sample_field(&config, &data, &volume, far).to_bits(),
            heightfield_term(&config, &data, far).to_bits()
        );
    }

    #[test]
    fn an_added_sphere_makes_solid_above_the_ground() {
        let config = test_config();
        let data = flat_data(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let centre = Vec3::new(0.0, 12.0, 0.0);
        apply_sphere(&config, &mut volume, centre, 4.0, CsgOp::Add, Some(TerrainMaterial::Sandstone));

        let sample = sample_field_parts(&config, &data, &volume, centre);
        assert!(sample.heightfield > 0.0, "the centre is above the ground");
        assert!((sample.value + 4.0).abs() < 1e-5, "the centre is 4 m inside the ball, got {}", sample.value);
        assert_eq!(sample.term(), FieldTerm::Add);
        // The gap between the ground and the ball is air.
        assert!(sample_field(&config, &data, &volume, Vec3::new(0.0, 6.0, 0.0)) > 0.0);
        assert_eq!(material_at(&config, &volume, Vec3::new(0.0, 16.0, 0.0)), Some(TerrainMaterial::Sandstone));
        // The gradient points out of the ball at its top.
        let normal = field_normal(&config, &data, &volume, Vec3::new(0.0, 16.0, 0.0));
        assert!(normal.y > 0.9, "top of the ball faces up, got {normal}");
    }

    #[test]
    fn an_add_fills_an_earlier_carve_back_in() {
        let config = test_config();
        let data = flat_data(&config, 10.0);
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::ZERO, 4.0, CsgOp::Carve, None);
        assert!(sample_field(&config, &data, &volume, Vec3::ZERO) > 0.0);
        apply_sphere(&config, &mut volume, Vec3::ZERO, 4.0, CsgOp::Add, Some(TerrainMaterial::Dirt));
        // (4, 0, 0) and (0, -4, 0) are lattice points on the old cave wall,
        // where an exact tie would have left a film of f = 0.
        for p in [
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, -4.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, 0.0, 6.0),
        ] {
            assert!(sample_field(&config, &data, &volume, p) < 0.0, "{p} should be solid again");
        }
    }

    #[test]
    fn a_carve_nearby_leaves_an_add_surface_its_material() {
        let config = test_config();
        let data = flat_data(&config, 0.0);
        let cell = lattice_cell_size(&config);
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(0.0, 12.0, 0.0), 4.0, CsgOp::Add, Some(TerrainMaterial::Sandstone));
        // A carve whose lowest point is three cells above the ball's top.
        apply_sphere(&config, &mut volume, Vec3::new(0.0, 16.0 + 3.0 * cell + 2.0, 0.0), 2.0, CsgOp::Carve, None);
        assert_eq!(material_at(&config, &volume, Vec3::new(0.0, 16.0, 0.0)), Some(TerrainMaterial::Sandstone));
        assert!(sample_field(&config, &data, &volume, Vec3::new(0.0, 15.0, 0.0)) < 0.0, "the ball is still solid");
    }

    #[test]
    fn an_add_nearby_leaves_a_cave_wall_its_material() {
        let config = test_config();
        let cell = lattice_cell_size(&config);
        let mut volume = TerrainVolume::new();
        // A cave under the ground, walled in the default rock.
        apply_sphere(&config, &mut volume, Vec3::ZERO, 4.0, CsgOp::Carve, None);
        // A ball whose nearest point is three cells beyond the wall at z = 4.
        let ball = Vec3::new(0.0, 0.0, 4.0 + 3.0 * cell + 2.0);
        apply_sphere(&config, &mut volume, ball, 2.0, CsgOp::Add, Some(TerrainMaterial::Grass));
        assert_eq!(material_at(&config, &volume, Vec3::new(0.0, 0.0, 4.0)), Some(TerrainMaterial::Rock));
    }

    #[test]
    fn a_tunnel_through_an_added_blob_takes_the_carve_material() {
        let config = test_config();
        let data = flat_data(&config, 0.0);
        let mut volume = TerrainVolume::new();
        let centre = Vec3::new(0.0, 12.0, 0.0);
        apply_sphere(&config, &mut volume, centre, 6.0, CsgOp::Add, Some(TerrainMaterial::Sandstone));
        apply_sphere(&config, &mut volume, centre, 3.0, CsgOp::Carve, Some(TerrainMaterial::Rock));
        assert!(sample_field(&config, &data, &volume, centre) > 0.0, "the tunnel is open");
        // The wall at x = 3 lies between lattice points x = 2 (carved) and
        // x = 4, whose add distance is strongly negative, deep inside the
        // blob: the carve wall is still the surface nearest it.
        assert_eq!(material_at(&config, &volume, Vec3::new(3.0, 12.0, 0.0)), Some(TerrainMaterial::Rock));
        assert_eq!(material_at(&config, &volume, Vec3::new(4.0, 12.0, 0.0)), Some(TerrainMaterial::Rock));
    }

    #[test]
    fn box_and_cylinder_distances_are_signed_and_exact_on_the_axes() {
        let cube = CsgShape::AxisBox { center: Vec3::new(1.0, 2.0, 3.0), half_extents: Vec3::new(1.0, 2.0, 3.0) };
        assert!((cube.distance(Vec3::new(1.0, 2.0, 3.0)) + 1.0).abs() < 1e-6);
        assert!((cube.distance(Vec3::new(4.0, 2.0, 3.0)) - 2.0).abs() < 1e-6);
        assert!((cube.distance(Vec3::new(3.0, 5.0, 3.0)) - 2f32.sqrt()).abs() < 1e-6);

        let can = CsgShape::Cylinder { center: Vec3::ZERO, radius: 2.0, half_height: 3.0 };
        assert!((can.distance(Vec3::ZERO) + 2.0).abs() < 1e-6);
        assert!((can.distance(Vec3::new(0.0, 5.0, 0.0)) - 2.0).abs() < 1e-6);
        assert!((can.distance(Vec3::new(0.0, 0.0, 5.0)) - 3.0).abs() < 1e-6);
        assert!((can.distance(Vec3::new(3.0, 4.0, 0.0)) - 2f32.sqrt()).abs() < 1e-6);

        let config = test_config();
        let data = flat_data(&config, 0.0);
        let mut volume = TerrainVolume::new();
        apply_box(&config, &mut volume, Vec3::new(0.0, 10.0, 0.0), Vec3::splat(3.0), CsgOp::Add, None);
        apply_cylinder(&config, &mut volume, Vec3::new(0.0, 0.0, 20.0), 3.0, 4.0, CsgOp::Carve, None);
        assert!(sample_field(&config, &data, &volume, Vec3::new(0.0, 10.0, 0.0)) < 0.0);
        assert!(sample_field(&config, &data, &volume, Vec3::new(0.0, -2.0, 20.0)) > 0.0);
        // Degenerate shapes change nothing.
        assert!(apply_sphere(&config, &mut volume, Vec3::ZERO, 0.0, CsgOp::Add, None).is_empty());
        assert!(apply_box(&config, &mut volume, Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0), CsgOp::Add, None).is_empty());
        assert!(apply_sphere(&config, &mut volume, Vec3::NAN, 2.0, CsgOp::Carve, None).is_empty());
    }

    #[test]
    fn the_field_is_continuous_across_a_brick_border() {
        let config = test_config();
        let data = flat_data(&config, 0.0);
        let mut volume = TerrainVolume::new();
        // Brick x = 0 owns world x 0..=30, brick x = 1 starts at 32.
        apply_sphere(&config, &mut volume, Vec3::new(32.0, 12.0, 0.0), 6.0, CsgOp::Add, None);
        assert!(volume.brick(IVec3::new(0, 0, 0)).is_some() && volume.brick(IVec3::new(1, 0, 0)).is_some());

        let cell = lattice_cell_size(&config);
        let add_at = |x: f32| volume.edit_distances(cell, Vec3::new(x, 12.0, 0.3)).0;
        let step = 0.05f32;
        // Trilinear over a 1-Lipschitz distance, each corner off by at most
        // 1/64 cell, moves at most 1 + 1/32 per unit.
        let bound = step * (1.0 + 1.0 / 16.0) + 1e-5;
        let mut x = 26.0f32;
        let mut previous = add_at(x);
        while x < 38.0 {
            x += step;
            let next = add_at(x);
            assert!(next.is_finite());
            assert!((next - previous).abs() <= bound, "jump of {} at x = {x}", (next - previous).abs());
            let f_prev = sample_field(&config, &data, &volume, Vec3::new(x - step, 12.0, 0.3));
            let f_next = sample_field(&config, &data, &volume, Vec3::new(x, 12.0, 0.3));
            assert!((f_next - f_prev).abs() <= bound, "field jump at x = {x}");
            previous = next;
        }
        let left = add_at(32.0 - 1e-3);
        let right = add_at(32.0 + 1e-3);
        assert!((left - right).abs() < 3e-3, "border values {left} and {right}");
    }

    #[test]
    fn bricks_left_without_edits_are_dropped() {
        let config = test_config();
        let mut volume = TerrainVolume::new();
        volume.set_brick(IVec3::new(2, 0, 0), Some(VolumeBrick::new()));
        assert!(volume.is_empty(), "a brick without edits is never stored");

        let mut painted_only = VolumeBrick::new();
        painted_only.material.iter_mut().for_each(|m| *m = TerrainMaterial::Snow.to_u8());
        volume.set_brick(IVec3::new(2, 0, 0), Some(painted_only));
        assert!(volume.is_empty(), "material without a distance edit is not an edit");

        let shape = CsgShape::Sphere { center: Vec3::new(5.0, 3.0, -7.0), radius: 5.0 };
        let (lo, hi) = shape.edit_bounds(&config);
        let coords = brick_coords_in_aabb(&config, lo, hi);
        let before = volume.snapshot_bricks(&coords);
        let edit = apply_shape(&config, &mut volume, shape, CsgOp::Carve, None);
        assert!(!volume.is_empty());
        assert!(edit.bricks.iter().all(|coord| coords.contains(coord)), "the op stays inside its edit bounds");

        let undo = volume.restore_bricks(&config, &before);
        assert!(volume.is_empty(), "restoring the pre-edit bricks leaves nothing behind");
        assert_eq!(undo.bricks, edit.bricks);

        // A brick whose only edit is reset by hand disappears on prune.
        let mut single = VolumeBrick::new();
        single.add[brick_cell_index(3, 4, 5)] = -10;
        volume.set_brick(IVec3::ONE, Some(single));
        let mut brick = volume.brick(IVec3::ONE).cloned().expect("stored");
        brick.add[brick_cell_index(3, 4, 5)] = Q_NONE;
        // Past `set_brick`, which would drop the emptied brick itself.
        volume.insert_brick(IVec3::ONE, brick);
        assert_eq!(volume.prune(), vec![IVec3::ONE]);
        assert!(volume.is_empty());
        assert!(volume.influence_bounds(2.0).is_none(), "the column index empties with the bricks");
    }

    #[test]
    fn smoothing_rounds_a_box_corner_and_ignores_untouched_ground() {
        let config = test_config();
        let mut volume = TerrainVolume::new();
        assert!(apply_smooth(&config, &mut volume, Vec3::ZERO, 6.0, 1.0).is_empty());

        apply_box(&config, &mut volume, Vec3::new(0.0, 20.0, 0.0), Vec3::splat(4.0), CsgOp::Add, None);
        let corner = IVec3::new(2, 12, 2); // world (4, 24, 4), on the box corner
        let before = volume.cell_at_lattice(corner).add;
        let edit = apply_smooth(&config, &mut volume, Vec3::new(4.0, 24.0, 4.0), 6.0, 1.0);
        assert!(!edit.is_empty());
        let after = volume.cell_at_lattice(corner).add;
        assert!(after > before, "the convex corner moves outward-positive: {before} -> {after}");

        // Far from every edit there is nothing to blur.
        assert!(apply_smooth(&config, &mut volume, Vec3::new(-60.0, -40.0, 60.0), 4.0, 1.0).is_empty());
    }

    #[test]
    fn chunk_columns_see_the_bricks_on_their_lattice() {
        let config = test_config();
        let mut volume = TerrainVolume::new();
        // One edited point at lattice (16, 0, 0): the border column shared
        // by chunks x = 0 and x = 1, and row z = 0 shared by z = -1 and 0.
        let mut brick = VolumeBrick::new();
        brick.add[brick_cell_index(0, 0, 0)] = 0;
        volume.set_brick(IVec3::new(1, 0, 0), Some(brick));
        for chunk in [IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(0, -1), IVec2::new(1, -1)] {
            assert!(chunk_has_volume(chunk, &config, &volume), "{chunk} holds lattice (16, 0)");
        }
        for chunk in [IVec2::new(-1, 0), IVec2::new(2, 0), IVec2::new(0, 1), IVec2::new(2, 2)] {
            assert!(!chunk_has_volume(chunk, &config, &volume), "{chunk} does not");
        }
        assert_eq!(chunk_volume_y_range(IVec2::new(0, 0), &config, &volume), Some((0.0, 30.0)));
        assert_eq!(chunk_volume_y_range(IVec2::new(2, 2), &config, &volume), None);

        let near: Vec<IVec3> = volume
            .bricks_in_aabb(&config, Vec3::new(20.0, 0.0, 0.0), Vec3::new(31.0, 1.0, 1.0))
            .map(|(coord, _)| coord)
            .collect();
        assert_eq!(near, vec![IVec3::new(1, 0, 0)], "influence reaches one cell before the brick");
        assert_eq!(volume.bricks_in_aabb(&config, Vec3::splat(100.0), Vec3::splat(120.0)).count(), 0);
    }

    #[test]
    fn chunk_column_queries_match_a_scan_of_every_brick() {
        let config = test_config();
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(-40.0, 10.0, 25.0), 9.0, CsgOp::Carve, None);
        apply_box(&config, &mut volume, Vec3::new(60.0, -30.0, -70.0), Vec3::new(20.0, 3.0, 5.0), CsgOp::Add, None);
        assert!(volume.brick_count() > 4);
        let cell = lattice_cell_size(&config);
        let r = config.resolution_for_lod(0) as i32;
        let sorted = |mut coords: Vec<IVec3>| {
            coords.sort_unstable_by_key(|c| (c.x, c.y, c.z));
            coords
        };
        for cz in -4..=4 {
            for cx in -4..=4 {
                let chunk = IVec2::new(cx, cz);
                let (x0, x1, z0, z1) = (cx * r, (cx + 1) * r, cz * r, (cz + 1) * r);
                let expected = sorted(
                    volume
                        .bricks()
                        .map(|(coord, _)| coord)
                        .filter(|c| {
                            let (bx0, bz0) = (c.x * BRICK_EDGE, c.z * BRICK_EDGE);
                            bx0 <= x1 && bx0 + BRICK_EDGE - 1 >= x0 && bz0 <= z1 && bz0 + BRICK_EDGE - 1 >= z0
                        })
                        .collect(),
                );
                let got = sorted(volume.bricks_in_chunk_column(&config, chunk).map(|(coord, _)| coord).collect());
                assert_eq!(got, expected, "chunk {chunk}");
                let range = expected
                    .iter()
                    .map(|&coord| {
                        let (lo, hi) = brick_world_bounds(coord, cell);
                        (lo.y, hi.y)
                    })
                    .reduce(|(a, b), (lo, hi)| (a.min(lo), b.max(hi)));
                assert_eq!(chunk_volume_y_range(chunk, &config, &volume), range, "chunk {chunk}");
            }
        }
        let scanned = volume
            .bricks()
            .map(|(coord, _)| brick_influence_bounds(coord, cell))
            .reduce(|(a, b), (lo, hi)| (a.min(lo), b.max(hi)));
        assert_eq!(volume.influence_bounds(cell), scanned);
    }

    #[test]
    fn lattice_heights_match_the_chunk_mesh_vertices_bit_for_bit() {
        let config = test_config();
        let data = rolling_data(&config);
        let resolution = config.resolution_for_lod(0);
        let r = resolution as i32;
        for chunk in [IVec2::new(-1, 0), IVec2::new(0, 0), IVec2::new(1, -1), IVec2::new(2, 2)] {
            let grid = chunk_height_grid(chunk, resolution, &config, &data);
            for z in 0..=r {
                for x in 0..=r {
                    let expected = grid[(z * (r + 1) + x) as usize];
                    let got = lattice_surface_height(&config, &data, chunk.x * r + x, chunk.y * r + z);
                    assert_eq!(got.to_bits(), expected.to_bits(), "chunk {chunk} vertex ({x}, {z})");
                }
            }
        }
        // And the lattice sample of an unedited point is that height's fh.
        let volume = TerrainVolume::new();
        let n = IVec3::new(5, 7, -3);
        let lattice = sample_field_lattice(&config, &data, &volume, n);
        let world = sample_field(&config, &data, &volume, lattice_point_world(n, 2.0));
        assert!((lattice.value - world).abs() < 1e-4);
        assert_eq!(lattice.term(), FieldTerm::Heightfield);
    }

    fn patterned_brick() -> VolumeBrick {
        let mut brick = VolumeBrick::new();
        for i in 0..BRICK_CELLS {
            brick.add[i] = ((i * 7 % 255) as i32 - 127) as i8;
            brick.carve[i] = if i % 3 == 0 { Q_NONE } else { ((i * 13 % 255) as i32 - 127) as i8 };
            brick.material[i] = if i % 5 == 0 { MATERIAL_NONE } else { (i % 23) as u8 };
        }
        brick
    }

    #[test]
    fn encoded_bricks_decode_to_the_same_cells() {
        let brick = patterned_brick();
        let bytes = encode_brick(&brick, 2.0);
        assert_eq!(&bytes[..4], b"EVBK");
        let (decoded, cell) = decode_brick(&bytes).expect("decodes");
        assert_eq!(cell, 2.0);
        assert!(decoded == brick);

        let mut bad_magic = bytes.clone();
        bad_magic[0] = b'X';
        assert!(decode_brick(&bad_magic).is_err());
        let mut bad_version = bytes.clone();
        bad_version[4] = 2;
        assert!(decode_brick(&bad_version).is_err());
        let mut bad_edge = bytes.clone();
        bad_edge[6] = 8;
        assert!(decode_brick(&bad_edge).is_err());
        let mut bad_cell = bytes.clone();
        bad_cell[8..12].copy_from_slice(&(-1.0f32).to_le_bytes());
        assert!(decode_brick(&bad_cell).is_err());
        let mut huge = bytes.clone();
        huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_brick(&huge).is_err());
        assert!(decode_brick(&bytes[..bytes.len() / 2]).is_err());
        assert!(decode_brick(&bytes[..10]).is_err());
    }

    #[test]
    fn brick_file_names_round_trip_with_negative_coordinates() {
        for coord in [IVec3::ZERO, IVec3::new(-3, 12, -40), IVec3::new(7, -1, 0)] {
            assert_eq!(parse_brick_file_name(&brick_file_name(coord)), Some(coord));
        }
        for name in ["b1_2.vbk", "b1_2_3_4.vbk", "x1_2_3.vbk", "b1_2_3.vbk.tmp", "b1_a_3.vbk", "notes.txt"] {
            assert_eq!(parse_brick_file_name(name), None, "{name}");
        }
    }

    #[test]
    fn saved_bricks_load_back_and_stale_files_are_removed() {
        let config = test_config();
        let dir = temp_terrain_dir("round_trip");
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(3.0, -4.0, 5.0), 6.0, CsgOp::Carve, None);
        apply_box(&config, &mut volume, Vec3::new(-20.0, 15.0, 8.0), Vec3::new(4.0, 2.0, 3.0), CsgOp::Add, Some(TerrainMaterial::Basalt));
        assert!(volume.brick_count() > 1);

        let folder = volume_dir(&dir);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("b99_0_-3.vbk"), b"stale").unwrap();
        std::fs::write(folder.join("b1_1_1.vbk.tmp"), b"interrupted").unwrap();
        std::fs::write(folder.join("notes.txt"), b"keep me").unwrap();

        let report = save_volume_bricks(&dir, &config, &volume).expect("saves");
        assert_eq!(report.written, volume.brick_count());
        assert_eq!(report.removed, 2);
        assert!(!folder.join("b99_0_-3.vbk").exists());
        assert!(!folder.join("b1_1_1.vbk.tmp").exists());
        assert!(folder.join("notes.txt").exists(), "files that are not bricks are left alone");

        let loaded = load_volume_bricks(&dir, &config);
        assert_eq!(loaded.brick_count(), volume.brick_count());
        for (coord, brick) in volume.bricks() {
            assert!(loaded.brick(coord) == Some(brick), "brick {coord} differs after reload");
        }

        // A lattice of another spacing would misplace every brick.
        let finer = TerrainConfig { chunk_resolution: 32, ..test_config() };
        assert!(load_volume_bricks(&dir, &finer).is_empty());

        // Saving an emptied volume clears the folder of bricks.
        let report = save_volume_bricks(&dir, &config, &TerrainVolume::new()).expect("saves");
        assert_eq!(report, VolumeSaveReport { written: 0, removed: volume.brick_count() });
        assert!(load_volume_bricks(&dir, &config).is_empty());
        assert!(load_volume_bricks(&temp_terrain_dir("missing"), &config).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn brick_files_the_load_skipped_survive_the_next_save() {
        let config = test_config();
        let dir = temp_terrain_dir("skipped");
        let mut volume = TerrainVolume::new();
        apply_sphere(&config, &mut volume, Vec3::new(3.0, -4.0, 5.0), 6.0, CsgOp::Carve, None);
        assert!(volume.brick_count() > 1);
        save_volume_bricks(&dir, &config, &volume).expect("saves");

        // At another lattice spacing the load skips every brick...
        let finer = TerrainConfig { chunk_resolution: 32, ..test_config() };
        let loaded = load_volume_bricks(&dir, &finer);
        assert!(loaded.is_empty());
        assert_eq!(loaded.unloaded_bricks().count(), volume.brick_count());

        // ...and saving what it loaded deletes none of them.
        let report = save_volume_bricks(&dir, &finer, &loaded).expect("saves");
        assert_eq!(report.removed, 0);
        for (coord, _) in volume.bricks() {
            assert!(brick_file_path(&dir, coord).exists(), "brick {coord} was deleted");
        }
        let reloaded = load_volume_bricks(&dir, &config);
        assert_eq!(reloaded.brick_count(), volume.brick_count());
        for (coord, brick) in volume.bricks() {
            assert!(reloaded.brick(coord) == Some(brick), "brick {coord} differs after the skipped save");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
