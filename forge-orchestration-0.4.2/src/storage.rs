//! Storage backends for Forge state management
//!
//! ## Table of Contents
//! - **StateStore**: Trait for state storage backends
//! - **MemoryStore**: In-memory store (default)
//! - **FileStore**: File-based persistent storage
//!
//! Optional backends (require feature flags):
//! - **RocksDbStore**: Local fast storage using RocksDB
//! - **EtcdStore**: Distributed storage using etcd

use crate::error::{ForgeError, Result};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Trait for state storage backends
#[async_trait]
pub trait StateStore: Send + Sync {
    /// Get a value by key
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a value
    async fn set(&self, key: &str, value: Vec<u8>) -> Result<()>;

    /// Delete a key
    async fn delete(&self, key: &str) -> Result<()>;

    /// List keys with a prefix
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>>;

    /// Store name for logging
    fn name(&self) -> &str;
}

/// Extension functions for StateStore (not in trait to keep it object-safe)

/// Check if a key exists in the store
pub async fn store_exists(store: &dyn StateStore, key: &str) -> Result<bool> {
    Ok(store.get(key).await?.is_some())
}

/// Get and deserialize JSON from the store
pub async fn store_get_json<T: DeserializeOwned>(store: &dyn StateStore, key: &str) -> Result<Option<T>> {
    match store.get(key).await? {
        Some(bytes) => {
            let value = serde_json::from_slice(&bytes)?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

/// Serialize and set JSON in the store
pub async fn store_set_json<T: Serialize>(store: &dyn StateStore, key: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    store.set(key, bytes).await
}

/// In-memory store for testing
#[derive(Debug, Default)]
pub struct MemoryStore {
    data: RwLock<HashMap<String, Vec<u8>>>,
}

impl MemoryStore {
    /// Create a new memory store
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl StateStore for MemoryStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(key);
        Ok(())
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let data = self.data.read().await;
        Ok(data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn name(&self) -> &str {
        "memory"
    }
}

/// File-based persistent storage
///
/// Simple JSON file storage for development and small deployments.
pub struct FileStore {
    path: PathBuf,
    data: RwLock<HashMap<String, Vec<u8>>>,
}

impl FileStore {
    /// Open or create a file store
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        let data = if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .map_err(|e| ForgeError::storage(format!("Failed to read store: {}", e)))?;
            serde_json::from_str(&contents).unwrap_or_default()
        } else {
            HashMap::new()
        };

        info!(path = %path.display(), "File store opened");

        Ok(Self {
            path,
            data: RwLock::new(data),
        })
    }

    /// Persist data to disk
    pub async fn flush(&self) -> Result<()> {
        let data = self.data.read().await;
        let contents = serde_json::to_string_pretty(&*data)?;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ForgeError::storage(format!("Failed to create dir: {}", e)))?;
        }

        std::fs::write(&self.path, contents)
            .map_err(|e| ForgeError::storage(format!("Failed to write store: {}", e)))?;

        debug!(path = %self.path.display(), "File store flushed");
        Ok(())
    }
}

#[async_trait]
impl StateStore for FileStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(key);
        Ok(())
    }

    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let data = self.data.read().await;
        Ok(data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn name(&self) -> &str {
        "file"
    }
}

/// Type alias for boxed store
pub type BoxedStateStore = Arc<dyn StateStore>;

/// Create a memory store
pub fn memory_store() -> BoxedStateStore {
    Arc::new(MemoryStore::new()) as BoxedStateStore
}

/// Key prefixes for different data types
pub mod keys {
    /// Job key prefix
    pub const JOBS: &str = "forge/jobs";
    /// Shard key prefix
    pub const SHARDS: &str = "forge/shards";
    /// Expert key prefix
    pub const EXPERTS: &str = "forge/experts";
    /// Node key prefix
    pub const NODES: &str = "forge/nodes";
    /// Config key prefix
    pub const CONFIG: &str = "forge/config";
    /// Metrics key prefix
    pub const METRICS: &str = "forge/metrics";

    /// Build a job key
    pub fn job(id: &str) -> String {
        format!("{}/{}", JOBS, id)
    }

    /// Build a shard key
    pub fn shard(id: u64) -> String {
        format!("{}/{}", SHARDS, id)
    }

    /// Build an expert key
    pub fn expert(index: usize) -> String {
        format!("{}/{}", EXPERTS, index)
    }

    /// Build a node key
    pub fn node(id: &str) -> String {
        format!("{}/{}", NODES, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store_basic() {
        let store = MemoryStore::new();

        // Set and get
        store.set("key1", b"value1".to_vec()).await.unwrap();
        let value = store.get("key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Delete
        store.delete("key1").await.unwrap();
        let value = store.get("key1").await.unwrap();
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_memory_store_prefix() {
        let store = MemoryStore::new();

        store.set("prefix/a", b"1".to_vec()).await.unwrap();
        store.set("prefix/b", b"2".to_vec()).await.unwrap();
        store.set("other/c", b"3".to_vec()).await.unwrap();

        let keys = store.list_prefix("prefix/").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"prefix/a".to_string()));
        assert!(keys.contains(&"prefix/b".to_string()));
    }

    #[tokio::test]
    async fn test_memory_store_json() {
        let store = MemoryStore::new();

        #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        store_set_json(&store, "json_key", &data).await.unwrap();
        let loaded: Option<TestData> = store_get_json(&store, "json_key").await.unwrap();
        assert_eq!(loaded, Some(data));
    }

    #[test]
    fn test_key_builders() {
        assert_eq!(keys::job("my-job"), "forge/jobs/my-job");
        assert_eq!(keys::shard(42), "forge/shards/42");
        assert_eq!(keys::expert(0), "forge/experts/0");
    }
}
