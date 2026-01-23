//! Mixture of Experts (MoE) routing for Forge
//!
//! ## Table of Contents
//! - **MoERouter**: Trait for implementing custom routing logic
//! - **DefaultMoERouter**: Hash-based default router
//! - **LoadAwareMoERouter**: Load-balanced routing
//! - **RouteResult**: Routing decision with metadata

use crate::types::Expert;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Result of a routing decision
#[derive(Debug, Clone)]
pub struct RouteResult {
    /// Selected expert index
    pub expert_index: usize,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Alternative experts (for fallback)
    pub alternatives: Vec<usize>,
    /// Routing metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl RouteResult {
    /// Create a new route result
    pub fn new(expert_index: usize) -> Self {
        Self {
            expert_index,
            confidence: 1.0,
            alternatives: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set confidence score
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Add alternative experts
    pub fn with_alternatives(mut self, alternatives: Vec<usize>) -> Self {
        self.alternatives = alternatives;
        self
    }
}

/// Trait for implementing MoE routing logic
///
/// Implement this trait to create custom routing strategies for
/// distributing work across expert instances.
///
/// # Example
///
/// ```rust,ignore
/// use forge_orchestration::moe::{MoERouter, RouteResult};
/// use async_trait::async_trait;
///
/// struct TypeBasedRouter;
///
/// #[async_trait]
/// impl MoERouter for TypeBasedRouter {
///     async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
///         let expert = if input.starts_with("code:") {
///             0  // Code expert
///         } else if input.starts_with("math:") {
///             1  // Math expert
///         } else {
///             2  // General expert
///         };
///         RouteResult::new(expert % num_experts)
///     }
/// }
/// ```
#[async_trait]
pub trait MoERouter: Send + Sync {
    /// Route an input to an expert
    ///
    /// # Arguments
    /// * `input` - The input string/key to route
    /// * `num_experts` - Total number of available experts
    ///
    /// # Returns
    /// A `RouteResult` containing the selected expert and metadata
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult;

    /// Route with expert health information
    ///
    /// Override this for load-aware routing
    async fn route_with_experts(&self, input: &str, experts: &[Expert]) -> RouteResult {
        let available: Vec<_> = experts.iter().filter(|e| e.available()).collect();
        if available.is_empty() {
            // Fallback to any expert if none available
            self.route(input, experts.len()).await
        } else {
            let result = self.route(input, available.len()).await;
            RouteResult::new(available[result.expert_index].index)
                .with_confidence(result.confidence)
        }
    }

    /// Get router name for metrics/logging
    fn name(&self) -> &str {
        "custom"
    }
}

/// Default hash-based MoE router
///
/// Routes inputs consistently using hash-based sharding.
/// Same input always routes to the same expert (assuming stable expert count).
#[derive(Debug, Clone, Default)]
pub struct DefaultMoERouter {
    /// Number of virtual shards per expert (for better distribution)
    virtual_shards: usize,
}

impl DefaultMoERouter {
    /// Create a new default router
    pub fn new() -> Self {
        Self { virtual_shards: 1 }
    }

    /// Create with virtual sharding for better distribution
    pub fn with_virtual_shards(mut self, shards: usize) -> Self {
        self.virtual_shards = shards.max(1);
        self
    }

    fn hash_input(&self, input: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        hasher.finish()
    }
}

#[async_trait]
impl MoERouter for DefaultMoERouter {
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
        if num_experts == 0 {
            return RouteResult::new(0);
        }

        let hash = self.hash_input(input);
        let expert_index = (hash % num_experts as u64) as usize;

        RouteResult::new(expert_index).with_confidence(1.0)
    }

    fn name(&self) -> &str {
        "default-hash"
    }
}

/// Load-aware MoE router
///
/// Routes to the least loaded available expert, with optional
/// affinity for consistent routing when loads are similar.
#[derive(Debug, Clone)]
pub struct LoadAwareMoERouter {
    /// Load difference threshold for preferring affinity
    affinity_threshold: f64,
    /// Fallback router for affinity decisions
    fallback: DefaultMoERouter,
}

impl LoadAwareMoERouter {
    /// Create a new load-aware router
    pub fn new() -> Self {
        Self {
            affinity_threshold: 0.1,
            fallback: DefaultMoERouter::new(),
        }
    }

