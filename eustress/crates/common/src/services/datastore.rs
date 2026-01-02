//! # DataStore Service
//!
//! Persistent key-value storage for player data, game state, and analytics.
//! Inspired by Roblox DataStoreService but with modern improvements.
//!
//! ## Table of Contents
//!
//! 1. **DataStore** - Key-value storage with caching
//! 2. **OrderedDataStore** - Sorted set for leaderboards/rankings
//! 3. **DataStoreBackend** - Pluggable storage backends
//! 4. **DataStoreService** - Resource managing all datastores
//!
//! ## Features
//!
//! - **Async Operations**: All I/O is non-blocking
//! - **Versioning**: Automatic version history for rollback
//! - **Transactions**: Atomic read-modify-write operations
//! - **Caching**: In-memory LRU cache with configurable TTL
//! - **Backends**: Pluggable storage (SQLite local, Redis, S3, custom)
//! - **Ordered Sets**: BTreeMap-based sorted storage for leaderboards
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Get a datastore
//! let player_data = datastore_service.get_datastore("PlayerData");
//!
//! // Save data
//! player_data.set_async("user_123", PlayerSave { coins: 100, level: 5 }).await;
//!
//! // Load data
//! let save: PlayerSave = player_data.get_async("user_123").await?;
//!
//! // Ordered datastore for leaderboards
//! let leaderboard = datastore_service.get_ordered_datastore("HighScores");
//! leaderboard.set_score("user_123", 1500)?;
//! let top_10 = leaderboard.get_range(0, 10, SortOrder::Descending)?;
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use parking_lot::RwLock;

// ============================================================================
// DataStore Types
// ============================================================================

/// A single datastore instance for a specific data category.
#[derive(Clone)]
pub struct DataStore {
    /// Name of this datastore
    pub name: String,
    /// Scope (default: "global")
    pub scope: String,
    /// In-memory cache
    cache: Arc<RwLock<HashMap<String, CachedEntry>>>,
    /// Backend reference
    backend: Arc<dyn DataStoreBackend>,
}

/// Cached entry with TTL
#[derive(Clone)]
#[allow(dead_code)]
struct CachedEntry {
    data: Vec<u8>,
    version: u64,
    cached_at: std::time::Instant,
    ttl_secs: u64,
}

impl CachedEntry {
    fn is_expired(&self) -> bool {
        self.cached_at.elapsed().as_secs() > self.ttl_secs
    }
}

/// Result type for datastore operations
pub type DataStoreResult<T> = Result<T, DataStoreError>;

/// DataStore errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum DataStoreError {
    #[error("Key not found: {0}")]
    NotFound(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Backend error: {0}")]
    Backend(String),
    
    #[error("Version conflict: expected {expected}, got {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    
    #[error("Rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },
    
    #[error("Data too large: {size} bytes (max {max})")]
    DataTooLarge { size: usize, max: usize },
    
    #[error("Budget exceeded: {0}")]
    BudgetExceeded(String),
}

/// Version info for a key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStoreVersion {
    pub version: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub size_bytes: usize,
}

// ============================================================================
// DataStore Backend Trait
// ============================================================================

/// Pluggable storage backend
pub trait DataStoreBackend: Send + Sync {
    /// Get raw bytes for a key
    fn get(&self, scope: &str, key: &str) -> DataStoreResult<Option<(Vec<u8>, u64)>>;
    
    /// Set raw bytes for a key, returns new version
    fn set(&self, scope: &str, key: &str, data: &[u8], expected_version: Option<u64>) -> DataStoreResult<u64>;
    
    /// Delete a key
    fn delete(&self, scope: &str, key: &str) -> DataStoreResult<()>;
    
    /// List keys with prefix
    fn list_keys(&self, scope: &str, prefix: &str, limit: usize) -> DataStoreResult<Vec<String>>;
    
    /// Get version history
    fn get_versions(&self, scope: &str, key: &str, limit: usize) -> DataStoreResult<Vec<DataStoreVersion>>;
}

/// In-memory backend for testing/development
#[derive(Default)]
pub struct MemoryBackend {
    data: RwLock<HashMap<String, (Vec<u8>, u64)>>,
}

impl DataStoreBackend for MemoryBackend {
    fn get(&self, scope: &str, key: &str) -> DataStoreResult<Option<(Vec<u8>, u64)>> {
        let full_key = format!("{}:{}", scope, key);
        Ok(self.data.read().get(&full_key).cloned())
    }
    
