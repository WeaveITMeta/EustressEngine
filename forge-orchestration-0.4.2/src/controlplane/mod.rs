//! Control Plane for Forge Orchestration
//!
//! A complete control plane comparable to Kubernetes API server:
//! - RESTful API for workload management
//! - Watch/subscribe for real-time updates
//! - RBAC and authentication
//! - Admission controllers
//! - Resource versioning and optimistic concurrency

pub mod api;
pub mod admission;
pub mod watch;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use api::{ApiServer, ApiServerConfig};
pub use admission::{AdmissionController, AdmissionResult, ValidationWebhook, MutatingWebhook};
pub use watch::{WatchEvent, WatchStream, ResourceVersion};

/// Resource kind
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    /// Workload resource
    Workload,
    /// Node resource
    Node,
    /// Service resource
    Service,
    /// ConfigMap resource
    ConfigMap,
    /// Secret resource
    Secret,
    /// PriorityClass resource
    PriorityClass,
    /// Custom resource
    Custom(String),
}

impl std::fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceKind::Workload => write!(f, "workloads"),
            ResourceKind::Node => write!(f, "nodes"),
            ResourceKind::Service => write!(f, "services"),
            ResourceKind::ConfigMap => write!(f, "configmaps"),
            ResourceKind::Secret => write!(f, "secrets"),
            ResourceKind::PriorityClass => write!(f, "priorityclasses"),
            ResourceKind::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Object metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    /// Resource name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Unique identifier
    pub uid: String,
    /// Resource version for optimistic concurrency
    pub resource_version: u64,
    /// Generation (incremented on spec changes)
    pub generation: u64,
    /// Creation timestamp
    pub creation_timestamp: chrono::DateTime<chrono::Utc>,
    /// Deletion timestamp (if being deleted)
    pub deletion_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Labels
    pub labels: HashMap<String, String>,
    /// Annotations
    pub annotations: HashMap<String, String>,
    /// Owner references
    pub owner_references: Vec<OwnerReference>,
    /// Finalizers
    pub finalizers: Vec<String>,
}

impl ObjectMeta {
    /// Create new metadata
    pub fn new(name: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            namespace: namespace.into(),
            uid: uuid::Uuid::new_v4().to_string(),
            resource_version: 1,
            generation: 1,
            creation_timestamp: chrono::Utc::now(),
            deletion_timestamp: None,
            labels: HashMap::new(),
            annotations: HashMap::new(),
            owner_references: Vec::new(),
            finalizers: Vec::new(),
        }
    }

    /// Add label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add annotation
    pub fn with_annotation(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.annotations.insert(key.into(), value.into());
        self
    }

    /// Add finalizer
    pub fn with_finalizer(mut self, finalizer: impl Into<String>) -> Self {
        self.finalizers.push(finalizer.into());
        self
    }

    /// Increment resource version
    pub fn bump_version(&mut self) {
        self.resource_version += 1;
    }

    /// Increment generation
    pub fn bump_generation(&mut self) {
        self.generation += 1;
        self.bump_version();
    }
}

/// Owner reference for garbage collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerReference {
    /// API version
    pub api_version: String,
    /// Resource kind
    pub kind: String,
    /// Owner name
    pub name: String,
    /// Owner UID
    pub uid: String,
    /// Controller flag
    pub controller: bool,
    /// Block owner deletion
    pub block_owner_deletion: bool,
}

/// Generic resource wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource<T> {
    /// API version
    pub api_version: String,
    /// Resource kind
    pub kind: ResourceKind,
    /// Metadata
    pub metadata: ObjectMeta,
    /// Spec
    pub spec: T,
    /// Status (optional)
    pub status: Option<serde_json::Value>,
}

impl<T> Resource<T> {
    /// Create new resource
    pub fn new(kind: ResourceKind, name: impl Into<String>, spec: T) -> Self {
        Self {
            api_version: "forge.io/v1".to_string(),
            kind,
            metadata: ObjectMeta::new(name, "default"),
            spec,
            status: None,
        }
    }

    /// Set namespace
    pub fn in_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.metadata.namespace = namespace.into();
        self
    }

    /// Add label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata = self.metadata.with_label(key, value);
        self
    }
}

/// Resource store for the control plane
pub struct ResourceStore {
    /// Resources by kind and namespace/name
    resources: RwLock<HashMap<ResourceKind, HashMap<String, serde_json::Value>>>,
    /// Global resource version counter
    version_counter: RwLock<u64>,
    /// Watch subscribers
    watchers: Arc<watch::WatchRegistry>,
}

impl ResourceStore {
    /// Create new resource store
    pub fn new() -> Self {
        Self {
            resources: RwLock::new(HashMap::new()),
            version_counter: RwLock::new(0),
            watchers: Arc::new(watch::WatchRegistry::new()),
        }
    }