    /// Set affinity threshold
    ///
    /// If load difference is below this threshold, prefer consistent routing
    pub fn with_affinity_threshold(mut self, threshold: f64) -> Self {
        self.affinity_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

impl Default for LoadAwareMoERouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MoERouter for LoadAwareMoERouter {
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
        // Without expert info, fall back to hash routing
        self.fallback.route(input, num_experts).await
    }

    async fn route_with_experts(&self, input: &str, experts: &[Expert]) -> RouteResult {
        let available: Vec<_> = experts.iter().filter(|e| e.available()).collect();

        if available.is_empty() {
            return RouteResult::new(0);
        }

        // Find least loaded expert
        let min_load = available
            .iter()
            .map(|e| e.load)
            .fold(f64::INFINITY, f64::min);

        // Get affinity expert from hash
        let affinity_result = self.fallback.route(input, experts.len()).await;
        let affinity_expert = experts.get(affinity_result.expert_index);

        // If affinity expert is available and load is close to minimum, use it
        if let Some(expert) = affinity_expert {
            if expert.available() && (expert.load - min_load) < self.affinity_threshold {
                return RouteResult::new(expert.index).with_confidence(0.9);
            }
        }

        // Otherwise, pick least loaded
        let selected = available
            .iter()
            .min_by(|a, b| a.load.partial_cmp(&b.load).unwrap_or(std::cmp::Ordering::Equal))
            .expect("available is non-empty, checked above");

        let alternatives: Vec<_> = available
            .iter()
            .filter(|e| e.index != selected.index)
            .take(2)
            .map(|e| e.index)
            .collect();

        RouteResult::new(selected.index)
            .with_confidence(0.8)
            .with_alternatives(alternatives)
    }

    fn name(&self) -> &str {
        "load-aware"
    }
}

/// Round-robin MoE router
///
/// Distributes requests evenly across experts in order.
#[derive(Debug)]
pub struct RoundRobinMoERouter {
    counter: std::sync::atomic::AtomicUsize,
}

impl RoundRobinMoERouter {
    /// Create a new round-robin router
    pub fn new() -> Self {
        Self {
            counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl Default for RoundRobinMoERouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MoERouter for RoundRobinMoERouter {
    async fn route(&self, _input: &str, num_experts: usize) -> RouteResult {
        if num_experts == 0 {
            return RouteResult::new(0);
        }

        let count = self
            .counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let expert_index = count % num_experts;

        RouteResult::new(expert_index)
    }

    fn name(&self) -> &str {
        "round-robin"
    }
}

/// GPU-aware MoE router for AI/ML workloads
///
/// Routes requests to experts with available GPU resources,
/// considering memory requirements and utilization.
#[derive(Debug, Clone)]
pub struct GpuAwareMoERouter {
    /// Minimum GPU memory required (MB)
    min_memory_mb: u64,
    /// Prefer tensor core capable GPUs
    prefer_tensor_cores: bool,
    /// Fallback router when no GPU experts available
    fallback: LoadAwareMoERouter,
}

impl GpuAwareMoERouter {
    /// Create a new GPU-aware router
    pub fn new() -> Self {
        Self {
            min_memory_mb: 0,
            prefer_tensor_cores: false,
            fallback: LoadAwareMoERouter::new(),
        }
    }

    /// Set minimum GPU memory requirement
    pub fn with_min_memory(mut self, memory_mb: u64) -> Self {
        self.min_memory_mb = memory_mb;
        self
    }

    /// Prefer experts with tensor core support
    pub fn prefer_tensor_cores(mut self, prefer: bool) -> Self {
        self.prefer_tensor_cores = prefer;
        self
    }
}

impl Default for GpuAwareMoERouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MoERouter for GpuAwareMoERouter {
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
        // Without expert info, fall back to load-aware routing
        self.fallback.route(input, num_experts).await
    }

    async fn route_with_experts(&self, input: &str, experts: &[Expert]) -> RouteResult {
        // Filter to GPU-capable experts with sufficient resources
        let gpu_experts: Vec<_> = experts
            .iter()
            .filter(|e| {
                e.available() && e.gpu.as_ref().map(|g| {
                    g.available() && g.available_memory_mb() >= self.min_memory_mb
                }).unwrap_or(false)
            })
            .collect();

        if gpu_experts.is_empty() {
            // Fall back to load-aware routing if no GPU experts
            return self.fallback.route_with_experts(input, experts).await;
        }

        // If preferring tensor cores, filter further
        let candidates: Vec<_> = if self.prefer_tensor_cores {
            let tensor_experts: Vec<_> = gpu_experts
                .iter()
                .filter(|e| e.gpu.as_ref().map(|g| g.tensor_cores).unwrap_or(false))
                .copied()
                .collect();
            if tensor_experts.is_empty() { gpu_experts } else { tensor_experts }
        } else {
            gpu_experts
        };

        // Select expert with most available GPU memory and lowest utilization
        let selected = candidates
            .iter()
            .min_by(|a, b| {
                let a_gpu = a.gpu.as_ref().unwrap();
                let b_gpu = b.gpu.as_ref().unwrap();
                // Primary: GPU utilization, Secondary: available memory (higher is better)
                a_gpu.utilization.partial_cmp(&b_gpu.utilization)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b_gpu.available_memory_mb().cmp(&a_gpu.available_memory_mb()))
            })
            .expect("candidates is non-empty");

        let alternatives: Vec<_> = candidates
            .iter()
            .filter(|e| e.index != selected.index)
            .take(2)
            .map(|e| e.index)
            .collect();

        RouteResult::new(selected.index)
            .with_confidence(0.9)
            .with_alternatives(alternatives)
    }

    fn name(&self) -> &str {
        "gpu-aware"
    }
}

/// Model version-aware router for A/B testing and canary deployments
#[derive(Debug, Clone)]
pub struct VersionAwareMoERouter {
    /// Target model version (if specified)
    target_version: Option<String>,
    /// Percentage of traffic to route to canary (0-100)
    canary_percent: u8,
    /// Canary version
    canary_version: Option<String>,
    /// Fallback router
    fallback: LoadAwareMoERouter,
}

impl VersionAwareMoERouter {
    /// Create a new version-aware router
    pub fn new() -> Self {
        Self {
            target_version: None,
            canary_percent: 0,
            canary_version: None,
            fallback: LoadAwareMoERouter::new(),
        }
    }

