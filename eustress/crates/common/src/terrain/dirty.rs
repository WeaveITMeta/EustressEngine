//! Shared dirty-chunk pipeline for terrain edits.
//!
//! Every writer of `TerrainData::height_cache` / `material_cache` (brush
//! strokes, Part to Terrain, undo) and of the `TerrainVolume`
//! (CSG edits, see [`TerrainDirtyChunks::mark_volume_edit`]) marks what it
//! touched in [`TerrainDirtyChunks`] rather than remeshing on the spot. One
//! system, [`apply_terrain_dirty_chunks`], then remeshes those chunks at their
//! current LOD within a per-frame budget, and rebuilds their colliders only
//! once the edits go quiet: rebuilding a heightfield on every brush dab would
//! churn the physics broadphase while nothing needs to stand on ground still
//! moving.
//!
//! The system is not gated on `TerrainMode`, because undo and layer edits
//! happen outside the terrain editor.
//!
//! A root with terrain layers draws its [`TerrainBaked`] rather than its
//! base (see `layers`). Every raster mark also queues its region for a
//! re-bake, and [`apply_terrain_dirty_chunks`] re-bakes those regions before
//! it remeshes anything, so the writers stay unaware of layers. Volume marks
//! queue nothing: a volume edit leaves the raster, and so the bake, alone.
//!
//! State derived from the surface beyond meshes and colliders (the scatter
//! of `scatter`) needs to know which chunks changed too, but the remesh set
//! is drained by this pipeline and never lists a chunk that is not spawned.
//! So every mark that changes the ground also stamps the chunks it covers
//! with a rising sequence number, [`TerrainDirtyChunks::surface_seq`]; a
//! reader keeps the number it last caught up to and asks
//! [`TerrainDirtyChunks::surface_changes_since`] for the rest. Any number of
//! readers can follow it, each at its own pace, and nothing is consumed. A
//! reader of the heights alone (the water's height texture) follows
//! [`TerrainDirtyChunks::height_seq`] instead, which paint strokes
//! ([`TerrainDirtyChunks::mark_world_rect_materials`]) and volume edits leave.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::platform::time::Instant;
use bevy::prelude::*;

use super::layers::{surface_data, TerrainBaked, TerrainRebake};
use super::volume::{brick_world_bounds, lattice_cell_size, TerrainVolume, VolumeEdit};
use super::{
    Chunk, TerrainChunkCollider, TerrainConfig, TerrainData, TerrainRoot, chunk_collider_cost,
    chunk_mesh_cost, generate_chunk_render_mesh,
};

/// Seconds without a new mark before stale colliders are rebuilt.
const COLLIDER_QUIET_SECS: f64 = 0.15;
/// Remesh at most this many heightfield chunks' worth per frame (a
/// marching-cubes chunk counts the cubes it marches per lattice cell, see
/// `chunk_mesh_cost`)...
const MAX_REMESH_PER_FRAME: usize = 16;
/// ...and stop early once this much time is spent. The chunk nearest the
/// edit always gets remeshed, even past the budget, so a stroke never stalls.
const REMESH_TIME_BUDGET: Duration = Duration::from_millis(4);
/// Collider rebuilds per frame once the edits are quiet, in heightfield
/// colliders (a trimesh counts the cubes its march visits per lattice cell,
/// see `chunk_collider_cost`). The first rebuild of a frame always runs.
const MAX_RECOLLIDE_PER_FRAME: usize = 4;
/// Queued re-bake rectangles past this many are merged into the one
/// rectangle around them, so the queue stays bounded however long a host
/// goes without applying it.
const MAX_REBAKE_RECTS: usize = 64;

