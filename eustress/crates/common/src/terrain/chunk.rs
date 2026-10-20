//! Terrain chunk component and the streaming systems (spawn and cull).
//!
//! A chunk is resident while its entity exists. It carries the chunk's render
//! mesh and, with the `physics` feature, its static collider (`collider.rs`):
//! a LOD-0 heightfield, or a trimesh of its marching-cubes surface when it
//! holds volumetric edits, or of the ground it keeps when it has holes. Edits
//! live on the root (`TerrainData`, `TerrainVolume`), so a chunk streamed back
//! in rebuilds from them.
//!
//! ## Streaming
//!
//! A raster terrain up to [`MAX_RESIDENT_RASTER_CHUNKS`] is fully resident:
//! every chunk spawns and none is culled. Procedural terrain and larger
//! rasters stream around the scene camera (see
//! [`super::scene_camera_translation`], [`chunk_streaming_origin`] and
//! [`chunk_stream_radius`]): chunks within the streaming radius spawn, nearest
//! first, and chunks past [`CULL_HYSTERESIS`] times it are culled.
//!
//! ## The far field
//!
//! A larger raster streams only `TerrainConfig::view_distance` while its root
//! carries [`TerrainFarFieldActive`]: its GPU far field (`far_field.rs`) draws
//! the whole map beyond a near disc that stays a chunk inside that distance.
//! Without the marker (the far field is switched off, or its textures are not
//! on the GPU yet) the radius reaches the footprint corner farthest from the
//! camera, so every chunk spawns.
//!
//! ## Body chunks
//!
//! With the `physics` feature streaming also holds the ground under bodies:
//! every chunk within [`body_collider_margin`] of a dynamic or kinematic Avian
//! body (the avatar controller moves the player as a kinematic body) spawns
//! wherever the camera is, and is never culled while the body stays near.
//! Each spawn scan collects those chunks once ([`body_chunks`]) and spawns
//! them ahead of the chunks in view. The chunks bodies are over come first
//! and spawn past the per-frame spawn budget, up to
//! [`MAX_UNBUDGETED_SPAWNS_PER_FRAME`] of them a frame, so the ground under a
//! body arrives on the scan that finds it missing rather than two chunks a
//! frame behind everything else; the rest of the margin and the view share
//! the budget. A cull pass keeps every chunk within
//! [`CULL_HYSTERESIS`] times the margin of a body ([`chunk_should_cull`]).
//!
//! ## Holes
//!
//! On a sparse surface (`TerrainData::sparse_surface`) a chunk with nothing to
//! draw, no ground and no volume bricks ([`chunk_should_spawn`]), is never
//! spawned, for the camera or for a body. The spawn scan remembers such chunks
//! in a [`TerrainEmptyChunks`] rather than testing them again on every pass.

use std::collections::{HashMap, HashSet};

use bevy::ecs::change_detection::Tick;
use bevy::prelude::*;
use super::far_field::TerrainFarFieldActive;
use super::{
    TerrainBaked, TerrainChunkMaterial, TerrainConfig, TerrainData, TerrainDirtyChunks, TerrainGenerationQueue,
    TerrainGridKey, TerrainRoot, TerrainVolume, attach_chunk_collider, chunk_has_volume, chunk_lod_distance,
    chunk_spawn_cost, chunk_world_position, generate_chunk_render_mesh_and_surface, new_terrain_chunk_material,
    scene_camera_translation, surface_data,
};

#[cfg(feature = "physics")]
use avian3d::prelude::*;

// ============================================================================
// Physics Layers (when physics feature enabled)
// ============================================================================

/// Physics collision layers for terrain
#[cfg(feature = "physics")]
#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum TerrainPhysicsLayer {
    /// Default layer for general objects
    #[default]
    Default,
    /// Terrain chunks - static colliders
    Terrain,
    /// Player/character controllers
    Player,
    /// Vehicles
    Vehicle,
    /// Projectiles (may ignore terrain for performance)
    Projectile,
}

// ============================================================================
// Components
// ============================================================================

/// Individual terrain chunk with LOD tracking
#[derive(Component, Clone, Reflect, Debug)]
#[reflect(Component)]
pub struct Chunk {
    /// Grid position of this chunk
    pub position: IVec2,
    
    /// Current LOD level (0 = highest detail)
    pub lod: u32,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            position: IVec2::ZERO,
            lod: 0,
        }
    }
}

/// Throttle state for chunk_spawn_system so it doesn't scan every frame.
#[derive(Resource)]
pub struct ChunkSpawnThrottle {
    /// Last camera chunk position used for the scan
    pub last_camera_chunk: IVec2,
    /// Seconds since last full scan
    pub last_scan_time: f64,
    /// Minimum interval between full scans (seconds)
    pub scan_interval: f64,
    /// The last scan found more chunks to spawn than its per-frame cap, so
    /// the next frame scans again instead of waiting out the interval.
    pub backlog: bool,
}

impl Default for ChunkSpawnThrottle {
    fn default() -> Self {
        Self {
            last_camera_chunk: IVec2::new(i32::MAX, i32::MAX), // Force first scan
            last_scan_time: 0.0,
            scan_interval: 0.5,
            backlog: false,
        }
    }
}

/// Most chunks the spawn scan may consider along one axis of its square,
/// so an extreme `view_distance / chunk_size` ratio cannot stall a frame.
const MAX_VIEW_CHUNKS: i32 = 128;

/// Horizontal distance from `point` to the nearest point of a chunk's XZ
/// footprint (0 when `point` is above the chunk). Streaming measures in XZ so
/// raising the camera to look over the terrain does not cull it away.
pub fn chunk_xz_distance(chunk_pos: IVec2, config: &TerrainConfig, point: Vec3) -> f32 {
    let size = config.chunk_size.max(1e-3);
    let min_x = chunk_pos.x as f32 * size;
    let min_z = chunk_pos.y as f32 * size;
    let dx = (min_x - point.x).max(point.x - (min_x + size)).max(0.0);
    let dz = (min_z - point.z).max(point.z - (min_z + size)).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

/// The point chunks stream and cull around. For a terrain backed by a height
/// raster (finite extent), a camera outside the footprint is pulled onto its
/// nearest edge. Only a raster too large to stay fully resident (see
/// [`chunk_stream_radius`]) streams at all; procedural terrain streams
/// around the camera itself.
pub fn chunk_streaming_origin(config: &TerrainConfig, data: &TerrainData, camera_pos: Vec3) -> Vec3 {
    if data.height_cache.is_empty() {
        return camera_pos;
    }
    let (min, max) = config.footprint_xz();
    Vec3::new(
        camera_pos.x.max(min.x).min(max.x),
        camera_pos.y,
        camera_pos.z.max(min.y).min(max.y),
    )
}

/// Most chunks a raster terrain may hold before it streams instead of
/// staying fully resident (32x32; flat-large is 17x17).
pub const MAX_RESIDENT_RASTER_CHUNKS: u32 = 1024;

/// Whether `spawn_terrain` and the spawn scan hold every chunk of this
/// terrain: a raster small enough to stay resident. Its chunks own the static
/// heightfield colliders bodies rest on, so none may be missing or culled.
pub fn raster_fully_resident(config: &TerrainConfig, data: &TerrainData) -> bool {
    !data.height_cache.is_empty() && config.total_chunks() <= MAX_RESIDENT_RASTER_CHUNKS
}

/// Streaming radius around `origin`. A raster terrain small enough to stay
/// resident returns `None` (spawn the whole grid, never cull). A larger
/// raster streams `view_distance` while `far_field` (its root carries
/// [`TerrainFarFieldActive`]): the far field draws the map beyond its near
/// disc, which stays a chunk inside that distance of the camera, and the
/// chunks of that disc lie within `view_distance` of the clamped origin too.
/// Without the far field the radius widens to reach the raster's farthest
/// footprint corner from the clamped origin, because its grid half-extent
/// equals `view_distance` by construction
/// (`TerrainTomlFile::to_terrain_config`), so a `view_distance` disc could
/// never cover it. Procedural terrain streams `view_distance`.
pub fn chunk_stream_radius(config: &TerrainConfig, data: &TerrainData, origin: Vec3, far_field: bool) -> Option<f32> {
    let view_distance = config.view_distance.max(0.0);
    if data.height_cache.is_empty() {
        return Some(view_distance);
    }
    if raster_fully_resident(config, data) {
        return None;
    }
    if far_field {
        return Some(view_distance);
    }
    let (min, max) = config.footprint_xz();
    let far_x = (origin.x - min.x).abs().max((max.x - origin.x).abs());
    let far_z = (origin.z - min.y).abs().max((max.y - origin.z).abs());
    Some(view_distance.max((far_x * far_x + far_z * far_z).sqrt()))
}

/// Whether chunk `chunk_pos` has anything to draw, so a spawn pass spawns
/// it: ground ([`TerrainData::chunk_has_ground`]), or volume bricks reaching
/// its columns, which draw by marching cubes even over holes. Every chunk of
/// a full surface has ground, so only a sparse surface leaves chunks out.
pub fn chunk_should_spawn(chunk_pos: IVec2, config: &TerrainConfig, data: &TerrainData, volume: &TerrainVolume) -> bool {
    data.chunk_has_ground(config, chunk_pos) || (!volume.is_empty() && chunk_has_volume(chunk_pos, config, volume))
}

/// Raster cells a spawn pass may scan in a frame finding chunks with nothing
/// to draw, each such chunk counting [`empty_chunk_scan_cells`], so the empty
/// expanse of a sparse surface is sorted out over several frames rather than
/// in one. The first chunk of a frame is always decided.
pub const EMPTY_CHUNK_SCAN_CELLS_PER_FRAME: usize = 1 << 18;

/// Raster cells finding one chunk empty scans: its whole tile, since a single
/// cell of ground would have ended the scan.
pub fn empty_chunk_scan_cells(config: &TerrainConfig) -> usize {
    let side = config.chunk_resolution as usize;
    side.saturating_mul(side).max(1)
}

/// Chunk positions of one terrain root known to have nothing to draw (see
/// [`chunk_should_spawn`]), which [`chunk_spawn_system`] skips rather than
/// testing them again on every scan.
///
/// A mark of [`TerrainDirtyChunks`] that changes a chunk's surface forgets
/// that chunk, since a paint can give a hole ground and a brick can land in
/// an empty chunk. Every entry is forgotten when the root, its grid or its
/// raster's layout changes, and when its data, bake or volume changes with no
/// such mark to say where.
#[derive(Debug, Default)]
pub struct TerrainEmptyChunks {
    /// The terrain the entries were found on.
    source: Option<EmptyChunkSource>,
    /// Change ticks of the root's data, bake and volume at the last sync.
    written: [Option<Tick>; 3],
    /// [`TerrainDirtyChunks::surface_seq`] at the last sync.
    surface_seq: u64,
    /// The chunks known to have nothing to draw.
    empty: HashSet<IVec2>,
}

/// The terrain a [`TerrainEmptyChunks`] holds entries for: its root, its grid,
/// and the layout and hole rule of the raster it draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EmptyChunkSource {
    root: Entity,
    grid: TerrainGridKey,
    raster: UVec2,
    sparse_surface: bool,
}

