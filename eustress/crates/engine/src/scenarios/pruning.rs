//! # Eustress Scenarios — Soft Pruning System
//!
//! Table of Contents:
//! 1. PruningConfig — Configurable thresholds and behavior
//! 2. PruningResult — Summary of a pruning pass
//! 3. apply_soft_pruning — Main pruning algorithm with cascade
//! 4. restore_branches — Undo collapse on branches
//! 5. PruningHistory — Audit trail of pruning decisions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{BranchStatus, Scenario};

// ─────────────────────────────────────────────
// 1. PruningConfig — Configurable thresholds
// ─────────────────────────────────────────────

/// Configuration for soft pruning behavior.
/// Soft pruning NEVER deletes branches — it only visually collapses them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningConfig {
    /// Posterior probability threshold below which branches are collapsed.
    /// Branches with posterior < threshold are soft-pruned.
    pub threshold: f64,
    /// Whether to cascade collapse to children of collapsed branches.
    /// If true, collapsing a parent also collapses all descendants.
    pub cascade: bool,
    /// Whether to auto-restore branches that rise above threshold after new evidence.
    pub auto_restore: bool,
    /// Minimum depth before pruning applies (0 = prune at any depth).
    /// Useful to keep top-level hypotheses always visible.
    pub min_depth: u32,
    /// Maximum number of branches to collapse in a single pass (0 = unlimited).
    /// Prevents mass collapse from a single simulation run.
    pub max_collapse_per_pass: usize,
    /// Whether to protect branches that have evidence attached.
    /// If true, branches with any evidence links are never collapsed.
    pub protect_evidenced: bool,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            threshold: 0.05,
            cascade: true,
            auto_restore: true,
            min_depth: 1,
            max_collapse_per_pass: 0,
            protect_evidenced: false,
        }
    }
}

impl PruningConfig {
    /// Conservative config: higher threshold, no cascade, protect evidenced.
    pub fn conservative() -> Self {
        Self {
            threshold: 0.10,
            cascade: false,
            auto_restore: true,
            min_depth: 1,
            max_collapse_per_pass: 10,
            protect_evidenced: true,
        }
    }

    /// Aggressive config: lower threshold, cascade, no protection.
    pub fn aggressive() -> Self {
        Self {
            threshold: 0.02,
            cascade: true,
            auto_restore: false,
            min_depth: 0,
            max_collapse_per_pass: 0,
            protect_evidenced: false,
        }
    }
}

// ─────────────────────────────────────────────
// 2. PruningResult — Summary of a pruning pass
// ─────────────────────────────────────────────

/// Summary of a soft pruning pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningResult {
    /// Number of branches collapsed in this pass
    pub collapsed_count: usize,
    /// Number of branches restored in this pass (if auto_restore)
    pub restored_count: usize,
    /// IDs of branches that were collapsed
    pub collapsed_ids: Vec<Uuid>,
    /// IDs of branches that were restored
    pub restored_ids: Vec<Uuid>,
    /// Total active branches after pruning
    pub active_after: usize,
    /// Total collapsed branches after pruning
    pub collapsed_after: usize,
    /// Threshold used
    pub threshold: f64,
    /// When this pass was executed
    pub executed_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────
// 3. apply_soft_pruning — Main algorithm
// ─────────────────────────────────────────────