/// Chunks whose render mesh or collider no longer matches `TerrainData`,
/// and the raster regions whose layer bake may be stale.
#[derive(Resource, Debug, Default)]
pub struct TerrainDirtyChunks {
    /// Chunks whose render mesh is stale.
    pub remesh: HashSet<IVec2>,
    /// Chunks whose collider is stale.
    pub recollide: HashSet<IVec2>,
    /// `Time::elapsed_secs_f64` of the latest frame that marked anything.
    /// Kept by [`apply_terrain_dirty_chunks`], since the writers that mark
    /// chunks have no clock of their own.
    pub last_mark_secs: f64,
    /// World XZ centre of the latest mark. Remeshing starts from the chunk
    /// nearest it, so the ground under the brush updates first.
    pub focus: Option<Vec2>,
    /// Set by every mark, consumed by the apply system to stamp
    /// `last_mark_secs`.
    marked_since_apply: bool,
    /// Chunks marked through [`Self::mark`], whose raster cells the layer
    /// bake must recompute.
    rebake_chunks: HashSet<IVec2>,
    /// Rectangles marked through [`Self::mark_world_rect`], exactly as
    /// marked, without the ring: the bake widens a region by whatever it
    /// reads around each cell itself.
    rebake_rects: Vec<(Vec2, Vec2)>,
    /// [`Self::mark_all`] was called: bake every cell.
    rebake_all: bool,
    /// Rises by one with every mark that changes the surface (heights,
    /// materials or the volume) of some chunks, or of all of them.
    surface_seq: u64,
    /// Per chunk, the `surface_seq` of the latest mark that changed its
    /// surface. Bounded by the chunk grid.
    surface_marks: HashMap<IVec2, u64>,
    /// The `surface_seq` of the latest mark that changed every chunk.
    surface_all_seq: u64,
    /// Rises with every mark that can move `height_cache` (or a bake's).
    /// Paint and volume marks leave it, so the water's height texture is not
    /// re-uploaded for them.
    height_seq: u64,
}

/// What changed on the terrain surface since a reader last looked (see
/// [`TerrainDirtyChunks::surface_changes_since`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceChanges {
    /// Every chunk changed.
    pub all: bool,
    /// The chunks that changed, in no particular order; empty when `all`.
    pub chunks: Vec<IVec2>,
}

impl TerrainDirtyChunks {
    /// Mark one chunk's mesh and collider stale.
    pub fn mark(&mut self, chunk: IVec2) {
        self.remesh.insert(chunk);
        self.recollide.insert(chunk);
        self.rebake_chunks.insert(chunk);
        self.note_surface_chunks(chunk, chunk);
        self.height_seq += 1;
        self.marked_since_apply = true;
    }

    /// The surface change counter: rises with every mark that changes the
    /// ground (see the module docs).
    pub fn surface_seq(&self) -> u64 {
        self.surface_seq
    }

    /// The height change counter: rises with every mark that can move the
    /// raster's heights, and not for paint or volume marks.
    pub fn height_seq(&self) -> u64 {
        self.height_seq
    }

    /// What the marks made since [`Self::surface_seq`] read `seen` changed:
    /// every chunk, or the chunks stamped after `seen`. A reader then keeps
    /// the current [`Self::surface_seq`] as its next `seen`.
    pub fn surface_changes_since(&self, seen: u64) -> SurfaceChanges {
        if self.surface_seq <= seen {
            return SurfaceChanges::default();
        }
        if self.surface_all_seq > seen {
            return SurfaceChanges { all: true, chunks: Vec::new() };
        }
        let chunks = self.surface_marks.iter().filter(|(_, seq)| **seq > seen).map(|(chunk, _)| *chunk).collect();
        SurfaceChanges { all: false, chunks }
    }

    /// Stamp the chunks `lo..=hi` (inclusive chunk coordinates) with a new
    /// surface sequence number.
    fn note_surface_chunks(&mut self, lo: IVec2, hi: IVec2) {
        self.surface_seq += 1;
        for x in lo.x..=hi.x {
            for z in lo.y..=hi.y {
                self.surface_marks.insert(IVec2::new(x, z), self.surface_seq);
            }
        }
    }

    /// Stamp every chunk with a new surface sequence number.
    fn note_surface_all(&mut self) {
        self.surface_seq += 1;
        self.surface_all_seq = self.surface_seq;
    }

    /// Stamp the chunks whose surface a change inside world rectangle
    /// `lo..hi` (ordered, finite) can move: those it overlaps once grown by
    /// one raster cell, since a height sample blends the cells around it.
    /// Tighter than the remesh ring, which also covers chunks whose shared
    /// border normals the change reaches.
    fn note_surface_rect(&mut self, config: &TerrainConfig, lo: Vec2, hi: Vec2) {
        let size = config.chunk_size.max(1e-3);
        let cell = size / config.chunk_resolution.max(1) as f32;
        let extent = IVec2::new(config.chunks_x as i32, config.chunks_z as i32);
        let first = IVec2::new(((lo.x - cell) / size).floor() as i32, ((lo.y - cell) / size).floor() as i32).max(-extent);
        let last = IVec2::new(((hi.x + cell) / size).floor() as i32, ((hi.y + cell) / size).floor() as i32).min(extent);
        if first.x <= last.x && first.y <= last.y {
            self.note_surface_chunks(first, last);
        }
    }

