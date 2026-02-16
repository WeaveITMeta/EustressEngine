//! # Eustress Scenarios — Monte Carlo Simulation Engine
//!
//! Table of Contents:
//! 1. SimulationConfig — Configuration for a simulation run
//! 2. SimulationResult — Output of a simulation run
//! 3. BranchSample — Single Monte Carlo sample result
//! 4. run_simulation — Main entry point (rayon parallel)
//! 5. bayesian_batch_update — Apply all evidence to all branches
//! 6. sample_branch_tree — Single Monte Carlo walk through the tree

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{BranchNode, BranchStatus, Evidence, Scenario};

// ─────────────────────────────────────────────
// 1. SimulationConfig
// ─────────────────────────────────────────────

/// Configuration for a Monte Carlo simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    /// Number of Monte Carlo samples to run
    pub num_samples: u64,
    /// Random seed (None = random)
    pub seed: Option<u64>,
    /// Whether to apply Bayesian updates from evidence before sampling
    pub apply_bayesian: bool,
    /// Whether to normalize sibling posteriors after updates
    pub normalize: bool,
    /// Whether to apply soft-pruning after simulation
    pub soft_prune: bool,
    /// Maximum tree depth to explore (None = unlimited)
    pub max_depth: Option<u32>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            num_samples: 10_000,
            seed: None,
            apply_bayesian: true,
            normalize: true,
            soft_prune: true,
            max_depth: None,
        }
    }
}

// ─────────────────────────────────────────────
// 2. SimulationResult
// ─────────────────────────────────────────────

/// Output of a Monte Carlo simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Configuration used for this run
    pub config: SimulationConfig,
    /// Total samples executed
    pub total_samples: u64,
    /// Per-branch hit counts from Monte Carlo sampling
    pub branch_hits: HashMap<Uuid, u64>,
    /// Per-branch posterior probabilities (post-simulation)
    pub posteriors: HashMap<Uuid, f64>,
    /// Leaf branch probability distribution (terminal outcomes)
    pub leaf_distribution: HashMap<Uuid, f64>,
    /// Wall-clock duration of the simulation in milliseconds
    pub duration_ms: u64,
    /// When the simulation completed
    pub completed_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────
// 3. BranchSample — Single MC sample path
// ─────────────────────────────────────────────

/// A single Monte Carlo sample: the path taken through the tree.
#[derive(Debug, Clone)]
struct BranchSample {
    /// Ordered list of branch IDs visited from root to leaf
    path: Vec<Uuid>,
    /// The terminal branch ID
    leaf_id: Uuid,
}

// ─────────────────────────────────────────────
// 4. run_simulation — Main entry point
// ─────────────────────────────────────────────

