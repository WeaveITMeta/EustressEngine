//! # AssetService - Bevy Resource for Asset Management
//!
//! Main interface for loading, uploading, and managing assets.
//! Integrates with the Studio Asset Browser and runtime loading.

use super::{ContentHash, AssetSource, AssetResolver, AssetConfig, ResolveError};
use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;

/// Asset metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfo {
    /// Content hash (ContentHash)
    pub id: ContentHash,
    /// Original filename
    pub name: String,
    /// MIME type (e.g., "model/gltf+json", "image/png")
    pub mime_type: String,
    /// Size in bytes
    pub size: usize,
    /// Creation timestamp (Unix epoch)
    pub created_at: u64,
    /// Optional tags for organization
    pub tags: Vec<String>,
    /// Optional description
    pub description: Option<String>,
}

/// Asset load state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetLoadState {
    /// Not loaded
    NotLoaded,
    /// Currently loading
    Loading,
    /// Loaded successfully
    Loaded,
    /// Failed to load
    Failed(String),
}

/// Tracked asset with state
struct TrackedAsset {
    info: AssetInfo,
    state: AssetLoadState,
    data: Option<Vec<u8>>,
}

/// Main asset service resource
/// 
/// Provides:
/// - Asset loading (sync and async)
/// - Asset uploading (generates ContentHash)
/// - Cache management
/// - Integration with Asset Browser UI
#[derive(Resource)]
pub struct AssetService {
    /// Internal resolver
    resolver: Arc<AssetResolver>,
    
    /// Configuration (reserved for future use)
    #[allow(dead_code)]
    config: AssetConfig,
    
    /// Tracked assets (for UI display)
    tracked: Arc<RwLock<std::collections::HashMap<ContentHash, TrackedAsset>>>,
    
    /// Local asset directory
    local_path: PathBuf,
    
    /// Asset index (name -> id mapping)
    index: Arc<RwLock<std::collections::HashMap<String, ContentHash>>>,
}