    /// Set target model version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.target_version = Some(version.into());
        self
    }

    /// Configure canary deployment
    pub fn with_canary(mut self, version: impl Into<String>, percent: u8) -> Self {
        self.canary_version = Some(version.into());
        self.canary_percent = percent.min(100);
        self
    }
}

impl Default for VersionAwareMoERouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MoERouter for VersionAwareMoERouter {
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
        self.fallback.route(input, num_experts).await
    }

    async fn route_with_experts(&self, input: &str, experts: &[Expert]) -> RouteResult {
        // Determine if this request should go to canary
        let use_canary = if self.canary_percent > 0 && self.canary_version.is_some() {
            // Simple hash-based routing for consistency
            let hash = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                input.hash(&mut hasher);
                hasher.finish()
            };
            (hash % 100) < self.canary_percent as u64
        } else {
            false
        };

        let target = if use_canary {
            self.canary_version.as_ref()
        } else {
            self.target_version.as_ref()
        };

        // Filter experts by version if specified
        let versioned_experts: Vec<_> = if let Some(version) = target {
            experts
                .iter()
                .filter(|e| e.available() && e.model_version.as_ref() == Some(version))
                .collect()
        } else {
            experts.iter().filter(|e| e.available()).collect()
        };

        if versioned_experts.is_empty() {
            return self.fallback.route_with_experts(input, experts).await;
        }

        // Route to least loaded matching expert
        let selected = versioned_experts
            .iter()
            .min_by(|a, b| a.load.partial_cmp(&b.load).unwrap_or(std::cmp::Ordering::Equal))
            .expect("versioned_experts is non-empty");

        RouteResult::new(selected.index)
            .with_confidence(if use_canary { 0.7 } else { 0.9 })
    }

    fn name(&self) -> &str {
        "version-aware"
    }
}

/// Type alias for boxed router
pub type BoxedMoERouter = Arc<dyn MoERouter>;

/// Create a boxed default router
pub fn default_router() -> BoxedMoERouter {
    Arc::new(DefaultMoERouter::new())
}

/// Create a boxed load-aware router
pub fn load_aware_router() -> BoxedMoERouter {
    Arc::new(LoadAwareMoERouter::new())
}

/// Create a boxed GPU-aware router
pub fn gpu_aware_router() -> BoxedMoERouter {
    Arc::new(GpuAwareMoERouter::new())
}

/// Create a boxed version-aware router
pub fn version_aware_router() -> BoxedMoERouter {
    Arc::new(VersionAwareMoERouter::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NodeId;

    #[tokio::test]
    async fn test_default_router_consistency() {
        let router = DefaultMoERouter::new();

        let result1 = router.route("test-input", 8).await;
        let result2 = router.route("test-input", 8).await;

        assert_eq!(result1.expert_index, result2.expert_index);
    }

    #[tokio::test]
    async fn test_default_router_distribution() {
        let router = DefaultMoERouter::new();
        let mut counts = vec![0usize; 4];

        for i in 0..1000 {
            let input = format!("input-{}", i);
            let result = router.route(&input, 4).await;
            counts[result.expert_index] += 1;
        }

        // Each expert should get roughly 25% (allow 15-35%)
        for count in counts {
            assert!(count > 150 && count < 350, "Uneven distribution: {}", count);
        }
    }

    #[tokio::test]
    async fn test_load_aware_router() {
        let router = LoadAwareMoERouter::new();

        let experts = vec![
            Expert::new(0, NodeId::new()),
            {
                let mut e = Expert::new(1, NodeId::new());
                e.update_load(0.9);
                e
            },
            Expert::new(2, NodeId::new()),
        ];

        let result = router.route_with_experts("test", &experts).await;

        // Should not pick the heavily loaded expert (index 1)
        assert_ne!(result.expert_index, 1);
    }

    #[tokio::test]
    async fn test_round_robin_router() {
        let router = RoundRobinMoERouter::new();

        let r0 = router.route("a", 3).await;
        let r1 = router.route("b", 3).await;
        let r2 = router.route("c", 3).await;
        let r3 = router.route("d", 3).await;

        assert_eq!(r0.expert_index, 0);
        assert_eq!(r1.expert_index, 1);
        assert_eq!(r2.expert_index, 2);
        assert_eq!(r3.expert_index, 0);
    }
}
