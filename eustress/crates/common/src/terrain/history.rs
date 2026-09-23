//! Terrain edit recording for the host's undo stack.
//!
//! A terrain writer records the cache TILES it touches. A tile is
//! `chunk_resolution` x `chunk_resolution` cells of the global raster, laid
//! out like the per-chunk `.r16` and matmap PNG blocks
//! `toml_loader::write_chunk_to_cache` copies in, so tile `(tx, tz)` holds
//! chunk `(tx - chunks_x, tz - chunks_z)`. The first write into a tile
//! snapshots its heights and material cells; when the edit ends,
//! [`TerrainEditRecorder::finish`] takes after-snapshots, drops the tiles
//! that did not change and returns the rest as [`TerrainTileDelta`]s. Undo
//! and redo write one side back with [`apply_terrain_tiles`].
//!
//! Only touched tiles are kept, so an edit costs a few tiles, never a copy
//! of the whole raster.
//!
//! Writers of the sparse `TerrainVolume` (the 3D brushes) record the BRICKS
//! they may write the same way: the first record of a brick coordinate keeps
//! a copy of the brick, or "no brick" where the write will create one, and
//! [`TerrainEditRecorder::finish_with_volume`] keeps the bricks that changed
//! as [`TerrainBrickDelta`]s. Undo and redo put one side back with
//! [`apply_terrain_bricks`].
//!
//! The brush takes the recorder as an optional resource: the Studio engine
//! inserts it and pushes each finished stroke onto its undo stack, while the
//! Client never inserts it, so its brush records nothing.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::height_query::{cache_cell_at_world, ensure_material_cache};
use super::material::MaterialCell;
use super::volume::{brick_coords_in_aabb, lattice_cell_size, TerrainVolume, VolumeBrick, VolumeEdit, VBK_PAYLOAD_LEN};
use super::{BrushMode, HeightBand, TerrainConfig, TerrainData, TerrainDirtyChunks};

/// Undo label for a brush stroke made in `mode`.
pub fn terrain_stroke_label(mode: BrushMode) -> &'static str {
    match mode {
        BrushMode::PaintTexture | BrushMode::Fill => "Paint Terrain",
        BrushMode::VoxelAdd => "Add Terrain",
        BrushMode::VoxelRemove => "Subtract Terrain",
        BrushMode::VoxelSmooth => "Smooth Terrain",
        BrushMode::Raise
        | BrushMode::Lower
        | BrushMode::Smooth
        | BrushMode::Flatten
        | BrushMode::Region => "Sculpt Terrain",
    }
}

// ============================================================================
// Tile deltas
// ============================================================================

/// Before and after contents of one cache tile, as the undo stack stores it.
///
/// Heights and material cells run row-major over the tile's cells, one
/// [`MaterialCell`] per cell. A pair of empty vectors means that layer did
/// not change. A material side that is empty while the other is not means
/// the terrain had no material layer at that moment, which is a different
/// state from any cells: the mesh then colours by altitude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainTileDelta {
    /// Tile column and row: cache cell index / `resolution`.
    pub tile: UVec2,
    /// Tile edge in cells, the terrain's `chunk_resolution` when recorded.
    pub resolution: u32,
    /// Raster width when recorded. Undo refuses a raster of another size.
    pub cache_width: u32,
    /// Raster depth when recorded.
    pub cache_height: u32,
    pub heights_before: Vec<f32>,
    pub heights_after: Vec<f32>,
    #[serde(default)]
    pub material_before: Vec<MaterialCell>,
    #[serde(default)]
    pub material_after: Vec<MaterialCell>,
}

impl TerrainTileDelta {
    /// Bytes of raster data this delta holds, for the host's history budget.
    pub fn byte_len(&self) -> usize {
        (self.heights_before.len() + self.heights_after.len()) * std::mem::size_of::<f32>()
            + (self.material_before.len() + self.material_after.len()) * std::mem::size_of::<MaterialCell>()
    }

    /// Whether this tile's material layer changed (or was allocated or
    /// dropped) in the edit.
    pub fn touches_materials(&self) -> bool {
        !self.material_before.is_empty() || !self.material_after.is_empty()
    }

    /// Whether the edit allocated or dropped the material layer: one side
    /// has cells and the other has none.
    pub fn changes_material_layout(&self) -> bool {
        self.material_before.is_empty() != self.material_after.is_empty()
    }

    /// Heights and material cells on `side`.
    fn side(&self, side: TerrainTileSide) -> (&[f32], &[MaterialCell]) {
        match side {
            TerrainTileSide::Before => (self.heights_before.as_slice(), self.material_before.as_slice()),
            TerrainTileSide::After => (self.heights_after.as_slice(), self.material_after.as_slice()),
        }
    }

    /// Material cells on the side opposite `side`.
    fn other_material(&self, side: TerrainTileSide) -> &[MaterialCell] {
        match side {
            TerrainTileSide::Before => self.material_after.as_slice(),
            TerrainTileSide::After => self.material_before.as_slice(),
        }
    }
}

/// Re-express the heights `tiles` hold, normalized in band `from`, in band
/// `to`. Tiles store raw raster samples, so once Save moves the terrain's
/// height band (`TerrainConfig::band_covering`) an entry left as it was
/// would restore other world heights than it recorded.
pub fn rebase_tile_heights(tiles: &mut [TerrainTileDelta], from: HeightBand, to: HeightBand) {
    for tile in tiles {
        from.rebase_into(to, &mut tile.heights_before);
        from.rebase_into(to, &mut tile.heights_after);
    }
}

// ============================================================================
// Brick deltas
// ============================================================================

/// Before and after contents of one volume brick, as the undo stack stores
/// it. `None` is no brick at that coordinate: none of its lattice points
/// carried an edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainBrickDelta {
    /// Brick coordinate: brick `b` owns lattice points `16 b ..= 16 b + 15`.
    pub coord: IVec3,
    /// Lattice cell size in world units when recorded. Undo refuses a
    /// terrain whose lattice spacing changed since, where the brick's cells
    /// would land somewhere else.
    pub cell_size: f32,
    #[serde(with = "packed_brick")]
    pub before: Option<VolumeBrick>,
    #[serde(with = "packed_brick")]
    pub after: Option<VolumeBrick>,
}

impl TerrainBrickDelta {
    /// Bytes of brick data this delta holds, for the host's history budget.
    pub fn byte_len(&self) -> usize {
        (self.before.is_some() as usize + self.after.is_some() as usize) * VBK_PAYLOAD_LEN
    }

    /// The brick on `side`.
    fn side(&self, side: TerrainTileSide) -> Option<&VolumeBrick> {
        match side {
            TerrainTileSide::Before => self.before.as_ref(),
            TerrainTileSide::After => self.after.as_ref(),
        }
    }
}

