//! # Goal System — Decomposable Goal Tree for World Model Reasoning
//!
//! ## Table of Contents
//! - GoalStatus  — terminal states for a goal node
//! - GoalNode    — one node in the decomposable goal tree
//! - GoalTree    — Bevy Resource: full inspectable goal hierarchy
//!
//! ## Design
//!
//! Goals are a tree, not a flat flag. Each node tracks sub-goal dependencies,
//! partial completion (0.0–1.0), and a salience weight that drives attention
//! in the `SalienceFilter`. `GoalTree` is updated by `BeginEpisode` /
//! `EndEpisode` agent actions and queried by the salience filter every step.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// GoalStatus
// ─────────────────────────────────────────────────────────────────────────────

/// Terminal state of a goal node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GoalStatus {
    /// Not yet started.
    #[default]
    Pending,
    /// In progress — completion is between 0.0 and 1.0.
    Active,
    /// Completed successfully.
    Achieved,
    /// Failed — will not be retried unless reset.
    Failed,
}

// ─────────────────────────────────────────────────────────────────────────────
// GoalNode
// ─────────────────────────────────────────────────────────────────────────────

/// One node in the decomposable goal hierarchy.
///
/// A goal can have sub-goals (AND semantics: all must reach Achieved before
/// the parent can complete). Partial completion is tracked as a float so
/// graded tasks like ARC scoring produce meaningful salience signals
/// throughout the episode, not just at the end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalNode {
    /// Unique identifier (UUID v4 as u128).
    pub id: u128,
    /// Human-readable description (e.g. "Place red tile at (2,3)").
    pub description: String,
    /// IDs of required sub-goals. All must be Achieved for this node to complete.
    pub subgoals: Vec<u128>,
    /// Partial completion progress [0.0–1.0].
    pub completion: f32,
    /// How much this goal drives salience scoring [0.0–1.0].
    /// Root goals have weight 1.0; sub-goals inherit a fraction.
    pub salience_weight: f32,
    /// Current status.
    pub status: GoalStatus,
    /// Final score if this node was closed (from ARC scorecard etc.).
    pub final_score: Option<f32>,
    /// ARC task identifier if this goal is episode-scoped.
    pub task_id: Option<String>,
}

