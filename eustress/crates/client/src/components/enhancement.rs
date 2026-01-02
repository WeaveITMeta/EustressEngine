//! Enhancement components for tracking AI generation state

use bevy::prelude::*;
use bevy::gltf::Gltf;
use eustress_common::{NodeCategory, DetailLevel};

/// Marks an entity as pending AI enhancement
#[derive(Component, Reflect)]
#[reflect(Component)]
#[allow(dead_code)]
pub struct PendingEnhancement {
    pub node_id: u32,
    pub prompt: String,
    pub category: NodeCategory,
    pub detail_level: DetailLevel,
}

/// Marks an entity as currently being enhanced
#[derive(Component)]
#[allow(dead_code)]
pub struct EnhancingInProgress {
    pub started_at: std::time::Instant,
}

impl Default for EnhancingInProgress {
    fn default() -> Self {
        Self {
            started_at: std::time::Instant::now(),
        }
    }
}

/// Marks an entity as fully enhanced
#[derive(Component)]
#[allow(dead_code)]
pub struct Enhanced {
    pub cache_key: String,
    pub generated_at: std::time::SystemTime,
}

impl Default for Enhanced {
    fn default() -> Self {
        Self {
            cache_key: String::new(),
            generated_at: std::time::SystemTime::now(),
        }
    }
}

/// Enhancement status for UI display
#[derive(Component)]
#[allow(dead_code)]
pub enum EnhancementStatus {
    Pending,
    Generating,
    Applying,
    Complete,
    Failed(String),
}

/// Holds the loaded GLTF asset handle after enhancement is complete
/// This enables Bevy's asset system to manage the lifecycle
#[derive(Component)]
#[allow(dead_code)]
pub struct EnhancedAssetHandle {
    pub gltf: Handle<Gltf>,
}

/// Configuration for HTTP-based asset streaming
#[derive(Resource)]
#[allow(dead_code)]
pub struct AssetStreamingConfig {
    /// Base URL for remote cache (e.g., "http://localhost:8001/cache")
    pub cache_url: Option<String>,
    /// Enable HTTP streaming (uses EUSTRESS_CACHE_URL env var if not set)
    pub use_http: bool,
    /// Timeout for HTTP requests in seconds
    pub timeout_secs: u64,
}

impl Default for AssetStreamingConfig {
    fn default() -> Self {
        Self {
            cache_url: std::env::var("EUSTRESS_CACHE_URL").ok(),
            use_http: std::env::var("EUSTRESS_CACHE_URL").is_ok(),
            timeout_secs: 30,
        }
    }
}
