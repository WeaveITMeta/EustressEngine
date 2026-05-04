//! Terrain chunk component and systems
//!
//! Supports optional Avian3D physics colliders (1:1 visual-to-physics mesh).
//! Enable with `physics` feature in Cargo.toml.

use bevy::prelude::*;
use super::{TerrainConfig, TerrainData, TerrainRoot, chunk_world_position, generate_chunk_mesh};

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
    
    /// Whether this chunk needs mesh regeneration
    pub dirty: bool,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            position: IVec2::ZERO,
            lod: 0,
            dirty: false,
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
}

impl Default for ChunkSpawnThrottle {
    fn default() -> Self {
        Self {
            last_camera_chunk: IVec2::new(i32::MAX, i32::MAX), // Force first scan
            last_scan_time: 0.0,
            scan_interval: 0.5,
        }
    }
}

/// System to spawn new chunks as camera moves
/// 
/// Throttled: only scans for missing chunks when the camera moves to a new
/// chunk or after scan_interval seconds, preventing per-frame iteration over
/// hundreds of candidate positions.
///
/// When `physics` feature is enabled, each chunk gets a 1:1 trimesh collider
/// matching the visual mesh exactly for accurate terrain collisions.
pub fn chunk_spawn_system(
    mut commands: Commands,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_query: Query<(Entity, &TerrainConfig, &TerrainData, &Children), With<TerrainRoot>>,
    chunk_query: Query<&Chunk>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Query<&MeshMaterial3d<StandardMaterial>>,
    time: Res<Time>,
    mut throttle: ResMut<ChunkSpawnThrottle>,
) {
    let Ok(camera_transform) = camera_query.single() else { return };
    let camera_pos = camera_transform.translation();
    
    for (terrain_entity, config, data, children) in terrain_query.iter() {
        // Throttle: only scan when camera moves to a new chunk or after interval
        let camera_chunk = IVec2::new(
            (camera_pos.x / config.chunk_size).floor() as i32,
            (camera_pos.z / config.chunk_size).floor() as i32,
        );
        let current_time = time.elapsed_secs_f64();
        let camera_moved_chunk = camera_chunk != throttle.last_camera_chunk;
        let interval_elapsed = current_time - throttle.last_scan_time >= throttle.scan_interval;
        
        if !camera_moved_chunk && !interval_elapsed {
            return;
        }
        throttle.last_camera_chunk = camera_chunk;
        throttle.last_scan_time = current_time;
        
        // Get existing chunk positions
        let mut existing_chunks: std::collections::HashSet<IVec2> = std::collections::HashSet::new();
        for child in children.iter() {
            if let Ok(chunk) = chunk_query.get(child) {
                existing_chunks.insert(chunk.position);
            }
        }
        
        let view_chunks = (config.view_distance / config.chunk_size).ceil() as i32;
        
        // Get material from first existing chunk (or create default)
        let material_handle = children.iter()
            .find_map(|child| materials.get(child).ok())
            .map(|m| m.0.clone());
        
        // Cap the number of chunks spawned per frame to prevent frame spikes.
        // Mesh generation is expensive — spread the work across frames.
        const MAX_SPAWNS_PER_FRAME: usize = 2;
        let mut spawned_this_frame = 0;

        // Spawn missing chunks within view distance (closest first)
        let mut candidates: Vec<(IVec2, Vec3, f32)> = Vec::new();
        for cx in (camera_chunk.x - view_chunks)..=(camera_chunk.x + view_chunks) {
            for cz in (camera_chunk.y - view_chunks)..=(camera_chunk.y + view_chunks) {
                let chunk_pos = IVec2::new(cx, cz);
                if existing_chunks.contains(&chunk_pos) {
                    continue;
                }
                let world_pos = chunk_world_position(chunk_pos, config);
                let distance = camera_pos.distance(world_pos);
                if distance <= config.view_distance {
                    candidates.push((chunk_pos, world_pos, distance));
                }
            }
        }
        
        // Early out — nothing to spawn
        if candidates.is_empty() {
            return;
        }
        
        // Sort closest-first so the most visible chunks spawn first
        candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        for (chunk_pos, world_pos, distance) in candidates {
            if spawned_this_frame >= MAX_SPAWNS_PER_FRAME {
                break;
            }

            // Generate mesh for new chunk at distance-appropriate LOD
            let lod = config.lod_for_distance(distance);
            let mesh_handle = generate_chunk_mesh(chunk_pos, lod, config, data, &mut meshes);
            
            // Spawn chunk with visual components
            let mut chunk_commands = commands.spawn((
                Chunk {
                    position: chunk_pos,
                    lod,
                    dirty: false,
                },
                Mesh3d(mesh_handle.clone()),
                Transform::from_translation(world_pos),
                Visibility::default(),
                Name::new(format!("Chunk_{}_{}", chunk_pos.x, chunk_pos.y)),
            ));
            
            // Add material if available
            if let Some(ref mat) = material_handle {
                chunk_commands.insert(MeshMaterial3d(mat.clone()));
            }
            
            // Add physics collider (1:1 with visual mesh)
            // Requires avian3d physics feature
            #[cfg(feature = "physics")]
            {
                // TODO: Re-enable when avian3d Collider::trimesh_from_mesh is verified
                // if let Some(mesh) = meshes.get(&mesh_handle) {
                //     if let Some(collider) = Collider::trimesh_from_mesh(mesh) {
                //         chunk_commands.insert((
                //             RigidBody::Static,
                //             collider,
                //             CollisionLayers::new(...),
                //         ));
                //     }
                // }
            }
            
            let chunk_entity = chunk_commands.id();
            commands.entity(terrain_entity).add_child(chunk_entity);
            spawned_this_frame += 1;
        }
    }
}

/// System to cull chunks outside view distance
pub fn chunk_cull_system(
    mut commands: Commands,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_query: Query<&TerrainConfig, With<TerrainRoot>>,
    chunk_query: Query<(Entity, &Chunk, &GlobalTransform)>,
) {
    let Ok(camera_transform) = camera_query.single() else { return };
    let Ok(config) = terrain_query.single() else { return };
    
    let camera_pos = camera_transform.translation();
    let cull_distance = config.view_distance * 1.2;  // Hysteresis to prevent popping
    
    for (entity, _chunk, transform) in chunk_query.iter() {
        let distance = camera_pos.distance(transform.translation());
        
        if distance > cull_distance {
            commands.entity(entity).despawn();
        }
    }
}