impl GoalNode {
    /// Create a new pending goal with default salience weight 1.0.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().as_u128(),
            description: description.into(),
            subgoals: Vec::new(),
            completion: 0.0,
            salience_weight: 1.0,
            status: GoalStatus::Pending,
            final_score: None,
            task_id: None,
        }
    }

    /// Attach a sub-goal id (builder pattern).
    pub fn with_subgoal(mut self, subgoal_id: u128) -> Self {
        self.subgoals.push(subgoal_id);
        self
    }

    /// Set salience weight (builder pattern).
    pub fn with_salience(mut self, weight: f32) -> Self {
        self.salience_weight = weight.clamp(0.0, 1.0);
        self
    }

    /// Attach an ARC task id (builder pattern).
    pub fn for_task(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    /// Whether this node is fully complete (completion == 1.0 AND all subgoals Achieved).
    pub fn is_complete(&self, tree: &GoalTree) -> bool {
        if self.completion < 1.0 {
            return false;
        }
        self.subgoals.iter().all(|&id| {
            tree.get(id)
                .map(|g| g.status == GoalStatus::Achieved)
                .unwrap_or(false)
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GoalTree
// ─────────────────────────────────────────────────────────────────────────────

/// Decomposable, inspectable goal hierarchy — Bevy Resource.
///
/// Updated by `BeginEpisode` / `EndEpisode` agent actions.
/// Queried by `SalienceFilter` to compute `goal_relevance` for each delta.
#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
pub struct GoalTree {
    /// All goal nodes keyed by ID.
    nodes: HashMap<u128, GoalNode>,
    /// ID of the currently active primary goal (None between episodes).
    pub active_root: Option<u128>,
}

impl GoalTree {
    /// Insert or replace a goal node.
    pub fn insert(&mut self, node: GoalNode) {
        self.nodes.insert(node.id, node);
    }

    /// Get a goal node by ID.
    pub fn get(&self, id: u128) -> Option<&GoalNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable goal node by ID.
    pub fn get_mut(&mut self, id: u128) -> Option<&mut GoalNode> {
        self.nodes.get_mut(&id)
    }

    /// Begin an episode: create and activate a root goal for the given ARC task.
    /// Returns the new goal's ID.
    pub fn begin_episode(&mut self, task_id: &str, max_steps: u32) -> u128 {
        let mut root =
            GoalNode::new(format!("Solve ARC task '{task_id}' in ≤{max_steps} steps"));
        root.status = GoalStatus::Active;
        root.task_id = Some(task_id.to_string());
        let id = root.id;
        self.insert(root);
        self.active_root = Some(id);
        id
    }

    /// End an episode: mark the root goal with the final outcome.
    pub fn end_episode(&mut self, final_score: f32, goal_reached: bool) {
        if let Some(root_id) = self.active_root {
            if let Some(node) = self.nodes.get_mut(&root_id) {
                node.final_score = Some(final_score);
                node.completion = final_score;
                node.status = if goal_reached {
                    GoalStatus::Achieved
                } else {
                    GoalStatus::Failed
                };
            }
        }
        self.active_root = None;
    }

    /// Update the completion of the active root goal (call each step).
    pub fn update_completion(&mut self, completion: f32) {
        if let Some(root_id) = self.active_root {
            if let Some(node) = self.nodes.get_mut(&root_id) {
                node.completion = completion.clamp(0.0, 1.0);
            }
        }
    }

    /// Salience weight of the active root goal (0.0 between episodes).
    pub fn active_salience_weight(&self) -> f32 {
        self.active_root
            .and_then(|id| self.nodes.get(&id))
            .map(|n| n.salience_weight)
            .unwrap_or(0.0)
    }

    /// Compute goal relevance for a description string via Jaccard word overlap.
    /// Returns 0.0 when no episode is active.
    pub fn goal_relevance(&self, description: &str) -> f32 {
        let active = match self.active_root.and_then(|id| self.nodes.get(&id)) {
            Some(n) => n,
            None => return 0.0,
        };

        let goal_words: std::collections::HashSet<&str> =
            active.description.split_whitespace().collect();
        let desc_words: std::collections::HashSet<&str> =
            description.split_whitespace().collect();

        let overlap = goal_words.intersection(&desc_words).count();
        let union = goal_words.union(&desc_words).count();

        if union == 0 {
            0.0
        } else {
            overlap as f32 / union as f32
        }
    }

    /// Iterator over all currently active (non-terminal) nodes.
    pub fn active_nodes(&self) -> impl Iterator<Item = &GoalNode> {
        self.nodes.values().filter(|n| n.status == GoalStatus::Active)
    }

    /// Iterator over all nodes.
    pub fn all_nodes(&self) -> impl Iterator<Item = &GoalNode> {
        self.nodes.values()
    }

    /// Remove all terminal (Achieved / Failed) nodes that are not the current episode root.
    pub fn prune_terminal(&mut self) {
        let active = self.active_root;
        self.nodes.retain(|_, n| {
            n.status != GoalStatus::Achieved
                && n.status != GoalStatus::Failed
                || active == Some(n.id)
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_and_end_episode() {
        let mut tree = GoalTree::default();
        let id = tree.begin_episode("ls20", 50);
        assert_eq!(tree.active_root, Some(id));
        assert_eq!(tree.get(id).unwrap().status, GoalStatus::Active);

        tree.end_episode(0.95, true);
        assert_eq!(tree.active_root, None);
        assert_eq!(tree.get(id).unwrap().status, GoalStatus::Achieved);
        assert!((tree.get(id).unwrap().final_score.unwrap() - 0.95).abs() < 1e-6);
    }

    #[test]
    fn begin_and_end_failed_episode() {
        let mut tree = GoalTree::default();
        let id = tree.begin_episode("re5f", 20);
        tree.end_episode(0.1, false);
        assert_eq!(tree.get(id).unwrap().status, GoalStatus::Failed);
    }

    #[test]
    fn goal_relevance_overlap() {
        let mut tree = GoalTree::default();
        tree.begin_episode("ls20", 50);
        // "Solve" and "task" appear in the generated goal description
        let rel = tree.goal_relevance("Solve this task now");
        assert!(rel > 0.0);
        let zero = tree.goal_relevance("xyz qrs uvw");
        assert_eq!(zero, 0.0);
    }

    #[test]
    fn update_completion() {
        let mut tree = GoalTree::default();
        let id = tree.begin_episode("t1", 10);
        tree.update_completion(0.5);
        assert!((tree.get(id).unwrap().completion - 0.5).abs() < 1e-6);
    }

    #[test]
    fn prune_terminal() {
        let mut tree = GoalTree::default();
        let id = tree.begin_episode("t2", 10);
        tree.end_episode(1.0, true);
        // After end, root is terminal but still present until pruned
        assert!(tree.get(id).is_some());
        tree.prune_terminal();
        // active_root is None so the terminal node should be pruned
        assert!(tree.get(id).is_none());
    }
}