/// Serde for an optional brick as its `add | carve | material` bytes, lz4
/// packed like a `.vbk` payload. serde implements no arrays longer than 32
/// elements, and the host's undo entries, which hold these deltas, derive
/// `Serialize`.
mod packed_brick {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::terrain::volume::{pack_brick_payload, unpack_brick_payload, VolumeBrick};

    pub fn serialize<S: Serializer>(brick: &Option<VolumeBrick>, serializer: S) -> Result<S::Ok, S::Error> {
        brick.as_ref().map(pack_brick_payload).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<VolumeBrick>, D::Error> {
        Option::<Vec<u8>>::deserialize(deserializer)?
            .map(|bytes| unpack_brick_payload(&bytes).map_err(<D::Error as serde::de::Error>::custom))
            .transpose()
    }
}

/// A finished recording: the undo label, every tile that changed and every
/// volume brick that changed.
#[derive(Clone, Debug)]
pub struct RecordedTerrainEdit {
    pub label: String,
    pub tiles: Vec<TerrainTileDelta>,
    /// Empty for an edit that only wrote the raster.
    pub bricks: Vec<TerrainBrickDelta>,
}

/// One tile's contents at one moment.
#[derive(Clone, Debug, PartialEq)]
struct TileSnapshot {
    heights: Vec<f32>,
    /// Empty when the material layer was not allocated.
    material: Vec<MaterialCell>,
}

/// Cell rectangle `(x0, z0, width, depth)` of `tile`. The last column and
/// row of tiles are clipped when the raster is not a whole number of tiles.
fn tile_rect(tile: UVec2, resolution: u32, cache_width: u32, cache_height: u32) -> Option<(usize, usize, usize, usize)> {
    let res = resolution as usize;
    let (w, h) = (cache_width as usize, cache_height as usize);
    let x0 = (tile.x as usize).checked_mul(res)?;
    let z0 = (tile.y as usize).checked_mul(res)?;
    if res == 0 || x0 >= w || z0 >= h {
        return None;
    }
    Some((x0, z0, res.min(w - x0), res.min(h - z0)))
}

/// The height raster covers its whole `cache_width` x `cache_height` grid,
/// the only layout tiles can be cut from.
fn raster_is_whole(data: &TerrainData) -> bool {
    let total = data.cache_width as usize * data.cache_height as usize;
    total > 0 && data.height_cache.len() == total
}

/// Copy one tile out of the raster. The caller checks [`raster_is_whole`]
/// and takes `rect` from [`tile_rect`] for the same raster size.
fn snapshot_tile(data: &TerrainData, rect: (usize, usize, usize, usize)) -> TileSnapshot {
    let (x0, z0, tw, th) = rect;
    let w = data.cache_width as usize;
    let mut heights = Vec::with_capacity(tw * th);
    for z in z0..z0 + th {
        let start = z * w + x0;
        heights.extend_from_slice(&data.height_cache[start..start + tw]);
    }
    let mut material = Vec::new();
    if data.has_material_layer() {
        material.reserve(tw * th);
        for z in z0..z0 + th {
            let start = z * w + x0;
            material.extend_from_slice(&data.material_cache[start..start + tw]);
        }
    }
    TileSnapshot { heights, material }
}

/// The delta from `before` to `after`, or `None` when nothing changed. A
/// layer that did not change is stored as two empty vectors, so a sculpt
/// stroke does not carry material cells and a paint stroke does not carry
/// heights.
fn tile_delta(
    tile: UVec2,
    resolution: u32,
    cache_width: u32,
    cache_height: u32,
    before: TileSnapshot,
    after: TileSnapshot,
) -> Option<TerrainTileDelta> {
    if before == after {
        return None;
    }
    let (heights_before, heights_after) = if before.heights == after.heights {
        (Vec::new(), Vec::new())
    } else {
        (before.heights, after.heights)
    };
    let (material_before, material_after) = if before.material == after.material {
        (Vec::new(), Vec::new())
    } else {
        (before.material, after.material)
    };
    Some(TerrainTileDelta {
        tile,
        resolution,
        cache_width,
        cache_height,
        heights_before,
        heights_after,
        material_before,
        material_after,
    })
}

/// Deltas for every tile that differs between two rasters of one layout,
/// from `before` to `after`. For writers that swap in a whole saved raster:
/// the swap becomes one undo entry holding only what changed. Empty when the
/// layouts differ.
pub fn diff_terrain_tiles(config: &TerrainConfig, before: &TerrainData, after: &TerrainData) -> Vec<TerrainTileDelta> {
    let res = config.chunk_resolution;
    let (w, h) = (before.cache_width, before.cache_height);
    if res == 0
        || after.cache_width != w
        || after.cache_height != h
        || !raster_is_whole(before)
        || !raster_is_whole(after)
    {
        return Vec::new();
    }
    let mut deltas = Vec::new();
    for tz in 0..h.div_ceil(res) {
        for tx in 0..w.div_ceil(res) {
            let tile = UVec2::new(tx, tz);
            let Some(rect) = tile_rect(tile, res, w, h) else { continue };
            if let Some(delta) = tile_delta(tile, res, w, h, snapshot_tile(before, rect), snapshot_tile(after, rect)) {
                deltas.push(delta);
            }
        }
    }
    deltas
}

// ============================================================================
// Recorder
// ============================================================================

/// Records the tiles and volume bricks one terrain edit touches, for the
/// host's undo stack.
///
/// Open an edit with [`Self::begin`], call a `record_*` method before every
/// write, then close it with [`Self::finish`] (raster writers) or
/// [`Self::finish_with_volume`] (writers that also recorded bricks). Only the
/// first record of a tile or brick snapshots it, so the snapshot holds it as
/// it was before the edit's first write there, however many writes follow.
#[derive(Resource, Debug, Default)]
pub struct TerrainEditRecorder {
    edit: Option<OpenEdit>,
}

#[derive(Debug)]
struct OpenEdit {
    label: String,
    /// The terrain root being written, when the writer knows it.
    root: Option<Entity>,
    resolution: u32,
    cache_width: u32,
    cache_height: u32,
    before: HashMap<UVec2, TileSnapshot>,
    /// Most recently recorded tile. A brush writes hundreds of consecutive
    /// samples into one tile, so this skips the map lookup for them.
    last_tile: Option<UVec2>,
    /// Volume lattice spacing when the edit began, stamped on its bricks.
    cell_size: f32,
    /// Every brick coordinate the edit may write, holding the brick as it
    /// was before the edit's first write there (`None`: no brick yet).
    bricks_before: HashMap<IVec3, Option<VolumeBrick>>,
}

impl OpenEdit {
    /// `data` still has the raster layout the edit began on.
    fn matches(&self, data: &TerrainData) -> bool {
        data.cache_width == self.cache_width && data.cache_height == self.cache_height && raster_is_whole(data)
    }

