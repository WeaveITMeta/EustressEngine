//! Scheduling algorithms for workload placement
//!
//! Implements multiple scheduling strategies including:
//! - Bin-packing (maximize utilization)
//! - Spread (maximize availability)
//! - GPU locality (minimize data movement)
//! - Learned routing (ML-based adaptive scheduling)

use super::{NodeResources, Workload};

/// Trait for scheduling algorithms
pub trait SchedulingAlgorithm: Send + Sync {
    /// Score a node for a workload (higher = better)
    fn score(&self, workload: &Workload, node: &NodeResources) -> f64;
    
    /// Algorithm name
    fn name(&self) -> &str;
}

/// Bin-packing scheduler - maximizes node utilization
#[derive(Debug, Clone)]
pub struct BinPackScheduler {
    /// Weight for CPU utilization
    cpu_weight: f64,
    /// Weight for memory utilization
    memory_weight: f64,
    /// Weight for GPU utilization
    gpu_weight: f64,
}

impl BinPackScheduler {
    /// Create new bin-pack scheduler
    pub fn new() -> Self {
        Self {
            cpu_weight: 1.0,
            memory_weight: 1.0,
            gpu_weight: 2.0, // GPUs are expensive, pack them tighter
        }
    }

    /// Set weights
    pub fn with_weights(mut self, cpu: f64, memory: f64, gpu: f64) -> Self {
        self.cpu_weight = cpu;
        self.memory_weight = memory;
        self.gpu_weight = gpu;
        self
    }
}

impl Default for BinPackScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulingAlgorithm for BinPackScheduler {
    fn score(&self, workload: &Workload, node: &NodeResources) -> f64 {
        // Prefer nodes that are already more utilized (bin-packing)
        let cpu_util = node.cpu_allocated as f64 / node.cpu_capacity as f64;
        let mem_util = node.memory_allocated as f64 / node.memory_capacity as f64;
        
        let gpu_util = if !node.gpus.is_empty() {
            node.gpus_allocated.len() as f64 / node.gpus.len() as f64
        } else {
            0.0
        };

        // Score based on current utilization (higher util = higher score for bin-packing)
        let base_score = (cpu_util * self.cpu_weight + mem_util * self.memory_weight + gpu_util * self.gpu_weight)
            / (self.cpu_weight + self.memory_weight + self.gpu_weight);

        // Penalize if workload barely fits
        let cpu_headroom = (node.cpu_available() as f64 - workload.resources.cpu_millis as f64) 
            / node.cpu_capacity as f64;
        let mem_headroom = (node.memory_available() as f64 - workload.resources.memory_mb as f64)
            / node.memory_capacity as f64;

        let headroom_penalty = if cpu_headroom < 0.05 || mem_headroom < 0.05 {
            0.1
        } else {
            0.0
        };

        (base_score - headroom_penalty).max(0.0)
    }

    fn name(&self) -> &str {
        "bin-pack"
    }
}

/// Spread scheduler - maximizes availability by spreading workloads
#[derive(Debug, Clone)]
pub struct SpreadScheduler {
    /// Topology key for spreading (e.g., "zone", "rack")
    topology_key: Option<String>,
}

impl SpreadScheduler {
    /// Create new spread scheduler
    pub fn new() -> Self {
        Self { topology_key: None }
    }

    /// Set topology key for spreading
    pub fn with_topology(mut self, key: impl Into<String>) -> Self {
        self.topology_key = Some(key.into());
        self
    }
}

impl Default for SpreadScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulingAlgorithm for SpreadScheduler {
    fn score(&self, _workload: &Workload, node: &NodeResources) -> f64 {
        // Prefer nodes with lower utilization (spreading)
        let cpu_util = node.cpu_allocated as f64 / node.cpu_capacity as f64;
        let mem_util = node.memory_allocated as f64 / node.memory_capacity as f64;

        // Invert utilization for spread scoring
        let spread_score = 1.0 - (cpu_util + mem_util) / 2.0;

        // Bonus for matching topology key
        let topology_bonus = if let Some(key) = &self.topology_key {
            if node.labels.contains_key(key) { 0.1 } else { 0.0 }
        } else {
            0.0
        };

        spread_score + topology_bonus
    }