impl TerrainEmptyChunks {
    /// Bring the entries up to date with root `root`, whose surface data (the
    /// bake when it has layers) is `data`, and whose data, bake and volume
    /// last changed at `written` (`None` for a component it lacks). `dirty`
    /// says which chunks the changes since the last sync reached (see the
    /// type docs).
    pub fn sync(
        &mut self,
        root: Entity,
        config: &TerrainConfig,
        data: &TerrainData,
        written: [Option<Tick>; 3],
        dirty: Option<&TerrainDirtyChunks>,
    ) {
        let source = EmptyChunkSource {
            root,
            grid: TerrainGridKey::of(config),
            raster: UVec2::new(data.cache_width, data.cache_height),
            sparse_surface: data.sparse_surface,
        };
        let surface_seq = dirty.map_or(0, TerrainDirtyChunks::surface_seq);
        let changes = dirty.map(|dirty| dirty.surface_changes_since(self.surface_seq)).unwrap_or_default();
        let unmarked_write = written != self.written && changes.chunks.is_empty();
        if self.source != Some(source) || surface_seq < self.surface_seq || changes.all || unmarked_write {
            self.empty.clear();
        } else {
            for chunk in &changes.chunks {
                self.empty.remove(chunk);
            }
        }
        self.source = Some(source);
        self.written = written;
        self.surface_seq = surface_seq;
    }

    /// Whether `chunk` is known to have nothing to draw.
    pub fn contains(&self, chunk: IVec2) -> bool {
        self.empty.contains(&chunk)
    }

    /// Remember that `chunk` has nothing to draw.
    pub fn insert(&mut self, chunk: IVec2) {
        self.empty.insert(chunk);
    }
}

/// Least margin, metres, of ground kept resident around a body (see
/// [`body_collider_margin`]).
pub const MIN_BODY_COLLIDER_MARGIN: f32 = 8.0;

/// How far around a body, in XZ, streaming keeps every chunk resident: one
/// chunk, and at least [`MIN_BODY_COLLIDER_MARGIN`], so a body that moves
/// between two spawn scans still has ground under it until the next scan
/// reaches its new place.
pub fn body_collider_margin(config: &TerrainConfig) -> f32 {
    let size = config.chunk_size;
    if size.is_finite() {
        size.max(MIN_BODY_COLLIDER_MARGIN)
    } else {
        MIN_BODY_COLLIDER_MARGIN
    }
}

/// The chunks bodies need resident (see [`body_chunks`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyChunks {
    /// Every chunk within the margin of a body.
    pub near: HashSet<IVec2>,
    /// The chunks bodies are over, each also in `near`. The spawn scan takes
    /// them first, since a body there is resting or falling on them.
    pub under: HashSet<IVec2>,
}

/// The chunks bodies at world positions `bodies` need resident: every chunk
/// within `margin` of a body in XZ (the distance [`chunk_xz_distance`]
/// measures), on the grid of a raster terrain (procedural terrain has no
/// edge), that has something to hold a body up ([`chunk_should_spawn`]). A
/// body over hole columns needs no chunk there unless volume bricks reach
/// them. Positions that are not finite are skipped.
///
/// Bodies are grouped by the chunk they are over, and each group keeps the XZ
/// rectangle its bodies span: a chunk is kept when it lies within `margin` of
/// that rectangle, which takes in every chunk within `margin` of each body in
/// it, and exactly those for a body alone. Thousands of bodies then cost a
/// map update each, and each group's neighbourhood is walked once.
pub fn body_chunks(
    bodies: impl IntoIterator<Item = Vec3>,
    margin: f32,
    config: &TerrainConfig,
    data: &TerrainData,
    volume: &TerrainVolume,
) -> BodyChunks {
    let size = config.chunk_size.max(1e-3);
    let margin = if margin.is_finite() { margin.max(0.0) } else { 0.0 };
    let mut groups: HashMap<IVec2, (Vec2, Vec2)> = HashMap::new();
    for position in bodies {
        let xz = Vec2::new(position.x, position.z);
        if !xz.is_finite() {
            continue;
        }
        let rect = groups.entry((xz / size).floor().as_ivec2()).or_insert((xz, xz));
        rect.0 = rect.0.min(xz);
        rect.1 = rect.1.max(xz);
    }

    let mut chunks = BodyChunks::default();
    // Chunks found to hold nothing, so a neighbourhood several groups share
    // tests each of them once.
    let mut hollow: HashSet<IVec2> = HashSet::new();
    let bounded = !data.height_cache.is_empty();
    let reach = IVec2::splat(MAX_VIEW_CHUNKS);
    for (over, (min, max)) in groups {
        // A chunk past the margin on each side as well, so one exactly
        // `margin` away is still measured.
        let mut lo = ((min - margin) / size)
            .floor()
            .as_ivec2()
            .saturating_sub(IVec2::ONE)
            .max(over.saturating_sub(reach));
        let mut hi = ((max + margin) / size)
            .floor()
            .as_ivec2()
            .saturating_add(IVec2::ONE)
            .min(over.saturating_add(reach));
        if bounded {
            lo = lo.max(config.chunk_min());
            hi = hi.min(config.chunk_max());
        }
        for z in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let chunk = IVec2::new(x, z);
                if chunk_xz_distance_to_rect(chunk, config, min, max) > margin
                    || chunks.near.contains(&chunk)
                    || hollow.contains(&chunk)
                {
                    continue;
                }
                if chunk_should_spawn(chunk, config, data, volume) {
                    chunks.near.insert(chunk);
                } else {
                    hollow.insert(chunk);
                }
            }
        }
        if chunks.near.contains(&over) {
            chunks.under.insert(over);
        }
    }
    chunks
}