    /// Push the collider quiet window forward without marking any chunk.
    /// A held 3D brush dabs less often than the quiet window at low
    /// strength, so without this the stale trimeshes would be rebuilt
    /// between every pair of dabs of one stroke.
    pub fn hold_colliders(&mut self) {
        self.marked_since_apply = true;
    }

    /// Mark every chunk the world XZ rectangle overlaps, plus one ring of
    /// neighbours: border vertices are shared between chunks, and a chunk's
    /// normals and bilinear samples read the ground just across its border,
    /// so an edit near a border changes the chunk next door too. Chunks
    /// outside the terrain's grid are skipped; nothing outside it has a
    /// raster to edit.
    pub fn mark_world_rect(&mut self, config: &TerrainConfig, min_xz: Vec2, max_xz: Vec2) {
        let Some((lo, hi)) = self.mark_rect_chunks(config, min_xz, max_xz) else {
            return;
        };
        self.height_seq += 1;
        self.queue_rebake_rect(lo, hi);
        self.focus = Some((lo + hi) * 0.5);
        self.marked_since_apply = true;
    }

    /// [`Self::mark_world_rect`] for a write that changed only the material
    /// layer (a paint stroke): the same chunks, stamps and re-bake, but the
    /// heights stay, so [`Self::height_seq`] does too.
    pub fn mark_world_rect_materials(&mut self, config: &TerrainConfig, min_xz: Vec2, max_xz: Vec2) {
        let Some((lo, hi)) = self.mark_rect_chunks(config, min_xz, max_xz) else {
            return;
        };
        self.queue_rebake_rect(lo, hi);
        self.focus = Some((lo + hi) * 0.5);
        self.marked_since_apply = true;
    }

