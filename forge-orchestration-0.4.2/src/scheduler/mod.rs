//! Distributed Scheduler for Forge Orchestration
//!
//! A full-featured scheduler comparable to Kubernetes, with:
//! - Bin-packing and spread scheduling algorithms
//! - Affinity/anti-affinity rules
//! - Resource-aware placement (CPU, memory, GPU)
//! - Preemption and priority-based scheduling
//! - Topology-aware scheduling for NUMA/GPU locality

pub mod algorithms;
pub mod optimized;
pub mod placement;
pub mod preemption;
pub mod queue;

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::types::{NodeId, GpuResources};

pub use algorithms::{SchedulingAlgorithm, BinPackScheduler, SpreadScheduler, GpuLocalityScheduler};
pub use optimized::{OptimizedScheduler, WorkloadBatch, SchedulerStats, ClusterUtilization, FFDBinPacker};
pub use placement::{PlacementConstraint, AffinityRule, Affinity};
pub use preemption::{PreemptionPolicy, PriorityClass};
pub use queue::{SchedulingQueue, QueuedWorkload};

/// Resource requirements for a workload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// CPU cores requested (millicores, 1000 = 1 core)
    pub cpu_millis: u64,
    /// Memory requested in MB
    pub memory_mb: u64,
    /// GPU count requested
    pub gpu_count: u32,
    /// GPU memory required per GPU in MB
    pub gpu_memory_mb: u64,
    /// Ephemeral storage in MB
    pub storage_mb: u64,
    /// Network bandwidth in Mbps
    pub network_mbps: u32,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_millis: 100,
            memory_mb: 128,
            gpu_count: 0,
            gpu_memory_mb: 0,
            storage_mb: 0,
            network_mbps: 0,
        }
    }
}

impl ResourceRequirements {
    /// Create new resource requirements
    pub fn new() -> Self {
        Self::default()
    }

    /// Set CPU requirement
    pub fn cpu(mut self, millis: u64) -> Self {
        self.cpu_millis = millis;
        self
    }

    /// Set memory requirement
    pub fn memory(mut self, mb: u64) -> Self {
        self.memory_mb = mb;
        self
    }

    /// Set GPU requirements
    pub fn gpu(mut self, count: u32, memory_mb: u64) -> Self {
        self.gpu_count = count;
        self.gpu_memory_mb = memory_mb;
        self
    }
}

/// Node resources and capacity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResources {
    /// Node identifier
    pub node_id: NodeId,
    /// Total CPU capacity (millicores)
    pub cpu_capacity: u64,
    /// Allocated CPU (millicores)
    pub cpu_allocated: u64,
    /// Total memory capacity (MB)
    pub memory_capacity: u64,
    /// Allocated memory (MB)
    pub memory_allocated: u64,
    /// GPU resources
    pub gpus: Vec<GpuResources>,
    /// GPUs allocated (by device ID)
    pub gpus_allocated: Vec<u32>,
    /// Node labels for affinity matching
    pub labels: HashMap<String, String>,
    /// Node taints
    pub taints: Vec<Taint>,
    /// Is node schedulable
    pub schedulable: bool,
    /// Node conditions
    pub conditions: Vec<NodeCondition>,
}

impl NodeResources {
    /// Create new node resources
    pub fn new(node_id: NodeId, cpu_capacity: u64, memory_capacity: u64) -> Self {
        Self {
            node_id,
            cpu_capacity,
            cpu_allocated: 0,
            memory_capacity,
            memory_allocated: 0,
            gpus: Vec::new(),
            gpus_allocated: Vec::new(),
            labels: HashMap::new(),
            taints: Vec::new(),
            schedulable: true,
            conditions: Vec::new(),
        }
    }

    /// Add GPU to node
    pub fn with_gpu(mut self, gpu: GpuResources) -> Self {
        self.gpus.push(gpu);
        self
    }

    /// Add label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add taint
    pub fn with_taint(mut self, taint: Taint) -> Self {
        self.taints.push(taint);
        self
    }

    /// Available CPU
    pub fn cpu_available(&self) -> u64 {
        self.cpu_capacity.saturating_sub(self.cpu_allocated)
    }

    /// Available memory
    pub fn memory_available(&self) -> u64 {
        self.memory_capacity.saturating_sub(self.memory_allocated)
    }