/// Horizontal distance from the XZ rectangle `min` to `max` (world X and Z in
/// a `Vec2`'s x and y) to the nearest point of a chunk's footprint, 0 where
/// they overlap: [`chunk_xz_distance`] for a rectangle of one point.
fn chunk_xz_distance_to_rect(chunk_pos: IVec2, config: &TerrainConfig, min: Vec2, max: Vec2) -> f32 {
    let size = config.chunk_size.max(1e-3);
    let min_x = chunk_pos.x as f32 * size;
    let min_z = chunk_pos.y as f32 * size;
    let dx = (min_x - max.x).max(min.x - (min_x + size)).max(0.0);
    let dz = (min_z - max.y).max(min.y - (min_z + size)).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

/// Whether streaming holds ground under a body of this kind: a dynamic body
/// falls onto it, and the avatar controller moves the player as a kinematic
/// body that stands on it (`avatar::spawn`). A static body, the terrain's own
/// colliders among them, needs none.
#[cfg(feature = "physics")]
fn body_needs_ground(body: &RigidBody) -> bool {
    matches!(body, RigidBody::Dynamic | RigidBody::Kinematic)
}

/// The order a spawn scan takes its candidates in, first to last, nearest to
/// the camera first within each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SpawnPriority {
    /// A chunk a body is over ([`BodyChunks::under`]).
    UnderBody,
    /// Another chunk within the margin of a body.
    NearBody,
    /// A chunk within the streaming radius.
    InView,
}

/// Put a spawn scan's candidates (position, priority, XZ distance from the
/// streaming origin) in the order it takes them: by [`SpawnPriority`], then
/// nearest first, then by position, so equal distances keep one order.
/// `total_cmp` keeps the order total even for a distance that is not a
/// number.
fn sort_spawn_candidates(candidates: &mut [(IVec2, SpawnPriority, f32)]) {
    candidates.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.2.total_cmp(&b.2))
            .then_with(|| (a.0.x, a.0.y).cmp(&(b.0.x, b.0.y)))
    });
}

/// Work a spawn pass may do in a frame, in heightfield chunks: mesh
/// generation is expensive, so it is spread across frames (see
/// [`chunk_spawn_cost`] for what a chunk costs).
const MAX_SPAWNS_PER_FRAME: usize = 2;

/// Most chunks under bodies a spawn pass spawns in a frame past its budget
/// (see [`chunk_spawn_system`]), so thousands of bodies spread over thousands
/// of chunks cannot stall one frame. The chunks past it wait for the budget,
/// still ahead of every other candidate.
pub const MAX_UNBUDGETED_SPAWNS_PER_FRAME: usize = 64;

/// What a spawn pass does with its next candidate (see [`SpawnBudget::admit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpawnAdmission {
    /// Spawn it without charging the budget: a chunk a body is over.
    Unbudgeted,
    /// Decide it against the budget.
    Budgeted,
    /// The budget is spent: leave it, and every candidate after it, to the
    /// next pass.
    Stop,
}

/// What one spawn pass has used of its frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SpawnBudget {
    /// Heightfield chunks' worth spawned against [`MAX_SPAWNS_PER_FRAME`].
    spent: usize,
    /// Raster cells scanned finding chunks empty, against
    /// [`EMPTY_CHUNK_SCAN_CELLS_PER_FRAME`].
    scanned: usize,
    /// Chunks spawned past the budget, against
    /// [`MAX_UNBUDGETED_SPAWNS_PER_FRAME`].
    unbudgeted: usize,
}

impl SpawnBudget {
    /// How the pass treats its next candidate, of `priority`. A chunk a body
    /// is over spawns past the budget while the frame has spawned fewer than
    /// [`MAX_UNBUDGETED_SPAWNS_PER_FRAME`] such chunks; every other candidate
    /// is decided while the spawn and empty-scan budgets both have room, so
    /// the first budgeted candidate of a frame is always decided.
    fn admit(&self, priority: SpawnPriority) -> SpawnAdmission {
        if priority == SpawnPriority::UnderBody && self.unbudgeted < MAX_UNBUDGETED_SPAWNS_PER_FRAME {
            SpawnAdmission::Unbudgeted
        } else if self.spent < MAX_SPAWNS_PER_FRAME && self.scanned < EMPTY_CHUNK_SCAN_CELLS_PER_FRAME {
            SpawnAdmission::Budgeted
        } else {
            SpawnAdmission::Stop
        }
    }

    /// A chunk spawned against the budget, costing `cost` heightfield chunks.
    fn spend(&mut self, cost: usize) {
        self.spent = self.spent.saturating_add(cost);
    }

    /// A chunk spawned past the budget.
    fn spend_unbudgeted(&mut self) {
        self.unbudgeted += 1;
    }

    /// A chunk found to have nothing to draw, after scanning `cells` raster
    /// cells.
    fn scan_empty(&mut self, cells: usize) {
        self.scanned = self.scanned.saturating_add(cells);
    }
}