/// Apply soft pruning to a scenario using the given config.
///
/// This is the primary entry point for the pruning system.
/// It collapses low-probability branches and optionally restores
/// branches that have risen above threshold.
///
/// **No branches are ever deleted.** Collapsed branches retain all
/// data and can be restored at any time.
pub fn apply_soft_pruning(scenario: &mut Scenario, config: &PruningConfig) -> PruningResult {
    let mut collapsed_ids = Vec::new();
    let mut restored_ids = Vec::new();

    // Phase 1: Auto-restore branches above threshold
    if config.auto_restore {
        let restore_candidates: Vec<Uuid> = scenario
            .branches
            .iter()
            .filter(|(_, b)| b.status == BranchStatus::Collapsed && b.posterior >= config.threshold)
            .map(|(&id, _)| id)
            .collect();

        for id in restore_candidates {
            if let Some(branch) = scenario.branches.get_mut(&id) {
                branch.status = BranchStatus::Active;
                restored_ids.push(id);
            }
        }
    }

    // Phase 2: Identify collapse candidates
    let mut candidates: Vec<(Uuid, f64)> = scenario
        .branches
        .iter()
        .filter(|(_, b)| {
            // Must be active
            b.status == BranchStatus::Active
            // Must be below threshold
            && b.posterior < config.threshold
            // Must meet minimum depth
            && b.depth >= config.min_depth
        })
        .filter(|(_, b)| {
            // Protect evidenced branches if configured
            if config.protect_evidenced {
                b.evidence_ids.is_empty()
            } else {
                true
            }
        })
        .map(|(&id, b)| (id, b.posterior))
        .collect();

    // Sort by posterior ascending (collapse lowest first)
    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Apply max_collapse_per_pass limit
    if config.max_collapse_per_pass > 0 {
        candidates.truncate(config.max_collapse_per_pass);
    }

    // Phase 3: Collapse candidates
    for (id, _) in &candidates {
        if let Some(branch) = scenario.branches.get_mut(id) {
            branch.status = BranchStatus::Collapsed;
            collapsed_ids.push(*id);
        }
    }

    // Phase 4: Cascade collapse to descendants
    if config.cascade && !collapsed_ids.is_empty() {
        let cascade_ids = collect_descendants(scenario, &collapsed_ids);
        for id in cascade_ids {
            if let Some(branch) = scenario.branches.get_mut(&id) {
                if branch.status == BranchStatus::Active {
                    branch.status = BranchStatus::Collapsed;
                    collapsed_ids.push(id);
                }
            }
        }
    }

    // Compute final counts
    let active_after = scenario
        .branches
        .values()
        .filter(|b| b.status == BranchStatus::Active)
        .count();
    let collapsed_after = scenario
        .branches
        .values()
        .filter(|b| b.status == BranchStatus::Collapsed)
        .count();

    if !collapsed_ids.is_empty() || !restored_ids.is_empty() {
        scenario.updated_at = Utc::now();
    }

    PruningResult {
        collapsed_count: collapsed_ids.len(),
        restored_count: restored_ids.len(),
        collapsed_ids,
        restored_ids,
        active_after,
        collapsed_after,
        threshold: config.threshold,
        executed_at: Utc::now(),
    }
}

/// Collect all descendant branch IDs from a set of root IDs.
fn collect_descendants(scenario: &Scenario, root_ids: &[Uuid]) -> Vec<Uuid> {
    let mut descendants = Vec::new();
    let mut stack: Vec<Uuid> = root_ids.to_vec();

    while let Some(id) = stack.pop() {
        if let Some(branch) = scenario.branches.get(&id) {
            for &child_id in &branch.children {
                if !root_ids.contains(&child_id) && !descendants.contains(&child_id) {
                    descendants.push(child_id);
                    stack.push(child_id);
                }
            }
        }
    }

    descendants
}

// ─────────────────────────────────────────────
// 4. restore_branches — Undo collapse
// ─────────────────────────────────────────────

/// Restore specific collapsed branches back to Active status.
/// Returns the number of branches restored.
pub fn restore_branches(scenario: &mut Scenario, branch_ids: &[Uuid]) -> usize {
    let mut count = 0;
    for &id in branch_ids {
        if let Some(branch) = scenario.branches.get_mut(&id) {
            if branch.status == BranchStatus::Collapsed {
                branch.status = BranchStatus::Active;
                count += 1;
            }
        }
    }

    if count > 0 {
        scenario.updated_at = Utc::now();
    }
    count
}

/// Restore all collapsed branches in the scenario.
/// Returns the number of branches restored.
pub fn restore_all(scenario: &mut Scenario) -> usize {
    let mut count = 0;
    for branch in scenario.branches.values_mut() {
        if branch.status == BranchStatus::Collapsed {
            branch.status = BranchStatus::Active;
            count += 1;
        }
    }

    if count > 0 {
        scenario.updated_at = Utc::now();
    }
    count
}

