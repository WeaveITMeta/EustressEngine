//! Watch and subscription system for real-time updates
//!
//! Implements Kubernetes-style watch with:
//! - Resource version tracking
//! - Bookmark events
//! - Efficient fan-out to multiple subscribers

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use serde::{Deserialize, Serialize};

use super::ResourceKind;

/// Resource version for tracking changes
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceVersion(pub u64);

impl ResourceVersion {
    /// Create new resource version
    pub fn new(version: u64) -> Self {
        Self(version)
    }

    /// Get inner value
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for ResourceVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Watch event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchEvent {
    /// Resource was added
    Added {
        /// Resource kind
        kind: ResourceKind,
        /// Resource key
        key: String,
        /// Resource value
        value: serde_json::Value,
        /// Resource version
        version: u64,
    },
    /// Resource was modified
    Modified {
        /// Resource kind
        kind: ResourceKind,
        /// Resource key
        key: String,
        /// Resource value
        value: serde_json::Value,
        /// Resource version
        version: u64,
    },
    /// Resource was deleted
    Deleted {
        /// Resource kind
        kind: ResourceKind,
        /// Resource key
        key: String,
        /// Last known value
        value: serde_json::Value,
        /// Resource version
        version: u64,
    },
    /// Bookmark event (periodic version update)
    Bookmark {
        /// Resource kind
        kind: ResourceKind,
        /// Current resource version
        version: u64,
    },
    /// Error event
    Error {
        /// Error message
        message: String,
        /// Error code
        code: u32,
    },
}

impl WatchEvent {
    /// Get the event type as string
    pub fn event_type(&self) -> &str {
        match self {
            WatchEvent::Added { .. } => "ADDED",
            WatchEvent::Modified { .. } => "MODIFIED",
            WatchEvent::Deleted { .. } => "DELETED",
            WatchEvent::Bookmark { .. } => "BOOKMARK",
            WatchEvent::Error { .. } => "ERROR",
        }
    }

    /// Get the resource version
    pub fn version(&self) -> Option<u64> {
        match self {
            WatchEvent::Added { version, .. } => Some(*version),
            WatchEvent::Modified { version, .. } => Some(*version),
            WatchEvent::Deleted { version, .. } => Some(*version),
            WatchEvent::Bookmark { version, .. } => Some(*version),
            WatchEvent::Error { .. } => None,
        }
    }
}

/// Watch stream for receiving events
pub struct WatchStream {
    /// Broadcast receiver
    rx: broadcast::Receiver<WatchEvent>,
    /// Resource kind being watched
    kind: ResourceKind,
    /// Minimum resource version to receive
    min_version: Option<u64>,
}

impl WatchStream {
    /// Create new watch stream
    pub fn new(rx: broadcast::Receiver<WatchEvent>, kind: ResourceKind) -> Self {
        Self {
            rx,
            kind,
            min_version: None,
        }
    }

    /// Set minimum resource version
    pub fn from_version(mut self, version: u64) -> Self {
        self.min_version = Some(version);
        self
    }