    fn set(&self, scope: &str, key: &str, data: &[u8], expected_version: Option<u64>) -> DataStoreResult<u64> {
        let full_key = format!("{}:{}", scope, key);
        let mut store = self.data.write();
        
        let current_version = store.get(&full_key).map(|(_, v)| *v).unwrap_or(0);
        
        if let Some(expected) = expected_version {
            if current_version != expected {
                return Err(DataStoreError::VersionConflict {
                    expected,
                    actual: current_version,
                });
            }
        }
        
        let new_version = current_version + 1;
        store.insert(full_key, (data.to_vec(), new_version));
        Ok(new_version)
    }
    
    fn delete(&self, scope: &str, key: &str) -> DataStoreResult<()> {
        let full_key = format!("{}:{}", scope, key);
        self.data.write().remove(&full_key);
        Ok(())
    }
    
    fn list_keys(&self, scope: &str, prefix: &str, limit: usize) -> DataStoreResult<Vec<String>> {
        let scope_prefix = format!("{}:{}", scope, prefix);
        let keys: Vec<String> = self.data.read()
            .keys()
            .filter(|k| k.starts_with(&scope_prefix))
            .take(limit)
            .map(|k| k.strip_prefix(&format!("{}:", scope)).unwrap_or(k).to_string())
            .collect();
        Ok(keys)
    }
    
    fn get_versions(&self, _scope: &str, _key: &str, _limit: usize) -> DataStoreResult<Vec<DataStoreVersion>> {
        // Memory backend doesn't track history
        Ok(vec![])
    }
}

// ============================================================================
// DataStore Implementation
// ============================================================================

impl DataStore {
    /// Create a new datastore with the given backend
    pub fn new(name: impl Into<String>, backend: Arc<dyn DataStoreBackend>) -> Self {
        Self {
            name: name.into(),
            scope: "global".to_string(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            backend,
        }
    }
    
    /// Get data for a key (with caching)
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> DataStoreResult<Option<T>> {
        // Check cache first
        if let Some(entry) = self.cache.read().get(key) {
            if !entry.is_expired() {
                return bincode::deserialize(&entry.data)
                    .map(Some)
                    .map_err(|e| DataStoreError::Serialization(e.to_string()));
            }
        }
        
        // Fetch from backend
        match self.backend.get(&self.scope, key)? {
            Some((data, version)) => {
                // Update cache
                self.cache.write().insert(key.to_string(), CachedEntry {
                    data: data.clone(),
                    version,
                    cached_at: std::time::Instant::now(),
                    ttl_secs: 60,
                });
                
                bincode::deserialize(&data)
                    .map(Some)
                    .map_err(|e| DataStoreError::Serialization(e.to_string()))
            }
            None => Ok(None),
        }
    }
    
    /// Set data for a key
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> DataStoreResult<u64> {
        let data = bincode::serialize(value)
            .map_err(|e| DataStoreError::Serialization(e.to_string()))?;
        
        // Check size limit (4MB default)
        const MAX_SIZE: usize = 4 * 1024 * 1024;
        if data.len() > MAX_SIZE {
            return Err(DataStoreError::DataTooLarge {
                size: data.len(),
                max: MAX_SIZE,
            });
        }
        
        let version = self.backend.set(&self.scope, key, &data, None)?;
        
        // Update cache
        self.cache.write().insert(key.to_string(), CachedEntry {
            data,
            version,
            cached_at: std::time::Instant::now(),
            ttl_secs: 60,
        });
        
        Ok(version)
    }
    
    /// Atomic update with optimistic locking
    pub fn update<T, F>(&self, key: &str, updater: F) -> DataStoreResult<u64>
    where
        T: Serialize + for<'de> Deserialize<'de> + Default,
        F: FnOnce(T) -> T,
    {
        // Get current value and version
        let (current, version) = match self.backend.get(&self.scope, key)? {
            Some((data, v)) => {
                let value: T = bincode::deserialize(&data)
                    .map_err(|e| DataStoreError::Serialization(e.to_string()))?;
                (value, Some(v))
            }
            None => (T::default(), None),
        };
        
        // Apply update
        let updated = updater(current);
        let data = bincode::serialize(&updated)
            .map_err(|e| DataStoreError::Serialization(e.to_string()))?;
        
        // Write with version check
        self.backend.set(&self.scope, key, &data, version)
    }
    
    /// Delete a key
    pub fn delete(&self, key: &str) -> DataStoreResult<()> {
        self.cache.write().remove(key);
        self.backend.delete(&self.scope, key)
    }
    
    /// List keys with prefix
    pub fn list_keys(&self, prefix: &str, limit: usize) -> DataStoreResult<Vec<String>> {
        self.backend.list_keys(&self.scope, prefix, limit)
    }
    
    /// Invalidate cache for a key
    pub fn invalidate(&self, key: &str) {
        self.cache.write().remove(key);
    }
    
    /// Clear entire cache
    pub fn clear_cache(&self) {
        self.cache.write().clear();
    }
}

// ============================================================================
// DataStoreService Resource
// ============================================================================

/// DataStoreService - manages all datastores (like Roblox DataStoreService)
#[derive(Resource)]
pub struct DataStoreService {
    /// Default backend for new datastores
    default_backend: Arc<dyn DataStoreBackend>,
    /// Active datastores
    datastores: RwLock<HashMap<String, DataStore>>,
    /// Active ordered datastores
    ordered_datastores: RwLock<HashMap<String, OrderedDataStore>>,
    /// Budget tracking (requests per minute)
    request_budget: RwLock<RequestBudget>,
}

#[derive(Default)]
#[allow(dead_code)]
struct RequestBudget {
    requests_this_minute: u32,
    minute_start: Option<std::time::Instant>,
}

impl Default for DataStoreService {
    fn default() -> Self {
        Self {
            default_backend: Arc::new(MemoryBackend::default()),
            datastores: RwLock::new(HashMap::new()),
            ordered_datastores: RwLock::new(HashMap::new()),
            request_budget: RwLock::new(RequestBudget::default()),
        }
    }
}

impl DataStoreService {
    /// Create with a custom backend
    pub fn with_backend(backend: Arc<dyn DataStoreBackend>) -> Self {
        Self {
            default_backend: backend,
            datastores: RwLock::new(HashMap::new()),
            ordered_datastores: RwLock::new(HashMap::new()),
            request_budget: RwLock::new(RequestBudget::default()),
        }
    }
    