/// System to spawn new chunks as camera moves
///
/// Throttled: only scans for missing chunks when the camera moves to a new
/// chunk, after scan_interval seconds, while a previous scan left chunks
/// unspawned, or (with the `physics` feature, on a streaming terrain) when a
/// body becomes dynamic or kinematic, so a body released by Play or by
/// unanchoring does not wait out the interval in free fall. It spawns at most
/// two heightfield chunks' worth per frame (a chunk holding volumetric edits
/// counts as several, see [`chunk_spawn_cost`]), plus the chunks bodies are
/// over (see below).
///
/// Never spawns a position that already has a chunk entity. While
/// [`TerrainGenerationQueue`] is still filling, the queue owns the grid and
/// this pass spawns only the chunks bodies are over, on any terrain, every
/// frame (the queue skips a position that already has a chunk): a body
/// placed during the fill, an avatar among them, does not fall through while
/// its chunk waits its turn. A terrain backed by a height raster has a
/// finite extent (its chunk grid, [`TerrainConfig::chunk_min`] to
/// [`TerrainConfig::chunk_max`]); sampling past it would just repeat the
/// clamped edge, so nothing is spawned there.
/// A raster that fits [`MAX_RESIDENT_RASTER_CHUNKS`] is filled whole and
/// stays resident; only procedural terrain (no edge) and oversized rasters
/// stream by radius (see [`chunk_stream_radius`]), an oversized raster only
/// to `view_distance` while its root carries [`TerrainFarFieldActive`].
///
/// A streaming terrain also spawns the chunks bodies need ([`body_chunks`]),
/// however far they are from the camera, ahead of the chunks in view: first
/// the chunks bodies are over, then the rest of their margin, then the view,
/// nearest to the camera first within each. The chunks bodies are over spawn
/// on the scan that finds them missing, past the budget, up to
/// [`MAX_UNBUDGETED_SPAWNS_PER_FRAME`] a frame (it logs the first time a scan
/// finds more); the rest of the margin and the view share the budget. The
/// ground under a body so arrives on the scan that finds it missing, however
/// much else is streaming in.
///
/// A chunk with nothing to draw ([`chunk_should_spawn`]) is not spawned: the
/// scan remembers it in a [`TerrainEmptyChunks`] and skips it until a change
/// to the ground or the volume there calls for another look. Finding chunks
/// empty counts against [`EMPTY_CHUNK_SCAN_CELLS_PER_FRAME`].
///
/// With the `physics` feature each spawned chunk gets its collider.
pub fn chunk_spawn_system(
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<
        (
            Entity,
            &TerrainConfig,
            Ref<TerrainData>,
            Option<Ref<TerrainVolume>>,
            Option<&Children>,
            Option<&TerrainChunkMaterial>,
            Option<Ref<TerrainBaked>>,
            Option<&TerrainFarFieldActive>,
        ),
        With<TerrainRoot>,
    >,
    chunk_query: Query<&Chunk>,
    chunk_materials: Query<&MeshMaterial3d<StandardMaterial>, With<Chunk>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    generation_queue: Res<TerrainGenerationQueue>,
    dirty: Option<Res<TerrainDirtyChunks>>,
    time: Res<Time>,
    mut throttle: ResMut<ChunkSpawnThrottle>,
    mut empty_chunks: Local<TerrainEmptyChunks>,
    mut under_cap_logged: Local<bool>,
    #[cfg(feature = "physics")] bodies: Query<(Ref<RigidBody>, &GlobalTransform)>,
) {
    let filling = generation_queue.is_generating();
    #[cfg(not(feature = "physics"))]
    {
        // Without bodies there is nothing to spawn ahead of the queue.
        if filling {
            return;
        }
    }
    let Some(camera_pos) = scene_camera_translation(&cameras) else { return };
    let Ok((terrain_entity, config, data, volume, children, root_material, baked, far_field)) = terrain_query.single()
    else {
        return;
    };
    let written = [
        Some(data.last_changed()),
        baked.as_ref().map(|baked| baked.last_changed()),
        volume.as_ref().map(|volume| volume.last_changed()),
    ];
    let volume = volume.as_deref().unwrap_or(TerrainVolume::empty());
    let data = surface_data(&data, baked.as_deref());
    // Ahead of the throttle, so the entries follow every change.
    empty_chunks.sync(terrain_entity, config, data, written, dirty.as_deref());
    let origin = chunk_streaming_origin(config, data, camera_pos);
    let radius = chunk_stream_radius(config, data, origin, far_field.is_some());

    // Throttle: only scan when camera moves to a new chunk, after the
    // interval, while the last scan left a backlog, or when a body that needs
    // ground has just appeared on a streaming terrain (a fully resident one
    // holds every chunk anyway).
    #[cfg(feature = "physics")]
    let body_arrived = (radius.is_some() || filling)
        && bodies.iter().any(|(body, _)| body.is_changed() && body_needs_ground(&body));
    #[cfg(not(feature = "physics"))]
    let body_arrived = false;
    let size = config.chunk_size.max(1e-3);
    let camera_chunk = IVec2::new(
        (origin.x / size).floor() as i32,
        (origin.z / size).floor() as i32,
    );
    let current_time = time.elapsed_secs_f64();
    let camera_moved_chunk = camera_chunk != throttle.last_camera_chunk;
    let interval_elapsed = current_time - throttle.last_scan_time >= throttle.scan_interval;
    // During the fill every frame looks, but only for the chunks under
    // bodies, which is cheap.
    if !filling && !camera_moved_chunk && !interval_elapsed && !throttle.backlog && !body_arrived {
        return;
    }
    // A look during the fill is not a scan of the view, so the first scan
    // after the fill runs at once rather than after the interval.
    if !filling {
        throttle.last_camera_chunk = camera_chunk;
        throttle.last_scan_time = current_time;
    }

    let (grid_min, grid_max) = (config.chunk_min(), config.chunk_max());
    let (mut min_x, mut max_x, mut min_z, mut max_z) = match radius {
        None => (grid_min.x, grid_max.x, grid_min.y, grid_max.y),
        Some(r) => {
            let n = ((r / size).ceil() as i32).saturating_add(1).min(MAX_VIEW_CHUNKS);
            (
                camera_chunk.x.saturating_sub(n),
                camera_chunk.x.saturating_add(n),
                camera_chunk.y.saturating_sub(n),
                camera_chunk.y.saturating_add(n),
            )
        }
    };
    if !data.height_cache.is_empty() {
        min_x = min_x.max(grid_min.x);
        max_x = max_x.min(grid_max.x);
        min_z = min_z.max(grid_min.y);
        max_z = max_z.min(grid_max.y);
    }

    // Every chunk entity counts, parented yet or not.
    let existing_chunks: HashSet<IVec2> = chunk_query.iter().map(|chunk| chunk.position).collect();

    // The chunks bodies need, however far they are from the camera. Each
    // has just been found to hold something, so the empty-chunk entries are
    // not consulted for them.
    #[cfg(feature = "physics")]
    let for_bodies = if radius.is_some() || filling {
        body_chunks(
            bodies.iter().filter(|(body, _)| body_needs_ground(body)).map(|(_, transform)| transform.translation()),
            body_collider_margin(config),
            config,
            data,
            volume,
        )
    } else {
        BodyChunks::default()
    };
    #[cfg(not(feature = "physics"))]
    let for_bodies = BodyChunks::default();

    let mut candidates: Vec<(IVec2, SpawnPriority, f32)> = Vec::new();
    for &chunk_pos in &for_bodies.near {
        if existing_chunks.contains(&chunk_pos) {
            continue;
        }
        let priority = if for_bodies.under.contains(&chunk_pos) {
            SpawnPriority::UnderBody
        } else {
            SpawnPriority::NearBody
        };
        // During the fill the queue brings everything but the ground under
        // bodies.
        if filling && priority != SpawnPriority::UnderBody {
            continue;
        }
        candidates.push((chunk_pos, priority, chunk_xz_distance(chunk_pos, config, origin)));
    }
    // The chunks in view; during the fill the queue brings them.
    if !filling {
        for cx in min_x..=max_x {
            for cz in min_z..=max_z {
                let chunk_pos = IVec2::new(cx, cz);
                if existing_chunks.contains(&chunk_pos)
                    || empty_chunks.contains(chunk_pos)
                    || for_bodies.near.contains(&chunk_pos)
                {
                    continue;
                }
                // Measured even when every chunk qualifies, so the backlog
                // still fills nearest first.
                let distance = chunk_xz_distance(chunk_pos, config, origin);
                if radius.map_or(true, |r| distance <= r) {
                    candidates.push((chunk_pos, SpawnPriority::InView, distance));
                }
            }
        }
    }

    if candidates.is_empty() {
        throttle.backlog = false;
        return;
    }

    // Bodies' chunks first, those they are over ahead of the rest of their
    // margin, then the chunks in view; closest first within each, so the most
    // visible chunks spawn first, and by position between equal distances.
    sort_spawn_candidates(&mut candidates);

    // More chunks under bodies are missing than a frame spawns past its
    // budget: say so once, since the ground under the rest follows a frame
    // or more later.
    let under_bodies = candidates.iter().take_while(|(_, priority, _)| *priority == SpawnPriority::UnderBody).count();
    if under_bodies > MAX_UNBUDGETED_SPAWNS_PER_FRAME && !*under_cap_logged {
        *under_cap_logged = true;
        tracing::warn!(
            "Terrain streaming: {under_bodies} chunks under bodies are missing, more than the \
             {MAX_UNBUDGETED_SPAWNS_PER_FRAME} a frame spawns past its budget; the rest follow over the next frames"
        );
    }

    // The root's material, else a surviving chunk's, else a fresh one kept
    // on the root. A terrain the loader spawned without `spawn_terrain`
    // (imported voxels) starts with none, and a mesh without a material is
    // never drawn.
    let material = match root_material {
        Some(material) => material.0.clone(),
        None => {
            let handle = children
                .and_then(|children| children.iter().find_map(|child| chunk_materials.get(child).ok()))
                .map(|material| material.0.clone())
                .unwrap_or_else(|| new_terrain_chunk_material(config, &mut materials));
            commands.entity(terrain_entity).insert(TerrainChunkMaterial(handle.clone()));
            handle
        }
    };

    let mut budget = SpawnBudget::default();
    let mut decided = 0;
    for &(chunk_pos, priority, _) in &candidates {
        let admission = budget.admit(priority);
        if admission == SpawnAdmission::Stop {
            break;
        }
        decided += 1;
        if !chunk_should_spawn(chunk_pos, config, data, volume) {
            empty_chunks.insert(chunk_pos);
            budget.scan_empty(empty_chunk_scan_cells(config));
            continue;
        }
        let lod = config.lod_for_distance(chunk_lod_distance(chunk_pos, config, data, camera_pos));
        if admission == SpawnAdmission::Unbudgeted {
            budget.spend_unbudgeted();
        } else {
            budget.spend(chunk_spawn_cost(chunk_pos, lod, config, data, volume));
        }
        let (mesh_handle, surface) =
            generate_chunk_render_mesh_and_surface(chunk_pos, lod, config, data, volume, &mut meshes);
        let chunk_entity = commands.spawn((
            Chunk {
                position: chunk_pos,
                lod,
            },
            Mesh3d(mesh_handle),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(chunk_world_position(chunk_pos, config)),
            Visibility::default(),
            Name::new(format!("Chunk_{}_{}", chunk_pos.x, chunk_pos.y)),
            ChildOf(terrain_entity),
        )).id();
        attach_chunk_collider(&mut commands, chunk_entity, chunk_pos, config, data, volume, surface);
    }
    throttle.backlog = decided < candidates.len();
}

/// Seconds between cull passes; chunks leave view slowly relative to a frame.
const CULL_INTERVAL_SECS: f64 = 0.25;

/// How far past the distance that spawns a chunk a cull pass lets it stay, as
/// a factor, so a chunk at the edge does not pop out and back in as the
/// camera or a body moves: the streaming radius for the camera, the body
/// margin for a body.
pub const CULL_HYSTERESIS: f32 = 1.2;

/// Whether a cull pass despawns the chunk at `chunk_pos`: it lies farther
/// than `cull_distance` from `origin` in XZ ([`chunk_xz_distance`]) and is not
/// among the chunks bodies keep (`kept`).
pub fn chunk_should_cull(
    chunk_pos: IVec2,
    config: &TerrainConfig,
    origin: Vec3,
    cull_distance: f32,
    kept: &HashSet<IVec2>,
) -> bool {
    chunk_xz_distance(chunk_pos, config, origin) > cull_distance && !kept.contains(&chunk_pos)
}