    /// Receive next event
    pub async fn recv(&mut self) -> Option<WatchEvent> {
        loop {
            match self.rx.recv().await {
                Ok(event) => {
                    // Filter by kind
                    let matches_kind = match &event {
                        WatchEvent::Added { kind, .. } => kind == &self.kind,
                        WatchEvent::Modified { kind, .. } => kind == &self.kind,
                        WatchEvent::Deleted { kind, .. } => kind == &self.kind,
                        WatchEvent::Bookmark { kind, .. } => kind == &self.kind,
                        WatchEvent::Error { .. } => true,
                    };

                    if !matches_kind {
                        continue;
                    }

                    // Filter by version
                    if let Some(min_version) = self.min_version {
                        if let Some(version) = event.version() {
                            if version <= min_version {
                                continue;
                            }
                        }
                    }

                    return Some(event);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Subscriber fell behind, return error event
                    return Some(WatchEvent::Error {
                        message: format!("Watch lagged by {} events", n),
                        code: 410, // Gone
                    });
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    }
}

/// Watch registry for managing subscriptions
pub struct WatchRegistry {
    /// Broadcast sender for events
    tx: broadcast::Sender<WatchEvent>,
    /// Active watchers by kind
    watchers: RwLock<HashMap<ResourceKind, usize>>,
}

impl WatchRegistry {
    /// Create new watch registry
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            tx,
            watchers: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to events for a resource kind
    pub fn subscribe(&self, kind: ResourceKind) -> WatchStream {
        let rx = self.tx.subscribe();
        
        // Track watcher count
        let mut watchers = self.watchers.write();
        *watchers.entry(kind.clone()).or_insert(0) += 1;
        
        WatchStream::new(rx, kind)
    }

    /// Notify all watchers of an event
    pub fn notify(&self, event: WatchEvent) {
        // Ignore send errors (no receivers)
        let _ = self.tx.send(event);
    }

    /// Get number of watchers for a kind
    pub fn watcher_count(&self, kind: &ResourceKind) -> usize {
        self.watchers.read().get(kind).copied().unwrap_or(0)
    }

    /// Send bookmark events to all watchers
    pub fn send_bookmarks(&self, version: u64) {
        let kinds: Vec<_> = self.watchers.read().keys().cloned().collect();
        
        for kind in kinds {
            self.notify(WatchEvent::Bookmark {
                kind,
                version,
            });
        }
    }
}

impl Default for WatchRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Watch cache for efficient list-then-watch
pub struct WatchCache<T> {
    /// Cached items
    items: RwLock<HashMap<String, (T, u64)>>,
    /// Current resource version
    version: RwLock<u64>,
    /// Watch registry
    registry: Arc<WatchRegistry>,
    /// Resource kind
    kind: ResourceKind,
}

impl<T: Clone + Send + Sync + 'static> WatchCache<T> {
    /// Create new watch cache
    pub fn new(kind: ResourceKind, registry: Arc<WatchRegistry>) -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
            version: RwLock::new(0),
            registry,
            kind,
        }
    }

    /// Add or update item
    pub fn set(&self, key: impl Into<String>, value: T, version: u64) {
        let key = key.into();
        let mut items = self.items.write();
        let is_new = !items.contains_key(&key);
        items.insert(key.clone(), (value, version));
        
        *self.version.write() = version;
    }

    /// Get item
    pub fn get(&self, key: &str) -> Option<T> {
        self.items.read().get(key).map(|(v, _)| v.clone())
    }

    /// Remove item
    pub fn remove(&self, key: &str) -> Option<T> {
        self.items.write().remove(key).map(|(v, _)| v)
    }

    /// List all items
    pub fn list(&self) -> Vec<T> {
        self.items.read().values().map(|(v, _)| v.clone()).collect()
    }

    /// Get current version
    pub fn version(&self) -> u64 {
        *self.version.read()
    }

    /// Subscribe to changes
    pub fn watch(&self) -> WatchStream {
        self.registry.subscribe(self.kind.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watch_event_type() {
        let event = WatchEvent::Added {
            kind: ResourceKind::Workload,
            key: "test".to_string(),
            value: serde_json::json!({}),
            version: 1,
        };
        assert_eq!(event.event_type(), "ADDED");
        assert_eq!(event.version(), Some(1));
    }

    #[test]
    fn test_watch_registry() {
        let registry = WatchRegistry::new();
        
        let _stream = registry.subscribe(ResourceKind::Workload);
        assert_eq!(registry.watcher_count(&ResourceKind::Workload), 1);
    }

    #[tokio::test]
    async fn test_watch_stream() {
        let registry = Arc::new(WatchRegistry::new());
        let mut stream = registry.subscribe(ResourceKind::Workload);

        // Send event in background
        let registry_clone = registry.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            registry_clone.notify(WatchEvent::Added {
                kind: ResourceKind::Workload,
                key: "test".to_string(),
                value: serde_json::json!({"name": "test"}),
                version: 1,
            });
        });

        let event = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            stream.recv()
        ).await.unwrap();

        assert!(event.is_some());
        assert_eq!(event.unwrap().event_type(), "ADDED");
    }
}