    fn record_tile(&mut self, data: &TerrainData, tile: UVec2) {
        if self.last_tile == Some(tile) {
            return;
        }
        if !self.before.contains_key(&tile) {
            let Some(rect) = tile_rect(tile, self.resolution, self.cache_width, self.cache_height) else {
                return;
            };
            self.before.insert(tile, snapshot_tile(data, rect));
        }
        self.last_tile = Some(tile);
    }
}

impl TerrainEditRecorder {
    /// An edit is open.
    pub fn is_recording(&self) -> bool {
        self.edit.is_some()
    }

    /// Tiles snapshotted by the open edit so far.
    pub fn recorded_tiles(&self) -> usize {
        self.edit.as_ref().map_or(0, |edit| edit.before.len())
    }

    /// Brick coordinates snapshotted by the open edit so far, those that
    /// held no brick included.
    pub fn recorded_bricks(&self) -> usize {
        self.edit.as_ref().map_or(0, |edit| edit.bricks_before.len())
    }

    /// Open an edit labelled `label` on the terrain holding `data`. Returns
    /// `false` and changes nothing when an edit is already open (a stroke
    /// keeps the label it started with) or the raster cannot be tiled.
    pub fn begin(
        &mut self,
        label: impl Into<String>,
        root: Option<Entity>,
        config: &TerrainConfig,
        data: &TerrainData,
    ) -> bool {
        if self.edit.is_some() || config.chunk_resolution == 0 || !raster_is_whole(data) {
            return false;
        }
        self.edit = Some(OpenEdit {
            label: label.into(),
            root,
            resolution: config.chunk_resolution,
            cache_width: data.cache_width,
            cache_height: data.cache_height,
            before: HashMap::new(),
            last_tile: None,
            cell_size: lattice_cell_size(config),
            bricks_before: HashMap::new(),
        });
        true
    }

    /// Record the tile holding cache cell `cell`, ahead of a write to it.
    pub fn record_cell(&mut self, data: &TerrainData, cell: UVec2) {
        let Some(edit) = self.edit.as_mut() else { return };
        if !edit.matches(data) || cell.x >= edit.cache_width || cell.y >= edit.cache_height {
            return;
        }
        let tile = cell / edit.resolution;
        edit.record_tile(data, tile);
    }

    /// Record the tile a write at world `world_x, world_z` lands in.
    pub fn record_world_point(&mut self, config: &TerrainConfig, data: &TerrainData, world_x: f32, world_z: f32) {
        if self.edit.is_none() {
            return;
        }
        if let Some(cell) = cache_cell_at_world(config, data, world_x, world_z) {
            self.record_cell(data, cell);
        }
    }

    /// Record every tile that a write anywhere in the world XZ rectangle can
    /// land in, for writers that stamp a known area. The cell a position
    /// maps to never decreases as the position grows (the clamp and the
    /// rounding in [`cache_cell_at_world`] both keep order), so the cells of
    /// the two corners bound every cell in between.
    pub fn record_world_rect(&mut self, config: &TerrainConfig, data: &TerrainData, min_xz: Vec2, max_xz: Vec2) {
        let Some(edit) = self.edit.as_mut() else { return };
        if !(min_xz.is_finite() && max_xz.is_finite()) || !edit.matches(data) {
            return;
        }
        let lo = min_xz.min(max_xz);
        let hi = min_xz.max(max_xz);
        let (Some(first), Some(last)) = (
            cache_cell_at_world(config, data, lo.x, lo.y),
            cache_cell_at_world(config, data, hi.x, hi.y),
        ) else {
            return;
        };
        let (first, last) = (first / edit.resolution, last / edit.resolution);
        for tz in first.y..=last.y {
            for tx in first.x..=last.x {
                edit.record_tile(data, UVec2::new(tx, tz));
            }
        }
    }

    /// Record the volume bricks at `coords`, ahead of a write to them. A
    /// coordinate with no brick is recorded as "no brick", which is what
    /// undo puts back when the write creates one there.
    pub fn record_bricks(&mut self, volume: &TerrainVolume, coords: &[IVec3]) {
        let Some(edit) = self.edit.as_mut() else { return };
        for &coord in coords {
            edit.bricks_before.entry(coord).or_insert_with(|| volume.brick(coord).cloned());
        }
    }

    /// Record every brick a volume write inside world box `min..max` can
    /// create, change or remove: a CSG shape's `edit_bounds`, or the box of
    /// the ball a smooth blurs. The coordinates come from the same lattice
    /// range the CSG ops walk, so none they write is missed.
    pub fn record_volume_aabb(&mut self, config: &TerrainConfig, volume: &TerrainVolume, min: Vec3, max: Vec3) {
        if self.edit.is_none() {
            return;
        }
        let coords = brick_coords_in_aabb(config, min, max);
        self.record_bricks(volume, &coords);
    }

    /// Drop the open edit without recording it.
    pub fn cancel(&mut self) {
        self.edit = None;
    }

    /// Re-express the heights the open edit snapshotted, after the host
    /// moved the height band of terrain `root` from `from` to `to` (Save
    /// widening it to fit). The after-snapshots are taken in the new band at
    /// finish, so without this the before side would be the only half in
    /// the old one. An edit recorded on another root is left alone.
    pub fn rebase_heights(&mut self, root: Option<Entity>, from: HeightBand, to: HeightBand) {
        let Some(edit) = self.edit.as_mut() else { return };
        if edit.root.is_some() && root.is_some() && edit.root != root {
            return;
        }
        for snapshot in edit.before.values_mut() {
            from.rebase_into(to, &mut snapshot.heights);
        }
    }

    /// Close an open edit that only wrote the raster: take after-snapshots
    /// of every recorded tile and keep those that changed. `None` when no
    /// edit was open, nothing changed, or the terrain was replaced mid-edit
    /// (another root, or a raster of another size), since before and after
    /// would then describe different terrains.
    ///
    /// An edit that recorded volume bricks needs the volume for their
    /// after-snapshots ([`Self::finish_with_volume`]); closed here it is
    /// dropped with a warning rather than kept with half its changes.
    pub fn finish(&mut self, root: Option<Entity>, data: &TerrainData) -> Option<RecordedTerrainEdit> {
        if self.edit.as_ref().is_some_and(|edit| !edit.bricks_before.is_empty()) {
            self.edit = None;
            tracing::warn!("a terrain edit that recorded volume bricks was closed without its volume; it is not undoable");
            return None;
        }
        self.finish_with_volume(root, data, TerrainVolume::empty())
    }

