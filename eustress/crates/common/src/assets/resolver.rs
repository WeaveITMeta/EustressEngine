//! # AssetResolver - Multi-Source Asset Resolution
//!
//! Tries multiple sources in priority order until one succeeds.
//! Includes LRU cache for performance.

use super::{ContentHash, AssetSource};
use dashmap::DashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// Error types for asset resolution
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    #[error("Asset not found: {0}")]
    NotFound(String),
    
    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    
    #[error("IO error: {0}")]
    Io(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("All sources failed")]
    AllSourcesFailed,
}

/// Statistics for resolver performance
#[derive(Debug, Default)]
pub struct ResolverStats {
    pub cache_hits: AtomicUsize,
    pub cache_misses: AtomicUsize,
    pub source_attempts: AtomicUsize,
    pub source_successes: AtomicUsize,
    pub source_failures: AtomicUsize,
    pub bytes_loaded: AtomicUsize,
}

impl ResolverStats {
    /// Get cache hit ratio (0.0 - 1.0)
    pub fn cache_hit_ratio(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 { 0.0 } else { hits as f64 / total as f64 }
    }
}

/// LRU cache entry
struct CacheEntry {
    data: Vec<u8>,
    last_access: std::time::Instant,
    access_count: usize,
}

/// Multi-source asset resolver with LRU cache
pub struct AssetResolver {
    /// Registered sources (tried in order)
    sources: RwLock<Vec<AssetSource>>,
    
    /// In-memory cache (ContentHash -> data)
    cache: DashMap<ContentHash, CacheEntry>,
    
    /// Maximum cache size in bytes
    max_cache_bytes: usize,
    
    /// Current cache size
    cache_bytes: AtomicUsize,
    
    /// Local asset directory
    local_path: PathBuf,
    
    /// Performance statistics
    pub stats: ResolverStats,
    
    /// Pending load queue (for async loading)
    load_queue: RwLock<VecDeque<(ContentHash, u8)>>, // (id, priority)
}

impl AssetResolver {
    /// Create a new resolver with local path and cache size
    pub fn new(local_path: PathBuf, max_cache_mb: usize) -> Self {
        Self {
            sources: RwLock::new(Vec::new()),
            cache: DashMap::new(),
            max_cache_bytes: max_cache_mb * 1024 * 1024,
            cache_bytes: AtomicUsize::new(0),
            local_path,
            stats: ResolverStats::default(),
            load_queue: RwLock::new(VecDeque::new()),
        }
    }
    
    /// Add a source to try when resolving assets
    pub fn add_source(&self, source: AssetSource) {
        let mut sources = self.sources.write().unwrap();
        sources.push(source);
        // Sort by priority (lower = try first)
        sources.sort_by_key(|s| s.priority());
    }
    
    /// Clear all sources
    pub fn clear_sources(&self) {
        self.sources.write().unwrap().clear();
    }
    
    /// Queue an asset for loading
    pub fn queue_load(&self, id: ContentHash, priority: u8) {
        let mut queue = self.load_queue.write().unwrap();
        
        // Insert sorted by priority (higher priority first)
        let pos = queue.iter().position(|(_, p)| *p < priority).unwrap_or(queue.len());
        queue.insert(pos, (id, priority));
    }
    
    /// Get next asset to load from queue
    pub fn pop_load_queue(&self) -> Option<ContentHash> {
        self.load_queue.write().unwrap().pop_front().map(|(id, _)| id)
    }
    
