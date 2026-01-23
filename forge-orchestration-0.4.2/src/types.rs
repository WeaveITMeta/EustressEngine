//! Core types for Forge orchestration
//!
//! ## Table of Contents
//! - **NodeId**: Unique identifier for cluster nodes
//! - **ShardId**: Unique identifier for data/work shards
//! - **Shard**: Represents a unit of distributed work
//! - **Expert**: MoE expert instance for routing

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a cluster node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Create a new random NodeId
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create a NodeId from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node-{}", &self.0.to_string()[..8])
    }
}

/// Unique identifier for a shard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardId(u64);

impl ShardId {
    /// Create a new ShardId from a u64
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for ShardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shard-{}", self.0)
    }
}

impl From<u64> for ShardId {
    fn from(id: u64) -> Self {
        Self::new(id)
    }
}

/// Represents a unit of distributed work or data partition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    /// Unique shard identifier
    pub id: ShardId,
    /// Node currently owning this shard
    pub owner: Option<NodeId>,
    /// Shard state
    pub state: ShardState,
    /// Number of replicas
    pub replicas: u32,
    /// Metadata key-value pairs
    pub metadata: std::collections::HashMap<String, String>,
}

impl Shard {
    /// Create a new shard with the given ID
    pub fn new(id: impl Into<ShardId>) -> Self {
        Self {
            id: id.into(),
            owner: None,
            state: ShardState::Pending,
            replicas: 1,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set the shard owner
    pub fn with_owner(mut self, owner: NodeId) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Set the number of replicas
    pub fn with_replicas(mut self, replicas: u32) -> Self {
        self.replicas = replicas;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// State of a shard in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardState {
    /// Shard is pending allocation
    Pending,
    /// Shard is being allocated to a node
    Allocating,
    /// Shard is active and serving
    Active,
    /// Shard is being migrated
    Migrating,
    /// Shard is draining before shutdown
    Draining,
    /// Shard has failed
    Failed,
}

/// Represents an MoE expert instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expert {
    /// Expert index (0-based)
    pub index: usize,
    /// Node hosting this expert
    pub node: NodeId,
    /// Expert capacity (concurrent requests)
    pub capacity: u32,
    /// Current load (0.0 - 1.0)
    pub load: f64,
    /// Whether expert is healthy
    pub healthy: bool,
    /// GPU resources available to this expert
    pub gpu: Option<GpuResources>,
    /// Model version running on this expert
    pub model_version: Option<String>,
    /// Expert-specific metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl Expert {
    /// Create a new expert with the given index
    pub fn new(index: usize, node: NodeId) -> Self {
        Self {
            index,
            node,
            capacity: 100,
            load: 0.0,
            healthy: true,
            gpu: None,
            model_version: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set expert capacity
    pub fn with_capacity(mut self, capacity: u32) -> Self {
        self.capacity = capacity;
        self
    }

    /// Set GPU resources
    pub fn with_gpu(mut self, gpu: GpuResources) -> Self {
        self.gpu = Some(gpu);
        self
    }

    /// Set model version
    pub fn with_model_version(mut self, version: impl Into<String>) -> Self {
        self.model_version = Some(version.into());
        self
    }

    /// Check if expert can accept more work
    pub fn available(&self) -> bool {
        self.healthy && self.load < 0.95
    }

    /// Check if expert has GPU
    pub fn has_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// Check if expert has sufficient GPU memory
    pub fn has_gpu_memory(&self, required_mb: u64) -> bool {
        self.gpu.as_ref().map(|g| g.available_memory_mb() >= required_mb).unwrap_or(false)
    }

    /// Update load factor
    pub fn update_load(&mut self, load: f64) {
        self.load = load.clamp(0.0, 1.0);
    }
}

/// GPU resources for an expert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuResources {
    /// GPU device index
    pub device_id: u32,
    /// GPU model name (e.g., "NVIDIA A100")
    pub model: String,
    /// Total VRAM in MB
    pub memory_mb: u64,
    /// Used VRAM in MB
    pub memory_used_mb: u64,
    /// GPU utilization (0.0 - 1.0)
    pub utilization: f64,
    /// Compute capability (e.g., 8.0 for A100)
    pub compute_capability: Option<f32>,
    /// Whether GPU supports tensor cores
    pub tensor_cores: bool,
}

impl GpuResources {
    /// Create new GPU resources
    pub fn new(device_id: u32, model: impl Into<String>, memory_mb: u64) -> Self {
        Self {
            device_id,
            model: model.into(),
            memory_mb,
            memory_used_mb: 0,
            utilization: 0.0,
            compute_capability: None,
            tensor_cores: false,
        }
    }

    /// Set compute capability
    pub fn with_compute_capability(mut self, cc: f32) -> Self {
        self.compute_capability = Some(cc);
        self
    }

    /// Set tensor core support
    pub fn with_tensor_cores(mut self, supported: bool) -> Self {
        self.tensor_cores = supported;
        self
    }

    /// Get available memory
    pub fn available_memory_mb(&self) -> u64 {
        self.memory_mb.saturating_sub(self.memory_used_mb)
    }

    /// Update memory usage
    pub fn update_memory(&mut self, used_mb: u64) {
        self.memory_used_mb = used_mb.min(self.memory_mb);
    }

    /// Update utilization
    pub fn update_utilization(&mut self, util: f64) {
        self.utilization = util.clamp(0.0, 1.0);
    }

    /// Check if GPU is available for work
    pub fn available(&self) -> bool {
        self.utilization < 0.95
    }
}

/// Region identifier for multi-region federation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Region(String);

impl Region {
    /// Create a new region
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Get region name
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for Region {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Region {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_display() {
        let id = NodeId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("node-"));
    }

    #[test]
    fn test_shard_builder() {
        let node = NodeId::new();
        let shard = Shard::new(42u64)
            .with_owner(node)
            .with_replicas(3)
            .with_metadata("type", "cache");

        assert_eq!(shard.id.as_u64(), 42);
        assert_eq!(shard.replicas, 3);
        assert_eq!(shard.metadata.get("type"), Some(&"cache".to_string()));
    }

    #[test]
    fn test_expert_availability() {
        let mut expert = Expert::new(0, NodeId::new());
        assert!(expert.available());

        expert.update_load(0.96);
        assert!(!expert.available());

        expert.update_load(0.5);
        expert.healthy = false;
        assert!(!expert.available());
    }
}
