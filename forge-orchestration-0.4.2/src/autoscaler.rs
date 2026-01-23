//! Autoscaling module for Forge
//!
//! ## Table of Contents
//! - **AutoscalerConfig**: Configuration for scaling behavior
//! - **Autoscaler**: Main autoscaling engine
//! - **ScalingDecision**: Result of scaling evaluation
//! - **ScalingPolicy**: Custom scaling policies

use crate::error::{ForgeError, Result};
use crate::job::Job;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Configuration for the autoscaler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoscalerConfig {
    /// Utilization threshold to trigger upscaling (0.0 - 1.0)
    pub upscale_threshold: f64,
    /// Utilization threshold to trigger downscaling (0.0 - 1.0)
    pub downscale_threshold: f64,
    /// Hysteresis period in seconds (cooldown between scaling actions)
    pub hysteresis_secs: u64,
    /// Evaluation interval in seconds
    pub eval_interval_secs: u64,
    /// Minimum instances (floor)
    pub min_instances: u32,
    /// Maximum instances (ceiling)
    pub max_instances: u32,
    /// Scale up increment
    pub scale_up_step: u32,
    /// Scale down increment
    pub scale_down_step: u32,
    /// Enable predictive scaling
    pub predictive_enabled: bool,
    /// Lookback window for metrics (seconds)
    pub metrics_window_secs: u64,
}

impl Default for AutoscalerConfig {
    fn default() -> Self {
        Self {
            upscale_threshold: 0.8,
            downscale_threshold: 0.3,
            hysteresis_secs: 300,
            eval_interval_secs: 30,
            min_instances: 1,
            max_instances: 100,
            scale_up_step: 1,
            scale_down_step: 1,
            predictive_enabled: false,
            metrics_window_secs: 300,
        }
    }
}

impl AutoscalerConfig {
    /// Create a new autoscaler config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set upscale threshold
    pub fn upscale_threshold(mut self, threshold: f64) -> Self {
        self.upscale_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set downscale threshold
    pub fn downscale_threshold(mut self, threshold: f64) -> Self {
        self.downscale_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Set hysteresis period
    pub fn hysteresis_secs(mut self, secs: u64) -> Self {
        self.hysteresis_secs = secs;
        self
    }

    /// Set instance bounds
    pub fn bounds(mut self, min: u32, max: u32) -> Self {
        self.min_instances = min;
        self.max_instances = max.max(min);
        self
    }

    /// Enable predictive scaling
    pub fn predictive(mut self, enabled: bool) -> Self {
        self.predictive_enabled = enabled;
        self
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.upscale_threshold <= self.downscale_threshold {
            return Err(ForgeError::config(
                "upscale_threshold must be greater than downscale_threshold",
            ));
        }
        if self.min_instances > self.max_instances {
            return Err(ForgeError::config(
                "min_instances cannot exceed max_instances",
            ));
        }
        Ok(())
    }
}

/// Scaling decision result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalingDecision {
    /// No scaling needed
    NoChange,
    /// Scale up by N instances
    ScaleUp(u32),
    /// Scale down by N instances
    ScaleDown(u32),
    /// Scale to exact count
    ScaleTo(u32),
}

impl ScalingDecision {
    /// Check if this is a scaling action
    pub fn is_scaling(&self) -> bool {
        !matches!(self, Self::NoChange)
    }

    /// Get the target delta (positive = up, negative = down)
    pub fn delta(&self) -> i32 {
        match self {
            Self::NoChange => 0,
            Self::ScaleUp(n) => *n as i32,
            Self::ScaleDown(n) => -(*n as i32),
            Self::ScaleTo(_) => 0, // Absolute, not delta
        }
    }
}

/// Metrics snapshot for scaling decisions
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    /// Average CPU utilization (0.0 - 1.0)
    pub cpu_utilization: f64,
    /// Average memory utilization (0.0 - 1.0)
    pub memory_utilization: f64,
    /// Request rate (requests per second)
    pub request_rate: f64,
    /// Average latency in milliseconds
    pub latency_ms: f64,
    /// Current instance count
    pub current_instances: u32,
    /// Timestamp
    pub timestamp: Instant,
}

impl MetricsSnapshot {
    /// Create a new metrics snapshot
    pub fn new(cpu: f64, memory: f64, instances: u32) -> Self {
        Self {
            cpu_utilization: cpu.clamp(0.0, 1.0),
            memory_utilization: memory.clamp(0.0, 1.0),
            request_rate: 0.0,
            latency_ms: 0.0,
            current_instances: instances,
            timestamp: Instant::now(),
        }
    }

