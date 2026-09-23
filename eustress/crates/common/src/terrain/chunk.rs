//! Terrain chunk component and the streaming systems (spawn and cull).
//!
//! A raster terrain up to [`MAX_RESIDENT_RASTER_CHUNKS`] is fully resident;
//! procedural terrain and larger rasters stream around the scene camera (see
//! [`super::scene_camera_translation`] and [`chunk_stream_radius`]). With the
//! `physics` feature each spawned chunk also gets its static collider
//! (`collider.rs`): a LOD-0 heightfield, or a trimesh of its marching-cubes
//! surface when it holds volumetric edits.

use bevy::prelude::*;
use super::{
    TerrainBaked, TerrainChunkMaterial, TerrainConfig, TerrainData, TerrainGenerationQueue, TerrainRoot,
    TerrainVolume, attach_chunk_collider, chunk_lod_distance, chunk_spawn_cost,
    chunk_world_position, generate_chunk_render_mesh_and_surface, new_terrain_chunk_material,
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
/// raster widens the radius to reach its farthest footprint corner from the
/// clamped origin, because its grid half-extent equals `view_distance` by
/// construction (`TerrainTomlFile::to_terrain_config`), so a
/// `view_distance` disc could never cover it. Procedural terrain streams
/// `view_distance`.
pub fn chunk_stream_radius(config: &TerrainConfig, data: &TerrainData, origin: Vec3) -> Option<f32> {
    let view_distance = config.view_distance.max(0.0);
    if data.height_cache.is_empty() {
        return Some(view_distance);
    }
    if raster_fully_resident(config, data) {
        return None;
    }
    let (min, max) = config.footprint_xz();
    let far_x = (origin.x - min.x).abs().max((max.x - origin.x).abs());
    let far_z = (origin.z - min.y).abs().max((max.y - origin.z).abs());
    Some(view_distance.max((far_x * far_x + far_z * far_z).sqrt()))
}

/// System to spawn new chunks as camera moves
///
/// Throttled: only scans for missing chunks when the camera moves to a new
/// chunk, after scan_interval seconds, or while a previous scan left chunks
/// unspawned, and spawns at most two heightfield chunks' worth per frame
/// (a chunk holding volumetric edits counts as several, see
/// [`chunk_spawn_cost`]), nearest first.
///
/// Never spawns a position that already has a chunk entity, and stands down
/// while [`TerrainGenerationQueue`] is still filling, so it cannot race the
/// queue for the same positions. A terrain backed by a height raster has a
/// finite extent (`-chunks_x..=chunks_x` by `-chunks_z..=chunks_z`); sampling
/// past it would just repeat the clamped edge, so nothing is spawned there.
/// A raster that fits [`MAX_RESIDENT_RASTER_CHUNKS`] is filled whole and
/// stays resident; only procedural terrain (no edge) and oversized rasters
/// stream by radius (see [`chunk_stream_radius`]).
///
/// With the `physics` feature each spawned chunk gets its collider.
pub fn chunk_spawn_system(
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<
        (
            Entity,
            &TerrainConfig,
            &TerrainData,
            Option<&TerrainVolume>,
            Option<&Children>,
            Option<&TerrainChunkMaterial>,
            Option<&TerrainBaked>,
        ),
        With<TerrainRoot>,
    >,
    chunk_query: Query<&Chunk>,
    chunk_materials: Query<&MeshMaterial3d<StandardMaterial>, With<Chunk>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    generation_queue: Res<TerrainGenerationQueue>,
    time: Res<Time>,
    mut throttle: ResMut<ChunkSpawnThrottle>,
) {
    if generation_queue.is_generating() {
        return;
    }
    let Some(camera_pos) = scene_camera_translation(&cameras) else { return };
    let Ok((terrain_entity, config, data, volume, children, root_material, baked)) = terrain_query.single() else {
        return;
    };
    let volume = volume.unwrap_or(TerrainVolume::empty());
    let data = surface_data(data, baked);
    let origin = chunk_streaming_origin(config, data, camera_pos);

    // Throttle: only scan when camera moves to a new chunk, after the
    // interval, or while the last scan left a backlog.
    let size = config.chunk_size.max(1e-3);
    let camera_chunk = IVec2::new(
        (origin.x / size).floor() as i32,
        (origin.z / size).floor() as i32,
    );
    let current_time = time.elapsed_secs_f64();
    let camera_moved_chunk = camera_chunk != throttle.last_camera_chunk;
    let interval_elapsed = current_time - throttle.last_scan_time >= throttle.scan_interval;
    if !camera_moved_chunk && !interval_elapsed && !throttle.backlog {
        return;
    }
    throttle.last_camera_chunk = camera_chunk;
    throttle.last_scan_time = current_time;

    let radius = chunk_stream_radius(config, data, origin);
    let (mut min_x, mut max_x, mut min_z, mut max_z) = match radius {
        None => (
            -(config.chunks_x as i32),
            config.chunks_x as i32,
            -(config.chunks_z as i32),
            config.chunks_z as i32,
        ),
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
        min_x = min_x.max(-(config.chunks_x as i32));
        max_x = max_x.min(config.chunks_x as i32);
        min_z = min_z.max(-(config.chunks_z as i32));
        max_z = max_z.min(config.chunks_z as i32);
    }

    // Every chunk entity counts, parented yet or not.
    let existing_chunks: std::collections::HashSet<IVec2> =
        chunk_query.iter().map(|chunk| chunk.position).collect();

    let mut candidates: Vec<(IVec2, f32)> = Vec::new();
    for cx in min_x..=max_x {
        for cz in min_z..=max_z {
            let chunk_pos = IVec2::new(cx, cz);
            if existing_chunks.contains(&chunk_pos) {
                continue;
            }
            // Measured even when every chunk qualifies, so the backlog still
            // fills nearest first.
            let distance = chunk_xz_distance(chunk_pos, config, origin);
            if radius.map_or(true, |r| distance <= r) {
                candidates.push((chunk_pos, distance));
            }
        }
    }

    // Cap the work spawned per frame to prevent frame spikes, in heightfield
    // chunks. Mesh generation is expensive, so it is spread across frames.
    const MAX_SPAWNS_PER_FRAME: usize = 2;
    if candidates.is_empty() {
        throttle.backlog = false;
        return;
    }

    // Sort closest-first so the most visible chunks spawn first
    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

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

    let mut spent = 0;
    let mut spawned = 0;
    for &(chunk_pos, _) in &candidates {
        if spent >= MAX_SPAWNS_PER_FRAME {
            break;
        }
        let lod = config.lod_for_distance(chunk_lod_distance(chunk_pos, config, data, camera_pos));
        spent += chunk_spawn_cost(chunk_pos, lod, config, data, volume);
        spawned += 1;
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
    throttle.backlog = spawned < candidates.len();
}

/// Seconds between cull passes; chunks leave view slowly relative to a frame.
const CULL_INTERVAL_SECS: f64 = 0.25;

/// System to cull chunks outside the streaming radius
///
/// A raster terrain small enough to stay resident is never culled: its
/// chunks own the static heightfield colliders that bodies rest on, and
/// despawning one would drop whatever stands there. An oversized raster
/// culls only beyond a radius that reaches its farthest corner, and
/// procedural terrain culls beyond `view_distance`. Despawning a chunk takes
/// its collider child with it; edits live in `TerrainData` on the root, so a
/// chunk streamed back in rebuilds from them.
pub fn chunk_cull_system(
    mut commands: Commands,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<(&TerrainConfig, &TerrainData), With<TerrainRoot>>,
    chunk_query: Query<(Entity, &Chunk)>,
    time: Res<Time>,
    mut last_cull: Local<f64>,
) {
    let Some(camera_pos) = scene_camera_translation(&cameras) else { return };
    let Ok((config, data)) = terrain_query.single() else { return };

    let now = time.elapsed_secs_f64();
    if now - *last_cull < CULL_INTERVAL_SECS {
        return;
    }
    *last_cull = now;

    let origin = chunk_streaming_origin(config, data, camera_pos);
    let Some(radius) = chunk_stream_radius(config, data, origin) else { return };
    let cull_distance = radius * 1.2; // Hysteresis to prevent popping

    for (entity, chunk) in chunk_query.iter() {
        if chunk_xz_distance(chunk.position, config, origin) > cull_distance {
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
        assert_eq!(chunk_stream_radius(&config, &raster, Vec3::ZERO), None);
        assert_eq!(chunk_stream_radius(&config, &raster, corner), None);

        let procedural = TerrainData::procedural();
        assert!(!raster_fully_resident(&config, &procedural));
        assert_eq!(chunk_stream_radius(&config, &procedural, corner), Some(256.0));
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
        let radius = chunk_stream_radius(&config, &raster, origin).expect("an oversized raster streams");
        assert!((radius - 528.0 * std::f32::consts::SQRT_2).abs() < 1e-2, "got {radius}");
        let far = chunk_xz_distance(IVec2::new(16, 16), &config, origin);
        assert!(far <= radius, "far corner chunk at {far} lies outside {radius}");
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
}