    /// Get next resource version
    fn next_version(&self) -> u64 {
        let mut counter = self.version_counter.write();
        *counter += 1;
        *counter
    }

    /// Create resource
    pub fn create(&self, kind: ResourceKind, key: &str, value: serde_json::Value) -> Result<u64, StoreError> {
        let mut resources = self.resources.write();
        let kind_map = resources.entry(kind.clone()).or_insert_with(HashMap::new);

        if kind_map.contains_key(key) {
            return Err(StoreError::AlreadyExists(key.to_string()));
        }

        let version = self.next_version();
        kind_map.insert(key.to_string(), value.clone());

        // Notify watchers
        self.watchers.notify(WatchEvent::Added {
            kind: kind.clone(),
            key: key.to_string(),
            value,
            version,
        });

        info!(kind = %kind, key = key, version = version, "Resource created");
        Ok(version)
    }

    /// Get resource
    pub fn get(&self, kind: &ResourceKind, key: &str) -> Option<serde_json::Value> {
        self.resources.read()
            .get(kind)
            .and_then(|m| m.get(key).cloned())
    }

    /// Update resource
    pub fn update(&self, kind: ResourceKind, key: &str, value: serde_json::Value, expected_version: Option<u64>) -> Result<u64, StoreError> {
        let mut resources = self.resources.write();
        let kind_map = resources.entry(kind.clone()).or_insert_with(HashMap::new);

        if !kind_map.contains_key(key) {
            return Err(StoreError::NotFound(key.to_string()));
        }

        // Check version for optimistic concurrency
        if let Some(expected) = expected_version {
            // In a real implementation, we'd check the stored version
            let current = *self.version_counter.read();
            if current != expected {
                return Err(StoreError::Conflict(expected, current));
            }
        }

        let version = self.next_version();
        kind_map.insert(key.to_string(), value.clone());

        // Notify watchers
        self.watchers.notify(WatchEvent::Modified {
            kind: kind.clone(),
            key: key.to_string(),
            value,
            version,
        });

        Ok(version)
    }

    /// Delete resource
    pub fn delete(&self, kind: &ResourceKind, key: &str) -> Result<(), StoreError> {
        let mut resources = self.resources.write();
        
        if let Some(kind_map) = resources.get_mut(kind) {
            if let Some(value) = kind_map.remove(key) {
                let version = self.next_version();
                
                // Notify watchers
                self.watchers.notify(WatchEvent::Deleted {
                    kind: kind.clone(),
                    key: key.to_string(),
                    value,
                    version,
                });

                info!(kind = %kind, key = key, "Resource deleted");
                return Ok(());
            }
        }

        Err(StoreError::NotFound(key.to_string()))
    }

    /// List resources of a kind
    pub fn list(&self, kind: &ResourceKind, namespace: Option<&str>) -> Vec<serde_json::Value> {
        self.resources.read()
            .get(kind)
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| {
                        namespace.map(|ns| k.starts_with(&format!("{}/", ns))).unwrap_or(true)
                    })
                    .map(|(_, v)| v.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Subscribe to watch events
    pub fn watch(&self, kind: ResourceKind) -> watch::WatchStream {
        self.watchers.subscribe(kind)
    }

    /// Get current resource version
    pub fn current_version(&self) -> u64 {
        *self.version_counter.read()
    }
}

impl Default for ResourceStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Store error
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Resource already exists
    #[error("Resource already exists: {0}")]
    AlreadyExists(String),
    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),
    /// Version conflict
    #[error("Version conflict: expected {0}, got {1}")]
    Conflict(u64, u64),
    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_store_crud() {
        let store = ResourceStore::new();
        let value = serde_json::json!({"name": "test"});

        // Create
        let version = store.create(ResourceKind::Workload, "default/test", value.clone()).unwrap();
        assert!(version > 0);

        // Get
        let retrieved = store.get(&ResourceKind::Workload, "default/test").unwrap();
        assert_eq!(retrieved, value);

        // Update
        let new_value = serde_json::json!({"name": "updated"});
        store.update(ResourceKind::Workload, "default/test", new_value.clone(), None).unwrap();
        
        let retrieved = store.get(&ResourceKind::Workload, "default/test").unwrap();
        assert_eq!(retrieved, new_value);

        // Delete
        store.delete(&ResourceKind::Workload, "default/test").unwrap();
        assert!(store.get(&ResourceKind::Workload, "default/test").is_none());
    }

    #[test]
    fn test_resource_store_conflict() {
        let store = ResourceStore::new();
        let value = serde_json::json!({"name": "test"});

        store.create(ResourceKind::Workload, "default/test", value.clone()).unwrap();

        // Try to create again
        let result = store.create(ResourceKind::Workload, "default/test", value);
        assert!(matches!(result, Err(StoreError::AlreadyExists(_))));
    }
}
