//! Cross-region replication for data and workloads
//!
//! Implements replication strategies for:
//! - Active-passive failover
//! - Active-active multi-region
//! - Data synchronization

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Replication policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationPolicy {
    /// No replication
    None,
    /// Replicate to specific regions
    Explicit { regions: Vec<String> },
    /// Replicate to N regions
    Count { count: usize },
    /// Replicate to all regions
    All,
    /// Replicate based on topology (e.g., one per zone)
    Topology { key: String, count_per_key: usize },
}

impl Default for ReplicationPolicy {
    fn default() -> Self {
        Self::None
    }
}

/// Replication status for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatus {
    /// Resource ID
    pub resource_id: String,
    /// Primary region
    pub primary_region: String,
    /// Replica regions
    pub replica_regions: Vec<String>,
    /// Replication state per region
    pub region_states: HashMap<String, ReplicaState>,
    /// Last sync time
    pub last_sync: chrono::DateTime<chrono::Utc>,
    /// Replication lag in milliseconds
    pub lag_ms: HashMap<String, u64>,
}

/// State of a replica
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplicaState {
    /// Replica is in sync
    InSync,
    /// Replica is syncing
    Syncing,
    /// Replica is lagging
    Lagging,
    /// Replica has failed
    Failed,
    /// Replica is being created
    Creating,
    /// Replica is being deleted
    Deleting,
}

/// Replication controller for managing cross-region replication
pub struct ReplicationController {
    /// Replication status by resource
    status: Arc<RwLock<HashMap<String, ReplicationStatus>>>,
    /// Default replication policy
    default_policy: RwLock<ReplicationPolicy>,
    /// Available regions
    regions: RwLock<HashSet<String>>,
    /// Replication callbacks
    callbacks: RwLock<Vec<Arc<dyn ReplicationCallback + Send + Sync>>>,
}

/// Callback for replication events
pub trait ReplicationCallback: Send + Sync {
    /// Called when replication is initiated
    fn on_replicate(&self, resource_id: &str, source: &str, target: &str);
    
    /// Called when replication completes
    fn on_sync_complete(&self, resource_id: &str, region: &str);
    
    /// Called when replication fails
    fn on_sync_failed(&self, resource_id: &str, region: &str, error: &str);
}