    /// Get combined utilization (max of CPU and memory)
    pub fn utilization(&self) -> f64 {
        self.cpu_utilization.max(self.memory_utilization)
    }
}

/// Trait for custom scaling policies
#[async_trait]
pub trait ScalingPolicy: Send + Sync {
    /// Evaluate metrics and return a scaling decision
    async fn evaluate(
        &self,
        metrics: &MetricsSnapshot,
        config: &AutoscalerConfig,
    ) -> ScalingDecision;

    /// Policy name for logging
    fn name(&self) -> &str;
}

/// Default threshold-based scaling policy
#[derive(Debug, Clone)]
pub struct ThresholdPolicy;

#[async_trait]
impl ScalingPolicy for ThresholdPolicy {
    async fn evaluate(
        &self,
        metrics: &MetricsSnapshot,
        config: &AutoscalerConfig,
    ) -> ScalingDecision {
        let utilization = metrics.utilization();

        if utilization >= config.upscale_threshold {
            let new_count = (metrics.current_instances + config.scale_up_step)
                .min(config.max_instances);
            if new_count > metrics.current_instances {
                return ScalingDecision::ScaleUp(new_count - metrics.current_instances);
            }
        } else if utilization <= config.downscale_threshold {
            let new_count = metrics
                .current_instances
                .saturating_sub(config.scale_down_step)
                .max(config.min_instances);
            if new_count < metrics.current_instances {
                return ScalingDecision::ScaleDown(metrics.current_instances - new_count);
            }
        }

        ScalingDecision::NoChange
    }

    fn name(&self) -> &str {
        "threshold"
    }
}

/// Target utilization scaling policy
#[derive(Debug, Clone)]
pub struct TargetUtilizationPolicy {
    /// Target utilization (0.0 - 1.0)
    target: f64,
    /// Tolerance band around target
    tolerance: f64,
}

impl TargetUtilizationPolicy {
    /// Create a new target utilization policy
    pub fn new(target: f64) -> Self {
        Self {
            target: target.clamp(0.1, 0.9),
            tolerance: 0.1,
        }
    }

    /// Set tolerance band
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = tolerance.clamp(0.01, 0.5);
        self
    }
}

#[async_trait]
impl ScalingPolicy for TargetUtilizationPolicy {
    async fn evaluate(
        &self,
        metrics: &MetricsSnapshot,
        config: &AutoscalerConfig,
    ) -> ScalingDecision {
        let utilization = metrics.utilization();
        let current = metrics.current_instances as f64;

        // Calculate desired instances to hit target utilization
        let desired = (current * utilization / self.target).ceil() as u32;
        let desired = desired.clamp(config.min_instances, config.max_instances);

        let diff = (utilization - self.target).abs();
        if diff <= self.tolerance {
            return ScalingDecision::NoChange;
        }

        if desired > metrics.current_instances {
            ScalingDecision::ScaleUp(desired - metrics.current_instances)
        } else if desired < metrics.current_instances {
            ScalingDecision::ScaleDown(metrics.current_instances - desired)
        } else {
            ScalingDecision::NoChange
        }
    }

    fn name(&self) -> &str {
        "target-utilization"
    }
}

/// State for a single job's autoscaling
#[derive(Debug)]
struct JobScalingState {
    last_scale_time: Option<Instant>,
    metrics_history: Vec<MetricsSnapshot>,
}

impl JobScalingState {
    fn new() -> Self {
        Self {
            last_scale_time: None,
            metrics_history: Vec::new(),
        }
    }

    fn can_scale(&self, hysteresis: Duration) -> bool {
        match self.last_scale_time {
            Some(t) => t.elapsed() >= hysteresis,
            None => true,
        }
    }

    fn record_scale(&mut self) {
        self.last_scale_time = Some(Instant::now());
    }

    fn add_metrics(&mut self, snapshot: MetricsSnapshot, max_history: usize) {
        self.metrics_history.push(snapshot);
        if self.metrics_history.len() > max_history {
            self.metrics_history.remove(0);
        }
    }
}

/// Main autoscaler engine
pub struct Autoscaler {
    config: AutoscalerConfig,
    policy: Arc<dyn ScalingPolicy>,
    job_states: RwLock<HashMap<String, JobScalingState>>,
}