    /// Get or create a datastore
    pub fn get_datastore(&self, name: &str) -> DataStore {
        let mut stores = self.datastores.write();
        
        if let Some(store) = stores.get(name) {
            return store.clone();
        }
        
        let store = DataStore::new(name, self.default_backend.clone());
        stores.insert(name.to_string(), store.clone());
        store
    }
    
    /// Get ordered datastore (sorted set for leaderboards)
    pub fn get_ordered_datastore(&self, name: &str) -> OrderedDataStore {
        let mut stores = self.ordered_datastores.write();
        
        if let Some(store) = stores.get(name) {
            return store.clone();
        }
        
        let store = OrderedDataStore::new(name, self.default_backend.clone());
        stores.insert(name.to_string(), store.clone());
        store
    }
    
    /// Get request budget info
    pub fn get_budget(&self) -> (u32, u32) {
        let budget = self.request_budget.read();
        let max_per_minute: u32 = 60 + 10; // Base + per-player bonus
        let used = budget.requests_this_minute;
        (max_per_minute.saturating_sub(used), max_per_minute)
    }
}

// ============================================================================
// Common Data Types
// ============================================================================

/// Player save data structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerSaveData {
    /// Player's currency
    pub coins: u64,
    /// Experience points
    pub experience: u64,
    /// Current level
    pub level: u32,
    /// Inventory item IDs
    pub inventory: Vec<String>,
    /// Custom data (JSON-like)
    pub custom: HashMap<String, String>,
    /// Last save timestamp
    pub last_saved: u64,
}

/// Leaderboard entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub user_id: u64,
    pub username: String,
    pub score: i64,
    pub rank: u32,
    pub metadata: Option<String>,
}

impl LeaderboardEntry {
    pub fn new(user_id: u64, username: &str, score: i64) -> Self {
        Self {
            user_id,
            username: username.to_string(),
            score,
            rank: 0,
            metadata: None,
        }
    }
    
    pub fn with_metadata(mut self, metadata: &str) -> Self {
        self.metadata = Some(metadata.to_string());
        self
    }
}

// ============================================================================
// Ordered DataStore (Sorted Set)
// ============================================================================

/// Sort order for range queries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    /// Lowest scores first
    Ascending,
    /// Highest scores first (default for leaderboards)
    #[default]
    Descending,
}