    fn name(&self) -> &str {
        "spread"
    }
}

/// GPU locality scheduler - optimizes for GPU workloads
#[derive(Debug, Clone)]
pub struct GpuLocalityScheduler {
    /// Prefer nodes with tensor cores
    prefer_tensor_cores: bool,
    /// Minimum compute capability
    min_compute_capability: Option<f32>,
    /// Prefer NVLink interconnect
    prefer_nvlink: bool,
}

impl GpuLocalityScheduler {
    /// Create new GPU locality scheduler
    pub fn new() -> Self {
        Self {
            prefer_tensor_cores: true,
            min_compute_capability: None,
            prefer_nvlink: true,
        }
    }

    /// Set minimum compute capability
    pub fn min_compute_capability(mut self, cc: f32) -> Self {
        self.min_compute_capability = Some(cc);
        self
    }

    /// Set tensor core preference
    pub fn prefer_tensor_cores(mut self, prefer: bool) -> Self {
        self.prefer_tensor_cores = prefer;
        self
    }
}

impl Default for GpuLocalityScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulingAlgorithm for GpuLocalityScheduler {
    fn score(&self, workload: &Workload, node: &NodeResources) -> f64 {
        if workload.resources.gpu_count == 0 {
            // Fall back to spread for non-GPU workloads
            let cpu_util = node.cpu_allocated as f64 / node.cpu_capacity as f64;
            return 1.0 - cpu_util;
        }

        let available_gpus: Vec<_> = node.gpus.iter()
            .filter(|g| !node.gpus_allocated.contains(&g.device_id))
            .collect();

        if available_gpus.is_empty() {
            return 0.0;
        }

        let mut score = 0.5; // Base score

        // Score based on GPU memory availability
        let total_available_mem: u64 = available_gpus.iter()
            .map(|g| g.available_memory_mb())
            .sum();
        let required_mem = workload.resources.gpu_memory_mb * workload.resources.gpu_count as u64;
        
        if total_available_mem >= required_mem {
            score += 0.2;
        }

        // Bonus for tensor cores
        if self.prefer_tensor_cores {
            let tensor_core_count = available_gpus.iter()
                .filter(|g| g.tensor_cores)
                .count();
            score += 0.1 * (tensor_core_count as f64 / available_gpus.len() as f64);
        }

        // Check compute capability
        if let Some(min_cc) = self.min_compute_capability {
            let meets_cc = available_gpus.iter()
                .all(|g| g.compute_capability.map(|cc| cc >= min_cc).unwrap_or(false));
            if meets_cc {
                score += 0.1;
            } else {
                score -= 0.2;
            }
        }

        // Prefer nodes where GPUs are on same NUMA node (locality)
        // This is approximated by preferring nodes with contiguous GPU IDs
        let gpu_ids: Vec<_> = available_gpus.iter().map(|g| g.device_id).collect();
        if gpu_ids.len() >= workload.resources.gpu_count as usize {
            let contiguous = gpu_ids.windows(2)
                .all(|w| w[1] == w[0] + 1);
            if contiguous {
                score += 0.1;
            }
        }

        score.min(1.0)
    }

    fn name(&self) -> &str {
        "gpu-locality"
    }
}

/// Adaptive learned scheduler using online learning
#[derive(Debug)]
pub struct LearnedScheduler {
    /// Feature weights learned from feedback
    weights: parking_lot::RwLock<Vec<f64>>,
    /// Learning rate
    learning_rate: f64,
    /// Number of features
    num_features: usize,
    /// Historical performance data
    history: parking_lot::RwLock<Vec<SchedulingFeedback>>,
}

/// Feedback for learning
#[derive(Debug, Clone)]
pub struct SchedulingFeedback {
    /// Features used for decision
    pub features: Vec<f64>,
    /// Actual performance (0.0 = bad, 1.0 = good)
    pub performance: f64,
}

