//! # Terrain Plugin for Client (Viewer-Only)
//!
//! Client-side terrain **rendering** with LOD, heightmaps, and procedural generation.
//! Viewer-only: No editing features, optimized for exploration at 60 FPS.
//!
//! ## Features
//! - Async heightmap/splatmap loading
//! - Distance-based LOD with seamless transitions
//! - Frustum culling for performance
//! - Physics collisions (via Avian3D trimesh colliders)
//! - Multi-terrain support via TerrainId
//!
//! ## Usage
//! Add `ClientTerrainPlugin` to your app, then use `spawn_client_terrain()` or
//! spawn a `Terrain` class component to create terrain.

use bevy::prelude::*;
use eustress_common::terrain::{
    TerrainPlugin as SharedTerrainPlugin,
    TerrainConfig, TerrainData,
    spawn_terrain, TerrainRoot,
};
use eustress_common::classes::Terrain;

// ============================================================================
// Plugin
// ============================================================================

/// Client terrain plugin - viewer-only, no editing
/// 
/// Add this plugin to enable terrain rendering in your client app.
/// Terrain is automatically spawned when a `Terrain` class component is added,
/// or use `spawn_client_terrain()` for manual spawning.
pub struct ClientTerrainPlugin;

impl Plugin for ClientTerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            // Shared terrain systems (LOD, culling, mesh gen)
            .add_plugins(SharedTerrainPlugin)
            
            // Client resources
            .init_resource::<TerrainLoadQueue>()
            .init_resource::<ClientTerrainSettings>()
            
            // Client-specific systems (fog handled by LightingService)
            .add_systems(Update, (
                sync_terrain_class_to_system,
                process_terrain_load_queue,
            ));
    }
}

// ============================================================================
// Resources
// ============================================================================

/// Queue for async terrain loading
#[derive(Resource, Default)]
pub struct TerrainLoadQueue {
    /// Pending terrain spawns (entity, config, heightmap path)
    pub pending: Vec<(Entity, TerrainConfig, Option<String>)>,
}

/// Client-specific terrain settings
/// 
/// These settings control client-side terrain behavior like chunk limits
/// and visibility culling. Configure via `ResMut<ClientTerrainSettings>`.
#[derive(Resource)]
#[allow(dead_code)]
pub struct ClientTerrainSettings {
    /// Max active chunks (for memory management)
    /// TODO: Implement chunk limiting system
    pub max_chunks: usize,
    /// Enable chunk visibility culling (beyond view distance)
    /// TODO: Implement visibility culling system
    pub visibility_culling: bool,
}

impl Default for ClientTerrainSettings {
    fn default() -> Self {
        Self {
            max_chunks: 256,
            visibility_culling: true,
        }
    }
}

// NOTE: Fog is now handled globally by LightingService in SharedLightingPlugin
// This affects ALL entities (BaseParts, Terrain, Models) uniformly

/// Unique ID for multi-terrain support
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TerrainId(pub u32);

// ============================================================================
// Systems
// ============================================================================

/// Sync Terrain class component to terrain system
/// When a Terrain class is added, queue it for async loading
fn sync_terrain_class_to_system(
    mut commands: Commands,
    query: Query<(Entity, &Terrain), Added<Terrain>>,
    existing_terrain: Query<(Entity, Option<&TerrainId>), With<TerrainRoot>>,
    mut load_queue: ResMut<TerrainLoadQueue>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (_entity, terrain_class) in query.iter() {
        // Generate unique ID for this terrain
        let terrain_id = TerrainId(rand::random());
        
        // Check if we should replace existing or add new
        let should_replace = existing_terrain.iter().count() > 0;
        
        if should_replace {
            // Despawn existing terrain with same ID or all if no ID
            for (existing, id) in existing_terrain.iter() {
                if id.is_none() {
                    commands.entity(existing).despawn();
                }
            }
        }
        
        // Convert class to config
        let config = terrain_class.to_config();
        let data = TerrainData::procedural();
        
        // Spawn terrain immediately (async heightmap loading later)
        let terrain_entity = spawn_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            config.clone(),
            data,
        );
        
        // Add terrain ID for multi-terrain support
        commands.entity(terrain_entity).insert(terrain_id);
        
        // Queue heightmap loading if specified
        if let Some(ref path) = terrain_class.heightmap_path {
            load_queue.pending.push((terrain_entity, config, Some(path.clone())));
        }
        
        info!("🏔️ Client terrain spawned (ID: {:?})", terrain_id.0);
    }
}

/// Process async terrain loading queue
fn process_terrain_load_queue(
    load_queue: ResMut<TerrainLoadQueue>,
    asset_server: Res<AssetServer>,
    mut terrain_query: Query<&mut TerrainData, With<TerrainRoot>>,
) {
    // Process one item per frame to avoid hitches
    if let Some((entity, _config, heightmap_path)) = load_queue.into_inner().pending.pop() {
        if let Some(path) = heightmap_path {
            // Load heightmap asynchronously
            let heightmap_handle: Handle<Image> = asset_server.load(&path);
            
            // Update terrain data with heightmap handle
            if let Ok(mut data) = terrain_query.get_mut(entity) {
                data.heightmap = Some(heightmap_handle);
                info!("📷 Heightmap queued for loading: {}", path);
            }
        }
    }
}

// NOTE: Fog is now handled globally by SharedLightingPlugin via FogSettings
// This affects all entities uniformly (BaseParts, Terrain, Models)
// Configure fog via LightingService resource: fog_enabled, fog_start, fog_end, fog_color

// ============================================================================
// Public API
// ============================================================================

/// Spawn a default terrain for the client
#[allow(dead_code)]
pub fn spawn_client_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let config = TerrainConfig::default();
    let data = TerrainData::procedural();
    
    let entity = spawn_terrain(commands, meshes, materials, config, data);
    commands.entity(entity).insert(TerrainId(rand::random()));
    entity
}

/// Spawn a small test terrain (for debugging)
#[allow(dead_code)]
pub fn spawn_test_terrain(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let config = TerrainConfig::small();
    let data = TerrainData::procedural();
    
    let entity = spawn_terrain(commands, meshes, materials, config, data);
    commands.entity(entity).insert(TerrainId(0)); // Test ID
    entity
}

/// Spawn terrain from heightmap path (async loading)
#[allow(dead_code)]
pub fn spawn_terrain_from_heightmap(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    load_queue: &mut TerrainLoadQueue,
    heightmap_path: &str,
    config: TerrainConfig,
) -> Entity {
    let data = TerrainData::procedural(); // Start procedural, load heightmap async
    
    let entity = spawn_terrain(commands, meshes, materials, config.clone(), data);
    let terrain_id = TerrainId(rand::random());
    commands.entity(entity).insert(terrain_id);
    
    // Queue heightmap for async loading
    load_queue.pending.push((entity, config, Some(heightmap_path.to_string())));
    
    entity
}

/// Despawn terrain by ID
#[allow(dead_code)]
pub fn despawn_terrain_by_id(
    commands: &mut Commands,
    terrain_query: &Query<(Entity, &TerrainId), With<TerrainRoot>>,
    id: TerrainId,
) {
    for (entity, terrain_id) in terrain_query.iter() {
        if *terrain_id == id {
            commands.entity(entity).despawn();
        }
    }
}
