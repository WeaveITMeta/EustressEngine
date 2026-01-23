//! Preemption and priority-based scheduling
//!
//! Implements workload preemption for high-priority workloads.

use serde::{Deserialize, Serialize};

/// Preemption policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreemptionPolicy {
    /// Never preempt other workloads
    Never,
    /// Preempt lower priority workloads
    PreemptLowerPriority,
}

impl Default for PreemptionPolicy {
    fn default() -> Self {
        Self::PreemptLowerPriority
    }
}

/// Priority class definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityClass {
    /// Class name
    pub name: String,
    /// Priority value (higher = more important)
    pub value: i32,
    /// Whether this class can preempt others
    pub preemption_policy: PreemptionPolicy,
    /// Description
    pub description: Option<String>,
    /// Is this the default class
    pub global_default: bool,
}

impl PriorityClass {
    /// Create a new priority class
    pub fn new(name: impl Into<String>, value: i32) -> Self {
        Self {
            name: name.into(),
            value,
            preemption_policy: PreemptionPolicy::PreemptLowerPriority,
            description: None,
            global_default: false,
        }
    }

    /// Set as global default
    pub fn as_default(mut self) -> Self {
        self.global_default = true;
        self
    }

    /// Set preemption policy
    pub fn with_preemption(mut self, policy: PreemptionPolicy) -> Self {
        self.preemption_policy = policy;
        self
    }

    /// Set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Standard priority classes
pub mod standard {
    use super::*;

    /// System critical priority (highest)
    pub fn system_critical() -> PriorityClass {
        PriorityClass::new("system-critical", 2000000000)
            .with_description("Critical system workloads")
    }

    /// System high priority
    pub fn system_high() -> PriorityClass {
        PriorityClass::new("system-high", 1000000000)
            .with_description("High priority system workloads")
    }

    /// Production high priority
    pub fn production_high() -> PriorityClass {
        PriorityClass::new("production-high", 100000)
            .with_description("High priority production workloads")
    }

    /// Production medium priority
    pub fn production_medium() -> PriorityClass {
        PriorityClass::new("production-medium", 50000)
            .with_description("Medium priority production workloads")
            .as_default()
    }

    /// Batch processing priority
    pub fn batch() -> PriorityClass {
        PriorityClass::new("batch", 10000)
            .with_description("Batch processing workloads")
            .with_preemption(PreemptionPolicy::Never)
    }

    /// Best effort (lowest)
    pub fn best_effort() -> PriorityClass {
        PriorityClass::new("best-effort", 0)
            .with_description("Best effort workloads, can be preempted")
            .with_preemption(PreemptionPolicy::Never)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let critical = standard::system_critical();
        let batch = standard::batch();
        let best_effort = standard::best_effort();

        assert!(critical.value > batch.value);
        assert!(batch.value > best_effort.value);
    }

    #[test]
    fn test_preemption_policy() {
        let critical = standard::system_critical();
        let batch = standard::batch();

        assert_eq!(critical.preemption_policy, PreemptionPolicy::PreemptLowerPriority);
        assert_eq!(batch.preemption_policy, PreemptionPolicy::Never);
    }
}