/// Run a Monte Carlo simulation on a scenario.
///
/// This is the main entry point for the simulation engine.
/// It uses rayon for parallel sampling across CPU cores.
///
/// Steps:
/// 1. (Optional) Apply Bayesian updates from all evidence
/// 2. (Optional) Normalize sibling posteriors
/// 3. Run N Monte Carlo samples in parallel (rayon)
/// 4. Aggregate hit counts per branch
/// 5. Update branch posteriors from MC results
/// 6. (Optional) Apply soft-pruning
pub fn run_simulation(scenario: &mut Scenario, config: &SimulationConfig) -> SimulationResult {
    let start = std::time::Instant::now();

    // Step 1: Bayesian updates
    if config.apply_bayesian {
        bayesian_batch_update(scenario);
    }

    // Step 2: Normalize
    if config.normalize {
        scenario.normalize_siblings();
    }

    // Step 3: Monte Carlo sampling (parallel via rayon)
    let root_id = match scenario.root_branch_id {
        Some(id) => id,
        None => {
            return SimulationResult {
                config: config.clone(),
                total_samples: 0,
                branch_hits: HashMap::new(),
                posteriors: HashMap::new(),
                leaf_distribution: HashMap::new(),
                duration_ms: start.elapsed().as_millis() as u64,
                completed_at: Utc::now(),
            };
        }
    };

    // Build a read-only snapshot of the branch tree for parallel access
    let tree_snapshot = BranchTreeSnapshot::from_scenario(scenario);

    // Atomic counters for thread-safe hit accumulation
    let hit_counters: HashMap<Uuid, Arc<AtomicU64>> = scenario
        .branches
        .keys()
        .map(|&id| (id, Arc::new(AtomicU64::new(0))))
        .collect();

    let seed_base = config.seed.unwrap_or_else(|| rand::thread_rng().next_u64());

    // Parallel sampling
    (0..config.num_samples).into_par_iter().for_each(|i| {
        let mut rng = StdRng::seed_from_u64(seed_base.wrapping_add(i));
        let sample = sample_branch_tree(&tree_snapshot, root_id, &mut rng, config.max_depth);

        // Increment hit counters for every branch in the path
        for &branch_id in &sample.path {
            if let Some(counter) = hit_counters.get(&branch_id) {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Step 4: Aggregate results
    let branch_hits: HashMap<Uuid, u64> = hit_counters
        .iter()
        .map(|(&id, counter)| (id, counter.load(Ordering::Relaxed)))
        .collect();

    // Step 5: Update branch posteriors from MC results
    let total = config.num_samples;
    for (&branch_id, &hits) in &branch_hits {
        if let Some(branch) = scenario.branches.get_mut(&branch_id) {
            branch.mc_hits = hits;
            branch.mc_total = total;
            // Blend MC estimate with Bayesian posterior (weighted average)
            if total > 0 {
                let mc_prob = hits as f64 / total as f64;
                // Weight MC more as sample count increases
                let mc_weight = (total as f64 / (total as f64 + 1000.0)).min(0.8);
                branch.posterior = mc_weight * mc_prob + (1.0 - mc_weight) * branch.posterior;
            }
        }
    }

    // Re-normalize after MC update
    if config.normalize {
        scenario.normalize_siblings();
    }

    // Compute posteriors snapshot and leaf distribution
    let posteriors: HashMap<Uuid, f64> = scenario
        .branches
        .iter()
        .map(|(&id, b)| (id, b.posterior))
        .collect();

    let leaf_distribution: HashMap<Uuid, f64> = scenario
        .branches
        .iter()
        .filter(|(_, b)| b.is_leaf() && b.status == BranchStatus::Active)
        .map(|(&id, b)| (id, b.posterior))
        .collect();

    // Step 6: Soft-prune
    if config.soft_prune {
        scenario.apply_soft_pruning();
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    SimulationResult {
        config: config.clone(),
        total_samples: total,
        branch_hits,
        posteriors,
        leaf_distribution,
        duration_ms,
        completed_at: Utc::now(),
    }
}

// ─────────────────────────────────────────────
// 5. bayesian_batch_update
// ─────────────────────────────────────────────

/// Apply all evidence to all branches via Bayesian updates.
/// For each piece of evidence linked to a branch, updates that
/// branch's posterior using the evidence's effective likelihood ratio.
pub fn bayesian_batch_update(scenario: &mut Scenario) {
    // Collect (branch_id, evidence_id) pairs to avoid borrow conflicts
    let updates: Vec<(Uuid, Uuid)> = scenario
        .evidence
        .iter()
        .flat_map(|e| {
            e.links
                .iter()
                .map(move |link| (link.branch_id, e.id))
        })
        .collect();

    for (branch_id, evidence_id) in updates {
        scenario.bayesian_update(branch_id, evidence_id);
    }
}

// ─────────────────────────────────────────────
// 6. Branch tree snapshot for parallel reads
// ─────────────────────────────────────────────

/// Read-only snapshot of the branch tree for parallel MC sampling.
/// Avoids holding a mutable borrow on Scenario during rayon parallel iteration.
#[derive(Debug, Clone)]
struct BranchTreeSnapshot {
    /// Branch ID → (posterior, children, status)
    nodes: HashMap<Uuid, SnapshotNode>,
}

#[derive(Debug, Clone)]
struct SnapshotNode {
    posterior: f64,
    children: Vec<Uuid>,
    status: BranchStatus,
}

impl BranchTreeSnapshot {
    fn from_scenario(scenario: &Scenario) -> Self {
        let nodes = scenario
            .branches
            .iter()
            .map(|(&id, b)| {
                (
                    id,
                    SnapshotNode {
                        posterior: b.posterior,
                        children: b.children.clone(),
                        status: b.status,
                    },
                )
            })
            .collect();
        Self { nodes }
    }
}

/// Walk the branch tree from root to a leaf, choosing children
/// probabilistically based on posterior weights.
fn sample_branch_tree(
    tree: &BranchTreeSnapshot,
    root_id: Uuid,
    rng: &mut StdRng,
    max_depth: Option<u32>,
) -> BranchSample {
    let mut path = Vec::new();
    let mut current_id = root_id;
    let mut depth = 0u32;

    loop {
        path.push(current_id);

        let node = match tree.nodes.get(&current_id) {
            Some(n) => n,
            None => break,
        };

        // Check depth limit
        if let Some(max) = max_depth {
            if depth >= max {
                break;
            }
        }

        // Filter to active children only
        let active_children: Vec<(Uuid, f64)> = node
            .children
            .iter()
            .filter_map(|&child_id| {
                tree.nodes.get(&child_id).and_then(|child| {
                    if child.status == BranchStatus::Active {
                        Some((child_id, child.posterior.max(1e-10)))
                    } else {
                        None
                    }
                })
            })
            .collect();

        if active_children.is_empty() {
            // Leaf node — end of path
            break;
        }

        // Weighted random selection among children
        let weights: Vec<f64> = active_children.iter().map(|(_, w)| *w).collect();
        match WeightedIndex::new(&weights) {
            Ok(dist) => {
                let chosen = dist.sample(rng);
                current_id = active_children[chosen].0;
                depth += 1;
            }
            Err(_) => break, // All weights zero — shouldn't happen
        }
    }

    let leaf_id = *path.last().unwrap_or(&root_id);
    BranchSample { path, leaf_id }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios::types::*;

    fn make_test_scenario() -> Scenario {
        let mut s = Scenario::new("MC Test", ScenarioScale::Micro);
        let root = s.set_root_branch("Root", 1.0);
        s.add_branch(root, "High Prob", 0.7);
        s.add_branch(root, "Low Prob", 0.3);
        s
    }

    #[test]
    fn test_simulation_runs() {
        let mut scenario = make_test_scenario();
        let config = SimulationConfig {
            num_samples: 5_000,
            seed: Some(42),
            apply_bayesian: false,
            normalize: true,
            soft_prune: false,
            max_depth: None,
        };

        let result = run_simulation(&mut scenario, &config);
        assert_eq!(result.total_samples, 5_000);
        assert!(!result.branch_hits.is_empty());
        assert!(!result.posteriors.is_empty());
    }

    #[test]
    fn test_high_prob_branch_gets_more_hits() {
        let mut scenario = make_test_scenario();
        let config = SimulationConfig {
            num_samples: 50_000,
            seed: Some(123),
            apply_bayesian: false,
            normalize: true,
            soft_prune: false,
            max_depth: None,
        };

        let result = run_simulation(&mut scenario, &config);

        // Find the two leaf branches
        let leaves: Vec<(&Uuid, &u64)> = result
            .branch_hits
            .iter()
            .filter(|(id, _)| scenario.branch(**id).map_or(false, |b| b.is_leaf()))
            .collect();

        assert_eq!(leaves.len(), 2);

        // The branch with prior 0.7 should have more hits than 0.3
        let root_id = scenario.root_branch_id.unwrap();
        let children: Vec<Uuid> = scenario.branch(root_id).unwrap().children.clone();
        let high_id = children.iter().find(|&&id| {
            scenario.branch(id).unwrap().label == "High Prob"
        }).unwrap();
        let low_id = children.iter().find(|&&id| {
            scenario.branch(id).unwrap().label == "Low Prob"
        }).unwrap();

        let high_hits = result.branch_hits[high_id];
        let low_hits = result.branch_hits[low_id];
        assert!(
            high_hits > low_hits,
            "High ({high_hits}) should beat Low ({low_hits})"
        );
    }

    #[test]
    fn test_empty_scenario_simulation() {
        let mut scenario = Scenario::new("Empty", ScenarioScale::Micro);
        let config = SimulationConfig::default();
        let result = run_simulation(&mut scenario, &config);
        assert_eq!(result.total_samples, 0);
    }

    #[test]
    fn test_deterministic_with_seed() {
        let config = SimulationConfig {
            num_samples: 10_000,
            seed: Some(42),
            apply_bayesian: false,
            normalize: true,
            soft_prune: false,
            max_depth: None,
        };

        let mut s1 = make_test_scenario();
        let r1 = run_simulation(&mut s1, &config);

        let mut s2 = make_test_scenario();
        let r2 = run_simulation(&mut s2, &config);

        // Same seed should produce same hit counts
        assert_eq!(r1.branch_hits, r2.branch_hits);
    }
}