/// Restore a collapsed branch and all its descendants.
/// Returns the number of branches restored.
pub fn restore_subtree(scenario: &mut Scenario, root_id: Uuid) -> usize {
    let descendants = collect_descendants(scenario, &[root_id]);
    let mut all_ids = vec![root_id];
    all_ids.extend(descendants);
    restore_branches(scenario, &all_ids)
}

// ─────────────────────────────────────────────
// 5. PruningHistory — Audit trail
// ─────────────────────────────────────────────

/// Audit trail of pruning decisions for a scenario.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PruningHistory {
    /// All pruning passes executed on this scenario
    pub passes: Vec<PruningResult>,
}

impl PruningHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a pruning pass.
    pub fn record(&mut self, result: PruningResult) {
        self.passes.push(result);
    }

    /// Total branches collapsed across all passes.
    pub fn total_collapsed(&self) -> usize {
        self.passes.iter().map(|p| p.collapsed_count).sum()
    }

    /// Total branches restored across all passes.
    pub fn total_restored(&self) -> usize {
        self.passes.iter().map(|p| p.restored_count).sum()
    }

    /// Get the most recent pruning pass.
    pub fn last_pass(&self) -> Option<&PruningResult> {
        self.passes.last()
    }
}

// ─────────────────────────────────────────────
// Utility: Pruning statistics
// ─────────────────────────────────────────────

/// Compute pruning statistics for a scenario without modifying it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningStats {
    /// Number of active branches
    pub active: usize,
    /// Number of collapsed branches
    pub collapsed: usize,
    /// Number of resolved branches
    pub resolved: usize,
    /// Number of branches that would be collapsed at the current threshold
    pub would_collapse: usize,
    /// Number of branches that would be restored at the current threshold
    pub would_restore: usize,
    /// Lowest posterior among active branches
    pub min_active_posterior: f64,
    /// Highest posterior among collapsed branches
    pub max_collapsed_posterior: f64,
}

/// Compute pruning statistics without modifying the scenario.
pub fn pruning_stats(scenario: &Scenario, config: &PruningConfig) -> PruningStats {
    let mut active = 0;
    let mut collapsed = 0;
    let mut resolved = 0;
    let mut would_collapse = 0;
    let mut would_restore = 0;
    let mut min_active_posterior = f64::INFINITY;
    let mut max_collapsed_posterior = f64::NEG_INFINITY;

    for branch in scenario.branches.values() {
        match branch.status {
            BranchStatus::Active => {
                active += 1;
                if branch.posterior < min_active_posterior {
                    min_active_posterior = branch.posterior;
                }
                if branch.posterior < config.threshold && branch.depth >= config.min_depth {
                    if !config.protect_evidenced || branch.evidence_ids.is_empty() {
                        would_collapse += 1;
                    }
                }
            }
            BranchStatus::Collapsed => {
                collapsed += 1;
                if branch.posterior > max_collapsed_posterior {
                    max_collapsed_posterior = branch.posterior;
                }
                if config.auto_restore && branch.posterior >= config.threshold {
                    would_restore += 1;
                }
            }
            BranchStatus::Resolved | BranchStatus::Frozen => {
                resolved += 1;
            }
        }
    }

    if min_active_posterior == f64::INFINITY {
        min_active_posterior = 0.0;
    }
    if max_collapsed_posterior == f64::NEG_INFINITY {
        max_collapsed_posterior = 0.0;
    }

    PruningStats {
        active,
        collapsed,
        resolved,
        would_collapse,
        would_restore,
        min_active_posterior,
        max_collapsed_posterior,
    }
}

