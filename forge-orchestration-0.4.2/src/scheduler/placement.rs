//! Placement constraints and affinity rules
//!
//! Implements Kubernetes-style affinity/anti-affinity with extensions for:
//! - GPU topology awareness
//! - Network locality
//! - Data locality

use super::NodeResources;
use serde::{Deserialize, Serialize};

/// Placement constraint for workload scheduling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlacementConstraint {
    /// Node must have specific label
    NodeSelector { key: String, value: String },
    /// Node must be in specific zone
    Zone(String),
    /// Node must be in specific region
    Region(String),
    /// Node must have GPU
    RequiresGpu,
    /// Node must have specific GPU model
    GpuModel(String),
    /// Node must have minimum GPU memory
    MinGpuMemory(u64),
    /// Node must have tensor cores
    RequiresTensorCores,
    /// Node must have specific architecture
    Architecture(String),
    /// Custom constraint with expression
    Expression(ConstraintExpression),
}

impl PlacementConstraint {
    /// Check if node matches constraint
    pub fn matches(&self, node: &NodeResources) -> bool {
        match self {
            PlacementConstraint::NodeSelector { key, value } => {
                node.labels.get(key).map(|v| v == value).unwrap_or(false)
            }
            PlacementConstraint::Zone(zone) => {
                node.labels.get("topology.kubernetes.io/zone")
                    .or_else(|| node.labels.get("zone"))
                    .map(|z| z == zone)
                    .unwrap_or(false)
            }
            PlacementConstraint::Region(region) => {
                node.labels.get("topology.kubernetes.io/region")
                    .or_else(|| node.labels.get("region"))
                    .map(|r| r == region)
                    .unwrap_or(false)
            }
            PlacementConstraint::RequiresGpu => {
                !node.gpus.is_empty() && node.gpus_available() > 0
            }
            PlacementConstraint::GpuModel(model) => {
                node.gpus.iter().any(|g| g.model.contains(model))
            }
            PlacementConstraint::MinGpuMemory(min_mb) => {
                node.gpus.iter().any(|g| g.memory_mb >= *min_mb)
            }
            PlacementConstraint::RequiresTensorCores => {
                node.gpus.iter().any(|g| g.tensor_cores)
            }
            PlacementConstraint::Architecture(arch) => {
                node.labels.get("kubernetes.io/arch")
                    .or_else(|| node.labels.get("arch"))
                    .map(|a| a == arch)
                    .unwrap_or(false)
            }
            PlacementConstraint::Expression(expr) => {
                expr.evaluate(node)
            }
        }
    }
}

/// Constraint expression for complex matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintExpression {
    /// Key to match
    pub key: String,
    /// Operator
    pub operator: ExpressionOperator,
    /// Values to match against
    pub values: Vec<String>,
}

/// Expression operator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpressionOperator {
    /// Key's value must be in values
    In,
    /// Key's value must not be in values
    NotIn,
    /// Key must exist
    Exists,
    /// Key must not exist
    DoesNotExist,
    /// Key's value must be greater than
    Gt,
    /// Key's value must be less than
    Lt,
}

impl ConstraintExpression {
    /// Evaluate expression against node
    pub fn evaluate(&self, node: &NodeResources) -> bool {
        let value = node.labels.get(&self.key);

        match self.operator {
            ExpressionOperator::In => {
                value.map(|v| self.values.contains(v)).unwrap_or(false)
            }
            ExpressionOperator::NotIn => {
                value.map(|v| !self.values.contains(v)).unwrap_or(true)
            }
            ExpressionOperator::Exists => value.is_some(),
            ExpressionOperator::DoesNotExist => value.is_none(),
            ExpressionOperator::Gt => {
                if let (Some(v), Some(threshold)) = (value, self.values.first()) {
                    v.parse::<i64>().ok()
                        .zip(threshold.parse::<i64>().ok())
                        .map(|(v, t)| v > t)
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            ExpressionOperator::Lt => {
                if let (Some(v), Some(threshold)) = (value, self.values.first()) {
                    v.parse::<i64>().ok()
                        .zip(threshold.parse::<i64>().ok())
                        .map(|(v, t)| v < t)
                        .unwrap_or(false)
                } else {
                    false
                }
            }
        }
    }
}

/// Affinity configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affinity {
    /// Node affinity rules
    pub node_affinity: Option<NodeAffinity>,
    /// Pod/workload affinity rules
    pub workload_affinity: Option<WorkloadAffinity>,
    /// Pod/workload anti-affinity rules
    pub workload_anti_affinity: Option<WorkloadAffinity>,
}

impl Affinity {
    /// Create empty affinity
    pub fn new() -> Self {
        Self {
            node_affinity: None,
            workload_affinity: None,
            workload_anti_affinity: None,
        }
    }

    /// Set node affinity
    pub fn with_node_affinity(mut self, affinity: NodeAffinity) -> Self {
        self.node_affinity = Some(affinity);
        self
    }

    /// Set workload affinity
    pub fn with_workload_affinity(mut self, affinity: WorkloadAffinity) -> Self {
        self.workload_affinity = Some(affinity);
        self
    }

    /// Set workload anti-affinity
    pub fn with_workload_anti_affinity(mut self, affinity: WorkloadAffinity) -> Self {
        self.workload_anti_affinity = Some(affinity);
        self
    }

    /// Check if node matches affinity rules
    pub fn matches(&self, node: &NodeResources) -> bool {
        // Check node affinity
        if let Some(node_affinity) = &self.node_affinity {
            // Required rules must all match
            for rule in &node_affinity.required {
                if !rule.matches(node) {
                    return false;
                }
            }
        }

        true
    }
}

impl Default for Affinity {
    fn default() -> Self {
        Self::new()
    }
}

/// Node affinity rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeAffinity {
    /// Required rules (must match)
    pub required: Vec<AffinityRule>,
    /// Preferred rules (soft preference)
    pub preferred: Vec<WeightedAffinityRule>,
}

