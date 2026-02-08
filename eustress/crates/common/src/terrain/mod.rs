//! # Terrain System for Eustress Engine
//! 
//! High-performance terrain rendering with LOD, heightmaps, and splat textures.
//! Shared between Engine Studio and Client.
//!
//! ## Features
//! - Chunk-based terrain with automatic LOD
//! - Procedural generation via Perlin noise
//! - Heightmap support (grayscale images)
//! - Splatmap texture blending (up to 8 layers)
//! - Runtime editing (height painting, texture painting)
//! - Frustum culling and view distance culling
//! - **Physics collisions** (1:1 visual-to-physics mesh, requires `physics` feature)
//! - **Undo/Redo** history for terrain edits
//! - **Advanced brushes** (noise stamps, erosion simulation)
//! - **GPU compute** mesh generation with CPU fallback
//! - **Greedy meshing** for optimized flat areas
//!
//! ## Architecture
//! - `TerrainConfig`: Configuration for terrain generation
//! - `TerrainData`: Runtime data (heightmap, splatmap)
//! - `Chunk`: Individual terrain tile with LOD level
//! - `TerrainPlugin`: Main plugin for terrain systems
//! - `TerrainHistory`: Undo/redo stack for edits
//! - `NoiseBrush`: Advanced noise-based brushes
//! - `ErosionSimulation`: Hydraulic/thermal erosion
//!
//! ## Physics
//! Enable the `physics` feature to add Avian3D colliders to terrain chunks.
//! Colliders are 1:1 with visual mesh (trimesh colliders).

pub mod config;
pub mod chunk;
pub mod mesh;
pub mod lod;
pub mod editor;
pub mod material;
pub mod history;
pub mod brushes;
pub mod compute;

pub use config::*;
pub use chunk::*;
pub use mesh::*;
pub use lod::*;
pub use editor::*;
pub use material::*;
pub use history::*;
pub use brushes::*;
pub use compute::*;

use bevy::prelude::*;
use tracing::info;

#[cfg(feature = "physics")]
use avian3d::prelude::*;

/// Main terrain plugin - add to both Engine and Client
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<TerrainMode>()
            .init_resource::<TerrainBrush>()
            .init_resource::<TerrainGenerationQueue>()
            .init_resource::<LodUpdateState>()  // Throttled LOD updates for performance
            
            // Types for reflection/serialization
            .register_type::<TerrainConfig>()
            .register_type::<TerrainData>()
            .register_type::<Chunk>()
            
            // Core systems
            .add_systems(Update, (
                process_terrain_generation_queue,  // Process async terrain generation
                // update_lod_system,
                // chunk_spawn_system,
                // chunk_cull_system,
            ))
            
            // Editor systems (only run in Editor mode)
            .add_systems(Update, (
                toggle_editor_system,
                terrain_paint_system,
            ).run_if(resource_equals(TerrainMode::Editor)));
    }
}

/// Terrain mode: Render-only or Editor (allows painting)
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerrainMode {
    #[default]
    Render,
    Editor,
}

/// Marker component for terrain root entity
#[derive(Component, Default)]
pub struct TerrainRoot;

/// Resource to track async terrain generation progress
#[derive(Resource, Default)]
pub struct TerrainGenerationQueue {
    /// Chunks waiting to be spawned (chunk_pos, terrain_entity)
    pub pending_chunks: Vec<(IVec2, Entity)>,
    /// Terrain material handle (shared across chunks)
    pub material: Option<Handle<StandardMaterial>>,
    /// Config for generation
    pub config: Option<TerrainConfig>,
    /// Data for generation
    pub data: Option<TerrainData>,
    /// Chunks spawned per frame (tune for performance)
    pub chunks_per_frame: usize,
    /// Total chunks to spawn
    pub total_chunks: usize,
    /// Chunks spawned so far
    pub spawned_count: usize,
}

impl TerrainGenerationQueue {
    /// Check if generation is in progress
    pub fn is_generating(&self) -> bool {
        !self.pending_chunks.is_empty()
    }
    
    /// Get progress as percentage (0.0 - 1.0)
    pub fn progress(&self) -> f32 {
        if self.total_chunks == 0 {
            1.0
        } else {
            self.spawned_count as f32 / self.total_chunks as f32
        }
    }
}