    /// Resolve an asset synchronously (blocking)
    /// 
    /// Tries cache first, then each source in priority order.
    pub fn resolve_sync(&self, id: &ContentHash) -> Result<Vec<u8>, ResolveError> {
        // Check cache first
        if let Some(mut entry) = self.cache.get_mut(id) {
            entry.last_access = std::time::Instant::now();
            entry.access_count += 1;
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(entry.data.clone());
        }
        
        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        
        // Try each source
        let sources = self.sources.read().unwrap();
        
        for source in sources.iter() {
            self.stats.source_attempts.fetch_add(1, Ordering::Relaxed);
            
            match self.fetch_from_source_sync(source, id) {
                Ok(data) => {
                    // Verify hash
                    if id.verify(&data) {
                        self.stats.source_successes.fetch_add(1, Ordering::Relaxed);
                        self.stats.bytes_loaded.fetch_add(data.len(), Ordering::Relaxed);
                        
                        // Cache the result
                        self.cache_data(id.clone(), data.clone());
                        
                        return Ok(data);
                    } else {
                        warn!("Hash mismatch from source: {:?}", source.description());
                        self.stats.source_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    debug!("Source failed: {:?} - {}", source.description(), e);
                    self.stats.source_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        
        // Try local path as last resort
        let local_file = self.local_path.join(id.to_base58());
        if local_file.exists() {
            if let Ok(data) = std::fs::read(&local_file) {
                if id.verify(&data) {
                    self.cache_data(id.clone(), data.clone());
                    return Ok(data);
                }
            }
        }
        
        Err(ResolveError::AllSourcesFailed)
    }
    
    /// Fetch from a specific source (synchronous)
    fn fetch_from_source_sync(&self, source: &AssetSource, _id: &ContentHash) -> Result<Vec<u8>, ResolveError> {
        match source {
            AssetSource::Local(path) => {
                std::fs::read(path).map_err(|e| ResolveError::Io(e.to_string()))
            }
            
            AssetSource::Embedded(data) => {
                Ok(data.clone())
            }
            
            AssetSource::Url(url) => {
                // Synchronous HTTP request (blocking)
                #[cfg(feature = "async-assets")]
                {
                    // Use blocking reqwest
                    let client = reqwest::blocking::Client::new();
                    let response = client.get(url).write()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    if !response.status().is_success() {
                        return Err(ResolveError::Network(format!("HTTP {}", response.status())));
                    }
                    
                    response.bytes()
                        .map(|b| b.to_vec())
                        .map_err(|e| ResolveError::Network(e.to_string()))
                }
                
                #[cfg(not(feature = "async-assets"))]
                {
                    let _ = url;
                    Err(ResolveError::Network("async-assets feature not enabled".to_string()))
                }
            }
            
            AssetSource::Ipfs { gateway, cid } => {
                let url = format!("{}/ipfs/{}", gateway, cid);
                
                #[cfg(feature = "async-assets")]
                {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .build()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    let response = client.get(&url).write()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    response.bytes()
                        .map(|b| b.to_vec())
                        .map_err(|e| ResolveError::Network(e.to_string()))
                }
                
                #[cfg(not(feature = "async-assets"))]
                {
                    let _ = url;
                    Err(ResolveError::Network("async-assets feature not enabled".to_string()))
                }
            }
            
            AssetSource::S3 { bucket, key, region: _, endpoint } => {
                // S3/MinIO: Use HTTP endpoint directly for sync access
                #[cfg(feature = "async-assets")]
                {
                    let url = if let Some(ep) = endpoint {
                        // MinIO or custom S3-compatible endpoint
                        format!("{}/{}/{}", ep, bucket, key)
                    } else {
                        // AWS S3 virtual-hosted style (won't work without auth)
                        return Err(ResolveError::Network(
                            "AWS S3 requires async + auth. Use MinIO with endpoint for sync access.".to_string()
                        ));
                    };
                    
                    let client = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .build()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    let response = client.get(&url).write()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    if !response.status().is_success() {
                        return Err(ResolveError::Network(format!("S3 HTTP {}", response.status())));
                    }
                    
                    response.bytes()
                        .map(|b| b.to_vec())
                        .map_err(|e| ResolveError::Network(e.to_string()))
                }
                
                #[cfg(not(feature = "async-assets"))]
                {
                    let _ = (bucket, key, endpoint);
                    Err(ResolveError::Network("async-assets feature not enabled".to_string()))
                }
            }
            
            AssetSource::CloudflareR2 { account_id, bucket, key } => {
                // R2 public bucket access
                #[cfg(feature = "async-assets")]
                {
                    let url = format!("https://{}.r2.cloudflarestorage.com/{}/{}", account_id, bucket, key);
                    
                    let client = reqwest::blocking::Client::new();
                    let response = client.get(&url).write()
                        .map_err(|e| ResolveError::Network(e.to_string()))?;
                    
                    if !response.status().is_success() {
                        return Err(ResolveError::Network(format!("R2 HTTP {}", response.status())));
                    }
                    
                    response.bytes()
                        .map(|b| b.to_vec())
                        .map_err(|e| ResolveError::Network(e.to_string()))
                }
                
                #[cfg(not(feature = "async-assets"))]
                {
                    let _ = (account_id, bucket, key);
                    Err(ResolveError::Network("async-assets feature not enabled".to_string()))
                }
            }
            
            AssetSource::P2P { info_hash, trackers: _ } => {
                // P2P is handled separately via the P2P subsystem
                // This is a fallback that indicates P2P should be tried
                // The actual P2P resolution happens in the service layer
                let hash_hex: String = info_hash.iter().map(|b| format!("{:02x}", b)).collect();
                Err(ResolveError::Network(format!(
                    "P2P fallback for {}: use AssetService.resolve_with_p2p()",
                    hash_hex
                )))
            }
        }
    }
    
    /// Cache data with LRU eviction
    fn cache_data(&self, id: ContentHash, data: Vec<u8>) {
        let data_size = data.len();
        
        // Evict if needed
        while self.cache_bytes.load(Ordering::Relaxed) + data_size > self.max_cache_bytes {
            self.evict_lru();
        }
        
        // Insert
        self.cache.insert(id, CacheEntry {
            data,
            last_access: std::time::Instant::now(),
            access_count: 1,
        });
        
        self.cache_bytes.fetch_add(data_size, Ordering::Relaxed);
    }
    
    /// Evict least recently used entry
    fn evict_lru(&self) {
        let mut oldest_key = None;
        let mut oldest_time = std::time::Instant::now();
        
        for entry in self.cache.iter() {
            if entry.value().last_access < oldest_time {
                oldest_time = entry.value().last_access;
                oldest_key = Some(entry.key().clone());
            }
        }
        
        if let Some(key) = oldest_key {
            if let Some((_, entry)) = self.cache.remove(&key) {
                self.cache_bytes.fetch_sub(entry.data.len(), Ordering::Relaxed);
                info!("Evicted asset from cache: {:?}", key);
            }
        }
    }
    
    /// Check if an asset is in cache
    pub fn is_cached(&self, id: &ContentHash) -> bool {
        self.cache.contains_key(id)
    }
    
    /// Get cache size in bytes
    pub fn cache_size(&self) -> usize {
        self.cache_bytes.load(Ordering::Relaxed)
    }
    
    /// Get number of cached assets
    pub fn cache_count(&self) -> usize {
        self.cache.len()
    }
    
    /// Clear the cache
    pub fn clear_cache(&self) {
        self.cache.clear();
        self.cache_bytes.store(0, Ordering::Relaxed);
    }
    
    /// Cleanup old cache entries (called periodically)
    pub fn cleanup(&self) {
        let now = std::time::Instant::now();
        let max_age = std::time::Duration::from_secs(300); // 5 minutes
        
        let mut to_remove = Vec::new();
        
        for entry in self.cache.iter() {
            if now.duration_since(entry.value().last_access) > max_age 
               && entry.value().access_count < 3 {
                to_remove.push(entry.key().clone());
            }
        }
        
        for key in to_remove {
            if let Some((_, entry)) = self.cache.remove(&key) {
                self.cache_bytes.fetch_sub(entry.data.len(), Ordering::Relaxed);
            }
        }
    }
    
    /// Pre-cache an asset (for known assets)
    pub fn precache(&self, id: ContentHash, data: Vec<u8>) {
        if id.verify(&data) {
            self.cache_data(id, data);
        }
    }
}

#[cfg(feature = "async-assets")]
impl AssetResolver {
    /// Resolve an asset asynchronously
    pub async fn resolve_async(&self, id: &ContentHash) -> Result<Vec<u8>, ResolveError> {
        // Check cache first
        if let Some(mut entry) = self.cache.get_mut(id) {
            entry.last_access = std::time::Instant::now();
            entry.access_count += 1;
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(entry.data.clone());
        }
        
        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        
        // Try each source
        let sources = self.sources.read().unwrap().clone();
        let client = reqwest::Client::new();
        
        for source in sources.iter() {
            self.stats.source_attempts.fetch_add(1, Ordering::Relaxed);
            
            match self.fetch_from_source_async(&client, source).await {
                Ok(data) => {
                    if id.verify(&data) {
                        self.stats.source_successes.fetch_add(1, Ordering::Relaxed);
                        self.stats.bytes_loaded.fetch_add(data.len(), Ordering::Relaxed);
                        self.cache_data(id.clone(), data.clone());
                        return Ok(data);
                    } else {
                        self.stats.source_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    self.stats.source_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        
        Err(ResolveError::AllSourcesFailed)
    }
    
    /// Fetch from source asynchronously
    async fn fetch_from_source_async(
        &self,
        client: &reqwest::Client,
        source: &AssetSource,
    ) -> Result<Vec<u8>, ResolveError> {
        match source {
            AssetSource::Local(path) => {
                tokio::fs::read(path).await
                    .map_err(|e| ResolveError::Io(e.to_string()))
            }
            
            AssetSource::Embedded(data) => Ok(data.clone()),
            
            AssetSource::Url(url) => {
                let response = client.get(url).write().await
                    .map_err(|e| ResolveError::Network(e.to_string()))?;
                
                response.bytes().await
                    .map(|b| b.to_vec())
                    .map_err(|e| ResolveError::Network(e.to_string()))
            }
            
            AssetSource::Ipfs { gateway, cid } => {
                let url = format!("{}/ipfs/{}", gateway, cid);
                let response = client.get(&url)
                    .timeout(std::time::Duration::from_secs(30))
                    .write().await
                    .map_err(|e| ResolveError::Network(e.to_string()))?;
                
                response.bytes().await
                    .map(|b| b.to_vec())
                    .map_err(|e| ResolveError::Network(e.to_string()))
            }
            
            _ => Err(ResolveError::Network("Source type not implemented for async".to_string())),
        }
    }
}