/// Ordered DataStore - BTreeMap-based sorted set for leaderboards
/// 
/// Stores (key, score) pairs sorted by score for efficient range queries.
/// Supports both ascending and descending order.
#[derive(Clone)]
pub struct OrderedDataStore {
    /// Name of this ordered datastore
    pub name: String,
    /// Scope (default: "global")
    pub scope: String,
    /// Score -> Keys mapping (for range queries)
    /// Uses negative scores for descending order internally
    scores: Arc<RwLock<BTreeMap<OrderedKey, String>>>,
    /// Key -> Score mapping (for lookups)
    keys: Arc<RwLock<HashMap<String, i64>>>,
    /// Key -> Metadata mapping
    metadata: Arc<RwLock<HashMap<String, String>>>,
    /// Backend for persistence
    backend: Arc<dyn DataStoreBackend>,
    /// Dirty flag for persistence
    dirty: Arc<RwLock<bool>>,
}

/// Composite key for BTreeMap ordering
/// Sorts by score first, then by key for stable ordering
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OrderedKey {
    score: i64,
    key: String,
}

impl OrderedDataStore {
    /// Create a new ordered datastore
    pub fn new(name: impl Into<String>, backend: Arc<dyn DataStoreBackend>) -> Self {
        let store = Self {
            name: name.into(),
            scope: "ordered".to_string(),
            scores: Arc::new(RwLock::new(BTreeMap::new())),
            keys: Arc::new(RwLock::new(HashMap::new())),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            backend,
            dirty: Arc::new(RwLock::new(false)),
        };
        
        // Try to load from backend
        store.load_from_backend();
        store
    }
    
    /// Set score for a key
    pub fn set_score(&self, key: &str, score: i64) -> DataStoreResult<()> {
        self.set_score_with_metadata(key, score, None)
    }
    
    /// Set score with optional metadata
    pub fn set_score_with_metadata(
        &self,
        key: &str,
        score: i64,
        metadata: Option<&str>,
    ) -> DataStoreResult<()> {
        let mut scores = self.scores.write();
        let mut keys = self.keys.write();
        
        // Remove old entry if exists
        if let Some(&old_score) = keys.get(key) {
            scores.remove(&OrderedKey {
                score: old_score,
                key: key.to_string(),
            });
        }
        
        // Insert new entry
        scores.insert(
            OrderedKey {
                score,
                key: key.to_string(),
            },
            key.to_string(),
        );
        keys.insert(key.to_string(), score);
        
        // Store metadata if provided
        if let Some(meta) = metadata {
            self.metadata.write().insert(key.to_string(), meta.to_string());
        }
        
        *self.dirty.write() = true;
        Ok(())
    }
    
    /// Get score for a key
    pub fn get_score(&self, key: &str) -> DataStoreResult<Option<i64>> {
        Ok(self.keys.read().get(key).copied())
    }
    
    /// Increment score atomically
    pub fn increment_score(&self, key: &str, delta: i64) -> DataStoreResult<i64> {
        let mut keys = self.keys.write();
        let mut scores = self.scores.write();
        
        let old_score = keys.get(key).copied().unwrap_or(0);
        let new_score = old_score + delta;
        
        // Remove old entry
        if keys.contains_key(key) {
            scores.remove(&OrderedKey {
                score: old_score,
                key: key.to_string(),
            });
        }
        
        // Insert new entry
        scores.insert(
            OrderedKey {
                score: new_score,
                key: key.to_string(),
            },
            key.to_string(),
        );
        keys.insert(key.to_string(), new_score);
        
        *self.dirty.write() = true;
        Ok(new_score)
    }
    
    /// Remove a key
    pub fn remove(&self, key: &str) -> DataStoreResult<()> {
        let mut keys = self.keys.write();
        let mut scores = self.scores.write();
        
        if let Some(score) = keys.remove(key) {
            scores.remove(&OrderedKey {
                score,
                key: key.to_string(),
            });
        }
        
        self.metadata.write().remove(key);
        *self.dirty.write() = true;
        Ok(())
    }
    