/// System to cull chunks outside the streaming radius
///
/// A raster terrain small enough to stay resident is never culled: its
/// chunks own the static heightfield colliders that bodies rest on, and
/// despawning one would drop whatever stands there. A streaming terrain culls
/// a chunk once it lies [`CULL_HYSTERESIS`] times the streaming radius from
/// the origin ([`chunk_stream_radius`]: `view_distance` for procedural
/// terrain and for an oversized raster whose far field draws, else a radius
/// reaching the raster's farthest corner), unless it holds ground within
/// [`CULL_HYSTERESIS`] times [`body_collider_margin`] of a dynamic or
/// kinematic body ([`chunk_should_cull`]). Despawning a chunk takes its
/// collider child with it; edits live in `TerrainData` on the root, so a
/// chunk streamed back in rebuilds from them.
pub fn chunk_cull_system(
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<
        (
            &TerrainConfig,
            &TerrainData,
            Option<&TerrainVolume>,
            Option<&TerrainBaked>,
            Option<&TerrainFarFieldActive>,
        ),
        With<TerrainRoot>,
    >,
    chunk_query: Query<(Entity, &Chunk)>,
    time: Res<Time>,
    mut last_cull: Local<f64>,
    #[cfg(feature = "physics")] bodies: Query<(&RigidBody, &GlobalTransform)>,
) {
    let Some(camera_pos) = scene_camera_translation(&cameras) else { return };
    let Ok((config, data, volume, baked, far_field)) = terrain_query.single() else { return };

    let now = time.elapsed_secs_f64();
    if now - *last_cull < CULL_INTERVAL_SECS {
        return;
    }
    *last_cull = now;

    let data = surface_data(data, baked);
    let origin = chunk_streaming_origin(config, data, camera_pos);
    let Some(radius) = chunk_stream_radius(config, data, origin, far_field.is_some()) else { return };
    let cull_distance = radius * CULL_HYSTERESIS;

    // The chunks bodies keep, over their margin widened like the radius.
    #[cfg(feature = "physics")]
    let kept = body_chunks(
        bodies.iter().filter(|(body, _)| body_needs_ground(body)).map(|(_, transform)| transform.translation()),
        body_collider_margin(config) * CULL_HYSTERESIS,
        config,
        data,
        volume.unwrap_or(TerrainVolume::empty()),
    )
    .near;
    #[cfg(not(feature = "physics"))]
    let kept = {
        let _ = volume;
        HashSet::new()
    };

    for (entity, chunk) in chunk_query.iter() {
        if chunk_should_cull(chunk.position, config, origin, cull_distance, &kept) {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 64.0,
            chunks_x: 2,
            chunks_z: 2,
            ..TerrainConfig::default()
        }
    }

    #[test]
    fn xz_distance_is_zero_over_the_chunk_and_ignores_height() {
        let config = config();
        // Chunk (1, 0) spans x 64..128, z 0..64 from its corner.
        assert_eq!(chunk_xz_distance(IVec2::new(1, 0), &config, Vec3::new(100.0, 900.0, 10.0)), 0.0);
        assert_eq!(chunk_xz_distance(IVec2::new(1, 0), &config, Vec3::new(40.0, 0.0, 30.0)), 24.0);
        let corner = chunk_xz_distance(IVec2::new(1, 0), &config, Vec3::new(131.0, 0.0, 68.0));
        assert!((corner - 5.0).abs() < 1e-5, "3-4-5 from the far corner, got {corner}");
    }

    #[test]
    fn streaming_origin_clamps_onto_a_raster_terrain_only() {
        let config = config();
        let far = Vec3::new(5_000.0, 50.0, -5_000.0);

        // Procedural: no edge, stream around the camera itself.
        let procedural = TerrainData::procedural();
        assert_eq!(chunk_streaming_origin(&config, &procedural, far), far);

        // Raster-backed: pulled onto the footprint (x -128..192, z -128..192).
        let mut raster = TerrainData::procedural();
        raster.resize_cache(&config);
        assert_eq!(chunk_streaming_origin(&config, &raster, far), Vec3::new(192.0, 50.0, -128.0));
        let inside = Vec3::new(10.0, 5.0, 20.0);
        assert_eq!(chunk_streaming_origin(&config, &raster, inside), inside);
    }

    #[test]
    fn a_small_raster_stays_resident_and_procedural_streams_view_distance() {
        let config = TerrainConfig {
            chunk_size: 64.0,
            chunks_x: 4,
            chunks_z: 4,
            view_distance: 256.0,
            ..TerrainConfig::default()
        };
        let corner = Vec3::new(300.0, 10.0, 300.0);

        // 9 x 9 chunks: the whole grid spawns and nothing is culled, even
        // with the origin on the footprint's far corner.
        let mut raster = TerrainData::procedural();
        raster.resize_cache(&config);
        assert!(raster_fully_resident(&config, &raster));
        assert_eq!(chunk_stream_radius(&config, &raster, Vec3::ZERO, false), None);
        assert_eq!(chunk_stream_radius(&config, &raster, corner, false), None);

        let procedural = TerrainData::procedural();
        assert!(!raster_fully_resident(&config, &procedural));
        assert_eq!(chunk_stream_radius(&config, &procedural, corner, false), Some(256.0));
    }

    #[test]
    fn an_oversized_raster_streams_far_enough_to_reach_every_corner() {
        // 33 x 33 chunks is past the resident cap.
        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 4,
            chunks_x: 16,
            chunks_z: 16,
            view_distance: 256.0,
            ..TerrainConfig::default()
        };
        let mut raster = TerrainData::procedural();
        raster.resize_cache(&config);
        assert!(!raster_fully_resident(&config, &raster));

        // Footprint x and z -256..272; from the min corner the far corner
        // is 528 m away on both axes.
        let origin = chunk_streaming_origin(&config, &raster, Vec3::new(-900.0, 0.0, -900.0));
        let radius = chunk_stream_radius(&config, &raster, origin, false).expect("an oversized raster streams");
        assert!((radius - 528.0 * std::f32::consts::SQRT_2).abs() < 1e-2, "got {radius}");
        let far = chunk_xz_distance(IVec2::new(16, 16), &config, origin);
        assert!(far <= radius, "far corner chunk at {far} lies outside {radius}");
    }

    /// 33 x 33 chunks of 16 m at 4 cells, past the resident cap: footprint x
    /// and z -256..272, streamed 64 m out while the far field draws.
    fn streaming_raster() -> (TerrainConfig, TerrainData) {
        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 4,
            chunks_x: 16,
            chunks_z: 16,
            view_distance: 64.0,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        (config, data)
    }

    #[test]
    fn the_far_field_holds_a_streaming_raster_to_its_view_distance() {
        use crate::terrain::far_field::farthest_corner_distance;

        let (config, raster) = streaming_raster();
        assert!(!raster_fully_resident(&config, &raster));
        // Over the middle, beyond a corner, and past one edge.
        for camera in [Vec3::new(8.0, 30.0, 8.0), Vec3::new(-900.0, 0.0, -900.0), Vec3::new(400.0, 5.0, -3.0)] {
            let origin = chunk_streaming_origin(&config, &raster, camera);
            assert_eq!(chunk_stream_radius(&config, &raster, origin, true), Some(64.0), "camera {camera}");
            let full = chunk_stream_radius(&config, &raster, origin, false).expect("an oversized raster streams");
            let farthest = farthest_corner_distance(&config, Vec2::new(origin.x, origin.z));
            assert!((full - farthest).abs() < 1e-2, "camera {camera}: {full} m, the farthest corner is {farthest} m");
        }

        // A raster small enough to stay resident keeps every chunk, and
        // procedural terrain streams its view distance, far field or not.
        let small = TerrainConfig { chunks_x: 4, chunks_z: 4, ..config.clone() };
        let mut resident = TerrainData::procedural();
        resident.resize_cache(&small);
        assert!(raster_fully_resident(&small, &resident));
        let procedural = TerrainData::procedural();
        for far_field in [false, true] {
            assert_eq!(chunk_stream_radius(&small, &resident, Vec3::new(300.0, 0.0, 300.0), far_field), None);
            assert_eq!(chunk_stream_radius(&config, &procedural, Vec3::new(5_000.0, 0.0, 0.0), far_field), Some(64.0));
        }
    }

    #[test]
    fn the_streamed_chunks_cover_the_disc_the_far_field_leaves_to_them() {
        use crate::terrain::far_field::far_field_near_radius;

        let (config, raster) = streaming_raster();
        let near_radius = far_field_near_radius(&config);
        assert_eq!(near_radius, 48.0);
        let (min, max) = config.footprint_xz();
        // The disc trails the camera by up to an eighth of a chunk.
        let trail = config.chunk_size / 8.0;
        // Over the middle, and beyond an edge and a corner, where the origin
        // is pulled onto the footprint but the disc stays on the camera.
        for camera in [Vec3::new(8.0, 30.0, 8.0), Vec3::new(-290.0, 10.0, 100.0), Vec3::new(290.0, 0.0, 290.0)] {
            let origin = chunk_streaming_origin(&config, &raster, camera);
            let radius = chunk_stream_radius(&config, &raster, origin, true).expect("an oversized raster streams");
            for offset in [Vec2::ZERO, Vec2::new(trail, 0.0), Vec2::new(-0.7, 0.7) * trail] {
                let centre = Vec2::new(camera.x, camera.z) + offset;
                let mut covered = 0;
                for i in -24..=24 {
                    for j in -24..=24 {
                        let point = centre + Vec2::new(i as f32, j as f32) * 2.0;
                        let inside = point.x >= min.x && point.x < max.x && point.y >= min.y && point.y < max.y;
                        if !inside || point.distance(centre) > near_radius {
                            continue;
                        }
                        let chunk = (point / config.chunk_size).floor().as_ivec2();
                        let distance = chunk_xz_distance(chunk, &config, origin);
                        assert!(distance <= radius, "camera {camera}: {point} of the near disc is on chunk {chunk}, {distance} m out");
                        covered += 1;
                    }
                }
                assert!(covered > 0, "camera {camera}: the near disc reaches the footprint");
            }
        }
    }

    #[test]
    fn lod_distance_measures_to_the_ground_under_the_nearest_point() {
        let config = config();
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        // A flat raster at normalized 0.5 is world Y = 25 with the default band.
        data.height_cache.iter_mut().for_each(|h| *h = 0.5);
        let ground = config.world_height(0.5);

        // Straight above the chunk: only the height above the ground counts.
        let above = chunk_lod_distance(IVec2::new(0, 0), &config, &data, Vec3::new(32.0, ground + 40.0, 32.0));
        assert!((above - 40.0).abs() < 1e-3, "got {above}");

        // Beside it: horizontal gap to the footprint edge, at ground level.
        let beside = chunk_lod_distance(IVec2::new(0, 0), &config, &data, Vec3::new(-30.0, ground, 10.0));
        assert!((beside - 30.0).abs() < 1e-3, "got {beside}");
    }

    /// 3 x 3 chunks of 16 m at 16 cells, so a volume brick (16 lattice cells)
    /// spans exactly one chunk: a 48 x 48 raster whose every cell is a hole of
    /// a sparse surface.
    fn all_holes() -> (TerrainConfig, TerrainData) {
        use crate::terrain::material::MATERIAL_SLOT_NONE;

        let config = TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            ..TerrainConfig::default()
        };
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        data.material_cache = vec![[MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]; data.height_cache.len()];
        data.sparse_surface = true;
        (config, data)
    }

    #[test]
    fn only_a_chunk_with_ground_or_bricks_spawns() {
        use crate::terrain::material::material_cell;
        use crate::terrain::volume::{apply_sphere, CsgOp};
        use crate::terrain::TerrainMaterial;

        let (config, mut data) = all_holes();
        let bare = TerrainVolume::new();
        for chunk in config.grid_chunks() {
            assert!(!chunk_should_spawn(chunk, &config, &data, &bare), "{chunk} is all holes");
        }

        // A block added over the holes of chunk (0, 0) spawns it. Its brick
        // owns the chunk's columns and the -X and -Z border columns it shares,
        // but no column of (-1, 1) or (1, 0).
        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::new(8.0, 5.0, 8.0), 2.0, CsgOp::Add, None);
        assert!(!edit.is_empty());
        assert!(chunk_should_spawn(IVec2::ZERO, &config, &data, &volume));
        assert!(!chunk_should_spawn(IVec2::new(-1, 1), &config, &data, &volume));
        assert!(!chunk_should_spawn(IVec2::new(1, 0), &config, &data, &volume));

        // One cell of ground at column 40, row 20: tile (2, 1), chunk (1, 0).
        let w = data.cache_width as usize;
        data.material_cache[20 * w + 40] = material_cell(TerrainMaterial::Grass.to_u8());
        assert!(chunk_should_spawn(IVec2::new(1, 0), &config, &data, &bare));
        assert!(!chunk_should_spawn(IVec2::ZERO, &config, &data, &bare));
        assert!(!chunk_should_spawn(IVec2::new(5, 5), &config, &data, &bare), "off the grid");

        // A full surface spawns every chunk, whatever its materials.
        data.sparse_surface = false;
        for chunk in config.grid_chunks().chain([IVec2::new(5, 5)]) {
            assert!(chunk_should_spawn(chunk, &config, &data, &bare), "{chunk}");
        }
    }

    #[test]
    fn empty_chunks_are_forgotten_where_the_ground_changes() {
        let (config, data) = all_holes();
        let mut world = World::new();
        let (root, other) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut dirty = TerrainDirtyChunks::default();
        let mut empty = TerrainEmptyChunks::default();
        let remembered = |empty: &TerrainEmptyChunks| config.grid_chunks().filter(|chunk| empty.contains(*chunk)).count();
        let fill = |empty: &mut TerrainEmptyChunks| config.grid_chunks().for_each(|chunk| empty.insert(chunk));

        let written = [Some(Tick::new(1)), None, None];
        empty.sync(root, &config, &data, written, Some(&dirty));
        fill(&mut empty);
        empty.sync(root, &config, &data, written, Some(&dirty));
        assert_eq!(remembered(&empty), 9, "nothing changed");

        // An edit marked on chunk (1, 0) forgets that chunk alone.
        dirty.mark(IVec2::new(1, 0));
        let written = [Some(Tick::new(2)), None, None];
        empty.sync(root, &config, &data, written, Some(&dirty));
        assert!(!empty.contains(IVec2::new(1, 0)));
        assert_eq!(remembered(&empty), 8);

        // A write no mark explains, such as a replaced raster, forgets all.
        let written = [Some(Tick::new(3)), None, None];
        empty.sync(root, &config, &data, written, Some(&dirty));
        assert_eq!(remembered(&empty), 0);

        // So does a volume arriving unmarked, a mark on every chunk, another
        // root and another grid.
        fill(&mut empty);
        let with_volume = [Some(Tick::new(3)), None, Some(Tick::new(4))];
        empty.sync(root, &config, &data, with_volume, Some(&dirty));
        assert_eq!(remembered(&empty), 0, "a volume appeared");
        fill(&mut empty);
        dirty.mark_all(&config);
        empty.sync(root, &config, &data, with_volume, Some(&dirty));
        assert_eq!(remembered(&empty), 0, "every chunk was marked");
        fill(&mut empty);
        empty.sync(other, &config, &data, with_volume, Some(&dirty));
        assert_eq!(remembered(&empty), 0, "another root");
        fill(&mut empty);
        let moved = TerrainConfig { center_chunk: IVec2::new(0, 1), ..config.clone() };
        empty.sync(other, &moved, &data, with_volume, Some(&dirty));
        assert!(!empty.contains(IVec2::ZERO), "another grid");
    }

    #[test]
    fn the_body_margin_is_a_chunk_and_never_under_eight_metres() {
        let with_size = |chunk_size: f32| body_collider_margin(&TerrainConfig { chunk_size, ..TerrainConfig::default() });
        assert_eq!(with_size(64.0), 64.0);
        assert_eq!(with_size(4.0), MIN_BODY_COLLIDER_MARGIN);
        assert_eq!(with_size(f32::NAN), MIN_BODY_COLLIDER_MARGIN);
        assert_eq!(with_size(f32::INFINITY), MIN_BODY_COLLIDER_MARGIN);
    }

    /// Every chunk of `config`'s grid within `margin` of one of `bodies`,
    /// found the slow way.
    fn grid_chunks_within(config: &TerrainConfig, bodies: &[Vec3], margin: f32) -> HashSet<IVec2> {
        config
            .grid_chunks()
            .filter(|chunk| bodies.iter().any(|body| chunk_xz_distance(*chunk, config, *body) <= margin))
            .collect()
    }

    #[test]
    fn a_body_keeps_every_chunk_within_the_margin_on_the_grid() {
        let (config, raster) = streaming_raster();
        let bare = TerrainVolume::new();
        let margin = body_collider_margin(&config);
        assert_eq!(margin, 16.0);

        // Over chunk (6, -4), x 96..112 and z -64..-48: 4 m from its -X edge,
        // 12 m from +X, 14 m from -Z and 2 m from +Z.
        let body = Vec3::new(100.0, 7.0, -50.0);
        let chunks = body_chunks([body], margin, &config, &raster, &bare);
        assert_eq!(chunks.under, HashSet::from([IVec2::new(6, -4)]));
        assert_eq!(chunks.near, grid_chunks_within(&config, &[body], margin));
        assert_eq!(chunks.near.len(), 8, "the 3 x 3 block around it less one corner");
        assert!(!chunks.near.contains(&IVec2::new(7, -5)), "sqrt(12^2 + 14^2) = 18.4 m away");
        assert!(chunks.near.contains(&IVec2::new(5, -5)), "sqrt(4^2 + 14^2) = 14.6 m away");

        // On the footprint's far corner nothing off the grid is kept, but
        // procedural terrain has no edge.
        let corner = Vec3::new(271.0, 0.0, 271.0);
        let chunks = body_chunks([corner], margin, &config, &raster, &bare);
        assert_eq!(chunks.near, HashSet::from([IVec2::new(16, 16), IVec2::new(15, 16), IVec2::new(16, 15)]));
        assert_eq!(chunks.near, grid_chunks_within(&config, &[corner], margin));
        let unbounded = body_chunks([corner], margin, &config, &TerrainData::procedural(), &bare);
        assert!(unbounded.near.is_superset(&chunks.near));
        assert!(unbounded.near.contains(&IVec2::new(17, 16)) && unbounded.near.contains(&IVec2::new(17, 17)));

        // A body off the raster keeps nothing, and a position that is not a
        // number is skipped.
        let off = body_chunks([Vec3::new(5_000.0, 0.0, 5_000.0)], margin, &config, &raster, &bare);
        assert_eq!(off, BodyChunks::default());
        let broken = [Vec3::NAN, Vec3::new(f32::INFINITY, 0.0, 0.0), Vec3::new(0.0, 0.0, f32::NEG_INFINITY)];
        assert_eq!(body_chunks(broken, margin, &config, &raster, &bare), BodyChunks::default());
    }

    #[test]
    fn a_body_over_holes_needs_no_chunk_there_unless_bricks_reach_it() {
        use crate::terrain::material::material_cell;
        use crate::terrain::volume::{apply_sphere, CsgOp};
        use crate::terrain::TerrainMaterial;

        // Every chunk of the 3 x 3 grid lies within the margin of a body over
        // the middle of chunk (0, 0), and every one is all holes.
        let (config, mut data) = all_holes();
        let margin = body_collider_margin(&config);
        let body = Vec3::new(8.0, 20.0, 8.0);
        assert_eq!(grid_chunks_within(&config, &[body], margin).len(), 9);
        let bare = TerrainVolume::new();
        assert_eq!(body_chunks([body], margin, &config, &data, &bare), BodyChunks::default(), "nothing to stand on");

        // A block added over the holes of chunk (0, 0) holds it up; (1, 0)
        // and (-1, 1) still hold nothing.
        let mut volume = TerrainVolume::new();
        let edit = apply_sphere(&config, &mut volume, Vec3::new(8.0, 5.0, 8.0), 2.0, CsgOp::Add, None);
        assert!(!edit.is_empty());
        let chunks = body_chunks([body], margin, &config, &data, &volume);
        assert!(chunks.under.contains(&IVec2::ZERO) && chunks.near.contains(&IVec2::ZERO));
        assert!(!chunks.near.contains(&IVec2::new(1, 0)) && !chunks.near.contains(&IVec2::new(-1, 1)));

        // One cell of ground in chunk (1, 0) gives it something to hold.
        let w = data.cache_width as usize;
        data.material_cache[20 * w + 40] = material_cell(TerrainMaterial::Grass.to_u8());
        let chunks = body_chunks([body], margin, &config, &data, &bare);
        assert_eq!(chunks.near, HashSet::from([IVec2::new(1, 0)]));
        assert!(chunks.under.is_empty(), "the body is still over the holes of (0, 0)");
    }

    #[test]
    fn bodies_over_one_chunk_are_walked_once_and_keep_all_each_needs() {
        let (config, raster) = streaming_raster();
        let bare = TerrainVolume::new();
        let margin = body_collider_margin(&config);

        // Two thousand bodies piled over chunk (2, 3) (x 32..48, z 48..64),
        // and one alone over chunk (-8, 12).
        let pile: Vec<Vec3> = (0..2000)
            .map(|i| Vec3::new(33.0 + (i % 40) as f32 * 0.3, i as f32 * 0.01, 50.0 + (i / 40) as f32 * 0.25))
            .collect();
        let alone = Vec3::new(-120.0, 3.0, 200.0);
        let bodies: Vec<Vec3> = pile.iter().copied().chain([alone]).collect();
        let chunks = body_chunks(bodies.iter().copied(), margin, &config, &raster, &bare);
        assert_eq!(chunks.under, HashSet::from([IVec2::new(2, 3), IVec2::new(-8, 12)]));

        // Every chunk each body needs is kept, and nothing past the margin of
        // the rectangle the pile spans or of the lone body.
        assert!(chunks.near.is_superset(&grid_chunks_within(&config, &bodies, margin)));
        let lo = pile.iter().fold(Vec2::MAX, |lo, p| lo.min(Vec2::new(p.x, p.z)));
        let hi = pile.iter().fold(Vec2::MIN, |hi, p| hi.max(Vec2::new(p.x, p.z)));
        for chunk in &chunks.near {
            let by_pile = chunk_xz_distance_to_rect(*chunk, &config, lo, hi) <= margin;
            let by_alone = chunk_xz_distance(*chunk, &config, alone) <= margin;
            assert!(by_pile || by_alone, "{chunk} is past every body's margin");
        }
        let alone_only = body_chunks([alone], margin, &config, &raster, &bare);
        assert_eq!(alone_only.near, grid_chunks_within(&config, &[alone], margin));
        assert!(chunks.near.is_superset(&alone_only.near));
    }

    #[test]
    fn a_cull_pass_keeps_chunks_in_reach_and_those_bodies_hold() {
        let (config, _) = streaming_raster();
        let origin = Vec3::new(8.0, 30.0, 8.0);
        let cull_distance = config.view_distance * CULL_HYSTERESIS;
        let kept = HashSet::from([IVec2::new(12, -13)]);
        let culled = |chunk: IVec2| chunk_should_cull(chunk, &config, origin, cull_distance, &kept);
        // (5, 0) spans x 80..96, 72 m out; (6, 0) starts 88 m out, past 76.8.
        assert!(!culled(IVec2::ZERO) && !culled(IVec2::new(5, 0)));
        assert!(culled(IVec2::new(6, 0)) && culled(IVec2::new(12, -12)));
        assert!(!culled(IVec2::new(12, -13)), "a body holds it");
        // A distance that is not a number culls nothing.
        assert!(!chunk_should_cull(IVec2::new(6, 0), &config, origin, f32::NAN, &HashSet::new()));
    }

    /// The admissions a spawn pass makes over `candidates` (priority, and
    /// what spawning each costs, `None` for a chunk with nothing to draw),
    /// taken in order until it stops, as `chunk_spawn_system` takes them.
    fn spawn_pass(candidates: &[(SpawnPriority, Option<usize>)]) -> Vec<SpawnAdmission> {
        let mut budget = SpawnBudget::default();
        let mut taken = Vec::new();
        for &(priority, cost) in candidates {
            let admission = budget.admit(priority);
            if admission == SpawnAdmission::Stop {
                break;
            }
            match (cost, admission) {
                (None, _) => budget.scan_empty(EMPTY_CHUNK_SCAN_CELLS_PER_FRAME),
                (Some(_), SpawnAdmission::Unbudgeted) => budget.spend_unbudgeted(),
                (Some(cost), _) => budget.spend(cost),
            }
            taken.push(admission);
        }
        taken
    }

    #[test]
    fn chunks_under_bodies_spawn_first_and_past_the_budget_up_to_a_cap() {
        use super::SpawnAdmission::{Budgeted, Unbudgeted};
        use super::SpawnPriority::{InView, NearBody, UnderBody};
        use crate::terrain::VOLUMETRIC_CHUNK_COST;

        // Priority first, then distance, then position.
        let mut candidates = vec![
            (IVec2::new(0, 0), InView, 1.0),
            (IVec2::new(4, 4), NearBody, 50.0),
            (IVec2::new(9, 9), UnderBody, 300.0),
            (IVec2::new(-1, 0), InView, 1.0),
            (IVec2::new(8, 8), UnderBody, 100.0),
            (IVec2::new(3, 3), InView, f32::NAN),
        ];
        sort_spawn_candidates(&mut candidates);
        let order: Vec<IVec2> = candidates.iter().map(|(chunk, _, _)| *chunk).collect();
        assert_eq!(
            order,
            [IVec2::new(8, 8), IVec2::new(9, 9), IVec2::new(4, 4), IVec2::new(-1, 0), IVec2::new(0, 0), IVec2::new(3, 3)]
        );

        // Seventy chunks under bodies: the cap spawns past the budget, the
        // next two take the budget, and the margin and the view wait.
        let mut crowd = vec![(UnderBody, Some(1)); 70];
        crowd.extend([(NearBody, Some(1)), (InView, Some(1))]);
        let taken = spawn_pass(&crowd);
        assert_eq!(taken.len(), MAX_UNBUDGETED_SPAWNS_PER_FRAME + MAX_SPAWNS_PER_FRAME);
        assert!(taken[..MAX_UNBUDGETED_SPAWNS_PER_FRAME].iter().all(|admission| *admission == Unbudgeted));
        assert!(taken[MAX_UNBUDGETED_SPAWNS_PER_FRAME..].iter().all(|admission| *admission == Budgeted));

        // A volumetric chunk under a body costs the budget nothing, so the
        // margin still gets its two heightfield chunks.
        let taken = spawn_pass(&[
            (UnderBody, Some(VOLUMETRIC_CHUNK_COST)),
            (NearBody, Some(1)),
            (NearBody, Some(1)),
            (InView, Some(1)),
        ]);
        assert_eq!(taken, [Unbudgeted, Budgeted, Budgeted]);

        // Without bodies the first chunk is always decided, however costly.
        assert_eq!(spawn_pass(&[(InView, Some(VOLUMETRIC_CHUNK_COST)), (InView, Some(1))]), [Budgeted]);

        // Empty chunks that used up the scan budget stop the view, not the
        // chunks under bodies.
        let budget = SpawnBudget { scanned: EMPTY_CHUNK_SCAN_CELLS_PER_FRAME, ..SpawnBudget::default() };
        assert_eq!(budget.admit(InView), SpawnAdmission::Stop);
        assert_eq!(budget.admit(UnderBody), Unbudgeted);
        assert_eq!(spawn_pass(&[(InView, None), (InView, Some(1))]), [Budgeted]);
    }

    /// Where [`streaming_world`] puts its camera: over chunk (0, 0), inside
    /// the footprint, so it is also the streaming origin.
    const CAMERA: Vec3 = Vec3::new(8.0, 30.0, 8.0);

    /// A world holding the streaming raster, a camera at [`CAMERA`] and the
    /// resources the chunk systems read, a second into its run.
    fn streaming_world() -> (World, Entity) {
        let (config, data) = streaming_raster();
        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<TerrainGenerationQueue>();
        world.init_resource::<ChunkSpawnThrottle>();
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
        world.spawn((Camera3d::default(), Camera::default(), GlobalTransform::from_translation(CAMERA)));
        let root = world.spawn((TerrainRoot, config, data)).id();
        (world, root)
    }

    /// Positions of the chunk entities in `world`.
    fn resident(world: &mut World) -> HashSet<IVec2> {
        let mut chunks = world.query::<&Chunk>();
        chunks.iter(world).map(|chunk| chunk.position).collect()
    }

    #[test]
    fn with_the_far_field_the_spawn_scan_stops_at_the_view_distance() {
        let (mut world, root) = streaming_world();
        let config = world.get::<TerrainConfig>(root).expect("the root").clone();
        world.entity_mut(root).insert(TerrainFarFieldActive { near_radius: 48.0 });
        let spawn = world.register_system(chunk_spawn_system);
        for _ in 0..64 {
            assert!(world.run_system(spawn).is_ok(), "the spawn system runs");
        }
        let in_view: HashSet<IVec2> = config
            .grid_chunks()
            .filter(|chunk| chunk_xz_distance(*chunk, &config, CAMERA) <= config.view_distance)
            .collect();
        assert_eq!(resident(&mut world), in_view);

        // Without the far field the scan reaches past the view distance.
        world.entity_mut(root).remove::<TerrainFarFieldActive>();
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
        assert!(world.run_system(spawn).is_ok());
        let past = resident(&mut world).into_iter().any(|chunk| chunk_xz_distance(chunk, &config, CAMERA) > config.view_distance);
        assert!(past, "every chunk of the map streams again");
    }

    #[test]
    fn with_the_far_field_the_cull_pass_takes_chunks_past_the_view_distance() {
        let (mut world, root) = streaming_world();
        for position in [IVec2::ZERO, IVec2::new(5, 0), IVec2::new(6, 0), IVec2::new(-16, 16)] {
            world.spawn(Chunk { position, lod: 0 });
        }
        let cull = world.register_system(chunk_cull_system);

        // Without the far field the radius reaches every corner: nothing goes.
        assert!(world.run_system(cull).is_ok(), "the cull system runs");
        assert_eq!(resident(&mut world).len(), 4);

        // With it, whatever lies past 1.2 view distances goes.
        world.entity_mut(root).insert(TerrainFarFieldActive { near_radius: 48.0 });
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
        assert!(world.run_system(cull).is_ok());
        assert_eq!(resident(&mut world), HashSet::from([IVec2::ZERO, IVec2::new(5, 0)]));
    }

    #[cfg(feature = "physics")]
    #[test]
    fn bodies_get_their_ground_first_wherever_the_camera_is() {
        let (mut world, root) = streaming_world();
        let config = world.get::<TerrainConfig>(root).expect("the root").clone();
        world.entity_mut(root).insert(TerrainFarFieldActive { near_radius: 48.0 });
        // A crate falling far to one side, over chunk (12, -13), the player (a
        // kinematic body) far to the other, over chunk (-10, 2), and an
        // anchored part, which needs no ground.
        let falling = Vec3::new(200.0, 20.0, -200.0);
        let player = Vec3::new(-150.0, 2.0, 40.0);
        let anchored = Vec3::new(0.0, 0.0, 250.0);
        for (body, at) in [(RigidBody::Dynamic, falling), (RigidBody::Kinematic, player), (RigidBody::Static, anchored)] {
            world.spawn((body, Transform::from_translation(at), GlobalTransform::from_translation(at)));
        }
        let spawn = world.register_system(chunk_spawn_system);

        // The first scan spawns the chunks the bodies are over past its
        // budget, and spends the budget on the rest of their margin, nearest
        // the camera first: (-9, 1) and (-9, 2) beside the player, 136.2 m
        // and 138.1 m out, where the view and the crate's margin wait.
        assert!(world.run_system(spawn).is_ok(), "the spawn system runs");
        assert_eq!(
            resident(&mut world),
            HashSet::from([IVec2::new(12, -13), IVec2::new(-10, 2), IVec2::new(-9, 1), IVec2::new(-9, 2)])
        );

        for _ in 0..128 {
            assert!(world.run_system(spawn).is_ok());
        }
        let margin = body_collider_margin(&config);
        let expected: HashSet<IVec2> = config
            .grid_chunks()
            .filter(|chunk| {
                chunk_xz_distance(*chunk, &config, CAMERA) <= config.view_distance
                    || [falling, player].iter().any(|body| chunk_xz_distance(*chunk, &config, *body) <= margin)
            })
            .collect();
        assert_eq!(resident(&mut world), expected, "the view, and the ground around the two moving bodies");
    }

    #[cfg(feature = "physics")]
    #[test]
    fn a_cull_pass_leaves_bodies_their_ground() {
        use bevy::ecs::system::RunSystemOnce;

        let (mut world, root) = streaming_world();
        world.entity_mut(root).insert(TerrainFarFieldActive { near_radius: 48.0 });
        let falling = Vec3::new(200.0, 20.0, -200.0);
        let anchored = Vec3::new(-200.0, 0.0, 200.0);
        world.spawn((RigidBody::Dynamic, Transform::from_translation(falling), GlobalTransform::from_translation(falling)));
        world.spawn((RigidBody::Static, Transform::from_translation(anchored), GlobalTransform::from_translation(anchored)));
        for position in [IVec2::new(12, -13), IVec2::new(13, -13), IVec2::new(14, -13), IVec2::new(-13, 12)] {
            world.spawn(Chunk { position, lod: 0 });
        }
        world.run_system_once(chunk_cull_system).expect("the cull system runs");
        // (12, -13) is under the crate and (13, -13) 8 m from it; (14, -13),
        // 24 m out, is past 1.2 margins, and the anchored part holds nothing.
        assert_eq!(resident(&mut world), HashSet::from([IVec2::new(12, -13), IVec2::new(13, -13)]));
    }
}
