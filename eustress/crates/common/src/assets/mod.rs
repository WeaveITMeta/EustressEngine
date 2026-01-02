//! # Eustress Asset System
//!
//! Content-addressable, multi-source asset hosting that beats Roblox.
//!
//! ## Table of Contents
//!
//! 1. **ContentHash** - SHA256 content hash with Base58 encoding
//! 2. **AssetSource** - Multi-source resolution (Local/IPFS/S3/P2P)
//! 3. **AssetResolver** - Async resolver with LRU cache
//! 4. **AssetService** - Bevy resource for asset management
//! 5. **AssetBundle** - Grouped assets for efficient loading
//! 6. **ProgressiveAsset** - LOD-based progressive loading
//!
//! ## Key Advantages Over Roblox
//!
//! | Feature | Roblox | Eustress |
//! |---------|--------|----------|
//! | Storage | Centralized CDN | Hybrid (local/IPFS/S3/P2P) |
//! | Asset IDs | Opaque numbers | SHA256 hashes (verifiable) |
//! | Self-hosting | ❌ | ✅ |
//! | Offline | Limited | ✅ Full |
//! | Formats | Limited | Any (GLTF/FBX/WAV/MP4) |

mod asset_id;
mod source;
mod resolver;
mod service;
mod bundle;
mod progressive;
mod config;
pub mod categories;
pub mod s3;
pub mod p2p;

pub use asset_id::*;
pub use source::*;
pub use resolver::*;
pub use service::*;
pub use bundle::*;
pub use progressive::*;
pub use config::*;
pub use categories::{
    AssetCategory, AssetMetadata, AssetExtraMetadata, AssetRegistry,
    MeshAsset, MeshFormat, MeshBounds, MeshLOD,
    TerrainRegion, TerrainRegionSet, TerrainRegionSettings, TerrainMaterialLayer, GeoBounds,
};
pub use s3::{S3Config, S3Error, MinioDeployment};
pub use p2p::{
    P2PConfig, PeerManager, PeerInfo, PeerState,
    ChunkTransferManager, SignalingClient, SignalingState,
    P2PAssetPlugin, fetch_asset_p2p, P2PFetchResult,
};

use bevy::prelude::*;

/// Asset system plugin for Bevy
/// 
/// Provides:
/// - `AssetService` resource for loading/uploading assets
/// - `AssetConfig` resource for configuration
/// - Systems for progressive loading and cache management
pub struct EustressAssetPlugin {
    /// Local asset directory (for development)
    pub local_path: std::path::PathBuf,
    /// Configuration file path (optional)
    pub config_path: Option<std::path::PathBuf>,
}

impl Default for EustressAssetPlugin {
    fn default() -> Self {
        Self {
            local_path: std::path::PathBuf::from("./assets"),
            config_path: None,
        }
    }
}

impl Plugin for EustressAssetPlugin {
    fn build(&self, app: &mut App) {
        // Load or create config
        let config = if let Some(path) = &self.config_path {
            AssetConfig::load_or_default(path)
        } else {
            AssetConfig::default()
        };
        
        // Create service
        let service = AssetService::new(self.local_path.clone(), config.clone());
        
        app
            .insert_resource(config)
            .insert_resource(service)
            .add_message::<AssetLoadEvent>()
            .add_message::<AssetUploadEvent>()
            .add_systems(Update, (
                handle_asset_load_events,
                update_progressive_assets,
                cleanup_cache,
            ));
    }
}

/// Event to request asset loading
#[derive(Message)]
pub struct AssetLoadEvent {
    /// Content hash to load
    pub id: ContentHash,
    /// Entity to attach the loaded asset to (optional)
    pub target_entity: Option<Entity>,
    /// Priority (higher = load first)
    pub priority: u8,
}

/// Event when asset upload completes
#[derive(Message)]
pub struct AssetUploadEvent {
    /// Resulting content hash
    pub id: ContentHash,
    /// Original file name
    pub name: String,
    /// Success or error
    pub result: Result<(), String>,
}

/// System to handle asset load requests
fn handle_asset_load_events(
    mut events: MessageReader<AssetLoadEvent>,
    service: Res<AssetService>,
) {
    for event in events.read() {
        // Queue the load request
        service.queue_load(event.id.clone(), event.priority);
        
        if let Some(_entity) = event.target_entity {
            // TODO: Track entity -> asset mapping for attachment
        }
    }
}

/// System to update progressive assets (LOD streaming)
fn update_progressive_assets(
    mut query: Query<(&mut ProgressiveAssetState, &Transform)>,
    camera_query: Query<&Transform, With<Camera>>,
    service: Res<AssetService>,
) {
    let Ok(camera_transform) = camera_query.single() else { return };
    let camera_pos = camera_transform.translation;
    
    for (mut state, transform) in query.iter_mut() {
        let distance = camera_pos.distance(transform.translation);
        
        // Determine target LOD based on distance
        let target_lod = if distance < 10.0 {
            0 // Full quality
        } else if distance < 50.0 {
            1
        } else if distance < 200.0 {
            2
        } else {
            3 // Lowest quality
        };
        
        // Request LOD upgrade if needed
        if target_lod < state.current_lod && !state.loading {
            if let Some(lod_id) = state.lods.get(target_lod) {
                service.queue_load(lod_id.clone(), (3 - target_lod) as u8);
                state.loading = true;
            }
        }
    }
}

/// System to periodically clean up cache
fn cleanup_cache(
    service: Res<AssetService>,
    time: Res<Time>,
    mut last_cleanup: Local<f32>,
) {
    *last_cleanup += time.delta_secs();
    
    // Cleanup every 60 seconds
    if *last_cleanup > 60.0 {
        service.cleanup_cache();
        *last_cleanup = 0.0;
    }
}