    /// Available GPUs
    pub fn gpus_available(&self) -> usize {
        self.gpus.len() - self.gpus_allocated.len()
    }

    /// Check if node can fit workload
    pub fn can_fit(&self, req: &ResourceRequirements) -> bool {
        if !self.schedulable {
            return false;
        }

        if self.cpu_available() < req.cpu_millis {
            return false;
        }

        if self.memory_available() < req.memory_mb {
            return false;
        }

        if req.gpu_count > 0 {
            let available_gpus: Vec<_> = self.gpus.iter()
                .filter(|g| !self.gpus_allocated.contains(&g.device_id))
                .filter(|g| g.available_memory_mb() >= req.gpu_memory_mb)
                .collect();
            
            if available_gpus.len() < req.gpu_count as usize {
                return false;
            }
        }

        true
    }

    /// Allocate resources for workload
    pub fn allocate(&mut self, req: &ResourceRequirements) -> bool {
        if !self.can_fit(req) {
            return false;
        }

        self.cpu_allocated += req.cpu_millis;
        self.memory_allocated += req.memory_mb;

        // Allocate GPUs
        for _ in 0..req.gpu_count {
            if let Some(gpu) = self.gpus.iter()
                .find(|g| !self.gpus_allocated.contains(&g.device_id) 
                    && g.available_memory_mb() >= req.gpu_memory_mb)
            {
                self.gpus_allocated.push(gpu.device_id);
            }
        }

        true
    }

    /// Release resources
    pub fn release(&mut self, req: &ResourceRequirements, gpu_ids: &[u32]) {
        self.cpu_allocated = self.cpu_allocated.saturating_sub(req.cpu_millis);
        self.memory_allocated = self.memory_allocated.saturating_sub(req.memory_mb);
        self.gpus_allocated.retain(|id| !gpu_ids.contains(id));
    }
}

/// Node taint for scheduling constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taint {
    /// Taint key
    pub key: String,
    /// Taint value
    pub value: String,
    /// Taint effect
    pub effect: TaintEffect,
}

/// Taint effect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaintEffect {
    /// Do not schedule new workloads
    NoSchedule,
    /// Prefer not to schedule
    PreferNoSchedule,
    /// Evict existing workloads
    NoExecute,
}

/// Node condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCondition {
    /// Condition type
    pub condition_type: String,
    /// Condition status
    pub status: bool,
    /// Last transition time
    pub last_transition: chrono::DateTime<chrono::Utc>,
    /// Reason for condition
    pub reason: Option<String>,
    /// Human-readable message
    pub message: Option<String>,
}

/// Workload to be scheduled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workload {
    /// Unique workload ID
    pub id: String,
    /// Workload name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Resource requirements
    pub resources: ResourceRequirements,
    /// Priority (higher = more important)
    pub priority: i32,
    /// Priority class name
    pub priority_class: Option<String>,
    /// Placement constraints
    pub constraints: Vec<PlacementConstraint>,
    /// Node affinity rules
    pub affinity: Option<Affinity>,
    /// Tolerations for taints
    pub tolerations: Vec<Toleration>,
    /// Preemption policy
    pub preemption_policy: PreemptionPolicy,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Workload {
    /// Create a new workload
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            namespace: "default".to_string(),
            resources: ResourceRequirements::default(),
            priority: 0,
            priority_class: None,
            constraints: Vec::new(),
            affinity: None,
            tolerations: Vec::new(),
            preemption_policy: PreemptionPolicy::PreemptLowerPriority,
            created_at: chrono::Utc::now(),
        }
    }

    /// Set resource requirements
    pub fn with_resources(mut self, resources: ResourceRequirements) -> Self {
        self.resources = resources;
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Add constraint
    pub fn with_constraint(mut self, constraint: PlacementConstraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    /// Set affinity
    pub fn with_affinity(mut self, affinity: Affinity) -> Self {
        self.affinity = Some(affinity);
        self
    }

    /// Add toleration
    pub fn with_toleration(mut self, toleration: Toleration) -> Self {
        self.tolerations.push(toleration);
        self
    }
}

/// Toleration for node taints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toleration {
    /// Key to match
    pub key: Option<String>,
    /// Operator for matching
    pub operator: TolerationOperator,
    /// Value to match
    pub value: Option<String>,
    /// Effect to tolerate
    pub effect: Option<TaintEffect>,
    /// Toleration seconds for NoExecute
    pub toleration_seconds: Option<u64>,
}