// ─────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::ScenarioScale;

    fn make_deep_scenario() -> Scenario {
        let mut s = Scenario::new("Pruning Test", ScenarioScale::Micro);
        let root = s.set_root_branch("Root", 1.0);
        let high = s.add_branch(root, "High Prob", 0.8).unwrap();
        let low = s.add_branch(root, "Low Prob", 0.03).unwrap();
        // Add children under low
        s.add_branch(low, "Low Child A", 0.01).unwrap();
        s.add_branch(low, "Low Child B", 0.02).unwrap();
        // Add child under high
        s.add_branch(high, "High Child", 0.7).unwrap();
        s
    }

    #[test]
    fn test_basic_pruning() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig::default(); // threshold 0.05

        let result = apply_soft_pruning(&mut scenario, &config);

        // Low Prob (0.03) and its children should be collapsed
        assert!(result.collapsed_count >= 1);
        assert!(result.active_after > 0);

        // High Prob should still be active
        let high = scenario.branches.values().find(|b| b.label == "High Prob").unwrap();
        assert_eq!(high.status, BranchStatus::Active);
    }

    #[test]
    fn test_cascade_collapse() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig {
            threshold: 0.05,
            cascade: true,
            ..Default::default()
        };

        let result = apply_soft_pruning(&mut scenario, &config);

        // Low Prob's children should also be collapsed via cascade
        let low_child_a = scenario.branches.values().find(|b| b.label == "Low Child A").unwrap();
        let low_child_b = scenario.branches.values().find(|b| b.label == "Low Child B").unwrap();
        assert_eq!(low_child_a.status, BranchStatus::Collapsed);
        assert_eq!(low_child_b.status, BranchStatus::Collapsed);
    }

    #[test]
    fn test_no_cascade() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig {
            threshold: 0.05,
            cascade: false,
            min_depth: 1,
            ..Default::default()
        };

        apply_soft_pruning(&mut scenario, &config);

        // Low Prob collapsed, but children may not be (they're below threshold too,
        // so they get collapsed on their own merit, not via cascade)
        let low = scenario.branches.values().find(|b| b.label == "Low Prob").unwrap();
        assert_eq!(low.status, BranchStatus::Collapsed);
    }

    #[test]
    fn test_auto_restore() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig::default();

        // First pass: collapse low branches
        apply_soft_pruning(&mut scenario, &config);

        // Manually boost a collapsed branch above threshold
        let low_id = scenario.branches.values()
            .find(|b| b.label == "Low Prob")
            .unwrap().id;
        scenario.branches.get_mut(&low_id).unwrap().posterior = 0.15;

        // Second pass: should auto-restore
        let result = apply_soft_pruning(&mut scenario, &config);
        assert!(result.restored_count >= 1);
        assert!(result.restored_ids.contains(&low_id));

        let low = scenario.branches.get(&low_id).unwrap();
        assert_eq!(low.status, BranchStatus::Active);
    }

    #[test]
    fn test_restore_subtree() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig {
            threshold: 0.05,
            cascade: true,
            auto_restore: false,
            ..Default::default()
        };

        apply_soft_pruning(&mut scenario, &config);

        let low_id = scenario.branches.values()
            .find(|b| b.label == "Low Prob")
            .unwrap().id;

        // Restore the entire subtree
        let restored = restore_subtree(&mut scenario, low_id);
        assert!(restored >= 1);

        let low = scenario.branches.get(&low_id).unwrap();
        assert_eq!(low.status, BranchStatus::Active);
    }

    #[test]
    fn test_restore_all() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig {
            threshold: 0.05,
            cascade: true,
            auto_restore: false,
            ..Default::default()
        };

        apply_soft_pruning(&mut scenario, &config);
        let collapsed_before = scenario.branches.values()
            .filter(|b| b.status == BranchStatus::Collapsed)
            .count();
        assert!(collapsed_before > 0);

        let restored = restore_all(&mut scenario);
        assert_eq!(restored, collapsed_before);

        let collapsed_after = scenario.branches.values()
            .filter(|b| b.status == BranchStatus::Collapsed)
            .count();
        assert_eq!(collapsed_after, 0);
    }

    #[test]
    fn test_pruning_stats() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig::default();

        let stats = pruning_stats(&scenario, &config);
        assert!(stats.would_collapse > 0);
        assert_eq!(stats.collapsed, 0);

        apply_soft_pruning(&mut scenario, &config);

        let stats = pruning_stats(&scenario, &config);
        assert!(stats.collapsed > 0);
        assert_eq!(stats.would_collapse, 0); // All eligible already collapsed
    }

    #[test]
    fn test_pruning_history() {
        let mut scenario = make_deep_scenario();
        let config = PruningConfig::default();
        let mut history = PruningHistory::new();

        let result = apply_soft_pruning(&mut scenario, &config);
        history.record(result);

        assert_eq!(history.passes.len(), 1);
        assert!(history.total_collapsed() > 0);
    }
}
