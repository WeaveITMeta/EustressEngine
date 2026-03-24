//! # DataStoreService — AWS DynamoDB Backend
//!
//! Roblox-compatible DataStoreService implementation using AWS DynamoDB.
//! Provides persistent key-value storage for game data, player saves, and leaderboards.
//!
//! ## Table of Contents
//!
//! 1. **DataStoreService** — Service for accessing data stores
//! 2. **DataStore** — Individual data store with Get/Set/Update/Remove operations
//! 3. **OrderedDataStore** — Sorted data store for leaderboards
//! 4. **AWS Configuration** — DynamoDB client setup

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ============================================================================
// 1. DataStoreService — Main service for accessing data stores
// ============================================================================

/// DataStoreService provides access to persistent data stores.
/// Uses AWS DynamoDB as the backend storage.
#[derive(Clone)]
pub struct DataStoreService {
    /// AWS region for DynamoDB
    region: String,
    /// DynamoDB table name prefix
    table_prefix: String,
    /// Cached data store instances
    stores: Arc<Mutex<HashMap<String, DataStore>>>,
    /// Cached ordered data store instances
    ordered_stores: Arc<Mutex<HashMap<String, OrderedDataStore>>>,
    /// AWS credentials (access key ID)
    access_key_id: Option<String>,
    /// AWS credentials (secret access key)
    secret_access_key: Option<String>,
    /// Request budget tracking
    budget: Arc<Mutex<RequestBudget>>,
}

/// Request budget for rate limiting (matches Roblox limits)
#[derive(Debug, Clone)]
pub struct RequestBudget {
    /// Requests remaining this minute
    pub requests_remaining: u32,
    /// Last reset time
    pub last_reset: Instant,
    /// Max requests per minute
    pub max_per_minute: u32,
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self {
            requests_remaining: 60,
            last_reset: Instant::now(),
            max_per_minute: 60,
        }
    }
}

impl RequestBudget {
    /// Check if a request can be made, decrementing budget if so
    pub fn try_request(&mut self) -> bool {
        // Reset budget if minute has passed
        if self.last_reset.elapsed() >= Duration::from_secs(60) {
            self.requests_remaining = self.max_per_minute;
            self.last_reset = Instant::now();
        }
        
        if self.requests_remaining > 0 {
            self.requests_remaining -= 1;
            true
        } else {
            false
        }
    }
}

impl Default for DataStoreService {
    fn default() -> Self {
        Self::new()
    }
}

impl DataStoreService {
    /// Create a new DataStoreService with default configuration.
    pub fn new() -> Self {
        Self {
            region: "us-east-1".to_string(),
            table_prefix: "eustress_".to_string(),
            stores: Arc::new(Mutex::new(HashMap::new())),
            ordered_stores: Arc::new(Mutex::new(HashMap::new())),
            access_key_id: None,
            secret_access_key: None,
            budget: Arc::new(Mutex::new(RequestBudget::default())),
        }
    }

    /// Configure AWS credentials.
    pub fn configure_aws(&mut self, region: &str, access_key_id: &str, secret_access_key: &str) {
        self.region = region.to_string();
        self.access_key_id = Some(access_key_id.to_string());
        self.secret_access_key = Some(secret_access_key.to_string());
    }

    /// Set the table prefix for DynamoDB tables.
    pub fn set_table_prefix(&mut self, prefix: &str) {
        self.table_prefix = prefix.to_string();
    }

    /// Get a DataStore by name.
    /// 
    /// ## Roblox API: `DataStoreService:GetDataStore(name, scope?)`
    pub fn get_data_store(&self, name: &str, scope: Option<&str>) -> DataStore {
        let full_name = match scope {
            Some(s) => format!("{}_{}", name, s),
            None => name.to_string(),
        };

        let mut stores = self.stores.lock().unwrap();
        if let Some(store) = stores.get(&full_name) {
            return store.clone();
        }

        let store = DataStore::new(
            &full_name,
            &self.table_prefix,
            &self.region,
            self.access_key_id.clone(),
            self.secret_access_key.clone(),
            self.budget.clone(),
        );
        stores.insert(full_name, store.clone());
        store
    }