/// Spawns a complete terrain entity with ASYNC chunk generation
/// 
/// This queues chunks for generation over multiple frames to prevent UI freezing.
/// When `physics` feature is enabled, each chunk gets a 1:1 trimesh collider
/// matching the visual mesh exactly for accurate terrain collisions.
pub fn spawn_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: TerrainConfig,
    data: TerrainData,
) -> Entity {
    #[cfg(feature = "physics")]
    info!("🏔️ Spawning terrain with PHYSICS: {}x{} chunks, {} LOD levels (async)", 
        config.chunks_x, config.chunks_z, config.lod_levels);
    
    #[cfg(not(feature = "physics"))]
    info!("🏔️ Spawning terrain: {}x{} chunks, {} LOD levels (async)", 
        config.chunks_x, config.chunks_z, config.lod_levels);
    
    // Create terrain material
    let terrain_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.5, 0.2),  // Default grass green
        perceptual_roughness: 0.9,
        metallic: 0.0,
        reflectance: 0.3,
        ..default()
    });
    
    // Spawn terrain root (without chunks - they'll be added async)
    let terrain_entity = commands.spawn((
        TerrainRoot,
        config.clone(),
        data.clone(),
        Transform::default(),
        Visibility::default(),
        Name::new("Terrain"),
    )).id();
    
    // Build list of chunks to spawn
    let half_x = (config.chunks_x / 2) as i32;
    let half_z = (config.chunks_z / 2) as i32;
    
    let mut pending_chunks = Vec::new();
    for cx in -half_x..=half_x {
        for cz in -half_z..=half_z {
            pending_chunks.push((IVec2::new(cx, cz), terrain_entity));
        }
    }
    
    let total_chunks = pending_chunks.len();
    
    // Queue for async generation
    commands.insert_resource(TerrainGenerationQueue {
        pending_chunks,
        material: Some(terrain_material),
        config: Some(config),
        data: Some(data),
        chunks_per_frame: 4,  // Spawn 4 chunks per frame for smooth generation
        total_chunks,
        spawned_count: 0,
    });
    
    info!("📋 Queued {} chunks for async generation", total_chunks);
    
    terrain_entity
}

/// System to process terrain generation queue over multiple frames
pub fn process_terrain_generation_queue(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut queue: ResMut<TerrainGenerationQueue>,
) {
    if queue.pending_chunks.is_empty() {
        return;
    }
    
    // Clone what we need to avoid borrow issues
    let config = match &queue.config {
        Some(c) => c.clone(),
        None => return,
    };
    let data = match &queue.data {
        Some(d) => d.clone(),
        None => return,
    };
    let material = match &queue.material {
        Some(m) => m.clone(),
        None => return,
    };
    
    // Process a batch of chunks this frame
    let chunks_to_process = queue.chunks_per_frame.min(queue.pending_chunks.len());
    
    // Collect chunks to process
    let mut chunks_batch = Vec::with_capacity(chunks_to_process);
    for _ in 0..chunks_to_process {
        if let Some(chunk) = queue.pending_chunks.pop() {
            chunks_batch.push(chunk);
        }
    }
    
    for (chunk_pos, terrain_entity) in chunks_batch {
        let world_pos = chunk_world_position(chunk_pos, &config);
        
        // Generate mesh for this chunk
        let mesh_handle = generate_chunk_mesh(
            chunk_pos,
            0,  // Start at LOD 0
            &config,
            &data,
            &mut meshes,
        );
        
        // Spawn chunk as child of terrain
        let mut chunk_commands = commands.spawn((
            Chunk {
                position: chunk_pos,
                lod: 0,
                dirty: false,
            },
            Mesh3d(mesh_handle.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(world_pos),
            Visibility::default(),
            Name::new(format!("Chunk_{}_{}", chunk_pos.x, chunk_pos.y)),
        ));
        
        // Add physics collider (1:1 with visual mesh) - DISABLED for Bevy 0.19
        // TODO: Fix avian3d Bundle usage for Bevy 0.19
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
        
        queue.spawned_count += 1;
    }
    
    // Log progress periodically
    if queue.pending_chunks.is_empty() {
        info!("✅ Terrain generation complete: {} chunks spawned", queue.spawned_count);
        // Clear the queue
        queue.config = None;
        queue.data = None;
        queue.material = None;
        queue.total_chunks = 0;
        queue.spawned_count = 0;
    } else if queue.spawned_count % 20 == 0 {
        info!("🏔️ Terrain generation: {:.0}% ({}/{})", 
            queue.progress() * 100.0, 
            queue.spawned_count, 
            queue.total_chunks);
    }
}

/// Calculate world position for a chunk
pub fn chunk_world_position(chunk_pos: IVec2, config: &TerrainConfig) -> Vec3 {
    Vec3::new(
        chunk_pos.x as f32 * config.chunk_size,
        0.0,
        chunk_pos.y as f32 * config.chunk_size,
    )
}

/// Toggle editor mode with 'T' key
fn toggle_editor_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<TerrainMode>,
) {
    if keys.just_pressed(KeyCode::KeyT) {
        *mode = match *mode {
            TerrainMode::Render => {
                info!("🎨 Terrain Editor: ENABLED");
                TerrainMode::Editor
            }
            TerrainMode::Editor => {
                info!("🎨 Terrain Editor: DISABLED");
                TerrainMode::Render
            }
        };
    }
}