    /// Mark the chunks a world XZ rectangle overlaps, plus their ring, and
    /// stamp the chunks whose surface it changed; nothing else: no focus, no
    /// mark flag, no re-bake. Returns the rectangle with its corners ordered,
    /// or `None` when it is not finite.
    fn mark_rect_chunks(&mut self, config: &TerrainConfig, min_xz: Vec2, max_xz: Vec2) -> Option<(Vec2, Vec2)> {
        if !(min_xz.is_finite() && max_xz.is_finite()) {
            return None;
        }
        let lo = min_xz.min(max_xz);
        let hi = min_xz.max(max_xz);
        self.note_surface_rect(config, lo, hi);
        let size = config.chunk_size.max(1e-3);
        let extent_x = config.chunks_x as i32;
        let extent_z = config.chunks_z as i32;
        let x0 = ((lo.x / size).floor() as i32).saturating_sub(1).max(-extent_x);
        let x1 = ((hi.x / size).floor() as i32).saturating_add(1).min(extent_x);
        let z0 = ((lo.y / size).floor() as i32).saturating_sub(1).max(-extent_z);
        let z1 = ((hi.y / size).floor() as i32).saturating_add(1).min(extent_z);
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.remesh.insert(IVec2::new(x, z));
                self.recollide.insert(IVec2::new(x, z));
            }
        }
        Some((lo, hi))
    }

    /// Queue a world rectangle for the layer bake.
    fn queue_rebake_rect(&mut self, lo: Vec2, hi: Vec2) {
        if self.rebake_all {
            return;
        }
        self.rebake_rects.push((lo, hi));
        if self.rebake_rects.len() > MAX_REBAKE_RECTS {
            let merged = self.rebake_rects.drain(..).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
            self.rebake_rects.extend(merged);
        }
    }

    /// Mark every chunk of the terrain's grid, for writes that change every
    /// chunk (an undo that allocates or drops the material layer).
    pub fn mark_all(&mut self, config: &TerrainConfig) {
        self.mark_grid(config);
        self.rebake_all = true;
        self.rebake_rects.clear();
        self.rebake_chunks.clear();
        self.focus = None;
        self.marked_since_apply = true;
    }

    /// Every chunk's mesh and collider stale, and every chunk's surface
    /// changed; nothing else.
    fn mark_grid(&mut self, config: &TerrainConfig) {
        self.note_surface_all();
        self.height_seq += 1;
        let extent_x = config.chunks_x as i32;
        let extent_z = config.chunks_z as i32;
        for x in -extent_x..=extent_x {
            for z in -extent_z..=extent_z {
                self.remesh.insert(IVec2::new(x, z));
                self.recollide.insert(IVec2::new(x, z));
            }
        }
    }

    /// Mark what a layer bake changed beyond the regions it was asked for
    /// (see `TerrainBaked::rebake`): the chunks under `min_xz..max_xz` and
    /// their ring, without queueing another bake. Keeps the focus of the edit
    /// that caused it, if there is one.
    fn mark_rebaked_rect(&mut self, config: &TerrainConfig, min_xz: Vec2, max_xz: Vec2) {
        if let Some((lo, hi)) = self.mark_rect_chunks(config, min_xz, max_xz) {
            self.height_seq += 1;
            if self.focus.is_none() {
                self.focus = Some((lo + hi) * 0.5);
            }
        }
    }

    /// Whether marks have queued raster regions for the layer bake.
    pub fn has_pending_rebake(&self) -> bool {
        self.rebake_all || !self.rebake_rects.is_empty() || !self.rebake_chunks.is_empty()
    }

    /// Take the raster regions queued for the layer bake since the last
    /// take, as world rectangles (a marked chunk as its footprint).
    pub fn take_rebake(&mut self, config: &TerrainConfig) -> TerrainRebake {
        let size = config.chunk_size;
        let mut rebake = TerrainRebake { whole: std::mem::take(&mut self.rebake_all), rects: std::mem::take(&mut self.rebake_rects) };
        for chunk in self.rebake_chunks.drain() {
            let lo = chunk.as_vec2() * size;
            rebake.rects.push((lo, lo + Vec2::splat(size)));
        }
        if rebake.whole {
            rebake.rects.clear();
        }
        rebake
    }

    /// Drop every queued re-bake region.
    fn clear_rebake(&mut self) {
        self.rebake_all = false;
        self.rebake_rects.clear();
        self.rebake_chunks.clear();
    }

    /// Mark every chunk's mesh stale but none of their colliders, for a
    /// change that only recolours the ground (the material slot palette).
    pub fn mark_all_meshes(&mut self, config: &TerrainConfig) {
        let extent_x = config.chunks_x as i32;
        let extent_z = config.chunks_z as i32;
        for x in -extent_x..=extent_x {
            for z in -extent_z..=extent_z {
                self.remesh.insert(IVec2::new(x, z));
            }
        }
    }

    /// Mark the chunks a volume edit changed: those its changed box overlaps
    /// plus one ring (see [`Self::mark_world_rect`]), and every chunk whose
    /// columns a brick it created, changed or removed reaches, since a brick
    /// appearing or going switches those chunks between heightfield and
    /// marching-cubes meshes and colliders. Remeshing starts under the edit.
    /// Queues no layer bake: the raster did not change.
    pub fn mark_volume_edit(&mut self, config: &TerrainConfig, edit: &VolumeEdit) {
        if edit.is_empty() {
            return;
        }
        let cell = lattice_cell_size(config);
        let bricks = edit.bricks.iter().map(|coord| {
            let (lo, hi) = brick_world_bounds(*coord, cell);
            (Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z))
        });
        // The edit's own box last, so it sets the focus.
        let edit_box = (Vec2::new(edit.min.x, edit.min.z), Vec2::new(edit.max.x, edit.max.z));
        for (min_xz, max_xz) in bricks.chain(std::iter::once(edit_box)) {
            if let Some((lo, hi)) = self.mark_rect_chunks(config, min_xz, max_xz) {
                self.focus = Some((lo + hi) * 0.5);
                self.marked_since_apply = true;
            }
        }
    }

    /// Nothing is waiting to be rebuilt.
    pub fn is_empty(&self) -> bool {
        self.remesh.is_empty() && self.recollide.is_empty()
    }
}

/// Squared distance from a chunk's centre to `focus`, the remesh priority.
fn focus_distance_sq(chunk: IVec2, config: &TerrainConfig, focus: Vec2) -> f32 {
    let size = config.chunk_size;
    let centre = Vec2::new((chunk.x as f32 + 0.5) * size, (chunk.y as f32 + 0.5) * size);
    centre.distance_squared(focus)
}