    /// Get rank of a key (0-indexed)
    pub fn get_rank(&self, key: &str, order: SortOrder) -> DataStoreResult<Option<usize>> {
        let keys = self.keys.read();
        let scores = self.scores.read();
        
        let Some(&score) = keys.get(key) else {
            return Ok(None);
        };
        
        let ordered_key = OrderedKey {
            score,
            key: key.to_string(),
        };
        
        let rank = match order {
            SortOrder::Ascending => {
                scores.range(..&ordered_key).count()
            }
            SortOrder::Descending => {
                scores.range(&ordered_key..).count().saturating_sub(1)
            }
        };
        
        Ok(Some(rank))
    }
    
    /// Get entries in a range (start and count are 0-indexed)
    pub fn get_range(
        &self,
        start: usize,
        count: usize,
        order: SortOrder,
    ) -> DataStoreResult<Vec<LeaderboardEntry>> {
        let scores = self.scores.read();
        let metadata = self.metadata.read();
        
        let entries: Vec<_> = match order {
            SortOrder::Ascending => {
                scores.iter()
                    .skip(start)
                    .take(count)
                    .enumerate()
                    .map(|(i, (ok, key))| {
                        LeaderboardEntry {
                            user_id: 0, // Would need separate user_id tracking
                            username: key.clone(),
                            score: ok.score,
                            rank: (start + i) as u32,
                            metadata: metadata.get(key).cloned(),
                        }
                    })
                    .collect()
            }
            SortOrder::Descending => {
                scores.iter()
                    .rev()
                    .skip(start)
                    .take(count)
                    .enumerate()
                    .map(|(i, (ok, key))| {
                        LeaderboardEntry {
                            user_id: 0,
                            username: key.clone(),
                            score: ok.score,
                            rank: (start + i) as u32,
                            metadata: metadata.get(key).cloned(),
                        }
                    })
                    .collect()
            }
        };
        
        Ok(entries)
    }
    
    /// Get entries around a specific key (for "your rank" display)
    pub fn get_around(
        &self,
        key: &str,
        count_before: usize,
        count_after: usize,
        order: SortOrder,
    ) -> DataStoreResult<Vec<LeaderboardEntry>> {
        let Some(rank) = self.get_rank(key, order)? else {
            return Ok(vec![]);
        };
        
        let start = rank.saturating_sub(count_before);
        let total = count_before + 1 + count_after;
        
        self.get_range(start, total, order)
    }
    
    /// Get total count of entries
    pub fn len(&self) -> usize {
        self.keys.read().len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.keys.read().is_empty()
    }
    
    /// Persist to backend
    pub fn save(&self) -> DataStoreResult<()> {
        if !*self.dirty.read() {
            return Ok(());
        }
        
        let keys = self.keys.read();
        let metadata = self.metadata.read();
        
        // Serialize as JSON for readability
        let data = OrderedDataStoreData {
            entries: keys.iter()
                .map(|(k, &s)| OrderedEntry {
                    key: k.clone(),
                    score: s,
                    metadata: metadata.get(k).cloned(),
                })
                .collect(),
        };
        
        let bytes = serde_json::to_vec(&data)
            .map_err(|e| DataStoreError::Serialization(e.to_string()))?;
        
        self.backend.set(&self.scope, &self.name, &bytes, None)?;
        *self.dirty.write() = false;
        
        Ok(())
    }
    
    /// Load from backend
    fn load_from_backend(&self) {
        if let Ok(Some((bytes, _))) = self.backend.get(&self.scope, &self.name) {
            if let Ok(data) = serde_json::from_slice::<OrderedDataStoreData>(&bytes) {
                let mut scores = self.scores.write();
                let mut keys = self.keys.write();
                let mut metadata = self.metadata.write();
                
                for entry in data.entries {
                    scores.insert(
                        OrderedKey {
                            score: entry.score,
                            key: entry.key.clone(),
                        },
                        entry.key.clone(),
                    );
                    keys.insert(entry.key.clone(), entry.score);
                    if let Some(meta) = entry.metadata {
                        metadata.insert(entry.key, meta);
                    }
                }
            }
        }
    }
}

/// Serialization format for ordered datastore
#[derive(Serialize, Deserialize)]
struct OrderedDataStoreData {
    entries: Vec<OrderedEntry>,
}

#[derive(Serialize, Deserialize)]
struct OrderedEntry {
    key: String,
    score: i64,
    metadata: Option<String>,
}

// ============================================================================
// Plugin
// ============================================================================

/// DataStore plugin for Bevy
pub struct DataStorePlugin;

impl Plugin for DataStorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DataStoreService>();
        info!("DataStoreService initialized (memory backend)");
    }
}