impl AssetService {
    /// Create a new asset service
    pub fn new(local_path: PathBuf, config: AssetConfig) -> Self {
        let resolver = Arc::new(AssetResolver::new(
            local_path.clone(),
            config.cache_size_mb,
        ));
        
        // Add configured sources
        for source in &config.sources {
            resolver.add_source(source.clone());
        }
        
        // Always add local path as fallback
        resolver.add_source(AssetSource::Local(local_path.clone()));
        
        Self {
            resolver,
            config,
            tracked: Arc::new(RwLock::new(std::collections::HashMap::new())),
            local_path,
            index: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    /// Load an asset by ID (synchronous)
    pub fn load(&self, id: &ContentHash) -> Result<Vec<u8>, ResolveError> {
        // Update state
        {
            let mut tracked = self.tracked.write();
            if let Some(asset) = tracked.get_mut(id) {
                asset.state = AssetLoadState::Loading;
            }
        }
        
        // Resolve
        let result = self.resolver.resolve_sync(id);
        
        // Update state
        {
            let mut tracked = self.tracked.write();
            if let Some(asset) = tracked.get_mut(id) {
                match &result {
                    Ok(data) => {
                        asset.state = AssetLoadState::Loaded;
                        asset.data = Some(data.clone());
                    }
                    Err(e) => {
                        asset.state = AssetLoadState::Failed(e.to_string());
                    }
                }
            }
        }
        
        result
    }
    
    /// Load an asset by name (looks up in index)
    pub fn load_by_name(&self, name: &str) -> Result<Vec<u8>, ResolveError> {
        let index = self.index.read();
        if let Some(id) = index.get(name) {
            self.load(id)
        } else {
            Err(ResolveError::NotFound(name.to_string()))
        }
    }
    
    /// Queue an asset for background loading
    pub fn queue_load(&self, id: ContentHash, priority: u8) {
        self.resolver.queue_load(id, priority);
    }
    
    /// Upload asset data and get its ContentHash
    /// 
    /// Returns the content hash (ContentHash) of the uploaded data.
    pub fn upload(&self, name: &str, data: &[u8]) -> Result<ContentHash, std::io::Error> {
        // Generate ID from content
        let id = ContentHash::from_content(data);
        
        // Determine MIME type
        let mime_type = mime_guess::from_path(name)
            .first_or_octet_stream()
            .to_string();
        
        // Save to local path
        let file_path = self.local_path.join(id.to_base58());
        std::fs::write(&file_path, data)?;
        
        // Create metadata
        let info = AssetInfo {
            id: id.clone(),
            name: name.to_string(),
            mime_type,
            size: data.len(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tags: Vec::new(),
            description: None,
        };
        
        // Track the asset
        {
            let mut tracked = self.tracked.write();
            tracked.insert(id.clone(), TrackedAsset {
                info,
                state: AssetLoadState::Loaded,
                data: Some(data.to_vec()),
            });
        }
        
        // Update index
        {
            let mut index = self.index.write();
            index.insert(name.to_string(), id.clone());
        }
        
        // Pre-cache
        self.resolver.precache(id.clone(), data.to_vec());
        
        Ok(id)
    }
    
    /// Register an asset (add to index without uploading)
    pub fn register(&self, name: &str, id: ContentHash, info: AssetInfo) {
        let mut tracked = self.tracked.write();
        tracked.insert(id.clone(), TrackedAsset {
            info,
            state: AssetLoadState::NotLoaded,
            data: None,
        });
        
        let mut index = self.index.write();
        index.insert(name.to_string(), id);
    }
    
    /// Get asset info
    pub fn get_info(&self, id: &ContentHash) -> Option<AssetInfo> {
        self.tracked.read().get(id).map(|t| t.info.clone())
    }
    
    /// Get asset load state
    pub fn get_state(&self, id: &ContentHash) -> AssetLoadState {
        self.tracked.read()
            .get(id)
            .map(|t| t.state.clone())
            .unwrap_or(AssetLoadState::NotLoaded)
    }
    
    /// List all tracked assets
    pub fn list_assets(&self) -> Vec<AssetInfo> {
        self.tracked.read()
            .values()
            .map(|t| t.info.clone())
            .collect()
    }
    
    /// List assets by tag
    pub fn list_by_tag(&self, tag: &str) -> Vec<AssetInfo> {
        self.tracked.read()
            .values()
            .filter(|t| t.info.tags.contains(&tag.to_string()))
            .map(|t| t.info.clone())
            .collect()
    }
    
    /// List assets by MIME type prefix (e.g., "image/", "model/")
    pub fn list_by_type(&self, type_prefix: &str) -> Vec<AssetInfo> {
        self.tracked.read()
            .values()
            .filter(|t| t.info.mime_type.starts_with(type_prefix))
            .map(|t| t.info.clone())
            .collect()
    }
    
    /// Search assets by name
    pub fn search(&self, query: &str) -> Vec<AssetInfo> {
        let query_lower = query.to_lowercase();
        self.tracked.read()
            .values()
            .filter(|t| t.info.name.to_lowercase().contains(&query_lower))
            .map(|t| t.info.clone())
            .collect()
    }
    
    /// Check if asset is cached
    pub fn is_cached(&self, id: &ContentHash) -> bool {
        self.resolver.is_cached(id)
    }
    
    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize, f64) {
        (
            self.resolver.cache_count(),
            self.resolver.cache_size(),
            self.resolver.stats.cache_hit_ratio(),
        )
    }
    
    /// Cleanup cache (remove old entries)
    pub fn cleanup_cache(&self) {
        self.resolver.cleanup();
    }
    
    /// Clear entire cache
    pub fn clear_cache(&self) {
        self.resolver.clear_cache();
    }
    
    /// Add a source for asset resolution
    pub fn add_source(&self, source: AssetSource) {
        self.resolver.add_source(source);
    }
    
    /// Get local asset path
    pub fn local_path(&self) -> &PathBuf {
        &self.local_path
    }
    
    /// Scan local directory and index all assets
    pub fn scan_local(&self) -> Result<usize, std::io::Error> {
        let mut count = 0;
        
        if !self.local_path.exists() {
            std::fs::create_dir_all(&self.local_path)?;
            return Ok(0);
        }
        
        for entry in std::fs::read_dir(&self.local_path)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // Read file and compute hash
                    if let Ok(data) = std::fs::read(&path) {
                        let id = ContentHash::from_content(&data);
                        
                        let mime_type = mime_guess::from_path(&path)
                            .first_or_octet_stream()
                            .to_string();
                        
                        let info = AssetInfo {
                            id: id.clone(),
                            name: name.to_string(),
                            mime_type,
                            size: data.len(),
                            created_at: entry.metadata()
                                .and_then(|m| m.created())
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs())
                                .unwrap_or(0),
                            tags: Vec::new(),
                            description: None,
                        };
                        
                        self.register(name, id, info);
                        count += 1;
                    }
                }
            }
        }
        
        Ok(count)
    }
    
    /// Export asset index to JSON
    pub fn export_index(&self) -> String {
        let index = self.index.read();
        let map: std::collections::HashMap<String, String> = index
            .iter()
            .map(|(k, v)| (k.clone(), v.to_base58()))
            .collect();
        
        serde_json::to_string_pretty(&map).unwrap_or_default()
    }
    
    /// Import asset index from JSON
    pub fn import_index(&self, json: &str) -> Result<usize, serde_json::Error> {
        let map: std::collections::HashMap<String, String> = serde_json::from_str(json)?;
        let mut index = self.index.write();
        let mut count = 0;
        
        for (name, id_str) in map {
            if let Ok(id) = id_str.parse::<ContentHash>() {
                index.insert(name, id);
                count += 1;
            }
        }
        
        Ok(count)
    }
    
    /// Resolve an asset with P2P fallback
    /// 
    /// Tries standard sources first, then falls back to P2P if available.
    /// Returns the result and whether P2P was used.
    pub fn resolve_with_p2p(
        &self,
        id: &ContentHash,
        peer_manager: &super::p2p::PeerManager,
        transfer_manager: &mut super::p2p::ChunkTransferManager,
        p2p_config: &super::p2p::P2PConfig,
    ) -> Result<(Vec<u8>, bool), ResolveError> {
        // Try standard resolution first
        match self.load(id) {
            Ok(data) => return Ok((data, false)),
            Err(ResolveError::AllSourcesFailed) => {
                // Fall through to P2P
            }
            Err(e) => return Err(e),
        }
        
        // Try P2P fallback
        let content_hash = id.to_base58();
        match super::p2p::fetch_asset_p2p(&content_hash, peer_manager, transfer_manager, p2p_config) {
            super::p2p::P2PFetchResult::Success(data) => {
                // Verify hash
                if id.verify(&data) {
                    // Cache the result
                    self.resolver.precache(id.clone(), data.clone());
                    Ok((data, true))
                } else {
                    Err(ResolveError::HashMismatch {
                        expected: id.to_base58(),
                        actual: ContentHash::from_content(&data).to_base58(),
                    })
                }
            }
            super::p2p::P2PFetchResult::InProgress { progress, speed: _ } => {
                Err(ResolveError::Network(format!(
                    "P2P download in progress: {:.1}%",
                    progress * 100.0
                )))
            }
            super::p2p::P2PFetchResult::NoPeers => {
                Err(ResolveError::Network("No P2P peers available".to_string()))
            }
            super::p2p::P2PFetchResult::Failed(e) => {
                Err(ResolveError::Network(format!("P2P failed: {}", e)))
            }
        }
    }
    
    /// Check P2P download status for an asset
    pub fn p2p_download_status(
        &self,
        id: &ContentHash,
        transfer_manager: &super::p2p::ChunkTransferManager,
    ) -> Option<(f32, f32)> {
        let content_hash = id.to_base58();
        transfer_manager.get_download(&content_hash)
            .map(|d| (d.progress(), d.speed()))
    }
    
    /// Start seeding an asset via P2P
    pub fn start_seeding(
        &self,
        id: &ContentHash,
        peer_manager: &mut super::p2p::PeerManager,
        chunk_size: usize,
    ) -> Result<(), ResolveError> {
        // Load the asset data
        let data = self.load(id)?;
        
        // Calculate chunk count
        let chunk_count = ((data.len() + chunk_size - 1) / chunk_size) as u32;
        
        // Register as seeder
        peer_manager.start_seeding(&id.to_base58(), chunk_count);
        
        Ok(())
    }
    
    /// Stop seeding an asset
    pub fn stop_seeding(
        &self,
        id: &ContentHash,
        peer_manager: &mut super::p2p::PeerManager,
    ) {
        peer_manager.stop_seeding(&id.to_base58());
    }
}

/// Component for entities with progressive asset loading
#[derive(Component)]
pub struct ProgressiveAssetState {
    /// LOD levels (index 0 = highest quality)
    pub lods: Vec<ContentHash>,
    /// Currently loaded LOD level
    pub current_lod: usize,
    /// Is currently loading?
    pub loading: bool,
    /// Placeholder asset (optional)
    pub placeholder: Option<ContentHash>,
}

impl ProgressiveAssetState {
    /// Create with LOD levels
    pub fn new(lods: Vec<ContentHash>) -> Self {
        Self {
            lods,
            current_lod: usize::MAX, // Not loaded
            loading: false,
            placeholder: None,
        }
    }
    
    /// Create with placeholder
    pub fn with_placeholder(mut self, placeholder: ContentHash) -> Self {
        self.placeholder = Some(placeholder);
        self
    }
}