impl LearnedScheduler {
    /// Create new learned scheduler
    pub fn new() -> Self {
        let num_features = 8; // CPU, mem, GPU util, headroom, etc.
        Self {
            weights: parking_lot::RwLock::new(vec![0.5; num_features]),
            learning_rate: 0.01,
            num_features,
            history: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// Extract features from workload and node
    fn extract_features(&self, workload: &Workload, node: &NodeResources) -> Vec<f64> {
        vec![
            // Utilization features
            node.cpu_allocated as f64 / node.cpu_capacity as f64,
            node.memory_allocated as f64 / node.memory_capacity as f64,
            if node.gpus.is_empty() { 0.0 } else { node.gpus_allocated.len() as f64 / node.gpus.len() as f64 },
            
            // Headroom features
            (node.cpu_available() as f64 - workload.resources.cpu_millis as f64) / node.cpu_capacity as f64,
            (node.memory_available() as f64 - workload.resources.memory_mb as f64) / node.memory_capacity as f64,
            
            // Workload features
            workload.priority as f64 / 100.0,
            if workload.resources.gpu_count > 0 { 1.0 } else { 0.0 },
            
            // Node features
            if node.schedulable { 1.0 } else { 0.0 },
        ]
    }

    /// Record feedback for learning
    pub fn record_feedback(&self, feedback: SchedulingFeedback) {
        // Online gradient descent update
        let mut weights = self.weights.write();
        
        // Compute prediction
        let prediction: f64 = feedback.features.iter()
            .zip(weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        
        // Compute error
        let error = feedback.performance - prediction;
        
        // Update weights
        for (i, feature) in feedback.features.iter().enumerate() {
            weights[i] += self.learning_rate * error * feature;
            // Clamp weights to reasonable range
            weights[i] = weights[i].clamp(-2.0, 2.0);
        }

        // Store in history for batch updates
        self.history.write().push(feedback);
    }

    /// Get current weights
    pub fn weights(&self) -> Vec<f64> {
        self.weights.read().clone()
    }
}

impl Default for LearnedScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulingAlgorithm for LearnedScheduler {
    fn score(&self, workload: &Workload, node: &NodeResources) -> f64 {
        let features = self.extract_features(workload, node);
        let weights = self.weights.read();
        
        // Linear combination of features and weights
        let score: f64 = features.iter()
            .zip(weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        
        // Sigmoid to bound output
        1.0 / (1.0 + (-score).exp())
    }

    fn name(&self) -> &str {
        "learned"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NodeId;

    fn test_node(cpu_alloc: u64, mem_alloc: u64) -> NodeResources {
        let mut node = NodeResources::new(NodeId::new(), 4000, 8192);
        node.cpu_allocated = cpu_alloc;
        node.memory_allocated = mem_alloc;
        node
    }

    #[test]
    fn test_bin_pack_prefers_utilized() {
        let scheduler = BinPackScheduler::new();
        let workload = Workload::new("w1", "test");

        let low_util = test_node(1000, 2048);
        let high_util = test_node(3000, 6144);

        let low_score = scheduler.score(&workload, &low_util);
        let high_score = scheduler.score(&workload, &high_util);

        assert!(high_score > low_score, "Bin-pack should prefer higher utilization");
    }

    #[test]
    fn test_spread_prefers_empty() {
        let scheduler = SpreadScheduler::new();
        let workload = Workload::new("w1", "test");

        let low_util = test_node(1000, 2048);
        let high_util = test_node(3000, 6144);

        let low_score = scheduler.score(&workload, &low_util);
        let high_score = scheduler.score(&workload, &high_util);

        assert!(low_score > high_score, "Spread should prefer lower utilization");
    }

    #[test]
    fn test_learned_scheduler() {
        let scheduler = LearnedScheduler::new();
        let workload = Workload::new("w1", "test");
        let node = test_node(2000, 4096);

        let score = scheduler.score(&workload, &node);
        assert!(score >= 0.0 && score <= 1.0, "Score should be bounded");

        // Test learning
        let features = vec![0.5, 0.5, 0.0, 0.25, 0.25, 0.0, 0.0, 1.0];
        scheduler.record_feedback(SchedulingFeedback {
            features,
            performance: 0.9,
        });

        // Weights should have changed
        let weights = scheduler.weights();
        assert!(weights.iter().any(|w| *w != 0.5));
    }
}