impl Autoscaler {
    /// Create a new autoscaler with default policy
    pub fn new(config: AutoscalerConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            policy: Arc::new(ThresholdPolicy),
            job_states: RwLock::new(HashMap::new()),
        })
    }

    /// Create with a custom scaling policy
    pub fn with_policy(config: AutoscalerConfig, policy: Arc<dyn ScalingPolicy>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            policy,
            job_states: RwLock::new(HashMap::new()),
        })
    }

    /// Get the current configuration
    pub fn config(&self) -> &AutoscalerConfig {
        &self.config
    }

    /// Evaluate scaling for a job
    pub async fn evaluate(&self, job_id: &str, metrics: MetricsSnapshot) -> ScalingDecision {
        let hysteresis = Duration::from_secs(self.config.hysteresis_secs);

        // Check hysteresis
        {
            let states = self.job_states.read().await;
            if let Some(state) = states.get(job_id) {
                if !state.can_scale(hysteresis) {
                    debug!(
                        job_id = %job_id,
                        "Scaling blocked by hysteresis"
                    );
                    return ScalingDecision::NoChange;
                }
            }
        }

        // Evaluate policy
        let decision = self.policy.evaluate(&metrics, &self.config).await;

        // Record metrics and scaling action
        {
            let mut states = self.job_states.write().await;
            let state = states
                .entry(job_id.to_string())
                .or_insert_with(JobScalingState::new);

            state.add_metrics(metrics, 100);

            if decision.is_scaling() {
                info!(
                    job_id = %job_id,
                    decision = ?decision,
                    policy = %self.policy.name(),
                    "Scaling decision made"
                );
                state.record_scale();
            }
        }

        decision
    }

    /// Force a scaling decision (bypasses hysteresis)
    pub async fn force_scale(&self, job_id: &str, decision: ScalingDecision) {
        let mut states = self.job_states.write().await;
        let state = states
            .entry(job_id.to_string())
            .or_insert_with(JobScalingState::new);

        warn!(
            job_id = %job_id,
            decision = ?decision,
            "Forced scaling decision"
        );
        state.record_scale();
    }

    /// Get metrics history for a job
    pub async fn get_metrics_history(&self, job_id: &str) -> Vec<MetricsSnapshot> {
        let states = self.job_states.read().await;
        states
            .get(job_id)
            .map(|s| s.metrics_history.clone())
            .unwrap_or_default()
    }

    /// Clear state for a job
    pub async fn clear_job(&self, job_id: &str) {
        let mut states = self.job_states.write().await;
        states.remove(job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_threshold_policy_scale_up() {
        let policy = ThresholdPolicy;
        let config = AutoscalerConfig::default();
        let metrics = MetricsSnapshot::new(0.85, 0.5, 5);

        let decision = policy.evaluate(&metrics, &config).await;
        assert_eq!(decision, ScalingDecision::ScaleUp(1));
    }

    #[tokio::test]
    async fn test_threshold_policy_scale_down() {
        let policy = ThresholdPolicy;
        let config = AutoscalerConfig::default();
        let metrics = MetricsSnapshot::new(0.2, 0.1, 5);

        let decision = policy.evaluate(&metrics, &config).await;
        assert_eq!(decision, ScalingDecision::ScaleDown(1));
    }

    #[tokio::test]
    async fn test_threshold_policy_no_change() {
        let policy = ThresholdPolicy;
        let config = AutoscalerConfig::default();
        let metrics = MetricsSnapshot::new(0.5, 0.5, 5);

        let decision = policy.evaluate(&metrics, &config).await;
        assert_eq!(decision, ScalingDecision::NoChange);
    }

    #[tokio::test]
    async fn test_autoscaler_hysteresis() {
        let config = AutoscalerConfig::default().hysteresis_secs(1);
        let autoscaler = Autoscaler::new(config).unwrap();

        // First evaluation should scale
        let metrics = MetricsSnapshot::new(0.9, 0.5, 5);
        let decision = autoscaler.evaluate("job-1", metrics.clone()).await;
        assert!(decision.is_scaling());

        // Immediate second evaluation should be blocked
        let decision = autoscaler.evaluate("job-1", metrics).await;
        assert_eq!(decision, ScalingDecision::NoChange);
    }

    #[tokio::test]
    async fn test_target_utilization_policy() {
        let policy = TargetUtilizationPolicy::new(0.7);
        let config = AutoscalerConfig::default();

        // High utilization -> scale up
        let metrics = MetricsSnapshot::new(0.9, 0.5, 5);
        let decision = policy.evaluate(&metrics, &config).await;
        assert!(matches!(decision, ScalingDecision::ScaleUp(_)));

        // Low utilization -> scale down
        let metrics = MetricsSnapshot::new(0.3, 0.2, 5);
        let decision = policy.evaluate(&metrics, &config).await;
        assert!(matches!(decision, ScalingDecision::ScaleDown(_)));
    }

    #[test]
    fn test_config_validation() {
        let config = AutoscalerConfig::default()
            .upscale_threshold(0.3)
            .downscale_threshold(0.8);

        assert!(config.validate().is_err());
    }
}