impl NodeAffinity {
    /// Create new node affinity
    pub fn new() -> Self {
        Self {
            required: Vec::new(),
            preferred: Vec::new(),
        }
    }

    /// Add required rule
    pub fn require(mut self, rule: AffinityRule) -> Self {
        self.required.push(rule);
        self
    }

    /// Add preferred rule
    pub fn prefer(mut self, weight: i32, rule: AffinityRule) -> Self {
        self.preferred.push(WeightedAffinityRule { weight, rule });
        self
    }
}

impl Default for NodeAffinity {
    fn default() -> Self {
        Self::new()
    }
}

/// Workload affinity/anti-affinity rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAffinity {
    /// Required rules
    pub required: Vec<WorkloadAffinityTerm>,
    /// Preferred rules
    pub preferred: Vec<WeightedWorkloadAffinityTerm>,
}

impl WorkloadAffinity {
    /// Create new workload affinity
    pub fn new() -> Self {
        Self {
            required: Vec::new(),
            preferred: Vec::new(),
        }
    }
}

impl Default for WorkloadAffinity {
    fn default() -> Self {
        Self::new()
    }
}

/// Workload affinity term
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAffinityTerm {
    /// Label selector for matching workloads
    pub label_selector: LabelSelector,
    /// Topology key for co-location
    pub topology_key: String,
    /// Namespaces to consider
    pub namespaces: Option<Vec<String>>,
}

/// Weighted workload affinity term
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedWorkloadAffinityTerm {
    /// Weight (1-100)
    pub weight: i32,
    /// Affinity term
    pub term: WorkloadAffinityTerm,
}

/// Label selector for matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSelector {
    /// Match labels exactly
    pub match_labels: std::collections::HashMap<String, String>,
    /// Match expressions
    pub match_expressions: Vec<ConstraintExpression>,
}

impl LabelSelector {
    /// Create new label selector
    pub fn new() -> Self {
        Self {
            match_labels: std::collections::HashMap::new(),
            match_expressions: Vec::new(),
        }
    }

    /// Add label match
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.match_labels.insert(key.into(), value.into());
        self
    }

    /// Add expression match
    pub fn with_expression(mut self, expr: ConstraintExpression) -> Self {
        self.match_expressions.push(expr);
        self
    }
}

impl Default for LabelSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Affinity rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffinityRule {
    /// Match expressions
    pub match_expressions: Vec<ConstraintExpression>,
}

impl AffinityRule {
    /// Create new affinity rule
    pub fn new() -> Self {
        Self {
            match_expressions: Vec::new(),
        }
    }

    /// Add expression
    pub fn with_expression(mut self, expr: ConstraintExpression) -> Self {
        self.match_expressions.push(expr);
        self
    }

    /// Check if node matches rule
    pub fn matches(&self, node: &NodeResources) -> bool {
        self.match_expressions.iter().all(|expr| expr.evaluate(node))
    }
}

impl Default for AffinityRule {
    fn default() -> Self {
        Self::new()
    }
}

/// Weighted affinity rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedAffinityRule {
    /// Weight (1-100)
    pub weight: i32,
    /// Rule
    pub rule: AffinityRule,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{NodeId, GpuResources};

    #[test]
    fn test_node_selector() {
        let mut node = NodeResources::new(NodeId::new(), 4000, 8192);
        node.labels.insert("env".to_string(), "production".to_string());

        let constraint = PlacementConstraint::NodeSelector {
            key: "env".to_string(),
            value: "production".to_string(),
        };

        assert!(constraint.matches(&node));

        let wrong_constraint = PlacementConstraint::NodeSelector {
            key: "env".to_string(),
            value: "staging".to_string(),
        };

        assert!(!wrong_constraint.matches(&node));
    }

    #[test]
    fn test_gpu_constraints() {
        let gpu = GpuResources::new(0, "NVIDIA A100", 40960)
            .with_tensor_cores(true)
            .with_compute_capability(8.0);

        let node = NodeResources::new(NodeId::new(), 4000, 8192)
            .with_gpu(gpu);

        assert!(PlacementConstraint::RequiresGpu.matches(&node));
        assert!(PlacementConstraint::RequiresTensorCores.matches(&node));
        assert!(PlacementConstraint::GpuModel("A100".to_string()).matches(&node));
        assert!(PlacementConstraint::MinGpuMemory(40000).matches(&node));
        assert!(!PlacementConstraint::MinGpuMemory(50000).matches(&node));
    }

    #[test]
    fn test_expression_operators() {
        let mut node = NodeResources::new(NodeId::new(), 4000, 8192);
        node.labels.insert("tier".to_string(), "frontend".to_string());
        node.labels.insert("priority".to_string(), "10".to_string());

        let in_expr = ConstraintExpression {
            key: "tier".to_string(),
            operator: ExpressionOperator::In,
            values: vec!["frontend".to_string(), "backend".to_string()],
        };
        assert!(in_expr.evaluate(&node));

        let gt_expr = ConstraintExpression {
            key: "priority".to_string(),
            operator: ExpressionOperator::Gt,
            values: vec!["5".to_string()],
        };
        assert!(gt_expr.evaluate(&node));

        let exists_expr = ConstraintExpression {
            key: "tier".to_string(),
            operator: ExpressionOperator::Exists,
            values: vec![],
        };
        assert!(exists_expr.evaluate(&node));
    }
}