impl ReplicationController {
    /// Create new replication controller
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(HashMap::new())),
            default_policy: RwLock::new(ReplicationPolicy::None),
            regions: RwLock::new(HashSet::new()),
            callbacks: RwLock::new(Vec::new()),
        }
    }

    /// Set default replication policy
    pub fn set_default_policy(&self, policy: ReplicationPolicy) {
        *self.default_policy.write() = policy;
    }

    /// Register available region
    pub fn register_region(&self, region: impl Into<String>) {
        self.regions.write().insert(region.into());
    }

    /// Unregister region
    pub fn unregister_region(&self, region: &str) {
        self.regions.write().remove(region);
    }

    /// Add replication callback
    pub fn add_callback<C: ReplicationCallback + 'static>(&self, callback: C) {
        self.callbacks.write().push(Arc::new(callback));
    }

    /// Start replication for a resource
    pub fn replicate(&self, resource_id: impl Into<String>, primary_region: impl Into<String>, policy: Option<ReplicationPolicy>) {
        let resource_id = resource_id.into();
        let primary_region = primary_region.into();
        let policy = policy.unwrap_or_else(|| self.default_policy.read().clone());

        let target_regions = self.resolve_target_regions(&primary_region, &policy);
        
        if target_regions.is_empty() {
            debug!(resource_id = %resource_id, "No replication targets");
            return;
        }

        info!(
            resource_id = %resource_id,
            primary = %primary_region,
            targets = ?target_regions,
            "Starting replication"
        );

        // Initialize status
        let mut region_states = HashMap::new();
        region_states.insert(primary_region.clone(), ReplicaState::InSync);
        
        for region in &target_regions {
            region_states.insert(region.clone(), ReplicaState::Creating);
        }

        let status = ReplicationStatus {
            resource_id: resource_id.clone(),
            primary_region: primary_region.clone(),
            replica_regions: target_regions.clone(),
            region_states,
            last_sync: chrono::Utc::now(),
            lag_ms: HashMap::new(),
        };

        self.status.write().insert(resource_id.clone(), status);

        // Notify callbacks
        let callbacks = self.callbacks.read();
        for region in &target_regions {
            for callback in callbacks.iter() {
                callback.on_replicate(&resource_id, &primary_region, region);
            }
        }
    }

    /// Resolve target regions based on policy
    fn resolve_target_regions(&self, primary: &str, policy: &ReplicationPolicy) -> Vec<String> {
        let regions = self.regions.read();
        let available: Vec<_> = regions.iter()
            .filter(|r| *r != primary)
            .cloned()
            .collect();

        match policy {
            ReplicationPolicy::None => Vec::new(),
            ReplicationPolicy::Explicit { regions: targets } => {
                targets.iter()
                    .filter(|r| available.contains(r))
                    .cloned()
                    .collect()
            }
            ReplicationPolicy::Count { count } => {
                available.into_iter().take(*count).collect()
            }
            ReplicationPolicy::All => available,
            ReplicationPolicy::Topology { key, count_per_key } => {
                // Simplified - in real implementation would check topology labels
                available.into_iter().take(*count_per_key).collect()
            }
        }
    }

    /// Update replica state
    pub fn update_state(&self, resource_id: &str, region: &str, state: ReplicaState) {
        let mut statuses = self.status.write();
        
        if let Some(status) = statuses.get_mut(resource_id) {
            let old_state = status.region_states.get(region).copied();
            status.region_states.insert(region.to_string(), state);

            // Notify on state changes
            if old_state != Some(state) {
                let callbacks = self.callbacks.read();
                
                match state {
                    ReplicaState::InSync => {
                        for callback in callbacks.iter() {
                            callback.on_sync_complete(resource_id, region);
                        }
                    }
                    ReplicaState::Failed => {
                        for callback in callbacks.iter() {
                            callback.on_sync_failed(resource_id, region, "Replication failed");
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Update replication lag
    pub fn update_lag(&self, resource_id: &str, region: &str, lag_ms: u64) {
        let mut statuses = self.status.write();
        
        if let Some(status) = statuses.get_mut(resource_id) {
            status.lag_ms.insert(region.to_string(), lag_ms);
            status.last_sync = chrono::Utc::now();

            // Update state based on lag
            let state = if lag_ms == 0 {
                ReplicaState::InSync
            } else if lag_ms < 1000 {
                ReplicaState::Syncing
            } else {
                ReplicaState::Lagging
            };

            status.region_states.insert(region.to_string(), state);
        }
    }

    /// Get replication status
    pub fn get_status(&self, resource_id: &str) -> Option<ReplicationStatus> {
        self.status.read().get(resource_id).cloned()
    }

    /// Get all replicated resources
    pub fn list_replicated(&self) -> Vec<ReplicationStatus> {
        self.status.read().values().cloned().collect()
    }

    /// Stop replication for a resource
    pub fn stop_replication(&self, resource_id: &str) {
        if let Some(mut status) = self.status.write().remove(resource_id) {
            info!(resource_id = %resource_id, "Stopping replication");
            
            // Mark all replicas as deleting
            for (region, state) in status.region_states.iter_mut() {
                if region != &status.primary_region {
                    *state = ReplicaState::Deleting;
                }
            }
        }
    }

    /// Promote a replica to primary
    pub fn promote(&self, resource_id: &str, new_primary: &str) -> bool {
        let mut statuses = self.status.write();
        
        if let Some(status) = statuses.get_mut(resource_id) {
            if !status.replica_regions.contains(&new_primary.to_string()) 
                && status.primary_region != new_primary {
                warn!(
                    resource_id = %resource_id,
                    new_primary = %new_primary,
                    "Cannot promote: region is not a replica"
                );
                return false;
            }

            let old_primary = status.primary_region.clone();
            status.primary_region = new_primary.to_string();
            
            // Update replica list
            status.replica_regions.retain(|r| r != new_primary);
            if !status.replica_regions.contains(&old_primary) {
                status.replica_regions.push(old_primary);
            }

            info!(
                resource_id = %resource_id,
                old_primary = %status.replica_regions.last().unwrap_or(&String::new()),
                new_primary = %new_primary,
                "Promoted replica to primary"
            );

            return true;
        }

        false
    }

    /// Check if resource is replicated to a region
    pub fn is_replicated_to(&self, resource_id: &str, region: &str) -> bool {
        self.status.read()
            .get(resource_id)
            .map(|s| s.primary_region == region || s.replica_regions.contains(&region.to_string()))
            .unwrap_or(false)
    }

    /// Get healthy replicas for a resource
    pub fn healthy_replicas(&self, resource_id: &str) -> Vec<String> {
        self.status.read()
            .get(resource_id)
            .map(|s| {
                let mut healthy = vec![s.primary_region.clone()];
                healthy.extend(
                    s.region_states.iter()
                        .filter(|(r, state)| {
                            *r != &s.primary_region && **state == ReplicaState::InSync
                        })
                        .map(|(r, _)| r.clone())
                );
                healthy
            })
            .unwrap_or_default()
    }
}

impl Default for ReplicationController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replication_policy() {
        let controller = ReplicationController::new();
        
        controller.register_region("us-east-1");
        controller.register_region("us-west-2");
        controller.register_region("eu-west-1");

        controller.replicate(
            "resource-1",
            "us-east-1",
            Some(ReplicationPolicy::Count { count: 2 })
        );

        let status = controller.get_status("resource-1").unwrap();
        assert_eq!(status.primary_region, "us-east-1");
        assert_eq!(status.replica_regions.len(), 2);
    }

    #[test]
    fn test_replica_promotion() {
        let controller = ReplicationController::new();
        
        controller.register_region("us-east-1");
        controller.register_region("eu-west-1");

        controller.replicate(
            "resource-1",
            "us-east-1",
            Some(ReplicationPolicy::All)
        );

        // Mark replica as in sync
        controller.update_state("resource-1", "eu-west-1", ReplicaState::InSync);

        // Promote
        assert!(controller.promote("resource-1", "eu-west-1"));

        let status = controller.get_status("resource-1").unwrap();
        assert_eq!(status.primary_region, "eu-west-1");
        assert!(status.replica_regions.contains(&"us-east-1".to_string()));
    }

    #[test]
    fn test_healthy_replicas() {
        let controller = ReplicationController::new();
        
        controller.register_region("us-east-1");
        controller.register_region("us-west-2");
        controller.register_region("eu-west-1");

        controller.replicate(
            "resource-1",
            "us-east-1",
            Some(ReplicationPolicy::All)
        );

        controller.update_state("resource-1", "us-west-2", ReplicaState::InSync);
        controller.update_state("resource-1", "eu-west-1", ReplicaState::Failed);

        let healthy = controller.healthy_replicas("resource-1");
        assert!(healthy.contains(&"us-east-1".to_string()));
        assert!(healthy.contains(&"us-west-2".to_string()));
        assert!(!healthy.contains(&"eu-west-1".to_string()));
    }
}