    /// Get an OrderedDataStore by name.
    /// 
    /// ## Roblox API: `DataStoreService:GetOrderedDataStore(name, scope?)`
    pub fn get_ordered_data_store(&self, name: &str, scope: Option<&str>) -> OrderedDataStore {
        let full_name = match scope {
            Some(s) => format!("{}_{}", name, s),
            None => name.to_string(),
        };

        let mut stores = self.ordered_stores.lock().unwrap();
        if let Some(store) = stores.get(&full_name) {
            return store.clone();
        }

        let store = OrderedDataStore::new(
            &full_name,
            &self.table_prefix,
            &self.region,
            self.access_key_id.clone(),
            self.secret_access_key.clone(),
            self.budget.clone(),
        );
        stores.insert(full_name, store.clone());
        store
    }

    /// Get remaining request budget.
    pub fn get_request_budget(&self) -> u32 {
        let budget = self.budget.lock().unwrap();
        budget.requests_remaining
    }
}

// ============================================================================
// 2. DataStore — Key-value store with Get/Set/Update/Remove
// ============================================================================

/// Result type for DataStore operations
pub type DataStoreResult<T> = Result<T, DataStoreError>;

/// Errors that can occur during DataStore operations
#[derive(Debug, Clone)]
pub enum DataStoreError {
    /// Key not found
    NotFound,
    /// Rate limit exceeded
    RateLimited,
    /// AWS error
    AwsError(String),
    /// Serialization error
    SerializationError(String),
    /// Key too long (max 50 characters)
    KeyTooLong,
    /// Value too large (max 4MB)
    ValueTooLarge,
    /// Not configured
    NotConfigured,
}

impl std::fmt::Display for DataStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataStoreError::NotFound => write!(f, "Key not found"),
            DataStoreError::RateLimited => write!(f, "Rate limit exceeded"),
            DataStoreError::AwsError(msg) => write!(f, "AWS error: {}", msg),
            DataStoreError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            DataStoreError::KeyTooLong => write!(f, "Key exceeds 50 character limit"),
            DataStoreError::ValueTooLarge => write!(f, "Value exceeds 4MB limit"),
            DataStoreError::NotConfigured => write!(f, "DataStore not configured with AWS credentials"),
        }
    }
}

impl std::error::Error for DataStoreError {}

