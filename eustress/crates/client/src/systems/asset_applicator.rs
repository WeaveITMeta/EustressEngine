//! Asset applicator - Replaces placeholders with generated assets
//! Uses Bevy 0.17's HTTP asset loading with caching for optimal performance

use bevy::prelude::*;
use bevy::gltf::Gltf;
use crate::components::{Enhanced, EnhancedAssetHandle};
use super::enhancement_scheduler::EnhancementCache;

/// Component to track loaded GLTF handle
#[derive(Component)]
pub struct LoadingGltf {
    pub handle: Handle<Gltf>,
}

/// System to initiate asset loading for enhanced entities
/// Uses Bevy 0.17's AssetServer which supports HTTP loading with caching
pub fn asset_applicator_system(
    mut commands: Commands,
    enhanced_query: Query<(Entity, &Enhanced), (Added<Enhanced>, Without<LoadingGltf>)>,
    asset_server: Res<AssetServer>,
    cache: Res<EnhancementCache>,
) {
    for (entity, enhanced) in enhanced_query.iter() {
        info!("🎨 Loading enhanced asset for entity {:?}", entity);
        
        // Build the cache file path
        let cache_path = cache.path.join(format!("{}.glb", enhanced.cache_key));
        
        // Use Bevy's AssetServer to load the GLB
        // Bevy 0.17 handles caching automatically
        let gltf_handle: Handle<Gltf> = asset_server.load(cache_path.clone());
        
        info!("   📦 Loading from cache: {:?}", cache_path);
        
        // Track the loading handle
        commands.entity(entity).insert(LoadingGltf {
            handle: gltf_handle,
        });
    }
}

/// System to apply loaded GLTF assets once they're ready
pub fn apply_loaded_assets_system(
    mut commands: Commands,
    gltf_assets: Res<Assets<Gltf>>,
    loading_query: Query<(Entity, &LoadingGltf, &Transform)>,
    asset_server: Res<AssetServer>,
) {
    for (entity, loading, _transform) in loading_query.iter() {
        // Check if the GLTF is loaded
        match asset_server.load_state(&loading.handle) {
            bevy::asset::LoadState::Loaded => {
                if let Some(gltf) = gltf_assets.get(&loading.handle) {
                    info!("✅ GLTF loaded for entity {:?}", entity);
                    
                    // Get the default scene from the GLTF
                    if let Some(scene_handle) = &gltf.default_scene {
                        // Spawn the GLTF scene as a child of the entity
                        commands.entity(entity).with_children(|parent| {
                            parent.spawn((
                                SceneRoot(scene_handle.clone()),
                                Transform::default(),
                            ));
                        });
                        
                        info!("   🎉 Applied GLTF scene to entity");
                    } else if !gltf.scenes.is_empty() {
                        // Use first scene if no default
                        let scene_handle = &gltf.scenes[0];
                        commands.entity(entity).with_children(|parent| {
                            parent.spawn((
                                SceneRoot(scene_handle.clone()),
                                Transform::default(),
                            ));
                        });
                        
                        info!("   🎉 Applied first GLTF scene to entity");
                    } else {
                        warn!("   ⚠️ GLTF has no scenes, using meshes directly");
                        
                        // Fallback: spawn meshes directly
                        for (name, _mesh_handle) in &gltf.named_meshes {
                            info!("   📐 Found mesh: {}", name);
                        }
                    }
                    
                    // Mark as fully enhanced
                    commands.entity(entity).insert(EnhancedAssetHandle {
                        gltf: loading.handle.clone(),
                    });
                    
                    // Remove the loading component
                    commands.entity(entity).remove::<LoadingGltf>();
                }
            }
            bevy::asset::LoadState::Failed(_) => {
                error!("❌ Failed to load GLTF for entity {:?}", entity);
                commands.entity(entity).remove::<LoadingGltf>();
            }
            _ => {
                // Still loading, do nothing
            }
        }
    }
}

/// System to handle HTTP-based asset streaming (for remote cache servers)
/// This enables loading from URLs like http://localhost:8001/cache/{key}.glb
#[allow(dead_code)]
pub fn stream_remote_assets_system(
    mut commands: Commands,
    enhanced_query: Query<(Entity, &Enhanced), (Added<Enhanced>, Without<LoadingGltf>)>,
    asset_server: Res<AssetServer>,
    _cache: Res<EnhancementCache>,
) {
    // Check if we should use HTTP streaming (e.g., for distributed cache)
    let use_http = std::env::var("EUSTRESS_CACHE_URL").ok();
    
    if let Some(base_url) = use_http {
        for (entity, enhanced) in enhanced_query.iter() {
            // Use Bevy 0.17's HTTP asset loading
            let url = format!("{}/{}.glb", base_url, enhanced.cache_key);
            
            info!("🌐 Streaming asset from: {}", url);
            
            // Bevy 0.17 handles HTTP loading natively
            let gltf_handle: Handle<Gltf> = asset_server.load(&url);
            
            commands.entity(entity).insert(LoadingGltf {
                handle: gltf_handle,
            });
        }
    }
}