/// Toleration operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolerationOperator {
    /// Key must equal value
    Equal,
    /// Key must exist
    Exists,
}

/// Scheduling decision
#[derive(Debug, Clone)]
pub struct SchedulingDecision {
    /// Workload ID
    pub workload_id: String,
    /// Selected node
    pub node_id: Option<NodeId>,
    /// Score for the selected node
    pub score: f64,
    /// Reason for decision
    pub reason: String,
    /// Preempted workloads (if any)
    pub preempted: Vec<String>,
    /// Scheduling latency
    pub latency_ms: u64,
}

/// The main scheduler
pub struct Scheduler {
    /// Registered nodes
    nodes: Arc<RwLock<HashMap<NodeId, NodeResources>>>,
    /// Scheduling queue
    queue: Arc<SchedulingQueue>,
    /// Scheduling algorithm
    algorithm: Arc<dyn SchedulingAlgorithm + Send + Sync>,
    /// Scheduled workloads (workload_id -> node_id)
    assignments: Arc<RwLock<HashMap<String, NodeId>>>,
    /// Priority classes
    priority_classes: Arc<RwLock<HashMap<String, PriorityClass>>>,
}

impl Scheduler {
    /// Create a new scheduler with default algorithm
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(SchedulingQueue::new()),
            algorithm: Arc::new(BinPackScheduler::new()),
            assignments: Arc::new(RwLock::new(HashMap::new())),
            priority_classes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with specific algorithm
    pub fn with_algorithm<A: SchedulingAlgorithm + Send + Sync + 'static>(algorithm: A) -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(SchedulingQueue::new()),
            algorithm: Arc::new(algorithm),
            assignments: Arc::new(RwLock::new(HashMap::new())),
            priority_classes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a node
    pub fn register_node(&self, node: NodeResources) {
        info!(node_id = %node.node_id, "Registering node");
        self.nodes.write().insert(node.node_id, node);
    }

    /// Unregister a node
    pub fn unregister_node(&self, node_id: &NodeId) {
        info!(node_id = %node_id, "Unregistering node");
        self.nodes.write().remove(node_id);
    }

    /// Update node resources
    pub fn update_node(&self, node: NodeResources) {
        self.nodes.write().insert(node.node_id, node);
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.read().len()
    }

    /// Submit workload for scheduling
    pub fn submit(&self, workload: Workload) {
        debug!(workload_id = %workload.id, "Submitting workload");
        self.queue.enqueue(workload);
    }

    /// Schedule next workload from queue
    pub fn schedule_next(&self) -> Option<SchedulingDecision> {
        let workload = self.queue.dequeue()?;
        Some(self.schedule(&workload))
    }

    /// Schedule a specific workload
    pub fn schedule(&self, workload: &Workload) -> SchedulingDecision {
        let start = std::time::Instant::now();
        let nodes = self.nodes.read();

        // Filter nodes that can fit the workload
        let candidates: Vec<_> = nodes.values()
            .filter(|n| n.can_fit(&workload.resources))
            .filter(|n| self.check_constraints(workload, n))
            .filter(|n| self.check_taints(workload, n))
            .collect();

        if candidates.is_empty() {
            // Try preemption
            drop(nodes);
            if let Some(decision) = self.try_preemption(workload) {
                return decision;
            }

            return SchedulingDecision {
                workload_id: workload.id.clone(),
                node_id: None,
                score: 0.0,
                reason: "No suitable nodes available".to_string(),
                preempted: Vec::new(),
                latency_ms: start.elapsed().as_millis() as u64,
            };
        }

        // Score and select best node
        let scored: Vec<_> = candidates.iter()
            .map(|n| (n, self.algorithm.score(workload, n)))
            .collect();

        let (best_node, score) = scored.iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(n, s)| (*n, *s))
            .unwrap();

        let node_id = best_node.node_id;
        drop(nodes);

        // Allocate resources
        if let Some(node) = self.nodes.write().get_mut(&node_id) {
            node.allocate(&workload.resources);
        }

        self.assignments.write().insert(workload.id.clone(), node_id);

        info!(
            workload_id = %workload.id,
            node_id = %node_id,
            score = score,
            "Workload scheduled"
        );