/// Rebuild the meshes and colliders [`TerrainDirtyChunks`] lists.
///
/// Meshes are rebuilt at each chunk's current LOD straight away, nearest the
/// latest edit first, within [`MAX_REMESH_PER_FRAME`] / [`REMESH_TIME_BUDGET`].
/// Colliders are rebuilt at LOD 0 once [`COLLIDER_QUIET_SECS`] pass without a
/// new mark. Marks for chunks that are not spawned (culled, not streamed in
/// yet) are dropped: a chunk spawned later meshes and collides from the
/// current data anyway. Both go through the volume-aware builders, so a
/// chunk holding volumetric edits gets its marching-cubes mesh and trimesh.
///
/// First, on a root with terrain layers, the regions the marks queued are
/// re-baked ([`TerrainBaked::rebake`]), whether or not their chunks are
/// spawned, since a chunk spawned later meshes from the bake. What the bake
/// changed beyond them (a layer list replaced, a spline profile moved) is
/// marked here. Meshes and colliders then read the bake through
/// [`surface_data`]. A mark made after this system ran is baked next frame,
/// ahead of the remesh it asks for.
pub fn apply_terrain_dirty_chunks(
    mut commands: Commands,
    time: Res<Time>,
    mut dirty: ResMut<TerrainDirtyChunks>,
    mut terrain_query: Query<
        (&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&mut TerrainBaked>),
        With<TerrainRoot>,
    >,
    chunks: Query<(Entity, &Chunk)>,
    colliders: Query<(Entity, &ChildOf), With<TerrainChunkCollider>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    // Read through `Deref` first so an idle frame leaves the resource
    // unchanged. A bake with work of its own (just inserted, or its layer
    // list replaced) runs even when nothing was marked.
    let bake_waiting = terrain_query
        .iter()
        .any(|(_, _, _, baked)| baked.is_some_and(TerrainBaked::wants_bake));
    if dirty.is_empty() && !dirty.marked_since_apply && !dirty.has_pending_rebake() && !bake_waiting {
        return;
    }
    let now = time.elapsed_secs_f64();
    let dirty = &mut *dirty;
    if dirty.marked_since_apply {
        dirty.last_mark_secs = now;
        dirty.marked_since_apply = false;
    }

    let Ok((config, data, volume, mut baked)) = terrain_query.single_mut() else {
        // No terrain (or an ambiguous pair mid-replacement): nothing to
        // rebuild, and a fresh terrain meshes from its own data.
        dirty.remesh.clear();
        dirty.recollide.clear();
        dirty.focus = None;
        dirty.clear_rebake();
        return;
    };
    let volume = volume.unwrap_or(TerrainVolume::empty());

    // Without layers there is no bake, and the queued regions go unused.
    let rebake = dirty.take_rebake(config);
    if let Some(baked) = baked.as_mut() {
        if !rebake.is_empty() || baked.wants_bake() {
            let outcome = baked.rebake(config, data, &rebake);
            if outcome.remesh_all {
                dirty.mark_grid(config);
            }
            for (min_xz, max_xz) in &outcome.remesh {
                dirty.mark_rebaked_rect(config, *min_xz, *max_xz);
            }
            if outcome.remesh_all || !outcome.remesh.is_empty() {
                // The collider quiet window starts over with these marks.
                dirty.last_mark_secs = now;
            }
        }
    }
    let data = surface_data(data, baked.as_deref());

    let spawned: HashMap<IVec2, Entity> = chunks.iter().map(|(entity, chunk)| (chunk.position, entity)).collect();
    dirty.remesh.retain(|pos| spawned.contains_key(pos));
    dirty.recollide.retain(|pos| spawned.contains_key(pos));

    if !dirty.remesh.is_empty() {
        let focus = dirty.focus.unwrap_or(Vec2::ZERO);
        let mut order: Vec<IVec2> = dirty.remesh.iter().copied().collect();
        order.sort_by(|a, b| {
            focus_distance_sq(*a, config, focus)
                .partial_cmp(&focus_distance_sq(*b, config, focus))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let started = Instant::now();
        let mut spent = 0;
        for pos in order {
            if spent >= MAX_REMESH_PER_FRAME || (spent > 0 && started.elapsed() >= REMESH_TIME_BUDGET) {
                break;
            }
            dirty.remesh.remove(&pos);
            let Some(&entity) = spawned.get(&pos) else { continue };
            let Ok((_, chunk)) = chunks.get(entity) else { continue };
            let mesh = generate_chunk_render_mesh(pos, chunk.lod, config, data, volume, &mut meshes);
            commands.entity(entity).try_insert(Mesh3d(mesh));
            spent += chunk_mesh_cost(pos, chunk.lod, config, data, volume);
        }
    }

    #[cfg(feature = "physics")]
    {
        if !dirty.recollide.is_empty() && now - dirty.last_mark_secs >= COLLIDER_QUIET_SECS {
            let child_of_chunk: HashMap<Entity, Entity> = colliders
                .iter()
                .map(|(collider, child_of)| (child_of.parent(), collider))
                .collect();
            let pending: Vec<IVec2> = dirty.recollide.iter().copied().collect();
            let mut spent = 0;
            for pos in pending {
                if spent >= MAX_RECOLLIDE_PER_FRAME {
                    break;
                }
                dirty.recollide.remove(&pos);
                let Some(&chunk_entity) = spawned.get(&pos) else { continue };
                super::refresh_chunk_collider(
                    &mut commands,
                    chunk_entity,
                    pos,
                    child_of_chunk.get(&chunk_entity).copied(),
                    config,
                    data,
                    volume,
                );
                spent += chunk_collider_cost(pos, config, data, volume);
            }
        }
    }
    #[cfg(not(feature = "physics"))]
    {
        // No colliders exist to go stale.
        let _ = (&colliders, COLLIDER_QUIET_SECS, MAX_RECOLLIDE_PER_FRAME, chunk_collider_cost);
        dirty.recollide.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 64.0,
            chunks_x: 3,
            chunks_z: 3,
            ..TerrainConfig::default()
        }
    }

    #[test]
    fn rect_inside_one_chunk_marks_it_and_its_ring() {
        let mut dirty = TerrainDirtyChunks::default();
        dirty.mark_world_rect(&config(), Vec2::new(70.0, 10.0), Vec2::new(80.0, 20.0));
        // Chunk (1, 0) plus its eight neighbours.
        assert_eq!(dirty.remesh.len(), 9);
        for x in 0..=2 {
            for z in -1..=1 {
                assert!(dirty.remesh.contains(&IVec2::new(x, z)), "missing ({x}, {z})");
                assert!(dirty.recollide.contains(&IVec2::new(x, z)));
            }
        }
        assert_eq!(dirty.focus, Some(Vec2::new(75.0, 15.0)));
    }

    #[test]
    fn rect_is_clamped_to_the_terrain_grid() {
        let mut dirty = TerrainDirtyChunks::default();
        // Straddles the positive X edge (chunk 3 is the last one) and runs
        // far past it; only chunks 2..=3 on X exist to mark.
        dirty.mark_world_rect(&config(), Vec2::new(200.0, -5.0), Vec2::new(10_000.0, 5.0));
        assert!(dirty.remesh.iter().all(|c| c.x >= 2 && c.x <= 3 && c.y >= -2 && c.y <= 1));
        assert!(dirty.remesh.contains(&IVec2::new(3, 0)));
        assert!(dirty.remesh.contains(&IVec2::new(3, -1)));
    }

    #[test]
    fn rect_outside_the_grid_or_non_finite_marks_nothing() {
        let mut dirty = TerrainDirtyChunks::default();
        dirty.mark_world_rect(&config(), Vec2::new(5_000.0, 5_000.0), Vec2::new(5_100.0, 5_100.0));
        dirty.mark_world_rect(&config(), Vec2::new(f32::NAN, 0.0), Vec2::new(1.0, 1.0));
        dirty.mark_world_rect(&config(), Vec2::new(-f32::INFINITY, 0.0), Vec2::new(1.0, 1.0));
        assert!(dirty.is_empty());
    }

    #[test]
    fn a_volume_edit_marks_its_box_and_every_chunk_its_bricks_reach() {
        use crate::terrain::volume::{apply_sphere, CsgOp};

        // 16 cells of 4 m: a brick spans 64 m, exactly one chunk.
        let config = TerrainConfig { chunk_resolution: 16, ..config() };
        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::new(100.0, 10.0, 30.0), 3.0, CsgOp::Carve, None);
        assert!(!edit.is_empty());

        let mut dirty = TerrainDirtyChunks::default();
        dirty.mark_volume_edit(&config, &edit);
        for coord in &edit.bricks {
            let (lo, hi) = brick_world_bounds(*coord, lattice_cell_size(&config));
            for corner in [Vec2::new(lo.x, lo.z), Vec2::new(hi.x, hi.z)] {
                let chunk = (corner / config.chunk_size).floor().as_ivec2();
                assert!(dirty.remesh.contains(&chunk), "brick {coord} reaches chunk {chunk}");
                assert!(dirty.recollide.contains(&chunk));
            }
        }
        assert!(dirty.remesh.contains(&IVec2::new(1, 0)), "the chunk under the edit");
        let centre = dirty.focus.expect("focused on the edit");
        assert!((centre - Vec2::new(100.0, 30.0)).length() < 8.0, "focus {centre} is off the edit");

        let mut untouched = TerrainDirtyChunks::default();
        untouched.mark_volume_edit(&config, &VolumeEdit::default());
        assert!(untouched.is_empty());
    }

    #[test]
    fn mark_all_covers_the_whole_grid() {
        let mut dirty = TerrainDirtyChunks::default();
        dirty.mark_all(&config());
        assert_eq!(dirty.remesh.len(), 49);
        assert_eq!(dirty.recollide.len(), 49);
    }

    #[test]
    fn raster_marks_queue_a_rebake_and_volume_marks_do_not() {
        use crate::terrain::volume::{apply_sphere, CsgOp};

        let config = TerrainConfig { chunk_resolution: 16, ..config() };
        let mut dirty = TerrainDirtyChunks::default();
        assert!(!dirty.has_pending_rebake());

        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::new(100.0, 10.0, 30.0), 3.0, CsgOp::Carve, None);
        dirty.mark_volume_edit(&config, &edit);
        assert!(!dirty.is_empty());
        assert!(!dirty.has_pending_rebake(), "a volume edit leaves the raster alone");

        dirty.mark_world_rect(&config, Vec2::new(70.0, 10.0), Vec2::new(80.0, 20.0));
        dirty.mark(IVec2::new(-1, 2));
        assert!(dirty.has_pending_rebake());
        let rebake = dirty.take_rebake(&config);
        assert!(!rebake.whole);
        assert_eq!(
            rebake.rects,
            vec![
                (Vec2::new(70.0, 10.0), Vec2::new(80.0, 20.0)),
                (Vec2::new(-64.0, 128.0), Vec2::new(0.0, 192.0)),
            ],
            "the rectangle as marked, without the ring, then the chunk's footprint"
        );
        assert!(!dirty.has_pending_rebake());
        assert!(dirty.take_rebake(&config).is_empty());

        dirty.mark_world_rect(&config, Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0));
        dirty.mark_all(&config);
        assert_eq!(dirty.take_rebake(&config), TerrainRebake::whole());
    }

    #[test]
    fn surface_changes_reach_every_reader_once_and_skip_the_border_ring() {
        let config = config();
        let mut dirty = TerrainDirtyChunks::default();
        assert_eq!(dirty.surface_changes_since(0), SurfaceChanges::default());

        // Well inside chunk (1, 0): only that chunk's surface moved, though
        // its ring of neighbours remeshes.
        dirty.mark_world_rect(&config, Vec2::new(90.0, 20.0), Vec2::new(100.0, 30.0));
        assert_eq!(dirty.remesh.len(), 9);
        let first = dirty.surface_changes_since(0);
        assert_eq!(first, SurfaceChanges { all: false, chunks: vec![IVec2::new(1, 0)] });
        let seen = dirty.surface_seq();
        assert!(seen > 0);
        assert!(dirty.surface_changes_since(seen).chunks.is_empty(), "nothing new since");

        // A second reader that never looked sees both marks; the first reader
        // sees only the new one.
        dirty.mark(IVec2::new(-2, 1));
        let mut chunks = dirty.surface_changes_since(seen).chunks;
        assert_eq!(chunks, vec![IVec2::new(-2, 1)]);
        chunks = dirty.surface_changes_since(0).chunks;
        chunks.sort_by_key(|c| (c.x, c.y));
        assert_eq!(chunks, vec![IVec2::new(-2, 1), IVec2::new(1, 0)]);

        // A rect on a chunk border stamps both sides; a palette recolour
        // stamps nothing; `mark_all` stamps everything.
        let seen = dirty.surface_seq();
        dirty.mark_world_rect(&config, Vec2::new(63.9, 10.0), Vec2::new(64.1, 12.0));
        let mut chunks = dirty.surface_changes_since(seen).chunks;
        chunks.sort_by_key(|c| (c.x, c.y));
        assert_eq!(chunks, vec![IVec2::new(0, 0), IVec2::new(1, 0)]);
        let seen = dirty.surface_seq();
        dirty.mark_all_meshes(&config);
        assert_eq!(dirty.surface_seq(), seen, "a recolour moves no ground");
        dirty.mark_all(&config);
        assert!(dirty.surface_changes_since(seen).all);
    }

    #[test]
    fn height_marks_raise_the_height_seq_and_paint_and_volume_marks_do_not() {
        use crate::terrain::volume::{apply_sphere, CsgOp};

        let config = TerrainConfig { chunk_resolution: 16, ..config() };
        let mut dirty = TerrainDirtyChunks::default();
        let rect = (Vec2::new(70.0, 10.0), Vec2::new(80.0, 20.0));

        let (heights, surface) = (dirty.height_seq(), dirty.surface_seq());
        dirty.mark_world_rect_materials(&config, rect.0, rect.1);
        assert_eq!(dirty.height_seq(), heights, "paint moves no height");
        assert!(dirty.surface_seq() > surface, "but it changes the surface");
        assert_eq!(dirty.surface_changes_since(surface).chunks, vec![IVec2::new(1, 0)]);
        assert!(dirty.has_pending_rebake(), "and the bake repaints it");

        let (heights, surface) = (dirty.height_seq(), dirty.surface_seq());
        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::new(100.0, 10.0, 30.0), 3.0, CsgOp::Carve, None);
        dirty.mark_volume_edit(&config, &edit);
        assert_eq!(dirty.height_seq(), heights, "a volume edit leaves the raster");
        assert!(dirty.surface_seq() > surface);

        let heights = dirty.height_seq();
        dirty.mark_all_meshes(&config);
        assert_eq!(dirty.height_seq(), heights, "a recolour moves no height");

        let heights = dirty.height_seq();
        dirty.mark_world_rect(&config, rect.0, rect.1);
        assert!(dirty.height_seq() > heights, "a sculpt stroke raises it");
        let heights = dirty.height_seq();
        dirty.mark(IVec2::new(-1, 2));
        assert!(dirty.height_seq() > heights, "a chunk mark raises it");
        let heights = dirty.height_seq();
        dirty.mark_all(&config);
        assert!(dirty.height_seq() > heights, "a whole-grid mark raises it");
    }

    #[test]
    fn a_long_rebake_backlog_merges_into_one_rectangle() {
        let mut dirty = TerrainDirtyChunks::default();
        for i in 0..=MAX_REBAKE_RECTS {
            let corner = Vec2::splat(i as f32);
            dirty.mark_world_rect(&config(), corner, corner + Vec2::ONE);
        }
        let rebake = dirty.take_rebake(&config());
        assert_eq!(rebake.rects, vec![(Vec2::ZERO, Vec2::splat(MAX_REBAKE_RECTS as f32 + 1.0))]);
    }

    #[test]
    fn the_apply_system_bakes_new_layers_and_rebakes_marked_base_edits() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::terrain::height_query::{cache_cell_at_world, height_at_world, set_height_at_world};
        use crate::terrain::layers::{FlattenPadLayer, LayerDesc, LayerKind};

        let config = TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            ..TerrainConfig::default()
        };
        let mut base = TerrainData::procedural();
        base.resize_cache(&config);
        let pad = LayerDesc {
            id: 1,
            order: 0,
            kind: LayerKind::FlattenPad(FlattenPadLayer {
                center: Vec3::new(10.0, 30.0, 10.0),
                yaw: 0.0,
                size: Vec2::new(8.0, 8.0),
                falloff: 2.0,
                material: None,
            }),
        };

        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<TerrainDirtyChunks>();
        let root = world.spawn((TerrainRoot, config.clone(), base.clone(), TerrainBaked::new(&base, vec![pad]))).id();

        // Nothing is marked, but a new stack bakes anyway.
        world.run_system_once(apply_terrain_dirty_chunks).expect("runs");
        let baked = world.get::<TerrainBaked>(root).expect("still baked");
        assert!(!baked.wants_bake());
        assert!((height_at_world(&config, &baked.data, 10.0, 10.0) - 30.0).abs() < 1e-3, "the pad is baked in");
        let data = world.get::<TerrainData>(root).expect("the base");
        assert_eq!(height_at_world(&config, data, 10.0, 10.0), 0.0, "the base is untouched");

        // A base edit away from the pad, marked the way writers mark.
        {
            let mut data = world.get_mut::<TerrainData>(root).expect("the base");
            set_height_at_world(&config, &mut data, -20.0, -20.0, 12.0, 1.0);
        }
        world
            .resource_mut::<TerrainDirtyChunks>()
            .mark_world_rect(&config, Vec2::splat(-20.5), Vec2::splat(-19.5));
        world.run_system_once(apply_terrain_dirty_chunks).expect("runs");
        let baked = world.get::<TerrainBaked>(root).expect("still baked");
        let cell = cache_cell_at_world(&config, &base, -20.0, -20.0).expect("a raster");
        let index = cell.y as usize * base.cache_width as usize + cell.x as usize;
        assert!((config.world_height(baked.data.height_cache[index]) - 12.0).abs() < 1e-3, "the edit reached the bake");
        assert!((height_at_world(&config, &baked.data, 10.0, 10.0) - 30.0).abs() < 1e-3, "and the pad stayed");
    }
}