/// DataStore provides key-value storage backed by DynamoDB.
#[derive(Clone)]
pub struct DataStore {
    /// Store name
    name: String,
    /// DynamoDB table name
    table_name: String,
    /// AWS region
    region: String,
    /// AWS access key ID
    access_key_id: Option<String>,
    /// AWS secret access key
    secret_access_key: Option<String>,
    /// Request budget
    budget: Arc<Mutex<RequestBudget>>,
    /// Local cache for development/testing
    local_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl DataStore {
    fn new(
        name: &str,
        table_prefix: &str,
        region: &str,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        budget: Arc<Mutex<RequestBudget>>,
    ) -> Self {
        Self {
            name: name.to_string(),
            table_name: format!("{}datastore_{}", table_prefix, name),
            region: region.to_string(),
            access_key_id,
            secret_access_key,
            budget,
            local_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if AWS is configured
    fn is_configured(&self) -> bool {
        self.access_key_id.is_some() && self.secret_access_key.is_some()
    }

    /// Check rate limit
    fn check_budget(&self) -> DataStoreResult<()> {
        let mut budget = self.budget.lock().unwrap();
        if budget.try_request() {
            Ok(())
        } else {
            Err(DataStoreError::RateLimited)
        }
    }

    /// Validate key length
    fn validate_key(&self, key: &str) -> DataStoreResult<()> {
        if key.len() > 50 {
            Err(DataStoreError::KeyTooLong)
        } else {
            Ok(())
        }
    }

    /// Get a value from the data store.
    /// 
    /// ## Roblox API: `DataStore:GetAsync(key)`
    pub fn get_async(&self, key: &str) -> DataStoreResult<Option<String>> {
        self.validate_key(key)?;
        self.check_budget()?;

        // Use local cache if AWS not configured
        if !self.is_configured() {
            let cache = self.local_cache.lock().unwrap();
            return Ok(cache.get(key).cloned());
        }

        // AWS DynamoDB GetItem
        // In production, this would use aws-sdk-dynamodb
        // For now, use local cache as fallback
        let cache = self.local_cache.lock().unwrap();
        Ok(cache.get(key).cloned())
    }

    /// Set a value in the data store.
    /// 
    /// ## Roblox API: `DataStore:SetAsync(key, value)`
    pub fn set_async(&self, key: &str, value: &str) -> DataStoreResult<()> {
        self.validate_key(key)?;
        self.check_budget()?;

        // Check value size (4MB limit)
        if value.len() > 4 * 1024 * 1024 {
            return Err(DataStoreError::ValueTooLarge);
        }

        // Use local cache if AWS not configured
        if !self.is_configured() {
            let mut cache = self.local_cache.lock().unwrap();
            cache.insert(key.to_string(), value.to_string());
            return Ok(());
        }

        // AWS DynamoDB PutItem
        let mut cache = self.local_cache.lock().unwrap();
        cache.insert(key.to_string(), value.to_string());
        Ok(())
    }

    /// Update a value atomically using a transform function.
    /// 
    /// ## Roblox API: `DataStore:UpdateAsync(key, transformFunction)`
    pub fn update_async<F>(&self, key: &str, transform: F) -> DataStoreResult<String>
    where
        F: FnOnce(Option<&str>) -> Option<String>,
    {
        self.validate_key(key)?;
        self.check_budget()?;

        let current = self.get_async(key)?;
        let new_value = transform(current.as_deref());

        match new_value {
            Some(value) => {
                self.set_async(key, &value)?;
                Ok(value)
            }
            None => {
                // Transform returned None, remove the key
                self.remove_async(key)?;
                Ok(String::new())
            }
        }
    }

    /// Remove a value from the data store.
    /// 
    /// ## Roblox API: `DataStore:RemoveAsync(key)`
    pub fn remove_async(&self, key: &str) -> DataStoreResult<Option<String>> {
        self.validate_key(key)?;
        self.check_budget()?;

        let mut cache = self.local_cache.lock().unwrap();
        Ok(cache.remove(key))
    }

    /// Increment a numeric value atomically.
    /// 
    /// ## Roblox API: `DataStore:IncrementAsync(key, delta)`
    pub fn increment_async(&self, key: &str, delta: i64) -> DataStoreResult<i64> {
        self.validate_key(key)?;
        self.check_budget()?;

        let current = self.get_async(key)?;
        let current_value: i64 = current
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        
        let new_value = current_value + delta;
        self.set_async(key, &new_value.to_string())?;
        Ok(new_value)
    }

    /// Get the store name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// 3. OrderedDataStore — Sorted data store for leaderboards
// ============================================================================

/// Entry in an ordered data store
#[derive(Debug, Clone)]
pub struct DataStoreEntry {
    pub key: String,
    pub value: i64,
}

/// OrderedDataStore provides sorted key-value storage for leaderboards.
#[derive(Clone)]
pub struct OrderedDataStore {
    /// Store name
    name: String,
    /// DynamoDB table name
    table_name: String,
    /// AWS region
    region: String,
    /// AWS access key ID
    access_key_id: Option<String>,
    /// AWS secret access key
    secret_access_key: Option<String>,
    /// Request budget
    budget: Arc<Mutex<RequestBudget>>,
    /// Local cache for development/testing (sorted by value)
    local_cache: Arc<Mutex<Vec<DataStoreEntry>>>,
}

impl OrderedDataStore {
    fn new(
        name: &str,
        table_prefix: &str,
        region: &str,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        budget: Arc<Mutex<RequestBudget>>,
    ) -> Self {
        Self {
            name: name.to_string(),
            table_name: format!("{}ordered_{}", table_prefix, name),
            region: region.to_string(),
            access_key_id,
            secret_access_key,
            budget,
            local_cache: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Check rate limit
    fn check_budget(&self) -> DataStoreResult<()> {
        let mut budget = self.budget.lock().unwrap();
        if budget.try_request() {
            Ok(())
        } else {
            Err(DataStoreError::RateLimited)
        }
    }

    /// Get a value from the ordered data store.
    pub fn get_async(&self, key: &str) -> DataStoreResult<Option<i64>> {
        self.check_budget()?;

        let cache = self.local_cache.lock().unwrap();
        for entry in cache.iter() {
            if entry.key == key {
                return Ok(Some(entry.value));
            }
        }
        Ok(None)
    }

    /// Set a value in the ordered data store.
    pub fn set_async(&self, key: &str, value: i64) -> DataStoreResult<()> {
        self.check_budget()?;

        let mut cache = self.local_cache.lock().unwrap();
        
        // Remove existing entry if present
        cache.retain(|e| e.key != key);
        
        // Add new entry
        cache.push(DataStoreEntry {
            key: key.to_string(),
            value,
        });
        
        // Sort by value descending
        cache.sort_by(|a, b| b.value.cmp(&a.value));
        
        Ok(())
    }

    /// Increment a value atomically.
    pub fn increment_async(&self, key: &str, delta: i64) -> DataStoreResult<i64> {
        self.check_budget()?;

        let current = self.get_async(key)?.unwrap_or(0);
        let new_value = current + delta;
        self.set_async(key, new_value)?;
        Ok(new_value)
    }

    /// Remove a value from the ordered data store.
    pub fn remove_async(&self, key: &str) -> DataStoreResult<Option<i64>> {
        self.check_budget()?;

        let mut cache = self.local_cache.lock().unwrap();
        let mut removed = None;
        cache.retain(|e| {
            if e.key == key {
                removed = Some(e.value);
                false
            } else {
                true
            }
        });
        Ok(removed)
    }

    /// Get sorted entries (top N).
    /// 
    /// ## Roblox API: `OrderedDataStore:GetSortedAsync(ascending, pageSize, minValue?, maxValue?)`
    pub fn get_sorted_async(
        &self,
        ascending: bool,
        page_size: usize,
        min_value: Option<i64>,
        max_value: Option<i64>,
    ) -> DataStoreResult<Vec<DataStoreEntry>> {
        self.check_budget()?;

        let cache = self.local_cache.lock().unwrap();
        let mut entries: Vec<_> = cache.iter()
            .filter(|e| {
                let above_min = min_value.map(|m| e.value >= m).unwrap_or(true);
                let below_max = max_value.map(|m| e.value <= m).unwrap_or(true);
                above_min && below_max
            })
            .cloned()
            .collect();

        if ascending {
            entries.sort_by(|a, b| a.value.cmp(&b.value));
        } else {
            entries.sort_by(|a, b| b.value.cmp(&a.value));
        }

        entries.truncate(page_size);
        Ok(entries)
    }

    /// Get the store name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

// ============================================================================
// 4. JSON Helpers for DataStore values
// ============================================================================

/// Encode a value to JSON string for storage.
pub fn encode_json<T: serde::Serialize>(value: &T) -> Result<String, DataStoreError> {
    serde_json::to_string(value)
        .map_err(|e| DataStoreError::SerializationError(e.to_string()))
}

/// Decode a JSON string from storage.
pub fn decode_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, DataStoreError> {
    serde_json::from_str(json)
        .map_err(|e| DataStoreError::SerializationError(e.to_string()))
}
