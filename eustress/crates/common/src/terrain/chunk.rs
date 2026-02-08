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

/// System to spawn new chunks as camera moves
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
) {
    let Ok(camera_transform) = camera_query.single() else { return };
    let camera_pos = camera_transform.translation();
    
    for (terrain_entity, config, data, children) in terrain_query.iter() {
        // Get existing chunk positions
        let mut existing_chunks: std::collections::HashSet<IVec2> = std::collections::HashSet::new();
        for child in children.iter() {
            if let Ok(chunk) = chunk_query.get(child) {
                existing_chunks.insert(chunk.position);
            }
        }
        
        // Calculate which chunks should exist based on camera position
        let camera_chunk = IVec2::new(
            (camera_pos.x / config.chunk_size).floor() as i32,
            (camera_pos.z / config.chunk_size).floor() as i32,
        );
        
        let view_chunks = (config.view_distance / config.chunk_size).ceil() as i32;
        
        // Get material from first existing chunk (or create default)
        let material_handle = children.iter()
            .find_map(|child| materials.get(child).ok())
            .map(|m| m.0.clone());
        
        // Spawn missing chunks within view distance
        for cx in (camera_chunk.x - view_chunks)..=(camera_chunk.x + view_chunks) {
            for cz in (camera_chunk.y - view_chunks)..=(camera_chunk.y + view_chunks) {
                let chunk_pos = IVec2::new(cx, cz);
                let world_pos = chunk_world_position(chunk_pos, config);
                let distance = camera_pos.distance(world_pos);
                
                if distance <= config.view_distance && !existing_chunks.contains(&chunk_pos) {
                    // Generate mesh for new chunk
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
                        Name::new(format!("Chunk_{}_{}", cx, cz)),
                    ));
                    
                    // Add material if available
                    if let Some(ref mat) = material_handle {
                        chunk_commands.insert(MeshMaterial3d(mat.clone()));
                    }
                    
                    // Add physics collider (1:1 with visual mesh) - DISABLED for Bevy 0.19
                    #[cfg(feature = "physics")]
                    {
                        // Physics temporarily disabled due to avian3d/Bevy 0.19 compatibility
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
                }
            }
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