    /// Close the open edit: take after-snapshots of every recorded tile and
    /// brick and keep those that changed. `volume` is the edited root's
    /// volume. `None` in the cases [`Self::finish`] lists.
    pub fn finish_with_volume(
        &mut self,
        root: Option<Entity>,
        data: &TerrainData,
        volume: &TerrainVolume,
    ) -> Option<RecordedTerrainEdit> {
        let edit = self.edit.take()?;
        if (edit.root.is_some() && edit.root != root) || !edit.matches(data) {
            return None;
        }
        let OpenEdit { label, resolution, cache_width, cache_height, before, cell_size, bricks_before, .. } = edit;
        let mut before: Vec<(UVec2, TileSnapshot)> = before.into_iter().collect();
        before.sort_by_key(|(tile, _)| (tile.y, tile.x));
        let tiles: Vec<TerrainTileDelta> = before
            .into_iter()
            .filter_map(|(tile, snapshot)| {
                let rect = tile_rect(tile, resolution, cache_width, cache_height)?;
                tile_delta(tile, resolution, cache_width, cache_height, snapshot, snapshot_tile(data, rect))
            })
            .collect();
        // Only bricks that differ are kept, so the entry costs the bricks the
        // edit changed, never every brick its dabs could have reached.
        let mut bricks: Vec<TerrainBrickDelta> = bricks_before
            .into_iter()
            .filter_map(|(coord, before)| {
                let after = volume.brick(coord);
                if before.as_ref() == after {
                    return None;
                }
                Some(TerrainBrickDelta { coord, cell_size, before, after: after.cloned() })
            })
            .collect();
        bricks.sort_by_key(|delta| (delta.coord.z, delta.coord.y, delta.coord.x));
        if tiles.is_empty() && bricks.is_empty() {
            return None;
        }
        Some(RecordedTerrainEdit { label, tiles, bricks })
    }
}

// ============================================================================
// Undo / redo
// ============================================================================

/// Which half of a [`TerrainTileDelta`] or [`TerrainBrickDelta`] to write
/// back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainTileSide {
    /// The contents before the edit (undo).
    Before,
    /// The contents after the edit (redo).
    After,
}

/// What [`apply_terrain_tiles`] wrote, so the caller can mark the right
/// chunks stale.
#[derive(Clone, Debug, Default)]
pub struct AppliedTerrainTiles {
    /// Chunk grid position of every tile written.
    pub chunks: Vec<IVec2>,
    /// The material layer was allocated or dropped. Every chunk's colouring
    /// depends on whether it exists, so every chunk is stale.
    pub material_layout_changed: bool,
}

impl AppliedTerrainTiles {
    /// Mark each written chunk plus one ring of neighbours (border vertices
    /// are shared, and normals and bilinear samples read across the
    /// border), or the whole grid when the material layout changed.
    pub fn mark_dirty(&self, config: &TerrainConfig, dirty: &mut TerrainDirtyChunks) {
        if self.material_layout_changed {
            dirty.mark_all(config);
            return;
        }
        if self.chunks.is_empty() {
            return;
        }
        let extent_x = config.chunks_x as i32;
        let extent_z = config.chunks_z as i32;
        let mut centre_sum = Vec2::ZERO;
        for chunk in &self.chunks {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let neighbour = *chunk + IVec2::new(dx, dz);
                    if neighbour.x.abs() <= extent_x && neighbour.y.abs() <= extent_z {
                        dirty.mark(neighbour);
                    }
                }
            }
            centre_sum += (chunk.as_vec2() + Vec2::splat(0.5)) * config.chunk_size;
        }
        // Remesh outward from the middle of the restored area.
        dirty.focus = Some(centre_sum / self.chunks.len() as f32);
    }
}

/// A layer vector holds either nothing (unchanged, or no material layer) or
/// exactly one value per cell.
fn layer_len_ok<T>(layer: &[T], cells: usize) -> bool {
    layer.is_empty() || layer.len() == cells
}

/// Write one side of `tiles` into `data`: `Before` for undo, `After` for
/// redo. Every tile is checked before anything is written, so a raster whose
/// layout changed since the edit (another size or chunk resolution) is
/// refused whole with the reason, instead of being written out of bounds
/// or half restored. Telling a replaced terrain of the same layout apart is
/// the host's job, since only it knows which root entity the edit was on.
pub fn apply_terrain_tiles(
    config: &TerrainConfig,
    data: &mut TerrainData,
    tiles: &[TerrainTileDelta],
    side: TerrainTileSide,
) -> Result<AppliedTerrainTiles, String> {
    if !raster_is_whole(data) {
        return Err("the terrain has no height raster".to_string());
    }
    let mut rects = Vec::with_capacity(tiles.len());
    for t in tiles {
        if t.cache_width != data.cache_width
            || t.cache_height != data.cache_height
            || t.resolution != config.chunk_resolution
        {
            return Err(format!(
                "the terrain raster changed layout since the edit (raster {}x{} at chunk resolution {}, edit recorded on {}x{} at {})",
                data.cache_width,
                data.cache_height,
                config.chunk_resolution,
                t.cache_width,
                t.cache_height,
                t.resolution,
            ));
        }
        let Some(rect) = tile_rect(t.tile, t.resolution, t.cache_width, t.cache_height) else {
            return Err(format!("tile ({}, {}) lies outside the raster", t.tile.x, t.tile.y));
        };
        let cells = rect.2 * rect.3;
        let sizes_ok = t.heights_before.len() == t.heights_after.len()
            && layer_len_ok(&t.heights_before, cells)
            && layer_len_ok(&t.material_before, cells)
            && layer_len_ok(&t.material_after, cells);
        if !sizes_ok {
            return Err(format!("tile ({}, {}) holds data of the wrong size", t.tile.x, t.tile.y));
        }
        rects.push(rect);
    }

    // A side with no material layer, where the other side had one, is the
    // terrain before its material layer existed: drop the layer whole
    // rather than leave it filled with the all-Grass default.
    let drop_material = tiles
        .iter()
        .any(|t| t.side(side).1.is_empty() && !t.other_material(side).is_empty());

    let mut applied = AppliedTerrainTiles::default();
    if drop_material && !data.material_cache.is_empty() {
        data.material_cache = Vec::new();
        data.material_dirty = true;
        applied.material_layout_changed = true;
    }

    let w = data.cache_width as usize;
    for (t, &(x0, z0, tw, th)) in tiles.iter().zip(&rects) {
        let (heights, material) = t.side(side);
        if !heights.is_empty() {
            for (row, z) in (z0..z0 + th).enumerate() {
                let dst = z * w + x0;
                data.height_cache[dst..dst + tw].copy_from_slice(&heights[row * tw..(row + 1) * tw]);
            }
        }
        if !drop_material && !material.is_empty() {
            if ensure_material_cache(data) {
                applied.material_layout_changed = true;
            }
            for (row, z) in (z0..z0 + th).enumerate() {
                let dst = z * w + x0;
                data.material_cache[dst..dst + tw].copy_from_slice(&material[row * tw..(row + 1) * tw]);
            }
            data.material_dirty = true;
        }
        applied.chunks.push(IVec2::new(
            t.tile.x as i32 - config.chunks_x as i32,
            t.tile.y as i32 - config.chunks_z as i32,
        ));
    }
    Ok(applied)
}

