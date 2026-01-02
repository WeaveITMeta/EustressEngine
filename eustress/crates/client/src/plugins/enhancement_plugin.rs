//! Enhancement plugin - orchestrates the AI generation pipeline
//! 
//! Bevy 0.17 Integration:
//! - Uses AssetServer for HTTP loading with caching
//! - Supports remote cache via EUSTRESS_CACHE_URL env var
//! - Automatic GLTF scene application

use bevy::prelude::*;
use crate::systems::*;
use crate::components::AssetStreamingConfig;

pub struct EnhancementPlugin;

impl Plugin for EnhancementPlugin {
    fn build(&self, app: &mut App) {
        app
            // Resources
            .init_resource::<EnhancementCache>()
            .init_resource::<GenerationResultChannel>()
            .init_resource::<LoadedScene>()
            .init_resource::<PlayerPosition>()
            .init_resource::<ChunkingSettings>()
            .init_resource::<QuestState>()
            .init_resource::<AssetStreamingConfig>()
            
            // Messages (Bevy 0.17)
            .add_message::<EnhanceNodeEvent>()
            .add_message::<LoadSceneEvent>()
            .add_message::<ConnectionTriggeredEvent>()
            
            // Systems - Scene loading first
            .add_systems(Startup, setup_enhancement_cache)
            .add_systems(Update, (
                // Track player for distance culling
                update_player_position_system,
                
                // Load scenes
                scene_loader_system,
                
                // Distance-based enhancement triggers
                distance_chunking_system,
                
                // Background enhancement (AI generation)
                enhancement_scheduler_system,
                
                // Initiate asset loading (Bevy 0.17 AssetServer)
                asset_applicator_system,
                
                // Apply loaded GLTF assets
                apply_loaded_assets_system,
                
                // Quest graph execution
                quest_executor_system,
            ).chain());
        
        // Log streaming config
        let config = AssetStreamingConfig::default();
        if config.use_http {
            info!("🌐 Enhancement Plugin: HTTP streaming enabled via {}", 
                  config.cache_url.unwrap_or_default());
        } else {
            info!("📦 Enhancement Plugin: Local cache mode");
        }
        
        info!("🎨 Enhancement Plugin initialized with Bevy 0.17 asset streaming");
    }
}