        SchedulingDecision {
            workload_id: workload.id.clone(),
            node_id: Some(node_id),
            score,
            reason: "Scheduled successfully".to_string(),
            preempted: Vec::new(),
            latency_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Check placement constraints
    fn check_constraints(&self, workload: &Workload, node: &NodeResources) -> bool {
        for constraint in &workload.constraints {
            if !constraint.matches(node) {
                return false;
            }
        }

        // Check affinity
        if let Some(affinity) = &workload.affinity {
            if !affinity.matches(node) {
                return false;
            }
        }

        true
    }

    /// Check if workload tolerates node taints
    fn check_taints(&self, workload: &Workload, node: &NodeResources) -> bool {
        for taint in &node.taints {
            let tolerated = workload.tolerations.iter().any(|t| {
                // Check key match
                let key_matches = t.key.as_ref().map(|k| k == &taint.key).unwrap_or(true);
                
                // Check operator
                let value_matches = match t.operator {
                    TolerationOperator::Exists => true,
                    TolerationOperator::Equal => {
                        t.value.as_ref().map(|v| v == &taint.value).unwrap_or(false)
                    }
                };

                // Check effect
                let effect_matches = t.effect.map(|e| e == taint.effect).unwrap_or(true);

                key_matches && value_matches && effect_matches
            });

            if !tolerated && taint.effect == TaintEffect::NoSchedule {
                return false;
            }
        }

        true
    }

    /// Try to preempt lower priority workloads
    fn try_preemption(&self, workload: &Workload) -> Option<SchedulingDecision> {
        if workload.preemption_policy == PreemptionPolicy::Never {
            return None;
        }

        // Find workloads that can be preempted
        let assignments = self.assignments.read();
        let mut nodes = self.nodes.write();

        for (node_id, node) in nodes.iter_mut() {
            // Find lower priority workloads on this node
            let preemptable: Vec<_> = assignments.iter()
                .filter(|(_, n)| *n == node_id)
                .map(|(w, _)| w.clone())
                .collect();

            // Simulate releasing resources
            // In a real implementation, we'd track workload resources per node
            if node.can_fit(&workload.resources) || !preemptable.is_empty() {
                // For now, just return that preemption is possible
                warn!(
                    workload_id = %workload.id,
                    node_id = %node_id,
                    "Preemption would be required"
                );
            }
        }

        None
    }

    /// Get workload assignment
    pub fn get_assignment(&self, workload_id: &str) -> Option<NodeId> {
        self.assignments.read().get(workload_id).copied()
    }

    /// Release workload resources
    pub fn release(&self, workload_id: &str, resources: &ResourceRequirements, gpu_ids: &[u32]) {
        if let Some(node_id) = self.assignments.write().remove(workload_id) {
            if let Some(node) = self.nodes.write().get_mut(&node_id) {
                node.release(resources, gpu_ids);
            }
        }
    }

    /// Get queue length
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Register priority class
    pub fn register_priority_class(&self, class: PriorityClass) {
        self.priority_classes.write().insert(class.name.clone(), class);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_resources() {
        let mut node = NodeResources::new(NodeId::new(), 4000, 8192);
        
        let req = ResourceRequirements::new().cpu(1000).memory(2048);
        assert!(node.can_fit(&req));
        
        assert!(node.allocate(&req));
        assert_eq!(node.cpu_available(), 3000);
        assert_eq!(node.memory_available(), 6144);
    }

    #[test]
    fn test_scheduler_basic() {
        let scheduler = Scheduler::new();
        
        let node = NodeResources::new(NodeId::new(), 4000, 8192);
        scheduler.register_node(node);
        
        let workload = Workload::new("w1", "test")
            .with_resources(ResourceRequirements::new().cpu(1000).memory(1024));
        
        let decision = scheduler.schedule(&workload);
        assert!(decision.node_id.is_some());
    }

    #[test]
    fn test_scheduler_no_capacity() {
        let scheduler = Scheduler::new();
        
        let node = NodeResources::new(NodeId::new(), 1000, 1024);
        scheduler.register_node(node);
        
        let workload = Workload::new("w1", "test")
            .with_resources(ResourceRequirements::new().cpu(2000).memory(2048));
        
        let decision = scheduler.schedule(&workload);
        assert!(decision.node_id.is_none());
    }
}