/// Refuse `bricks` when the terrain's lattice spacing is not the one they
/// were recorded at: written back, their cells would land somewhere else.
/// [`apply_terrain_bricks`] checks this itself; a host writing tiles for the
/// same edit checks it before the tiles too, so a refused edit writes
/// nothing at all.
pub fn check_terrain_bricks(config: &TerrainConfig, bricks: &[TerrainBrickDelta]) -> Result<(), String> {
    let cell = lattice_cell_size(config);
    match bricks.iter().find(|delta| !((delta.cell_size - cell).abs() <= cell * 1e-4)) {
        Some(delta) => Err(format!(
            "the terrain lattice changed since the edit (cells of {cell} m, edit recorded at {} m)",
            delta.cell_size
        )),
        None => Ok(()),
    }
}

/// Write one side of `bricks` into `volume`: `Before` for undo, `After` for
/// redo. Returns the bricks that changed and the world box their field
/// covers; the host marks it with `TerrainDirtyChunks::mark_volume_edit`, so
/// those chunks remesh and re-collide, switching between heightfield and
/// marching cubes where a brick appeared or went.
pub fn apply_terrain_bricks(
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    bricks: &[TerrainBrickDelta],
    side: TerrainTileSide,
) -> Result<VolumeEdit, String> {
    check_terrain_bricks(config, bricks)?;
    let snapshot: Vec<(IVec3, Option<VolumeBrick>)> =
        bricks.iter().map(|delta| (delta.coord, delta.side(side).cloned())).collect();
    Ok(volume.restore_bricks(config, &snapshot))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::height_query::{paint_material_at_world, set_height_at_world};
    use crate::terrain::material::{canonical_material_cell, material_cell};

    /// 3 x 3 chunks of 8 x 8 cells: a 24 x 24 raster cut into 9 tiles.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        }
    }

    /// A raster where every cell holds a different height, so a restore that
    /// puts any value in the wrong cell shows up.
    fn patterned(config: &TerrainConfig) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = i as f32 * 1e-3;
        }
        data
    }

    #[test]
    fn stroke_across_a_tile_border_snapshots_both_tiles_before_the_write() {
        let config = config();
        let mut data = patterned(&config);
        let original = data.clone();
        let w = data.cache_width as usize;

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin("Sculpt Terrain", None, &config, &data));

        // Columns 7 and 8 sit either side of the border between tile columns
        // 0 and 1. Row 11 is written after row 10, into tiles that already
        // hold a snapshot, so a re-snapshot would capture row 10's writes.
        for cell in [UVec2::new(7, 10), UVec2::new(8, 10), UVec2::new(7, 11), UVec2::new(8, 11)] {
            recorder.record_cell(&data, cell);
            data.height_cache[cell.y as usize * w + cell.x as usize] = 5.0;
        }
        assert_eq!(recorder.recorded_tiles(), 2);

        let edit = recorder.finish(None, &data).expect("the stroke changed two tiles");
        assert!(!recorder.is_recording());
        assert_eq!(edit.label, "Sculpt Terrain");
        let tiles: Vec<UVec2> = edit.tiles.iter().map(|t| t.tile).collect();
        assert_eq!(tiles, vec![UVec2::new(0, 1), UVec2::new(1, 1)]);
        for delta in &edit.tiles {
            let rect = tile_rect(delta.tile, 8, 24, 24).unwrap();
            assert_eq!(delta.heights_before, snapshot_tile(&original, rect).heights, "tile {:?} before", delta.tile);
            assert_eq!(delta.heights_after, snapshot_tile(&data, rect).heights, "tile {:?} after", delta.tile);
            assert!(!delta.touches_materials(), "no material layer to store");
        }
    }

    #[test]
    fn undo_and_redo_write_each_side_back_exactly() {
        let config = config();
        let mut data = patterned(&config);
        let original = data.clone();
        let w = data.cache_width as usize;
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", None, &config, &data);
        for cell in [UVec2::new(7, 10), UVec2::new(8, 10)] {
            recorder.record_cell(&data, cell);
            data.height_cache[cell.y as usize * w + cell.x as usize] = 5.0;
        }
        let edited = data.clone();
        let edit = recorder.finish(None, &data).unwrap();

        let applied = apply_terrain_tiles(&config, &mut data, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert_eq!(data.height_cache, original.height_cache);
        // Tile column 0 is chunk -1 and tile row 1 is chunk row 0.
        assert_eq!(applied.chunks, vec![IVec2::new(-1, 0), IVec2::new(0, 0)]);
        assert!(!applied.material_layout_changed);

        apply_terrain_tiles(&config, &mut data, &edit.tiles, TerrainTileSide::After).unwrap();
        assert_eq!(data.height_cache, edited.height_cache);
    }

    #[test]
    fn a_stroke_that_changes_nothing_records_nothing() {
        let config = config();
        let data = patterned(&config);
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", None, &config, &data);
        recorder.record_cell(&data, UVec2::new(3, 3));
        assert_eq!(recorder.recorded_tiles(), 1);
        assert!(recorder.finish(None, &data).is_none());
    }

    #[test]
    fn a_second_begin_keeps_the_open_stroke() {
        let config = config();
        let data = patterned(&config);
        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin("Sculpt Terrain", None, &config, &data));
        recorder.record_cell(&data, UVec2::new(3, 3));
        assert!(!recorder.begin("Paint Terrain", None, &config, &data));
        assert_eq!(recorder.recorded_tiles(), 1);
    }

    #[test]
    fn painting_a_terrain_without_materials_undoes_to_no_material_layer() {
        let config = config();
        let mut data = patterned(&config);
        assert!(data.material_cache.is_empty());
        let original = data.clone();
        let rock = TerrainMaterial::Rock.to_u8();

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Paint Terrain", None, &config, &data);
        // World (0, 0) lands in tile (1, 1) and (20, 20) in tile (2, 2). The
        // first write allocates the material layer.
        for (x, z) in [(0.0f32, 0.0f32), (20.0, 20.0)] {
            recorder.record_world_point(&config, &data, x, z);
            paint_material_at_world(&config, &mut data, x, z, rock, 1.0);
        }
        let painted = data.clone();
        let edit = recorder.finish(None, &data).unwrap();
        assert_eq!(edit.tiles.len(), 2);
        assert!(edit.tiles.iter().all(|t| t.heights_before.is_empty() && t.heights_after.is_empty()), "painting leaves heights alone");
        assert!(edit.tiles.iter().any(|t| t.material_before.is_empty() && !t.material_after.is_empty()));
        assert!(edit.tiles.iter().any(TerrainTileDelta::changes_material_layout));

        let mut undone = painted.clone();
        let applied = apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert!(applied.material_layout_changed);
        assert!(undone.material_cache.is_empty(), "undo returns the terrain to having no material layer");
        assert_eq!(undone.height_cache, original.height_cache);

        let mut redone = undone.clone();
        let applied = apply_terrain_tiles(&config, &mut redone, &edit.tiles, TerrainTileSide::After).unwrap();
        assert!(applied.material_layout_changed);
        assert_eq!(redone.material_cache, painted.material_cache);
    }

    #[test]
    fn a_paint_stroke_undoes_through_its_material_tiles() {
        let config = config();
        let mut data = patterned(&config);
        // A material layer that already exists, every cell different, so a
        // restore that puts a cell in the wrong place shows up.
        data.material_cache = (0..data.height_cache.len())
            .map(|i| canonical_material_cell((i % 23) as u8, ((i / 23) % 23) as u8, (i % 120) as u8))
            .collect();
        data.material_dirty = false;
        let original = data.clone();
        let sand = TerrainMaterial::Sand.to_u8();

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Paint Terrain", None, &config, &data);
        // A partial-strength dab either side of the border between tile
        // columns 0 and 1, twice, so the second dab writes into tiles that
        // already hold a snapshot.
        for _ in 0..2 {
            for x in [7.2f32, 8.9] {
                let world_x = x * config.chunk_size / config.chunk_resolution as f32 - config.chunk_size;
                recorder.record_world_point(&config, &data, world_x, 1.0);
                paint_material_at_world(&config, &mut data, world_x, 1.0, sand, 0.4);
            }
        }
        assert!(data.material_dirty, "the paint flags the GPU copy stale");
        let painted = data.clone();
        let edit = recorder.finish(None, &data).expect("the stroke painted two tiles");
        assert_eq!(edit.tiles.len(), 2);
        for delta in &edit.tiles {
            assert!(delta.heights_before.is_empty() && delta.heights_after.is_empty(), "a paint stroke carries no heights");
            let rect = tile_rect(delta.tile, 8, 24, 24).unwrap();
            assert_eq!(delta.material_before, snapshot_tile(&original, rect).material, "tile {:?} before", delta.tile);
            assert_eq!(delta.material_after, snapshot_tile(&painted, rect).material, "tile {:?} after", delta.tile);
            assert!(delta.touches_materials() && !delta.changes_material_layout());
            assert_eq!(delta.byte_len(), 2 * 64 * 4, "two 8 x 8 tiles of four-byte cells");
        }
        let changed: Vec<usize> = (0..data.material_cache.len())
            .filter(|&i| data.material_cache[i] != original.material_cache[i])
            .collect();
        assert!(!changed.is_empty() && changed.iter().all(|&i| {
            let cell = data.material_cache[i];
            cell[0] == sand || cell[1] == sand
        }));

        let mut undone = painted.clone();
        undone.material_dirty = false;
        let applied = apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert!(!applied.material_layout_changed);
        assert!(undone.material_dirty);
        assert_eq!(undone.material_cache, original.material_cache);
        assert_eq!(undone.height_cache, original.height_cache);
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::After).unwrap();
        assert_eq!(undone.material_cache, painted.material_cache);

        // The undo stack serializes its entries.
        let json = serde_json::to_string(&edit.tiles).unwrap();
        let back: Vec<TerrainTileDelta> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, edit.tiles);
    }

    #[test]
    fn material_tiles_of_the_wrong_size_are_refused_untouched() {
        let config = config();
        let mut data = patterned(&config);
        data.material_cache = vec![material_cell(TerrainMaterial::Grass.to_u8()); data.height_cache.len()];
        let delta = TerrainTileDelta {
            tile: UVec2::new(1, 1),
            resolution: 8,
            cache_width: 24,
            cache_height: 24,
            heights_before: Vec::new(),
            heights_after: Vec::new(),
            material_before: vec![material_cell(TerrainMaterial::Rock.to_u8()); 63],
            material_after: vec![material_cell(TerrainMaterial::Rock.to_u8()); 64],
        };
        let untouched = data.material_cache.clone();
        assert!(apply_terrain_tiles(&config, &mut data, &[delta], TerrainTileSide::Before).is_err());
        assert_eq!(data.material_cache, untouched);
    }

    #[test]
    fn a_replaced_terrain_is_refused_untouched() {
        let config = config();
        let mut data = patterned(&config);
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", None, &config, &data);
        recorder.record_cell(&data, UVec2::new(1, 1));
        data.height_cache[25] = 9.0;
        let edit = recorder.finish(None, &data).unwrap();

        let other_config = TerrainConfig { chunks_x: 2, ..config.clone() };
        let mut other = patterned(&other_config);
        let untouched = other.height_cache.clone();
        let err = apply_terrain_tiles(&other_config, &mut other, &edit.tiles, TerrainTileSide::Before).unwrap_err();
        assert!(err.contains("changed layout"), "{err}");
        assert_eq!(other.height_cache, untouched);

        // Same 24 x 24 raster cut into one 24-cell chunk: the tiles would
        // land on the wrong cells, so this is refused too.
        let regridded = TerrainConfig { chunk_resolution: 24, chunks_x: 0, chunks_z: 0, ..config.clone() };
        let mut same_size = patterned(&regridded);
        assert_eq!(same_size.cache_width, data.cache_width);
        let untouched = same_size.height_cache.clone();
        assert!(apply_terrain_tiles(&regridded, &mut same_size, &edit.tiles, TerrainTileSide::Before).is_err());
        assert_eq!(same_size.height_cache, untouched);
    }

    #[test]
    fn a_terrain_replaced_mid_stroke_drops_the_stroke() {
        let config = config();
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();

        let mut data = patterned(&config);
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", Some(first), &config, &data);
        recorder.record_cell(&data, UVec2::new(1, 1));
        data.height_cache[25] = 9.0;
        assert!(recorder.finish(Some(second), &data).is_none());
        assert!(!recorder.is_recording());

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", Some(first), &config, &data);
        recorder.record_cell(&data, UVec2::new(1, 1));
        let resized = patterned(&TerrainConfig { chunks_x: 2, ..config.clone() });
        assert!(recorder.finish(Some(first), &resized).is_none());
    }

    #[test]
    fn world_rect_covers_every_cell_a_write_inside_it_touches() {
        let config = config();
        let mut data = patterned(&config);
        let original = data.clone();
        let (min, max) = (Vec2::new(-3.0, 5.5), Vec2::new(9.25, 17.0));

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Part to Terrain", None, &config, &data);
        recorder.record_world_rect(&config, &data, min, max);

        // Write densely over the rectangle, edges included, recording
        // nothing per write: the up-front rectangle has to cover it all.
        let mut x = min.x;
        while x <= max.x {
            let mut z = min.y;
            while z <= max.y {
                set_height_at_world(&config, &mut data, x, z, 30.0, 1.0);
                z += 0.37;
            }
            set_height_at_world(&config, &mut data, x, max.y, 30.0, 1.0);
            x += 0.37;
        }
        set_height_at_world(&config, &mut data, max.x, max.y, 30.0, 1.0);

        let edit = recorder.finish(None, &data).unwrap();
        apply_terrain_tiles(&config, &mut data, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert_eq!(data.height_cache, original.height_cache);
    }

    #[test]
    fn diff_holds_only_the_tiles_that_differ() {
        let config = config();
        let before = patterned(&config);
        let mut after = before.clone();
        after.height_cache[20] = 9.0; // row 0, column 20: tile (2, 0)

        let deltas = diff_terrain_tiles(&config, &before, &after);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].tile, UVec2::new(2, 0));

        let mut restored = after.clone();
        apply_terrain_tiles(&config, &mut restored, &deltas, TerrainTileSide::Before).unwrap();
        assert_eq!(restored.height_cache, before.height_cache);

        let resized = patterned(&TerrainConfig { chunks_x: 2, ..config.clone() });
        assert!(diff_terrain_tiles(&config, &before, &resized).is_empty());
    }

    #[test]
    fn mark_dirty_marks_the_chunk_and_its_ring_inside_the_grid() {
        let config = config();
        let applied = AppliedTerrainTiles { chunks: vec![IVec2::new(1, 0)], material_layout_changed: false };
        let mut dirty = TerrainDirtyChunks::default();
        applied.mark_dirty(&config, &mut dirty);
        // Chunk 1 is the last column (the grid runs -1..=1), so only
        // columns 0 and 1 of the ring exist.
        assert_eq!(dirty.remesh.len(), 6);
        assert!(dirty.remesh.iter().all(|c| (0..=1).contains(&c.x) && (-1..=1).contains(&c.y)));
        assert_eq!(dirty.focus, Some(Vec2::new(24.0, 8.0)));

        let relayout = AppliedTerrainTiles { chunks: vec![IVec2::new(1, 0)], material_layout_changed: true };
        let mut dirty = TerrainDirtyChunks::default();
        relayout.mark_dirty(&config, &mut dirty);
        assert_eq!(dirty.remesh.len(), 9);
    }

    #[test]
    fn stroke_labels_follow_the_brush_mode() {
        assert_eq!(terrain_stroke_label(BrushMode::Raise), "Sculpt Terrain");
        assert_eq!(terrain_stroke_label(BrushMode::Smooth), "Sculpt Terrain");
        assert_eq!(terrain_stroke_label(BrushMode::PaintTexture), "Paint Terrain");
        assert_eq!(terrain_stroke_label(BrushMode::VoxelAdd), "Add Terrain");
        assert_eq!(terrain_stroke_label(BrushMode::VoxelRemove), "Subtract Terrain");
        assert_eq!(terrain_stroke_label(BrushMode::VoxelSmooth), "Smooth Terrain");
    }

    // ------------------------------------------------------------------------
    // Volume bricks
    // ------------------------------------------------------------------------

    use crate::terrain::volume::{apply_shape, CsgOp, CsgShape};
    use crate::terrain::TerrainMaterial;

    /// Every brick of `volume`, in a fixed order, for comparing volumes.
    fn brick_list(volume: &TerrainVolume) -> Vec<(IVec3, VolumeBrick)> {
        let mut bricks: Vec<(IVec3, VolumeBrick)> = volume.bricks().map(|(coord, brick)| (coord, brick.clone())).collect();
        bricks.sort_by_key(|(coord, _)| (coord.x, coord.y, coord.z));
        bricks
    }

    /// Apply `shape` with `op`, recording its bricks first, the way the 3D
    /// brush does.
    fn recorded_dab(
        recorder: &mut TerrainEditRecorder,
        config: &TerrainConfig,
        volume: &mut TerrainVolume,
        shape: CsgShape,
        op: CsgOp,
    ) -> VolumeEdit {
        let (lo, hi) = shape.edit_bounds(config);
        recorder.record_volume_aabb(config, volume, lo, hi);
        apply_shape(config, volume, shape, op, None)
    }

    #[test]
    fn a_volume_stroke_snapshots_each_brick_before_its_first_write() {
        // The config's 2 m lattice cuts the world into 32 m bricks.
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin("Add Terrain", None, &config, &data));

        // The second dab writes into bricks the first one created; they must
        // keep their pre-stroke "no brick" snapshot, not take a copy of the
        // first dab's writes.
        for center in [Vec3::new(0.0, 5.0, 0.0), Vec3::new(4.0, 5.0, 0.0)] {
            let edit = recorded_dab(&mut recorder, &config, &mut volume, CsgShape::Sphere { center, radius: 3.0 }, CsgOp::Add);
            assert!(!edit.is_empty());
        }
        let edited = brick_list(&volume);
        assert!(!edited.is_empty());
        assert!(recorder.recorded_bricks() >= edited.len());

        let edit = recorder.finish_with_volume(None, &data, &volume).expect("the stroke added bricks");
        assert!(!recorder.is_recording());
        assert_eq!(edit.label, "Add Terrain");
        assert!(edit.tiles.is_empty(), "a 3D stroke leaves the raster alone");
        // Only the bricks that exist now changed; coordinates the dabs only
        // might have reached are not kept.
        let mut coords: Vec<IVec3> = edit.bricks.iter().map(|delta| delta.coord).collect();
        coords.sort_by_key(|c| (c.x, c.y, c.z));
        assert_eq!(coords, edited.iter().map(|(coord, _)| *coord).collect::<Vec<_>>());
        for delta in &edit.bricks {
            assert!(delta.before.is_none(), "brick {} existed before the stroke", delta.coord);
            assert_eq!(delta.after.as_ref(), volume.brick(delta.coord));
            assert_eq!(delta.cell_size, lattice_cell_size(&config));
            assert_eq!(delta.byte_len(), VBK_PAYLOAD_LEN);
        }

        // Undo empties the volume again; redo brings every brick back exactly.
        let mut undone = volume.clone();
        let changed = apply_terrain_bricks(&config, &mut undone, &edit.bricks, TerrainTileSide::Before).unwrap();
        assert!(undone.is_empty());
        assert_eq!(changed.bricks, coords);
        let mut redone = undone.clone();
        apply_terrain_bricks(&config, &mut redone, &edit.bricks, TerrainTileSide::After).unwrap();
        assert_eq!(brick_list(&redone), edited);
    }

    #[test]
    fn a_carve_into_existing_bricks_undoes_to_their_old_contents() {
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        let block = CsgShape::AxisBox { center: Vec3::new(0.0, 4.0, 0.0), half_extents: Vec3::splat(6.0) };
        apply_shape(&config, &mut volume, block, CsgOp::Add, Some(TerrainMaterial::Sand));
        let original = brick_list(&volume);

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Subtract Terrain", None, &config, &data);
        let tunnel = CsgShape::Cylinder { center: Vec3::new(2.0, 4.0, 2.0), radius: 3.0, half_height: 5.0 };
        recorded_dab(&mut recorder, &config, &mut volume, tunnel, CsgOp::Carve);
        // Recording the same box again after the write must not replace the
        // snapshots with the carved bricks.
        let (lo, hi) = tunnel.edit_bounds(&config);
        recorder.record_volume_aabb(&config, &volume, lo, hi);
        let carved = brick_list(&volume);
        assert_ne!(carved, original);

        let edit = recorder.finish_with_volume(None, &data, &volume).expect("the carve changed bricks");
        assert!(edit.bricks.iter().any(|delta| delta.before.is_some()), "the carve changed the block's bricks");
        for delta in &edit.bricks {
            let was = original.iter().find(|(coord, _)| *coord == delta.coord).map(|(_, brick)| brick);
            assert_eq!(delta.before.as_ref(), was, "brick {} before", delta.coord);
        }

        let mut undone = volume.clone();
        apply_terrain_bricks(&config, &mut undone, &edit.bricks, TerrainTileSide::Before).unwrap();
        assert_eq!(brick_list(&undone), original);
        apply_terrain_bricks(&config, &mut undone, &edit.bricks, TerrainTileSide::After).unwrap();
        assert_eq!(brick_list(&undone), carved);
    }

    #[test]
    fn bricks_a_stroke_left_unchanged_are_not_kept() {
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        apply_shape(&config, &mut volume, CsgShape::Sphere { center: Vec3::ZERO, radius: 4.0 }, CsgOp::Add, None);

        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Smooth Terrain", None, &config, &data);
        recorder.record_volume_aabb(&config, &volume, Vec3::splat(-40.0), Vec3::splat(40.0));
        assert!(recorder.recorded_bricks() >= volume.brick_count());
        assert!(recorder.finish_with_volume(None, &data, &volume).is_none(), "nothing changed, so nothing to undo");
    }

    #[test]
    fn bricks_are_only_recorded_inside_an_open_edit() {
        let config = config();
        let volume = TerrainVolume::new();
        let mut recorder = TerrainEditRecorder::default();
        recorder.record_volume_aabb(&config, &volume, Vec3::splat(-5.0), Vec3::splat(5.0));
        recorder.record_bricks(&volume, &[IVec3::ZERO]);
        assert_eq!(recorder.recorded_bricks(), 0);
        assert!(!recorder.is_recording());
    }

    #[test]
    fn closing_a_volume_stroke_without_its_volume_keeps_nothing() {
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Add Terrain", None, &config, &data);
        recorded_dab(&mut recorder, &config, &mut volume, CsgShape::Sphere { center: Vec3::ZERO, radius: 3.0 }, CsgOp::Add);
        // The raster-only close cannot take the bricks' after side.
        assert!(recorder.finish(None, &data).is_none());
        assert!(!recorder.is_recording());
    }

    #[test]
    fn brick_undo_refuses_a_terrain_whose_lattice_changed() {
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Add Terrain", None, &config, &data);
        recorded_dab(&mut recorder, &config, &mut volume, CsgShape::Sphere { center: Vec3::ZERO, radius: 3.0 }, CsgOp::Add);
        let edit = recorder.finish_with_volume(None, &data, &volume).unwrap();

        // Twice the resolution halves the cell: the bricks would shrink.
        let regridded = TerrainConfig { chunk_resolution: 16, ..config.clone() };
        let untouched = brick_list(&volume);
        let err = apply_terrain_bricks(&regridded, &mut volume, &edit.bricks, TerrainTileSide::Before).unwrap_err();
        assert!(err.contains("lattice changed"), "{err}");
        assert!(check_terrain_bricks(&regridded, &edit.bricks).is_err());
        assert!(check_terrain_bricks(&config, &edit.bricks).is_ok());
        assert_eq!(brick_list(&volume), untouched);
    }

    #[test]
    fn brick_deltas_survive_serialization() {
        let config = config();
        let data = patterned(&config);
        let mut volume = TerrainVolume::new();
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Add Terrain", None, &config, &data);
        recorded_dab(&mut recorder, &config, &mut volume, CsgShape::Sphere { center: Vec3::ZERO, radius: 3.0 }, CsgOp::Add);
        let edit = recorder.finish_with_volume(None, &data, &volume).unwrap();

        let json = serde_json::to_string(&edit.bricks).unwrap();
        let back: Vec<TerrainBrickDelta> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, edit.bricks);

        let (_, brick) = brick_list(&volume).into_iter().next().unwrap();
        use crate::terrain::volume::{pack_brick_payload, unpack_brick_payload};
        assert_eq!(unpack_brick_payload(&pack_brick_payload(&brick)).unwrap(), brick);
        // A corrupt size prefix is refused before anything is allocated for it.
        assert!(unpack_brick_payload(&[255, 255, 255, 127, 0]).is_err());
        assert!(unpack_brick_payload(&[]).is_err());
    }

    fn world_heights(config: &TerrainConfig, data: &TerrainData) -> Vec<f32> {
        data.height_cache.iter().map(|&n| config.world_height(n)).collect()
    }

    fn assert_same_world(got: &[f32], want: &[f32], what: &str) {
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert!((g - w).abs() < 1e-3, "{what}: cell {i} is at {g}, expected {w}");
        }
    }

    #[test]
    fn a_band_move_keeps_undo_entries_and_an_open_stroke_true() {
        use crate::terrain::widen_height_band_to_fit;
        let mut config = config();
        let mut data = patterned(&config);
        let w = data.cache_width as usize;
        let original = world_heights(&config, &data);

        // A finished stroke that left a height above the band ceiling.
        let mut recorder = TerrainEditRecorder::default();
        recorder.begin("Sculpt Terrain", None, &config, &data);
        recorder.record_cell(&data, UVec2::new(7, 10));
        data.height_cache[10 * w + 7] = 1.5;
        let mut finished = recorder.finish(None, &data).unwrap();
        let sculpted = world_heights(&config, &data);

        // A second stroke is still open, in another tile, when Save widens
        // the band and rebases every copy of the raster.
        recorder.begin("Sculpt Terrain", None, &config, &data);
        recorder.record_cell(&data, UVec2::new(20, 3));
        let (from, to) = widen_height_band_to_fit(&mut config, &mut data).expect("1.5 is outside the band");
        rebase_tile_heights(&mut finished.tiles, from, to);
        recorder.rebase_heights(None, from, to);
        assert_same_world(&world_heights(&config, &data), &sculpted, "widening");
        data.height_cache[3 * w + 20] = config.normalized_height(70.0);
        let open = recorder.finish(None, &data).unwrap();

        apply_terrain_tiles(&config, &mut data, &open.tiles, TerrainTileSide::Before).unwrap();
        assert_same_world(&world_heights(&config, &data), &sculpted, "undo of the open stroke");
        apply_terrain_tiles(&config, &mut data, &finished.tiles, TerrainTileSide::Before).unwrap();
        assert_same_world(&world_heights(&config, &data), &original, "undo of the finished stroke");
        apply_terrain_tiles(&config, &mut data, &finished.tiles, TerrainTileSide::After).unwrap();
        assert_same_world(&world_heights(&config, &data), &sculpted, "redo of the finished stroke");
    }
}
